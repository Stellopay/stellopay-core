#[cfg(test)]
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal,
};

use crate::payroll::PayrollContractClient;

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

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64; // 1 day in seconds

    env.mock_all_auths();

    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &5000i128);
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

    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.last_payment_time, env.ledger().timestamp());
}

#[test]
#[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
fn test_disburse_salary_unauthorized() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
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
                args: (&employer, &token, &5000i128).into_val(&env),
                sub_invokes: &[],
            },
        },
        MockAuth {
            address: &employer,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "create_or_update_escrow",
                args: (&employer, &employee, &token, &amount, &interval).into_val(&env),
                sub_invokes: &[],
            },
        },
    ]);

    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &5000i128);
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

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

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;

    env.mock_all_auths();

    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &5000i128);
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

    client.disburse_salary(&employer, &employee);
}

#[test]
fn test_employee_withdraw_success() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;

    env.mock_all_auths();

    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &5000i128);
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

    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.last_payment_time, env.ledger().timestamp());
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_employee_withdraw_interval_not_reached() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;

    env.mock_all_auths();

    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &5000i128);
    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

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

    env.mock_all_auths();

    client.initialize(&employer);
    client.create_or_update_escrow(&employer, &employee, &token, &min_amount, &min_interval);

    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.amount, min_amount);
    assert_eq!(payroll.interval, min_interval);
}

#[test]
fn test_multiple_disbursements() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;

    env.mock_all_auths();
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token, &5000i128);

    client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

    // First disbursement
    let first_disbursement_time = env.ledger().timestamp() + interval + 1;
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
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.last_payment_time, first_disbursement_time);

    // Second disbursement
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
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.last_payment_time, second_disbursement_time);
}