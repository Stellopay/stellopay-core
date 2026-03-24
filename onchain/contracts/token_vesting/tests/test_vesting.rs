#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};

use token_vesting::{
    CustomCheckpoint, TokenVestingContract, TokenVestingContractClient, VestingKind, VestingStatus,
};

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
fn linear_vesting_claim_flow() {
    let env = create_env();
    let client = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let employer = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let asset_admin = StellarAssetClient::new(&env, &token.address);
    asset_admin.mint(&employer, &1_000i128);

    // linear schedule: 600 total over [0, 60]
    env.ledger().with_mut(|li| {
        li.timestamp = 0;
    });

    let schedule_id = client.create_linear_schedule(
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
    assert_eq!(client.get_vested_amount(&schedule_id), 0);
    assert_eq!(client.get_releasable_amount(&schedule_id), 0);

    // halfway at t=30: ~300 vested
    env.ledger().with_mut(|li| {
        li.timestamp = 30;
    });
    let vested = client.get_vested_amount(&schedule_id);
    assert!(vested >= 290 && vested <= 310);

    // claim once
    let claimed = client.claim(&beneficiary, &schedule_id);
    assert_eq!(claimed, client.get_vested_amount(&schedule_id));

    // at end all vested and claimable remainder
    env.ledger().with_mut(|li| {
        li.timestamp = 60;
    });
    let remaining = client.get_releasable_amount(&schedule_id);
    assert!(remaining > 0);
    let claimed2 = client.claim(&beneficiary, &schedule_id);
    assert_eq!(claimed2, remaining);

    let second = client.try_claim(&beneficiary, &schedule_id);
    assert!(second.is_err());

    let schedule = client.get_schedule(&schedule_id).unwrap();
    assert_eq!(schedule.status, VestingStatus::Completed);
}

#[test]
fn cliff_vesting_and_revocation() {
    let env = create_env();
    let client = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let employer = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let asset_admin = StellarAssetClient::new(&env, &token.address);
    asset_admin.mint(&employer, &1_000i128);

    env.ledger().with_mut(|li| {
        li.timestamp = 0;
    });

    let cliff_time = 100u64;
    let schedule_id = client.create_cliff_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &400i128,
        &cliff_time,
        &true,
    );

    // before cliff: nothing vested or releasable
    assert_eq!(client.get_vested_amount(&schedule_id), 0);

    // revoke before cliff: all refunded
    let refunded = client.revoke(&employer, &schedule_id);
    assert_eq!(refunded, 400i128);

    let schedule = client.get_schedule(&schedule_id).unwrap();
    assert_eq!(schedule.status, VestingStatus::Revoked);
}

#[test]
fn custom_schedule_and_early_release() {
    let env = create_env();
    let client = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let employer = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let asset_admin = StellarAssetClient::new(&env, &token.address);
    asset_admin.mint(&employer, &1_000i128);

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

    env.ledger().with_mut(|li| {
        li.timestamp = 0;
    });

    let schedule_id = client.create_custom_schedule(
        &employer,
        &beneficiary,
        &token.address,
        &500i128,
        &checkpoints,
        &true,
    );

    // at t=15, second checkpoint not reached, so 100 vested
    env.ledger().with_mut(|li| {
        li.timestamp = 15;
    });
    assert_eq!(client.get_vested_amount(&schedule_id), 100);

    // admin can approve early release of remaining unvested portion
    let early = client.approve_early_release(&owner, &schedule_id, &200i128);
    assert_eq!(early, 200i128);

    let schedule = client.get_schedule(&schedule_id).unwrap();
    assert!(schedule.released_amount >= 200);
}
