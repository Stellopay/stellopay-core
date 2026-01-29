

#![cfg(test)]

use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::{testutils::Address as _, testutils::Ledger, Address, Env, Vec};
use stello_pay_contract::storage::{
    Agreement, AgreementMode, AgreementStatus, DataKey, DisputeStatus, EmployeeInfo, PayrollError,
    StorageKey,
};
use stello_pay_contract::PayrollContract;

// ============================================================================
// Test Helpers
// ============================================================================

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
    let token_admin_client = StellarAssetClient::new(env, token);
    token_admin_client.mint(to, &amount);
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
            grace_period_seconds: period_duration * 10,
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

fn claim_payroll(
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

fn get_claimed_periods(env: &Env, contract_id: &Address, agreement_id: u128, employee_index: u32) -> u32 {
    env.as_contract(contract_id, || {
        PayrollContract::get_employee_claimed_periods(env.clone(), agreement_id, employee_index)
    })
}

// ============================================================================
// Employee Claiming Tests
// ============================================================================

/// Test that an employee can successfully claim their payroll
#[test]
fn test_employee_can_claim_payroll() {
    let env = create_test_environment();
    #[allow(deprecated)]
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

    // Fast forward 1 period
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration + 1;
    });

    // Employee claims payroll
    let result = claim_payroll(&env, &contract_id, &employee, agreement_id, 0);
    assert!(result.is_ok());

    // Verify payment received
    let token_client = TokenClient::new(&env, &token);
    let balance = token_client.balance(&employee);
    assert_eq!(balance, salary);
}

/// Test that an employee cannot claim another employee's payroll
#[test]
fn test_employee_cannot_claim_others() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee1 = create_test_address(&env);
    let employee2 = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let escrow_amount = 20000i128;

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
        li.timestamp = period_duration + 1;
    });

    // Employee1 tries to claim Employee2's payroll
    let result = claim_payroll(&env, &contract_id, &employee1, agreement_id, 1);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), PayrollError::Unauthorized);
}

/// Test that claiming before activation fails
#[test]
fn test_claim_before_activation() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let escrow_amount = 10000i128;

    // Setup agreement but don't activate it
    env.as_contract(&contract_id, || {
        DataKey::set_employee_count(&env, agreement_id, 1);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);

        let current_time = env.ledger().timestamp();
        let agreement = Agreement {
            id: agreement_id,
            employer: employer.clone(),
            token: token.clone(),
            mode: AgreementMode::Payroll,
            status: AgreementStatus::Created, // Not activated
            total_amount: 0,
            paid_amount: 0,
            created_at: current_time,
            activated_at: None, // Not activated
            cancelled_at: None,
            grace_period_seconds: period_duration * 10,
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

    mint(&env, &token, &contract_id, escrow_amount);

    // Try to claim
    let result = claim_payroll(&env, &contract_id, &employee, agreement_id, 0);
    assert!(result.is_err());
}

/// Test claiming after exactly one period
#[test]
fn test_claim_after_one_period() {
    let env = create_test_environment();
    #[allow(deprecated)]
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

    // Fast forward exactly 1 period
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration;
    });

    let result = claim_payroll(&env, &contract_id, &employee, agreement_id, 0);
    assert!(result.is_ok());

    let claimed = get_claimed_periods(&env, &contract_id, agreement_id, 0);
    assert_eq!(claimed, 1);

    let token_client = TokenClient::new(&env, &token);
    let balance = token_client.balance(&employee);
    assert_eq!(balance, salary);
}

/// Test claiming after multiple periods
#[test]
fn test_claim_after_multiple_periods() {
    let env = create_test_environment();
    #[allow(deprecated)]
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

    // Fast forward 5 periods
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 5 + 1;
    });

    let result = claim_payroll(&env, &contract_id, &employee, agreement_id, 0);
    assert!(result.is_ok());

    let claimed = get_claimed_periods(&env, &contract_id, agreement_id, 0);
    assert_eq!(claimed, 5);

    let token_client = TokenClient::new(&env, &token);
    let balance = token_client.balance(&employee);
    assert_eq!(balance, salary * 5);
}

/// Test that unauthorized caller cannot claim
#[test]
fn test_claim_unauthorized_fails() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee = create_test_address(&env);
    let unauthorized = create_test_address(&env);
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

    // Unauthorized user tries to claim
    let result = claim_payroll(&env, &contract_id, &unauthorized, agreement_id, 0);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), PayrollError::Unauthorized);
}

/// Test that invalid employee index fails
#[test]
fn test_claim_invalid_index_fails() {
    let env = create_test_environment();
    #[allow(deprecated)]
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

    env.ledger().with_mut(|li| {
        li.timestamp = period_duration + 1;
    });

    // Try to claim with invalid index (only 1 employee at index 0)
    let result = claim_payroll(&env, &contract_id, &employee, agreement_id, 5);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), PayrollError::InvalidEmployeeIndex);
}

/// Test that claiming with wrong agreement status fails
#[test]
fn test_claim_wrong_status_fails() {
    let env = create_test_environment();
    #[allow(deprecated)]
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

    // Set agreement to Paused status
    env.as_contract(&contract_id, || {
        let mut agreement: Agreement = env
            .storage()
            .persistent()
            .get(&StorageKey::Agreement(agreement_id))
            .unwrap();
        agreement.status = AgreementStatus::Paused;
        env.storage()
            .persistent()
            .set(&StorageKey::Agreement(agreement_id), &agreement);
    });

    env.ledger().with_mut(|li| {
        li.timestamp = period_duration + 1;
    });

    let result = claim_payroll(&env, &contract_id, &employee, agreement_id, 0);
    assert!(result.is_err());
}

// ============================================================================
// Period Calculation Tests
// ============================================================================

/// Test period calculation for individual employee
#[test]
fn test_periods_calculation_individual_employee() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let escrow_amount = 50000i128;

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

    // Test at different time intervals
    let test_cases = vec![
        (period_duration / 2, 0), // Half period = 0 periods
        (period_duration, 1),     // 1 period
        (period_duration * 3 + period_duration / 2, 3), // 3.5 periods = 3 periods
        (period_duration * 7, 7), // 7 periods
    ];

    for (time, expected_periods) in test_cases {
        env.ledger().with_mut(|li| {
            li.timestamp = time;
        });

        // Reset claimed periods
        env.as_contract(&contract_id, || {
            DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        });

        if expected_periods > 0 {
            claim_payroll(&env, &contract_id, &employee, agreement_id, 0).unwrap();
            let claimed = get_claimed_periods(&env, &contract_id, agreement_id, 0);
            assert_eq!(claimed, expected_periods);
        }
    }
}

/// Test that employee claimed periods updates correctly
#[test]
fn test_employee_claimed_periods_updates() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let escrow_amount = 50000i128;

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

    // Initial claimed periods should be 0
    let initial_claimed = get_claimed_periods(&env, &contract_id, agreement_id, 0);
    assert_eq!(initial_claimed, 0);

    // Claim after 2 periods
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 2;
    });
    claim_payroll(&env, &contract_id, &employee, agreement_id, 0).unwrap();
    
    let claimed_after_first = get_claimed_periods(&env, &contract_id, agreement_id, 0);
    assert_eq!(claimed_after_first, 2);

    // Claim after 3 more periods (total 5)
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 5;
    });
    claim_payroll(&env, &contract_id, &employee, agreement_id, 0).unwrap();
    
    let claimed_after_second = get_claimed_periods(&env, &contract_id, agreement_id, 0);
    assert_eq!(claimed_after_second, 5);
}

/// Test that employee cannot claim same period twice
#[test]
fn test_cannot_claim_same_period_twice() {
    let env = create_test_environment();
    #[allow(deprecated)]
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

    // Advance 2 periods
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 2;
    });

    // First claim succeeds
    claim_payroll(&env, &contract_id, &employee, agreement_id, 0).unwrap();

    // Second claim at same time should fail (no new periods)
    let result = claim_payroll(&env, &contract_id, &employee, agreement_id, 0);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), PayrollError::NoPeriodsToClaim);
}

/// Test multiple employees claim independently
#[test]
fn test_multiple_employees_claim_independently() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee1 = create_test_address(&env);
    let employee2 = create_test_address(&env);
    let employee3 = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let escrow_amount = 100000i128;

    setup_test_agreement(
        &env,
        &contract_id,
        agreement_id,
        &employer,
        &[
            (employee1.clone(), salary),
            (employee2.clone(), salary),
            (employee3.clone(), salary),
        ],
        period_duration,
        &token,
        escrow_amount,
    );
    mint(&env, &token, &contract_id, escrow_amount);

    // Advance 3 periods
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 3;
    });

    // Employee 1 claims
    claim_payroll(&env, &contract_id, &employee1, agreement_id, 0).unwrap();
    let claimed1 = get_claimed_periods(&env, &contract_id, agreement_id, 0);
    assert_eq!(claimed1, 3);

    // Advance to 5 periods
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 5;
    });

    // Employee 2 claims (should get all 5 periods)
    claim_payroll(&env, &contract_id, &employee2, agreement_id, 1).unwrap();
    let claimed2 = get_claimed_periods(&env, &contract_id, agreement_id, 1);
    assert_eq!(claimed2, 5);

    // Employee 1 claims again (should get periods 4-5)
    claim_payroll(&env, &contract_id, &employee1, agreement_id, 0).unwrap();
    let claimed1_updated = get_claimed_periods(&env, &contract_id, agreement_id, 0);
    assert_eq!(claimed1_updated, 5);

    // Employee 3 hasn't claimed yet
    let claimed3 = get_claimed_periods(&env, &contract_id, agreement_id, 2);
    assert_eq!(claimed3, 0);
}

/// Test different employees have different claimed periods
#[test]
fn test_different_employees_different_claimed_periods() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee1 = create_test_address(&env);
    let employee2 = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
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

    // Employee 1 claims after 2 periods
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 2;
    });
    claim_payroll(&env, &contract_id, &employee1, agreement_id, 0).unwrap();

    // Employee 2 claims after 4 periods
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 4;
    });
    claim_payroll(&env, &contract_id, &employee2, agreement_id, 1).unwrap();

    // Verify different claimed periods
    let claimed1 = get_claimed_periods(&env, &contract_id, agreement_id, 0);
    let claimed2 = get_claimed_periods(&env, &contract_id, agreement_id, 1);
    
    assert_eq!(claimed1, 2);
    assert_eq!(claimed2, 4);
}

// ============================================================================
// Payment Amount Tests
// ============================================================================

/// Test payment amount matches employee salary
#[test]
fn test_payment_amount_matches_employee_salary() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 5000i128;
    let period_duration = 86400u64;
    let escrow_amount = 50000i128;

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
        li.timestamp = period_duration;
    });

    claim_payroll(&env, &contract_id, &employee, agreement_id, 0).unwrap();

    let token_client = TokenClient::new(&env, &token);
    let balance = token_client.balance(&employee);
    assert_eq!(balance, salary);
}

/// Test payment for single period
#[test]
fn test_payment_single_period() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 2000i128;
    let period_duration = 86400u64;
    let escrow_amount = 20000i128;

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
        li.timestamp = period_duration;
    });

    claim_payroll(&env, &contract_id, &employee, agreement_id, 0).unwrap();

    let token_client = TokenClient::new(&env, &token);
    let balance = token_client.balance(&employee);
    assert_eq!(balance, salary * 1);
}

/// Test payment for multiple periods
#[test]
fn test_payment_multiple_periods() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1500i128;
    let period_duration = 86400u64;
    let escrow_amount = 50000i128;

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

    // Advance 10 periods
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 10;
    });

    claim_payroll(&env, &contract_id, &employee, agreement_id, 0).unwrap();

    let token_client = TokenClient::new(&env, &token);
    let balance = token_client.balance(&employee);
    assert_eq!(balance, salary * 10);
}

#[test]
fn test_different_employees_different_amounts() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee1 = create_test_address(&env);
    let employee2 = create_test_address(&env);
    let employee3 = create_test_address(&env);
    let token = create_token(&env);
    let salary1 = 1000i128;
    let salary2 = 2000i128;
    let salary3 = 3000i128;
    let period_duration = 86400u64;
    let escrow_amount = 100000i128;

    setup_test_agreement(
        &env,
        &contract_id,
        agreement_id,
        &employer,
        &[
            (employee1.clone(), salary1),
            (employee2.clone(), salary2),
            (employee3.clone(), salary3),
        ],
        period_duration,
        &token,
        escrow_amount,
    );
    mint(&env, &token, &contract_id, escrow_amount);

    // Advance 3 periods
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 3;
    });

    // All employees claim
    claim_payroll(&env, &contract_id, &employee1, agreement_id, 0).unwrap();
    claim_payroll(&env, &contract_id, &employee2, agreement_id, 1).unwrap();
    claim_payroll(&env, &contract_id, &employee3, agreement_id, 2).unwrap();

    // Verify different payment amounts
    let token_client = TokenClient::new(&env, &token);
    let balance1 = token_client.balance(&employee1);
    let balance2 = token_client.balance(&employee2);
    let balance3 = token_client.balance(&employee3);

    assert_eq!(balance1, salary1 * 3);
    assert_eq!(balance2, salary2 * 3);
    assert_eq!(balance3, salary3 * 3);
}

// ============================================================================
// Grace Period Tests
// ============================================================================

/// Test that employee can claim during grace period
#[test]
fn test_can_claim_during_grace_period() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let escrow_amount = 50000i128;
    let grace_period = period_duration * 10;

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

    // Advance 2 periods
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 2;
    });

    // Cancel the agreement
    env.as_contract(&contract_id, || {
        PayrollContract::cancel_agreement(env.clone(), agreement_id);
    });

    // Advance 1 more period (still within grace period)
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 3;
    });

    // Should be able to claim during grace period
    let result = claim_payroll(&env, &contract_id, &employee, agreement_id, 0);
    assert!(result.is_ok());

    let claimed = get_claimed_periods(&env, &contract_id, agreement_id, 0);
    assert_eq!(claimed, 3);
}

/// Test that employee cannot claim after grace period expires
#[test]
fn test_cannot_claim_after_grace_period_expires() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let escrow_amount = 50000i128;
    let grace_period = period_duration * 10;

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

    // Advance 2 periods
    let cancel_time = period_duration * 2;
    env.ledger().with_mut(|li| {
        li.timestamp = cancel_time;
    });

    // Cancel the agreement
    env.as_contract(&contract_id, || {
        PayrollContract::cancel_agreement(env.clone(), agreement_id);
    });

    // Advance past grace period
    env.ledger().with_mut(|li| {
        li.timestamp = cancel_time + grace_period + 1;
    });

    // Should NOT be able to claim after grace period
    let result = claim_payroll(&env, &contract_id, &employee, agreement_id, 0);
    assert!(result.is_err());
}

// ============================================================================
// Edge Cases Tests
// ============================================================================

/// Test employee with very high salary
#[test]
fn test_employee_very_high_salary() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1_000_000_000i128; // 1 billion
    let period_duration = 86400u64;
    let escrow_amount = 10_000_000_000i128; // 10 billion

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
        li.timestamp = period_duration * 5;
    });

    claim_payroll(&env, &contract_id, &employee, agreement_id, 0).unwrap();

    let token_client = TokenClient::new(&env, &token);
    let balance = token_client.balance(&employee);
    assert_eq!(balance, salary * 5);
}

/// Test employee with very low salary
#[test]
fn test_employee_very_low_salary() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1i128; // Minimum positive salary
    let period_duration = 86400u64;
    let escrow_amount = 1000i128;

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
        li.timestamp = period_duration * 100;
    });

    claim_payroll(&env, &contract_id, &employee, agreement_id, 0).unwrap();

    let token_client = TokenClient::new(&env, &token);
    let balance = token_client.balance(&employee);
    assert_eq!(balance, salary * 100);
}

/// Test many employees claiming simultaneously
#[test]
fn test_many_employees_claiming_simultaneously() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let num_employees = 10u32;
    let escrow_amount = salary * (num_employees as i128) * 100;

    // Create multiple employees
    let mut employees = Vec::new(&env);
    for _ in 0..num_employees {
        employees.push_back(create_test_address(&env));
    }

    // Setup employees
    let mut employee_tuples = vec![];
    for i in 0..num_employees {
        employee_tuples.push((employees.get(i).unwrap(), salary));
    }

    setup_test_agreement(
        &env,
        &contract_id,
        agreement_id,
        &employer,
        &employee_tuples,
        period_duration,
        &token,
        escrow_amount,
    );
    mint(&env, &token, &contract_id, escrow_amount);

    // Advance 5 periods
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 5;
    });

    // All employees claim
    for i in 0..num_employees {
        let employee = employees.get(i).unwrap();
        claim_payroll(&env, &contract_id, &employee, agreement_id, i).unwrap();
    }

    // Verify all received correct amounts
    let token_client = TokenClient::new(&env, &token);
    for i in 0..num_employees {
        let employee = employees.get(i).unwrap();
        let balance = token_client.balance(&employee);
        assert_eq!(balance, salary * 5);
    }
}

/// Test claiming at exact period boundaries
#[test]
fn test_claiming_at_exact_period_boundaries() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let escrow_amount = 50000i128;

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

    // Test exact boundaries
    let boundaries = vec![
        (period_duration, 1),
        (period_duration * 2, 1), // 1 more period
        (period_duration * 5, 3), // 3 more periods
    ];

    for (time, expected_new_periods) in boundaries {
        env.ledger().with_mut(|li| {
            li.timestamp = time;
        });

        let before = get_claimed_periods(&env, &contract_id, agreement_id, 0);
        claim_payroll(&env, &contract_id, &employee, agreement_id, 0).unwrap();
        let after = get_claimed_periods(&env, &contract_id, agreement_id, 0);
        
        assert_eq!(after - before, expected_new_periods);
    }
}

/// Test insufficient escrow balance
#[test]
fn test_insufficient_escrow_balance() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let insufficient_escrow = 500i128; // Less than one period's salary

    setup_test_agreement(
        &env,
        &contract_id,
        agreement_id,
        &employer,
        &[(employee.clone(), salary)],
        period_duration,
        &token,
        insufficient_escrow,
    );
    mint(&env, &token, &contract_id, insufficient_escrow);

    env.ledger().with_mut(|li| {
        li.timestamp = period_duration;
    });

    let result = claim_payroll(&env, &contract_id, &employee, agreement_id, 0);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), PayrollError::InsufficientEscrowBalance);
}

/// Test claiming with zero periods elapsed
#[test]
fn test_claiming_with_zero_periods() {
    let env = create_test_environment();
    #[allow(deprecated)]
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

    // Don't advance time (0 periods elapsed)
    let result = claim_payroll(&env, &contract_id, &employee, agreement_id, 0);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), PayrollError::NoPeriodsToClaim);
}

/// Test partial period doesn't allow claim
#[test]
fn test_partial_period_no_claim() {
    let env = create_test_environment();
    #[allow(deprecated)]
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

    // Advance only half a period
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration / 2;
    });

    let result = claim_payroll(&env, &contract_id, &employee, agreement_id, 0);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), PayrollError::NoPeriodsToClaim);
}

/// Test escrow balance decreases correctly
#[test]
fn test_escrow_balance_decreases() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let escrow_amount = 50000i128;

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

    // Check initial escrow balance
    let initial_balance = env.as_contract(&contract_id, || {
        DataKey::get_agreement_escrow_balance(&env, agreement_id, &token)
    });
    assert_eq!(initial_balance, escrow_amount);

    // Claim after 3 periods
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 3;
    });
    claim_payroll(&env, &contract_id, &employee, agreement_id, 0).unwrap();

    // Check escrow balance decreased
    let after_claim_balance = env.as_contract(&contract_id, || {
        DataKey::get_agreement_escrow_balance(&env, agreement_id, &token)
    });
    assert_eq!(after_claim_balance, escrow_amount - (salary * 3));
}

/// Test multiple claims progressively decrease escrow
#[test]
fn test_multiple_claims_decrease_escrow() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);

    let agreement_id = 1u128;
    let employer = create_test_address(&env);
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let escrow_amount = 50000i128;

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

    // First claim
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 2;
    });
    claim_payroll(&env, &contract_id, &employee, agreement_id, 0).unwrap();

    let balance_after_first = env.as_contract(&contract_id, || {
        DataKey::get_agreement_escrow_balance(&env, agreement_id, &token)
    });
    assert_eq!(balance_after_first, escrow_amount - (salary * 2));

    // Second claim
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 5;
    });
    claim_payroll(&env, &contract_id, &employee, agreement_id, 0).unwrap();

    let balance_after_second = env.as_contract(&contract_id, || {
        DataKey::get_agreement_escrow_balance(&env, agreement_id, &token)
    });
    assert_eq!(balance_after_second, escrow_amount - (salary * 5));
}