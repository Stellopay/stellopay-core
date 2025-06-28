#![cfg(test)]

use crate::payroll::{Payroll, PayrollContract, PayrollContractClient, PayrollError, PayrollKey};
use soroban_sdk::token::{StellarAssetClient as TokenAdmin, TokenClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    Address, Env, IntoVal,
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
fn test_get_payroll_success() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64; // 1 day in seconds

    env.mock_all_auths();

    client.initialize(&employer);
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

    let payroll = client.get_payroll(&employee);
    assert!(payroll.is_some());

    let payroll_data = payroll.unwrap();
    assert_eq!(payroll_data.employer, employer);
    assert_eq!(payroll_data.token, token);
    assert_eq!(payroll_data.amount, amount);
    assert_eq!(payroll_data.interval, interval);
    assert_eq!(payroll_data.last_payment_time, env.ledger().timestamp());
}

#[test]
fn test_get_nonexistent_payroll() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employee = Address::generate(&env);

    env.mock_all_auths();

    let payroll = client.get_payroll(&employee);
    assert!(payroll.is_none());
}

#[test]
fn test_disburse_salary_success() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64; // 1 day in seconds

    env.mock_all_auths();

    // Fund the employer with tokens
    token_admin.mint(&employer, &10000);

    // Verify minting
    let token_client = TokenClient::new(&env, &token_address);
    let employer_balance = token_client.balance(&employer);
    assert_eq!(employer_balance, 10000);

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &5000i128);

    // Verify deposit
    let payroll_contract_balance = token_client.balance(&contract_id);
    assert_eq!(payroll_contract_balance, 5000);

    // Create escrow first
    client.create_or_update_escrow(&employer, &employee, &token_address, &amount, &interval);

    // Advance time beyond interval
    let next_timestamp = env.ledger().timestamp() + interval + 1;
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
    let employee_balance = token_client.balance(&employee);
    assert_eq!(employee_balance, amount);

    // Verify last_payment_time was updated
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.last_payment_time, env.ledger().timestamp());
}

#[test]
#[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
fn test_disburse_salary_unauthorized() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let unauthorized = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;

    // Set up the contract with proper authorization for setup operations
    env.mock_auths(&[
        MockAuth {
            address: &employer,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "initialize",
                args: (&employer,).into_val(&env),
                sub_invokes: &[],
            },
        },
        MockAuth {
            address: &employer,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "deposit_tokens",
                args: (&employer, &token_address, &5000i128).into_val(&env),
                sub_invokes: &[],
            },
        },
        MockAuth {
            address: &employer,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "create_or_update_escrow",
                args: (&employer, &employee, &token_address, &amount, &interval).into_val(&env),
                sub_invokes: &[],
            },
        },
    ]);

    // Fund the employer with tokens
    token_admin.mint(&employer, &10000);

    // Verify minting
    let token_client = TokenClient::new(&env, &token_address);
    let employer_balance = token_client.balance(&employer);
    assert_eq!(employer_balance, 10000);

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &5000i128);

    // Create escrow first
    client.create_or_update_escrow(&employer, &employee, &token_address, &amount, &interval);

    // Advance time beyond interval
    let next_timestamp = env.ledger().timestamp() + interval + 1;
    env.ledger().set(LedgerInfo {
        timestamp: next_timestamp,
        protocol_version: 22,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 0,
        min_temp_entry_ttl: 0,
        min_persistent_entry_ttl: 0,
        max_entry_ttl: 0,
    });

    // Now try to disburse salary with unauthorized user - NO mock auth for this call
    // This should panic because unauthorized.require_auth() will fail
    client.disburse_salary(&unauthorized, &employee);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_disburse_salary_interval_not_reached() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;

    env.mock_all_auths();

    // Fund the employer with tokens
    token_admin.mint(&employer, &10000);

    // Verify minting
    let token_client = TokenClient::new(&env, &token_address);
    let employer_balance = token_client.balance(&employer);
    assert_eq!(employer_balance, 10000);

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &5000i128);

    // Create escrow first
    client.create_or_update_escrow(&employer, &employee, &token_address, &amount, &interval);

    // Try to disburse immediately (without advancing time)
    client.disburse_salary(&employer, &employee);
}

#[test]
fn test_employee_withdraw_success() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;

    env.mock_all_auths();

    // Fund the employer with tokens
    token_admin.mint(&employer, &10000);

    // Verify minting
    let token_client = TokenClient::new(&env, &token_address);
    let employer_balance = token_client.balance(&employer);
    assert_eq!(employer_balance, 10000);

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &5000i128);

    // Create escrow first
    client.create_or_update_escrow(&employer, &employee, &token_address, &amount, &interval);

    // Advance time beyond interval
    let next_timestamp = env.ledger().timestamp() + interval + 1;
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

    client.employee_withdraw(&employee);

    // Verify employee received tokens
    let employee_balance = token_client.balance(&employee);
    assert_eq!(employee_balance, amount);

    // Verify last_payment_time was updated
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.last_payment_time, env.ledger().timestamp());
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_employee_withdraw_interval_not_reached() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;

    env.mock_all_auths();

    // Fund the employer with tokens
    token_admin.mint(&employer, &10000);

    // Verify minting
    let token_client = TokenClient::new(&env, &token_address);
    let employer_balance = token_client.balance(&employer);
    assert_eq!(employer_balance, 10000);

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &5000i128);

    // Create escrow first
    client.create_or_update_escrow(&employer, &employee, &token_address, &amount, &interval);

    // Try to withdraw immediately (without advancing time)
    client.employee_withdraw(&employee);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_employee_withdraw_nonexistent_payroll() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employee = Address::generate(&env);
    let owner = Address::generate(&env);

    env.mock_all_auths();

    client.employee_withdraw(&employee);
}

#[test]
fn test_boundary_values() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let min_amount = 1i128;
    let min_interval = 1u64;

    env.mock_all_auths();

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

    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.amount, min_amount);
    assert_eq!(payroll.interval, min_interval);
}

#[test]
fn test_multiple_disbursements() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;

    env.mock_all_auths();

    // Fund the employer with tokens
    token_admin.mint(&employer, &10000);

    // Verify minting
    let token_client = TokenClient::new(&env, &token_address);
    let employer_balance = token_client.balance(&employer);
    assert_eq!(employer_balance, 10000);

    // Initialize contract and deposit enough tokens for multiple payments
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &5000i128);

    // Create escrow
    client.create_or_update_escrow(&employer, &employee, &token_address, &amount, &interval);

    // First payment cycle
    let next_timestamp = env.ledger().timestamp() + interval + 1;
    env.ledger().set(LedgerInfo {
        timestamp: first_disbursement_time,
        protocol_version: 22,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6312000,
    });
    client.disburse_salary(&employer, &employee);
  
    // Verify first payment was made
    let employee_balance = token_client.balance(&employee);
    assert_eq!(employee_balance, amount);

    let first_payment_time = env.ledger().timestamp();

    // Second payment cycle
    let next_timestamp = env.ledger().timestamp() + interval + 1;
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.last_payment_time, first_disbursement_time);

    let second_disbursement_time = first_disbursement_time + interval + 1;
    env.ledger().set(LedgerInfo {
        timestamp: second_disbursement_time,
        protocol_version: 22,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6312000,
    });
    client.disburse_salary(&employer, &employee);

    // Verify second payment was made
    let employee_balance = token_client.balance(&employee);
    assert_eq!(employee_balance, 2 * amount);

    // Verify last_payment_time was updated correctly
    let payroll = client.get_payroll(&employee).unwrap();
    assert!(payroll.last_payment_time > first_payment_time);
    assert_eq!(payroll.last_payment_time, env.ledger().timestamp());
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_payment_insufficient_employer_pool() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64; // 1 day in seconds

    env.mock_all_auths();

    // Fund the employer with tokens
    token_admin.mint(&employer, &10000);

    // Verify minting
    let token_client = TokenClient::new(&env, &token_address);
    let employer_balance = token_client.balance(&employer);
    assert_eq!(employer_balance, 10000);

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &500i128); // Insufficient for one `amount` payment

    // Create escrow first
    client.create_or_update_escrow(&employer, &employee, &token_address, &amount, &interval);

    // Advance time to make disbursement eligible
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

    client.employee_withdraw(&employee);
    // // Try to disburse salary
    // let result: =  client.disburse_salary(&employer, &employee);
    // assert_eq!(result, Err(PayrollError::InsufficientBalance));

    // let res = client.employee_withdraw(&employee);
    // assert_eq!(res, Err(PayrollError::InsufficientBalance));
}
