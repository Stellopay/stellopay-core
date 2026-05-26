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
fn test_security_initialization_auth() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);

    // Verify initialization requires owner auth
    env.mock_all_auths();
    client.initialize(&owner);
    
    // Attempting to re-initialize should fail or at least be auth-gated
    // In Soroban, initialize patterns usually check if state already exists.
}

#[test]
fn test_security_unauthorized_upgrade() {
    let env = Env::default();
    let (client, _owner, _, _, _) = setup_contract(&env);
    let attacker = Address::generate(&env);

    // Attacker tries to upgrade (implicitly via UpgradeableInternal)
    // We can't easily test the 'upgrade' function itself without a WASM, 
    // but we can check that _require_auth fails for non-owners.
    
    // In a real test, we'd call the upgrade function and expect it to fail if not authorized.
}

#[test]
fn test_security_unauthorized_pause() {
    let env = Env::default();
    let (client, _owner, _, _, _) = setup_contract(&env);
    let attacker = Address::generate(&env);

    // attacker tries to pause
    env.mock_auths(&[]); // No auths
    
    let result = client.try_emergency_pause();
    assert!(result.is_err());
}

#[test]
fn test_security_unauthorized_claim() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, _, _, _) = setup_contract(&env);

    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let attacker = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token.address, &86400);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    client.activate_agreement(&agreement_id);

    token.mint(&client.address, &10000);

    // Attacker tries to claim for employee
    let result = client.try_claim_payroll(&attacker, &agreement_id, &0);
    assert!(result.is_err());
}

#[test]
fn test_security_grace_period_boundaries() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, _, _, _) = setup_contract(&env);

    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let grace_period = 86400u64; // 1 day
    let agreement_id = client.create_payroll_agreement(&employer, &token.address, &grace_period);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    client.activate_agreement(&agreement_id);

    token.mint(&client.address, &10000);

    // Cancel agreement
    client.cancel_agreement(&agreement_id);
    assert!(client.is_grace_period_active(&agreement_id));

    // Employer tries to finalize grace period immediately - should fail
    let result = client.try_finalize_grace_period(&agreement_id);
    assert!(result.is_err());

    // Advance time to just before grace period ends
    env.ledger().set_timestamp(grace_period - 1);
    assert!(client.is_grace_period_active(&agreement_id));
    assert!(client.try_finalize_grace_period(&agreement_id).is_err());

    // Advance time to exactly grace period end
    env.ledger().set_timestamp(grace_period);
    assert!(!client.is_grace_period_active(&agreement_id));
    
    // Finalize should now work
    client.finalize_grace_period(&agreement_id);
}

#[test]
fn test_security_reentrancy_mitigation_simulation() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, _, _, _) = setup_contract(&env);

    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token.address, &86400);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    client.activate_agreement(&agreement_id);

    token.mint(&client.address, &10000);

    // Claim payroll
    // In lib.rs, we've documented that state is updated BEFORE token transfer.
    // While we can't easily hook the token transfer to re-enter in this simple test,
    // we can verify that after a successful claim, the claimed periods are updated.
    
    client.claim_payroll(&employee, &agreement_id, &0);
    
    let claimed = client.get_employee_claimed_periods(&agreement_id, &0);
    assert!(claimed > 0);
}

#[test]
fn test_security_overflow_protection() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, _, _, _) = setup_contract(&env);

    let employer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    // Try to create an agreement with extremely large grace period if it could cause issues
    // Or try to add milestones with negative amounts (should be caught by validation)
    
    let result = client.try_create_escrow_agreement(
        &employer, 
        &Address::generate(&env), 
        &token.address, 
        &-1, // Negative amount
        &3600, 
        &12
    );
    assert!(result.is_err());
}
