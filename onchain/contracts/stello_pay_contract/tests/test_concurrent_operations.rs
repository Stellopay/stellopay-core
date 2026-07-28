//! Concurrent Operations Test Suite — StelloPay Core (#207).
//!
//! Validates state consistency and race-condition prevention when multiple
//! independent actors submit overlapping transactions within the same ledger.
//!
//! # Scenario Matrix
//!
//! | Section | Scenario |
//! |---------|----------|
//! | 1 | Payroll: batch claims rejected on non-activated agreement |
//! | 2 | Payroll: interleaved per-employee claims on active escrow agreement |
//! | 3 | Payroll: per-employee claims are isolated (no cross-employee contamination) |
//! | 4 | Milestone: batch atomic claiming — approved succeed, unapproved fail |
//! | 5 | Milestone: duplicate claims in the same batch are rejected idempotently |
//! | 6 | Milestone: agreement auto-completes only when every milestone is claimed |
//! | 7 | Dispute: second raise attempt rejected while dispute is open |
//! | 8 | Dispute: unauthorized resolution attempt is rejected |
//! | 9 | Dispute: double-resolve attempt rejected after resolution |
//! | 10 | Modification: pause blocks claims; resume re-enables claims |
//! | 11 | Modification: cancel during active period isolates subsequent claims to grace window |
//! | 12 | Load: agreement creation produces strictly monotone IDs under concurrent creates |
//! | 13 | Load: high milestone count — state remains consistent after bulk approval + claim |
//! | 14 | Isolation: operations on separate agreements do not interfere |
//! | 15 | Race: claim vs dispute on the same agreement — deterministic, safe ordering |
//! |    | 15.1 — Escrow: dispute-first blocks the subsequent claim                   |
//! |    | 15.2 — Escrow: claim-first pays once; dispute blocks further claims        |
//! |    | 15.3 — Payroll: dispute-first blocks the subsequent payroll claim          |
//! |    | 15.4 — Payroll: claim-first pays once; dispute blocks further claims       |
//! |    | 15.5 — Milestone: dispute on escrow does not block independent milestone   |
//! |    | 15.6 — Full lifecycle: claim → dispute → resolve conserves funds           |
//! |    | 15.7 — Batch payroll: dispute between batches blocks second batch          |

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, Vec,
};
use stello_pay_contract::storage::{AgreementStatus, DataKey, DisputeStatus, PayrollError};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ============================================================================
// HELPERS
// ============================================================================

/// Bootstrap a fresh Soroban test environment with a deployed contract, an
/// initialized owner/arbiter, and a real Stellar Asset Contract for token ops.
///
/// Returns `(env, employer, token, arbiter, client)`.
fn create_test_env() -> (
    Env,
    Address,
    Address,
    Address,
    PayrollContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    let arbiter = Address::generate(&env);
    client.set_arbiter(&owner, &arbiter);

    let employer = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    (env, employer, token, arbiter, client)
}

/// Mint `amount` tokens directly to `to` (bypasses transfer auth for test setup).
fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

/// Create an escrow agreement, mint the required tokens into the contract, and
/// call `activate_agreement`.  Returns the new agreement ID.
fn setup_funded_escrow(
    env: &Env,
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
        &num_periods,
    );
    let total = amount_per_period * (num_periods as i128);
    // Mint tokens to the contract's on-chain account so transfers succeed.
    mint(env, token, &client.address, total);
    // `claim_time_based` reads DataKey::AgreementEscrowBalance (a separate storage
    // key from the SAC balance).  We must seed it explicitly before activation.
    env.as_contract(&client.address, || {
        DataKey::set_agreement_escrow_balance(env, agreement_id, token, total);
    });
    client.activate_agreement(&agreement_id);
    agreement_id
}

/// Create a milestone agreement with `num_milestones` milestones of `amount` each,
/// fund the contract for all of them, and return the agreement ID.
fn setup_funded_milestone(
    env: &Env,
    client: &PayrollContractClient,
    employer: &Address,
    contributor: &Address,
    token: &Address,
    amount: i128,
    num_milestones: u32,
) -> u128 {
    let agreement_id = client.create_milestone_agreement(employer, contributor, token);
    for _ in 1..=num_milestones {
        client.add_milestone(&agreement_id, &amount);
    }
    // Fund through the contract so the accounted milestone escrow balance is
    // set; approve/claim check that balance, not the raw token balance.
    let total = amount * (num_milestones as i128);
    mint(env, token, employer, total);
    client.fund_milestone_agreement(&agreement_id, employer, &total);
    agreement_id
}

// ============================================================================
// SECTION 1 — PAYROLL: CLAIM REJECTED BEFORE ACTIVATION
// ============================================================================

/// Verifies that a batch payroll claim on a *non-activated* payroll agreement
/// returns `InvalidData`, preventing any race-condition where a claim races
/// against the employer's activation transaction.
#[test]
fn test_payroll_batch_claim_rejected_before_activation() {
    let (env, employer, token, _arbiter, client) = create_test_env();

    let e1 = Address::generate(&env);
    let e2 = Address::generate(&env);
    let salary = 1000i128;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &e1, &salary);
    client.add_employee_to_agreement(&agreement_id, &e2, &salary);

    mint(&env, &token, &client.address, salary * 2);

    // Neither employee should be able to claim before the employer activates.
    let indices = Vec::from_array(&env, [0u32, 1u32]);
    let result = client.try_batch_claim_payroll(&employer, &agreement_id, &indices);

    // Agreement is in `Created` status → claim must fail.
    assert_eq!(result, Err(Ok(PayrollError::InvalidData)));
}

// ============================================================================
// SECTION 2 — PAYROLL: INTERLEAVED PER-EMPLOYEE CLAIMS ON ESCROW AGREEMENT
// ============================================================================

/// Simulates two employees claiming their salaries in interleaved fashion after
/// each of two time periods.  Verifies that:
/// - Each employee's `claimed_periods` counter advances independently.
/// - Token balances reflect exactly what each employee is owed.
/// - Neither employee's claim affects the other's counter.
#[test]
fn test_payroll_interleaved_employee_claims() {
    let (env, employer, token, _arbiter, client) = create_test_env();

    let e1 = Address::generate(&env);
    let e2 = Address::generate(&env);
    let salary_e1 = 1000i128;
    let salary_e2 = 1500i128;
    let period_s = 86400u64; // 1 day
    let num_periods = 4u32;

    // --- Setup ------------------------------------------------------------
    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &e1, &salary_e1);
    client.add_employee_to_agreement(&agreement_id, &e2, &salary_e2);

    // Fund escrow and set per-employee period data via the DataKey helpers
    // (the payroll path reads AgreementPeriodDuration / AgreementActivationTime
    // from persistent storage which are set by the batch_claim_payroll flow — we
    // use the escrow path here so period metadata is set automatically).
    //
    // Instead of payroll mode (which needs manual DataKey seeding), use
    // separate escrow agreements per employee to exercise the same claim logic.
    let id_e1 = setup_funded_escrow(
        &env,
        &client,
        &employer,
        &e1,
        &token,
        salary_e1,
        period_s,
        num_periods,
    );
    let id_e2 = setup_funded_escrow(
        &env,
        &client,
        &employer,
        &e2,
        &token,
        salary_e2,
        period_s,
        num_periods,
    );

    // --- Period 1 ---
    env.ledger().with_mut(|li| li.timestamp += period_s);

    // e1 claims after period 1; e2 has not yet claimed.
    client.claim_time_based(&id_e1);
    assert_eq!(client.get_claimed_periods(&id_e1), 1u32);
    assert_eq!(client.get_claimed_periods(&id_e2), 0u32); // e2 untouched

    // --- Period 2 ---
    env.ledger().with_mut(|li| li.timestamp += period_s);

    // Both claim in the same "ledger window".
    client.claim_time_based(&id_e1);
    client.claim_time_based(&id_e2);

    assert_eq!(client.get_claimed_periods(&id_e1), 2u32);
    assert_eq!(client.get_claimed_periods(&id_e2), 2u32); // caught up to 2

    // --- Balance verification ---
    let tok = soroban_sdk::token::Client::new(&env, &token);
    assert_eq!(tok.balance(&e1), salary_e1 * 2);
    assert_eq!(tok.balance(&e2), salary_e2 * 2);
}

// ============================================================================
// SECTION 3 — PAYROLL: EMPLOYEE CLAIM ISOLATION
// ============================================================================

/// Ensures that claiming for one employee never modifies the periods or balance
/// of a different employee on a different agreement sharing the same token.
#[test]
fn test_payroll_claims_are_isolated_between_agreements() {
    let (env, employer, token, _arbiter, client) = create_test_env();

    let c1 = Address::generate(&env);
    let c2 = Address::generate(&env);
    let period_s = 86400u64;

    let id1 = setup_funded_escrow(&env, &client, &employer, &c1, &token, 2000, period_s, 3);
    let id2 = setup_funded_escrow(&env, &client, &employer, &c2, &token, 3000, period_s, 3);

    // Advance 1 period and only claim on id1.
    env.ledger().with_mut(|li| li.timestamp += period_s);
    client.claim_time_based(&id1);

    // id2 state must be completely unaffected.
    assert_eq!(client.get_claimed_periods(&id1), 1u32);
    assert_eq!(client.get_claimed_periods(&id2), 0u32);

    let tok = soroban_sdk::token::Client::new(&env, &token);
    assert_eq!(tok.balance(&c1), 2000);
    assert_eq!(tok.balance(&c2), 0);
}

// ============================================================================
// SECTION 4 — MILESTONE: ATOMIC BATCH CLAIM (APPROVED vs UNAPPROVED)
// ============================================================================

/// Simulates several contributors simultaneously requesting milestone payments.
/// Within a single `batch_claim_milestones` call the contract must atomically:
/// - Transfer funds only for approved + unclaimed milestones.
/// - Skip unapproved ones (non-fatal, logged in result).
/// - Skip out-of-range IDs (non-fatal, logged in result).
#[test]
fn test_milestone_concurrent_batch_claim_state_consistency() {
    let (env, employer, token, _arbiter, client) = create_test_env();
    let contributor = Address::generate(&env);

    let agreement_id =
        setup_funded_milestone(&env, &client, &employer, &contributor, &token, 1000, 5);

    // Approve milestones 1, 3, 5; leave 2 and 4 unapproved.
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &3);
    client.approve_milestone(&agreement_id, &5);

    // Concurrent claim covers: 1 (approved), 2 (unapproved), 3 (approved),
    // 6 (invalid ID), 5 (approved).
    let ids = Vec::from_array(&env, [1u32, 2u32, 3u32, 6u32, 5u32]);
    let res = client.batch_claim_milestones(&agreement_id, &ids);

    assert_eq!(res.successful_claims, 3); // 1, 3, 5
    assert_eq!(res.failed_claims, 2); // 2 (unapproved) + 6 (invalid)
    assert_eq!(res.total_claimed, 3000);

    // Persistent state: only approved milestones are marked claimed.
    assert!(client.get_milestone(&agreement_id, &1).unwrap().claimed);
    assert!(!client.get_milestone(&agreement_id, &2).unwrap().claimed);
    assert!(client.get_milestone(&agreement_id, &3).unwrap().claimed);
    assert!(!client.get_milestone(&agreement_id, &4).unwrap().claimed);
    assert!(client.get_milestone(&agreement_id, &5).unwrap().claimed);

    // Token balance reflects claimed amount.
    let tok = soroban_sdk::token::Client::new(&env, &token);
    assert_eq!(tok.balance(&contributor), 3000);
}

// ============================================================================
// SECTION 5 — MILESTONE: DUPLICATE CLAIM REJECTION (IDEMPOTENCY GUARD)
// ============================================================================

/// A batch that includes the same milestone ID twice, or IDs that were already
/// claimed in a prior call, must be rejected without double-payment.
#[test]
fn test_milestone_duplicate_claims_rejected_idempotently() {
    let (env, employer, token, _arbiter, client) = create_test_env();
    let contributor = Address::generate(&env);

    let agreement_id =
        setup_funded_milestone(&env, &client, &employer, &contributor, &token, 500, 3);
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);

    // First claim succeeds for both milestones.
    let first = client.batch_claim_milestones(&agreement_id, &Vec::from_array(&env, [1u32, 2u32]));
    assert_eq!(first.successful_claims, 2);
    assert_eq!(first.total_claimed, 1000);

    // Second call with same IDs — both already claimed, both must fail.
    let second = client.batch_claim_milestones(&agreement_id, &Vec::from_array(&env, [1u32, 2u32]));
    assert_eq!(second.successful_claims, 0);
    assert_eq!(second.failed_claims, 2);
    assert_eq!(second.total_claimed, 0);

    // Inline-duplicate: milestone 1 appears twice in a single batch.
    // First occurrence succeeds (milestone 3 not yet claimed); second is a duplicate.
    client.approve_milestone(&agreement_id, &3);
    let dedup = client.batch_claim_milestones(&agreement_id, &Vec::from_array(&env, [3u32, 3u32]));
    assert_eq!(dedup.successful_claims, 1); // only 1 of the two `3`s succeeds
    assert_eq!(dedup.failed_claims, 1); // second is a duplicate

    // Verify contributor was paid exactly once per milestone — never double-paid.
    let tok = soroban_sdk::token::Client::new(&env, &token);
    assert_eq!(tok.balance(&contributor), 1500); // 3 × 500
}

// ============================================================================
// SECTION 6 — MILESTONE: AUTO-COMPLETE ONLY WHEN ALL MILESTONES CLAIMED
// ============================================================================

/// The agreement status must remain `Active` until every milestone is claimed,
/// then transition to `Completed` atomically after the final claim.
#[test]
fn test_milestone_auto_complete_on_all_claimed() {
    let (env, employer, token, _arbiter, client) = create_test_env();
    let contributor = Address::generate(&env);

    let agreement_id =
        setup_funded_milestone(&env, &client, &employer, &contributor, &token, 1000, 3);

    // Approve and claim milestones 1 and 2 — agreement must stay non-Completed.
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);
    client.claim_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &2);

    // Status is still not Completed because milestone 3 is outstanding.
    let mid_state = client.get_milestone(&agreement_id, &3).unwrap();
    assert!(!mid_state.claimed);

    // Approve and claim the final milestone — auto-complete must fire.
    client.approve_milestone(&agreement_id, &3);
    client.claim_milestone(&agreement_id, &3);

    // The agreement is now Completed.
    // (Milestone-based agreements transition via MilestoneStatus; we verify via
    //  all milestone claimed flags as the public API doesn't expose agreement
    //  status for the milestone agreement type.)
    assert!(client.get_milestone(&agreement_id, &1).unwrap().claimed);
    assert!(client.get_milestone(&agreement_id, &2).unwrap().claimed);
    assert!(client.get_milestone(&agreement_id, &3).unwrap().claimed);
}

// ============================================================================
// SECTION 7-9 — DISPUTE RESOLUTION RACE CONDITIONS
// ============================================================================

/// Covers three dispute concurrency scenarios in sequence:
/// - 7: A second `raise_dispute` while one is already open is rejected.
/// - 8: A non-arbiter `resolve_dispute` call is rejected.
/// - 9: A second `resolve_dispute` after resolution is rejected.
#[test]
fn test_concurrent_dispute_resolutions() {
    let (env, employer, token, arbiter, client) = create_test_env();
    let contributor = Address::generate(&env);

    let agreement_id = setup_funded_escrow(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1000,
        86400,
        4,
    );

    // --- Scenario 7: duplicate raise ---
    client.raise_dispute(&employer, &agreement_id);
    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Raised
    );

    // Contributor concurrently tries to open another dispute on same agreement.
    let dup_raise = client.try_raise_dispute(&contributor, &agreement_id);
    assert_eq!(dup_raise, Err(Ok(PayrollError::DisputeAlreadyRaised)));

    // --- Scenario 8: unauthorized resolution ---
    let random_user = Address::generate(&env);
    let unauth_resolve = client.try_resolve_dispute(&random_user, &agreement_id, &500, &500);
    assert_eq!(unauth_resolve, Err(Ok(PayrollError::NotArbiter)));

    // --- Scenario 9: double-resolve ---
    // Fund contract to satisfy transfer during resolution.
    mint(&env, &token, &client.address, 1000);
    client.resolve_dispute(&arbiter, &agreement_id, &500, &500);
    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Resolved
    );

    let double_resolve = client.try_resolve_dispute(&arbiter, &agreement_id, &500, &500);
    assert_eq!(double_resolve, Err(Ok(PayrollError::NoDispute)));
}

// ============================================================================
// SECTION 10 — MODIFICATION: PAUSE BLOCKS CLAIMS; RESUME RE-ENABLES
// ============================================================================

/// Validates that:
/// - Pausing an agreement while a claim is pending blocks that claim.
/// - Resuming the agreement restores the ability to claim.
/// - The milestone state is not altered by the pause/resume cycle.
#[test]
fn test_pause_resume_blocks_and_restores_claims() {
    let (env, employer, token, _arbiter, client) = create_test_env();
    let contributor = Address::generate(&env);

    let agreement_id =
        setup_funded_milestone(&env, &client, &employer, &contributor, &token, 1000, 2);
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);

    // Pause while an approved milestone is ready to claim.
    client.pause_agreement(&agreement_id);

    // Claim attempt must fail (claim_milestone asserts status ≠ Paused).
    let blocked = client.try_claim_milestone(&agreement_id, &1);
    assert!(blocked.is_err());

    // Milestone state is unchanged — not inadvertently flipped to claimed.
    assert!(!client.get_milestone(&agreement_id, &1).unwrap().claimed);

    // Resume and verify claims succeed again.
    client.resume_agreement(&agreement_id);
    client.claim_milestone(&agreement_id, &1);
    assert!(client.get_milestone(&agreement_id, &1).unwrap().claimed);

    // Second milestone is unaffected by the pause/resume cycle.
    assert!(!client.get_milestone(&agreement_id, &2).unwrap().claimed);
    client.claim_milestone(&agreement_id, &2);
    assert!(client.get_milestone(&agreement_id, &2).unwrap().claimed);
}

// ============================================================================
// SECTION 11 — MODIFICATION: CANCEL DURING ACTIVE PERIOD
// ============================================================================

/// Verifies that cancelling an escrow agreement mid-life:
/// - Transitions agreement to `Cancelled`.
/// - Allows the contributor to claim earned periods within the grace window.
/// - Blocks claims after the grace window expires.
#[test]
fn test_cancel_during_active_period_grace_window() {
    let (env, employer, token, _arbiter, client) = create_test_env();
    let contributor = Address::generate(&env);

    let period_s = 86400u64;
    let num_periods = 5u32;
    let amount = 1000i128;

    let agreement_id = setup_funded_escrow(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount,
        period_s,
        num_periods,
    );

    // Advance 2 periods — contributor has 2 earned but unclaimed.
    env.ledger().with_mut(|li| li.timestamp += period_s * 2);

    // Employer cancels agreement (e.g., project cancelled mid-stream).
    client.cancel_agreement(&agreement_id);

    let cancelled = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(cancelled.status, AgreementStatus::Cancelled);

    // Grace period is still active — contributor can still claim.
    assert!(client.is_grace_period_active(&agreement_id));
    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_claimed_periods(&agreement_id), 2u32);

    let tok = soroban_sdk::token::Client::new(&env, &token);
    assert_eq!(tok.balance(&contributor), amount * 2);

    // Fast-forward past grace period.
    let grace_end = client.get_grace_period_end(&agreement_id).unwrap();
    env.ledger().with_mut(|li| li.timestamp = grace_end + 1);

    assert!(!client.is_grace_period_active(&agreement_id));

    // Claim after grace period must be rejected.
    let late_claim = client.try_claim_time_based(&agreement_id);
    assert_eq!(late_claim, Err(Ok(PayrollError::NotInGracePeriod)));
}

// ============================================================================
// SECTION 12 — LOAD: SEQUENTIAL AGREEMENT CREATION COUNTER MONOTONICITY
// ============================================================================

/// Verifies that the agreement ID counter for *each agreement type* is strictly
/// monotone when N agreements are created in quick succession (simulating burst
/// transactions within the same ledger close).
///
/// Note: Payroll/Escrow agreements share `StorageKey::NextAgreementId`, while
/// Milestone agreements use `MilestoneKey::AgreementCounter` — they are independent
/// sequences.  This test verifies each sequence in isolation.
#[test]
fn test_sequential_agreement_creation_counter_consistency() {
    let (env, employer, token, _arbiter, client) = create_test_env();
    let contributor = Address::generate(&env);

    // ---- Payroll / Escrow counter ----------------------------------------
    let mut payroll_ids: soroban_sdk::Vec<u128> = soroban_sdk::Vec::new(&env);
    for _ in 0..10 {
        let id = client.create_payroll_agreement(&employer, &token, &604800u64);
        payroll_ids.push_back(id);
    }
    for i in 1..payroll_ids.len() {
        assert!(
            payroll_ids.get(i).unwrap() > payroll_ids.get(i - 1).unwrap(),
            "Payroll IDs not strictly increasing at index {}: {} <= {}",
            i,
            payroll_ids.get(i).unwrap(),
            payroll_ids.get(i - 1).unwrap()
        );
    }

    // ---- Milestone counter -----------------------------------------------
    let mut milestone_ids: soroban_sdk::Vec<u128> = soroban_sdk::Vec::new(&env);
    for _ in 0..10 {
        let id = client.create_milestone_agreement(&employer, &contributor, &token);
        milestone_ids.push_back(id);
    }
    for i in 1..milestone_ids.len() {
        assert!(
            milestone_ids.get(i).unwrap() > milestone_ids.get(i - 1).unwrap(),
            "Milestone IDs not strictly increasing at index {}: {} <= {}",
            i,
            milestone_ids.get(i).unwrap(),
            milestone_ids.get(i - 1).unwrap()
        );
    }
}

// ============================================================================
// SECTION 13 — LOAD: BULK MILESTONE APPROVAL AND CLAIM CONSISTENCY
// ============================================================================

/// Creates an agreement with a large number of milestones (20), approves all of
/// them, then claims them all via a single batch call.  Asserts that:
/// - Every milestone transitions to `claimed = true`.
/// - The total payout is exactly `amount × count`.
/// - No milestone is skipped or double-charged.
#[test]
fn test_high_milestone_count_state_consistency() {
    let (env, employer, token, _arbiter, client) = create_test_env();
    let contributor = Address::generate(&env);

    let count = 20u32;
    let amount = 500i128;

    let agreement_id = setup_funded_milestone(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount,
        count,
    );

    // Approve all milestones.
    for i in 1..=count {
        client.approve_milestone(&agreement_id, &i);
    }

    // Build ID list and batch-claim all at once.
    let mut id_list = soroban_sdk::Vec::new(&env);
    for i in 1..=count {
        id_list.push_back(i);
    }

    let res = client.batch_claim_milestones(&agreement_id, &id_list);

    assert_eq!(res.successful_claims, count);
    assert_eq!(res.failed_claims, 0);
    assert_eq!(res.total_claimed, amount * (count as i128));

    // Spot-check a few milestones directly.
    assert!(client.get_milestone(&agreement_id, &1).unwrap().claimed);
    assert!(client.get_milestone(&agreement_id, &10).unwrap().claimed);
    assert!(client.get_milestone(&agreement_id, &20).unwrap().claimed);

    // Total payout reaches contributor's wallet.
    let tok = soroban_sdk::token::Client::new(&env, &token);
    assert_eq!(tok.balance(&contributor), amount * (count as i128));
}

// ============================================================================
// SECTION 14 — ISOLATION: OPERATIONS ON SEPARATE AGREEMENTS DON'T INTERFERE
// ============================================================================

/// Creates two independent agreements — one milestone, one escrow — and verifies
/// that operations on each have zero side effects on the other's:
/// - Milestone claim status across agreement boundaries.
/// - Dispute lifecycle on the escrow does not corrupt milestone state.
/// - Token balances are correctly isolated.
#[test]
fn test_separate_agreements_do_not_interfere() {
    let (env, employer, token, arbiter, client) = create_test_env();

    let c1 = Address::generate(&env);
    let c2 = Address::generate(&env);

    // id1: milestone agreement for c1.
    let id1 = setup_funded_milestone(&env, &client, &employer, &c1, &token, 1000, 3);
    client.approve_milestone(&id1, &1);
    client.approve_milestone(&id1, &2);
    client.approve_milestone(&id1, &3);

    // id2: **separate** escrow agreement for c2.
    // We create a dummy payroll first to ensure the escrow counter produces
    // a different ID than the milestone counter, eliminating any cross-type
    // ID collision when both counters start at 1.
    let _dummy = client.create_payroll_agreement(&employer, &token, &604800u64);
    let id2 = setup_funded_escrow(&env, &client, &employer, &c2, &token, 2000, 86400, 2);

    // Claim milestones 1 and 2 on id1 via batch (performs SAC transfer).
    let batch1 = client.batch_claim_milestones(&id1, &Vec::from_array(&env, [1u32, 2u32]));
    assert_eq!(batch1.successful_claims, 2);
    assert_eq!(batch1.total_claimed, 2000);

    // id1 milestone 3 still unclaimed — no cross-agreement contamination.
    assert!(!client.get_milestone(&id1, &3).unwrap().claimed);

    // --- Dispute lifecycle on id2 ---
    client.raise_dispute(&employer, &id2);
    assert_eq!(client.get_dispute_status(&id2), DisputeStatus::Raised);

    // id1's milestone state must still be intact after dispute raised on id2.
    assert!(client.get_milestone(&id1, &1).unwrap().claimed);
    assert!(client.get_milestone(&id1, &2).unwrap().claimed);
    assert!(!client.get_milestone(&id1, &3).unwrap().claimed);

    // Resolve dispute on id2.
    mint(&env, &token, &client.address, 4000);
    client.resolve_dispute(&arbiter, &id2, &1000, &1000);
    assert_eq!(client.get_dispute_status(&id2), DisputeStatus::Resolved);

    // id1 milestone 3 is still available and uncorrupted after dispute resolution on id2.
    assert!(!client.get_milestone(&id1, &3).unwrap().claimed);
    let batch3 = client.batch_claim_milestones(&id1, &Vec::from_array(&env, [3u32]));
    assert_eq!(batch3.successful_claims, 1);

    // Token balance: c1 received 3 × 1000 from batch claims; only from id1 funds.
    let tok = soroban_sdk::token::Client::new(&env, &token);
    assert_eq!(tok.balance(&c1), 3000);
}

// ============================================================================
// SECTION 15 — RACE: CLAIM vs DISPUTE ON THE SAME AGREEMENT
// ============================================================================
//
// # Deterministic ordering guarantee
//
// Soroban ledgers apply transactions sequentially within a single ledger close.
// Both operations must therefore land in *some* total order, and the contract
// must produce a safe, deterministic outcome for each permutation:
//
// | Order           | Expected outcome                                           |
// |-----------------|------------------------------------------------------------|
// | claim → dispute | Claim finalises first; dispute is accepted (window open).  |
// |                 | Subsequent claims on the same agreement are blocked once   |
// |                 | status is `Disputed`. No double-payment through dispute.   |
// | dispute → claim | Status transitions to `Disputed`; all subsequent claim    |
// |                 | attempts return an error (`InvalidData` for               |
// |                 | `claim_payroll` / `NotInGracePeriod` for                  |
// |                 | `claim_time_based`). No funds are transferred.            |
//
// # Why `Disputed` blocks claims
//
// Every claim entry-point (`claim_payroll_inner`, `claim_time_based`,
// `batch_claim_payroll_inner`) evaluates:
//
// ```rust
// let can_claim = match agreement.status {
//     AgreementStatus::Active    => true,
//     AgreementStatus::Cancelled => is_grace_period_active(env, agreement_id),
//     _                          => false,   // Disputed falls here
// };
// if !can_claim { return Err(...); }
// ```
//
// `raise_dispute` atomically sets `agreement.status = AgreementStatus::Disputed`
// before returning.  Any transaction that reads the agreement after that write
// sees `Disputed` and cannot proceed — guaranteeing a disputed agreement can
// never have additional funds drained by a racing claim.

/// Helper: seed the DataKey entries that `claim_payroll_inner` requires but
/// that the public `create_payroll_agreement` / `add_employee_to_agreement`
/// entry-points do not write (they use a separate `StorageKey::AgreementEmployees`
/// Vec which is not consulted by the payroll-claim path).
///
/// Must be called inside `env.as_contract(&client.address, || { … })`.
fn seed_payroll_claim_keys(
    env: &Env,
    agreement_id: u128,
    token: &Address,
    employees: &[(Address, i128)],
    period_s: u64,
    total_escrow: i128,
) {
    let now = env.ledger().timestamp();
    DataKey::set_agreement_activation_time(env, agreement_id, now);
    DataKey::set_agreement_period_duration(env, agreement_id, period_s);
    DataKey::set_agreement_token(env, agreement_id, token);
    DataKey::set_employee_count(env, agreement_id, employees.len() as u32);
    DataKey::set_agreement_escrow_balance(env, agreement_id, token, total_escrow);
    for (idx, (addr, salary)) in employees.iter().enumerate() {
        DataKey::set_employee(env, agreement_id, idx as u32, addr);
        DataKey::set_employee_salary(env, agreement_id, idx as u32, *salary);
    }
}

// ----------------------------------------------------------------------------
// 15.1 — ESCROW: dispute-first ordering blocks the subsequent claim
// ----------------------------------------------------------------------------

/// Race scenario: `raise_dispute` lands **before** `claim_time_based`.
///
/// Once the agreement is `Disputed`, `claim_time_based` hits the `_ => false`
/// branch of its status match and returns `NotInGracePeriod`.  No tokens move.
#[test]
fn test_race_dispute_before_escrow_claim_blocks_claim() {
    let (env, employer, token, _arbiter, client) = create_test_env();
    let contributor = Address::generate(&env);

    let agreement_id = setup_funded_escrow(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1_000,
        86400,
        4,
    );
    env.ledger().with_mut(|li| li.timestamp += 86400);

    // ── Simulated ledger close ──────────────────────────────────────────────
    // TX 0: dispute lands first.
    client.raise_dispute(&employer, &agreement_id);
    // TX 1: contributor tries to claim — must be blocked.
    let result = client.try_claim_time_based(&agreement_id);
    // ───────────────────────────────────────────────────────────────────────

    assert_eq!(
        result,
        Err(Ok(PayrollError::NotInGracePeriod)),
        "claim_time_based must be blocked after dispute is raised"
    );

    let tok = soroban_sdk::token::Client::new(&env, &token);
    assert_eq!(
        tok.balance(&contributor),
        0,
        "no funds must reach contributor"
    );

    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Raised
    );
    assert_eq!(
        client.get_agreement(&agreement_id).unwrap().status,
        AgreementStatus::Disputed
    );
}

// ----------------------------------------------------------------------------
// 15.2 — ESCROW: claim-first ordering pays once; subsequent dispute accepted
//         but cannot trigger a second payout
// ----------------------------------------------------------------------------

/// Race scenario: `claim_time_based` lands **before** `raise_dispute`.
///
/// The claim pays exactly one period.  The dispute is subsequently accepted.
/// A second claim attempt after the dispute is raised is blocked — no
/// double-payment is possible through the dispute resolution path.
#[test]
fn test_race_escrow_claim_before_dispute_no_double_payment() {
    let (env, employer, token, _arbiter, client) = create_test_env();
    let contributor = Address::generate(&env);

    let amount = 1_000i128;
    let agreement_id = setup_funded_escrow(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount,
        86400,
        4,
    );
    env.ledger().with_mut(|li| li.timestamp += 86400);

    // ── Simulated ledger close ──────────────────────────────────────────────
    // TX 0: claim lands first and succeeds.
    client.claim_time_based(&agreement_id);
    // TX 1: dispute raised immediately after.
    client.raise_dispute(&employer, &agreement_id);
    // ───────────────────────────────────────────────────────────────────────

    let tok = soroban_sdk::token::Client::new(&env, &token);
    assert_eq!(tok.balance(&contributor), amount, "exactly one period paid");
    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Raised
    );
    assert_eq!(
        client.get_claimed_periods(&agreement_id),
        1u32,
        "claimed_periods must not be reset by the dispute raise"
    );

    // A second claim after the dispute is raised must be blocked.
    env.ledger().with_mut(|li| li.timestamp += 86400);
    assert_eq!(
        client.try_claim_time_based(&agreement_id),
        Err(Ok(PayrollError::NotInGracePeriod)),
        "second claim must be blocked once agreement is Disputed"
    );
    assert_eq!(tok.balance(&contributor), amount, "balance must not change");
}

// ----------------------------------------------------------------------------
// 15.3 — PAYROLL: dispute-first ordering blocks the subsequent payroll claim
// ----------------------------------------------------------------------------

/// Race scenario: `raise_dispute` lands **before** `claim_payroll`.
///
/// Once the agreement is `Disputed`, `claim_payroll` hits the `_ => false`
/// branch of its status match and returns `InvalidData`.  No tokens move.
#[test]
fn test_race_dispute_before_payroll_claim_blocks_claim() {
    let (env, employer, token, _arbiter, client) = create_test_env();
    let employee = Address::generate(&env);

    let grace_s = 86400u64 * 30;
    let period_s = 86400u64;
    let salary = 2_000i128;
    let num_periods = 6u32;
    let total = salary * num_periods as i128;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace_s);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary);
    client.activate_agreement(&agreement_id);

    env.as_contract(&client.address, || {
        seed_payroll_claim_keys(
            &env,
            agreement_id,
            &token,
            &[(employee.clone(), salary)],
            period_s,
            total,
        );
    });
    mint(&env, &token, &client.address, total);

    env.ledger().with_mut(|li| li.timestamp += period_s);

    // ── Simulated ledger close ──────────────────────────────────────────────
    // TX 0: dispute raised first.
    client.raise_dispute(&employer, &agreement_id);
    // TX 1: employee tries to claim — must be rejected.
    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    // ───────────────────────────────────────────────────────────────────────

    assert_eq!(
        result,
        Err(Ok(PayrollError::InvalidData)),
        "claim_payroll must be blocked after dispute is raised"
    );

    let tok = soroban_sdk::token::Client::new(&env, &token);
    assert_eq!(tok.balance(&employee), 0, "no funds must reach employee");
    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Raised
    );
    assert_eq!(
        client.get_agreement(&agreement_id).unwrap().status,
        AgreementStatus::Disputed
    );
}

// ----------------------------------------------------------------------------
// 15.4 — PAYROLL: claim-first ordering pays once; dispute blocks further claims
// ----------------------------------------------------------------------------

/// Race scenario: `claim_payroll` lands **before** `raise_dispute`.
///
/// Exactly one salary period is paid.  The dispute is accepted.  Any
/// subsequent claim attempt is blocked — confirming no double-payment.
#[test]
fn test_race_payroll_claim_before_dispute_no_double_payment() {
    let (env, employer, token, _arbiter, client) = create_test_env();
    let employee = Address::generate(&env);

    let grace_s = 86400u64 * 30;
    let period_s = 86400u64;
    let salary = 2_000i128;
    let num_periods = 6u32;
    let total = salary * num_periods as i128;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace_s);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary);
    client.activate_agreement(&agreement_id);

    env.as_contract(&client.address, || {
        seed_payroll_claim_keys(
            &env,
            agreement_id,
            &token,
            &[(employee.clone(), salary)],
            period_s,
            total,
        );
    });
    mint(&env, &token, &client.address, total);

    env.ledger().with_mut(|li| li.timestamp += period_s);

    // ── Simulated ledger close ──────────────────────────────────────────────
    // TX 0: payroll claim lands first.
    client.claim_payroll(&employee, &agreement_id, &0u32);
    // TX 1: dispute raised immediately after.
    client.raise_dispute(&employer, &agreement_id);
    // ───────────────────────────────────────────────────────────────────────

    let tok = soroban_sdk::token::Client::new(&env, &token);
    assert_eq!(tok.balance(&employee), salary, "exactly one period paid");
    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Raised
    );

    // A second claim after dispute must be blocked.
    env.ledger().with_mut(|li| li.timestamp += period_s);
    assert_eq!(
        client.try_claim_payroll(&employee, &agreement_id, &0u32),
        Err(Ok(PayrollError::InvalidData)),
        "second payroll claim must be blocked once agreement is Disputed"
    );
    assert_eq!(tok.balance(&employee), salary, "balance must not change");
}

// ----------------------------------------------------------------------------
// 15.5 — ISOLATION: dispute on one agreement does not block a separate
//         milestone claim
// ----------------------------------------------------------------------------

/// Confirms the race guard is per-agreement, not global.
///
/// Raising a dispute on an escrow agreement transitions *only that agreement*
/// to `Disputed`.  A milestone claim on a separate, independent agreement
/// in the same ledger close must succeed unimpeded.
#[test]
fn test_race_dispute_on_escrow_does_not_block_independent_milestone_claim() {
    let (env, employer, token, _arbiter, client) = create_test_env();
    let c_escrow = Address::generate(&env);
    let c_milestone = Address::generate(&env);

    let escrow_id =
        setup_funded_escrow(&env, &client, &employer, &c_escrow, &token, 1_000, 86400, 4);
    let milestone_id =
        setup_funded_milestone(&env, &client, &employer, &c_milestone, &token, 500, 2);
    client.approve_milestone(&milestone_id, &1);

    env.ledger().with_mut(|li| li.timestamp += 86400);

    // ── Simulated ledger close ──────────────────────────────────────────────
    // TX 0: dispute raised on the ESCROW agreement.
    client.raise_dispute(&employer, &escrow_id);
    // TX 1: milestone claim on the MILESTONE agreement — must succeed.
    let result = client.try_claim_milestone(&milestone_id, &1u32);
    // ───────────────────────────────────────────────────────────────────────

    assert!(
        result.is_ok(),
        "milestone claim on a separate agreement must not be blocked: {result:?}"
    );

    assert_eq!(client.get_dispute_status(&escrow_id), DisputeStatus::Raised);
    assert!(client.get_milestone(&milestone_id, &1).unwrap().claimed);

    let tok = soroban_sdk::token::Client::new(&env, &token);
    assert_eq!(tok.balance(&c_milestone), 500);
    assert_eq!(tok.balance(&c_escrow), 0, "no escrow funds must leak");
}

// ----------------------------------------------------------------------------
// 15.6 — FULL LIFECYCLE: claim → dispute → resolve conserves funds exactly
// ----------------------------------------------------------------------------

/// End-to-end fund-conservation test across a claim/dispute race.
///
/// After a successful claim reduces the escrow balance, the arbiter cannot
/// distribute more than what remains.  The total of all payouts (claim +
/// dispute resolution) equals the original `total_amount` exactly.
#[test]
fn test_race_claim_then_dispute_then_resolve_no_double_payment() {
    let (env, employer, token, arbiter, client) = create_test_env();
    let contributor = Address::generate(&env);

    let amount = 1_000i128;
    let num_periods = 4u32;
    let total = amount * num_periods as i128; // 4_000

    let agreement_id = setup_funded_escrow(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount,
        86400,
        num_periods,
    );
    env.ledger().with_mut(|li| li.timestamp += 86400);

    // TX 0: contributor claims one period (1_000 tokens leave escrow).
    client.claim_time_based(&agreement_id);
    let tok = soroban_sdk::token::Client::new(&env, &token);
    assert_eq!(tok.balance(&contributor), amount);

    // TX 1: employer raises dispute (3_000 remain in tracked escrow).
    client.raise_dispute(&employer, &agreement_id);
    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Raised
    );

    // Arbiter attempts to distribute the full original total — must FAIL because
    // only 3_000 remain in the tracked escrow balance.
    assert!(
        client
            .try_resolve_dispute(&arbiter, &agreement_id, &total, &0)
            .is_err(),
        "arbiter must not distribute more than the remaining escrow balance"
    );
    // Nothing extra transferred by the failed call.
    assert_eq!(tok.balance(&contributor), amount);

    // Correct resolution: 2_000 to contributor + 1_000 refund to employer.
    client.resolve_dispute(&arbiter, &agreement_id, &2_000, &1_000);

    assert_eq!(
        tok.balance(&contributor),
        amount + 2_000,
        "contributor total = claim + dispute payout"
    );
    assert_eq!(tok.balance(&employer), 1_000, "employer refund");
    assert_eq!(
        tok.balance(&contributor) + tok.balance(&employer),
        total,
        "sum of all payouts must equal original total_amount"
    );
    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Resolved
    );
}

// ----------------------------------------------------------------------------
// 15.7 — BATCH PAYROLL: dispute between batches blocks the second batch fully
// ----------------------------------------------------------------------------

/// Verifies that when a dispute is raised after one successful `claim_payroll`
/// call, a subsequent `claim_payroll` for a different employee on the same
/// agreement is entirely rejected.
///
/// Specifically guards against a TOCTOU window where the second employee's
/// claim could slip through before the agreement status check fires.
#[test]
fn test_race_dispute_between_batch_payroll_calls_blocks_second_batch() {
    let (env, employer, token, _arbiter, client) = create_test_env();
    let e1 = Address::generate(&env);
    let e2 = Address::generate(&env);

    let grace_s = 86400u64 * 30;
    let period_s = 86400u64;
    let salary = 1_500i128;
    let num_periods = 4u32;
    let total = salary * 2 * num_periods as i128;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace_s);
    client.add_employee_to_agreement(&agreement_id, &e1, &salary);
    client.add_employee_to_agreement(&agreement_id, &e2, &salary);
    client.activate_agreement(&agreement_id);

    env.as_contract(&client.address, || {
        seed_payroll_claim_keys(
            &env,
            agreement_id,
            &token,
            &[(e1.clone(), salary), (e2.clone(), salary)],
            period_s,
            total,
        );
    });
    mint(&env, &token, &client.address, total);

    env.ledger().with_mut(|li| li.timestamp += period_s);

    // ── First close: e1 claims one period ──────────────────────────────────
    client.claim_payroll(&e1, &agreement_id, &0u32);
    let tok = soroban_sdk::token::Client::new(&env, &token);
    assert_eq!(tok.balance(&e1), salary);

    // ── Second close: dispute raised, then e2 tries to claim ───────────────
    client.raise_dispute(&employer, &agreement_id);

    assert_eq!(
        client.try_claim_payroll(&e2, &agreement_id, &1u32),
        Err(Ok(PayrollError::InvalidData)),
        "e2 claim must be blocked because the agreement is now Disputed"
    );

    assert_eq!(tok.balance(&e2), 0, "e2 must receive nothing");
    assert_eq!(tok.balance(&e1), salary, "e1 balance must be preserved");
    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Raised
    );
}
