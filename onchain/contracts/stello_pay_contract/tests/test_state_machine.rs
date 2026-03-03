//! State machine test suite (#217).
//!
//! Validates correct state transitions for agreements through their lifecycle,
//! including valid paths, invalid paths, state persistence, and state recovery.
//!
//! # Coverage
//!
//! | Section | Scenario |
//! |---------|----------|
//! | 1  | Created -> Active transition (payroll) |
//! | 2  | Active -> Paused transition (payroll) |
//! | 3  | Paused -> Active transition (payroll) |
//! | 4  | Created -> Cancelled transition (payroll) |
//! | 5  | Active -> Cancelled transition (payroll) |
//! | 6  | Created -> Active transition (escrow) |
//! | 7  | Active -> Completed via all claims (escrow) |
//! | 8  | Grace period finalization refunds escrow |
//! | 9  | Active -> Disputed via raise_dispute |
//! | 10 | Disputed -> Completed via resolve_dispute |
//! | 11 | Created -> Paused transition (milestone) |
//! | 12 | Paused -> Active transition (milestone) |
//! | 13 | Auto-complete on last milestone claim |
//! | 14 | Activate Active agreement rejected |
//! | 15 | Activate Paused agreement rejected |
//! | 16 | Activate Cancelled agreement rejected |
//! | 17 | Pause Created payroll agreement rejected |
//! | 18 | Pause already Paused agreement rejected |
//! | 19 | Pause Cancelled agreement rejected |
//! | 20 | Resume Active agreement rejected |
//! | 21 | Resume Created agreement rejected |
//! | 22 | Cancel Paused agreement rejected |
//! | 23 | Cancel Disputed agreement rejected |
//! | 24 | Cancel already Cancelled agreement rejected |
//! | 25 | Add employee to Active agreement rejected |
//! | 26 | Finalize before grace period expiry rejected |
//! | 27 | Duplicate dispute raise returns error |
//! | 28 | Dispute outside grace window returns error |
//! | 29 | Activation timestamp persisted correctly |
//! | 30 | Cancellation timestamp persisted correctly |
//! | 31 | Pause/resume preserves all agreement fields |
//! | 32 | Employee list preserved across transitions |
//! | 33 | State unchanged after failed transition |
//! | 34 | Multiple pause/resume cycles consistent |
//! | 35 | Full lifecycle: Created through finalization |

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};
use stello_pay_contract::storage::{AgreementStatus, DataKey, DisputeStatus, MilestoneKey};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ============================================================================
// CONSTANTS
// ============================================================================

const ONE_DAY: u64 = 86400;
const ONE_WEEK: u64 = 604800;
const SALARY: i128 = 1000;

// ============================================================================
// HELPERS
// ============================================================================

/// Creates a test environment with all auths mocked.
fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Generates a random test address.
fn create_address(env: &Env) -> Address {
    Address::generate(env)
}

/// Deploys a Stellar Asset Contract and returns its address.
fn create_token(env: &Env) -> Address {
    let admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

/// Mints tokens to a given address.
fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

/// Deploys and initializes the payroll contract.
fn setup_contract(env: &Env) -> (Address, PayrollContractClient<'static>) {
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = create_address(env);
    client.initialize(&owner);
    (contract_id, client)
}

/// Advances the ledger timestamp by the given number of seconds.
fn advance_time(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| {
        li.timestamp += seconds;
    });
}

// ============================================================================
// 1. VALID PAYROLL/ESCROW LIFECYCLE TRANSITIONS
// ============================================================================

/// Verifies Created -> Active transition for a payroll agreement.
/// Requires at least one employee before activation.
#[test]
fn test_payroll_created_to_active() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Created
    );

    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);

    let a = client.get_agreement(&id).unwrap();
    assert_eq!(a.status, AgreementStatus::Active);
    assert!(a.activated_at.is_some());
}

/// Verifies Active -> Paused transition. Only Active agreements can be paused.
#[test]
fn test_payroll_active_to_paused() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);
    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Active
    );

    client.pause_agreement(&id);

    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Paused
    );
}

/// Verifies Paused -> Active transition via resume_agreement.
#[test]
fn test_payroll_paused_to_active() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);
    client.pause_agreement(&id);
    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Paused
    );

    client.resume_agreement(&id);

    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Active
    );
}

/// Verifies Created -> Cancelled transition.
/// Agreements can be cancelled directly from Created without activation.
#[test]
fn test_payroll_created_to_cancelled() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Created
    );

    client.cancel_agreement(&id);

    let a = client.get_agreement(&id).unwrap();
    assert_eq!(a.status, AgreementStatus::Cancelled);
    assert!(a.cancelled_at.is_some());
}

/// Verifies Active -> Cancelled transition.
/// Cancelling an active agreement initiates the grace period.
#[test]
fn test_payroll_active_to_cancelled() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);
    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Active
    );

    client.cancel_agreement(&id);

    let a = client.get_agreement(&id).unwrap();
    assert_eq!(a.status, AgreementStatus::Cancelled);
    assert!(a.cancelled_at.is_some());
}

/// Verifies Created -> Active transition for an escrow agreement.
/// Escrow agreements have a contributor set at creation.
#[test]
fn test_escrow_created_to_active() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_address(&env);

    let id =
        client.create_escrow_agreement(&employer, &contributor, &token, &SALARY, &ONE_DAY, &4u32);
    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Created
    );

    client.activate_agreement(&id);

    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Active
    );
}

/// Verifies Active -> Completed transition for an escrow agreement when all
/// periods are claimed via claim_time_based.
#[test]
fn test_escrow_active_to_completed_via_all_claims() {
    let env = create_test_env();
    let (cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);
    let amount_per_period = SALARY;
    let period_seconds = ONE_DAY;
    let num_periods = 2u32;
    let total = amount_per_period * (num_periods as i128);

    let id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &amount_per_period,
        &period_seconds,
        &num_periods,
    );
    client.activate_agreement(&id);
    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Active
    );

    mint(&env, &token, &cid, total);
    env.as_contract(&cid, || {
        DataKey::set_agreement_escrow_balance(&env, id, &token, total);
    });

    advance_time(&env, period_seconds * (num_periods as u64) + 1);
    client.claim_time_based(&id);

    let a = client.get_agreement(&id).unwrap();
    assert_eq!(a.status, AgreementStatus::Completed);
    assert_eq!(a.claimed_periods, Some(num_periods));
}

/// Verifies that finalize_grace_period refunds remaining escrow to the employer
/// after the grace period has expired on a Cancelled agreement.
#[test]
fn test_finalize_grace_period_refunds_escrow() {
    let env = create_test_env();
    let (cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_token(&env);
    let employee = create_address(&env);
    let grace = ONE_WEEK;

    let id = client.create_payroll_agreement(&employer, &token, &grace);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);

    mint(&env, &token, &cid, SALARY);
    env.as_contract(&cid, || {
        DataKey::set_agreement_escrow_balance(&env, id, &token, SALARY);
    });

    client.cancel_agreement(&id);
    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Cancelled
    );

    advance_time(&env, grace + 1);
    client.finalize_grace_period(&id);

    env.as_contract(&cid, || {
        let balance = DataKey::get_agreement_escrow_balance(&env, id, &token);
        assert_eq!(balance, 0);
    });

    // Agreement remains in Cancelled status after finalization.
    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Cancelled
    );
}

// ============================================================================
// 2. DISPUTE LIFECYCLE TRANSITIONS
// ============================================================================

/// Verifies Active -> Disputed transition via raise_dispute.
/// A party can raise a dispute within the grace window from creation.
#[test]
fn test_active_to_disputed_via_raise_dispute() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);

    client.raise_dispute(&employer, &id);

    let a = client.get_agreement(&id).unwrap();
    assert_eq!(a.status, AgreementStatus::Disputed);
    assert_eq!(a.dispute_status, DisputeStatus::Raised);
    assert!(a.dispute_raised_at.is_some());
}

/// Verifies Disputed -> Completed transition via resolve_dispute.
/// The arbiter resolves the dispute and distributes funds accordingly.
#[test]
fn test_disputed_to_completed_via_resolve_dispute() {
    let env = create_test_env();
    let (cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_token(&env);
    let employee = create_address(&env);
    let arbiter = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);

    client.set_arbiter(&employer, &arbiter);
    mint(&env, &token, &cid, SALARY);

    client.raise_dispute(&employer, &id);
    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Disputed
    );

    let half = SALARY / 2;
    client.resolve_dispute(&arbiter, &id, &half, &half);

    let a = client.get_agreement(&id).unwrap();
    assert_eq!(a.status, AgreementStatus::Completed);
    assert_eq!(a.dispute_status, DisputeStatus::Resolved);
}

// ============================================================================
// 3. MILESTONE LIFECYCLE TRANSITIONS
// ============================================================================

/// Verifies Created -> Paused transition for a milestone agreement.
/// Milestone agreements can be paused directly from Created status.
#[test]
fn test_milestone_created_to_paused() {
    let env = create_test_env();
    let (cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_address(&env);

    let ms_id = client.create_milestone_agreement(&employer, &contributor, &token);

    client.pause_agreement(&ms_id);

    env.as_contract(&cid, || {
        let status: AgreementStatus = env
            .storage()
            .instance()
            .get(&MilestoneKey::Status(ms_id))
            .unwrap();
        assert_eq!(status, AgreementStatus::Paused);
    });
}

/// Verifies Paused -> Active transition for a milestone agreement.
/// Resuming sets the milestone agreement to Active regardless of prior state.
#[test]
fn test_milestone_paused_to_active() {
    let env = create_test_env();
    let (cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_address(&env);

    let ms_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.pause_agreement(&ms_id);
    client.resume_agreement(&ms_id);

    env.as_contract(&cid, || {
        let status: AgreementStatus = env
            .storage()
            .instance()
            .get(&MilestoneKey::Status(ms_id))
            .unwrap();
        assert_eq!(status, AgreementStatus::Active);
    });
}

/// Verifies auto-completion when the last milestone in an agreement is claimed.
/// Status transitions to Completed only after every milestone is claimed.
#[test]
fn test_milestone_complete_on_last_claim() {
    let env = create_test_env();
    let (cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_address(&env);

    let ms_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&ms_id, &1000i128);
    client.add_milestone(&ms_id, &2000i128);

    client.approve_milestone(&ms_id, &1u32);
    client.approve_milestone(&ms_id, &2u32);

    // First claim: not yet complete.
    client.claim_milestone(&ms_id, &1u32);
    env.as_contract(&cid, || {
        let status: AgreementStatus = env
            .storage()
            .instance()
            .get(&MilestoneKey::Status(ms_id))
            .unwrap();
        assert_ne!(status, AgreementStatus::Completed);
    });

    // Second (last) claim: triggers auto-complete.
    client.claim_milestone(&ms_id, &2u32);
    env.as_contract(&cid, || {
        let status: AgreementStatus = env
            .storage()
            .instance()
            .get(&MilestoneKey::Status(ms_id))
            .unwrap();
        assert_eq!(status, AgreementStatus::Completed);
    });
}

// ============================================================================
// 4. INVALID STATE TRANSITIONS
// ============================================================================

/// Activating an already Active agreement must be rejected.
#[test]
#[should_panic]
fn test_activate_active_agreement_panics() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);
    client.activate_agreement(&id);
}

/// Activating a Paused agreement must be rejected.
#[test]
#[should_panic]
fn test_activate_paused_agreement_panics() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);
    client.pause_agreement(&id);
    client.activate_agreement(&id);
}

/// Activating a Cancelled agreement must be rejected.
#[test]
#[should_panic]
fn test_activate_cancelled_agreement_panics() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.cancel_agreement(&id);
    client.activate_agreement(&id);
}

/// Pausing a Created payroll agreement must be rejected.
/// Only Active payroll agreements can be paused.
#[test]
#[should_panic]
fn test_pause_created_payroll_panics() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.pause_agreement(&id);
}

/// Pausing an already Paused agreement must be rejected.
#[test]
#[should_panic]
fn test_pause_paused_agreement_panics() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);
    client.pause_agreement(&id);
    client.pause_agreement(&id);
}

/// Pausing a Cancelled agreement must be rejected.
#[test]
#[should_panic]
fn test_pause_cancelled_agreement_panics() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);
    client.cancel_agreement(&id);
    client.pause_agreement(&id);
}

/// Resuming an Active agreement must be rejected.
/// Only Paused agreements can be resumed.
#[test]
#[should_panic]
fn test_resume_active_agreement_panics() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);
    client.resume_agreement(&id);
}

/// Resuming a Created agreement must be rejected.
#[test]
#[should_panic]
fn test_resume_created_agreement_panics() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.resume_agreement(&id);
}

/// Cancelling a Paused agreement must be rejected.
/// Only Active or Created agreements can be cancelled.
#[test]
#[should_panic]
fn test_cancel_paused_agreement_panics() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);
    client.pause_agreement(&id);
    client.cancel_agreement(&id);
}

/// Cancelling a Disputed agreement must be rejected.
#[test]
#[should_panic]
fn test_cancel_disputed_agreement_panics() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);
    client.raise_dispute(&employer, &id);
    client.cancel_agreement(&id);
}

/// Cancelling an already Cancelled agreement must be rejected.
#[test]
#[should_panic]
fn test_cancel_cancelled_agreement_panics() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.cancel_agreement(&id);
    client.cancel_agreement(&id);
}

/// Adding an employee to an Active agreement must be rejected.
/// Employees can only be added while the agreement is in Created status.
#[test]
#[should_panic]
fn test_add_employee_to_active_panics() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let e1 = create_address(&env);
    let e2 = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &e1, &SALARY);
    client.activate_agreement(&id);
    client.add_employee_to_agreement(&id, &e2, &SALARY);
}

/// Finalizing the grace period before it expires must be rejected.
#[test]
#[should_panic]
fn test_finalize_before_grace_expiry_panics() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);
    client.cancel_agreement(&id);
    client.finalize_grace_period(&id);
}

/// Raising a dispute when one is already active must return an error.
#[test]
fn test_dispute_already_raised_returns_error() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);

    client.raise_dispute(&employer, &id);

    let result = client.try_raise_dispute(&employer, &id);
    assert!(result.is_err());
}

/// Raising a dispute after the grace window from creation has elapsed
/// must return an error.
#[test]
fn test_dispute_outside_grace_window_returns_error() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);

    advance_time(&env, ONE_WEEK + 1);

    let result = client.try_raise_dispute(&employer, &id);
    assert!(result.is_err());
}

// ============================================================================
// 5. STATE PERSISTENCE
// ============================================================================

/// Verifies that the activation timestamp is correctly set and persisted.
#[test]
fn test_activation_timestamp_persisted() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);

    advance_time(&env, 1000);
    client.activate_agreement(&id);

    let a = client.get_agreement(&id).unwrap();
    assert_eq!(a.activated_at, Some(1000u64));

    let a2 = client.get_agreement(&id).unwrap();
    assert_eq!(a2.activated_at, Some(1000u64));
}

/// Verifies that the cancellation timestamp is correctly set and persisted.
#[test]
fn test_cancellation_timestamp_persisted() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);

    advance_time(&env, 5000);
    client.cancel_agreement(&id);

    let a = client.get_agreement(&id).unwrap();
    assert_eq!(a.cancelled_at, Some(5000u64));

    let a2 = client.get_agreement(&id).unwrap();
    assert_eq!(a2.cancelled_at, Some(5000u64));
}

/// Verifies that a pause/resume cycle preserves every agreement field
/// except status, which returns to Active.
#[test]
fn test_pause_resume_preserves_all_agreement_fields() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);

    let before = client.get_agreement(&id).unwrap();

    client.pause_agreement(&id);
    client.resume_agreement(&id);

    let after = client.get_agreement(&id).unwrap();

    assert_eq!(before.id, after.id);
    assert_eq!(before.employer, after.employer);
    assert_eq!(before.token, after.token);
    assert_eq!(before.mode, after.mode);
    assert_eq!(after.status, AgreementStatus::Active);
    assert_eq!(before.total_amount, after.total_amount);
    assert_eq!(before.paid_amount, after.paid_amount);
    assert_eq!(before.created_at, after.created_at);
    assert_eq!(before.activated_at, after.activated_at);
    assert_eq!(before.cancelled_at, after.cancelled_at);
    assert_eq!(before.grace_period_seconds, after.grace_period_seconds);
}

/// Verifies that the employee list is preserved across all state transitions.
#[test]
fn test_employee_list_preserved_across_transitions() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let e1 = create_address(&env);
    let e2 = create_address(&env);
    let e3 = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &e1, &SALARY);
    client.add_employee_to_agreement(&id, &e2, &SALARY);
    client.add_employee_to_agreement(&id, &e3, &SALARY);

    let in_created = client.get_agreement_employees(&id);
    assert_eq!(in_created.len(), 3);

    client.activate_agreement(&id);
    let in_active = client.get_agreement_employees(&id);
    assert_eq!(in_active.len(), 3);

    client.pause_agreement(&id);
    let in_paused = client.get_agreement_employees(&id);
    assert_eq!(in_paused.len(), 3);

    client.resume_agreement(&id);
    let in_resumed = client.get_agreement_employees(&id);
    assert_eq!(in_resumed.len(), 3);

    client.cancel_agreement(&id);
    let in_cancelled = client.get_agreement_employees(&id);
    assert_eq!(in_cancelled.len(), 3);
}

// ============================================================================
// 6. STATE RECOVERY AND CONSISTENCY
// ============================================================================

/// Verifies that a failed transition attempt does not alter agreement state.
/// Uses try_activate_agreement to catch the error without panicking.
#[test]
fn test_state_unchanged_after_failed_transition() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);

    let before = client.get_agreement(&id).unwrap();

    let result = client.try_activate_agreement(&id);
    assert!(result.is_err());

    let after = client.get_agreement(&id).unwrap();
    assert_eq!(before.status, after.status);
    assert_eq!(before.activated_at, after.activated_at);
    assert_eq!(before.total_amount, after.total_amount);
    assert_eq!(before.paid_amount, after.paid_amount);
}

/// Verifies that repeated pause/resume cycles do not corrupt state.
#[test]
fn test_multiple_pause_resume_cycles() {
    let env = create_test_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);

    for _ in 0..5 {
        client.pause_agreement(&id);
        assert_eq!(
            client.get_agreement(&id).unwrap().status,
            AgreementStatus::Paused
        );

        client.resume_agreement(&id);
        assert_eq!(
            client.get_agreement(&id).unwrap().status,
            AgreementStatus::Active
        );
    }

    let a = client.get_agreement(&id).unwrap();
    assert_eq!(a.status, AgreementStatus::Active);
    assert_eq!(a.total_amount, SALARY);
    assert_eq!(a.paid_amount, 0);
}

/// Exercises the full agreement lifecycle from creation through finalization:
/// Created -> Active -> Paused -> Active -> Cancelled -> finalized.
#[test]
fn test_full_lifecycle_created_to_finalized() {
    let env = create_test_env();
    let (cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_token(&env);
    let employee = create_address(&env);
    let grace = ONE_WEEK;

    // Created
    let id = client.create_payroll_agreement(&employer, &token, &grace);
    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Created
    );

    // Created -> Active
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);
    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Active
    );

    // Active -> Paused
    client.pause_agreement(&id);
    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Paused
    );

    // Paused -> Active
    client.resume_agreement(&id);
    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Active
    );

    // Fund escrow for finalization
    mint(&env, &token, &cid, SALARY);
    env.as_contract(&cid, || {
        DataKey::set_agreement_escrow_balance(&env, id, &token, SALARY);
    });

    // Active -> Cancelled
    client.cancel_agreement(&id);
    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Cancelled
    );
    assert!(client.is_grace_period_active(&id));

    // Wait for grace period to expire, then finalize
    advance_time(&env, grace + 1);
    assert!(!client.is_grace_period_active(&id));
    client.finalize_grace_period(&id);

    env.as_contract(&cid, || {
        let balance = DataKey::get_agreement_escrow_balance(&env, id, &token);
        assert_eq!(balance, 0);
    });

    // Status remains Cancelled; finalize handles refund only.
    assert_eq!(
        client.get_agreement(&id).unwrap().status,
        AgreementStatus::Cancelled
    );
}
