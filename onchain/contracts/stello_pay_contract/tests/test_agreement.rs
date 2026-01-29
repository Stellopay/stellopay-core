#![cfg(test)]
#![allow(deprecated)]

//! Comprehensive test suite for agreement-based payroll system (#158).
//! Covers agreement creation, employee management, activation, retrieval, events, and edge cases.

use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env,
};
use stello_pay_contract::storage::{AgreementMode, AgreementStatus};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn create_test_env() -> (Env, Address, Address, PayrollContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    client.initialize(&owner);
    (env, owner, employer, client)
}

fn create_token(env: &Env) -> Address {
    let admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

// -----------------------------------------------------------------------------
// Agreement creation tests
// -----------------------------------------------------------------------------

/// Creates a payroll agreement and verifies it returns a valid ID and status.
#[test]
fn test_create_payroll_agreement() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);

    assert!(agreement_id >= 1);
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Created);
    assert_eq!(agreement.mode, AgreementMode::Payroll);
    assert_eq!(agreement.employer, employer);
}

/// Creates an escrow agreement and verifies it returns a valid ID and status.
#[test]
fn test_create_escrow_agreement() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let contributor = Address::generate(&env);

    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &1000,
        &86400,
        &12,
    );

    assert!(agreement_id >= 1);
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Created);
    assert_eq!(agreement.mode, AgreementMode::Escrow);
}

/// Verifies that creating an escrow agreement with invalid parameters returns an error.
#[test]
#[should_panic]
fn test_create_agreement_invalid_parameters_zero_amount() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let contributor = Address::generate(&env);
    let _ = client.create_escrow_agreement(&employer, &contributor, &token, &0, &86400, &12);
}

#[test]
#[should_panic]
fn test_create_agreement_invalid_parameters_zero_period() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let contributor = Address::generate(&env);
    let _ = client.create_escrow_agreement(&employer, &contributor, &token, &1000, &0, &12);
}

#[test]
#[should_panic]
fn test_create_agreement_invalid_parameters_zero_num_periods() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let contributor = Address::generate(&env);
    let _ = client.create_escrow_agreement(&employer, &contributor, &token, &1000, &86400, &0);
}

// -----------------------------------------------------------------------------
// Employee management tests
// -----------------------------------------------------------------------------

/// Adds one employee to a payroll agreement and verifies via get_agreement_employees.
#[test]
fn test_add_employee_to_agreement() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let employee = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);

    let employees = client.get_agreement_employees(&agreement_id);
    assert_eq!(employees.len(), 1);
    assert_eq!(employees.get(0).unwrap(), employee);
}

/// Adds multiple employees and verifies order and count.
#[test]
fn test_add_multiple_employees() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let e1 = Address::generate(&env);
    let e2 = Address::generate(&env);
    let e3 = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);
    client.add_employee_to_agreement(&agreement_id, &e1, &1000);
    client.add_employee_to_agreement(&agreement_id, &e2, &2000);
    client.add_employee_to_agreement(&agreement_id, &e3, &3000);

    let employees = client.get_agreement_employees(&agreement_id);
    assert_eq!(employees.len(), 3);
    assert_eq!(employees.get(0).unwrap(), e1);
    assert_eq!(employees.get(1).unwrap(), e2);
    assert_eq!(employees.get(2).unwrap(), e3);
}

#[test]
#[should_panic(expected = "Salary per period must be positive")]
fn test_add_employee_zero_salary_fails() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let employee = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);
    client.add_employee_to_agreement(&agreement_id, &employee, &0);
}

#[test]
#[should_panic(expected = "Can only add employees to Created agreements")]
fn test_add_employee_wrong_status_fails() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let employee = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    client.activate_agreement(&agreement_id);
    client.add_employee_to_agreement(&agreement_id, &Address::generate(&env), &500);
}

#[test]
#[should_panic(expected = "HostError")]
fn test_add_employee_unauthorized_fails() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let employee = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);

    env.mock_auths(&[]);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
}

#[test]
fn test_get_agreement_employees() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let employee = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);
    assert_eq!(client.get_agreement_employees(&agreement_id).len(), 0);

    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    let employees = client.get_agreement_employees(&agreement_id);
    assert_eq!(employees.len(), 1);
    assert_eq!(employees.get(0).unwrap(), employee);
}

// -----------------------------------------------------------------------------
// Agreement activation tests
// -----------------------------------------------------------------------------

#[test]
fn test_activate_agreement_with_employees() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let employee = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    client.activate_agreement(&agreement_id);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Active);
    assert!(agreement.activated_at.is_some());
}

#[test]
#[should_panic(expected = "Agreement must have at least one employee before activation")]
fn test_activate_agreement_no_employees_fails() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);
    client.activate_agreement(&agreement_id);
}

#[test]
#[should_panic(expected = "Agreement must be in Created status")]
fn test_activate_already_active_fails() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let employee = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    client.activate_agreement(&agreement_id);
    client.activate_agreement(&agreement_id);
}

#[test]
#[should_panic(expected = "HostError")]
fn test_activate_unauthorized_fails() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let employee = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);

    env.mock_auths(&[]);
    client.activate_agreement(&agreement_id);
}

#[test]
fn test_activated_at_timestamp_set() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let employee = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    let before = env.ledger().timestamp();
    client.activate_agreement(&agreement_id);
    let after = env.ledger().timestamp();

    let agreement = client.get_agreement(&agreement_id).unwrap();
    let activated_at = agreement.activated_at.unwrap();
    assert!(activated_at >= before && activated_at <= after);
}

// -----------------------------------------------------------------------------
// Agreement retrieval tests
// -----------------------------------------------------------------------------

#[test]
fn test_get_agreement_by_id() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.id, agreement_id);
    assert_eq!(agreement.employer, employer);
    assert_eq!(agreement.token, token);
}

#[test]
fn test_get_nonexistent_agreement() {
    let (env, _owner, _employer, client) = create_test_env();
    env.mock_all_auths();

    let agreement = client.get_agreement(&99999);
    assert!(agreement.is_none());
}

#[test]
fn test_get_agreement_status() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let employee = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);
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
}

#[test]
fn test_get_agreement_mode() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let contributor = Address::generate(&env);

    let payroll_id = client.create_payroll_agreement(&employer, &token, &86400);
    assert_eq!(
        client.get_agreement(&payroll_id).unwrap().mode,
        AgreementMode::Payroll
    );

    let escrow_id = client.create_escrow_agreement(&employer, &contributor, &token, &1000, &86400, &12);
    assert_eq!(
        client.get_agreement(&escrow_id).unwrap().mode,
        AgreementMode::Escrow
    );
}

// -----------------------------------------------------------------------------
// Event tests (events are emitted; we verify flow completes without panic)
// -----------------------------------------------------------------------------

#[test]
fn test_agreement_created_event() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);
    assert!(agreement_id >= 1);
    let events = env.events().all();
    assert!(!events.is_empty());
}

#[test]
fn test_agreement_activated_event() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let employee = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    client.activate_agreement(&agreement_id);
    assert_eq!(
        client.get_agreement(&agreement_id).unwrap().status,
        AgreementStatus::Active
    );
}

#[test]
fn test_employee_added_event() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let employee = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    let employees = client.get_agreement_employees(&agreement_id);
    assert_eq!(employees.len(), 1);
}

// -----------------------------------------------------------------------------
// Edge cases
// -----------------------------------------------------------------------------

#[test]
fn test_maximum_employees_per_agreement() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);
    let n = 20u32;
    for _ in 0..n {
        client.add_employee_to_agreement(&agreement_id, &Address::generate(&env), &100);
    }
    let employees = client.get_agreement_employees(&agreement_id);
    assert_eq!(employees.len(), n);
}

#[test]
fn test_agreement_with_large_amounts() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let employee = Address::generate(&env);
    let large: i128 = 1_000_000_000;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);
    client.add_employee_to_agreement(&agreement_id, &employee, &large);
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.total_amount, large);
}

#[test]
fn test_agreement_with_long_periods() {
    let (env, _owner, employer, client) = create_test_env();
    env.mock_all_auths();
    let token = create_token(&env);
    let one_year: u64 = 365 * 24 * 3600;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &one_year);
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.grace_period_seconds, one_year);
}
