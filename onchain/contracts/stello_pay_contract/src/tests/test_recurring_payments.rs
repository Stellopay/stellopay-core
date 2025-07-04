#![cfg(test)]

use crate::payroll::{PayrollContract, PayrollContractClient};
use soroban_sdk::token::{StellarAssetClient as TokenAdmin, TokenClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal, Vec,
};

fn setup_token(env: &Env) -> (Address, TokenAdmin) {
    let token_admin = Address::generate(env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone());
    (
        token_contract_id.address(),
        TokenAdmin::new(&env, &token_contract_id.address()),
    )
}

#[test]
fn test_create_escrow_with_recurrence() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64; // 1 day in seconds
    let recurrence_frequency = 2592000u64; // 30 days in seconds

    env.mock_all_auths();

    client.initialize(&employer);
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval, &recurrence_frequency);

    let payroll = client.get_payroll(&employee);
    assert!(payroll.is_some());

    let payroll_data = payroll.unwrap();
    assert_eq!(payroll_data.employer, employer);
    assert_eq!(payroll_data.token, token);
    assert_eq!(payroll_data.amount, amount);
    assert_eq!(payroll_data.interval, interval);
    assert_eq!(payroll_data.recurrence_frequency, recurrence_frequency);
    assert_eq!(payroll_data.last_payment_time, env.ledger().timestamp());
    assert_eq!(payroll_data.next_payout_timestamp, env.ledger().timestamp() + recurrence_frequency);
}

#[test]
fn test_disburse_salary_with_recurrence() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64; // 1 day in seconds
    let recurrence_frequency = 2592000u64; // 30 days in seconds

    env.mock_all_auths();

    // Fund the employer with tokens
    token_admin.mint(&employer, &10000);

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &5000i128);

    // Create escrow with recurrence
    client.create_or_update_escrow(&employer, &employee, &token_address, &amount, &interval, &recurrence_frequency);

    // Advance time beyond next payout timestamp
    let next_timestamp = env.ledger().timestamp() + recurrence_frequency + 1;
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

    // Verify employee received tokens
    let token_client = TokenClient::new(&env, &token_address);
    let employee_balance = token_client.balance(&employee);
    assert_eq!(employee_balance, amount);

    // Verify timestamps were updated
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.last_payment_time, env.ledger().timestamp());
    assert_eq!(payroll.next_payout_timestamp, env.ledger().timestamp() + recurrence_frequency);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #9)")]
fn test_disburse_salary_before_next_payout_time() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days

    env.mock_all_auths();

    // Fund the employer with tokens
    token_admin.mint(&employer, &10000);

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &5000i128);

    // Create escrow with recurrence
    client.create_or_update_escrow(&employer, &employee, &token_address, &amount, &interval, &recurrence_frequency);

    // Try to disburse before next payout time (should fail)
    client.disburse_salary(&employer, &employee);
}

#[test]
fn test_is_eligible_for_disbursement() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days

    env.mock_all_auths();

    client.initialize(&employer);
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval, &recurrence_frequency);

    // Should not be eligible immediately after creation
    assert!(!client.is_eligible_for_disbursement(&employee));

    // Advance time beyond next payout timestamp
    let next_timestamp = env.ledger().timestamp() + recurrence_frequency + 1;
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

    // Should be eligible now
    assert!(client.is_eligible_for_disbursement(&employee));
}

#[test]
fn test_process_recurring_disbursements() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days

    env.mock_all_auths();

    // Fund the employer with tokens
    token_admin.mint(&employer, &10000);

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &5000i128);

    // Create escrows for two employees
    client.create_or_update_escrow(&employer, &employee1, &token_address, &amount, &interval, &recurrence_frequency);
    client.create_or_update_escrow(&employer, &employee2, &token_address, &amount, &interval, &recurrence_frequency);

    // Advance time beyond next payout timestamp
    let next_timestamp = env.ledger().timestamp() + recurrence_frequency + 1;
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

    // Create vector of employees
    let mut employees = Vec::new(&env);
    employees.push_back(employee1.clone());
    employees.push_back(employee2.clone());

    // Process recurring disbursements
    let processed_employees = client.process_recurring_disbursements(&employer, &employees);
    assert_eq!(processed_employees.len(), 2);

    // Verify both employees received tokens
    let token_client = TokenClient::new(&env, &token_address);
    let employee1_balance = token_client.balance(&employee1);
    let employee2_balance = token_client.balance(&employee2);
    assert_eq!(employee1_balance, amount);
    assert_eq!(employee2_balance, amount);
}

#[test]
#[should_panic(expected = "Unauthorized")]
fn test_process_recurring_disbursements_unauthorized() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let unauthorized = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&employer);

    let employees = Vec::new(&env);
    
    // Try to process recurring disbursements with unauthorized user
    client.process_recurring_disbursements(&unauthorized, &employees);
}

#[test]
fn test_get_next_payout_timestamp() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days

    env.mock_all_auths();

    client.initialize(&employer);
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval, &recurrence_frequency);

    let next_payout = client.get_next_payout_timestamp(&employee);
    assert!(next_payout.is_some());
    assert_eq!(next_payout.unwrap(), env.ledger().timestamp() + recurrence_frequency);
}

#[test]
fn test_get_recurrence_frequency() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days

    env.mock_all_auths();

    client.initialize(&employer);
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval, &recurrence_frequency);

    let frequency = client.get_recurrence_frequency(&employee);
    assert!(frequency.is_some());
    assert_eq!(frequency.unwrap(), recurrence_frequency);
}

#[test]
fn test_multiple_recurring_disbursements() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 86400u64; // 1 day for testing

    env.mock_all_auths();

    // Fund the employer with tokens
    token_admin.mint(&employer, &10000);

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &5000i128);

    // Create escrow with recurrence
    client.create_or_update_escrow(&employer, &employee, &token_address, &amount, &interval, &recurrence_frequency);

    let token_client = TokenClient::new(&env, &token_address);
    let mut total_disbursed = 0;

    // Process multiple disbursements
    for i in 1..=3 {
        // Advance time beyond next payout timestamp
        let next_timestamp = env.ledger().timestamp() + recurrence_frequency + 1;
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
        total_disbursed += amount;

        // Verify employee received tokens
        let employee_balance = token_client.balance(&employee);
        assert_eq!(employee_balance, total_disbursed);
    }
}

#[test]
fn test_update_escrow_with_recurrence() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    let initial_recurrence = 2592000u64; // 30 days
    let updated_recurrence = 604800u64; // 7 days

    env.mock_all_auths();

    client.initialize(&employer);
    
    // Create initial escrow
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval, &initial_recurrence);
    
    let initial_payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(initial_payroll.recurrence_frequency, initial_recurrence);

    // Update escrow with new recurrence frequency
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval, &updated_recurrence);
    
    let updated_payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(updated_payroll.recurrence_frequency, updated_recurrence);
    assert_eq!(updated_payroll.next_payout_timestamp, env.ledger().timestamp() + updated_recurrence);
} 