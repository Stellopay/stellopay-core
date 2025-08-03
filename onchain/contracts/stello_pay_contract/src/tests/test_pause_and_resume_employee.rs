use soroban_sdk::{testutils::Address as _, Address, Env};
use crate::payroll::{PayrollContractClient, PayrollError};

#[test]
fn test_pause_resume_employee_payroll_by_owner() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    
    let owner = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days

    env.mock_all_auths();

    // Initialize contract and create payroll
    client.initialize(&owner);
    client.create_or_update_escrow(&owner, &employee, &token, &amount, &interval, &recurrence_frequency);

    // Default should be false
    client.resume_employee_payroll(&owner, &employee);
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.is_paused, false);

    // Test pause
    client.pause_employee_payroll(&owner, &employee);
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.is_paused, true);

    // Test resume
    client.resume_employee_payroll(&owner, &employee);
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.is_paused, false);
}

#[test]
fn test_pause_resume_employee_payroll_by_employer() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64;

    env.mock_all_auths();

    // Initialize contract and create payroll
    client.initialize(&owner);
    client.create_or_update_escrow(&owner, &employee, &token, &amount, &interval, &recurrence_frequency);

    // Transfer ownership to employer
    client.transfer_ownership(&owner, &employer);

    // Test pause by employer
    client.pause_employee_payroll(&employer, &employee);
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.is_paused, true);

    // Test resume by employer
    client.resume_employee_payroll(&employer, &employee);
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.is_paused, false);
}


#[test]
#[should_panic(expected = "HostError: Error(Contract, #1)")]
fn test_pause_employee_payroll_unauthorized() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    
    let owner = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64;

    env.mock_all_auths();

    // Initialize contract and create payroll
    client.initialize(&owner);
    client.create_or_update_escrow(&owner, &employee, &token, &amount, &interval, &recurrence_frequency);

    client.resume_employee_payroll(&unauthorized, &employee);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #1)")]
fn test_resume_employee_payroll_unauthorized() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    
    let owner = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64;

    env.mock_all_auths();

    client.initialize(&owner);
    client.create_or_update_escrow(&owner, &employee, &token, &amount, &interval, &recurrence_frequency);
    client.pause_employee_payroll(&owner, &employee);

    // Should fail when non-owner/non-employer tries to resume
    client.resume_employee_payroll(&unauthorized, &employee);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #4)")]
fn test_pause_nonexistent_employee_payroll() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    
    let owner = Address::generate(&env);
    let employee = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&owner);

    // Should fail when trying to pause non-existent payroll
    client.pause_employee_payroll(&owner, &employee);
}
