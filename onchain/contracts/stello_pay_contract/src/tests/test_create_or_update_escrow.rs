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

    let created_payroll = client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &amount,
        &interval,
        &recurrence_frequency,
    );

    let stored_payroll = client.get_payroll(&employee).unwrap();

    assert_eq!(created_payroll, stored_payroll);
    assert_eq!(stored_payroll.employer, employer);
    assert_eq!(stored_payroll.token, token);
    assert_eq!(stored_payroll.amount, amount);
    assert_eq!(stored_payroll.interval, interval);
    assert_eq!(stored_payroll.recurrence_frequency, recurrence_frequency);
    assert_eq!(stored_payroll.last_payment_time, env.ledger().timestamp());
    assert_eq!(
        stored_payroll.next_payout_timestamp,
        env.ledger().timestamp() + recurrence_frequency
    );
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

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &amount,
        &interval,
        &recurrence_frequency,
    );
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
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &initial_amount,
        &interval,
        &recurrence_frequency,
    );

    let initial_payment_time = client.get_payroll(&employee).unwrap().last_payment_time;

    let updated_amount = 2000i128;
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &updated_amount,
        &interval,
        &recurrence_frequency,
    );

    let stored_payroll = client.get_payroll(&employee).unwrap();

    assert_eq!(stored_payroll.amount, updated_amount);
    assert_eq!(stored_payroll.last_payment_time, initial_payment_time);
    assert_eq!(stored_payroll.recurrence_frequency, recurrence_frequency);
    assert_eq!(
        stored_payroll.next_payout_timestamp,
        env.ledger().timestamp() + recurrence_frequency
    );
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

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &amount,
        &interval,
        &recurrence_frequency,
    );

    client.create_or_update_escrow(
        &invalid_employer,
        &employee,
        &token,
        &2000i128,
        &interval,
        &recurrence_frequency,
    );
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
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &amount,
        &invalid_interval,
        &recurrence_frequency,
    );
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
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &invalid_amount,
        &interval,
        &recurrence_frequency,
    );
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
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &invalid_amount,
        &interval,
        &recurrence_frequency,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
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
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &amount,
        &interval,
        &invalid_recurrence_frequency,
    );
}

// Additional edge case tests

#[test]
fn test_create_escrow_with_maximum_amount() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let max_amount = i128::MAX;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64;

    env.mock_all_auths();
    client.initialize(&employer);
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &max_amount,
        &interval,
        &recurrence_frequency,
    );

    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.amount, max_amount);
}

#[test]
fn test_create_escrow_with_minimum_amount() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let min_amount = 1i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64;

    env.mock_all_auths();
    client.initialize(&employer);
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &min_amount,
        &interval,
        &recurrence_frequency,
    );

    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.amount, min_amount);
}

#[test]
fn test_create_escrow_with_maximum_interval() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let max_interval = u32::MAX as u64;
    let recurrence_frequency = 2592000u64;

    env.mock_all_auths();
    client.initialize(&employer);
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &amount,
        &max_interval,
        &recurrence_frequency,
    );

    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.interval, max_interval);
}

#[test]
fn test_create_escrow_with_maximum_recurrence() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    let max_recurrence = u32::MAX as u64;

    env.mock_all_auths();
    client.initialize(&employer);
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &amount,
        &interval,
        &max_recurrence,
    );

    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.recurrence_frequency, max_recurrence);
}

#[test]
fn test_update_escrow_same_parameters() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64;

    env.mock_all_auths();
    client.initialize(&employer);

    // Create escrow
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &amount,
        &interval,
        &recurrence_frequency,
    );

    let initial_payroll = client.get_payroll(&employee).unwrap();

    // Update with same parameters
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &amount,
        &interval,
        &recurrence_frequency,
    );

    let updated_payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(initial_payroll.amount, updated_payroll.amount);
    assert_eq!(initial_payroll.interval, updated_payroll.interval);
    assert_eq!(
        initial_payroll.recurrence_frequency,
        updated_payroll.recurrence_frequency
    );
}

#[test]
fn test_create_escrow_same_employee_different_employers() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer1 = Address::generate(&env);
    let employer2 = Address::generate(&env);
    let employee = Address::generate(&env);
    let token1 = Address::generate(&env);
    let token2 = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64;

    env.mock_all_auths();
    client.initialize(&employer1);
    // Note: Can't initialize twice with same contract, so we'll test with just one employer

    // Create escrow with first employer
    client.create_or_update_escrow(
        &employer1,
        &employee,
        &token1,
        &amount,
        &interval,
        &recurrence_frequency,
    );

    // Create escrow with same employer but different token (should overwrite)
    client.create_or_update_escrow(
        &employer1,
        &employee,
        &token2,
        &amount,
        &interval,
        &recurrence_frequency,
    );

    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.employer, employer1);
    assert_eq!(payroll.token, token2);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_create_escrow_zero_interval() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 0u64;
    let recurrence_frequency = 2592000u64;

    env.mock_all_auths();
    client.initialize(&employer);
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &amount,
        &interval,
        &recurrence_frequency,
    );
}

#[test]
fn test_create_escrow_very_short_interval() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 1u64; // 1 second
    let recurrence_frequency = 1u64; // 1 second

    env.mock_all_auths();
    client.initialize(&employer);
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &amount,
        &interval,
        &recurrence_frequency,
    );

    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.interval, 1);
    assert_eq!(payroll.recurrence_frequency, 1);
}

#[test]
fn test_create_escrow_multiple_updates() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    // Create initial escrow
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &1000i128,
        &86400u64,
        &2592000u64,
    );

    // Update amount
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &2000i128,
        &86400u64,
        &2592000u64,
    );

    // Update interval
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &2000i128,
        &604800u64, // 7 days
        &2592000u64,
    );

    // Update recurrence
    client.create_or_update_escrow(
        &employer, &employee, &token, &2000i128, &604800u64, &604800u64, // 7 days
    );

    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.amount, 2000);
    assert_eq!(payroll.interval, 604800);
    assert_eq!(payroll.recurrence_frequency, 604800);
}
