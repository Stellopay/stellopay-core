#![cfg(test)]

use crate::payroll::{PayrollContract, PayrollContractClient};
use soroban_sdk::token::{StellarAssetClient as TokenAdmin, TokenClient};
use soroban_sdk::TryFromVal;
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger, LedgerInfo, MockAuth, MockAuthInvoke},
    vec, Address, Env, IntoVal, Symbol,
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

    let payroll = client.get_payroll(&employee);
    assert!(payroll.is_some());

    let payroll_data = payroll.unwrap();
    assert_eq!(payroll_data.employer, employer);
    assert_eq!(payroll_data.token, token);
    assert_eq!(payroll_data.amount, amount);
    assert_eq!(payroll_data.interval, interval);
    assert_eq!(payroll_data.recurrence_frequency, recurrence_frequency);
    assert_eq!(payroll_data.last_payment_time, env.ledger().timestamp());
    assert_eq!(
        payroll_data.next_payout_timestamp,
        env.ledger().timestamp() + recurrence_frequency
    );
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
    let recurrence_frequency = 2592000u64; // 30 days in seconds

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
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &amount,
        &interval,
        &recurrence_frequency,
    );

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
    let recurrence_frequency = 2592000u64; // 30 days in seconds

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
                args: (
                    &employer,
                    &employee,
                    &token_address,
                    &amount,
                    &interval,
                    &recurrence_frequency,
                )
                    .into_val(&env),
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
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &amount,
        &interval,
        &recurrence_frequency,
    );

    // Advance time beyond next payout timestamp
    let next_timestamp = env.ledger().timestamp() + recurrence_frequency + 1;
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
#[should_panic(expected = "Error(Contract, #9)")]
fn test_disburse_salary_interval_not_reached() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days in seconds

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
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &amount,
        &interval,
        &recurrence_frequency,
    );

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
    let recurrence_frequency = 2592000u64; // 30 days in seconds

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
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &amount,
        &interval,
        &recurrence_frequency,
    );

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

    client.employee_withdraw(&employee);

    // Verify employee received tokens
    let employee_balance = token_client.balance(&employee);
    assert_eq!(employee_balance, amount);

    // Verify last_payment_time was updated
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.last_payment_time, env.ledger().timestamp());
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_employee_withdraw_interval_not_reached() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days in seconds

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
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &amount,
        &interval,
        &recurrence_frequency,
    );

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
    let recurrence_frequency = 2592000u64; // 30 days in seconds

    env.mock_all_auths();

    // Create escrow with minimum interval
    client.initialize(&employer);
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &min_amount,
        &min_interval,
        &recurrence_frequency,
    );

    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.amount, min_amount);
    assert_eq!(payroll.interval, min_interval);
    assert_eq!(payroll.recurrence_frequency, recurrence_frequency);
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
    let recurrence_frequency = 2592000u64; // 30 days in seconds

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
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &amount,
        &interval,
        &recurrence_frequency,
    );

    // First payment cycle
    let first_disbursement_time = env.ledger().timestamp() + recurrence_frequency + 1;
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
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.last_payment_time, first_disbursement_time);

    let second_disbursement_time = first_disbursement_time + recurrence_frequency + 1;
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
    let recurrence_frequency = 2592000u64; // 30 days in seconds

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
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &amount,
        &interval,
        &recurrence_frequency,
    );

    // Advance time to make disbursement eligible
    let next_timestamp = env.ledger().timestamp() + recurrence_frequency + 1;
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

#[test]
#[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
fn test_employee_withdraw_unauthorized() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days in seconds

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
                args: (
                    &employer,
                    &employee,
                    &token_address,
                    &amount,
                    &interval,
                    &recurrence_frequency,
                )
                    .into_val(&env),
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
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &amount,
        &interval,
        &recurrence_frequency,
    );

    // Advance time beyond next payout timestamp
    let next_timestamp = env.ledger().timestamp() + recurrence_frequency + 1;
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
    client.employee_withdraw(&employee);
}

#[test]
fn test_disburse_salary_emit_event() {
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

    // Verify minting
    let token_client = TokenClient::new(&env, &token_address);
    let employer_balance = token_client.balance(&employer);
    assert_eq!(employer_balance, 10000);

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &5000i128);

    // Create escrow first
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &amount,
        &interval,
        &recurrence_frequency,
    );

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

    // Verify event was emitted
    let events = env.events().all();

    // Token `transfer`, `SalaryDisbursed` and record_audit events are expected
    assert_eq!(events.len(), 4);

    // Get the emitted `SalaryDisbursed` event data
    let data = events.get(3).unwrap();

    assert_eq!(data.0, contract_id);

    let salary_topic = data.1;
    assert_eq!(
        salary_topic,
        vec![&env, Symbol::new(&env, "SalaryDisbursed")].into_val(&env)
    );

    let salary_data = data.2;

    let expected_data: soroban_sdk::Map<soroban_sdk::Symbol, soroban_sdk::Val> =
        soroban_sdk::Map::from_array(
            &env,
            [
                (
                    Symbol::new(&env, "employer"),
                    employer.clone().into_val(&env),
                ),
                (
                    Symbol::new(&env, "employee"),
                    employee.clone().into_val(&env),
                ),
                (
                    Symbol::new(&env, "token"),
                    token_address.clone().into_val(&env),
                ),
                (Symbol::new(&env, "amount"), amount.into_val(&env)),
                (
                    Symbol::new(&env, "timestamp"),
                    env.ledger().timestamp().into_val(&env),
                ),
            ],
        );

    let salary_data_map: soroban_sdk::Map<soroban_sdk::Symbol, soroban_sdk::Val> =
        soroban_sdk::Map::try_from_val(&env, &salary_data).unwrap();

    assert!(
        salary_data_map == expected_data,
        "Expected: {:?}, Actual: {:?}",
        expected_data,
        salary_data_map
    );
}

// Additional edge case tests

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_disburse_salary_nonexistent_employee() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let nonexistent_employee = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    // Try to disburse salary for non-existent employee
    client.disburse_salary(&employer, &nonexistent_employee);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_employee_withdraw_nonexistent_employee() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let nonexistent_employee = Address::generate(&env);

    env.mock_all_auths();

    // Try to withdraw for non-existent employee
    client.employee_withdraw(&nonexistent_employee);
}

#[test]
fn test_get_balance_nonexistent_employer() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let nonexistent_employer = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();

    // Should return 0 for non-existent employer
    let balance = client.get_employer_balance(&nonexistent_employer, &token);
    assert_eq!(balance, 0);
}

#[test]
fn test_get_balance_nonexistent_token() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let nonexistent_token = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    // Should return 0 for non-existent token
    let balance = client.get_employer_balance(&employer, &nonexistent_token);
    assert_eq!(balance, 0);
}

#[test]
fn test_very_small_amounts() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    // Test with very small amount (1 unit)
    let tiny_amount = 1i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64;

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &tiny_amount,
        &interval,
        &recurrence_frequency,
    );

    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.amount, tiny_amount);
}

#[test]
fn test_very_short_intervals() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    // Test with very short interval (1 second)
    let amount = 1000i128;
    let short_interval = 1u64;
    let recurrence_frequency = 60u64; // 1 minute

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &amount,
        &short_interval,
        &recurrence_frequency,
    );

    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.interval, short_interval);
    assert_eq!(payroll.recurrence_frequency, recurrence_frequency);
}
