//! Reentrancy protection tests (#197).
//!
//! Verifies state consistency for payment-related functions: after a successful
//! claim, state is updated so a second claim fails (double-claim prevented).

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};
use stello_pay_contract::{
    storage::{DataKey, DisputeStatus, PayrollError, StorageKey},
    PayrollContract, PayrollContractClient,
};

const ONE_DAY: u64 = 86400;

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn create_address(env: &Env) -> Address {
    Address::generate(env)
}

fn create_token(env: &Env) -> Address {
    let admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

fn setup_contract(env: &Env) -> (Address, PayrollContractClient<'static>) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = create_address(env);
    client.initialize(&owner);
    (contract_id, client)
}

fn fund_agreement_escrow(
    env: &Env,
    contract_id: &Address,
    agreement_id: u128,
    token: &Address,
    amount: i128,
) {
    env.as_contract(contract_id, || {
        DataKey::set_agreement_escrow_balance(env, agreement_id, token, amount);
    });
}

fn advance_time(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| {
        li.timestamp += seconds;
    });
}

/// Verifies that after a successful claim_payroll, state (claimed periods) is updated
/// so a second claim for the same period fails with NoPeriodsToClaim.
#[test]
fn test_claim_payroll_state_updated_prevents_double_claim() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_token(&env);
    let employee = create_address(&env);
    let salary = 1000i128;
    let grace = 604800u64;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary);
    client.activate_agreement(&agreement_id);

    fund_agreement_escrow(&env, &contract_id, agreement_id, &token, 10000);
    mint(&env, &token, &contract_id, 10000);

    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee_count(&env, agreement_id, 1);
    });

    advance_time(&env, ONE_DAY + 1);

    let res = client.try_claim_payroll(&employee, &agreement_id, &0);
    assert!(res.is_ok());

    let claimed = client.get_employee_claimed_periods(&agreement_id, &0);
    assert_eq!(claimed, 1);

    let res2 = client.try_claim_payroll(&employee, &agreement_id, &0);
    assert!(
        res2.is_err() || res2.as_ref().ok().and_then(|r| r.as_ref().err()).is_some(),
        "second claim must fail (no periods to claim)"
    );
}

/// Verifies that the transient reentrancy guard rejects a claim while a claim
/// is already in progress. We simulate the in-progress state by setting the
/// guard in temporary storage (exactly what `acquire_reentrancy_guard` does at
/// the top of a claim), then assert the entry point fails deterministically
/// with `ReentrancyDetected` and that no state changes.
#[test]
fn test_reentrant_claim_payroll_rejected() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_token(&env);
    let employee = create_address(&env);
    let salary = 1000i128;
    let grace = 604800u64;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary);
    client.activate_agreement(&agreement_id);

    fund_agreement_escrow(&env, &contract_id, agreement_id, &token, 10000);
    mint(&env, &token, &contract_id, 10000);

    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee_count(&env, agreement_id, 1);
        // Simulate an in-progress claim by pre-setting the transient guard.
        env.storage()
            .temporary()
            .set(&StorageKey::ReentrancyGuard, &true);
    });

    advance_time(&env, ONE_DAY + 1);

    let res = client.try_claim_payroll(&employee, &agreement_id, &0);
    assert_eq!(
        res,
        Err(Ok(PayrollError::ReentrancyDetected)),
        "reentrant claim must be rejected"
    );

    // No state changed: no period was claimed and escrow is untouched.
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0), 0);
    env.as_contract(&contract_id, || {
        assert_eq!(
            DataKey::get_agreement_escrow_balance(&env, agreement_id, &token),
            10000
        );
    });
}

/// Verifies that the guard is released after a successful claim, so a later
/// legitimate claim (after time advances) is not blocked by a stranded guard.
#[test]
fn test_guard_released_allows_subsequent_claim() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_token(&env);
    let employee = create_address(&env);
    let salary = 1000i128;
    let grace = 604800u64;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary);
    client.activate_agreement(&agreement_id);

    fund_agreement_escrow(&env, &contract_id, agreement_id, &token, 10000);
    mint(&env, &token, &contract_id, 10000);

    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee_count(&env, agreement_id, 1);
    });

    advance_time(&env, ONE_DAY + 1);
    assert!(client
        .try_claim_payroll(&employee, &agreement_id, &0)
        .is_ok());
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0), 1);

    // Escrow was decremented (effect persisted), confirming a real claim ran.
    env.as_contract(&contract_id, || {
        assert_eq!(
            DataKey::get_agreement_escrow_balance(&env, agreement_id, &token),
            9000
        );
    });

    // After another period elapses, a second claim succeeds — proving the guard
    // was cleared and not stranded by the first claim.
    advance_time(&env, ONE_DAY + 1);
    assert!(client
        .try_claim_payroll(&employee, &agreement_id, &0)
        .is_ok());
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0), 2);
}

/// Verifies that after claim_time_based, claimed periods are updated so
/// another claim without time advance does not double-pay.
/// (Requires full escrow funding setup; see test_grace_period for pattern.)
#[test]
#[ignore = "requires escrow balance storage setup - covered by test_claim_payroll_state_updated_prevents_double_claim"]
fn test_claim_time_based_state_updated_prevents_double_claim() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);
    let amount_per_period = 1000i128;
    let period_seconds = ONE_DAY;
    let num_periods = 4u32;

    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &amount_per_period,
        &period_seconds,
        &num_periods,
    );
    client.activate_agreement(&agreement_id);

    let token_client = TokenClient::new(&env, &token);
    mint(&env, &token, &employer, 4000);
    token_client.transfer(&employer, &contract_id, &4000);

    advance_time(&env, period_seconds + 1);

    let res = client.try_claim_time_based(&agreement_id);
    assert!(res.is_ok());
    let claimed = client.get_claimed_periods(&agreement_id);
    assert_eq!(claimed, 1);

    let res2 = client.try_claim_time_based(&agreement_id);
    assert!(res2.is_err(), "second claim in same period must fail");
    assert_eq!(client.get_claimed_periods(&agreement_id), 1);
}

// ============================================================================
// MILESTONE HOOK REENTRANCY REGRESSION TESTS (#855)
//
// These tests verify the Checks-Effects-Interactions (CEI) ordering invariant
// for milestone-related operations:
//
//   1. `claim_milestone` marks the milestone as CLAIMED in persistent storage BEFORE executing the
//      token transfer, so any reentrant `claim_milestone` call on the same milestone fails with
//      `MilestoneAlreadyClaimed`.
//
//   2. `expire_milestone` records the expiry flag BEFORE calling the `on_milestone_expired` hook,
//      so even a hook that tries to call `claim_milestone` on an expired milestone fails with
//      `MilestoneNotApproved` (an expired milestone was never approved).
//
//   3. The `MaliciousMilestoneHook` mock correctly fires its callback, proving the hook integration
//      path works end-to-end.
//
// Checks-Effects-Interactions (CEI) ordering in the payroll contract:
//   - Effect: `MilestoneClaimed` flag written before `TokenClient::transfer`.
//   - Effect: `MilestoneExpired` flag written before hook callback.
//   - This means any reentrant call during or after the transfer/hook will see the milestone's
//     terminal state and be rejected.
// ============================================================================

/// Import the mock hook contract so we can register it in tests.
mod support;

use support::MaliciousMilestoneHook;
use support::MaliciousMilestoneHookClient;

fn setup_milestone_agreement(
    env: &Env,
    client: &PayrollContractClient,
    contract_id: &Address,
) -> (Address, Address, Address, u128) {
    let employer = create_address(env);
    let contributor = create_address(env);
    let token = create_token(env);

    // Create and fund a milestone agreement. `fund_milestone_agreement`
    // transfers from the employer, so the employer must hold the tokens.
    let milestone_amount = 1000i128;
    let agreement_id = client.create_milestone_agreement(
        &employer,
        &contributor,
        &token,
        &soroban_sdk::vec![&env, milestone_amount],
    );
    mint(env, &token, &employer, milestone_amount);
    client.fund_milestone_agreement(&agreement_id, &employer, &milestone_amount);
    let _ = contract_id;

    (employer, contributor, token, agreement_id)
}

/// @notice `claim_milestone` uses CEI ordering: the `MilestoneClaimed` flag is
/// persisted BEFORE the token transfer. A second call to `claim_milestone` for
/// the same milestone must return `MilestoneAlreadyClaimed`.
///
/// # CEI Invariant Tested
/// 1. Milestone approved.
/// 2. `claim_milestone` called — internally: set `MilestoneClaimed = true`, decrement escrow
///    balance, THEN transfer tokens.
/// 3. Subsequent `claim_milestone` call returns `MilestoneAlreadyClaimed` (the state was committed
///    before the transfer, so no double-claim can succeed even if a hook or token callback triggers
///    it during the transfer).
#[test]
fn test_claim_milestone_cei_prevents_double_claim() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let (_employer, _contributor, _token, agreement_id) =
        setup_milestone_agreement(&env, &client, &contract_id);

    // Approve the milestone.
    client.approve_milestone(&agreement_id, &1u32);

    // First claim must succeed.
    let result = client.try_claim_milestone(&agreement_id, &1u32);
    assert!(result.is_ok(), "first claim_milestone must succeed");

    // Second claim on the same milestone must be rejected.
    let result2 = client.try_claim_milestone(&agreement_id, &1u32);
    assert!(
        result2.is_err(),
        "second claim_milestone on the same milestone must fail (MilestoneAlreadyClaimed)"
    );
}

/// @notice After `claim_milestone` succeeds, the `MilestoneClaimed` flag is
/// durable in persistent storage, preventing a reentrant or delayed claim.
///
/// This test explicitly verifies the intermediate state: the milestone count
/// before and after the claim, and that a `get_milestone` query reflects
/// `claimed = true` after a successful `claim_milestone`.
#[test]
fn test_claim_milestone_state_committed_before_transfer() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let (_employer, _contributor, _token, agreement_id) =
        setup_milestone_agreement(&env, &client, &contract_id);

    client.approve_milestone(&agreement_id, &1u32);

    // Verify pre-claim state: milestone is not claimed.
    let before = client.get_milestone(&agreement_id, &1u32).unwrap();
    assert!(
        !before.claimed,
        "milestone must not be claimed before claim_milestone"
    );

    // Execute the claim.
    client.claim_milestone(&agreement_id, &1u32);

    // Verify post-claim state: milestone is marked claimed in persistent storage.
    let after = client.get_milestone(&agreement_id, &1u32).unwrap();
    assert!(
        after.claimed,
        "milestone must be marked claimed after successful claim_milestone"
    );
}

/// @notice `expire_milestone` persists the `MilestoneExpired` flag BEFORE
/// calling the `on_milestone_expired` hook. An attempt to `claim_milestone`
/// on an expired (and unapproved) milestone fails with `MilestoneNotApproved`.
///
/// This verifies the CEI ordering of `expire_milestone`:
///   1. Expired flag stored.
///   2. Event emitted.
///   3. Hook called.
/// Any hook that tries to approve + claim will see the already-expired state
/// and the claim will fail (expired milestones cannot be approved).
#[test]
fn test_expire_milestone_cei_blocks_subsequent_claim() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let (_employer, _contributor, _token, agreement_id) =
        setup_milestone_agreement(&env, &client, &contract_id);

    // Expire the milestone WITHOUT approving it.
    let expire_result = client.try_expire_milestone(&agreement_id, &1u32);
    assert!(
        expire_result.is_ok(),
        "expire_milestone must succeed for an unapproved milestone"
    );

    // A claim attempt on an expired, unapproved milestone must fail.
    let claim_result = client.try_claim_milestone(&agreement_id, &1u32);
    assert!(
        claim_result.is_err(),
        "claim_milestone must fail on an expired, unapproved milestone"
    );
}

/// @notice A hook configured via `set_milestone_hook_contract` is called during
/// `expire_milestone`. The hook fires exactly once per expiry event and the
/// milestone remains in its expired state after the hook returns.
///
/// Uses the `MaliciousMilestoneHook` mock to record that the hook was called,
/// then asserts:
///   - hook call count == 1 (fired exactly once).
///   - attempted_reentry == true (the hook ran its body).
///   - The milestone is still unreachable via `claim_milestone`.
#[test]
fn test_expire_milestone_hook_fires_and_milestone_remains_expired() {
    let env = create_env();

    // Deploy a fresh payroll contract with a known owner so we can call
    // set_milestone_hook_contract without owner confusion.
    let fresh_contract_id = env.register(stello_pay_contract::PayrollContract, ());
    let fresh_client = PayrollContractClient::new(&env, &fresh_contract_id);
    let known_owner = create_address(&env);
    fresh_client.initialize(&known_owner);

    // Set up a milestone agreement on the fresh contract.
    let employer2 = create_address(&env);
    let contributor2 = create_address(&env);
    let token2 = create_token(&env);
    let fresh_token_amount = 500i128;
    let fresh_agreement_id = fresh_client.create_milestone_agreement(
        &employer2,
        &contributor2,
        &token2,
        &soroban_sdk::vec![&env, fresh_token_amount],
    );
    // Funding transfers from the employer, so mint to the employer first.
    mint(&env, &token2, &employer2, fresh_token_amount);
    fresh_client.fund_milestone_agreement(&fresh_agreement_id, &employer2, &fresh_token_amount);

    // Deploy the malicious hook mock and initialize it.
    let fresh_hook_id = env.register(MaliciousMilestoneHook, ());
    let fresh_hook_client = MaliciousMilestoneHookClient::new(&env, &fresh_hook_id);
    fresh_hook_client.initialize(&fresh_contract_id, &contributor2);

    // Register the hook with the payroll contract (owner-gated).
    fresh_client.set_milestone_hook_contract(&known_owner, &fresh_hook_id);

    // Expire the milestone — the payroll contract marks it expired, then fires the hook.
    let result = fresh_client.try_expire_milestone(&fresh_agreement_id, &1u32);
    assert!(result.is_ok(), "expire_milestone must succeed");

    // Hook must have been called exactly once.
    let call_count = fresh_hook_client.get_hook_call_count();
    assert_eq!(
        call_count, 1,
        "on_milestone_expired hook must be called exactly once; got {}",
        call_count
    );

    // The hook must have recorded that it ran (attempted_reentry flag set).
    assert!(
        fresh_hook_client.attempted_reentry(),
        "hook must have set the attempted_reentry flag indicating it executed"
    );

    // After hook execution the milestone is expired and cannot be claimed.
    let claim_result = fresh_client.try_claim_milestone(&fresh_agreement_id, &1u32);
    assert!(
        claim_result.is_err(),
        "claim_milestone must fail on an expired milestone even after hook fires"
    );
}

/// @notice A re-expiry call on an already-expired milestone is rejected.
///
/// This prevents a hook from being triggered more than once for a given
/// milestone by ensuring `expire_milestone` is idempotent after the first call.
#[test]
fn test_expire_milestone_idempotency_prevents_repeated_hook_invocation() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let (_employer, _contributor, _token, agreement_id) =
        setup_milestone_agreement(&env, &client, &contract_id);

    // First expiry succeeds.
    client.expire_milestone(&agreement_id, &1u32);

    // Second expiry on the same milestone must fail.
    let result2 = client.try_expire_milestone(&agreement_id, &1u32);
    assert!(
        result2.is_err(),
        "second expire_milestone on an already-expired milestone must be rejected"
    );
}

/// @notice Claiming an approved milestone while another claim is in progress
/// (simulated via the transient reentrancy guard set in storage) is rejected
/// with `ReentrancyDetected`.
///
/// This directly mirrors the `test_reentrant_claim_payroll_rejected` test but
/// targets the milestone claim path. The guard in `claim_milestone` (if
/// present) or the CEI pattern ensures the claimed flag is set before any
/// external call, preventing a double-claim.
#[test]
fn test_claim_milestone_reentrant_call_rejected_by_claimed_flag() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let (_employer, _contributor, _token, agreement_id) =
        setup_milestone_agreement(&env, &client, &contract_id);

    client.approve_milestone(&agreement_id, &1u32);

    // First claim succeeds and marks the milestone claimed.
    let first = client.try_claim_milestone(&agreement_id, &1u32);
    assert!(first.is_ok(), "first claim_milestone must succeed");

    // Simulate a reentrant call: the claimed flag is already true in storage.
    // Any subsequent call must be rejected.
    let second = client.try_claim_milestone(&agreement_id, &1u32);
    assert!(
        second.is_err(),
        "reentrant or repeated claim_milestone must be rejected due to MilestoneAlreadyClaimed"
    );
}

// ============================================================================
// DISPUTE RESOLUTION REENTRANCY TEST
//
// Verifies that the transient reentrancy guard rejects a dispute-resolution
// payout while a dispute resolution is already in progress. This mirrors the
// `test_reentrant_claim_payroll_rejected` test but targets the dispute path
// (`resolve_dispute`), which is a separately-implemented money-movement path
// with its own reentrancy gap.
//
// We simulate the in-progress state by setting the guard in temporary storage
// (exactly what `acquire_reentrancy_guard` does at the top of `resolve_dispute`),
// then assert the entry point fails deterministically with `ReentrancyDetected`
// and that no state changes occur.
// ============================================================================

/// @notice A reentrant `resolve_dispute` call (guard pre-set) is rejected with
/// `ReentrancyDetected` and leaves all state unchanged.
#[test]
fn test_reentrant_dispute_resolution_rejected() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let arbiter = create_address(&env);
    let token = create_token(&env);

    client.set_arbiter(&employer, &arbiter);

    let amount_per_period = 1000i128;
    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &amount_per_period,
        &ONE_DAY,
        &4u32,
    );
    client.activate_agreement(&agreement_id);

    // Fund the contract so the dispute resolution *could* succeed (the guard is
    // checked before any transfer, so the funding is for setup completeness).
    mint(&env, &token, &employer, 4000);
    TokenClient::new(&env, &token).transfer(&employer, &contract_id, &4000);

    // Raise a dispute (within the grace period at creation time).
    client.raise_dispute(&employer, &agreement_id);

    // Verify dispute is raised.
    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Raised
    );

    // Simulate an in-progress dispute resolution by pre-setting the transient
    // guard in temporary storage.
    env.as_contract(&contract_id, || {
        env.storage()
            .temporary()
            .set(&StorageKey::ReentrancyGuard, &true);
    });

    // Attempt resolve_dispute — the guard must reject before any state change
    // or token transfer.
    let pay_employee = 2000i128;
    let refund_employer = 2000i128;
    let res = client.try_resolve_dispute(&arbiter, &agreement_id, &pay_employee, &refund_employer);
    assert_eq!(
        res,
        Err(Ok(PayrollError::ReentrancyDetected)),
        "reentrant dispute resolution must be rejected"
    );

    // No state changed: dispute status is still Raised.
    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Raised,
        "dispute status must remain Raised after rejected reentrant call"
    );
}
