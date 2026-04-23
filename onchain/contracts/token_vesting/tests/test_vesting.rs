#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, IntoVal, Symbol, Vec, vec,
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
