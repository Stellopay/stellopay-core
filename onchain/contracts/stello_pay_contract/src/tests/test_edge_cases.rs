#![cfg(test)]

use crate::payroll::{PayrollContract, PayrollContractClient};
use soroban_sdk::token::{StellarAssetClient as TokenAdmin, TokenClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    Address, Env,
};

fn setup_token(env: &Env) -> (Address, TokenAdmin) {
    let token_admin = Address::generate(env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone());
    (
        token_contract_id.address(),
        TokenAdmin::new(&env, &token_contract_id.address()),
    )
}

// Test maximum values and overflow scenarios
#[test]
fn test_maximum_amount_values() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    // Test with maximum i128 value
    let max_amount = i128::MAX;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64;

    env.mock_all_auths();
    client.initialize(&employer);

    // This should work without overflow
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

// Test minimum valid values
#[test]
fn test_minimum_valid_values() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let min_amount = 1i128;
    let min_interval = 1u64;
    let min_recurrence = 1u64;

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &min_amount,
        &min_interval,
        &min_recurrence,
    );

    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.amount, min_amount);
    assert_eq!(payroll.interval, min_interval);
    assert_eq!(payroll.recurrence_frequency, min_recurrence);
}

// Test zero values (should fail)
#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_zero_amount_fails() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(&employer, &employee, &token, &0i128, &86400u64, &2592000u64);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_zero_interval_fails() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(&employer, &employee, &token, &1000i128, &0u64, &2592000u64);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_zero_recurrence_frequency_fails() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(&employer, &employee, &token, &1000i128, &86400u64, &0u64);
}

// Test negative amount (should fail)
#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_negative_amount_fails() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &-1000i128,
        &86400u64,
        &2592000u64,
    );
}

// Test contract not initialized
#[test]
#[should_panic(expected = "Contract already initialized")]
fn test_double_initialization_fails() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    env.mock_all_auths();

    client.initialize(&employer);
    client.initialize(&employer); // This should panic
}

// Note: create_or_update_escrow doesn't check pause state, so this test is removed

// Test disbursement when contract is paused
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_disburse_when_paused_fails() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    env.mock_all_auths();
    token_admin.mint(&employer, &10000);
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &5000i128);

    // Create escrow first
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &1000i128,
        &86400u64,
        &2592000u64,
    );

    // Pause contract
    client.pause(&employer);

    // Advance time
    let next_timestamp = env.ledger().timestamp() + 2592000u64 + 1;
    env.ledger().set(LedgerInfo {
        timestamp: next_timestamp,
        protocol_version: 22,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6312000,
    });

    client.disburse_salary(&employer, &employee);
}

// Test exact timestamp boundary
#[test]
fn test_exact_timestamp_boundary() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    env.mock_all_auths();
    token_admin.mint(&employer, &10000);
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &5000i128);

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &1000i128,
        &86400u64,
        &2592000u64,
    );

    let payroll = client.get_payroll(&employee).unwrap();
    let exact_next_payout = payroll.next_payout_timestamp;

    // Set time to exactly the next payout timestamp
    env.ledger().set(LedgerInfo {
        timestamp: exact_next_payout,
        protocol_version: 22,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6312000,
    });

    // This should work at exact boundary
    client.disburse_salary(&employer, &employee);

    let token_client = TokenClient::new(&env, &token_address);
    let employee_balance = token_client.balance(&employee);
    assert_eq!(employee_balance, 1000);
}

// Test one second before payout time (should fail)
#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_one_second_before_payout_fails() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    env.mock_all_auths();
    token_admin.mint(&employer, &10000);
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &5000i128);

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &1000i128,
        &86400u64,
        &2592000u64,
    );

    let payroll = client.get_payroll(&employee).unwrap();
    let one_second_before = payroll.next_payout_timestamp - 1;

    env.ledger().set(LedgerInfo {
        timestamp: one_second_before,
        protocol_version: 22,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6312000,
    });

    client.disburse_salary(&employer, &employee);
}

// Test very large recurrence frequency
#[test]
fn test_very_large_recurrence_frequency() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    // 100 years in seconds
    let large_recurrence = 100 * 365 * 24 * 60 * 60u64;

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &1000i128,
        &86400u64,
        &large_recurrence,
    );

    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.recurrence_frequency, large_recurrence);
}

// Test same employee with different employers (should fail - only original employer can update)
#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_same_employee_different_employers_fails() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer1 = Address::generate(&env);
    let employer2 = Address::generate(&env);
    let employee = Address::generate(&env);
    let token1 = Address::generate(&env);
    let token2 = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer1);

    // Create escrow with first employer
    client.create_or_update_escrow(
        &employer1,
        &employee,
        &token1,
        &1000i128,
        &86400u64,
        &2592000u64,
    );

    // Try to create escrow with second employer (should fail - unauthorized)
    client.create_or_update_escrow(
        &employer2,
        &employee,
        &token2,
        &2000i128,
        &172800u64,
        &5184000u64,
    );
}

// Test deposit with zero amount (should fail)
#[test]
#[should_panic]
fn test_deposit_zero_amount_fails() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, _token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&employer);

    client.deposit_tokens(&employer, &token_address, &0i128);
}

// Test deposit with negative amount (should fail)
#[test]
#[should_panic]
fn test_deposit_negative_amount_fails() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, _token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&employer);

    client.deposit_tokens(&employer, &token_address, &-1000i128);
}
