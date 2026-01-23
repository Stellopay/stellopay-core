#![cfg(test)]
use soroban_sdk::{
    log,
    testutils::{Address as _, Ledger},
    token, Address, Env,
};
use stello_pay_contract::storage::DisputeStatus;
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn create_test_env() -> (
    Env,
    Address,
    Address,
    Address,
    PayrollContractClient<'static>,
) {
    let env = Env::default();
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    (env, employer, contributor, token, client)
}

fn create_token_contract<'a>(
    e: &'a Env,
    admin: &Address,
) -> (Address, token::Client<'a>, token::StellarAssetClient<'a>) {
    let token_id = e.register_stellar_asset_contract_v2(admin.clone());
    let token = token_id.address();
    let token_client = token::Client::new(e, &token);
    let token_admin_client = token::StellarAssetClient::new(e, &token);
    (token, token_client, token_admin_client)
}

#[test]
fn test_dispute_flow() {
    // let env = Env::default();

    let (env, employer, employee, _token, client) = create_test_env();
    let employee = Address::generate(&env);
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);

    // Initialize
    client.initialize(&owner.clone());

    // Set arbiter
    client.set_arbiter(&owner, &arbiter.clone());

    // Setup token
    let token_admin = Address::generate(&env);
    let (token, token_client, token_admin_client) = create_token_contract(&env, &token_admin);

    // Create agreement
    let agreement_id = client.create_escrow_agreement(
        &employer.clone(),
        &employee.clone(),
        &token.clone(),
        &200,
        &86400,
        &5,
    );

    // Raise dispute by employee
    client.raise_dispute(&employer, &agreement_id);

    let status = client.get_dispute_status(&agreement_id);
    log!(&env, "Status: {}", status);

    // Check status
    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Raised
    );

    env.mock_all_auths();
    token_admin_client.mint(&employer, &2000);
    token_client.transfer(&employer, &client.address, &1000);

    assert_eq!(token_client.balance(&employer), 1000);
    assert_eq!(token_client.balance(&employee), 0);
    assert_eq!(token_client.balance(&client.address), 1000);

    // Resolve by arbiter
    client.resolve_dispute(&arbiter, &agreement_id, &700, &300);

    assert_eq!(token_client.balance(&employer), 1300);
    assert_eq!(token_client.balance(&employee), 700);
    assert_eq!(token_client.balance(&client.address), 0);

    // Check resolved
    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Resolved
    );
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #3)")]
fn test_raise_dispute_non_arbiter() {
    // let env = Env::default();

    let (env, employer, employee, _token, client) = create_test_env();
    let employee = Address::generate(&env);
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let non_party = Address::generate(&env);

    // Initialize
    client.initialize(&owner.clone());

    // Set arbiter
    client.set_arbiter(&owner, &arbiter.clone());

    // Setup token
    let token_admin = Address::generate(&env);
    let (token, token_client, token_admin_client) = create_token_contract(&env, &token_admin);

    // Create agreement
    let agreement_id = client.create_escrow_agreement(
        &employer.clone(),
        &employee.clone(),
        &token.clone(),
        &200,
        &86400,
        &5,
    );

    // Raise dispute by employee
    client.raise_dispute(&non_party, &agreement_id);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #2)")]
fn test_raise_dispute_outisde_grace_period() {
    // let env = Env::default();

    let (env, employer, employee, _token, client) = create_test_env();
    let employee = Address::generate(&env);
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);

    // Initialize
    client.initialize(&owner.clone());

    // Set arbiter
    client.set_arbiter(&owner, &arbiter.clone());

    // Setup token
    let token_admin = Address::generate(&env);
    let (token, token_client, token_admin_client) = create_token_contract(&env, &token_admin);

    // Create agreement
    let agreement_id = client.create_escrow_agreement(
        &employer.clone(),
        &employee.clone(),
        &token.clone(),
        &200,
        &3000,
        &5,
    );

    env.ledger().set_timestamp(3000 * 5 + 1);
    // Raise dispute by employee
    client.raise_dispute(&employer, &agreement_id);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #5)")]
fn test_resolve_dispute_invalid_payout() {
    // let env = Env::default();

    let (env, employer, employee, _token, client) = create_test_env();
    let employee = Address::generate(&env);
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);

    // Initialize
    client.initialize(&owner.clone());

    // Set arbiter
    client.set_arbiter(&owner, &arbiter.clone());

    // Setup token
    let token_admin = Address::generate(&env);
    let (token, token_client, token_admin_client) = create_token_contract(&env, &token_admin);

    // Create agreement
    let agreement_id = client.create_escrow_agreement(
        &employer.clone(),
        &employee.clone(),
        &token.clone(),
        &200,
        &86400,
        &5,
    );

    // Raise dispute by employee
    client.raise_dispute(&employer, &agreement_id);

    let status = client.get_dispute_status(&agreement_id);
    log!(&env, "Status: {}", status);

    // Check status
    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Raised
    );

    env.mock_all_auths();
    token_admin_client.mint(&employer, &2000);
    token_client.transfer(&employer, &client.address, &1000);

    assert_eq!(token_client.balance(&employer), 1000);
    assert_eq!(token_client.balance(&employee), 0);
    assert_eq!(token_client.balance(&client.address), 1000);

    // Resolve by arbiter
    client.resolve_dispute(&arbiter, &agreement_id, &1000, &300);
}

#[test]
fn test_dispute_payroll_distribution() {
    let (env, employer, employee, _token, client) = create_test_env();
    let employee = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);

    // Initialize
    client.initialize(&owner.clone());

    // Set arbiter
    client.set_arbiter(&owner, &arbiter.clone());

    // Setup token
    let token_admin = Address::generate(&env);
    let (token, token_client, token_admin_client) = create_token_contract(&env, &token_admin);

    env.mock_all_auths();
    let agreement_id = client.create_payroll_agreement(&employer, &token, &3600);

    client.add_employee_to_agreement(&agreement_id, &employee.clone(), &300);
    client.add_employee_to_agreement(&agreement_id, &employee1.clone(), &300);
    client.add_employee_to_agreement(&agreement_id, &employee2, &300);

    // Raise dispute by employee
    client.raise_dispute(&employer, &agreement_id);

    let status = client.get_dispute_status(&agreement_id);
    log!(&env, "Status: {}", status);

    // Check status
    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Raised
    );

    env.mock_all_auths();
    token_admin_client.mint(&employer, &2000);
    token_client.transfer(&employer, &client.address, &900);

    assert_eq!(token_client.balance(&employer), 1100);
    assert_eq!(token_client.balance(&employee), 0);
    assert_eq!(token_client.balance(&employee1), 0);
    assert_eq!(token_client.balance(&employee2), 0);
    assert_eq!(token_client.balance(&client.address), 900);

    // Resolve by arbiter
    client.resolve_dispute(&arbiter, &agreement_id, &600, &300);

    assert_eq!(token_client.balance(&employer), 1400);
    assert_eq!(token_client.balance(&employee), 200);
    assert_eq!(token_client.balance(&employee1), 200);
    assert_eq!(token_client.balance(&employee2), 200);
    assert_eq!(token_client.balance(&client.address), 0);

    // Check resolved
    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Resolved
    );
}
