#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

#[test]
fn test_pause_resume_flow() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    client.initialize(&owner);

    // Create an agreement
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &1000,
        &86400,
        &2592000, // recurrence
    );

    // Verify initial state
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.is_paused, false);

    // 1. Pause the agreement
    client.pause_agreement(&employee);

    let payroll_paused = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll_paused.is_paused, true);

    // 2. Try to claim (should panic)
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.claim_payroll(&employee);
    }));
    assert!(result.is_err());

    // 3. Resume the agreement
    client.resume_agreement(&employee);

    let payroll_resumed = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll_resumed.is_paused, false);

    // 4. Try to claim (should succeed - strictly speaking it might panic due to missing logic in claim_payroll, 
    // but definitely NOT due to "Agreement is paused")
    // In our simplified impl, claim_payroll does nothing else, so it should succeed.
    client.claim_payroll(&employee);
}

#[test]
#[should_panic(expected = "Agreement already paused")]
fn test_double_pause() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    client.create_or_update_escrow(&employer, &employee, &token, &1000, &1, &1);
    client.pause_agreement(&employee);
    client.pause_agreement(&employee);
}

#[test]
#[should_panic(expected = "Agreement not paused")]
fn test_resume_not_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    client.create_or_update_escrow(&employer, &employee, &token, &1000, &1, &1);
    client.resume_agreement(&employee);
}
