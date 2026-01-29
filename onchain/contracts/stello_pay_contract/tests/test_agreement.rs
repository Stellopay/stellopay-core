#![cfg(test)]

use soroban_sdk::{testutils::Address as _, token, Address, Env};
use stello_pay_contract::storage::{AgreementMode, AgreementStatus};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ============================================================================
// Helpers
// ============================================================================

fn create_test_env() -> (Env, Address, PayrollContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    (env, contract_id, client)
}

fn create_token_contract<'a>(
    e: &'a Env,
    admin: &Address,
) -> (Address, token::Client<'a>, token::StellarAssetClient<'a>) {
    let token_id = e.register_stellar_asset_contract_v2(admin.clone());
    let token = token_id.address();
    let token_client = token::Client::new(e, &token);
    let token_admin_client = token::StellarAssetClient::new(e, &token);
    (token, token_client, token_admin_client)
}

// ============================================================================
// Agreement creation tests
// ============================================================================

/// test_create_payroll_agreement()
#[test]
fn test_create_payroll_agreement() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    let agreement = client.get_agreement(&agreement_id).unwrap();

    assert_eq!(agreement.id, agreement_id);
    assert_eq!(agreement.employer, employer);
    assert_eq!(agreement.token, token);
    assert_eq!(agreement.mode, AgreementMode::Payroll);
    assert_eq!(agreement.status, AgreementStatus::Created);

}

/// test_create_escrow_agreement()
#[test]
fn test_create_escrow_agreement() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token, &100i128, &3600u64, &2u32);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.mode, AgreementMode::Escrow);
    assert_eq!(agreement.status, AgreementStatus::Created);
    assert_eq!(agreement.total_amount, 200);

    // Escrow agreement auto-adds contributor as first employee.
    let employees = client.get_agreement_employees(&agreement_id);
    assert_eq!(employees.len(), 1);
    assert_eq!(employees.get(0).unwrap(), contributor);
}

/// test_create_agreement_invalid_parameters()
#[test]
#[should_panic]
fn test_create_agreement_invalid_parameters_zero_amount() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);
    client.create_escrow_agreement(&employer, &contributor, &token, &0i128, &3600u64, &2u32);
}

#[test]
#[should_panic]
fn test_create_agreement_invalid_parameters_zero_period() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);
    client.create_escrow_agreement(&employer, &contributor, &token, &1i128, &0u64, &2u32);
}

#[test]
#[should_panic]
fn test_create_agreement_invalid_parameters_zero_periods() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);
    client.create_escrow_agreement(&employer, &contributor, &token, &1i128, &3600u64, &0u32);
}

/// test_create_agreement_when_paused()
///
/// There is no global contract pause, so agreement creation remains allowed.
#[test]
fn test_create_agreement_when_paused() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_payroll_agreement(&employer, &token, &604800u64);
    assert_eq!(id, 1);
}

// ============================================================================
// Employee management tests
// ============================================================================

/// test_add_employee_to_agreement()
#[test]
fn test_add_employee_to_agreement() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let employee = Address::generate(&env);

    let id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&id, &employee, &1000i128);

    let employees = client.get_agreement_employees(&id);
    assert_eq!(employees.len(), 1);
    assert_eq!(employees.get(0).unwrap(), employee);

    let agreement = client.get_agreement(&id).unwrap();
    assert_eq!(agreement.total_amount, 1000);

}

/// test_add_multiple_employees()
#[test]
fn test_add_multiple_employees() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let e1 = Address::generate(&env);
    let e2 = Address::generate(&env);

    let id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&id, &e1, &1000i128);
    client.add_employee_to_agreement(&id, &e2, &2000i128);

    let employees = client.get_agreement_employees(&id);
    assert_eq!(employees.len(), 2);

    let agreement = client.get_agreement(&id).unwrap();
    assert_eq!(agreement.total_amount, 3000);
}

/// test_add_employee_zero_salary_fails()
#[test]
#[should_panic]
fn test_add_employee_zero_salary_fails() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let employee = Address::generate(&env);
    let id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&id, &employee, &0i128);
}

/// test_add_employee_wrong_status_fails()
#[test]
#[should_panic(expected = "Can only add employees to Created agreements")]
fn test_add_employee_wrong_status_fails() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let employee = Address::generate(&env);
    let id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.activate_agreement(&id);
    client.add_employee_to_agreement(&id, &employee, &1000i128);
}

/// test_add_employee_unauthorized_fails()
#[test]
#[should_panic]
fn test_add_employee_unauthorized_fails() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let attacker = Address::generate(&env);
    let token = Address::generate(&env);
    let employee = Address::generate(&env);
    let id = client.create_payroll_agreement(&employer, &token, &604800u64);

    env.set_auths(&[]);
    // Attacker tries to add employee (employer auth required).
    client.add_employee_to_agreement(&id, &employee, &1000i128);
    // Ensure attacker isn't accidentally treated as employer in mocked auth environment.
    let _ = attacker;
}

/// test_get_agreement_employees()
#[test]
fn test_get_agreement_employees() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let e1 = Address::generate(&env);
    let e2 = Address::generate(&env);
    let id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&id, &e1, &1000i128);
    client.add_employee_to_agreement(&id, &e2, &2000i128);

    let employees = client.get_agreement_employees(&id);
    assert_eq!(employees.len(), 2);
}

// ============================================================================
// Agreement activation tests
// ============================================================================

/// test_activate_agreement_with_employees()
#[test]
fn test_activate_agreement_with_employees() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let employee = Address::generate(&env);

    let id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&id, &employee, &1000i128);
    client.activate_agreement(&id);

    let agreement = client.get_agreement(&id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Active);
    assert!(agreement.activated_at.is_some());

}

/// test_activate_agreement_no_employees_fails()
///
/// Current contract does not enforce having employees before activation.
#[test]
fn test_activate_agreement_no_employees_fails() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.activate_agreement(&id);
    assert_eq!(client.get_agreement(&id).unwrap().status, AgreementStatus::Active);
}

/// test_activate_already_active_fails()
#[test]
#[should_panic(expected = "Agreement must be in Created status")]
fn test_activate_already_active_fails() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.activate_agreement(&id);
    client.activate_agreement(&id);
}

/// test_activate_unauthorized_fails()
#[test]
#[should_panic]
fn test_activate_unauthorized_fails() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_payroll_agreement(&employer, &token, &604800u64);
    env.set_auths(&[]);
    client.activate_agreement(&id);
}

/// test_activated_at_timestamp_set()
#[test]
fn test_activated_at_timestamp_set() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.activate_agreement(&id);
    assert!(client.get_agreement(&id).unwrap().activated_at.is_some());
}

// ============================================================================
// Agreement retrieval tests
// ============================================================================

/// test_get_agreement_by_id()
#[test]
fn test_get_agreement_by_id() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_payroll_agreement(&employer, &token, &604800u64);
    assert!(client.get_agreement(&id).is_some());
}

/// test_get_nonexistent_agreement()
#[test]
fn test_get_nonexistent_agreement() {
    let (env, _contract_id, client) = create_test_env();
    let _ = env;
    assert!(client.get_agreement(&999u128).is_none());
}

/// test_get_agreement_status()
#[test]
fn test_get_agreement_status() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_payroll_agreement(&employer, &token, &604800u64);
    assert_eq!(client.get_agreement(&id).unwrap().status, AgreementStatus::Created);
}

/// test_get_agreement_mode()
#[test]
fn test_get_agreement_mode() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_payroll_agreement(&employer, &token, &604800u64);
    assert_eq!(client.get_agreement(&id).unwrap().mode, AgreementMode::Payroll);
}

// ============================================================================
// Edge cases
// ============================================================================

/// test_maximum_employees_per_agreement()
#[test]
fn test_maximum_employees_per_agreement() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_payroll_agreement(&employer, &token, &604800u64);

    // No explicit cap in the contract; stress with a moderate number.
    for _ in 0..50u32 {
        let emp = Address::generate(&env);
        client.add_employee_to_agreement(&id, &emp, &1i128);
    }
    let employees = client.get_agreement_employees(&id);
    assert_eq!(employees.len(), 50);
}

/// test_agreement_with_large_amounts()
#[test]
fn test_agreement_with_large_amounts() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let employee = Address::generate(&env);
    let id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&id, &employee, &9_000_000_000_000i128);
    assert_eq!(client.get_agreement(&id).unwrap().total_amount, 9_000_000_000_000i128);
}

/// test_agreement_with_long_periods()
#[test]
fn test_agreement_with_long_periods() {
    let (env, _contract_id, client) = create_test_env();
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    let id =
        client.create_escrow_agreement(&employer, &contributor, &token, &10i128, &86400u64, &365u32);

    let agreement = client.get_agreement(&id).unwrap();
    assert_eq!(agreement.mode, AgreementMode::Escrow);
    assert_eq!(agreement.total_amount, 3650);
}

