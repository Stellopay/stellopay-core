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

    let amount = 1000;
    let interval = 86400;

    env.mock_all_auths();

    client.create_or_update_escrow(&employer, &employee, &amount, &interval);

    let stored_payroll: Payroll = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get(&PayrollKey(employee.clone()))
            .unwrap()
    });

    assert_eq!(stored_payroll.employer, employer);
    assert_eq!(stored_payroll.employee, employee);
    assert_eq!(stored_payroll.amount, amount);
    assert_eq!(stored_payroll.interval, interval);
    assert_eq!(stored_payroll.last_payment_time, env.ledger().timestamp());
}

#[test]
fn test_update_existing_escrow_valid_employer() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let initial_amount = 1000;
    let interval = 86400;

    env.mock_all_auths();

    client.create_or_update_escrow(&employer, &employee, &initial_amount, &interval);

    let updated_amount = 2000;
    client.create_or_update_escrow(&employer, &employee, &updated_amount, &interval);

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

    let amount = 1000;
    let interval = 86400;

    env.mock_all_auths();

    client.create_or_update_escrow(&employer, &employee, &amount, &interval);

    client.create_or_update_escrow(&invalid_employer, &employee, &2000, &interval);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_create_escrow_invalid_interval() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let amount = 1000;
    let invalid_interval = 0;

    env.mock_all_auths();
    client.create_or_update_escrow(&employer, &employee, &amount, &invalid_interval);
}
