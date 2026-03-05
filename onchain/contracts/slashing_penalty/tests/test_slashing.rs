#![cfg(test)]

use slashing_penalty::{
    PenaltyError, PenaltyPolicy, PenaltyReason, SlashingPenaltyContract,
    SlashingPenaltyContractClient, SlashingRecord,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_contract(env: &Env) -> (Address, SlashingPenaltyContractClient<'static>, Address) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, SlashingPenaltyContract);
    let client = SlashingPenaltyContractClient::new(env, &contract_id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (contract_id, client, owner)
}

fn create_token(env: &Env) -> (Address, TokenClient<'static>) {
    let admin = Address::generate(env);
    let token_addr = env.register_stellar_asset_contract_v2(admin).address();
    let client = TokenClient::new(env, &token_addr);
    (token_addr, client)
}

#[test]
fn initialize_once_and_owner_access() {
    let env = create_env();
    let (contract_id, client, owner) = setup_contract(&env);

    assert_eq!(client.get_owner(), Some(owner.clone()));
    assert!(client.get_operator().is_none());

    // Second initialize must fail.
    let second = client.try_initialize(&owner);
    assert_eq!(second, Err(Ok(PenaltyError::AlreadyInitialized)));

    // Owner can set an operator.
    let operator = Address::generate(&env);
    client.set_operator(&owner, &operator);
    assert_eq!(client.get_operator(), Some(operator.clone()));

    // Non-owner cannot set operator.
    let other = Address::generate(&env);
    let res = client.try_set_operator(&other, &operator);
    assert!(matches!(res, Err(Ok(PenaltyError::NotAuthorized))));

    // Basic policy creation.
    let desc = soroban_sdk::String::from_str(&env, "Late payment policy");
    let policy_id = client.create_policy(&owner, &None, &2_000u32, &None, &desc);
    let policy: PenaltyPolicy = client.get_policy(&policy_id).unwrap();
    assert!(policy.is_active);
    assert_eq!(policy.max_penalty_bps, 2_000);
}

#[test]
fn policy_validation_and_activation() {
    let env = create_env();
    let (_contract_id, client, owner) = setup_contract(&env);

    let bad_bps = client.try_create_policy(
        &owner,
        &None,
        &0u32,
        &None,
        &soroban_sdk::String::from_str(&env, "invalid"),
    );
    assert_eq!(bad_bps, Err(Ok(PenaltyError::InvalidConfig)));

    let bad_cap = client.try_create_policy(
        &owner,
        &None,
        &1_000u32,
        &Some(-5i128),
        &soroban_sdk::String::from_str(&env, "invalid"),
    );
    assert_eq!(bad_cap, Err(Ok(PenaltyError::InvalidConfig)));

    let desc = soroban_sdk::String::from_str(&env, "Scoped escrow policy");
    let agreement_id: u128 = 42;
    let policy_id = client.create_policy(
        &owner,
        &Some(agreement_id),
        &5_000u32,
        &Some(1_000i128),
        &desc,
    );

    let mut policy = client.get_policy(&policy_id).unwrap();
    assert!(policy.is_active);
    assert_eq!(policy.agreement_id, Some(agreement_id));

    // Deactivate and reactivate.
    client.set_policy_active(&owner, &policy_id, &false);
    policy = client.get_policy(&policy_id).unwrap();
    assert!(!policy.is_active);

    client.set_policy_active(&owner, &policy_id, &true);
    policy = client.get_policy(&policy_id).unwrap();
    assert!(policy.is_active);
}

#[test]
fn slashing_respects_caps_and_records_state() {
    let env = create_env();
    let (contract_id, client, owner) = setup_contract(&env);
    let (token_addr, token_client) = create_token(&env);

    // Seed escrow tokens into the slashing contract.
    let admin = Address::generate(&env);
    let sac = StellarAssetClient::new(&env, &token_addr);
    sac.mint(&contract_id, &10_000i128);
    assert_eq!(token_client.balance(&contract_id), 10_000i128);

    let offender = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let agreement_id: u128 = 777;

    let desc = soroban_sdk::String::from_str(&env, "Late payment tier 1");
    // Max 25% of locked, absolute cap 2_000.
    let policy_id = client.create_policy(
        &owner,
        &Some(agreement_id),
        &2_500u32,
        &Some(2_000i128),
        &desc,
    );

    // Locked amount 8_000, 25% cap => 2_000 (also matches absolute cap).
    let current_locked = 8_000i128;

    env.ledger().with_mut(|li| {
        li.timestamp = 1234;
    });

    client.slash(
        &owner,
        &policy_id,
        &offender,
        &beneficiary,
        &token_addr,
        &Some(agreement_id),
        &2_000i128,
        &current_locked,
        &PenaltyReason::LatePayment,
    );

    // Beneficiary receives slashed funds.
    assert_eq!(token_client.balance(&beneficiary), 2_000i128);

    // Record is stored and can be read back.
    let record: SlashingRecord = client.get_record(&1u64).unwrap();
    assert_eq!(record.policy_id, policy_id);
    assert_eq!(record.agreement_id, Some(agreement_id));
    assert_eq!(record.amount, 2_000i128);
    assert_eq!(record.beneficiary, beneficiary);
    assert_eq!(record.token, token_addr);
    assert_eq!(record.reason, PenaltyReason::LatePayment);
    assert_eq!(record.timestamp, 1234);
}

#[test]
fn slashing_out_of_bounds_fails() {
    let env = create_env();
    let (_contract_id, client, owner) = setup_contract(&env);

    let desc = soroban_sdk::String::from_str(&env, "Strict cap policy");
    let policy_id = client.create_policy(&owner, &None, &1_000u32, &Some(500i128), &desc);

    let offender = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let token = Address::generate(&env);
    let locked = 1_000i128;

    // Amount greater than locked.
    let res = client.try_slash(
        &owner,
        &policy_id,
        &offender,
        &beneficiary,
        &token,
        &None,
        &2_000i128,
        &locked,
        &PenaltyReason::PolicyViolation,
    );
    assert_eq!(res, Err(Ok(PenaltyError::CapExceeded)));

    // Amount greater than bps cap (10% of 1_000 = 100).
    let res = client.try_slash(
        &owner,
        &policy_id,
        &offender,
        &beneficiary,
        &token,
        &None,
        &150i128,
        &locked,
        &PenaltyReason::PolicyViolation,
    );
    assert_eq!(res, Err(Ok(PenaltyError::CapExceeded)));

    // Amount greater than absolute cap 500.
    let locked_large = 10_000i128;
    let res = client.try_slash(
        &owner,
        &policy_id,
        &offender,
        &beneficiary,
        &token,
        &None,
        &800i128,
        &locked_large,
        &PenaltyReason::PolicyViolation,
    );
    assert_eq!(res, Err(Ok(PenaltyError::CapExceeded)));
}

#[test]
fn scoped_policy_mismatch_rejected() {
    let env = create_env();
    let (_contract_id, client, owner) = setup_contract(&env);

    let desc = soroban_sdk::String::from_str(&env, "Scoped policy");
    let policy_id = client.create_policy(&owner, &Some(10u128), &2_000u32, &None, &desc);

    let offender = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let token = Address::generate(&env);

    // Agreement id mismatch.
    let res = client.try_slash(
        &owner,
        &policy_id,
        &offender,
        &beneficiary,
        &token,
        &Some(11u128),
        &10i128,
        &100i128,
        &PenaltyReason::BreachOfAgreement,
    );
    assert_eq!(res, Err(Ok(PenaltyError::InvalidConfig)));
}
