#[cfg(test)]
use soroban_sdk::{testutils::Address as _, Address, Env, testutils::Ledger, testutils::LedgerInfo};

use crate::payroll::{Payroll, PayrollContract, PayrollContractClient, PayrollKey, PayrollError};

#[test]
fn test_get_payroll_success() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64; // 1 day in seconds

    env.mock_all_auths();
    
    client.initialize(&employer);
    // Create escrow first
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

    // Get payroll details
    let payroll = client.get_payroll(&employee);
    assert!(payroll.is_some());
    
    let payroll_data = payroll.unwrap();
    assert_eq!(payroll_data.employer, employer);
    assert_eq!(payroll_data.employee, employee);
    assert_eq!(payroll_data.token, token);
    assert_eq!(payroll_data.amount, amount);
    assert_eq!(payroll_data.interval, interval);
    assert_eq!(payroll_data.last_payment_time, env.ledger().timestamp());
}

#[test]
fn test_get_nonexistent_payroll() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employee = Address::generate(&env);
    
    env.mock_all_auths();
    // Try to get payroll for non-existent employee
    let payroll = client.get_payroll(&employee);
    assert!(payroll.is_none());
}

#[test]
fn test_disburse_salary_success() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let owner = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64; // 1 day in seconds

    env.mock_all_auths();

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    
    client.deposit_tokens(&employer, &token, &5000i128);

    // Create escrow first
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);
    
    // Advance time beyond interval
    let next_timestamp = env.ledger().timestamp() + interval + 1;
    env.ledger().set(LedgerInfo {
        timestamp: next_timestamp,
        protocol_version: env.ledger().protocol_version(),
        sequence_number: env.ledger().sequence(),
        network_id: env.ledger().network_id().into(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6312000,
    });

    // Disburse salary
    client.disburse_salary(&employer, &employee);

    // Verify last_payment_time was updated
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.last_payment_time, env.ledger().timestamp());
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_disburse_salary_unauthorized() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let invalid_caller = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;

    env.mock_all_auths();

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &5000i128);

    // Create escrow first
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);
    
    // Advance time beyond interval
    let next_timestamp = env.ledger().timestamp() + interval + 1;
    env.ledger().set(LedgerInfo {
        timestamp: next_timestamp,
        protocol_version: env.ledger().protocol_version(),
        sequence_number: env.ledger().sequence(),
        network_id: env.ledger().network_id().into(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6312000,
    });

    // Try to disburse with invalid caller
    client.disburse_salary(&invalid_caller, &employee);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_disburse_salary_interval_not_reached() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let owner = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;

    env.mock_all_auths();

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &5000i128);

    // Create escrow first
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);
    
    // Try to disburse immediately (without advancing time)
    client.disburse_salary(&employer, &employee);
}

#[test]
fn test_employee_withdraw_success() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let owner = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64; // 1 day in seconds

    env.mock_all_auths();

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &5000i128);

    // Create escrow first
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);
    
    // Advance time beyond interval
    let next_timestamp = env.ledger().timestamp() + interval + 1;
    env.ledger().set(LedgerInfo {
        timestamp: next_timestamp,
        protocol_version: env.ledger().protocol_version(),
        sequence_number: env.ledger().sequence(),
        network_id: env.ledger().network_id().into(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6312000,
    });

    // Employee withdraws payment
    client.employee_withdraw(&employee);

    // Verify last_payment_time was updated
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.last_payment_time, env.ledger().timestamp());
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_employee_withdraw_interval_not_reached() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let owner = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;

    env.mock_all_auths();

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &5000i128);

    // Create escrow first
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);
    
    // Try to withdraw immediately (without advancing time)
    client.employee_withdraw(&employee);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_employee_withdraw_nonexistent_payroll() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employee = Address::generate(&env);
    let owner = Address::generate(&env);
    
    env.mock_all_auths();

    // Initialize contract
    client.initialize(&owner);

    // Try to withdraw without existing payroll
    client.employee_withdraw(&employee);
}

#[test]
fn test_boundary_values() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 1u64; // Minimum possible interval (1 second)

    env.mock_all_auths();
    client.initialize(&employer);

    // Create escrow with minimum interval
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);
    
    // Advance time exactly to the interval boundary
    let next_timestamp = env.ledger().timestamp() + interval;
    env.ledger().set(LedgerInfo {
        timestamp: next_timestamp,
        protocol_version: env.ledger().protocol_version(),
        sequence_number: env.ledger().sequence(),
        network_id: env.ledger().network_id().into(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6312000,
    });

    // Get payroll to verify boundary values
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.amount, amount);
    assert_eq!(payroll.interval, interval);
}

#[test]
fn test_multiple_disbursements() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let owner = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64; // 1 day in seconds

    env.mock_all_auths();

    // Initialize contract and deposit enough tokens for multiple payments
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &10000i128);

    // Create escrow
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);
    
    // First payment cycle
    let next_timestamp = env.ledger().timestamp() + interval + 1;
    env.ledger().set(LedgerInfo {
        timestamp: next_timestamp,
        protocol_version: env.ledger().protocol_version(),
        sequence_number: env.ledger().sequence(),
        network_id: env.ledger().network_id().into(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6312000,
    });
    client.disburse_salary(&employer, &employee);
    
    let first_payment_time = env.ledger().timestamp();
    
    // Second payment cycle
    let next_timestamp = env.ledger().timestamp() + interval + 1;
    env.ledger().set(LedgerInfo {
        timestamp: next_timestamp,
        protocol_version: env.ledger().protocol_version(),
        sequence_number: env.ledger().sequence(),
        network_id: env.ledger().network_id().into(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6312000,
    });
    client.disburse_salary(&employer, &employee);
    
    // Verify last_payment_time was updated correctly
    let payroll = client.get_payroll(&employee).unwrap();
    assert!(payroll.last_payment_time > first_payment_time);
    assert_eq!(payroll.last_payment_time, env.ledger().timestamp());
}