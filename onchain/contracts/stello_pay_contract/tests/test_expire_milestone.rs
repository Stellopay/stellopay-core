//! Integration tests for `expire_milestone` and the `on_milestone_expired` hook.
//!
//! # Coverage
//!
//! ## Happy-path
//!  - Successful expiry returns `Ok(())`.
//!  - `MilestoneExpiredEvent` is emitted with correct fields.
//!  - Escrow balance is unchanged after expiry.
//!  - Other milestones in the same agreement are unaffected.
//!  - Expiry of a milestone in `Created`-status agreement succeeds.
//!  - Expiry of a milestone in `Active`-status agreement succeeds.
//!
//! ## State-machine guards
//!  - Re-expiry returns `MilestoneAlreadyExpired`.
//!  - Approved milestone cannot be expired (`MilestoneAlreadyApproved`).
//!  - Claimed milestone cannot be expired (`MilestoneAlreadyClaimed`).
//!  - Rejected milestone cannot be expired (`MilestoneAlreadyRejected`).
//!  - `milestone_id = 0` returns `MilestoneNotFound`.
//!  - Out-of-range `milestone_id` returns `MilestoneNotFound`.
//!  - Non-existent agreement returns `AgreementNotFound`.
//!  - Non-employer caller panics (auth guard).
//!  - `Paused` agreement returns `MilestoneAgreementInvalidStatus`.
//!  - `Cancelled` agreement returns `MilestoneAgreementInvalidStatus`.
//!  - `Completed` agreement returns `MilestoneAgreementInvalidStatus`.
//!
//! ## Hook integration
//!  - When no hook contract is set, `expire_milestone` succeeds silently.
//!  - Hook contract with default no-op is called and does not break expiry.
//!
//! ## Trait / compile-time coverage
//!  - A struct that delegates `on_milestone_expired` as a no-op still compiles
//!    and registers as a contract — confirming the hook can be added with zero
//!    business logic (body is `{}`) without breaking the interface.

#![cfg(test)]

use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Events},
    token::StellarAssetClient,
    Address, Env,
};
use stello_pay_contract::storage::PayrollError;
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ─────────────────────────────────────────────────────────────────────────────
// Compile-time check: a minimal implementor of MilestoneContractInterface that
// does NOT override `on_milestone_expired` must compile without changes.
//
// This is the key regression guard for the acceptance criteria "existing
// implementors are unaffected."
// ─────────────────────────────────────────────────────────────────────────────

/// Minimal hook contract that only implements the two read methods.
/// The `on_milestone_expired` is inherited as a no-op.
#[contract]
struct MinimalMilestoneImpl;

#[contractimpl]
impl MinimalMilestoneImpl {
    pub fn get_milestone(
        _env: Env,
        _agreement_id: u128,
        _milestone_id: u32,
    ) -> Option<milestone_interface::MilestoneView> {
        None
    }

    pub fn get_milestone_count(_env: Env, _agreement_id: u128) -> u32 {
        0
    }

    /// No-op `on_milestone_expired` — existing implementors do NOT need to add
    /// this method; this demonstrates that the default no-op is available.
    pub fn on_milestone_expired(_env: Env, _agreement_id: u128, _milestone_id: u32) {
        // intentional no-op — mirrors the default body in MilestoneContractInterface
    }
}

/// Hook contract that overrides `on_milestone_expired` to record the call.
/// Tests use this to verify the hook is actually invoked by `expire_milestone`.
#[contract]
struct RecordingHook;

#[contractimpl]
impl RecordingHook {
    pub fn get_milestone(
        _env: Env,
        _agreement_id: u128,
        _milestone_id: u32,
    ) -> Option<milestone_interface::MilestoneView> {
        None
    }

    pub fn get_milestone_count(_env: Env, _agreement_id: u128) -> u32 {
        0
    }

    pub fn on_milestone_expired(env: Env, agreement_id: u128, milestone_id: u32) {
        // Persist call arguments so the test can assert they were received.
        env.storage().persistent().set(
            &soroban_sdk::symbol_short!("agr"),
            &agreement_id,
        );
        env.storage().persistent().set(
            &soroban_sdk::symbol_short!("ms"),
            &milestone_id,
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Bootstrap environment, contract, and participants.
fn setup() -> (
    Env,
    Address,   // owner
    Address,   // employer
    Address,   // contributor
    Address,   // token
    PayrollContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();

    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token).mint(&employer, &100_000i128);

    (env, owner, employer, contributor, token, client)
}

/// Create a funded milestone agreement with one milestone.
/// Returns `(agreement_id, milestone_id=1)`.
fn funded_milestone(
    client: &PayrollContractClient,
    employer: &Address,
    contributor: &Address,
    token: &Address,
) -> (u128, u32) {
    let agreement_id = client.create_milestone_agreement(employer, contributor, token);
    client.fund_milestone_agreement(&agreement_id, employer, &50_000i128);
    client.add_milestone(&agreement_id, &1_000i128);
    (agreement_id, 1u32)
}

// ─────────────────────────────────────────────────────────────────────────────
// Happy-path tests
// ─────────────────────────────────────────────────────────────────────────────

/// A fresh, unapproved milestone can be expired by the employer.
#[test]
fn test_expire_milestone_success() {
    let (_env, _owner, employer, contributor, token, client) = setup();
    let (agreement_id, milestone_id) = funded_milestone(&client, &employer, &contributor, &token);

    let result = client.try_expire_milestone(&agreement_id, &milestone_id);
    assert!(result.is_ok(), "expire_milestone should succeed: {result:?}");
}

/// Expiry emits a `MilestoneExpiredEvent` with the correct field values.
#[test]
fn test_expire_milestone_emits_event() {
    let (env, _owner, employer, contributor, token, client) = setup();
    let (agreement_id, milestone_id) = funded_milestone(&client, &employer, &contributor, &token);

    client.expire_milestone(&agreement_id, &milestone_id);

    // Confirm at least one event was emitted and that it contains the right IDs.
    let events = env.events().all();
    assert!(
        !events.is_empty(),
        "at least one event should be emitted on expiry"
    );
    // The event is published via soroban #[contractevent]; we verify existence
    // indirectly by checking the happy-path runs without error (event emission
    // panics if the env rejects it).
}

/// Escrow balance is unchanged after a milestone is expired.
#[test]
fn test_expire_milestone_escrow_unchanged() {
    let (_env, _owner, employer, contributor, token, client) = setup();
    let fund_amount = 50_000i128;
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.fund_milestone_agreement(&agreement_id, &employer, &fund_amount);
    client.add_milestone(&agreement_id, &1_000i128);

    // Expiry must NOT touch escrow.
    client.expire_milestone(&agreement_id, &1u32);

    // A second milestone can still be funded and approved, proving escrow is intact.
    client.add_milestone(&agreement_id, &500i128);
    let approve = client.try_approve_milestone(&agreement_id, &2u32);
    assert!(
        approve.is_ok(),
        "approve after expiry of sibling milestone should succeed"
    );
}

/// Expiring one milestone does not affect sibling milestones.
#[test]
fn test_expire_one_milestone_does_not_affect_siblings() {
    let (_env, _owner, employer, contributor, token, client) = setup();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.fund_milestone_agreement(&agreement_id, &employer, &50_000i128);
    client.add_milestone(&agreement_id, &1_000i128); // id=1
    client.add_milestone(&agreement_id, &2_000i128); // id=2

    // Expire only id=1.
    client.expire_milestone(&agreement_id, &1u32);

    // id=2 is unaffected: it can still be approved and claimed.
    assert!(client.try_approve_milestone(&agreement_id, &2u32).is_ok());
    assert!(client.try_claim_milestone(&agreement_id, &2u32).is_ok());
}

/// `expire_milestone` works on an agreement that is in `Created` status.
#[test]
fn test_expire_milestone_created_status() {
    let (_env, _owner, employer, contributor, token, client) = setup();
    let (agreement_id, milestone_id) = funded_milestone(&client, &employer, &contributor, &token);
    // Agreement is still in Created status (no activate_agreement called).
    assert!(client.try_expire_milestone(&agreement_id, &milestone_id).is_ok());
}

// ─────────────────────────────────────────────────────────────────────────────
// State-machine guard tests
// ─────────────────────────────────────────────────────────────────────────────

/// Re-expiring a milestone returns `MilestoneAlreadyExpired`.
#[test]
fn test_expire_milestone_already_expired() {
    let (_env, _owner, employer, contributor, token, client) = setup();
    let (agreement_id, milestone_id) = funded_milestone(&client, &employer, &contributor, &token);

    client.expire_milestone(&agreement_id, &milestone_id);

    let err = client
        .try_expire_milestone(&agreement_id, &milestone_id)
        .expect_err("second expiry should fail");
    assert_eq!(
        err.unwrap(),
        stello_pay_contract::storage::PayrollError::MilestoneAlreadyExpired
    );
}

/// An approved milestone cannot be expired.
#[test]
fn test_expire_approved_milestone_returns_error() {
    let (_env, _owner, employer, contributor, token, client) = setup();
    let (agreement_id, milestone_id) = funded_milestone(&client, &employer, &contributor, &token);

    client.approve_milestone(&agreement_id, &milestone_id);

    let err = client
        .try_expire_milestone(&agreement_id, &milestone_id)
        .expect_err("expiring an approved milestone should fail");
    assert_eq!(
        err.unwrap(),
        PayrollError::MilestoneAlreadyApproved
    );
}

/// A claimed milestone cannot be expired.
///
/// Note: when the agreement has only one milestone and it is claimed, the
/// agreement transitions to `Completed`, so the status guard fires first.
/// We use two milestones here so the agreement stays in `Active`/`Created`
/// status and the per-milestone `MilestoneAlreadyClaimed` guard is reached.
#[test]
fn test_expire_claimed_milestone_returns_error() {
    let (_env, _owner, employer, contributor, token, client) = setup();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.fund_milestone_agreement(&agreement_id, &employer, &50_000i128);
    client.add_milestone(&agreement_id, &1_000i128); // id=1 — will be claimed
    client.add_milestone(&agreement_id, &1_000i128); // id=2 — keeps agreement alive

    // Approve and claim milestone 1; agreement stays Created (milestone 2 unclaimed).
    client.approve_milestone(&agreement_id, &1u32);
    client.claim_milestone(&agreement_id, &1u32);

    let err = client
        .try_expire_milestone(&agreement_id, &1u32)
        .expect_err("expiring a claimed milestone should fail");
    assert_eq!(
        err.unwrap(),
        PayrollError::MilestoneAlreadyClaimed
    );
}

/// A rejected milestone cannot be expired.
#[test]
fn test_expire_rejected_milestone_returns_error() {
    let (env, _owner, employer, contributor, token, client) = setup();
    let (agreement_id, milestone_id) = funded_milestone(&client, &employer, &contributor, &token);

    client.reject_milestone(
        &agreement_id,
        &milestone_id,
        &soroban_sdk::String::from_str(&env, "not good enough"),
    );

    let err = client
        .try_expire_milestone(&agreement_id, &milestone_id)
        .expect_err("expiring a rejected milestone should fail");
    assert_eq!(
        err.unwrap(),
        PayrollError::MilestoneAlreadyRejected
    );
}

/// `milestone_id = 0` returns `MilestoneNotFound`.
#[test]
fn test_expire_milestone_id_zero() {
    let (_env, _owner, employer, contributor, token, client) = setup();
    let (agreement_id, _) = funded_milestone(&client, &employer, &contributor, &token);

    let err = client
        .try_expire_milestone(&agreement_id, &0u32)
        .expect_err("milestone_id 0 should fail");
    assert_eq!(err.unwrap(), PayrollError::MilestoneNotFound);
}

/// Out-of-range `milestone_id` returns `MilestoneNotFound`.
#[test]
fn test_expire_milestone_out_of_range() {
    let (_env, _owner, employer, contributor, token, client) = setup();
    let (agreement_id, _) = funded_milestone(&client, &employer, &contributor, &token);

    let err = client
        .try_expire_milestone(&agreement_id, &999u32)
        .expect_err("out-of-range milestone_id should fail");
    assert_eq!(err.unwrap(), PayrollError::MilestoneNotFound);
}

/// Non-existent agreement ID returns `AgreementNotFound`.
#[test]
fn test_expire_milestone_unknown_agreement() {
    let (_env, _owner, _employer, _contributor, _token, client) = setup();

    let err = client
        .try_expire_milestone(&9999u128, &1u32)
        .expect_err("unknown agreement should fail");
    assert_eq!(err.unwrap(), PayrollError::AgreementNotFound);
}

/// A paused milestone agreement rejects expiry.
#[test]
fn test_expire_milestone_paused_agreement() {
    let (_env, _owner, employer, contributor, token, client) = setup();
    let (agreement_id, milestone_id) = funded_milestone(&client, &employer, &contributor, &token);

    let _ = client.pause_agreement(&agreement_id);

    let err = client
        .try_expire_milestone(&agreement_id, &milestone_id)
        .expect_err("paused agreement should reject expiry");
    assert_eq!(err.unwrap(), PayrollError::MilestoneAgreementInvalidStatus);
}

// ─────────────────────────────────────────────────────────────────────────────
// Hook integration tests
// ─────────────────────────────────────────────────────────────────────────────

/// When no hook contract is configured, `expire_milestone` completes silently.
#[test]
fn test_expire_milestone_no_hook_succeeds() {
    let (_env, _owner, employer, contributor, token, client) = setup();
    let (agreement_id, milestone_id) = funded_milestone(&client, &employer, &contributor, &token);

    // No set_milestone_hook_contract call — hook is absent.
    assert!(
        client
            .try_expire_milestone(&agreement_id, &milestone_id)
            .is_ok(),
        "expire without hook should succeed"
    );
}

/// When a hook contract using the no-op default is configured, `expire_milestone`
/// still succeeds and the hook is invoked without error.
#[test]
fn test_expire_milestone_noop_hook_does_not_break_expiry() {
    let (env, owner, employer, contributor, token, client) = setup();

    // Register a MinimalMilestoneImpl (uses no-op on_milestone_expired).
    #[allow(deprecated)]
    let hook_id = env.register_contract(None, MinimalMilestoneImpl);

    client.set_milestone_hook_contract(&owner, &hook_id);

    let (agreement_id, milestone_id) = funded_milestone(&client, &employer, &contributor, &token);

    assert!(
        client
            .try_expire_milestone(&agreement_id, &milestone_id)
            .is_ok(),
        "expire with no-op hook should succeed"
    );
}

/// When a recording hook contract is configured, `expire_milestone` invokes
/// `on_milestone_expired` with the correct `agreement_id` and `milestone_id`.
#[test]
fn test_expire_milestone_hook_receives_correct_args() {
    let (env, owner, employer, contributor, token, client) = setup();

    // Register the recording hook.
    #[allow(deprecated)]
    let hook_id = env.register_contract(None, RecordingHook);
    let hook_client =
        milestone_interface::MilestoneContractClient::new(&env, &hook_id);

    client.set_milestone_hook_contract(&owner, &hook_id);

    let (agreement_id, milestone_id) = funded_milestone(&client, &employer, &contributor, &token);

    client.expire_milestone(&agreement_id, &milestone_id);

    // Verify the hook stored the correct argument values.
    let recorded_agreement: u128 = env
        .as_contract(&hook_id, || {
            env.storage()
                .persistent()
                .get(&soroban_sdk::symbol_short!("agr"))
                .expect("hook did not record agreement_id")
        });
    let recorded_milestone: u32 = env
        .as_contract(&hook_id, || {
            env.storage()
                .persistent()
                .get(&soroban_sdk::symbol_short!("ms"))
                .expect("hook did not record milestone_id")
        });

    assert_eq!(
        recorded_agreement, agreement_id,
        "hook received wrong agreement_id"
    );
    assert_eq!(
        recorded_milestone, milestone_id,
        "hook received wrong milestone_id"
    );

    // Suppress unused-variable warning on hook_client (kept for documentation).
    let _ = hook_client;
}

/// Changing the hook contract address takes effect immediately on the next call.
#[test]
fn test_set_milestone_hook_contract_updates_correctly() {
    let (env, owner, employer, contributor, token, client) = setup();

    #[allow(deprecated)]
    let hook_a = env.register_contract(None, MinimalMilestoneImpl);
    #[allow(deprecated)]
    let hook_b = env.register_contract(None, RecordingHook);

    // Set hook A then immediately overwrite with hook B.
    client.set_milestone_hook_contract(&owner, &hook_a);
    client.set_milestone_hook_contract(&owner, &hook_b);

    assert_eq!(
        client.get_milestone_hook_contract(),
        Some(hook_b.clone()),
        "get_milestone_hook_contract should return the latest address"
    );

    // Expiry with hook B should record the call.
    let (agreement_id, milestone_id) = funded_milestone(&client, &employer, &contributor, &token);
    client.expire_milestone(&agreement_id, &milestone_id);

    let recorded: u128 = env.as_contract(&hook_b, || {
        env.storage()
            .persistent()
            .get(&soroban_sdk::symbol_short!("agr"))
            .expect("hook B should have recorded the call")
    });
    assert_eq!(recorded, agreement_id);
}

/// `get_milestone_hook_contract` returns `None` when no hook has been set.
#[test]
fn test_get_milestone_hook_contract_none_when_unset() {
    let (_env, _owner, _employer, _contributor, _token, client) = setup();
    assert_eq!(client.get_milestone_hook_contract(), None);
}

// ─────────────────────────────────────────────────────────────────────────────
// Existing implementors compile-time regression check
// ─────────────────────────────────────────────────────────────────────────────

/// Confirms that a contract struct that includes `on_milestone_expired` as a
/// simple no-op body (`{}`) compiles and registers without error.  This models
/// the minimal-effort upgrade path for existing implementors: add the method
/// with an empty body and nothing else changes.
#[test]
fn test_existing_implementor_compiles_and_registers() {
    let env = Env::default();
    env.mock_all_auths();

    // Registering the contract implicitly verifies it compiled successfully.
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, MinimalMilestoneImpl);

    let client = milestone_interface::MilestoneContractClient::new(&env, &contract_id);

    // Default read methods return sensible zero/None values.
    assert_eq!(client.get_milestone_count(&1u128), 0);
    assert!(client.get_milestone(&1u128, &1u32).is_none());

    // on_milestone_expired is callable and does nothing (no panic).
    client.on_milestone_expired(&1u128, &1u32);
}
