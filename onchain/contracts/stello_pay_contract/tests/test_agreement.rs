//! Comprehensive test suite for agreement-based payroll system (#158).
//!
//! Covers: agreement creation (payroll and escrow), employee management,
//! activation, retrieval, events, and edge cases.

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::Address as _,
    Address, Env,
};
use stello_pay_contract::storage::{AgreementMode, AgreementStatus};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ============================================================================
// HELPERS
// ============================================================================

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn create_test_address(env: &Env) -> Address {
    Address::generate(env)
}

fn setup_contract(env: &Env) -> (Address, PayrollContractClient<'static>) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = create_test_address(env);
    client.initialize(&owner);
    (contract_id, client)
}

// ============================================================================
// Agreement creation tests
// ============================================================================

/// Creates a payroll agreement and verifies ID and initial state.
#[test]
fn test_create_payroll_agreement() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let grace_period = 604800u64;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace_period);

    assert!(agreement_id >= 1);
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.employer, employer);
    assert_eq!(agreement.token, token);
    assert_eq!(agreement.mode, AgreementMode::Payroll);
    assert_eq!(agreement.status, AgreementStatus::Created);
    assert_eq!(agreement.grace_period_seconds, grace_period);
}

/// Creates an escrow agreement and verifies ID and initial state.
#[test]
fn test_create_escrow_agreement() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);
    let amount_per_period = 1000i128;
    let period_seconds = 86400u64;
    let num_periods = 4u32;

    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &amount_per_period,
        &period_seconds,
        &num_periods,
    );
    assert!(agreement_id >= 1);
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.employer, employer);
    assert_eq!(agreement.mode, AgreementMode::Escrow);
    assert_eq!(agreement.status, AgreementStatus::Created);
    let employees = client.get_agreement_employees(&agreement_id);
    assert_eq!(employees.len(), 1);
}

/// Creating escrow agreement with zero amount per period fails.
#[test]
#[should_panic]
fn test_create_agreement_invalid_parameters_zero_amount() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    let _ = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &0i128,
        &86400u64,
        &4u32,
    );
}

/// Creating escrow agreement with zero num_periods fails.
#[test]
#[should_panic]
fn test_create_agreement_invalid_parameters_zero_periods() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    let _ = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &1000i128,
        &86400u64,
        &0u32,
    );
}

// ============================================================================
// Employee management tests
// ============================================================================

/// Adds one employee to a payroll agreement.
#[test]
fn test_add_employee_to_agreement() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let employee = create_test_address(&env);
    let salary = 2000i128;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary);

    let employees = client.get_agreement_employees(&agreement_id);
    assert_eq!(employees.len(), 1);
    assert_eq!(employees.get(0).unwrap(), employee);
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.total_amount, salary);
}

/// Adds multiple employees to a payroll agreement.
#[test]
fn test_add_multiple_employees() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    let e1 = create_test_address(&env);
    let e2 = create_test_address(&env);
    let e3 = create_test_address(&env);
    client.add_employee_to_agreement(&agreement_id, &e1, &1000);
    client.add_employee_to_agreement(&agreement_id, &e2, &2000);
    client.add_employee_to_agreement(&agreement_id, &e3, &3000);

    let employees = client.get_agreement_employees(&agreement_id);
    assert_eq!(employees.len(), 3);
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.total_amount, 6000);
}

/// Adding employee with zero salary must fail.
#[test]
#[should_panic(expected = "Salary must be positive")]
fn test_add_employee_zero_salary_fails() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let employee = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &employee, &0);
}

/// Adding employee when agreement is not in Created status must fail.
#[test]
#[should_panic(expected = "Can only add employees to Created agreements")]
fn test_add_employee_wrong_status_fails() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let employee = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    client.activate_agreement(&agreement_id);
    client.add_employee_to_agreement(&agreement_id, &create_test_address(&env), &500);
}

/// Only employer can add employees.
#[test]
#[should_panic]
fn test_add_employee_unauthorized_fails() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let employee = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    env.mock_auths(&[]);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
}

/// Retrieves agreement employees.
#[test]
fn test_get_agreement_employees() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let e1 = create_test_address(&env);
    let e2 = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    assert_eq!(client.get_agreement_employees(&agreement_id).len(), 0);
    client.add_employee_to_agreement(&agreement_id, &e1, &1000);
    client.add_employee_to_agreement(&agreement_id, &e2, &2000);
    let employees = client.get_agreement_employees(&agreement_id);
    assert_eq!(employees.len(), 2);
}

// ============================================================================
// Agreement activation tests
// ============================================================================

/// Activates agreement after adding employees.
#[test]
fn test_activate_agreement_with_employees() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let employee = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    client.activate_agreement(&agreement_id);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Active);
    assert!(agreement.activated_at.is_some());
}

/// Activating payroll agreement with no employees must fail.
#[test]
#[should_panic(expected = "Payroll agreement must have at least one employee to activate")]
fn test_activate_agreement_no_employees_fails() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.activate_agreement(&agreement_id);
}

/// Activating already active agreement must fail.
#[test]
#[should_panic(expected = "Agreement must be in Created status")]
fn test_activate_already_active_fails() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let employee = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    client.activate_agreement(&agreement_id);
    client.activate_agreement(&agreement_id);
}

/// Only employer can activate.
#[test]
#[should_panic]
fn test_activate_unauthorized_fails() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let employee = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    env.mock_auths(&[]);
    client.activate_agreement(&agreement_id);
}

/// Activated_at timestamp is set after activation.
#[test]
fn test_activated_at_timestamp_set() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let employee = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    let before = env.ledger().timestamp();
    client.activate_agreement(&agreement_id);
    let agreement = client.get_agreement(&agreement_id).unwrap();
    let activated_at = agreement.activated_at.unwrap();
    assert!(activated_at >= before);
}

// ============================================================================
// Agreement retrieval tests
// ============================================================================

/// Retrieves agreement by ID.
#[test]
fn test_get_agreement_by_id() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    let agreement = client.get_agreement(&agreement_id);
    assert!(agreement.is_some());
    let a = agreement.unwrap();
    assert_eq!(a.id, agreement_id);
    assert_eq!(a.employer, employer);
}

/// Nonexistent agreement returns None.
#[test]
fn test_get_nonexistent_agreement() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);

    let agreement = client.get_agreement(&99999u128);
    assert!(agreement.is_none());
}

/// Agreement status is correct through lifecycle.
#[test]
fn test_get_agreement_status() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let employee = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    assert_eq!(
        client.get_agreement(&agreement_id).unwrap().status,
        AgreementStatus::Created
    );
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    client.activate_agreement(&agreement_id);
    assert_eq!(
        client.get_agreement(&agreement_id).unwrap().status,
        AgreementStatus::Active
    );
    client.pause_agreement(&agreement_id);
    assert_eq!(
        client.get_agreement(&agreement_id).unwrap().status,
        AgreementStatus::Paused
    );
}

/// Agreement created event: agreement exists with correct initial state.
#[test]
fn test_agreement_created_event() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let grace_period = 604800u64;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace_period);
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Created);
    assert_eq!(agreement.mode, AgreementMode::Payroll);
}

/// Agreement activated event: status and activated_at set after activation.
#[test]
fn test_agreement_activated_event() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let employee = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    client.activate_agreement(&agreement_id);
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Active);
    assert!(agreement.activated_at.is_some());
}

/// Employee added event: employee appears in get_agreement_employees.
#[test]
fn test_employee_added_event() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let employee = create_test_address(&env);
    let salary = 1500i128;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary);
    let employees = client.get_agreement_employees(&agreement_id);
    assert_eq!(employees.len(), 1);
    assert_eq!(employees.get(0).unwrap(), employee);
}

/// Agreement mode is set correctly.
#[test]
fn test_get_agreement_mode() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let contributor = create_test_address(&env);

    let payroll_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    assert_eq!(
        client.get_agreement(&payroll_id).unwrap().mode,
        AgreementMode::Payroll
    );

    let escrow_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &1000i128,
        &86400u64,
        &4u32,
    );
    assert_eq!(
        client.get_agreement(&escrow_id).unwrap().mode,
        AgreementMode::Escrow
    );
}

// ============================================================================
// Edge cases
// ============================================================================

/// Agreement with many employees (stress test).
#[test]
fn test_maximum_employees_per_agreement() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    let n = 20u32;
    let mut total = 0i128;
    for i in 0..n {
        let employee = Address::generate(&env);
        let salary = (i as i128 + 1) * 100;
        client.add_employee_to_agreement(&agreement_id, &employee, &salary);
        total += salary;
    }
    let employees = client.get_agreement_employees(&agreement_id);
    assert_eq!(employees.len(), n);
    assert_eq!(client.get_agreement(&agreement_id).unwrap().total_amount, total);
}

/// Agreement with large amounts.
#[test]
fn test_agreement_with_large_amounts() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let employee = create_test_address(&env);
    let large_salary = i128::MAX / 2;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &employee, &large_salary);
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.total_amount, large_salary);
}

/// Agreement with long grace period.
#[test]
fn test_agreement_with_long_periods() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let long_grace = 365 * 24 * 3600u64;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &long_grace);
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.grace_period_seconds, long_grace);
}
