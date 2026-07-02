#![cfg(test)]
//
// Tests that claim_milestone and batch_claim_milestones actually transfer
// escrowed tokens to the contributor (issue #483).
//
// The contract's CEI pattern: mark claimed → update escrow balance → transfer.
// These tests verify the transfer leg produces the correct balances.

use crate::{PayrollContract, PayrollContractClient};
use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

// ── helpers ───────────────────────────────────────────────────────────────────

fn setup() -> (Env, Address, Address, Address, PayrollContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);

    // Deploy a real Stellar Asset Contract so token balances work.
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin.clone()).address();

    // Fund employer with 10_000 tokens.
    StellarAssetClient::new(&env, &token).mint(&employer, &10_000i128);

    (env, employer, contributor, token, client)
}

fn balance(env: &Env, token: &Address, who: &Address) -> i128 {
    TokenClient::new(env, token).balance(who)
}

// ── claim_milestone transfers tokens to contributor ───────────────────────────

#[test]
fn test_claim_milestone_transfers_to_contributor() {
    let (env, employer, contributor, token, client) = setup();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    // Employer funds the escrow with 1000 tokens.
    client.fund_milestone_agreement(&agreement_id, &employer, &1_000i128);

    // Add and approve a milestone worth 400.
    client.add_milestone(&agreement_id, &400i128).unwrap();
    client.approve_milestone(&agreement_id, &1u32).unwrap();

    let before = balance(&env, &token, &contributor);
    client.claim_milestone(&agreement_id, &1u32).unwrap();
    let after = balance(&env, &token, &contributor);

    assert_eq!(after - before, 400i128, "contributor should receive 400 tokens");
}

#[test]
fn test_claim_milestone_reduces_contract_escrow() {
    let (env, employer, contributor, token, client) = setup();

    let contract_id = env.current_contract_address();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.fund_milestone_agreement(&agreement_id, &employer, &500i128);
    client.add_milestone(&agreement_id, &200i128).unwrap();
    client.approve_milestone(&agreement_id, &1u32).unwrap();

    let before = balance(&env, &token, &contract_id);
    client.claim_milestone(&agreement_id, &1u32).unwrap();
    let after = balance(&env, &token, &contract_id);

    assert_eq!(before - after, 200i128, "contract escrow should decrease by milestone amount");
}

// ── batch_claim_milestones transfers for each milestone ───────────────────────

#[test]
fn test_batch_claim_milestones_transfers_all() {
    let (env, employer, contributor, token, client) = setup();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.fund_milestone_agreement(&agreement_id, &employer, &3_000i128);

    // Add and approve 3 milestones.
    for amount in [300i128, 500i128, 700i128] {
        client.add_milestone(&agreement_id, &amount).unwrap();
    }
    client.approve_milestone(&agreement_id, &1u32).unwrap();
    client.approve_milestone(&agreement_id, &2u32).unwrap();
    client.approve_milestone(&agreement_id, &3u32).unwrap();

    let before = balance(&env, &token, &contributor);
    let ids = soroban_sdk::vec![&env, 1u32, 2u32, 3u32];
    client.batch_claim_milestones(&agreement_id, &ids);
    let after = balance(&env, &token, &contributor);

    assert_eq!(after - before, 1_500i128, "batch should transfer 300+500+700=1500 tokens");
}

#[test]
fn test_batch_claim_partial_success_transfers_approved_only() {
    let (env, employer, contributor, token, client) = setup();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.fund_milestone_agreement(&agreement_id, &employer, &2_000i128);

    // Add 2 milestones but only approve the first.
    client.add_milestone(&agreement_id, &400i128).unwrap();
    client.add_milestone(&agreement_id, &600i128).unwrap();
    client.approve_milestone(&agreement_id, &1u32).unwrap();
    // milestone 2 is NOT approved

    let before = balance(&env, &token, &contributor);
    let ids = soroban_sdk::vec![&env, 1u32, 2u32];
    client.batch_claim_milestones(&agreement_id, &ids);
    let after = balance(&env, &token, &contributor);

    // Only milestone 1 (400) should have been transferred.
    assert_eq!(after - before, 400i128, "only approved milestone should be transferred");
}
