#![cfg(test)]

use soroban_sdk::{testutils::Address as _, token, Address, Env, Vec};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn create_token_contract<'a>(env: &Env, admin: &Address) -> token::StellarAssetClient<'a> {
    let contract_address = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    token::StellarAssetClient::new(env, &contract_address)
}

fn setup_contract(
    env: &Env,
) -> (
    PayrollContractClient<'_>,
    Address,
    Address,
    Address,
    Address,
) {
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(env, &contract_id);

    let owner = Address::generate(env);
    let guardian1 = Address::generate(env);
    let guardian2 = Address::generate(env);
    let guardian3 = Address::generate(env);

    client.initialize(&owner);

    (client, owner, guardian1, guardian2, guardian3)
}

#[test]
fn test_owner_can_emergency_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, _, _, _) = setup_contract(&env);

    // Initially not paused
    assert!(!client.is_emergency_paused());

    // Owner activates emergency pause
    let _ = client.emergency_pause();

    // Verify paused
    assert!(client.is_emergency_paused());

    let state = client.get_emergency_pause_state().unwrap();
    assert!(state.is_paused);
    assert!(state.paused_at.is_some());
}

#[test]
fn test_owner_can_unpause() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, _, _, _) = setup_contract(&env);

    // Pause
    let _ = client.emergency_pause();
    assert!(client.is_emergency_paused());

    // Unpause
    let _ = client.emergency_unpause();
    assert!(!client.is_emergency_paused());
}

#[test]
fn test_set_emergency_guardians() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);

    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());

    client.set_emergency_guardians(&guardians);

    let stored = client.get_emergency_guardians().unwrap();
    assert_eq!(stored.len(), 3);
}

#[test]
fn test_multisig_pause_proposal() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);

    // Set guardians
    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());
    client.set_emergency_guardians(&guardians);

    // Guardian1 proposes pause with no timelock
    let _ = client.propose_emergency_pause(&guardian1, &0);

    // Not paused yet
    assert!(!client.is_emergency_paused());
}

#[test]
fn test_multisig_pause_approval_threshold() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);

    // Set 3 guardians (threshold = 2)
    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());
    client.set_emergency_guardians(&guardians);

    // Guardian1 proposes
    let _ = client.propose_emergency_pause(&guardian1, &0);
    assert!(!client.is_emergency_paused());

    // Guardian2 approves - reaches threshold (2/3)
    let _ = client.approve_emergency_pause(&guardian2);

    // Should be paused now
    assert!(client.is_emergency_paused());
}

#[test]
fn test_timelock_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);

    // Set guardians
    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());
    client.set_emergency_guardians(&guardians);

    // Propose with 1 hour timelock
    let timelock = 3600u64;
    let _ = client.propose_emergency_pause(&guardian1, &timelock);

    // Approve immediately - should fail due to timelock
    let result = client.try_approve_emergency_pause(&guardian2);

    // Timelock prevents immediate activation
    assert!(result.is_err() || !client.is_emergency_paused());
}

#[test]
fn test_paused_blocks_claims() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, _, _, _) = setup_contract(&env);

    // Create token and agreement
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    // Create and activate payroll agreement
    let agreement_id = client.create_payroll_agreement(&employer, &token.address, &86400);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    client.activate_agreement(&agreement_id);

    // Mint tokens to contract
    token.mint(&client.address, &10000);

    // Emergency pause
    let _ = client.emergency_pause();

    // Attempt to claim should fail
    let result = client.try_claim_payroll(&employee, &agreement_id, &0);
    assert!(result.is_err());
}

#[test]
fn test_paused_blocks_milestone_claims() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, _, _, _) = setup_contract(&env);

    // Create token and milestone agreement
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token.address);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);

    // Mint tokens
    token.mint(&client.address, &10000);

    // Emergency pause
    let _ = client.emergency_pause();

    // Attempt to claim milestone should fail
    let result = client.try_claim_milestone(&agreement_id, &1);
    assert!(result.is_err());
}

#[test]
fn test_unpause_restores_functionality() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, _, _, _) = setup_contract(&env);

    // Create token and milestone agreement (simpler than payroll)
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token.address);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);

    token.mint(&client.address, &10000);

    // Pause
    let _ = client.emergency_pause();

    // Verify claim fails
    assert!(client.try_claim_milestone(&agreement_id, &1).is_err());

    // Unpause
    let _ = client.emergency_unpause();

    // Claim should work now
    client.claim_milestone(&agreement_id, &1);

    // Verify milestone was claimed
    let milestone = client.get_milestone(&agreement_id, &1).unwrap();
    assert!(milestone.claimed);
}

#[test]
fn test_guardian_duplicate_approval_ignored() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, _) = setup_contract(&env);

    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    client.set_emergency_guardians(&guardians);

    // Guardian1 proposes
    let _ = client.propose_emergency_pause(&guardian1, &0);

    // Guardian1 tries to approve again (should be ignored)
    let _ = client.approve_emergency_pause(&guardian1);

    // Still not paused (needs 2/2)
    assert!(!client.is_emergency_paused());

    // Guardian2 approves
    let _ = client.approve_emergency_pause(&guardian2);

    // Now paused
    assert!(client.is_emergency_paused());
}

#[test]
fn test_pause_state_details() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, owner, _, _, _) = setup_contract(&env);

    // Pause
    let _ = client.emergency_pause();

    let state = client.get_emergency_pause_state().unwrap();
    assert!(state.is_paused);
    assert!(state.paused_at.is_some());
    assert_eq!(state.paused_by.unwrap(), owner);
    assert!(state.timelock_end.is_none());
}

#[test]
fn test_emergency_recovery_workflow() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);

    // Setup guardians
    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());
    client.set_emergency_guardians(&guardians);

    // Simulate security incident detection
    // Guardian1 proposes immediate pause
    let _ = client.propose_emergency_pause(&guardian1, &0);

    // Guardian2 confirms
    let _ = client.approve_emergency_pause(&guardian2);

    // Contract is now paused
    assert!(client.is_emergency_paused());

    // After incident resolved, owner unpauses
    let _ = client.emergency_unpause();
    assert!(!client.is_emergency_paused());
}
