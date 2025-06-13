#[cfg(test)]
use soroban_sdk::{testutils::Address as _, Address, Env};
use crate::payroll::{Payroll, PayrollContract, PayrollContractClient, PayrollKey};

#[test]
fn test_create_new_escrow() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let interval = 86400u64;

    env.mock_all_auths();

    client.initialize(&employer);

    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

    let stored_payroll: Payroll = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get(&PayrollKey(employee.clone()))
            .unwrap()
    });

    assert_eq!(stored_payroll.employer, employer);
    assert_eq!(stored_payroll.employee, employee);
    assert_eq!(stored_payroll.token, token);
    assert_eq!(stored_payroll.amount, amount);
    assert_eq!(stored_payroll.interval, interval);
    assert_eq!(stored_payroll.last_payment_time, env.ledger().timestamp());
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_create_new_escrow_panic() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let owner = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    
    env.mock_all_auths();

    // Initialize contract
    client.initialize(&owner);

    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);
}

#[test]
fn test_update_existing_escrow_valid_employer() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let initial_amount = 1000i128;
    let interval = 86400u64;

    env.mock_all_auths();
    
    client.initialize(&employer);
    client.create_or_update_escrow(&employer, &employee, &token, &initial_amount, &interval);

    let updated_amount = 2000i128;
    client.create_or_update_escrow(&employer, &employee, &token, &updated_amount, &interval);

    let stored_payroll: Payroll = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get(&PayrollKey(employee.clone()))
            .unwrap()
    });

    assert_eq!(stored_payroll.amount, updated_amount);
    assert_eq!(stored_payroll.last_payment_time, env.ledger().timestamp()); // Should remain unchanged
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_update_existing_escrow_invalid_employer() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let invalid_employer = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;

    env.mock_all_auths();

    client.initialize(&employer);

    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

    client.create_or_update_escrow(&invalid_employer, &employee, &token, &2000i128, &interval);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_create_escrow_invalid_interval() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let invalid_interval = 0u64;

    env.mock_all_auths();
    client.initialize(&employer);
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &invalid_interval);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_create_escrow_invalid_amount() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let invalid_amount = 0i128;
    let interval = 86400u64;

    env.mock_all_auths();
    client.initialize(&employer);
    client.create_or_update_escrow(&employer, &employee, &token, &invalid_amount, &interval);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_create_escrow_negative_amount() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let invalid_amount = -1000i128;
    let interval = 86400u64;

    env.mock_all_auths();
    client.initialize(&employer);
    client.create_or_update_escrow(&employer, &employee, &token, &invalid_amount, &interval);
}