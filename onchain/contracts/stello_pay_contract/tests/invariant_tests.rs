#![cfg(test)]

use soroban_sdk::{testutils::{Address as _, Ledger}, Address, Env, Vec};
use stello_pay_contract::storage::{AgreementMode, AgreementStatus, DataKey, Milestone};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_contract(env: &Env) -> (Address, PayrollContractClient<'static>) {
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (contract_id, client)
}

fn setup_token<'a>(
    env: &'a Env,
    admin: &Address,
) -> (Address, soroban_sdk::token::Client<'a>, soroban_sdk::token::StellarAssetClient<'a>) {
    let token_contract = env.register_stellar_asset_contract_v2(admin.clone());
    let token_client = soroban_sdk::token::Client::new(env, &token_contract.address());
    let token_admin_client = soroban_sdk::token::StellarAssetClient::new(env, &token_contract.address());
    (token_contract.address(), token_client, token_admin_client)
}

#[test]
fn test_invariant_escrow_claimed_periods_limit() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let (token_id, _token_client, token_admin_client) = setup_token(&env, &token_admin);
    
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    
    // Create escrow agreement with 2 periods
    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token_id,
        &1000i128,
        &3600u64,
        &2u32,
    );
    
    // Fund contract
    token_admin_client.mint(&contract_id, &2000i128);
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token_id, 2000i128);
    });
    client.activate_agreement(&agreement_id);
    
    // Jump 1 hour - claim 1 period
    env.ledger().with_mut(|l| l.timestamp = 3600);
    client.claim_time_based(&agreement_id);
    
    // Jump another hour - claim 2nd period
    env.ledger().with_mut(|l| l.timestamp = 7200);
    client.claim_time_based(&agreement_id);
    
    // Verification: agreement is completed
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Completed);
}

#[test]
#[should_panic(expected = "Insufficient contract balance for unclaimed milestones")]
fn test_invariant_milestone_balance_insufficient() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let (token_id, _token_client, _token_admin_client) = setup_token(&env, &token_admin);
    
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token_id);
    client.add_milestone(&agreement_id, &1000i128);
    client.add_milestone(&agreement_id, &2000i128);
    
    // Total unclaimed = 3000. Contract balance = 0.
    // Approving a milestone should trigger the invariant check.
    client.approve_milestone(&agreement_id, &1u32);
}

#[test]
fn test_invariant_milestone_balance_sufficient() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let (token_id, _token_client, token_admin_client) = setup_token(&env, &token_admin);
    
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token_id);
    client.add_milestone(&agreement_id, &1000i128);
    
    // Fund contract
    token_admin_client.mint(&contract_id, &1000i128);
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token_id, 1000i128);
    });
    
    // Should succeed
    client.approve_milestone(&agreement_id, &1u32);
}

#[test]
#[should_panic(expected = "Invariant violation: escrow balance < sum of unclaimed milestones")]
fn test_invariant_milestone_claim_insufficient_balance() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let (token_id, token_client, token_admin_client) = setup_token(&env, &token_admin);
    
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token_id);
    client.add_milestone(&agreement_id, &1000i128);
    
    // Fund contract
    token_admin_client.mint(&contract_id, &1000i128);
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token_id, 1000i128);
    });
    client.approve_milestone(&agreement_id, &1u32);
    
    // Now someone steals the funds from the contract (mocked by transfer and manual balance update)
    token_client.transfer(&contract_id, &Address::generate(&env), &1000i128);
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token_id, 0);
    });
    
    // Claim should fail due to invariant
    client.claim_milestone(&agreement_id, &1u32);
}
