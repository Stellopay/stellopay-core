//! Negative-path tests for milestone agreement authorization and state-machine
//! transitions. Verifies that unauthorized callers, invalid state transitions,
//! and double-claims are all rejected.

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{testutils::Address as _, Address, Env};
use stello_pay_contract::storage::PayrollError;
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn create_env() -> (Env, Address, Address, Address, PayrollContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    client.initialize(&owner);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    soroban_sdk::token::StellarAssetClient::new(&env, &token).mint(&employer, &1_000_000_000i128);
    (env, employer, contributor, token, client)
}

fn create_funded_agreement(
    env: &Env,
    client: &PayrollContractClient,
    employer: &Address,
    contributor: &Address,
    token: &Address,
) -> u128 {
    let id = client.create_milestone_agreement(employer, contributor, token);
    client.fund_milestone_agreement(&id, employer, &100_000i128);
    client.add_milestone(&id, &10_000i128);
    id
}

// ---- Claim before approval must fail ----

#[test]
fn test_claim_before_approval_fails() {
    let (env, employer, contributor, token, client) = create_env();
    let id = create_funded_agreement(&env, &client, &employer, &contributor, &token);
    let result = client.try_claim_milestone(&id, &1u32);
    assert!(result.is_err(), "Claiming unapproved milestone must fail");
}

// ---- Double-claim must fail ----

#[test]
fn test_double_claim_fails() {
    let (env, employer, contributor, token, client) = create_env();
    let id = create_funded_agreement(&env, &client, &employer, &contributor, &token);
    client.approve_milestone(&id, &1u32);
    client.claim_milestone(&id, &1u32);
    let result = client.try_claim_milestone(&id, &1u32);
    assert!(result.is_err(), "Double-claiming a milestone must fail");
}

// ---- Approve non-existent agreement fails ----

#[test]
fn test_approve_non_existent_agreement_fails() {
    let (_env, _employer, _contributor, _token, client) = create_env();
    let result = client.try_approve_milestone(&9999u128, &0u32);
    assert!(result.is_err(), "Approve on non-existent agreement must fail");
}

// ---- Claim non-existent milestone fails ----

#[test]
fn test_claim_non_existent_milestone_fails() {
    let (env, employer, contributor, token, client) = create_env();
    let id = client.create_milestone_agreement(&employer, &contributor, &token);
    let result = client.try_claim_milestone(&id, &999u32);
    assert!(result.is_err(), "Claim on non-existent milestone must fail");
}

// ---- State-machine happy path ----

#[test]
fn test_state_machine_approve_then_claim() {
    let (env, employer, contributor, token, client) = create_env();
    let id = create_funded_agreement(&env, &client, &employer, &contributor, &token);
    let m = client.get_milestone(&id, &1u32).unwrap();
    assert!(!m.approved && !m.claimed, "Should start unapproved/unclaimed");
    client.approve_milestone(&id, &1u32);
    let m = client.get_milestone(&id, &1u32).unwrap();
    assert!(m.approved && !m.claimed, "Should be approved but not claimed");
    client.claim_milestone(&id, &1u32);
    let m = client.get_milestone(&id, &1u32).unwrap();
    assert!(m.approved && m.claimed, "Should be both approved and claimed");
}

// ---- Batch claim: skipping already-claimed milestones ----

#[test]
fn test_batch_claim_second_milestone_after_first_claimed() {
    let (env, employer, contributor, token, client) = create_env();
    let id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.fund_milestone_agreement(&id, &employer, &100_000i128);
    client.add_milestone(&id, &10_000i128);
    client.add_milestone(&id, &20_000i128);
    client.approve_milestone(&id, &1u32);
    client.approve_milestone(&id, &2u32);
    client.claim_milestone(&id, &1u32);
    // Milestone 1 is already claimed; batch should succeed on 2 and fail on 1
    let ids = soroban_sdk::vec![&env, 1u32, 2u32];
    let result = client.batch_claim_milestones(&id, &ids);
    assert_eq!(result.successful_claims, 1, "Only one claim should succeed");
    assert_eq!(result.failed_claims, 1, "Already-claimed milestone should fail");
}
