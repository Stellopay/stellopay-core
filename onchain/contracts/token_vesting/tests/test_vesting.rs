#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    vec, Address, Env, IntoVal, Vec,
};
use token_vesting::{
    ClaimedEvent, CreatedEvent, CustomCheckpoint, EarlyReleaseEvent, RevokedEvent,
    TokenVestingContract, TokenVestingContractClient, VestingKind, VestingStatus,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn register_contract(env: &Env) -> TokenVestingContractClient<'static> {
    #[allow(deprecated)]
    let id = env.register_contract(None, TokenVestingContract);
    TokenVestingContractClient::new(env, &id)
}

fn create_token_contract<'a>(env: &Env, admin: &Address) -> TokenClient<'a> {
    let token_addr = env.register_stellar_asset_contract(admin.clone());
    TokenClient::new(env, &token_addr)
}

/// Shorthand to set the ledger timestamp.
fn set_time(env: &Env, ts: u64) {
    env.ledger().with_mut(|li| {
        li.timestamp = ts;
    });
}

/// Full setup: returns (client, owner, employer, beneficiary, token_client)
/// with 10 000 tokens minted to the employer.
fn full_setup(
    env: &Env,
) -> (
    TokenVestingContractClient<'static>,
    Address,
    Address,
    Address,
    TokenClient<'static>,
) {
    let client = register_contract(env);
    let owner = Address::generate(env);
    let employer = Address::generate(env);
    let beneficiary = Address::generate(env);
    let token_admin = Address::generate(env);
    let token = create_token_contract(env, &token_admin);

    let asset_admin = StellarAssetClient::new(env, &token.address);
    asset_admin.mint(&employer, &10_000i128);

    client.initialize(&owner);

    (client, owner, employer, beneficiary, token)
}

// ===========================================================================
// A. Initialization (3 tests)
// ===========================================================================

#[test]
fn initialize_and_owner() {
    let env = create_env();
    let client = register_contract(&env);
    let owner = Address::generate(&env);

    client.initialize(&owner);

    let stored = client.get_owner();
    assert_eq!(stored, Some(owner.clone()));

    // second initialize should fail
    let res = client.try_initialize(&owner);
    assert!(res.is_err());
}

#[test]
fn init_required_before_operations() {
    let env = create_env();
    let client = register_contract(&env);
    let employer = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let asset_admin = StellarAssetClient::new(&env, &token.address);
    asset_admin.mint(&employer, &1_000i128);

    // create_linear_schedule without initialize should fail
    let res = client.try_create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &600i128,
        &0u64,
        &60u64,
        &None,
        &true,
    );
    assert!(res.is_err());
}

#[test]
fn get_schedule_returns_none_for_missing_id() {
    let env = create_env();
    let client = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    assert_eq!(client.get_schedule(&999u128), None);
}

#[test]
fn get_owner_before_init_returns_none() {
    let env = create_env();
    let client = register_contract(&env);
    assert_eq!(client.get_owner(), None);
}

// ===========================================================================
// B. Linear vesting (7 tests)
// ===========================================================================

#[test]
fn linear_vesting_claim_flow() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &600i128,
        &0u64,
        &60u64,
        &None,
        &true,
    );

    // before start: nothing vested
    assert_eq!(client.get_vested_amount(&sid), 0);
    assert_eq!(client.get_releasable_amount(&sid), 0);

    // halfway at t=30: ~300 vested
    set_time(&env, 30);
    let vested = client.get_vested_amount(&sid);
    assert!(vested >= 290 && vested <= 310);

    // claim once
    let claimed = client.claim(&beneficiary, &sid);
    assert_eq!(claimed, client.get_vested_amount(&sid));

    // at end all vested and claimable remainder
    set_time(&env, 60);
    let remaining = client.get_releasable_amount(&sid);
    assert!(remaining > 0);
    let claimed2 = client.claim(&beneficiary, &sid);
    assert_eq!(claimed2, remaining);

    let schedule = client.get_schedule(&sid).unwrap();
    assert_eq!(schedule.status, VestingStatus::Completed);
}

#[test]
fn linear_at_exact_start_returns_zero() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 100);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &100u64,
        &200u64,
        &None,
        &false,
    );

    assert_eq!(client.get_vested_amount(&sid), 0);
}

#[test]
fn linear_one_second_after_start() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 100);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &100u64,
        &200u64,
        &None,
        &false,
    );

    set_time(&env, 101);
    let vested = client.get_vested_amount(&sid);
    // 1000 * 1 / 100 = 10
    assert_eq!(vested, 10);
}

#[test]
fn linear_at_end_returns_total() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 100);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &100u64,
        &200u64,
        &None,
        &false,
    );

    set_time(&env, 200);
    assert_eq!(client.get_vested_amount(&sid), 1_000);
}

#[test]
fn linear_past_end_capped_at_total() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 100);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &100u64,
        &200u64,
        &None,
        &false,
    );

    set_time(&env, 999);
    assert_eq!(client.get_vested_amount(&sid), 1_000);
}

#[test]
fn linear_with_cliff_before_cliff_returns_zero() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    // Linear 1000 over [0,100], cliff at 50
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &Some(50u64),
        &false,
    );

    // At t=25, would be 250 linearly but cliff blocks it
    set_time(&env, 25);
    assert_eq!(client.get_vested_amount(&sid), 0);
}

#[test]
fn linear_with_cliff_at_cliff_returns_proportional() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &Some(50u64),
        &false,
    );

    // At t=50 (cliff), linear kicks in: 1000 * 50/100 = 500
    set_time(&env, 50);
    assert_eq!(client.get_vested_amount(&sid), 500);
}

#[test]
fn linear_with_cliff_after_cliff_interpolates() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &Some(50u64),
        &false,
    );

    // At t=75, past cliff: 1000 * 75/100 = 750
    set_time(&env, 75);
    assert_eq!(client.get_vested_amount(&sid), 750);
}

// ===========================================================================
// C. Cliff vesting (4 tests)
// ===========================================================================

#[test]
fn cliff_vesting_and_revocation() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_cliff_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &400i128,
        &100u64,
        &true,
    );

    // before cliff: nothing vested
    assert_eq!(client.get_vested_amount(&sid), 0);

    // revoke before cliff: all refunded
    let refunded = client.revoke(&employer, &sid);
    assert_eq!(refunded, 400i128);

    let schedule = client.get_schedule(&sid).unwrap();
    assert_eq!(schedule.status, VestingStatus::Revoked);
}

#[test]
fn cliff_one_second_before_cliff_returns_zero() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_cliff_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &500i128,
        &100u64,
        &false,
    );

    set_time(&env, 99);
    assert_eq!(client.get_vested_amount(&sid), 0);
}

#[test]
fn cliff_at_exact_cliff_returns_total() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_cliff_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &500i128,
        &100u64,
        &false,
    );

    set_time(&env, 100);
    assert_eq!(client.get_vested_amount(&sid), 500);
}

#[test]
fn cliff_full_claim_after_cliff() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_cliff_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &500i128,
        &100u64,
        &false,
    );

    set_time(&env, 200);
    let claimed = client.claim(&beneficiary, &sid);
    assert_eq!(claimed, 500);

    let schedule = client.get_schedule(&sid).unwrap();
    assert_eq!(schedule.status, VestingStatus::Completed);
}

#[test]
fn cliff_exact_second_boundary() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_cliff_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &200u64,
        &false,
    );

    // One second before cliff: nothing vested
    set_time(&env, 199);
    assert_eq!(client.get_vested_amount(&sid), 0);

    // Exactly at cliff: full amount vests
    set_time(&env, 200);
    assert_eq!(client.get_vested_amount(&sid), 1_000);

    // One second after cliff: still total (no further linear accrual)
    set_time(&env, 201);
    assert_eq!(client.get_vested_amount(&sid), 1_000);
}

// ===========================================================================
// D. Custom schedule (3 tests)
// ===========================================================================

#[test]
fn custom_schedule_and_early_release() {
    let env = create_env();
    let (client, owner, employer, beneficiary, token) = full_setup(&env);

    let mut checkpoints = Vec::new(&env);
    checkpoints.push_back(CustomCheckpoint {
        time: 10,
        cumulative_amount: 100,
    });
    checkpoints.push_back(CustomCheckpoint {
        time: 20,
        cumulative_amount: 300,
    });
    checkpoints.push_back(CustomCheckpoint {
        time: 30,
        cumulative_amount: 500,
    });

    set_time(&env, 0);
    let sid = client.create_custom_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &500i128,
        &checkpoints,
        &true,
    );

    // at t=15, second checkpoint not reached, so 100 vested
    set_time(&env, 15);
    assert_eq!(client.get_vested_amount(&sid), 100);

    // admin can approve early release of remaining unvested portion
    let early = client.approve_early_release(&owner, &sid, &200i128);
    assert_eq!(early, 200i128);

    let schedule = client.get_schedule(&sid).unwrap();
    assert!(schedule.released_amount >= 200);
}

#[test]
fn custom_before_first_checkpoint_returns_zero() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    let mut checkpoints = Vec::new(&env);
    checkpoints.push_back(CustomCheckpoint {
        time: 50,
        cumulative_amount: 200,
    });
    checkpoints.push_back(CustomCheckpoint {
        time: 100,
        cumulative_amount: 500,
    });

    set_time(&env, 0);
    let sid = client.create_custom_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &500i128,
        &checkpoints,
        &false,
    );

    set_time(&env, 10);
    assert_eq!(client.get_vested_amount(&sid), 0);
}

#[test]
fn custom_between_checkpoints() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    let mut checkpoints = Vec::new(&env);
    checkpoints.push_back(CustomCheckpoint {
        time: 50,
        cumulative_amount: 200,
    });
    checkpoints.push_back(CustomCheckpoint {
        time: 100,
        cumulative_amount: 500,
    });

    set_time(&env, 0);
    let sid = client.create_custom_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &500i128,
        &checkpoints,
        &false,
    );

    // At t=75 — past first checkpoint, before second
    set_time(&env, 75);
    assert_eq!(client.get_vested_amount(&sid), 200);
}

#[test]
fn custom_at_final_checkpoint() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    let mut checkpoints = Vec::new(&env);
    checkpoints.push_back(CustomCheckpoint {
        time: 50,
        cumulative_amount: 200,
    });
    checkpoints.push_back(CustomCheckpoint {
        time: 100,
        cumulative_amount: 500,
    });

    set_time(&env, 0);
    let sid = client.create_custom_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &500i128,
        &checkpoints,
        &false,
    );

    set_time(&env, 100);
    assert_eq!(client.get_vested_amount(&sid), 500);
}

// ===========================================================================
// E. Claim security (5 tests)
// ===========================================================================

#[test]
fn claim_non_beneficiary_fails() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_cliff_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &500i128,
        &100u64,
        &false,
    );

    set_time(&env, 200);
    let stranger = Address::generate(&env);
    let res = client.try_claim(&stranger, &sid);
    assert!(res.is_err());
}

#[test]
fn double_claim_same_timestamp_fails() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &None,
        &false,
    );

    set_time(&env, 50);
    let _first = client.claim(&beneficiary, &sid);

    // Second claim at same timestamp — nothing left to claim
    let res = client.try_claim(&beneficiary, &sid);
    assert!(res.is_err());
}

#[test]
fn claim_on_completed_schedule_fails() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_cliff_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &500i128,
        &10u64,
        &false,
    );

    set_time(&env, 10);
    client.claim(&beneficiary, &sid);

    // Schedule is now Completed; second claim should fail
    let res = client.try_claim(&beneficiary, &sid);
    assert!(res.is_err());
}

#[test]
fn released_amount_accumulates_correctly() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &None,
        &false,
    );

    set_time(&env, 25);
    let c1 = client.claim(&beneficiary, &sid);

    set_time(&env, 50);
    let c2 = client.claim(&beneficiary, &sid);

    set_time(&env, 100);
    let c3 = client.claim(&beneficiary, &sid);

    assert_eq!(c1 + c2 + c3, 1_000);

    let schedule = client.get_schedule(&sid).unwrap();
    assert_eq!(schedule.released_amount, 1_000);
    assert_eq!(schedule.status, VestingStatus::Completed);
}

#[test]
fn claim_verifies_token_balances() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    let employer_before = token.balance(&employer);

    set_time(&env, 0);
    let sid = client.create_cliff_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &500i128,
        &50u64,
        &false,
    );

    // Employer balance decreased by escrow
    assert_eq!(token.balance(&employer), employer_before - 500);

    set_time(&env, 50);
    client.claim(&beneficiary, &sid);

    assert_eq!(token.balance(&beneficiary), 500);
}

// ===========================================================================
// F. Revocation (4 tests)
// ===========================================================================

#[test]
fn revoke_non_revocable_fails() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &None,
        &false, // not revocable
    );

    set_time(&env, 50);
    let res = client.try_revoke(&employer, &sid);
    assert!(res.is_err());
}

#[test]
fn revoke_non_employer_fails() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &None,
        &true,
    );

    set_time(&env, 50);
    let stranger = Address::generate(&env);
    let res = client.try_revoke(&stranger, &sid);
    assert!(res.is_err());
}

#[test]
fn double_revoke_fails() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &None,
        &true,
    );

    set_time(&env, 50);
    client.revoke(&employer, &sid);

    // Second revoke: schedule is no longer Active
    let res = client.try_revoke(&employer, &sid);
    assert!(res.is_err());
}

#[test]
fn revoke_partial_vesting_splits_correctly() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    let employer_before = token.balance(&employer);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &None,
        &true,
    );

    // Employer escrowed 1000
    assert_eq!(token.balance(&employer), employer_before - 1_000);

    set_time(&env, 50);
    let refunded = client.revoke(&employer, &sid);
    // ~500 vested, ~500 refunded
    assert!(refunded >= 490 && refunded <= 510);

    // Employer got refund
    let employer_after = token.balance(&employer);
    assert_eq!(employer_after, employer_before - 1_000 + refunded);

    // Beneficiary can still claim vested portion
    let claimed = client.claim(&beneficiary, &sid);
    assert_eq!(claimed, 1_000 - refunded);
    assert_eq!(token.balance(&beneficiary), claimed);
}

// ===========================================================================
// G. Early release (3 tests)
// ===========================================================================

#[test]
fn early_release_non_owner_fails() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &None,
        &false,
    );

    set_time(&env, 50);
    let stranger = Address::generate(&env);
    let res = client.try_approve_early_release(&stranger, &sid, &100i128);
    assert!(res.is_err());
}

#[test]
fn early_release_capped_at_unvested() {
    let env = create_env();
    let (client, owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &None,
        &false,
    );

    // At t=80, 800 vested, 200 unvested. Request 500 → capped at 200.
    set_time(&env, 80);
    let released = client.approve_early_release(&owner, &sid, &500i128);
    assert_eq!(released, 200);
}

#[test]
fn early_release_on_revoked_schedule_fails() {
    let env = create_env();
    let (client, owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &None,
        &true,
    );

    set_time(&env, 50);
    client.revoke(&employer, &sid);

    let res = client.try_approve_early_release(&owner, &sid, &100i128);
    assert!(res.is_err());
}

// ===========================================================================
// H. State consistency (2 tests)
// ===========================================================================

#[test]
fn claim_after_revoke_gets_vested_remainder() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &None,
        &true,
    );

    // At t=40, revoke — 400 vested, 600 refunded
    set_time(&env, 40);
    let refunded = client.revoke(&employer, &sid);
    assert_eq!(refunded, 600);

    // Even at t=999, beneficiary can only claim the 400 frozen at revocation
    set_time(&env, 999);
    let claimed = client.claim(&beneficiary, &sid);
    assert_eq!(claimed, 400);
}

#[test]
fn schedule_ids_are_sequential() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);

    let id1 = client.create_cliff_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &100i128,
        &10u64,
        &false,
    );
    let id2 = client.create_cliff_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &100i128,
        &10u64,
        &false,
    );
    let id3 = client.create_cliff_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &100i128,
        &10u64,
        &false,
    );

    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert_eq!(id3, 3);
}

// ===========================================================================
// I. Input validation (5 tests)
// ===========================================================================

#[test]
fn create_linear_zero_amount_fails() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let res = client.try_create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &0i128,
        &0u64,
        &100u64,
        &None,
        &false,
    );
    assert!(res.is_err());
}

#[test]
fn create_linear_end_before_start_fails() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let res = client.try_create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &100u64,
        &50u64, // end < start
        &None,
        &false,
    );
    assert!(res.is_err());
}

#[test]
fn create_linear_cliff_outside_range_fails() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let res = client.try_create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &100u64,
        &200u64,
        &Some(300u64), // cliff > end
        &false,
    );
    assert!(res.is_err());
}

#[test]
fn create_custom_empty_checkpoints_fails() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    let checkpoints: Vec<CustomCheckpoint> = Vec::new(&env);

    set_time(&env, 0);
    let res = client.try_create_custom_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &500i128,
        &checkpoints,
        &false,
    );
    assert!(res.is_err());
}

#[test]
fn create_custom_unsorted_checkpoints_fails() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    let mut checkpoints = Vec::new(&env);
    checkpoints.push_back(CustomCheckpoint {
        time: 100,
        cumulative_amount: 300,
    });
    checkpoints.push_back(CustomCheckpoint {
        time: 50, // out of order
        cumulative_amount: 500,
    });

    set_time(&env, 0);
    let res = client.try_create_custom_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &500i128,
        &checkpoints,
        &false,
    );
    assert!(res.is_err());
}

// ===========================================================================
// J. Additional edge cases (3 tests)
// ===========================================================================

#[test]
fn linear_minimal_duration_vests_correctly() {
    // Tightest valid window (1-second duration). Exercises the duration == 0
    // guard neighbourhood and confirms no off-by-one at boundaries.
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 10);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &10u64,
        &11u64,
        &None,
        &false,
    );

    // At start: 0
    assert_eq!(client.get_vested_amount(&sid), 0);

    // At end: full amount
    set_time(&env, 11);
    assert_eq!(client.get_vested_amount(&sid), 1_000);
}

#[test]
fn custom_vested_never_exceeds_total() {
    // compute_vested_amount caps at total_amount. Verify with a single
    // checkpoint well in the past.
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    let mut checkpoints = Vec::new(&env);
    checkpoints.push_back(CustomCheckpoint {
        time: 10,
        cumulative_amount: 500,
    });

    set_time(&env, 0);
    let sid = client.create_custom_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &500i128,
        &checkpoints,
        &false,
    );

    set_time(&env, 9999);
    assert_eq!(client.get_vested_amount(&sid), 500);
}

#[test]
fn claim_invalid_schedule_id_fails() {
    let env = create_env();
    let (client, _owner, _employer, beneficiary, _token) = full_setup(&env);

    set_time(&env, 100);
    let res = client.try_claim(&beneficiary, &999u128);
    assert!(res.is_err());
}

// ===========================================================================
// K. Events (4 tests)
// ===========================================================================

#[test]
fn test_create_event_emitted() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 100);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &100u64,
        &200u64,
        &None,
        &true,
    );

    let events = env.events().all();
    let last_event = events.last().unwrap();

    // Topics: ("vesting_created", sid)
    assert_eq!(last_event.0, client.address);
    assert_eq!(
        last_event.1,
        vec![
            &env,
            soroban_sdk::String::from_str(&env, "vesting_created").into_val(&env),
            sid.into_val(&env)
        ]
    );

    // Data should be CreatedEvent
    let event: CreatedEvent = last_event.2.into_val(&env);
    assert_eq!(event.id, sid);
    assert_eq!(event.employer, employer);
    assert_eq!(event.beneficiary, beneficiary);
    assert_eq!(event.amount, 1_000);
    assert_eq!(event.kind, VestingKind::Linear);
}

#[test]
fn test_claim_event_emitted() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &None,
        &false,
    );

    set_time(&env, 50);
    client.claim(&beneficiary, &sid);

    let events = env.events().all();
    let last_event = events.last().unwrap();

    assert_eq!(
        last_event.1,
        vec![
            &env,
            soroban_sdk::String::from_str(&env, "vesting_claimed").into_val(&env),
            sid.into_val(&env)
        ]
    );
    let event: ClaimedEvent = last_event.2.into_val(&env);
    assert_eq!(event.id, sid);
    assert_eq!(event.beneficiary, beneficiary);
    assert_eq!(event.amount, 500);
}

#[test]
fn test_revoke_event_emitted() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_cliff_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &400i128,
        &100u64,
        &true,
    );

    set_time(&env, 50);
    client.revoke(&employer, &sid);

    let events = env.events().all();
    let last_event = events.last().unwrap();

    assert_eq!(
        last_event.1,
        vec![
            &env,
            soroban_sdk::String::from_str(&env, "vesting_revoked").into_val(&env),
            sid.into_val(&env)
        ]
    );
    let event: RevokedEvent = last_event.2.into_val(&env);
    assert_eq!(event.id, sid);
    assert_eq!(event.refunded, 400);
    assert_eq!(event.at, 50);
}

#[test]
fn test_early_release_event_emitted() {
    let env = create_env();
    let (client, owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_cliff_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &500i128,
        &100u64,
        &true,
    );

    client.approve_early_release(&owner, &sid, &200i128);

    let events = env.events().all();
    let last_event = events.last().unwrap();

    assert_eq!(
        last_event.1,
        vec![
            &env,
            soroban_sdk::String::from_str(&env, "vesting_early_release").into_val(&env),
            sid.into_val(&env)
        ]
    );
    let event: EarlyReleaseEvent = last_event.2.into_val(&env);
    assert_eq!(event.id, sid);
    assert_eq!(event.amount, 200);
}

// ===========================================================================
// L. Cliff + Linear interaction property tests (added for #516)
// ===========================================================================

/// Verifies that a linear schedule with a cliff correctly gates vesting:
/// - Before both start and cliff: 0
/// - After start but before cliff: 0 (cliff blocks)
/// - At cliff: linear interpolation from start to cliff
/// - Between cliff and end: linear interpolation
/// - At end: total
/// - After end: capped at total
#[test]
fn cliff_plus_linear_full_spectrum() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &Some(30u64),
        &false,
    );

    let cases = [
        (0, 0),  // before start + cliff
        (10, 0), // after start, before cliff
        (20, 0),
        (30, 300), // exactly at cliff -> 1000 * 30/100 = 300
        (40, 400),
        (50, 500),
        (60, 600),
        (70, 700),
        (80, 800),
        (90, 900),
        (100, 1000),
        (110, 1000),
    ];

    for &(ts, expected) in &cases {
        set_time(&env, ts);
        let vested = client.get_vested_amount(&sid);
        assert_eq!(
            vested, expected,
            "cliff+linear vested amount mismatch at t={}: got {}, expected {}",
            ts, vested, expected
        );
    }
}

/// Edge case: cliff equals end_time (no linear segment after cliff).
#[test]
fn cliff_equals_end() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &50u64,
        &Some(50u64),
        &false,
    );

    set_time(&env, 0);
    assert_eq!(client.get_vested_amount(&sid), 0);

    set_time(&env, 50);
    assert_eq!(client.get_vested_amount(&sid), 1_000);
}

/// Edge case: cliff equals start_time (effectively no cliff gate).
#[test]
fn cliff_equals_start() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &30u64,
        &100u64,
        &Some(30u64),
        &false,
    );

    set_time(&env, 30);
    assert_eq!(client.get_vested_amount(&sid), 0);

    set_time(&env, 65);
    // 1000 * (65-30) / (100-30) = 1000 * 35 / 70 = 500
    assert_eq!(client.get_vested_amount(&sid), 500);
}

/// Edge case: very small total_amount with cliff.
#[test]
fn cliff_plus_linear_small_amount() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &3i128,
        &0u64,
        &100u64,
        &Some(50u64),
        &false,
    );

    set_time(&env, 50);
    // 3 * 50/100 = 1.5 -> integer 1
    assert_eq!(client.get_vested_amount(&sid), 1);

    set_time(&env, 100);
    assert_eq!(client.get_vested_amount(&sid), 3);
}

/// Edge case: revoked linear schedule with cliff.
#[test]
fn cliff_plus_linear_revoked() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &Some(30u64),
        &true,
    );

    set_time(&env, 20);
    client.revoke(&employer, &sid);

    set_time(&env, 999);
    assert_eq!(client.get_vested_amount(&sid), 0);
    assert_eq!(client.get_releasable_amount(&sid), 0);
}

// ===========================================================================
// L. Overflow safety tests (3 tests)
// ===========================================================================

/// Test that large total_amount near i128::MAX does not cause overflow panic.
#[test]
fn linear_large_total_no_overflow() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    // Mint enough tokens for the large amount
    let asset_admin = StellarAssetClient::new(&env, &token.address);
    let large_total = i128::MAX / 2;
    asset_admin.mint(&employer, &large_total);

    set_time(&env, 0);
    // Use a large total_amount that would overflow with naive multiplication
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &large_total,
        &0u64,
        &100u64,
        &None,
        &false,
    );

    // At midpoint: should vest approximately half without overflow
    set_time(&env, 50);
    let vested = client.get_vested_amount(&sid);
    // Should be approximately half (truncates toward zero)
    assert!(vested > 0 && vested <= large_total);

    // At end: should vest full amount
    set_time(&env, 100);
    assert_eq!(client.get_vested_amount(&sid), large_total);
}

/// Test that long duration with large total_amount does not overflow.
#[test]
fn linear_long_duration_no_overflow() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    // Mint enough tokens for the large amount
    let asset_admin = StellarAssetClient::new(&env, &token.address);
    let large_total = i128::MAX / 10;
    asset_admin.mint(&employer, &large_total);

    set_time(&env, 0);
    // Large total with long duration (simulating years)
    let long_duration = u64::MAX / 100; // Very long duration
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &large_total,
        &0u64,
        &long_duration,
        &None,
        &false,
    );

    // At various points, should not overflow
    set_time(&env, long_duration / 4);
    let vested_quarter = client.get_vested_amount(&sid);
    assert!(vested_quarter > 0 && vested_quarter <= large_total);

    set_time(&env, long_duration / 2);
    let vested_half = client.get_vested_amount(&sid);
    assert!(vested_half > vested_quarter && vested_half <= large_total);

    set_time(&env, long_duration);
    assert_eq!(client.get_vested_amount(&sid), large_total);
}

/// Test that vested amount never exceeds total_amount even with edge cases.
#[test]
fn linear_vested_never_exceeds_total() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    // Mint enough tokens for the large amount
    let asset_admin = StellarAssetClient::new(&env, &token.address);
    let total = 1_000_000i128;
    asset_admin.mint(&employer, &total);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &total,
        &0u64,
        &100u64,
        &None,
        &false,
    );

    // Check at multiple points that vested <= total
    for t in [0u64, 10, 25, 50, 75, 90, 99, 100, 200] {
        set_time(&env, t);
        let vested = client.get_vested_amount(&sid);
        assert!(
            vested <= total,
            "Vested amount {} exceeds total {} at time {}",
            vested,
            total,
            t
        );
    }

    // At end, should equal total
    set_time(&env, 100);
    assert_eq!(client.get_vested_amount(&sid), total);
}

// ===========================================================================
// N. Early release bounded by releasable amount (issue #884)
// ===========================================================================

/// Verifies that approve_early_release is capped at the unvested remainder
/// even when a prior claim has already been made against the schedule.
///
/// Regression: without the cap, the owner could approve an early release that
/// exceeds what get_releasable_amount (plus already-released early portion)
/// actually allows. The contract's unvested_remaining guard protects against
/// over-release. This test proves the cap holds after a prior claim.
#[test]
fn early_release_capped_after_prior_claim() {
    let env = create_env();
    let (client, owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &None,
        &false,
    );

    // At t=50: 500 vested, 500 unvested → releasable = 500
    set_time(&env, 50);
    assert_eq!(client.get_releasable_amount(&sid), 500);

    // Claim the vested 500
    let claimed = client.claim(&beneficiary, &sid);
    assert_eq!(claimed, 500);

    // After claim: released=500, vested=500, releasable=0
    assert_eq!(client.get_releasable_amount(&sid), 0);

    // Request early release for 600 (more than 500 unvested remaining)
    // Must be capped at 500 (total - vested = 1000 - 500 = 500)
    let early = client.approve_early_release(&owner, &sid, &600i128);
    assert_eq!(early, 500);

    // Schedule completed: 500 claimed + 500 early-released = 1000 = total
    let schedule = client.get_schedule(&sid).unwrap();
    assert_eq!(schedule.status, VestingStatus::Completed);
    assert_eq!(schedule.released_amount, 1_000);

    // Beneficiary received exactly 500 (claimed) + 500 (early) = 1000
    assert_eq!(token.balance(&beneficiary), 1_000);
}

/// Verifies that a correctly bounded early-release approval followed by a
/// claim transfers exactly the expected amounts (early portion + vested
/// remainder), confirming no double-counting or fund leakage.
///
/// Contract behavior:
///   - `approve_early_release` increments `released_amount` (and transfers to beneficiary).
///   - `get_releasable_amount` returns `vested - released_amount`.
///   - So after an early release of 200 at t=30 (vested=300): releasable = 300 - 200 = 100
///   - The beneficiary can only claim that 100 remaining vested-but-unreleased amount.
#[test]
fn early_release_then_claim_transfers_exact_amounts() {
    let env = create_env();
    let (client, owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &None,
        &false,
    );

    // At t=30: 300 vested, 700 unvested → releasable = 300
    set_time(&env, 30);
    assert_eq!(client.get_vested_amount(&sid), 300);
    assert_eq!(client.get_releasable_amount(&sid), 300);

    // Approve early release of 200 (within the 700 unvested remainder).
    // The early release is transferred directly to the beneficiary.
    let early = client.approve_early_release(&owner, &sid, &200i128);
    assert_eq!(early, 200);

    // After early release: released_amount=200, vested=300, so releasable = 100.
    // (releasable = vested - released_amount = 300 - 200 = 100)
    assert_eq!(client.get_releasable_amount(&sid), 100);

    // Claim the remaining releasable 100
    let claimed = client.claim(&beneficiary, &sid);
    assert_eq!(claimed, 100);

    // Total received: 200 (early) + 100 (claim) = 300
    assert_eq!(token.balance(&beneficiary), 300);

    // Schedule still active: released_amount=300, total=1000
    let schedule = client.get_schedule(&sid).unwrap();
    assert_eq!(schedule.status, VestingStatus::Active);
    assert_eq!(schedule.released_amount, 300);
}

// ===========================================================================
// O. Monotonicity property tests (issue #886)
//
// Core invariant: for any non-decreasing sequence of timestamps t0 <= t1 <= …
// the value returned by get_vested_amount (and compute_vested_amount) must
// also be non-decreasing:
//
//   t_i <= t_j  ⟹  vested(t_i) <= vested(t_j)
//
// This is a fundamental safety property: a beneficiary's accrued entitlement
// must never decrease as time advances.  Regression against this invariant
// would allow, for example, re-claiming already-paid tokens after a ledger
// rewind scenario or smart-contract logic error.
//
// Implementation note:
//   Soroban's no_std environment does not support `proptest` or `quickcheck`.
//   We generate deterministic pseudo-random timestamp sequences using a
//   seeded 64-bit LCG (Numerical Recipes constants) so the test corpus is
//   reproducible, covers a wide range of timestamp distributions, and
//   requires no external crates.
// ===========================================================================

// ---------------------------------------------------------------------------
// LCG helper — deterministic pseudo-random u64 sequence.
// Constants from Numerical Recipes (Press et al., 3rd ed.).
// ---------------------------------------------------------------------------

/// Advances a 64-bit LCG state and returns the new state.
/// `state` is the full 64-bit register; callers should seed with a non-zero value.
fn lcg_next(state: u64) -> u64 {
    state
        .wrapping_mul(6_364_136_223_846_793_005_u64)
        .wrapping_add(1_442_695_040_888_963_407_u64)
}

/// Generates a sorted (non-decreasing), deduplicated sequence of `n` timestamps
/// sampled from `[lo, hi]` using an LCG seeded with `seed`.
///
/// The first element is always `lo` and the last is always `hi` so that edge
/// boundaries are always exercised.
fn gen_timestamps(seed: u64, lo: u64, hi: u64, n: usize) -> std::vec::Vec<u64> {
    assert!(hi >= lo, "hi must be >= lo");
    assert!(n >= 2, "need at least 2 timestamps");

    let range = hi - lo;
    let mut state = seed;
    let mut ts: std::vec::Vec<u64> = std::vec::Vec::with_capacity(n);

    // Always include boundaries so that pre-start, at-start, at-end and
    // post-end semantics are all exercised.
    ts.push(lo);
    ts.push(hi);

    for _ in 2..n {
        state = lcg_next(state);
        let offset = if range == 0 { 0 } else { state % (range + 1) };
        ts.push(lo + offset);
    }

    ts.sort_unstable();
    ts
}

// ---------------------------------------------------------------------------
// O-1. Linear schedule monotonicity
// ---------------------------------------------------------------------------

/// Property: vested amount is non-decreasing across a pseudo-random sequence
/// of 50 timestamps spanning the full lifecycle of a linear schedule (before
/// start, within the vesting window, and beyond end_time).
///
/// Repeats for 10 distinct LCG seeds to widen coverage without blowing up
/// test runtime.
#[test]
fn prop_linear_vested_amount_is_monotonic() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    // Schedule parameters: 1 000 tokens vesting linearly over [100, 500].
    let total: i128 = 1_000;
    let start: u64 = 100;
    let end: u64 = 500;

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &total,
        &start,
        &end,
        &None,
        &false,
    );

    // Run the property check for 10 independent seeds.
    for seed in [
        0xDEAD_BEEF_u64,
        0x1234_5678,
        0xCAFE_BABE,
        0x0000_0001,
        0xFFFF_FFFF,
        0xA5A5_A5A5,
        0x1111_1111,
        0x8888_8888,
        0xFEDC_BA98,
        0x0102_0304,
    ] {
        // Sample from a wider window [0, end + 200] to capture pre/post
        // boundaries and ensure the "beyond end" cap is exercised.
        let timestamps = gen_timestamps(seed, 0, end + 200, 50);

        let mut prev_vested: i128 = -1; // sentinel
        for &ts in &timestamps {
            set_time(&env, ts);
            let vested = client.get_vested_amount(&sid);

            if prev_vested >= 0 {
                assert!(
                    vested >= prev_vested,
                    "[Linear monotonicity] seed={:#x} t={}: vested {} < prev {}",
                    seed,
                    ts,
                    vested,
                    prev_vested,
                );
            }

            // Sanity: must always be within [0, total].
            assert!(
                vested >= 0 && vested <= total,
                "[Linear monotonicity] vested {} out of [0, {}] at t={}",
                vested,
                total,
                ts,
            );

            prev_vested = vested;
        }
    }
}

/// Property: linear schedule WITH a cliff is non-decreasing across timestamps
/// that deliberately straddle the cliff boundary (timestamps below, at, and
/// above the cliff are all sampled in the sequence).
#[test]
fn prop_linear_with_cliff_vested_amount_is_monotonic() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    // 2 000 tokens, linear [0, 1000], cliff at 400.
    let total: i128 = 2_000;
    let start: u64 = 0;
    let end: u64 = 1_000;
    let cliff: u64 = 400;

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &total,
        &start,
        &end,
        &Some(cliff),
        &false,
    );

    for seed in [
        0xBEEF_CAFE_u64,
        0x2345_6789,
        0x9876_5432,
        0xAAAA_BBBB,
        0x5A5A_5A5A,
    ] {
        // Sample from [0, end + 100] — ensures cliff straddle and post-end cap.
        let timestamps = gen_timestamps(seed, 0, end + 100, 60);

        let mut prev_vested: i128 = -1;
        for &ts in &timestamps {
            set_time(&env, ts);
            let vested = client.get_vested_amount(&sid);

            if prev_vested >= 0 {
                assert!(
                    vested >= prev_vested,
                    "[Linear+cliff monotonicity] seed={:#x} t={}: vested {} < prev {}",
                    seed,
                    ts,
                    vested,
                    prev_vested,
                );
            }

            assert!(
                vested >= 0 && vested <= total,
                "[Linear+cliff monotonicity] vested {} out of [0, {}] at t={}",
                vested,
                total,
                ts,
            );

            prev_vested = vested;
        }
    }
}

/// Regression: monotonicity must hold for every individual step in the
/// timestamp sequence, including single-step transitions from "just below
/// cliff" to "just at cliff".
#[test]
fn prop_linear_cliff_boundary_monotonic() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    // Tight window around the cliff to stress-test the boundary.
    let total: i128 = 1_000;
    let start: u64 = 0;
    let end: u64 = 100;
    let cliff: u64 = 50;

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &total,
        &start,
        &end,
        &Some(cliff),
        &false,
    );

    // Dense sweep ±2 around the cliff boundary (48..52) plus full range.
    let check_points: std::vec::Vec<u64> = (0u64..=110).collect();

    let mut prev: i128 = -1;
    for &ts in &check_points {
        set_time(&env, ts);
        let vested = client.get_vested_amount(&sid);

        if prev >= 0 {
            assert!(
                vested >= prev,
                "[Linear cliff boundary] t={}: vested {} decreased from {}",
                ts,
                vested,
                prev,
            );
        }
        prev = vested;
    }
}

// ---------------------------------------------------------------------------
// O-2. Cliff schedule monotonicity
// ---------------------------------------------------------------------------

/// Property: a pure cliff schedule's vested amount is non-decreasing — it is
/// 0 before `cliff_time` and jumps to `total_amount` at the cliff, never
/// decreasing afterwards.
#[test]
fn prop_cliff_vested_amount_is_monotonic() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    let total: i128 = 500;
    let cliff_ts: u64 = 200;

    set_time(&env, 0);
    let sid = client.create_cliff_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &total,
        &cliff_ts,
        &false,
    );

    for seed in [
        0x1357_2468_u64,
        0xFADE_D00D,
        0x0BAD_C0DE,
        0x1001_0010,
        0xC0C0_C0C0,
    ] {
        // Sample from [0, cliff + 300] — captures pre-cliff, exact-cliff,
        // and post-cliff regions.
        let timestamps = gen_timestamps(seed, 0, cliff_ts + 300, 50);

        let mut prev_vested: i128 = -1;
        for &ts in &timestamps {
            set_time(&env, ts);
            let vested = client.get_vested_amount(&sid);

            if prev_vested >= 0 {
                assert!(
                    vested >= prev_vested,
                    "[Cliff monotonicity] seed={:#x} t={}: vested {} < prev {}",
                    seed,
                    ts,
                    vested,
                    prev_vested,
                );
            }

            assert!(
                vested == 0 || vested == total,
                "[Cliff monotonicity] vested {} is not 0 or total={} at t={}",
                vested,
                total,
                ts,
            );

            prev_vested = vested;
        }
    }
}

/// Dense sweep through the one-second transition window around cliff_time to
/// confirm the step is monotonic (0 → total, never total → 0).
#[test]
fn prop_cliff_boundary_step_is_monotonic() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    let total: i128 = 750;
    let cliff_ts: u64 = 1_000;

    set_time(&env, 0);
    let sid = client.create_cliff_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &total,
        &cliff_ts,
        &false,
    );

    let mut prev: i128 = -1;
    // Sweep [995..1005] — straddles the cliff boundary with single-second steps.
    for ts in 995u64..=1_005 {
        set_time(&env, ts);
        let vested = client.get_vested_amount(&sid);

        if prev >= 0 {
            assert!(
                vested >= prev,
                "[Cliff boundary] t={}: vested {} decreased from {}",
                ts,
                vested,
                prev,
            );
        }
        prev = vested;
    }

    // Verify the exact values at key points.
    set_time(&env, cliff_ts - 1);
    assert_eq!(client.get_vested_amount(&sid), 0);
    set_time(&env, cliff_ts);
    assert_eq!(client.get_vested_amount(&sid), total);
}

// ---------------------------------------------------------------------------
// O-3. Custom schedule monotonicity
// ---------------------------------------------------------------------------

/// Property: a custom step-function schedule's vested amount is non-decreasing
/// across pseudo-random timestamps that straddle each checkpoint boundary.
///
/// The schedule uses three checkpoints, so the interesting regions are:
///   [0, cp1.time)  →  0 vested
///   [cp1.time, cp2.time)  →  cp1.cumulative_amount vested
///   [cp2.time, cp3.time)  →  cp2.cumulative_amount vested
///   [cp3.time, ∞)  →  total vested
#[test]
fn prop_custom_vested_amount_is_monotonic() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    let total: i128 = 900;
    let mut checkpoints = soroban_sdk::Vec::new(&env);
    checkpoints.push_back(CustomCheckpoint {
        time: 100,
        cumulative_amount: 300,
    });
    checkpoints.push_back(CustomCheckpoint {
        time: 200,
        cumulative_amount: 600,
    });
    checkpoints.push_back(CustomCheckpoint {
        time: 300,
        cumulative_amount: 900,
    });

    set_time(&env, 0);
    let sid = client.create_custom_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &total,
        &checkpoints,
        &false,
    );

    for seed in [
        0xDECA_FBAD_u64,
        0x1234_ABCD,
        0x5678_EF01,
        0xABCD_1234,
        0x0F0F_0F0F,
        0xF0F0_F0F0,
        0x5555_AAAA,
        0xAAAA_5555,
    ] {
        // Sample from [0, 400] — covers all checkpoint regions plus post-end.
        let timestamps = gen_timestamps(seed, 0, 400, 60);

        let mut prev_vested: i128 = -1;
        for &ts in &timestamps {
            set_time(&env, ts);
            let vested = client.get_vested_amount(&sid);

            if prev_vested >= 0 {
                assert!(
                    vested >= prev_vested,
                    "[Custom monotonicity] seed={:#x} t={}: vested {} < prev {}",
                    seed,
                    ts,
                    vested,
                    prev_vested,
                );
            }

            assert!(
                vested >= 0 && vested <= total,
                "[Custom monotonicity] vested {} out of [0, {}] at t={}",
                vested,
                total,
                ts,
            );

            prev_vested = vested;
        }
    }
}

/// Dense sweep through each checkpoint boundary confirms the step-function
/// transitions are always non-decreasing (never step downward).
#[test]
fn prop_custom_checkpoint_boundaries_monotonic() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    let total: i128 = 600;
    let mut checkpoints = soroban_sdk::Vec::new(&env);
    checkpoints.push_back(CustomCheckpoint {
        time: 50,
        cumulative_amount: 200,
    });
    checkpoints.push_back(CustomCheckpoint {
        time: 100,
        cumulative_amount: 400,
    });
    checkpoints.push_back(CustomCheckpoint {
        time: 150,
        cumulative_amount: 600,
    });

    set_time(&env, 0);
    let sid = client.create_custom_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &total,
        &checkpoints,
        &false,
    );

    // Dense sweep [0..200] with single-second steps.
    let mut prev: i128 = -1;
    for ts in 0u64..=200 {
        set_time(&env, ts);
        let vested = client.get_vested_amount(&sid);

        if prev >= 0 {
            assert!(
                vested >= prev,
                "[Custom boundary] t={}: vested {} decreased from {}",
                ts,
                vested,
                prev,
            );
        }
        prev = vested;
    }

    // Spot-check expected step values.
    let cases: &[(u64, i128)] = &[
        (0, 0),
        (49, 0),
        (50, 200),
        (99, 200),
        (100, 400),
        (149, 400),
        (150, 600),
        (999, 600),
    ];
    for &(ts, expected) in cases {
        set_time(&env, ts);
        assert_eq!(
            client.get_vested_amount(&sid),
            expected,
            "[Custom boundary] unexpected vested at t={}",
            ts,
        );
    }
}

/// Property: a custom schedule with a single checkpoint is monotonic — 0
/// before the checkpoint, `total_amount` at and after.
#[test]
fn prop_custom_single_checkpoint_is_monotonic() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    let total: i128 = 300;
    let mut checkpoints = soroban_sdk::Vec::new(&env);
    checkpoints.push_back(CustomCheckpoint {
        time: 77,
        cumulative_amount: 300,
    });

    set_time(&env, 0);
    let sid = client.create_custom_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &total,
        &checkpoints,
        &false,
    );

    let timestamps = gen_timestamps(0xBAAD_F00D, 0, 200, 40);
    let mut prev: i128 = -1;
    for &ts in &timestamps {
        set_time(&env, ts);
        let vested = client.get_vested_amount(&sid);

        if prev >= 0 {
            assert!(
                vested >= prev,
                "[Custom single-cp] t={}: vested {} < prev {}",
                ts,
                vested,
                prev,
            );
        }
        prev = vested;
    }
}

// ---------------------------------------------------------------------------
// O-4. Cross-kind monotonicity with large / extreme values
// ---------------------------------------------------------------------------

/// Property: monotonicity holds for a linear schedule with total near i128::MAX.
/// The overflow-safe interpolation in compute_vested_amount must not produce a
/// non-monotonic result at any timestamp.
#[test]
fn prop_linear_large_total_is_monotonic() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    let asset_admin = soroban_sdk::token::StellarAssetClient::new(&env, &token.address);
    let total: i128 = i128::MAX / 4;
    asset_admin.mint(&employer, &total);

    let start: u64 = 0;
    let end: u64 = 10_000;

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &total,
        &start,
        &end,
        &None,
        &false,
    );

    let timestamps = gen_timestamps(0xFEED_FACE, 0, end + 500, 80);
    let mut prev: i128 = -1;
    for &ts in &timestamps {
        set_time(&env, ts);
        let vested = client.get_vested_amount(&sid);

        if prev >= 0 {
            assert!(
                vested >= prev,
                "[Linear large-total monotonicity] t={}: vested {} < prev {}",
                ts,
                vested,
                prev,
            );
        }
        assert!(
            vested >= 0 && vested <= total,
            "[Linear large-total monotonicity] vested {} out of [0, {}] at t={}",
            vested,
            total,
            ts,
        );
        prev = vested;
    }
}

/// Property: monotonicity holds for a custom schedule whose checkpoints are
/// very closely spaced (adjacent timestamps), testing the off-by-one region
/// around checkpoint transitions.
#[test]
fn prop_custom_dense_checkpoints_monotonic() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    // 5 checkpoints one second apart.
    let total: i128 = 500;
    let mut checkpoints = soroban_sdk::Vec::new(&env);
    for i in 1u64..=5 {
        checkpoints.push_back(CustomCheckpoint {
            time: i,
            cumulative_amount: (i as i128) * 100,
        });
    }

    set_time(&env, 0);
    let sid = client.create_custom_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &total,
        &checkpoints,
        &false,
    );

    // Dense sweep [0..10].
    let mut prev: i128 = -1;
    for ts in 0u64..=10 {
        set_time(&env, ts);
        let vested = client.get_vested_amount(&sid);

        if prev >= 0 {
            assert!(
                vested >= prev,
                "[Custom dense checkpoints] t={}: vested {} < prev {}",
                ts,
                vested,
                prev,
            );
        }
        prev = vested;
    }
}

// ===========================================================================
// Fully-vested revoke: safe no-op (issue #1066)
// ===========================================================================
//
// Once `get_vested_amount` reaches `total_amount`, there is nothing left to
// claw back. `revoke` must not transfer a spurious zero-amount payment, must
// not corrupt schedule bookkeeping, and must leave `get_releasable_amount`
// unaffected for whatever the beneficiary has not yet claimed.

#[test]
fn test_revoke_after_fully_vested_is_safe_noop() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &None,
        &true, // revocable
    );

    // Advance past the end of the schedule: fully vested, nothing claimed yet.
    set_time(&env, 200);
    assert_eq!(client.get_vested_amount(&sid), 1_000);
    let releasable_before = client.get_releasable_amount(&sid);
    assert_eq!(releasable_before, 1_000);

    let employer_balance_before = token.balance(&employer);
    let contract_balance_before = token.balance(&client.address);

    let refunded = client.revoke(&employer, &sid);

    // No tokens moved: nothing was unvested to claw back.
    assert_eq!(refunded, 0);
    assert_eq!(token.balance(&employer), employer_balance_before);
    assert_eq!(token.balance(&client.address), contract_balance_before);

    // Schedule bookkeeping stays consistent: still fully vested, and the
    // beneficiary's already-vested (but unclaimed) balance is unaffected.
    let schedule = client.get_schedule(&sid).unwrap();
    assert_eq!(schedule.status, VestingStatus::Revoked);
    assert_eq!(schedule.total_amount, 1_000);
    assert_eq!(schedule.released_amount, 0);
    assert_eq!(client.get_vested_amount(&sid), 1_000);
    assert_eq!(client.get_releasable_amount(&sid), releasable_before);

    // The beneficiary can still claim their fully-vested tokens after the
    // no-op revoke — revoke must not lock funds that were already earned.
    let claimed = client.claim(&beneficiary, &sid);
    assert_eq!(claimed, 1_000);
    assert_eq!(token.balance(&beneficiary), 1_000);
}

#[test]
fn test_revoke_after_fully_vested_with_partial_claim_is_safe_noop() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &None,
        &true,
    );

    // Beneficiary claims partway through vesting.
    set_time(&env, 50);
    let first_claim = client.claim(&beneficiary, &sid);
    assert_eq!(first_claim, 500);

    // Advance to fully vested, then revoke without claiming the rest first.
    set_time(&env, 100);
    assert_eq!(client.get_vested_amount(&sid), 1_000);
    let releasable_before = client.get_releasable_amount(&sid); // 500 unclaimed

    let contract_balance_before = token.balance(&client.address);
    let refunded = client.revoke(&employer, &sid);

    assert_eq!(refunded, 0);
    assert_eq!(token.balance(&client.address), contract_balance_before);

    let schedule = client.get_schedule(&sid).unwrap();
    assert_eq!(schedule.status, VestingStatus::Revoked);
    assert_eq!(schedule.released_amount, 500);
    assert_eq!(client.get_releasable_amount(&sid), releasable_before);

    // The remaining already-vested balance is still claimable post-revoke.
    let second_claim = client.claim(&beneficiary, &sid);
    assert_eq!(second_claim, 500);
    assert_eq!(token.balance(&beneficiary), 1_000);
}

#[test]
fn test_revoke_fully_vested_does_not_emit_a_spurious_transfer() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_cliff_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &400i128,
        &100u64,
        &true,
    );

    set_time(&env, 500); // well past the cliff/end
    client.revoke(&employer, &sid);

    let events = env.events().all();
    let last_event = events.last().unwrap();
    let event: RevokedEvent = last_event.2.into_val(&env);
    assert_eq!(event.refunded, 0);
}

#[test]
#[should_panic(expected = "Schedule not active")]
fn test_revoke_twice_after_fully_vested_fails() {
    let env = create_env();
    let (client, _owner, employer, beneficiary, token) = full_setup(&env);

    set_time(&env, 0);
    let sid = client.create_linear_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &1_000i128,
        &0u64,
        &100u64,
        &None,
        &true,
    );

    set_time(&env, 200);
    client.revoke(&employer, &sid);
    // Second revoke on an already-revoked (still fully-vested) schedule must
    // be rejected explicitly, not silently accepted as another no-op.
    client.revoke(&employer, &sid);
}
