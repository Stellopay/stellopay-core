use crate::payroll::PayrollContractClient;
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    Address, Env,
};

#[test]
fn test_deposit_tokens_success() {
    let env = Env::default();
    let contract_id = env.register_contract_wasm(None, crate::payroll::WASM);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let deposit_amount = 5000i128;

    env.mock_all_auths();

    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &deposit_amount);

    let balance = client.get_employer_balance(&employer, &token);
    assert_eq!(balance, deposit_amount);
}

#[test]
fn test_deposit_multiple_times() {
    let env = Env::default();
    let contract_id = env.register_contract_wasm(None, crate::payroll::WASM);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let first_deposit = 3000i128;
    let second_deposit = 2000i128;

    env.mock_all_auths();

    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &first_deposit);
    client.deposit_tokens(&employer, &token, &second_deposit);

    let balance = client.get_employer_balance(&employer, &token);
    assert_eq!(balance, first_deposit + second_deposit);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #3)")]
fn test_deposit_zero_amount() {
    let env = Env::default();
    let contract_id = env.register_contract_wasm(None, crate::payroll::WASM);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &0i128);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #3)")]
fn test_deposit_negative_amount() {
    let env = Env::default();
    let contract_id = env.register_contract_wasm(None, crate::payroll::WASM);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &-100i128);
}

#[test]
fn test_get_employer_balance_initial() {
    let env = Env::default();
    let contract_id = env.register_contract_wasm(None, crate::payroll::WASM);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let token = Address::generate(&env);

    let balance = client.get_employer_balance(&employer, &token);
    assert_eq!(balance, 0);
}

#[test]
fn test_disburse_salary_deducts_balance() {
    let env = Env::default();
    let contract_id = env.register_contract_wasm(None, crate::payroll::WASM);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let initial_deposit = 5000i128;
    let interval = 86400u64;

    env.mock_all_auths();

    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &initial_deposit);
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

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

    let final_balance = client.get_employer_balance(&employer, &token);
    assert_eq!(final_balance, initial_deposit - amount);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_disburse_salary_insufficient_balance() {
    let env = Env::default();
    let contract_id = env.register_contract_wasm(None, crate::payroll::WASM);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let initial_deposit = 500i128;
    let interval = 86400u64;

    env.mock_all_auths();

    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &initial_deposit);
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

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
}

#[test]
fn test_employee_withdraw_deducts_balance() {
    let env = Env::default();
    let contract_id = env.register_contract_wasm(None, crate::payroll::WASM);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let initial_deposit = 5000i128;
    let interval = 86400u64;

    env.mock_all_auths();

    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &initial_deposit);
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

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

    let final_balance = client.get_employer_balance(&employer, &token);
    assert_eq!(final_balance, initial_deposit - amount);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_employee_withdraw_insufficient_balance() {
    let env = Env::default();
    let contract_id = env.register_contract_wasm(None, crate::payroll::WASM);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let initial_deposit = 500i128;
    let interval = 86400u64;

    env.mock_all_auths();

    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &initial_deposit);
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

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
}

#[test]
fn test_disburse_salary_deducts_balance_with_setup() {
    let env = Env::default();
    let contract_id = env.register_contract_wasm(None, crate::payroll::WASM);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let initial_deposit = 5000i128;
    let interval = 86400u64;

    env.mock_all_auths();

    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &initial_deposit);
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

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

    let final_balance = client.get_employer_balance(&employer, &token);
    assert_eq!(final_balance, initial_deposit - amount);
}

#[test]
fn test_disburse_salary_deducts_balance_with_setup_and_deposit() {
    let env = Env::default();
    let contract_id = env.register_contract_wasm(None, crate::payroll::WASM);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let initial_deposit = 5000i128;
    let interval = 86400u64;

    env.mock_all_auths();

    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &initial_deposit);
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

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

    let final_balance = client.get_employer_balance(&employer, &token);
    assert_eq!(final_balance, initial_deposit - amount);
}