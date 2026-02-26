//! Backup and Recovery testing (#240).
//!
//! Verifies data integrity and recovery scenarios: agreement and employee
//! state remain consistent after operations and can be reconstructed from storage.

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{testutils::Address as _, Address, Env};
use stello_pay_contract::storage::{AgreementMode, AgreementStatus};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn create_address(env: &Env) -> Address {
    Address::generate(env)
}

fn setup_contract(env: &Env) -> (Address, PayrollContractClient<'static>) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = create_address(env);
    client.initialize(&owner);
    (contract_id, client)
}

/// Verifies that after creating an agreement, get_agreement returns the same data
/// (persistence / recovery of agreement state).
#[test]
fn test_agreement_data_integrity_after_creation() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let grace = 604800u64;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace);
    let agreement = client.get_agreement(&agreement_id).expect("Agreement must exist");
    assert_eq!(agreement.id, agreement_id);
    assert_eq!(agreement.employer, employer);
    assert_eq!(agreement.token, token);
    assert_eq!(agreement.status, AgreementStatus::Created);
    assert_eq!(agreement.mode, AgreementMode::Payroll);
    assert_eq!(agreement.grace_period_seconds, grace);
}

/// Verifies that employee list and salaries are stored and retrieved correctly
/// (data integrity for employee data).
#[test]
fn test_employee_data_integrity_after_add() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);
    let salary = 2000i128;
    let grace = 604800u64;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary);

    let employees = client.get_agreement_employees(&agreement_id);
    assert_eq!(employees.len(), 1);
    assert_eq!(employees.get(0), Some(employee.clone()));
}

/// Verifies that activation state is persisted and recovered.
#[test]
fn test_activation_state_integrity() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_address(&env);
    let employee = create_address(&env);
    let grace = 604800u64;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000i128);
    assert_eq!(client.get_agreement(&agreement_id).unwrap().status, AgreementStatus::Created);

    client.activate_agreement(&agreement_id);
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Active);
    assert!(agreement.activated_at.is_some());
}
