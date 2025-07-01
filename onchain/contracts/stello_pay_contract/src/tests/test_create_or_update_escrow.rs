use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::payroll::PayrollContractClient;

#[test]
fn test_create_new_escrow() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days in seconds

    env.mock_all_auths();

    client.initialize(&employer);

    let created_payroll =
        client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval, &recurrence_frequency);

    let stored_payroll = client.get_payroll(&employee).unwrap();

    assert_eq!(created_payroll, stored_payroll);
    assert_eq!(stored_payroll.employer, employer);
    assert_eq!(stored_payroll.token, token);
    assert_eq!(stored_payroll.amount, amount);
    assert_eq!(stored_payroll.interval, interval);
    assert_eq!(stored_payroll.recurrence_frequency, recurrence_frequency);
    assert_eq!(stored_payroll.last_payment_time, env.ledger().timestamp());
    assert_eq!(stored_payroll.next_payout_timestamp, env.ledger().timestamp() + recurrence_frequency);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_create_new_escrow_unauthorized() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let owner = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days in seconds

    env.mock_all_auths();

    client.initialize(&owner);

    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval, &recurrence_frequency);
}

#[test]
fn test_update_existing_escrow_valid_employer() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let initial_amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days in seconds

    env.mock_all_auths();

    client.initialize(&employer);
    client.create_or_update_escrow(&employer, &employee, &token, &initial_amount, &interval, &recurrence_frequency);

    let initial_payment_time = client.get_payroll(&employee).unwrap().last_payment_time;

    let updated_amount = 2000i128;
    client.create_or_update_escrow(&employer, &employee, &token, &updated_amount, &interval, &recurrence_frequency);

    let stored_payroll = client.get_payroll(&employee).unwrap();

    assert_eq!(stored_payroll.amount, updated_amount);
    assert_eq!(stored_payroll.last_payment_time, initial_payment_time);
    assert_eq!(stored_payroll.recurrence_frequency, recurrence_frequency);
    assert_eq!(stored_payroll.next_payout_timestamp, env.ledger().timestamp() + recurrence_frequency);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_update_existing_escrow_invalid_employer() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let invalid_employer = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days in seconds

    env.mock_all_auths();

    client.initialize(&employer);

    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval, &recurrence_frequency);

    client.create_or_update_escrow(&invalid_employer, &employee, &token, &2000i128, &interval, &recurrence_frequency);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_create_escrow_invalid_interval() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let invalid_interval = 0u64;
    let recurrence_frequency = 2592000u64; // 30 days in seconds

    env.mock_all_auths();
    client.initialize(&employer);
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &invalid_interval, &recurrence_frequency);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_create_escrow_invalid_amount() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let invalid_amount = 0i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days in seconds

    env.mock_all_auths();
    client.initialize(&employer);
    client.create_or_update_escrow(&employer, &employee, &token, &invalid_amount, &interval, &recurrence_frequency);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_create_escrow_negative_amount() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let invalid_amount = -1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days in seconds

    env.mock_all_auths();
    client.initialize(&employer);
    client.create_or_update_escrow(&employer, &employee, &token, &invalid_amount, &interval, &recurrence_frequency);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_create_escrow_invalid_recurrence_frequency() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    let invalid_recurrence_frequency = 0u64;

    env.mock_all_auths();
    client.initialize(&employer);
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval, &invalid_recurrence_frequency);
}