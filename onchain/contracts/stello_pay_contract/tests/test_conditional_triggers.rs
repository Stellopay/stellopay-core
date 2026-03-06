//! Comprehensive test suite for conditional payment triggers (#294).
//!
//! Validates that payments are released only when their specific trigger
//! condition is satisfied, and are correctly blocked when the condition is
//! not yet met.  All three trigger categories supported by the contract are
//! covered:
//!
//! 1. **Time-based triggers** — escrow `claim_time_based` and payroll
//!    `claim_payroll` honour the activation timestamp and period duration.
//! 2. **Milestone-based triggers** — `approve_milestone` / `claim_milestone`
//!    gate payment behind an explicit employer approval step.
//! 3. **Composite / cross-cutting triggers** — combinations of the above,
//!    plus emergency-pause, dispute, and grace-period conditions.
//!
//! # Coverage map
//!
//! | Section | Scenario |
//! |---------|----------|
//! | 1  | Time-based: claim after exactly one period |
//! | 2  | Time-based: claim blocked before first period elapses |
//! | 3  | Time-based: boundary — last second before period end |
//! | 4  | Time-based: boundary — first second after period end |
//! | 5  | Time-based: multiple periods accumulate correctly |
//! | 6  | Time-based: cannot over-claim beyond num_periods |
//! | 7  | Time-based: second sequential claim for next period |
//! | 8  | Time-based: claim blocked on Paused agreement |
//! | 9  | Time-based: claim blocked during emergency pause |
//! | 10 | Time-based: partial periods — only elapsed periods claimable |
//! | 11 | Time-based: large period duration boundary |
//! | 12 | Time-based: escrow completes after all periods claimed |
//! | 13 | Milestone: payment blocked until employer approves |
//! | 14 | Milestone: approval without prior claim does not pay |
//! | 15 | Milestone: claim immediately after approval succeeds |
//! | 16 | Milestone: second claim on same milestone rejected |
//! | 17 | Milestone: out-of-order approval and claim succeeds |
//! | 18 | Milestone: wrong caller cannot claim |
//! | 19 | Milestone: wrong caller cannot approve |
//! | 20 | Milestone: claim blocked when agreement paused |
//! | 21 | Milestone: batch claim only processes approved milestones |
//! | 22 | Milestone: batch claim skips duplicate IDs |
//! | 23 | Milestone: batch claim partial success returns correct counts |
//! | 24 | Milestone: claim of invalid milestone ID rejected |
//! | 25 | Payroll: claim for period 0 — no periods elapsed |
//! | 26 | Payroll: claim after one period credits correct amount |
//! | 27 | Payroll: batch payroll distributes to multiple employees |
//! | 28 | Payroll: wrong employee index rejected |
//! | 29 | Payroll: claim blocked if agreement not active |
//! | 30 | Payroll: claim blocked after grace period expired |
//! | 31 | Payroll: claim_payroll_in_token applies FX rate |
//! | 32 | Composite: paused then resumed — trigger fires after resume |
//! | 33 | Composite: emergency pause blocks all three trigger types |
//! | 34 | Composite: dispute blocks new payroll claims |
//! | 35 | Composite: grace-period window — claim succeeds, then expires |

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};
use stello_pay_contract::storage::{AgreementStatus, DataKey};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ============================================================================
// CONSTANTS
// ============================================================================

/// One second in seconds.
const ONE_SECOND: u64 = 1;
/// One day in seconds.
const ONE_DAY: u64 = 86_400;
/// One week in seconds.
const ONE_WEEK: u64 = 604_800;
/// Standard salary per period used across tests.
const STANDARD_SALARY: i128 = 1_000;

// ============================================================================
// HELPERS
// ============================================================================

/// Creates a Soroban test environment with all auths mocked.
fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Deploys and initialises the payroll contract; returns (contract_address, client).
fn setup_contract(env: &Env) -> (Address, PayrollContractClient<'static>) {
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (contract_id, client)
}

/// Registers a Stellar Asset Contract and returns its address.
fn create_token(env: &Env) -> Address {
    let admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

/// Mints `amount` of `token` to `to`.
fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

/// Returns the token balance of `address`.
fn balance(env: &Env, token: &Address, address: &Address) -> i128 {
    TokenClient::new(env, token).balance(address)
}

/// Advances ledger time by `seconds`.
fn advance_time(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| li.timestamp += seconds);
}

/// Sets ledger time to an absolute `timestamp`.
fn set_time(env: &Env, timestamp: u64) {
    env.ledger().with_mut(|li| li.timestamp = timestamp);
}

/// Creates an escrow agreement, activates it, writes all DataKey metadata
/// needed for `claim_time_based`, funds the escrow, and mints tokens to the
/// contract so that on-chain transfers succeed.
///
/// Returns the agreement ID.
fn setup_active_escrow(
    env: &Env,
    contract_id: &Address,
    client: &PayrollContractClient,
    employer: &Address,
    contributor: &Address,
    token: &Address,
    amount_per_period: i128,
    period_seconds: u64,
    num_periods: u32,
) -> u128 {
    let agreement_id = client.create_escrow_agreement(
        employer,
        contributor,
        token,
        &amount_per_period,
        &period_seconds,
        &(num_periods),
    );
    client.activate_agreement(&agreement_id);

    let now = env.ledger().timestamp();
    let total = amount_per_period * (num_periods as i128);

    // Fund the contract with real tokens.
    mint(env, token, contract_id, total);

    // Write DataKey storage expected by claim_time_based.
    env.as_contract(contract_id, || {
        DataKey::set_agreement_escrow_balance(env, agreement_id, token, total);
        DataKey::set_agreement_activation_time(env, agreement_id, now);
        DataKey::set_agreement_period_duration(env, agreement_id, period_seconds);
        DataKey::set_agreement_token(env, agreement_id, token);
    });

    agreement_id
}

/// Creates a payroll agreement with `employees`, activates it, writes DataKey
/// metadata, funds the escrow, and mints tokens to the contract.
///
/// `employees` is a slice of `(address, salary_per_period)`.
/// Returns the agreement ID.
fn setup_active_payroll(
    env: &Env,
    contract_id: &Address,
    client: &PayrollContractClient,
    employer: &Address,
    token: &Address,
    period_seconds: u64,
    employees: &[(Address, i128)],
) -> u128 {
    let agreement_id =
        client.create_payroll_agreement(employer, token, &ONE_WEEK);

    for (emp, salary) in employees.iter() {
        client.add_employee_to_agreement(&agreement_id, emp, salary);
    }
    client.activate_agreement(&agreement_id);

    let now = env.ledger().timestamp();
    let total_per_period: i128 = employees.iter().map(|(_, s)| s).sum();
    let escrow_total = total_per_period * 20; // ample buffer

    mint(env, token, contract_id, escrow_total);

    env.as_contract(contract_id, || {
        DataKey::set_agreement_activation_time(env, agreement_id, now);
        DataKey::set_agreement_period_duration(env, agreement_id, period_seconds);
        DataKey::set_agreement_token(env, agreement_id, token);
        DataKey::set_agreement_escrow_balance(env, agreement_id, token, escrow_total);

        for (index, (emp, salary)) in employees.iter().enumerate() {
            DataKey::set_employee(env, agreement_id, index as u32, emp);
            DataKey::set_employee_salary(env, agreement_id, index as u32, *salary);
            DataKey::set_employee_claimed_periods(env, agreement_id, index as u32, 0);
        }
        DataKey::set_employee_count(env, agreement_id, employees.len() as u32);
    });

    agreement_id
}

// ============================================================================
// SECTION 1 — TIME-BASED TRIGGERS
// ============================================================================

/// @notice Payment is released after exactly one period has elapsed.
/// @dev Verifies that claim_time_based transfers amount_per_period tokens
///      and increments claimed_periods from 0 to 1.
#[test]
fn test_time_based_claim_after_one_period() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    let agreement_id = setup_active_escrow(
        &env, &contract_id, &client, &employer, &contributor, &token,
        STANDARD_SALARY, ONE_DAY, 4,
    );

    advance_time(&env, ONE_DAY);
    client.claim_time_based(&agreement_id);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.claimed_periods, Some(1));
    assert_eq!(agreement.paid_amount, STANDARD_SALARY);
    assert_eq!(balance(&env, &token, &contributor), STANDARD_SALARY);
}

/// @notice Claim is blocked when fewer than one full period has elapsed.
/// @dev Calling claim_time_based before the first period elapses must fail
///      with NoPeriodsToClaim.
#[test]
fn test_time_based_claim_blocked_before_first_period() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    let agreement_id = setup_active_escrow(
        &env, &contract_id, &client, &employer, &contributor, &token,
        STANDARD_SALARY, ONE_DAY, 4,
    );

    // Advance only 23 hours — not yet a full day.
    advance_time(&env, ONE_DAY - ONE_SECOND);
    let result = client.try_claim_time_based(&agreement_id);
    assert!(result.is_err());
}

/// @notice Boundary: last second before the period ends — claim blocked.
/// @dev At exactly `activation_time + period_seconds - 1` the condition is
///      not satisfied.
#[test]
fn test_time_based_boundary_last_second_before_period_end() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    let start = 1_000_000u64;
    set_time(&env, start);

    let agreement_id = setup_active_escrow(
        &env, &contract_id, &client, &employer, &contributor, &token,
        STANDARD_SALARY, ONE_DAY, 4,
    );

    // One second before the period boundary.
    set_time(&env, start + ONE_DAY - ONE_SECOND);
    let result = client.try_claim_time_based(&agreement_id);
    assert!(result.is_err());
}

/// @notice Boundary: first second after period end — claim succeeds.
/// @dev At exactly `activation_time + period_seconds` one period is claimable.
#[test]
fn test_time_based_boundary_first_second_after_period_end() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    let start = 1_000_000u64;
    set_time(&env, start);

    let agreement_id = setup_active_escrow(
        &env, &contract_id, &client, &employer, &contributor, &token,
        STANDARD_SALARY, ONE_DAY, 4,
    );

    // Exactly at the period boundary.
    set_time(&env, start + ONE_DAY);
    client.claim_time_based(&agreement_id);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.claimed_periods, Some(1));
}

/// @notice Multiple elapsed periods are all claimable in a single call.
/// @dev After 3 periods elapse, claimed_periods advances from 0 to 3.
#[test]
fn test_time_based_multiple_periods_accumulate() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    let agreement_id = setup_active_escrow(
        &env, &contract_id, &client, &employer, &contributor, &token,
        STANDARD_SALARY, ONE_DAY, 5,
    );

    advance_time(&env, ONE_DAY * 3);
    client.claim_time_based(&agreement_id);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.claimed_periods, Some(3));
    assert_eq!(agreement.paid_amount, STANDARD_SALARY * 3);
    assert_eq!(balance(&env, &token, &contributor), STANDARD_SALARY * 3);
}

/// @notice Claim cannot exceed num_periods even if more time elapses.
/// @dev Advancing beyond the final period pays only the remaining unclaimed
///      periods, not more.
#[test]
fn test_time_based_cannot_over_claim_beyond_num_periods() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let num_periods = 3u32;

    let agreement_id = setup_active_escrow(
        &env, &contract_id, &client, &employer, &contributor, &token,
        STANDARD_SALARY, ONE_DAY, num_periods,
    );

    // Advance far past all periods.
    advance_time(&env, ONE_DAY * 10);
    client.claim_time_based(&agreement_id);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.claimed_periods, Some(num_periods));
    assert_eq!(agreement.paid_amount, STANDARD_SALARY * num_periods as i128);

    // Second claim must fail because all periods are exhausted.
    let result = client.try_claim_time_based(&agreement_id);
    assert!(result.is_err());
}

/// @notice Sequential claims accumulate: second claim covers only new periods.
/// @dev Claim period 1, then claim period 2; each call pays one salary.
#[test]
fn test_time_based_sequential_claims_correct_amounts() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    let agreement_id = setup_active_escrow(
        &env, &contract_id, &client, &employer, &contributor, &token,
        STANDARD_SALARY, ONE_DAY, 4,
    );

    // First period.
    advance_time(&env, ONE_DAY);
    client.claim_time_based(&agreement_id);
    assert_eq!(balance(&env, &token, &contributor), STANDARD_SALARY);

    // Second period.
    advance_time(&env, ONE_DAY);
    client.claim_time_based(&agreement_id);
    assert_eq!(balance(&env, &token, &contributor), STANDARD_SALARY * 2);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.claimed_periods, Some(2));
}

/// @notice Claim is blocked when the agreement is Paused.
/// @dev Pausing the agreement must cause claim_time_based to fail.
#[test]
fn test_time_based_claim_blocked_on_paused_agreement() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    let agreement_id = setup_active_escrow(
        &env, &contract_id, &client, &employer, &contributor, &token,
        STANDARD_SALARY, ONE_DAY, 4,
    );

    advance_time(&env, ONE_DAY);
    client.pause_agreement(&agreement_id);

    let result = client.try_claim_time_based(&agreement_id);
    assert!(result.is_err());
}

/// @notice Emergency pause blocks time-based claims.
/// @dev With the contract-level emergency pause active, claim_time_based
///      must return an error.
#[test]
fn test_time_based_claim_blocked_during_emergency_pause() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    let agreement_id = setup_active_escrow(
        &env, &contract_id, &client, &employer, &contributor, &token,
        STANDARD_SALARY, ONE_DAY, 4,
    );

    advance_time(&env, ONE_DAY);

    // Activate emergency pause.
    client.emergency_pause();
    assert!(client.is_emergency_paused());

    let result = client.try_claim_time_based(&agreement_id);
    assert!(result.is_err());
}

/// @notice Only elapsed whole periods are claimable.
/// @dev After 1.5 periods, only 1 period is claimable.
#[test]
fn test_time_based_partial_period_not_claimable() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    let agreement_id = setup_active_escrow(
        &env, &contract_id, &client, &employer, &contributor, &token,
        STANDARD_SALARY, ONE_DAY, 4,
    );

    // Advance 1.5 days — only 1 complete period.
    advance_time(&env, ONE_DAY + ONE_DAY / 2);
    client.claim_time_based(&agreement_id);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.claimed_periods, Some(1));
}

/// @notice Large period duration boundary: claim at exactly period end.
/// @dev Uses a one-week period; verifies payment at the exact boundary.
#[test]
fn test_time_based_large_period_boundary() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let period_seconds = ONE_WEEK;

    let start = 5_000_000u64;
    set_time(&env, start);

    let agreement_id = setup_active_escrow(
        &env, &contract_id, &client, &employer, &contributor, &token,
        STANDARD_SALARY, period_seconds, 2,
    );

    // One second before expiry — no claim.
    set_time(&env, start + period_seconds - ONE_SECOND);
    assert!(client.try_claim_time_based(&agreement_id).is_err());

    // Exactly at expiry — one period claimable.
    set_time(&env, start + period_seconds);
    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_agreement(&agreement_id).unwrap().claimed_periods, Some(1));
}

/// @notice Escrow transitions to Completed when all periods are claimed.
/// @dev After claiming the final period, agreement.status must be Completed.
#[test]
fn test_time_based_escrow_completes_after_all_periods() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let num_periods = 2u32;

    let agreement_id = setup_active_escrow(
        &env, &contract_id, &client, &employer, &contributor, &token,
        STANDARD_SALARY, ONE_DAY, num_periods,
    );

    advance_time(&env, ONE_DAY * num_periods as u64);
    client.claim_time_based(&agreement_id);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Completed);
    assert_eq!(agreement.claimed_periods, Some(num_periods));
}

// ============================================================================
// SECTION 2 — MILESTONE-BASED TRIGGERS
// ============================================================================

/// @notice Payment is blocked until the employer explicitly approves the milestone.
/// @dev A fresh milestone has approved=false; claiming before approval must fail.
#[test]
fn test_milestone_claim_blocked_before_approval() {
    let env = create_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &STANDARD_SALARY);

    let result = client.try_claim_milestone(&agreement_id, &1u32);
    assert!(result.is_err());
}

/// @notice Approving a milestone does not itself transfer funds.
/// @dev After approve_milestone, the milestone is approved but not claimed.
#[test]
fn test_milestone_approval_does_not_transfer_funds() {
    let env = create_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &STANDARD_SALARY);
    client.approve_milestone(&agreement_id, &1u32);

    let milestone = client.get_milestone(&agreement_id, &1u32).unwrap();
    assert!(milestone.approved);
    assert!(!milestone.claimed);
}

/// @notice Claim succeeds immediately after approval and marks the milestone claimed.
/// @dev The full lifecycle: add → approve → claim.
#[test]
fn test_milestone_claim_succeeds_immediately_after_approval() {
    let env = create_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &STANDARD_SALARY);
    client.approve_milestone(&agreement_id, &1u32);
    client.claim_milestone(&agreement_id, &1u32);

    let milestone = client.get_milestone(&agreement_id, &1u32).unwrap();
    assert!(milestone.approved);
    assert!(milestone.claimed);
}

/// @notice A claimed milestone cannot be claimed a second time.
/// @dev Double-claim must fail with "Milestone already claimed".
#[test]
#[should_panic(expected = "Milestone already claimed")]
fn test_milestone_double_claim_rejected() {
    let env = create_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &STANDARD_SALARY);
    client.approve_milestone(&agreement_id, &1u32);
    client.claim_milestone(&agreement_id, &1u32);
    client.claim_milestone(&agreement_id, &1u32); // must panic
}

/// @notice Milestones can be approved and claimed in any order.
/// @dev Approve milestone 3 then 1, claim 3 then 1; both must succeed.
#[test]
fn test_milestone_out_of_order_approval_and_claim() {
    let env = create_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100i128);
    client.add_milestone(&agreement_id, &200i128);
    client.add_milestone(&agreement_id, &300i128);

    // Approve and claim milestone 3 first.
    client.approve_milestone(&agreement_id, &3u32);
    client.claim_milestone(&agreement_id, &3u32);
    assert!(client.get_milestone(&agreement_id, &3u32).unwrap().claimed);

    // Approve and claim milestone 1 second.
    client.approve_milestone(&agreement_id, &1u32);
    client.claim_milestone(&agreement_id, &1u32);
    assert!(client.get_milestone(&agreement_id, &1u32).unwrap().claimed);

    // Milestone 2 is still unclaimed.
    assert!(!client.get_milestone(&agreement_id, &2u32).unwrap().claimed);
}

/// @notice Only the contributor (correct caller) can claim a milestone.
/// @dev Removing all mocked auths must cause the claim to fail auth.
#[test]
#[should_panic]
fn test_milestone_wrong_caller_cannot_claim() {
    let env = create_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &STANDARD_SALARY);
    client.approve_milestone(&agreement_id, &1u32);

    env.mock_auths(&[]); // strip all auth — claim must fail
    client.claim_milestone(&agreement_id, &1u32);
}

/// @notice Only the employer can approve a milestone.
/// @dev Removing all mocked auths must cause approve_milestone to fail auth.
#[test]
#[should_panic]
fn test_milestone_wrong_caller_cannot_approve() {
    let env = create_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &STANDARD_SALARY);

    env.mock_auths(&[]); // strip all auth — approve must fail
    client.approve_milestone(&agreement_id, &1u32);
}

/// @notice Milestone claim is blocked while the agreement is Paused.
/// @dev Pausing before claim_milestone triggers "Cannot claim when agreement is paused".
#[test]
#[should_panic(expected = "Cannot claim when agreement is paused")]
fn test_milestone_claim_blocked_when_paused() {
    let env = create_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &STANDARD_SALARY);
    client.approve_milestone(&agreement_id, &1u32);
    client.pause_agreement(&agreement_id);
    client.claim_milestone(&agreement_id, &1u32); // must panic
}

/// @notice Batch claim processes only milestones that are already approved.
/// @dev Milestones 1 and 3 are approved; 2 is not. Batch should succeed on
///      1 and 3, fail on 2.
#[test]
fn test_milestone_batch_claim_only_approved() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    // Fund the contract so token transfers succeed.
    mint(&env, &token, &contract_id, 10_000i128);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100i128); // id 1
    client.add_milestone(&agreement_id, &200i128); // id 2
    client.add_milestone(&agreement_id, &300i128); // id 3

    client.approve_milestone(&agreement_id, &1u32);
    client.approve_milestone(&agreement_id, &3u32);
    // id 2 intentionally left unapproved.

    let mut ids = Vec::new(&env);
    ids.push_back(1u32);
    ids.push_back(2u32);
    ids.push_back(3u32);
    let result = client.batch_claim_milestones(&agreement_id, &ids);

    assert_eq!(result.successful_claims, 2);
    assert_eq!(result.failed_claims, 1);
    assert_eq!(result.total_claimed, 400i128); // 100 + 300
    assert!(client.get_milestone(&agreement_id, &1u32).unwrap().claimed);
    assert!(!client.get_milestone(&agreement_id, &2u32).unwrap().claimed);
    assert!(client.get_milestone(&agreement_id, &3u32).unwrap().claimed);
}

/// @notice Batch claim deduplicates: submitting the same ID twice counts only once.
/// @dev Two entries of id=1; only one should succeed, the duplicate is skipped.
#[test]
fn test_milestone_batch_claim_skips_duplicates() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    // Fund the contract so the successful transfer can execute.
    mint(&env, &token, &contract_id, 10_000i128);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &STANDARD_SALARY);
    client.approve_milestone(&agreement_id, &1u32);

    let mut ids = Vec::new(&env);
    ids.push_back(1u32);
    ids.push_back(1u32); // duplicate
    let result = client.batch_claim_milestones(&agreement_id, &ids);

    assert_eq!(result.successful_claims, 1);
    assert_eq!(result.failed_claims, 1); // duplicate counted as failure
    assert_eq!(result.total_claimed, STANDARD_SALARY);
}

/// @notice Batch claim with mixed outcomes reports correct counts.
/// @dev Three milestones: one approved, one not approved, one invalid ID.
#[test]
fn test_milestone_batch_claim_partial_success_correct_counts() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    // Fund the contract so the successful transfer can execute.
    mint(&env, &token, &contract_id, 10_000i128);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &500i128); // id 1 — will be approved
    client.add_milestone(&agreement_id, &500i128); // id 2 — left unapproved
    client.approve_milestone(&agreement_id, &1u32);

    let mut ids = Vec::new(&env);
    ids.push_back(1u32); // success
    ids.push_back(2u32); // fail: not approved
    ids.push_back(99u32); // fail: invalid ID
    let result = client.batch_claim_milestones(&agreement_id, &ids);

    assert_eq!(result.successful_claims, 1);
    assert_eq!(result.failed_claims, 2);
}

/// @notice Claiming a milestone ID that does not exist panics.
/// @dev milestone_id=0 and IDs larger than count are both invalid.
#[test]
#[should_panic(expected = "Invalid milestone ID")]
fn test_milestone_invalid_id_rejected() {
    let env = create_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &STANDARD_SALARY);
    client.approve_milestone(&agreement_id, &99u32); // does not exist
}

// ============================================================================
// SECTION 3 — PAYROLL PERIOD TRIGGERS
// ============================================================================

/// @notice No periods claimable before first period elapses.
/// @dev claim_payroll called at time zero must fail with NoPeriodsToClaim.
#[test]
fn test_payroll_claim_blocked_before_first_period() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = create_token(&env);

    let _agreement_id = setup_active_payroll(
        &env, &contract_id, &client, &employer, &token,
        ONE_DAY, &[(employee.clone(), STANDARD_SALARY)],
    );

    // No time advance — zero periods elapsed.
    let result = client.try_claim_payroll(&employee, &_agreement_id, &0u32);
    assert!(result.is_err());
}

/// @notice After one period, claim_payroll transfers exactly one salary.
/// @dev Employee balance moves from 0 to STANDARD_SALARY.
#[test]
fn test_payroll_claim_after_one_period_correct_amount() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = create_token(&env);

    let agreement_id = setup_active_payroll(
        &env, &contract_id, &client, &employer, &token,
        ONE_DAY, &[(employee.clone(), STANDARD_SALARY)],
    );

    advance_time(&env, ONE_DAY);
    client.claim_payroll(&employee, &agreement_id, &0u32);

    assert_eq!(balance(&env, &token, &employee), STANDARD_SALARY);
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0u32), 1);
}

/// @notice Batch payroll distributes salaries to multiple employees at once.
/// @dev Three employees each receive one period's salary after one period elapses.
#[test]
fn test_payroll_batch_distributes_to_multiple_employees() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let e0 = Address::generate(&env);
    let e1 = Address::generate(&env);
    let e2 = Address::generate(&env);
    let token = create_token(&env);

    let agreement_id = setup_active_payroll(
        &env, &contract_id, &client, &employer, &token,
        ONE_DAY,
        &[
            (e0.clone(), 1_000i128),
            (e1.clone(), 2_000i128),
            (e2.clone(), 3_000i128),
        ],
    );

    advance_time(&env, ONE_DAY);

    // batch_claim_payroll enforces caller == employee at each index,
    // so each employee claims their own index individually.
    let mut idx0 = Vec::new(&env);
    idx0.push_back(0u32);
    let b0 = client.batch_claim_payroll(&e0, &agreement_id, &idx0);

    let mut idx1 = Vec::new(&env);
    idx1.push_back(1u32);
    let b1 = client.batch_claim_payroll(&e1, &agreement_id, &idx1);

    let mut idx2 = Vec::new(&env);
    idx2.push_back(2u32);
    let b2 = client.batch_claim_payroll(&e2, &agreement_id, &idx2);

    assert_eq!(b0.successful_claims, 1);
    assert_eq!(b1.successful_claims, 1);
    assert_eq!(b2.successful_claims, 1);
    assert_eq!(b0.total_claimed + b1.total_claimed + b2.total_claimed, 6_000i128);
    assert_eq!(balance(&env, &token, &e0), 1_000i128);
    assert_eq!(balance(&env, &token, &e1), 2_000i128);
    assert_eq!(balance(&env, &token, &e2), 3_000i128);
}

/// @notice An out-of-range employee index is rejected.
/// @dev Requesting employee index 99 on a one-employee agreement must error.
#[test]
fn test_payroll_wrong_employee_index_rejected() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = create_token(&env);

    let agreement_id = setup_active_payroll(
        &env, &contract_id, &client, &employer, &token,
        ONE_DAY, &[(employee.clone(), STANDARD_SALARY)],
    );

    advance_time(&env, ONE_DAY);
    let result = client.try_claim_payroll(&employee, &agreement_id, &99u32);
    assert!(result.is_err());
}

/// @notice Claim is blocked when the agreement has not been activated.
/// @dev A Created agreement must reject payroll claims.
#[test]
fn test_payroll_claim_blocked_if_agreement_not_active() {
    let env = create_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&agreement_id, &employee, &STANDARD_SALARY);
    // Intentionally not activated.

    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert!(result.is_err());
}

/// @notice Claim is blocked after the grace period has expired post-cancellation.
/// @dev Cancel, advance past grace, then claim must fail.
#[test]
fn test_payroll_claim_blocked_after_grace_period_expired() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = create_token(&env);
    let grace_period = ONE_DAY;

    // Use a payroll agreement with a known grace period.
    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace_period);
    client.add_employee_to_agreement(&agreement_id, &employee, &STANDARD_SALARY);
    client.activate_agreement(&agreement_id);

    let now = env.ledger().timestamp();
    let escrow_total = STANDARD_SALARY * 10;
    mint(&env, &token, &contract_id, escrow_total);

    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, now);
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, escrow_total);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, STANDARD_SALARY);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee_count(&env, agreement_id, 1);
    });

    // Advance one period, then cancel.
    advance_time(&env, ONE_DAY);
    client.cancel_agreement(&agreement_id);

    // Advance past the grace period.
    advance_time(&env, grace_period + ONE_SECOND);
    assert!(!client.is_grace_period_active(&agreement_id));

    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert!(result.is_err());
}

/// @notice claim_payroll_in_token applies the configured FX rate.
/// @dev FX rate 2:1 (base→payout) means 1_000 base units yield 2_000 payout tokens.
#[test]
fn test_payroll_claim_in_token_applies_fx_rate() {
    let env = create_env();
    // Deploy a fresh contract and capture the owner so we can call set_exchange_rate.
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let base_token = create_token(&env);
    let payout_token = create_token(&env);

    // FX: 1 base = 2 payout (rate * FX_SCALE = 2_000_000).
    client.set_exchange_rate(&owner, &base_token, &payout_token, &2_000_000i128);

    let salary: i128 = 1_000;
    let period_seconds = ONE_DAY;

    let agreement_id = client.create_payroll_agreement(&employer, &base_token, &ONE_WEEK);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary);
    client.activate_agreement(&agreement_id);

    let now = env.ledger().timestamp();
    let escrow_payout: i128 = 20_000;
    mint(&env, &payout_token, &contract_id, escrow_payout);

    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, now);
        DataKey::set_agreement_period_duration(&env, agreement_id, period_seconds);
        DataKey::set_agreement_token(&env, agreement_id, &base_token);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &payout_token, escrow_payout);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee_count(&env, agreement_id, 1);
    });

    advance_time(&env, period_seconds);
    client.claim_payroll_in_token(&employee, &agreement_id, &0u32, &payout_token);

    // Employee receives 2_000 payout tokens (1_000 base × FX rate 2).
    assert_eq!(balance(&env, &payout_token, &employee), salary * 2);
}

// ============================================================================
// SECTION 4 — COMPOSITE / CROSS-CUTTING TRIGGERS
// ============================================================================

/// @notice A paused-then-resumed agreement allows time-based claims after resume.
/// @dev Pause blocks claim at t=period; resume then allows the claim.
#[test]
fn test_composite_pause_resume_time_trigger_fires_after_resume() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    let agreement_id = setup_active_escrow(
        &env, &contract_id, &client, &employer, &contributor, &token,
        STANDARD_SALARY, ONE_DAY, 4,
    );

    advance_time(&env, ONE_DAY);
    client.pause_agreement(&agreement_id);

    // Claim must be blocked while paused.
    assert!(client.try_claim_time_based(&agreement_id).is_err());

    // Resume and retry — must succeed.
    client.resume_agreement(&agreement_id);
    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_agreement(&agreement_id).unwrap().claimed_periods, Some(1));
}

/// @notice Emergency pause blocks time-based, milestone, and payroll triggers.
/// @dev After emergency_pause all three claim paths must return an error.
#[test]
fn test_composite_emergency_pause_blocks_all_trigger_types() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = create_token(&env);

    // Set up one agreement of each type.
    let escrow_id = setup_active_escrow(
        &env, &contract_id, &client, &employer, &contributor, &token,
        STANDARD_SALARY, ONE_DAY, 4,
    );

    let payroll_id = setup_active_payroll(
        &env, &contract_id, &client, &employer, &token,
        ONE_DAY, &[(employee.clone(), STANDARD_SALARY)],
    );

    let ms_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&ms_id, &STANDARD_SALARY);
    client.approve_milestone(&ms_id, &1u32);

    // Advance past one period.
    advance_time(&env, ONE_DAY);

    // Activate emergency pause.
    client.emergency_pause();

    // All three must fail.
    assert!(client.try_claim_time_based(&escrow_id).is_err());
    assert!(client.try_claim_payroll(&employee, &payroll_id, &0u32).is_err());
    assert!(client.try_claim_milestone(&ms_id, &1u32).is_err());
}

/// @notice An active dispute changes the agreement status and blocks new payroll claims.
/// @dev After raise_dispute the agreement is Disputed; subsequent payroll claims fail.
#[test]
fn test_composite_dispute_blocks_payroll_claims() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = create_token(&env);

    let agreement_id = setup_active_payroll(
        &env, &contract_id, &client, &employer, &token,
        ONE_DAY, &[(employee.clone(), STANDARD_SALARY)],
    );

    advance_time(&env, ONE_DAY);

    // Employer raises dispute.
    client.raise_dispute(&employer, &agreement_id);
    assert_eq!(
        client.get_agreement(&agreement_id).unwrap().status,
        AgreementStatus::Disputed
    );

    // Payroll claim must now fail.
    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert!(result.is_err());
}

/// @notice Claims succeed within the grace period and fail once it expires.
/// @dev After cancellation, employee claims one period; after grace expiry the
///      same call is rejected.
#[test]
fn test_composite_grace_period_claim_succeeds_then_expires() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = create_token(&env);
    let grace_period = ONE_DAY;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace_period);
    client.add_employee_to_agreement(&agreement_id, &employee, &STANDARD_SALARY);
    client.activate_agreement(&agreement_id);

    let now = env.ledger().timestamp();
    let escrow_total = STANDARD_SALARY * 10;
    mint(&env, &token, &contract_id, escrow_total);

    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, now);
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, escrow_total);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, STANDARD_SALARY);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee_count(&env, agreement_id, 1);
    });

    // Advance one period, then cancel.
    advance_time(&env, ONE_DAY);
    client.cancel_agreement(&agreement_id);
    assert!(client.is_grace_period_active(&agreement_id));

    // Claim within grace period must succeed.
    client.claim_payroll(&employee, &agreement_id, &0u32);
    assert_eq!(balance(&env, &token, &employee), STANDARD_SALARY);

    // Advance past grace period.
    advance_time(&env, grace_period + ONE_SECOND);
    assert!(!client.is_grace_period_active(&agreement_id));

    // A second claim (for the same period) must fail — both because all
    // claimable periods are exhausted and because the grace window closed.
    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert!(result.is_err());
}
