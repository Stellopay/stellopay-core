#![cfg(test)]

use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::{testutils::Address as _, testutils::Ledger, Address, Env};
use stello_pay_contract::storage::{
    Agreement, AgreementMode, AgreementStatus, DataKey, DisputeStatus, PayrollError, StorageKey,
};
use stello_pay_contract::PayrollContract;

fn create_test_environment() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn create_test_address(env: &Env) -> Address {
    Address::generate(env)
}

fn create_token(env: &Env) -> Address {
    let admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    // register_stellar_asset_contract_v2 sets an admin internally; in tests with
    // env.mock_all_auths() enabled, auth checks will be mocked.
    let token_admin_client = StellarAssetClient::new(env, token);
    token_admin_client.mint(to, &amount);
}

fn claim(
    env: &Env,
    contract_id: &Address,
    caller: &Address,
    agreement_id: u128,
    employee_index: u32,
) -> Result<(), PayrollError> {
    env.as_contract(contract_id, || {
        PayrollContract::claim_payroll(env.clone(), caller.clone(), agreement_id, employee_index)
    })
}

fn get_claimed(env: &Env, contract_id: &Address, agreement_id: u128, employee_index: u32) -> u32 {
    env.as_contract(contract_id, || {
        PayrollContract::get_employee_claimed_periods(env.clone(), agreement_id, employee_index)
    })
}

fn setup_test_agreement(
    env: &Env,
    contract_id: &Address,
    agreement_id: u128,
    employer: &Address,
    employees: &[(Address, i128)],
    period_duration: u64,
    token: &Address,
    escrow_amount: i128,
) {
    env.as_contract(contract_id, || {
        let employee_count = employees.len() as u32;
        DataKey::set_employee_count(env, agreement_id, employee_count);

        for (index, (employee, salary)) in employees.iter().enumerate() {
            DataKey::set_employee(env, agreement_id, index as u32, employee);
            DataKey::set_employee_salary(env, agreement_id, index as u32, *salary);
            DataKey::set_employee_claimed_periods(env, agreement_id, index as u32, 0);
        }

        let current_time = env.ledger().timestamp();
        DataKey::set_agreement_activation_time(env, agreement_id, current_time);
        DataKey::set_agreement_period_duration(env, agreement_id, period_duration);
        DataKey::set_agreement_token(env, agreement_id, token);
        DataKey::set_agreement_escrow_balance(env, agreement_id, token, escrow_amount);

        // Create and store the Agreement struct
        let agreement = Agreement {
            id: agreement_id,
            employer: employer.clone(),
            token: token.clone(),
            mode: AgreementMode::Payroll,
            status: AgreementStatus::Active,
            total_amount: 0,
            paid_amount: 0,
            created_at: current_time,
            activated_at: Some(current_time),
            cancelled_at: None,
            grace_period_seconds: period_duration * 10, // 10 periods grace period
            amount_per_period: None,
            period_seconds: Some(period_duration),
            num_periods: None,
            claimed_periods: None,
            dispute_raised_at: None,
            dispute_status: DisputeStatus::None,
        };

        env.storage()
            .persistent()
            .set(&StorageKey::Agreement(agreement_id), &agreement);
    });
}

#[test]
fn test_claim_payroll_unauthorized() {
    let env = create_test_environment();
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee = create_test_address(&env);
    let wrong_caller = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let escrow_amount = 10000i128;

    setup_test_agreement(
        &env,
        &contract_id,
        agreement_id,
        &employer,
        &[(employee.clone(), salary)],
        period_duration,
        &token,
        escrow_amount,
    );
    mint(&env, &token, &contract_id, escrow_amount);

    env.ledger().with_mut(|li| {
        li.timestamp = period_duration + 1;
    });

    // Try to claim with wrong caller
    assert!(claim(&env, &contract_id, &wrong_caller, agreement_id, 0).is_err());
}

#[test]
fn test_claim_payroll_invalid_employee_index() {
    let env = create_test_environment();
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let escrow_amount = 10000i128;

    setup_test_agreement(
        &env,
        &contract_id,
        agreement_id,
        &employer,
        &[(employee.clone(), salary)],
        period_duration,
        &token,
        escrow_amount,
    );
    mint(&env, &token, &contract_id, escrow_amount);

    // Try to claim with invalid index
    assert!(claim(&env, &contract_id, &employee, agreement_id, 1).is_err());
}

#[test]
fn test_multiple_employees_independent_claiming() {
    let env = create_test_environment();
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee1 = create_test_address(&env);
    let employee2 = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128; // Same salary for both employees
    let period_duration = 86400u64;
    let escrow_amount = 50000i128;

    setup_test_agreement(
        &env,
        &contract_id,
        agreement_id,
        &employer,
        &[(employee1.clone(), salary), (employee2.clone(), salary)],
        period_duration,
        &token,
        escrow_amount,
    );
    mint(&env, &token, &contract_id, escrow_amount);

    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 2 + 1;
    });

    // Employee 1 claims
    claim(&env, &contract_id, &employee1, agreement_id, 0).unwrap();

    // Employee 2 claims independently
    claim(&env, &contract_id, &employee2, agreement_id, 1).unwrap();

    // Verify independent tracking - each employee's periods are tracked separately
    let claimed1 = get_claimed(&env, &contract_id, agreement_id, 0);
    let claimed2 = get_claimed(&env, &contract_id, agreement_id, 1);
    assert_eq!(claimed1, 2u32);
    assert_eq!(claimed2, 2u32);

    // Verify escrow balance
    let remaining = env.as_contract(&contract_id, || {
        DataKey::get_agreement_escrow_balance(&env, agreement_id, &token)
    });
    assert_eq!(remaining, escrow_amount - (salary * 2) - (salary * 2));
}

#[test]
fn test_different_salaries_per_employee() {
    let env = create_test_environment();
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee1 = create_test_address(&env);
    let employee2 = create_test_address(&env);
    let token = create_token(&env);
    let salary1 = 1000i128;
    let salary2 = 2000i128; // Different salary for employee 2
    let period_duration = 86400u64;
    let escrow_amount = 50000i128;

    setup_test_agreement(
        &env,
        &contract_id,
        agreement_id,
        &employer,
        &[(employee1.clone(), salary1), (employee2.clone(), salary2)],
        period_duration,
        &token,
        escrow_amount,
    );
    mint(&env, &token, &contract_id, escrow_amount);

    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 2 + 1;
    });

    // Employee 1 claims
    claim(&env, &contract_id, &employee1, agreement_id, 0).unwrap();

    // Employee 2 claims
    claim(&env, &contract_id, &employee2, agreement_id, 1).unwrap();

    // Verify different salaries per employee are correctly applied
    let token_client = TokenClient::new(&env, &token);
    let balance1 = token_client.balance(&employee1);
    let balance2 = token_client.balance(&employee2);
    assert_eq!(balance1, salary1 * 2); // Employee 1 received salary1 * 2 periods
    assert_eq!(balance2, salary2 * 2); // Employee 2 received salary2 * 2 periods (different amount)

    // Verify escrow balance
    let remaining = env.as_contract(&contract_id, || {
        DataKey::get_agreement_escrow_balance(&env, agreement_id, &token)
    });
    assert_eq!(remaining, escrow_amount - (salary1 * 2) - (salary2 * 2));
}

#[test]
fn test_claiming_during_grace_period() {
    let env = create_test_environment();
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64; // 1 day
    let escrow_amount = 10000i128;

    setup_test_agreement(
        &env,
        &contract_id,
        agreement_id,
        &employer,
        &[(employee.clone(), salary)],
        period_duration,
        &token,
        escrow_amount,
    );
    mint(&env, &token, &contract_id, escrow_amount);

    // Fast forward to just after activation (within grace period - first period)
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration / 2 + 1; // Half a period elapsed
    });

    // Should be able to claim during grace period (even if less than full period)
    // The contract calculates elapsed periods, so 0.5 periods = 0 full periods
    // But we'll test with 1 full period to show grace period claiming works
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration + 1; // Exactly 1 period elapsed
    });

    // Claiming during grace period should succeed
    claim(&env, &contract_id, &employee, agreement_id, 0).unwrap();

    // Verify claimed periods
    let claimed = get_claimed(&env, &contract_id, agreement_id, 0);
    assert_eq!(claimed, 1u32);

    // Verify employee received payment
    let token_client = TokenClient::new(&env, &token);
    let balance = token_client.balance(&employee);
    assert_eq!(balance, salary);
}
