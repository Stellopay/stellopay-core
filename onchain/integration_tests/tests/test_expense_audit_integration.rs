//! Cross-contract integration tests for the expense_reimbursement →
//! audit_logger approval flow (#519).
//!
//! These tests complement `test_expense_audit_logger_integration.rs` by focusing
//! on the audit-linkage *invariants* and the negative paths:
//!
//! * the `audit_log_id` stored on the approved expense resolves to a real,
//!   field-consistent entry in the audit logger (linkage is consistent);
//! * an unauthorized approval neither succeeds nor produces an audit entry;
//! * a second (double) approval cannot create a second audit entry.
//!
//! ## Cross-contract flow
//!
//! 1. `ExpenseReimbursementContract` and `AuditLoggerContract` are deployed.
//! 2. The expense contract is pointed at the audit logger via `set_audit_logger`.
//! 3. An expense is submitted, funded, then approved by its designated approver.
//! 4. On approval, the expense contract records an entry in the audit logger and
//!    stores the returned id in `Expense::audit_log_id`, giving an on-chain link
//!    between the expense and its audit trail.
//!
//! Scope: test only — no runtime logic, storage schema, or APIs are changed.
#![cfg(test)]

use soroban_sdk::{
    testutils::Address as _, token::StellarAssetClient, Address, Env, String, Symbol,
};

use audit_logger::{AuditLoggerContract, AuditLoggerContractClient};
use expense_reimbursement::{
    ExpenseReimbursementContract, ExpenseReimbursementContractClient, ExpenseStatus,
};

// ============================================================================
// HELPERS
// ============================================================================

/// Builds an `Env` with all auths mocked (the cross-contract calls in this flow
/// require multiple distinct authorizers).
fn env() -> Env {
    let e = Env::default();
    e.mock_all_auths();
    e
}

/// Generates a fresh random address.
fn addr(env: &Env) -> Address {
    Address::generate(env)
}

/// Registers a Stellar asset (token) contract and returns its address.
fn token(env: &Env) -> Address {
    let admin = addr(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

/// Mints `amount` of `tok` to `to`.
fn mint(env: &Env, tok: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, tok).mint(to, &amount);
}

/// Deploys both contracts, wires the audit logger into the expense contract,
/// and registers `approver` as an authorized approver. Returns the two clients
/// plus the audit logger contract address.
fn deploy(
    env: &Env,
    owner: &Address,
    approver: &Address,
) -> (
    ExpenseReimbursementContractClient<'static>,
    AuditLoggerContractClient<'static>,
    Address,
) {
    let expense_id = env.register_contract(None, ExpenseReimbursementContract);
    let expense_client = ExpenseReimbursementContractClient::new(env, &expense_id);
    let audit_id = env.register_contract(None, AuditLoggerContract);
    let audit_client = AuditLoggerContractClient::new(env, &audit_id);

    expense_client.initialize(owner);
    expense_client.add_approver(owner, approver);
    audit_client.initialize(owner, &100u32); // retention limit 100
    expense_client.set_audit_logger(owner, &audit_id);

    (expense_client, audit_client, audit_id)
}

/// Submits and fully funds an expense, returning its id.
fn submit_and_fund(
    env: &Env,
    expense_client: &ExpenseReimbursementContractClient,
    submitter: &Address,
    approver: &Address,
    tok: &Address,
    payer: &Address,
    amount: i128,
) -> u128 {
    let eid = expense_client.submit_expense(
        submitter,
        approver,
        tok,
        &amount,
        &String::from_str(env, "receipt://expense/abc/invoice.pdf"),
        &String::from_str(env, "Cross-contract audit linkage test"),
    );
    mint(env, tok, payer, amount);
    expense_client.fund_expense(payer, &eid, &amount);
    eid
}

// ============================================================================
// TESTS
// ============================================================================

/// Submit → fund → approve, then assert the stored `audit_log_id` resolves to a
/// real audit entry whose fields are consistent with the approval (actor,
/// action, subject, amount). This proves the linkage end-to-end.
#[test]
fn test_audit_log_id_linkage_is_consistent() {
    let env = env();
    let owner = addr(&env);
    let submitter = addr(&env);
    let approver = addr(&env);
    let payer = addr(&env);
    let tok = token(&env);

    let (expense_client, audit_client, _audit_id) = deploy(&env, &owner, &approver);

    let amount: i128 = 2_000;
    let eid = submit_and_fund(
        &env,
        &expense_client,
        &submitter,
        &approver,
        &tok,
        &payer,
        amount,
    );

    let count_before = audit_client.get_log_count();
    expense_client.approve_expense(&approver, &eid, &amount);

    // Exactly one new audit entry was produced by the approval.
    assert_eq!(audit_client.get_log_count(), count_before + 1);

    let expense = expense_client.get_expense(&eid).unwrap();
    assert_eq!(expense.status, ExpenseStatus::Approved);
    let audit_log_id = expense
        .audit_log_id
        .expect("approved expense must carry an audit_log_id");
    assert!(audit_log_id > 0, "audit_log_id must be a real positive id");

    // The id stored on the expense resolves to an entry whose fields match the
    // approval — confirming the link points at the correct record.
    let entry = audit_client
        .get_log(&audit_log_id)
        .expect("audit_log_id must resolve to an existing entry");
    assert_eq!(entry.actor, approver);
    assert_eq!(entry.action, Symbol::new(&env, "expense_approved"));
    assert_eq!(entry.subject, Some(submitter.clone()));
    assert_eq!(entry.amount, Some(amount));
}

/// An unauthorized caller (not the designated approver) must not be able to
/// approve, and the failed attempt must not write any audit entry.
#[test]
fn test_unauthorized_approval_rejected_and_no_audit_entry() {
    let env = env();
    let owner = addr(&env);
    let submitter = addr(&env);
    let approver = addr(&env);
    let attacker = addr(&env);
    let payer = addr(&env);
    let tok = token(&env);

    let (expense_client, audit_client, _audit_id) = deploy(&env, &owner, &approver);

    let amount: i128 = 1_500;
    let eid = submit_and_fund(
        &env,
        &expense_client,
        &submitter,
        &approver,
        &tok,
        &payer,
        amount,
    );

    let count_before = audit_client.get_log_count();

    // The attacker is not the expense's approver; approval must fail.
    let result = expense_client.try_approve_expense(&attacker, &eid, &amount);
    assert!(
        result.is_err(),
        "approval by a non-approver must be rejected"
    );

    // The expense is untouched and, crucially, no audit entry was created.
    let expense = expense_client.get_expense(&eid).unwrap();
    assert_eq!(expense.status, ExpenseStatus::Pending);
    assert_eq!(expense.audit_log_id, None);
    assert_eq!(
        audit_client.get_log_count(),
        count_before,
        "a rejected approval must not produce an audit entry"
    );
}

/// A second approval of an already-approved expense must fail and must not
/// create a duplicate audit entry, keeping the 1:1 approval↔audit link intact.
#[test]
fn test_double_approval_does_not_duplicate_audit_entry() {
    let env = env();
    let owner = addr(&env);
    let submitter = addr(&env);
    let approver = addr(&env);
    let payer = addr(&env);
    let tok = token(&env);

    let (expense_client, audit_client, _audit_id) = deploy(&env, &owner, &approver);

    let amount: i128 = 3_000;
    let eid = submit_and_fund(
        &env,
        &expense_client,
        &submitter,
        &approver,
        &tok,
        &payer,
        amount,
    );

    expense_client.approve_expense(&approver, &eid, &amount);
    let first = expense_client.get_expense(&eid).unwrap();
    let first_log_id = first.audit_log_id.expect("first approval records audit id");
    let count_after_first = audit_client.get_log_count();

    // Second approval attempt: the expense is no longer Pending, so it fails.
    let result = expense_client.try_approve_expense(&approver, &eid, &amount);
    assert!(
        result.is_err(),
        "re-approving an approved expense must fail"
    );

    // No new audit entry, and the stored linkage is unchanged.
    assert_eq!(
        audit_client.get_log_count(),
        count_after_first,
        "double approval must not create a second audit entry"
    );
    let after = expense_client.get_expense(&eid).unwrap();
    assert_eq!(after.audit_log_id, Some(first_log_id));
    assert_eq!(after.status, ExpenseStatus::Approved);
}
