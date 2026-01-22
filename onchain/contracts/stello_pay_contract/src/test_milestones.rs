#![cfg(test)]
#![allow(deprecated)]
use crate::{PayrollContract, PayrollContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn create_test_env() -> (
    Env,
    Address,
    Address,
    Address,
    PayrollContractClient<'static>,
) {
    let env = Env::default();
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    (env, employer, contributor, token, client)
}

#[test]
#[should_panic(expected = "Caller is not an employee")]
fn test_claiming_for_wrong_employee() {
    let (env, employer, _contributor, token, client) = create_test_env();
    let employee1 = Address::generate(&env);
    let wrong_employee = Address::generate(&env);

    env.mock_all_auths();

    // Create payroll agreement
    let agreement_id = client.create_payroll_agreement(&employer, &token, &3600);

    // Add only employee1
    client.add_employee_to_agreement(&agreement_id, &employee1, &1000);

    // Activate agreement
    client.activate_agreement(&agreement_id);

    // Try to claim with wrong employee - should fail
    client.claim_payroll(&agreement_id, &wrong_employee);
}

#[test]
fn test_multiple_employees_claiming_independently() {
    let (env, employer, _contributor, token, client) = create_test_env();
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let employee3 = Address::generate(&env);

    env.mock_all_auths();

    // Create payroll agreement
    let agreement_id = client.create_payroll_agreement(&employer, &token, &3600);

    // Add multiple employees
    client.add_employee_to_agreement(&agreement_id, &employee1, &1000);
    client.add_employee_to_agreement(&agreement_id, &employee2, &2000);
    client.add_employee_to_agreement(&agreement_id, &employee3, &1500);

    // Activate agreement
    client.activate_agreement(&agreement_id);

    // Each employee claims independently
    client.claim_payroll(&agreement_id, &employee1);
    client.claim_payroll(&agreement_id, &employee2);
    client.claim_payroll(&agreement_id, &employee3);

    // Verify agreement is updated with paid amounts
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.paid_amount, 4500); // 1000 + 2000 + 1500
}

#[test]
fn test_different_salaries_per_employee() {
    let (env, employer, _contributor, token, client) = create_test_env();
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let employee3 = Address::generate(&env);

    env.mock_all_auths();

    // Create payroll agreement
    let agreement_id = client.create_payroll_agreement(&employer, &token, &3600);

    // Add employees with different salaries
    client.add_employee_to_agreement(&agreement_id, &employee1, &5000); // High salary
    client.add_employee_to_agreement(&agreement_id, &employee2, &3000); // Medium salary
    client.add_employee_to_agreement(&agreement_id, &employee3, &1000); // Low salary

    // Activate agreement
    client.activate_agreement(&agreement_id);

    // Get initial state
    let agreement_before = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement_before.paid_amount, 0);
    assert_eq!(agreement_before.total_amount, 9000); // 5000 + 3000 + 1000

    // Employee 1 claims
    client.claim_payroll(&agreement_id, &employee1);
    let agreement_after1 = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement_after1.paid_amount, 5000);

    // Employee 3 claims
    client.claim_payroll(&agreement_id, &employee3);
    let agreement_after2 = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement_after2.paid_amount, 6000); // 5000 + 1000

    // Employee 2 claims
    client.claim_payroll(&agreement_id, &employee2);
    let agreement_final = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement_final.paid_amount, 9000); // All paid
}

#[test]
fn test_claiming_during_grace_period() {
    let (env, employer, _contributor, token, client) = create_test_env();
    let employee = Address::generate(&env);

    env.mock_all_auths();

    let agreement_id = client.create_escrow_agreement(
        &employer, &employee, &token, &1000, &3600, // 1 hour period
        &3,    // 3 periods
    );

    client.activate_agreement(&agreement_id);

    client.claim_payroll(&agreement_id, &employee);
}
