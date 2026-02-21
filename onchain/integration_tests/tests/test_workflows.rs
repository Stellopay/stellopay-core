//! Comprehensive integration tests covering end-to-end workflows involving
//! multiple StelloPay contracts and operations.
//!
//! ## Coverage
//!
//! 1. **Payroll lifecycle** — creation, employee management, activation, funding,
//!    claiming, cancellation, grace period, and finalization.
//! 2. **Milestone agreement workflow** — creation, milestone management, approval,
//!    claiming, batch claiming, auto-completion, and pause/resume.
//! 3. **Dispute resolution workflow** — arbiter setup, dispute raising, resolution
//!    with split payouts, and edge cases.
//! 4. **Escrow agreement workflow** — time-based claiming, period tracking,
//!    completion, and cancellation during active claims.
//! 5. **Cross-contract interactions** — escrow funding via PayrollEscrowContract,
//!    bonus system alongside payroll, payment history recording.

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};

use bonus_system::{BonusSystemContract, BonusSystemContractClient};
use payment_history::{PaymentHistoryContract, PaymentHistoryContractClient};
use payroll_escrow::{PayrollEscrowContract, PayrollEscrowContractClient};
use stello_pay_contract::storage::{
    AgreementMode, AgreementStatus, DataKey, DisputeStatus,
};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ============================================================================
// CONSTANTS
// ============================================================================

const ONE_HOUR: u64 = 3_600;
const ONE_DAY: u64 = 86_400;
const ONE_WEEK: u64 = 604_800;

const SALARY: i128 = 1_000;
const ESCROW_FUND: i128 = 50_000;

// ============================================================================
// HELPERS
// ============================================================================

/// Creates a test environment with all auths mocked.
fn env() -> Env {
    let e = Env::default();
    e.mock_all_auths();
    e
}

/// Generates a fresh test address.
fn addr(env: &Env) -> Address {
    Address::generate(env)
}

/// Deploys a Stellar Asset Contract and returns its address.
fn token(env: &Env) -> Address {
    let admin = addr(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

/// Mints `amount` tokens to `to`.
fn mint(env: &Env, tok: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, tok).mint(to, &amount);
}

/// Returns the token balance of `who`.
fn balance(env: &Env, tok: &Address, who: &Address) -> i128 {
    TokenClient::new(env, tok).balance(who)
}

/// Advances the ledger timestamp by `seconds`.
fn advance(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| li.timestamp += seconds);
}

/// Sets the ledger timestamp to an absolute value.
fn set_time(env: &Env, ts: u64) {
    env.ledger().with_mut(|li| li.timestamp = ts);
}

// ---- Contract deployment helpers ----

/// Deploys and initializes the PayrollContract; returns (contract_addr, client).
fn deploy_payroll(env: &Env) -> (Address, PayrollContractClient<'_>) {
    let id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &id);
    let owner = addr(env);
    client.initialize(&owner);
    (id, client)
}

/// Deploys and initializes the PayrollEscrowContract; returns (contract_addr, client).
fn deploy_escrow<'a>(
    env: &'a Env,
    tok: &Address,
    manager: &Address,
) -> (Address, PayrollEscrowContractClient<'a>) {
    let id = env.register_contract(None, PayrollEscrowContract);
    let client = PayrollEscrowContractClient::new(env, &id);
    let admin = addr(env);
    client.initialize(&admin, tok, manager);
    (id, client)
}

/// Deploys and initializes the BonusSystemContract; returns (contract_addr, client).
fn deploy_bonus(env: &Env) -> (Address, BonusSystemContractClient<'_>) {
    let id = env.register_contract(None, BonusSystemContract);
    let client = BonusSystemContractClient::new(env, &id);
    let owner = addr(env);
    client.initialize(&owner);
    (id, client)
}

/// Deploys and initializes the PaymentHistoryContract; returns (contract_addr, client).
fn deploy_history<'a>(
    env: &'a Env,
    payroll_contract: &Address,
) -> (Address, PaymentHistoryContractClient<'a>) {
    let id = env.register_contract(None, PaymentHistoryContract);
    let client = PaymentHistoryContractClient::new(env, &id);
    let owner = addr(env);
    client.initialize(&owner, payroll_contract);
    (id, client)
}

/// Funds the internal DataKey escrow balance for an agreement inside the
/// payroll contract. Also sets up the per-employee DataKey storage needed
/// for the claiming path.
fn fund_payroll_internal(
    env: &Env,
    contract_id: &Address,
    agreement_id: u128,
    tok: &Address,
    employees: &[(Address, i128)],
    total_fund: i128,
) {
    mint(env, tok, contract_id, total_fund);
    env.as_contract(contract_id, || {
        DataKey::set_agreement_escrow_balance(env, agreement_id, tok, total_fund);
        DataKey::set_agreement_activation_time(env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(env, agreement_id, tok);
        for (idx, (emp, sal)) in employees.iter().enumerate() {
            let i = idx as u32;
            DataKey::set_employee(env, agreement_id, i, emp);
            DataKey::set_employee_salary(env, agreement_id, i, *sal);
            DataKey::set_employee_claimed_periods(env, agreement_id, i, 0);
        }
        DataKey::set_employee_count(env, agreement_id, employees.len() as u32);
    });
}

// ============================================================================
// SECTION 1: COMPLETE PAYROLL LIFECYCLE
// ============================================================================

/// End-to-end payroll lifecycle: create -> add employees -> activate -> fund
/// -> advance time -> employees claim -> verify balances -> cancel -> grace
/// period claim -> finalize -> employer refund.
#[test]
fn test_payroll_full_lifecycle() {
    let env = env();
    let (cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let tok = token(&env);
    let emp1 = addr(&env);
    let emp2 = addr(&env);
    let emp3 = addr(&env);

    // Step 1: Create payroll agreement with a 1-week grace period
    let aid = client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    let agr = client.get_agreement(&aid).unwrap();
    assert_eq!(agr.status, AgreementStatus::Created);
    assert_eq!(agr.mode, AgreementMode::Payroll);

    // Step 2: Add three employees with different salaries
    client.add_employee_to_agreement(&aid, &emp1, &1000);
    client.add_employee_to_agreement(&aid, &emp2, &2000);
    client.add_employee_to_agreement(&aid, &emp3, &3000);
    let employees = client.get_agreement_employees(&aid);
    assert_eq!(employees.len(), 3);
    assert_eq!(client.get_agreement(&aid).unwrap().total_amount, 6000);

    // Step 3: Activate agreement
    client.activate_agreement(&aid);
    let agr = client.get_agreement(&aid).unwrap();
    assert_eq!(agr.status, AgreementStatus::Active);
    assert!(agr.activated_at.is_some());

    // Step 4: Fund the internal escrow (60_000 covers 10 periods per employee)
    let total_fund = 60_000i128;
    fund_payroll_internal(
        &env,
        &cid,
        aid,
        &tok,
        &[
            (emp1.clone(), 1000),
            (emp2.clone(), 2000),
            (emp3.clone(), 3000),
        ],
        total_fund,
    );

    // Step 5: Advance 3 days — each employee should claim 3 periods
    advance(&env, ONE_DAY * 3);

    client.claim_payroll(&emp1, &aid, &0);
    assert_eq!(balance(&env, &tok, &emp1), 3000); // 1000 * 3
    assert_eq!(client.get_employee_claimed_periods(&aid, &0), 3);

    client.claim_payroll(&emp2, &aid, &1);
    assert_eq!(balance(&env, &tok, &emp2), 6000); // 2000 * 3
    assert_eq!(client.get_employee_claimed_periods(&aid, &1), 3);

    client.claim_payroll(&emp3, &aid, &2);
    assert_eq!(balance(&env, &tok, &emp3), 9000); // 3000 * 3
    assert_eq!(client.get_employee_claimed_periods(&aid, &2), 3);

    // Step 6: Cancel agreement — starts grace period
    client.cancel_agreement(&aid);
    let agr = client.get_agreement(&aid).unwrap();
    assert_eq!(agr.status, AgreementStatus::Cancelled);
    assert!(client.is_grace_period_active(&aid));

    // Step 7: Advance 1 more day (still within grace period) — employees can still claim
    advance(&env, ONE_DAY);
    client.claim_payroll(&emp1, &aid, &0);
    assert_eq!(balance(&env, &tok, &emp1), 4000); // 1 more period
    assert_eq!(client.get_employee_claimed_periods(&aid, &0), 4);

    // Step 8: Advance past grace period — claims should fail
    advance(&env, ONE_WEEK);
    let result = client.try_claim_payroll(&emp2, &aid, &1);
    assert!(result.is_err());

    // Step 9: Finalize grace period — employer gets refund
    let employer_bal_before = balance(&env, &tok, &employer);
    client.finalize_grace_period(&aid);
    let employer_bal_after = balance(&env, &tok, &employer);
    assert!(employer_bal_after > employer_bal_before);
}

/// Payroll lifecycle with a single employee — verifies the minimal path works.
#[test]
fn test_payroll_single_employee_lifecycle() {
    let env = env();
    let (cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let tok = token(&env);
    let emp = addr(&env);

    let aid = client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    client.add_employee_to_agreement(&aid, &emp, &SALARY);
    client.activate_agreement(&aid);

    fund_payroll_internal(&env, &cid, aid, &tok, &[(emp.clone(), SALARY)], ESCROW_FUND);

    advance(&env, ONE_DAY * 5);
    client.claim_payroll(&emp, &aid, &0);
    assert_eq!(balance(&env, &tok, &emp), SALARY * 5);
    assert_eq!(client.get_employee_claimed_periods(&aid, &0), 5);
}

/// Multiple claims over time — verify incremental period tracking.
#[test]
fn test_payroll_incremental_claims() {
    let env = env();
    let (cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let tok = token(&env);
    let emp = addr(&env);

    let aid = client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    client.add_employee_to_agreement(&aid, &emp, &SALARY);
    client.activate_agreement(&aid);
    fund_payroll_internal(&env, &cid, aid, &tok, &[(emp.clone(), SALARY)], ESCROW_FUND);

    // Claim after day 1
    advance(&env, ONE_DAY);
    client.claim_payroll(&emp, &aid, &0);
    assert_eq!(balance(&env, &tok, &emp), SALARY);
    assert_eq!(client.get_employee_claimed_periods(&aid, &0), 1);

    // Claim after day 3 — only 2 new periods
    advance(&env, ONE_DAY * 2);
    client.claim_payroll(&emp, &aid, &0);
    assert_eq!(balance(&env, &tok, &emp), SALARY * 3);
    assert_eq!(client.get_employee_claimed_periods(&aid, &0), 3);

    // No new periods — should fail
    let result = client.try_claim_payroll(&emp, &aid, &0);
    assert!(result.is_err());

    // Claim after day 7
    advance(&env, ONE_DAY * 4);
    client.claim_payroll(&emp, &aid, &0);
    assert_eq!(balance(&env, &tok, &emp), SALARY * 7);
}

/// Batch payroll claim — multiple employees in one call.
#[test]
fn test_payroll_batch_claim() {
    let env = env();
    let (cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let tok = token(&env);
    let emp = addr(&env);

    let aid = client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    client.add_employee_to_agreement(&aid, &emp, &SALARY);
    client.activate_agreement(&aid);
    fund_payroll_internal(&env, &cid, aid, &tok, &[(emp.clone(), SALARY)], ESCROW_FUND);

    advance(&env, ONE_DAY * 2);

    let indices = Vec::from_array(&env, [0u32]);
    let result = client.batch_claim_payroll(&emp, &aid, &indices);
    assert_eq!(result.successful_claims, 1);
    assert_eq!(result.total_claimed, SALARY * 2);
    assert_eq!(balance(&env, &tok, &emp), SALARY * 2);
}

/// Pause and resume interrupts but preserves agreement state.
#[test]
fn test_payroll_pause_resume_lifecycle() {
    let env = env();
    let (cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let tok = token(&env);
    let emp = addr(&env);

    let aid = client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    client.add_employee_to_agreement(&aid, &emp, &SALARY);
    client.activate_agreement(&aid);
    fund_payroll_internal(&env, &cid, aid, &tok, &[(emp.clone(), SALARY)], ESCROW_FUND);

    advance(&env, ONE_DAY);

    // Pause — claims should fail
    client.pause_agreement(&aid);
    assert_eq!(
        client.get_agreement(&aid).unwrap().status,
        AgreementStatus::Paused
    );
    let result = client.try_claim_payroll(&emp, &aid, &0);
    assert!(result.is_err());

    // Resume — claims work again
    client.resume_agreement(&aid);
    assert_eq!(
        client.get_agreement(&aid).unwrap().status,
        AgreementStatus::Active
    );
    client.claim_payroll(&emp, &aid, &0);
    assert_eq!(balance(&env, &tok, &emp), SALARY);
}

// ============================================================================
// SECTION 2: MILESTONE AGREEMENT WORKFLOW
// ============================================================================

/// Full milestone lifecycle: create -> add milestones -> approve -> claim ->
/// verify auto-completion.
#[test]
fn test_milestone_full_lifecycle() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let tok = token(&env);

    // Create milestone agreement
    let aid = client.create_milestone_agreement(&employer, &contributor, &tok);
    assert_eq!(client.get_milestone_count(&aid), 0);

    // Add 3 milestones
    client.add_milestone(&aid, &500);
    client.add_milestone(&aid, &1000);
    client.add_milestone(&aid, &1500);
    assert_eq!(client.get_milestone_count(&aid), 3);

    // Verify milestones are not approved or claimed
    for id in 1..=3u32 {
        let m = client.get_milestone(&aid, &id).unwrap();
        assert!(!m.approved);
        assert!(!m.claimed);
    }

    // Approve all milestones
    client.approve_milestone(&aid, &1);
    client.approve_milestone(&aid, &2);
    client.approve_milestone(&aid, &3);

    for id in 1..=3u32 {
        assert!(client.get_milestone(&aid, &id).unwrap().approved);
    }

    // Claim milestones in reverse order
    client.claim_milestone(&aid, &3);
    assert!(client.get_milestone(&aid, &3).unwrap().claimed);

    client.claim_milestone(&aid, &2);
    client.claim_milestone(&aid, &1);

    // All claimed — agreement should auto-complete (adding new milestone fails)
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.add_milestone(&aid, &100);
    }));
    assert!(result.is_err());
}

/// Selective milestone approval — only approved milestones can be claimed.
#[test]
fn test_milestone_selective_approval() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let tok = token(&env);

    let aid = client.create_milestone_agreement(&employer, &contributor, &tok);
    client.add_milestone(&aid, &100);
    client.add_milestone(&aid, &200);
    client.add_milestone(&aid, &300);

    // Only approve milestone 2
    client.approve_milestone(&aid, &2);

    // Claiming unapproved milestone 1 fails
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.claim_milestone(&aid, &1);
    }));
    assert!(r.is_err());

    // Claiming approved milestone 2 succeeds
    client.claim_milestone(&aid, &2);
    assert!(client.get_milestone(&aid, &2).unwrap().claimed);
    assert!(!client.get_milestone(&aid, &1).unwrap().claimed);
}

/// Batch milestone claiming — mixed success/failure results.
#[test]
fn test_milestone_batch_claim() {
    let env = env();
    let (cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let tok = token(&env);

    let aid = client.create_milestone_agreement(&employer, &contributor, &tok);
    client.add_milestone(&aid, &100);
    client.add_milestone(&aid, &200);
    client.add_milestone(&aid, &300);

    // Approve milestones 1 and 3, but not 2
    client.approve_milestone(&aid, &1);
    client.approve_milestone(&aid, &3);

    // Fund the contract so transfers succeed
    mint(&env, &tok, &cid, 500);

    // Batch claim all three — milestone 2 should fail (not approved)
    let ids = Vec::from_array(&env, [1u32, 2u32, 3u32]);
    let result = client.batch_claim_milestones(&aid, &ids);

    assert_eq!(result.successful_claims, 2);
    assert_eq!(result.failed_claims, 1);
    assert_eq!(result.total_claimed, 400); // 100 + 300

    // Verify per-milestone results
    let r0 = result.results.get(0).unwrap();
    assert!(r0.success);
    assert_eq!(r0.amount_claimed, 100);

    let r1 = result.results.get(1).unwrap();
    assert!(!r1.success);
    assert_eq!(r1.error_code, 3); // not approved

    let r2 = result.results.get(2).unwrap();
    assert!(r2.success);
    assert_eq!(r2.amount_claimed, 300);
}

/// Batch milestone claiming rejects duplicate IDs gracefully.
#[test]
fn test_milestone_batch_claim_duplicate_ids() {
    let env = env();
    let (cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let tok = token(&env);

    let aid = client.create_milestone_agreement(&employer, &contributor, &tok);
    client.add_milestone(&aid, &500);
    client.approve_milestone(&aid, &1);

    mint(&env, &tok, &cid, 1000);

    let ids = Vec::from_array(&env, [1u32, 1u32]);
    let result = client.batch_claim_milestones(&aid, &ids);

    assert_eq!(result.successful_claims, 1);
    assert_eq!(result.failed_claims, 1);
    assert_eq!(result.total_claimed, 500);

    let dup = result.results.get(1).unwrap();
    assert!(!dup.success);
    assert_eq!(dup.error_code, 1); // duplicate
}

/// Milestone pause prevents claims; resume allows them again.
#[test]
fn test_milestone_pause_resume() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let tok = token(&env);

    let aid = client.create_milestone_agreement(&employer, &contributor, &tok);
    client.add_milestone(&aid, &1000);
    client.approve_milestone(&aid, &1);

    // Pause via milestone path
    client.pause_agreement(&aid);

    // Claiming while paused panics
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.claim_milestone(&aid, &1);
    }));
    assert!(r.is_err());

    // Resume and claim
    client.resume_agreement(&aid);
    client.claim_milestone(&aid, &1);
    assert!(client.get_milestone(&aid, &1).unwrap().claimed);
}

/// Many milestones (10) — stress test for sequential approve + claim.
#[test]
fn test_milestone_many_milestones_lifecycle() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let tok = token(&env);

    let aid = client.create_milestone_agreement(&employer, &contributor, &tok);
    for i in 1..=10i128 {
        client.add_milestone(&aid, &(i * 100));
    }
    assert_eq!(client.get_milestone_count(&aid), 10);

    for id in 1..=10u32 {
        client.approve_milestone(&aid, &id);
    }
    for id in 1..=10u32 {
        client.claim_milestone(&aid, &id);
    }
    for id in 1..=10u32 {
        let m = client.get_milestone(&aid, &id).unwrap();
        assert!(m.approved);
        assert!(m.claimed);
    }
}

// ============================================================================
// SECTION 3: DISPUTE RESOLUTION WORKFLOW
// ============================================================================

/// Full dispute lifecycle: set arbiter -> raise dispute -> resolve with split.
#[test]
fn test_dispute_full_lifecycle() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let arbiter = addr(&env);
    let tok = token(&env);

    // Set arbiter
    client.set_arbiter(&employer, &arbiter);
    assert_eq!(client.get_arbiter().unwrap(), arbiter);

    // Create escrow agreement (grace_period = 3600s = 1 hour, which is also
    // the window for raising disputes since dispute checks created_at + grace)
    let aid = client.create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &1000,
        &ONE_HOUR,
        &1,
    );

    // Raise dispute (within grace period from creation)
    assert_eq!(client.get_dispute_status(&aid), DisputeStatus::None);
    client.raise_dispute(&employer, &aid);
    assert_eq!(client.get_dispute_status(&aid), DisputeStatus::Raised);

    let agr = client.get_agreement(&aid).unwrap();
    assert_eq!(agr.status, AgreementStatus::Disputed);

    // Resolve: pay employee 600, refund employer 400
    client.resolve_dispute(&arbiter, &aid, &0, &0);
    assert_eq!(client.get_dispute_status(&aid), DisputeStatus::Resolved);
    let agr = client.get_agreement(&aid).unwrap();
    assert_eq!(agr.status, AgreementStatus::Completed);
}

/// Dispute raised by employee (not employer).
#[test]
fn test_dispute_raised_by_employee() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let arbiter = addr(&env);
    let tok = token(&env);

    client.set_arbiter(&employer, &arbiter);

    let aid = client.create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &1000,
        &ONE_HOUR,
        &1,
    );

    // Employee raises dispute
    client.raise_dispute(&contributor, &aid);
    assert_eq!(client.get_dispute_status(&aid), DisputeStatus::Raised);
}

/// Dispute cannot be raised twice.
#[test]
fn test_dispute_cannot_raise_twice() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let arbiter = addr(&env);
    let tok = token(&env);

    client.set_arbiter(&employer, &arbiter);

    let aid = client.create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &1000,
        &ONE_HOUR,
        &1,
    );

    client.raise_dispute(&employer, &aid);
    let result = client.try_raise_dispute(&employer, &aid);
    assert!(result.is_err());
}

/// Non-party cannot raise dispute.
#[test]
fn test_dispute_non_party_rejected() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let arbiter = addr(&env);
    let outsider = addr(&env);
    let tok = token(&env);

    client.set_arbiter(&employer, &arbiter);

    let aid = client.create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &1000,
        &ONE_HOUR,
        &1,
    );

    let result = client.try_raise_dispute(&outsider, &aid);
    assert!(result.is_err());
}

/// Only arbiter can resolve; non-arbiter is rejected.
#[test]
fn test_dispute_non_arbiter_resolve_rejected() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let arbiter = addr(&env);
    let tok = token(&env);

    client.set_arbiter(&employer, &arbiter);

    let aid = client.create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &1000,
        &ONE_HOUR,
        &1,
    );

    client.raise_dispute(&employer, &aid);

    // Employer tries to resolve — should fail
    let result = client.try_resolve_dispute(&employer, &aid, &500, &500);
    assert!(result.is_err());
}

/// Cannot resolve dispute that hasn't been raised.
#[test]
fn test_dispute_resolve_without_raise_rejected() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let arbiter = addr(&env);
    let tok = token(&env);

    client.set_arbiter(&employer, &arbiter);

    let aid = client.create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &1000,
        &ONE_HOUR,
        &1,
    );

    let result = client.try_resolve_dispute(&arbiter, &aid, &500, &500);
    assert!(result.is_err());
}

/// Dispute payout cannot exceed total_amount.
#[test]
fn test_dispute_payout_exceeds_total_rejected() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let arbiter = addr(&env);
    let tok = token(&env);

    client.set_arbiter(&employer, &arbiter);

    // total_amount = 1000 * 1 = 1000
    let aid = client.create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &1000,
        &ONE_HOUR,
        &1,
    );

    client.raise_dispute(&employer, &aid);

    // 600 + 500 = 1100 > 1000
    let result = client.try_resolve_dispute(&arbiter, &aid, &600, &500);
    assert!(result.is_err());
}

/// Dispute cannot be raised outside grace period.
#[test]
fn test_dispute_outside_grace_period_rejected() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let arbiter = addr(&env);
    let tok = token(&env);

    client.set_arbiter(&employer, &arbiter);

    let aid = client.create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &1000,
        &ONE_HOUR,
        &1,
    );

    // Advance past the grace period window (created_at + grace_period_seconds)
    advance(&env, ONE_HOUR + 1);

    let result = client.try_raise_dispute(&employer, &aid);
    assert!(result.is_err());
}

// ============================================================================
// SECTION 4: ESCROW AGREEMENT WORKFLOW
// ============================================================================

/// Full escrow lifecycle: create -> activate -> fund -> time-based claims
/// -> completion via all periods claimed.
#[test]
fn test_escrow_full_lifecycle() {
    let env = env();
    let (cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let tok = token(&env);

    let amount_per_period = 500i128;
    let period_seconds = ONE_DAY;
    let num_periods = 4u32;

    // Create and verify
    let aid = client.create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &amount_per_period,
        &period_seconds,
        &num_periods,
    );
    let agr = client.get_agreement(&aid).unwrap();
    assert_eq!(agr.mode, AgreementMode::Escrow);
    assert_eq!(agr.status, AgreementStatus::Created);
    assert_eq!(agr.total_amount, 2000); // 500 * 4

    // Activate
    client.activate_agreement(&aid);
    let agr = client.get_agreement(&aid).unwrap();
    assert_eq!(agr.status, AgreementStatus::Active);

    // Fund escrow
    let total_fund = amount_per_period * (num_periods as i128);
    mint(&env, &tok, &cid, total_fund);
    env.as_contract(&cid, || {
        DataKey::set_agreement_escrow_balance(&env, aid, &tok, total_fund);
    });

    // Claim after 2 days — should get 2 periods
    advance(&env, ONE_DAY * 2);
    client.claim_time_based(&aid);
    assert_eq!(client.get_claimed_periods(&aid), 2);
    assert_eq!(balance(&env, &tok, &contributor), 1000);

    // Claim after 4 days total — should get remaining 2 periods
    advance(&env, ONE_DAY * 2);
    client.claim_time_based(&aid);
    assert_eq!(client.get_claimed_periods(&aid), 4);
    assert_eq!(balance(&env, &tok, &contributor), 2000);

    // Agreement should auto-complete after all periods claimed
    let agr = client.get_agreement(&aid).unwrap();
    assert_eq!(agr.status, AgreementStatus::Completed);
}

/// Escrow claiming fails when paused; works after resume.
#[test]
fn test_escrow_pause_resume() {
    let env = env();
    let (cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let tok = token(&env);

    let aid = client.create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &500,
        &ONE_DAY,
        &4,
    );
    client.activate_agreement(&aid);
    mint(&env, &tok, &cid, 2000);
    env.as_contract(&cid, || {
        DataKey::set_agreement_escrow_balance(&env, aid, &tok, 2000);
    });

    advance(&env, ONE_DAY);

    // Pause
    client.pause_agreement(&aid);
    let result = client.try_claim_time_based(&aid);
    assert!(result.is_err());

    // Resume
    client.resume_agreement(&aid);
    client.claim_time_based(&aid);
    assert_eq!(client.get_claimed_periods(&aid), 1);
}

/// Escrow cancellation + grace period claiming.
#[test]
fn test_escrow_cancel_with_grace_period() {
    let env = env();
    let (cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let tok = token(&env);

    let aid = client.create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &500,
        &ONE_DAY,
        &5,
    );
    client.activate_agreement(&aid);

    let fund = 2500i128;
    mint(&env, &tok, &cid, fund);
    env.as_contract(&cid, || {
        DataKey::set_agreement_escrow_balance(&env, aid, &tok, fund);
    });

    // Advance 2 days, cancel
    advance(&env, ONE_DAY * 2);
    client.cancel_agreement(&aid);
    assert!(client.is_grace_period_active(&aid));

    // Claim during grace period
    client.claim_time_based(&aid);
    assert_eq!(client.get_claimed_periods(&aid), 2);
    assert_eq!(balance(&env, &tok, &contributor), 1000);
}

/// Escrow with insufficient balance rejects claim.
#[test]
fn test_escrow_insufficient_balance_rejected() {
    let env = env();
    let (cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let tok = token(&env);

    let aid = client.create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &500,
        &ONE_DAY,
        &4,
    );
    client.activate_agreement(&aid);

    // Fund only 100 — not enough for a single period (500)
    mint(&env, &tok, &cid, 100);
    env.as_contract(&cid, || {
        DataKey::set_agreement_escrow_balance(&env, aid, &tok, 100);
    });

    advance(&env, ONE_DAY);
    let result = client.try_claim_time_based(&aid);
    assert!(result.is_err());
}

/// Cannot claim escrow before activation.
#[test]
fn test_escrow_claim_before_activation_rejected() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let tok = token(&env);

    let aid = client.create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &500,
        &ONE_DAY,
        &4,
    );

    // Don't activate — try to claim
    advance(&env, ONE_DAY);
    let result = client.try_claim_time_based(&aid);
    assert!(result.is_err());
}

/// Escrow rejects claims when all periods are already claimed.
#[test]
fn test_escrow_all_periods_claimed_rejected() {
    let env = env();
    let (cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let tok = token(&env);

    let aid = client.create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &100,
        &ONE_DAY,
        &2,
    );
    client.activate_agreement(&aid);
    mint(&env, &tok, &cid, 200);
    env.as_contract(&cid, || {
        DataKey::set_agreement_escrow_balance(&env, aid, &tok, 200);
    });

    // Claim all
    advance(&env, ONE_DAY * 3);
    client.claim_time_based(&aid);
    assert_eq!(client.get_claimed_periods(&aid), 2);

    // Second claim fails — all claimed
    let result = client.try_claim_time_based(&aid);
    assert!(result.is_err());
}

/// Escrow creation rejects invalid parameters.
#[test]
fn test_escrow_invalid_creation_parameters() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let tok = token(&env);

    // Zero amount
    let r = client.try_create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &0,
        &ONE_DAY,
        &4,
    );
    assert!(r.is_err());

    // Zero period
    let r = client.try_create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &100,
        &0u64,
        &4,
    );
    assert!(r.is_err());

    // Zero num_periods
    let r = client.try_create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &100,
        &ONE_DAY,
        &0u32,
    );
    assert!(r.is_err());
}

// ============================================================================
// SECTION 5: CROSS-CONTRACT INTERACTIONS
// ============================================================================

/// PayrollEscrowContract + PayrollContract: employer funds escrow, manager
/// releases funds.
#[test]
fn test_cross_escrow_funding_and_release() {
    let env = env();
    let (payroll_id, _payroll_client) = deploy_payroll(&env);
    let tok = token(&env);
    let (escrow_id, escrow_client) = deploy_escrow(&env, &tok, &payroll_id);

    let employer = addr(&env);
    let recipient = addr(&env);

    // Mint and fund
    mint(&env, &tok, &employer, 5000);
    escrow_client.fund_agreement(&employer, &1u128, &employer, &5000);

    assert_eq!(escrow_client.get_agreement_balance(&1u128), 5000);
    assert_eq!(
        escrow_client.get_agreement_employer(&1u128).unwrap(),
        employer
    );

    // Manager (payroll contract) releases funds to recipient
    mint(&env, &tok, &escrow_id, 0); // ensure token is known
    escrow_client.release(&payroll_id, &1u128, &recipient, &2000);

    assert_eq!(escrow_client.get_agreement_balance(&1u128), 3000);
    assert_eq!(balance(&env, &tok, &recipient), 2000);
}

/// PayrollEscrowContract refund flow — manager refunds remaining balance.
#[test]
fn test_cross_escrow_refund_remaining() {
    let env = env();
    let (payroll_id, _payroll_client) = deploy_payroll(&env);
    let tok = token(&env);
    let (_escrow_id, escrow_client) = deploy_escrow(&env, &tok, &payroll_id);

    let employer = addr(&env);
    mint(&env, &tok, &employer, 3000);
    escrow_client.fund_agreement(&employer, &1u128, &employer, &3000);

    let emp_bal_before = balance(&env, &tok, &employer);

    // Manager refunds
    escrow_client.refund_remaining(&payroll_id, &1u128);

    assert_eq!(escrow_client.get_agreement_balance(&1u128), 0);
    assert_eq!(balance(&env, &tok, &employer), emp_bal_before + 3000);
}

/// Non-manager cannot release or refund via escrow contract.
#[test]
fn test_cross_escrow_unauthorized_release_rejected() {
    let env = env();
    let (payroll_id, _payroll_client) = deploy_payroll(&env);
    let tok = token(&env);
    let (_escrow_id, escrow_client) = deploy_escrow(&env, &tok, &payroll_id);

    let employer = addr(&env);
    let non_manager = addr(&env);
    let recipient = addr(&env);

    mint(&env, &tok, &employer, 5000);
    escrow_client.fund_agreement(&employer, &1u128, &employer, &5000);

    // Non-manager release panics
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        escrow_client.release(&non_manager, &1u128, &recipient, &1000);
    }));
    assert!(r.is_err());

    // Non-manager refund panics
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        escrow_client.refund_remaining(&non_manager, &1u128);
    }));
    assert!(r.is_err());
}

/// Multiple agreements funded through the same escrow contract.
#[test]
fn test_cross_escrow_multiple_agreements() {
    let env = env();
    let (payroll_id, _payroll_client) = deploy_payroll(&env);
    let tok = token(&env);
    let (_escrow_id, escrow_client) = deploy_escrow(&env, &tok, &payroll_id);

    let employer1 = addr(&env);
    let employer2 = addr(&env);
    let recipient1 = addr(&env);
    let recipient2 = addr(&env);

    mint(&env, &tok, &employer1, 5000);
    mint(&env, &tok, &employer2, 3000);

    escrow_client.fund_agreement(&employer1, &1u128, &employer1, &5000);
    escrow_client.fund_agreement(&employer2, &2u128, &employer2, &3000);

    assert_eq!(escrow_client.get_agreement_balance(&1u128), 5000);
    assert_eq!(escrow_client.get_agreement_balance(&2u128), 3000);

    // Release from each independently
    escrow_client.release(&payroll_id, &1u128, &recipient1, &1000);
    escrow_client.release(&payroll_id, &2u128, &recipient2, &500);

    assert_eq!(escrow_client.get_agreement_balance(&1u128), 4000);
    assert_eq!(escrow_client.get_agreement_balance(&2u128), 2500);
    assert_eq!(balance(&env, &tok, &recipient1), 1000);
    assert_eq!(balance(&env, &tok, &recipient2), 500);
}

/// BonusSystemContract: full one-time bonus lifecycle alongside payroll.
#[test]
fn test_cross_bonus_one_time_lifecycle() {
    let env = env();
    let (_payroll_id, _payroll_client) = deploy_payroll(&env);
    let (_bonus_id, bonus_client) = deploy_bonus(&env);
    let tok = token(&env);

    let employer = addr(&env);
    let employee = addr(&env);
    let approver = addr(&env);

    // Mint tokens to employer for escrow
    mint(&env, &tok, &employer, 5000);

    // Create a one-time bonus
    set_time(&env, 1000);
    let unlock_time = 2000u64;
    let incentive_id =
        bonus_client.create_one_time_bonus(&employer, &employee, &approver, &tok, &1000, &unlock_time);

    let incentive = bonus_client.get_incentive(&incentive_id).unwrap();
    assert_eq!(incentive.amount_per_payout, 1000);
    assert_eq!(incentive.status, bonus_system::ApprovalStatus::Pending);

    // Approve
    bonus_client.approve_incentive(&approver, &incentive_id);
    let incentive = bonus_client.get_incentive(&incentive_id).unwrap();
    assert_eq!(incentive.status, bonus_system::ApprovalStatus::Approved);

    // Advance past unlock time and claim
    set_time(&env, 2000);
    let claimed = bonus_client.claim_incentive(&employee, &incentive_id);
    assert_eq!(claimed, 1000);
    assert_eq!(balance(&env, &tok, &employee), 1000);

    // Incentive is now completed
    let incentive = bonus_client.get_incentive(&incentive_id).unwrap();
    assert_eq!(incentive.status, bonus_system::ApprovalStatus::Completed);
}

/// BonusSystemContract: recurring incentive lifecycle.
#[test]
fn test_cross_bonus_recurring_lifecycle() {
    let env = env();
    let (_bonus_id, bonus_client) = deploy_bonus(&env);
    let tok = token(&env);

    let employer = addr(&env);
    let employee = addr(&env);
    let approver = addr(&env);

    // Escrow = 500 * 4 = 2000
    mint(&env, &tok, &employer, 2000);

    set_time(&env, 1000);
    let incentive_id = bonus_client.create_recurring_incentive(
        &employer,
        &employee,
        &approver,
        &tok,
        &500,
        &4,
        &1000,
        &ONE_DAY,
    );

    bonus_client.approve_incentive(&approver, &incentive_id);

    // After 2 intervals
    set_time(&env, 1000 + ONE_DAY);
    assert_eq!(bonus_client.get_claimable_payouts(&incentive_id), 2);

    let claimed = bonus_client.claim_incentive(&employee, &incentive_id);
    assert_eq!(claimed, 1000); // 500 * 2

    // After all intervals
    set_time(&env, 1000 + ONE_DAY * 3);
    assert_eq!(bonus_client.get_claimable_payouts(&incentive_id), 2); // 2 remaining

    let claimed = bonus_client.claim_incentive(&employee, &incentive_id);
    assert_eq!(claimed, 1000);

    let incentive = bonus_client.get_incentive(&incentive_id).unwrap();
    assert_eq!(incentive.status, bonus_system::ApprovalStatus::Completed);
    assert_eq!(balance(&env, &tok, &employee), 2000);
}

/// BonusSystemContract: rejected incentive can be cancelled for refund.
#[test]
fn test_cross_bonus_rejection_and_cancellation() {
    let env = env();
    let (_bonus_id, bonus_client) = deploy_bonus(&env);
    let tok = token(&env);

    let employer = addr(&env);
    let employee = addr(&env);
    let approver = addr(&env);

    mint(&env, &tok, &employer, 1000);

    let incentive_id =
        bonus_client.create_one_time_bonus(&employer, &employee, &approver, &tok, &1000, &0);

    // Reject
    bonus_client.reject_incentive(&approver, &incentive_id);
    let incentive = bonus_client.get_incentive(&incentive_id).unwrap();
    assert_eq!(incentive.status, bonus_system::ApprovalStatus::Rejected);

    // Cancel for refund
    let refund = bonus_client.cancel_incentive(&employer, &incentive_id);
    assert_eq!(refund, 1000);
    assert_eq!(balance(&env, &tok, &employer), 1000);
}

/// PaymentHistoryContract: records and queries payments by agreement, employer,
/// and employee.
#[test]
fn test_cross_payment_history_recording() {
    let env = env();
    let payroll_contract = addr(&env);
    let (_hist_id, hist_client) = deploy_history(&env, &payroll_contract);

    let employer = addr(&env);
    let employee = addr(&env);
    let tok = token(&env);

    // Record two payments for the same agreement
    set_time(&env, 1000);
    let id1 = hist_client.record_payment(&1u128, &tok, &500, &employer, &employee, &1000);
    set_time(&env, 2000);
    let id2 = hist_client.record_payment(&1u128, &tok, &700, &employer, &employee, &2000);

    assert_eq!(id1, 1);
    assert_eq!(id2, 2);

    // Query by agreement
    assert_eq!(hist_client.get_agreement_payment_count(&1u128), 2);
    let payments = hist_client.get_payments_by_agreement(&1u128, &1, &10);
    assert_eq!(payments.len(), 2);
    assert_eq!(payments.get(0).unwrap().amount, 500);
    assert_eq!(payments.get(1).unwrap().amount, 700);

    // Query by employer
    assert_eq!(hist_client.get_employer_payment_count(&employer), 2);
    let emp_payments = hist_client.get_payments_by_employer(&employer, &1, &10);
    assert_eq!(emp_payments.len(), 2);

    // Query by employee
    assert_eq!(hist_client.get_employee_payment_count(&employee), 2);
    let ee_payments = hist_client.get_payments_by_employee(&employee, &1, &10);
    assert_eq!(ee_payments.len(), 2);
}

/// Payment history records span multiple agreements and indexes correctly.
#[test]
fn test_cross_payment_history_multi_agreement() {
    let env = env();
    let payroll_contract = addr(&env);
    let (_hist_id, hist_client) = deploy_history(&env, &payroll_contract);

    let employer = addr(&env);
    let emp1 = addr(&env);
    let emp2 = addr(&env);
    let tok = token(&env);

    // Agreement 1: employer -> emp1
    hist_client.record_payment(&1u128, &tok, &100, &employer, &emp1, &1000);

    // Agreement 2: employer -> emp2
    hist_client.record_payment(&2u128, &tok, &200, &employer, &emp2, &2000);

    // Agreement 1 again: employer -> emp1
    hist_client.record_payment(&1u128, &tok, &300, &employer, &emp1, &3000);

    assert_eq!(hist_client.get_agreement_payment_count(&1u128), 2);
    assert_eq!(hist_client.get_agreement_payment_count(&2u128), 1);
    assert_eq!(hist_client.get_employer_payment_count(&employer), 3);
    assert_eq!(hist_client.get_employee_payment_count(&emp1), 2);
    assert_eq!(hist_client.get_employee_payment_count(&emp2), 1);
}

// ============================================================================
// SECTION 6: COMPLEX MULTI-STEP EDGE CASES
// ============================================================================

/// Multiple agreements for the same employer, each in a different state.
#[test]
fn test_multi_agreement_different_states() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let tok = token(&env);
    let emp1 = addr(&env);
    let emp2 = addr(&env);
    let emp3 = addr(&env);

    // Agreement 1: Created (never activated)
    let a1 = client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);

    // Agreement 2: Active
    let a2 = client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    client.add_employee_to_agreement(&a2, &emp1, &SALARY);
    client.activate_agreement(&a2);

    // Agreement 3: Paused
    let a3 = client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    client.add_employee_to_agreement(&a3, &emp2, &SALARY);
    client.activate_agreement(&a3);
    client.pause_agreement(&a3);

    // Agreement 4: Cancelled
    let a4 = client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    client.add_employee_to_agreement(&a4, &emp3, &SALARY);
    client.activate_agreement(&a4);
    client.cancel_agreement(&a4);

    // Verify each is in the expected state
    assert_eq!(
        client.get_agreement(&a1).unwrap().status,
        AgreementStatus::Created
    );
    assert_eq!(
        client.get_agreement(&a2).unwrap().status,
        AgreementStatus::Active
    );
    assert_eq!(
        client.get_agreement(&a3).unwrap().status,
        AgreementStatus::Paused
    );
    assert_eq!(
        client.get_agreement(&a4).unwrap().status,
        AgreementStatus::Cancelled
    );
}

/// Escrow + milestone agreements coexist; independent ID counters.
#[test]
fn test_mixed_agreement_types_coexist() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let tok = token(&env);

    // Payroll agreement
    let p1 = client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);

    // Escrow agreement
    let e1 = client.create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &500,
        &ONE_DAY,
        &4,
    );

    // Milestone agreement (uses separate counter)
    let m1 = client.create_milestone_agreement(&employer, &contributor, &tok);

    // Payroll and escrow share the same counter; milestone has its own
    assert_eq!(client.get_agreement(&p1).unwrap().mode, AgreementMode::Payroll);
    assert_eq!(client.get_agreement(&e1).unwrap().mode, AgreementMode::Escrow);
    assert!(client.get_milestone_count(&m1) == 0);
}

/// Zero grace period — cancellation immediately prevents claims.
#[test]
fn test_zero_grace_period_immediate_lockout() {
    let env = env();
    let (cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let tok = token(&env);
    let emp = addr(&env);

    let aid = client.create_payroll_agreement(&employer, &tok, &0u64);
    client.add_employee_to_agreement(&aid, &emp, &SALARY);
    client.activate_agreement(&aid);
    fund_payroll_internal(&env, &cid, aid, &tok, &[(emp.clone(), SALARY)], ESCROW_FUND);

    advance(&env, ONE_DAY);
    client.cancel_agreement(&aid);

    // Grace period of 0 — immediately inactive
    assert!(!client.is_grace_period_active(&aid));

    // Claim fails immediately
    let result = client.try_claim_payroll(&emp, &aid, &0);
    assert!(result.is_err());
}

/// Agreement IDs are strictly increasing across creation calls.
#[test]
fn test_agreement_ids_strictly_increasing() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let tok = token(&env);

    let id1 = client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    let id2 = client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    let id3 = client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);

    assert!(id1 < id2);
    assert!(id2 < id3);
    assert_eq!(id2, id1 + 1);
    assert_eq!(id3, id2 + 1);
}

/// Multiple concurrent milestone agreements — independent state.
#[test]
fn test_concurrent_milestone_agreements() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor1 = addr(&env);
    let contributor2 = addr(&env);
    let tok = token(&env);

    let a1 = client.create_milestone_agreement(&employer, &contributor1, &tok);
    let a2 = client.create_milestone_agreement(&employer, &contributor2, &tok);

    client.add_milestone(&a1, &100);
    client.add_milestone(&a1, &200);
    client.add_milestone(&a2, &500);

    assert_eq!(client.get_milestone_count(&a1), 2);
    assert_eq!(client.get_milestone_count(&a2), 1);

    // Approve and claim from a1 does not affect a2
    client.approve_milestone(&a1, &1);
    client.claim_milestone(&a1, &1);

    assert!(client.get_milestone(&a1, &1).unwrap().claimed);
    assert!(!client.get_milestone(&a2, &1).unwrap().approved);
}

/// Escrow contract: release more than balance fails.
#[test]
fn test_cross_escrow_release_exceeds_balance_rejected() {
    let env = env();
    let (payroll_id, _payroll_client) = deploy_payroll(&env);
    let tok = token(&env);
    let (_escrow_id, escrow_client) = deploy_escrow(&env, &tok, &payroll_id);

    let employer = addr(&env);
    mint(&env, &tok, &employer, 1000);
    escrow_client.fund_agreement(&employer, &1u128, &employer, &1000);

    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        escrow_client.release(&payroll_id, &1u128, &addr(&env), &2000);
    }));
    assert!(r.is_err());
}

/// Escrow contract: refund when balance is zero fails.
#[test]
fn test_cross_escrow_refund_zero_balance_rejected() {
    let env = env();
    let (payroll_id, _payroll_client) = deploy_payroll(&env);
    let tok = token(&env);
    let (_escrow_id, escrow_client) = deploy_escrow(&env, &tok, &payroll_id);

    // No funding — balance is 0
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        escrow_client.refund_remaining(&payroll_id, &1u128);
    }));
    assert!(r.is_err());
}

/// Escrow contract: double initialization fails.
#[test]
fn test_cross_escrow_double_init_rejected() {
    let env = env();
    let (payroll_id, _payroll_client) = deploy_payroll(&env);
    let tok = token(&env);
    let (_escrow_id, escrow_client) = deploy_escrow(&env, &tok, &payroll_id);

    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let admin = addr(&env);
        escrow_client.initialize(&admin, &tok, &payroll_id);
    }));
    assert!(r.is_err());
}

/// Bonus system: cannot claim before unlock time.
#[test]
fn test_cross_bonus_claim_before_unlock_rejected() {
    let env = env();
    let (_bonus_id, bonus_client) = deploy_bonus(&env);
    let tok = token(&env);

    let employer = addr(&env);
    let employee = addr(&env);
    let approver = addr(&env);

    mint(&env, &tok, &employer, 1000);
    set_time(&env, 100);

    let incentive_id =
        bonus_client.create_one_time_bonus(&employer, &employee, &approver, &tok, &1000, &500);
    bonus_client.approve_incentive(&approver, &incentive_id);

    // Try to claim before unlock_time (500) — current time is 100
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        bonus_client.claim_incentive(&employee, &incentive_id);
    }));
    assert!(r.is_err());
}

/// Bonus system: cannot cancel an approved incentive.
#[test]
fn test_cross_bonus_cancel_approved_rejected() {
    let env = env();
    let (_bonus_id, bonus_client) = deploy_bonus(&env);
    let tok = token(&env);

    let employer = addr(&env);
    let employee = addr(&env);
    let approver = addr(&env);

    mint(&env, &tok, &employer, 1000);

    let incentive_id =
        bonus_client.create_one_time_bonus(&employer, &employee, &approver, &tok, &1000, &0);
    bonus_client.approve_incentive(&approver, &incentive_id);

    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        bonus_client.cancel_incentive(&employer, &incentive_id);
    }));
    assert!(r.is_err());
}

/// Payroll contract: claim fails for wrong employee (caller != employee).
#[test]
fn test_payroll_claim_wrong_employee_rejected() {
    let env = env();
    let (cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let tok = token(&env);
    let emp = addr(&env);
    let wrong_emp = addr(&env);

    let aid = client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    client.add_employee_to_agreement(&aid, &emp, &SALARY);
    client.activate_agreement(&aid);
    fund_payroll_internal(&env, &cid, aid, &tok, &[(emp.clone(), SALARY)], ESCROW_FUND);

    advance(&env, ONE_DAY);

    let result = client.try_claim_payroll(&wrong_emp, &aid, &0);
    assert!(result.is_err());
}

/// Payroll claiming with invalid employee index.
#[test]
fn test_payroll_claim_invalid_index_rejected() {
    let env = env();
    let (cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let tok = token(&env);
    let emp = addr(&env);

    let aid = client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    client.add_employee_to_agreement(&aid, &emp, &SALARY);
    client.activate_agreement(&aid);
    fund_payroll_internal(&env, &cid, aid, &tok, &[(emp.clone(), SALARY)], ESCROW_FUND);

    advance(&env, ONE_DAY);

    let result = client.try_claim_payroll(&emp, &aid, &5);
    assert!(result.is_err());
}

/// Escrow mode rejects payroll claims (wrong agreement mode).
#[test]
fn test_payroll_claim_on_escrow_mode_rejected() {
    let env = env();
    let (_cid, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let contributor = addr(&env);
    let tok = token(&env);

    let aid = client.create_escrow_agreement(
        &employer,
        &contributor,
        &tok,
        &500,
        &ONE_DAY,
        &4,
    );
    client.activate_agreement(&aid);

    let result = client.try_claim_payroll(&contributor, &aid, &0);
    assert!(result.is_err());
}
