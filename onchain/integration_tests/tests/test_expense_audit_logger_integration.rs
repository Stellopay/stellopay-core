//! Cross-contract integration test verifying that expense_reimbursement
//! approval records are correctly written to the audit_logger contract.
//!
//! This test deploys ExpenseReimbursementContract and AuditLoggerContract,
//! configures the expense contract with the audit logger address, submits
//! an expense, funds it, and approves it — then verifies a corresponding
//! audit log entry exists in the logger.
//!
//! Scope: test only — no runtime logic, storage schema, or APIs are changed.
#![cfg(test)]

use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, String, Symbol,
};

use audit_logger::{AuditLogEntry, AuditLoggerContract, AuditLoggerContractClient};
use expense_reimbursement::{
    ExpenseReimbursementContract, ExpenseReimbursementContractClient, ExpenseStatus,
};

// ============================================================================
// HELPERS
// ============================================================================

fn env() -> Env {
    let e = Env::default();
    e.mock_all_auths();
    e
}

fn addr(env: &Env) -> Address {
    Address::generate(env)
}

fn token(env: &Env) -> Address {
    let admin = addr(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

fn mint(env: &Env, tok: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, tok).mint(to, &amount);
}

fn receipt_payload(env: &Env) -> String {
    String::from_str(env, "receipt://expense/001/invoice.pdf")
}

fn description(env: &Env) -> String {
    String::from_str(env, "Office supplies Q2")
}

// ============================================================================
// TESTS
// ============================================================================

/// Full happy path: submit → fund → approve → verify audit log entry.
#[test]
fn test_expense_approval_records_audit_log() {
    let env = env();

    // Deploy contracts
    let expense_id = env.register_contract(None, ExpenseReimbursementContract);
    let expense_client = ExpenseReimbursementContractClient::new(&env, &expense_id);
    let audit_id = env.register_contract(None, AuditLoggerContract);
    let audit_client = AuditLoggerContractClient::new(&env, &audit_id);

    // Setup addresses
    let expense_owner = addr(&env);
    let submitter = addr(&env);
    let approver = addr(&env);
    let payer = addr(&env);
    let tok = token(&env);

    mint(&env, &tok, &payer, 10_000);

    // Initialize contracts
    expense_client.initialize(&expense_owner);
    expense_client.add_approver(&approver);
    audit_client.initialize(&expense_owner, &100u32); // retention limit 100

    // Configure expense contract to use audit logger
    expense_client.set_audit_logger(&expense_owner, &audit_id);

    // Step 1: Submit expense
    let eid = expense_client.submit_expense(
        &submitter,
        &approver,
        &tok,
        &2_000,
        &receipt_payload(&env),
        &description(&env),
    );
    let expense = expense_client.get_expense(&eid).unwrap();
    assert_eq!(expense.status, ExpenseStatus::Pending);

    // Step 2: Fund the expense with escrow
    let fund_amount: i128 = 2_000;
    mint(&env, &tok, &payer, fund_amount);
    expense_client.fund_expense(&payer, &eid, &fund_amount);

    let expense = expense_client.get_expense(&eid).unwrap();
    assert_eq!(expense.escrow_amount, fund_amount);

    // Step 3: Approve the expense
    let approved_amount: i128 = 2_000;
    expense_client.approve_expense(&approver, &eid, &approved_amount);

    let expense = expense_client.get_expense(&eid).unwrap();
    assert_eq!(expense.status, ExpenseStatus::Approved);

    // Step 4: Verify audit log was recorded
    let audit_log_id = expense.audit_log_id.expect("audit_log_id should be Some");
    assert!(audit_log_id > 0, "audit_log_id should be positive");

    let log_entry = audit_client
        .get_log(&audit_log_id)
        .expect("Audit log entry should exist");
    assert_eq!(log_entry.actor, approver);
    assert_eq!(log_entry.action, Symbol::new(&env, "expense_approved"));
    assert_eq!(log_entry.subject, Some(submitter.clone()));
    assert_eq!(log_entry.amount, Some(approved_amount));
}

/// Verifies that rejecting an expense does NOT create an audit log entry.
#[test]
fn test_expense_rejection_does_not_create_audit_log() {
    let env = env();

    let expense_id = env.register_contract(None, ExpenseReimbursementContract);
    let expense_client = ExpenseReimbursementContractClient::new(&env, &expense_id);
    let audit_id = env.register_contract(None, AuditLoggerContract);
    let audit_client = AuditLoggerContractClient::new(&env, &audit_id);

    let expense_owner = addr(&env);
    let submitter = addr(&env);
    let approver = addr(&env);
    let tok = token(&env);

    expense_client.initialize(&expense_owner);
    expense_client.add_approver(&approver);
    audit_client.initialize(&expense_owner, &100u32);
    expense_client.set_audit_logger(&expense_owner, &audit_id);

    let eid = expense_client.submit_expense(
        &submitter,
        &approver,
        &tok,
        &1_000,
        &receipt_payload(&env),
        &description(&env),
    );

    expense_client.reject_expense(&approver, &eid);
    let expense = expense_client.get_expense(&eid).unwrap();
    assert_eq!(expense.status, ExpenseStatus::Rejected);
    assert_eq!(
        expense.audit_log_id, None,
        "Rejected expense should not have audit_log_id"
    );
}

/// Verifies that approving without audit logger configured still works (no crash).
#[test]
fn test_expense_approval_without_audit_logger() {
    let env = env();

    let expense_id = env.register_contract(None, ExpenseReimbursementContract);
    let expense_client = ExpenseReimbursementContractClient::new(&env, &expense_id);

    let expense_owner = addr(&env);
    let submitter = addr(&env);
    let approver = addr(&env);
    let tok = token(&env);

    expense_client.initialize(&expense_owner);
    expense_client.add_approver(&approver);

    let eid = expense_client.submit_expense(
        &submitter,
        &approver,
        &tok,
        &500,
        &receipt_payload(&env),
        &description(&env),
    );

    // Fund with enough for approval
    mint(&env, &tok, &submitter, 500);
    expense_client.fund_expense(&submitter, &eid, &500);

    // Approve without audit logger — should succeed with audit_log_id = None
    expense_client.approve_expense(&approver, &eid, &500);
    let expense = expense_client.get_expense(&eid).unwrap();
    assert_eq!(expense.status, ExpenseStatus::Approved);
    assert_eq!(
        expense.audit_log_id, None,
        "No audit logger means no audit_log_id"
    );
}
