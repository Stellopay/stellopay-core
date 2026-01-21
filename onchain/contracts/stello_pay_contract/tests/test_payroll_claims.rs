#![cfg(test)]

use soroban_sdk::{testutils::Address as _, testutils::Ledger, Address, Env};
use soroban_sdk::token::StellarAssetClient;
use stello_pay_contract::PayrollContract;
use stello_pay_contract::storage::DataKey;

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
) -> Result<(), soroban_sdk::Error> {
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
    });
}

#[test]
fn test_claim_payroll_success() {
    let env = create_test_environment();
    let contract_id = env.register_contract(None, PayrollContract);
    
    let agreement_id = 1u128;
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64; // 1 day
    let escrow_amount = 10000i128;
    
    setup_test_agreement(
        &env,
        &contract_id,
        agreement_id,
        &[(employee.clone(), salary)],
        period_duration,
        &token,
        escrow_amount,
    );
    // Fund the payroll contract with actual token balance to match escrow accounting.
    mint(&env, &token, &contract_id, escrow_amount);
    
    // Fast forward 2 periods
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 2 + 1;
    });
    
    // Claim payroll
    claim(&env, &contract_id, &employee, agreement_id, 0).unwrap();
    
    // Verify claimed periods updated
    let claimed = get_claimed(&env, &contract_id, agreement_id, 0);
    assert_eq!(claimed, 2u32);
    
    // Verify escrow balance decreased
    let remaining_escrow = env.as_contract(&contract_id, || {
        DataKey::get_agreement_escrow_balance(&env, agreement_id, &token)
    });
    assert_eq!(remaining_escrow, escrow_amount - (salary * 2));
    
    // Verify paid amount updated
    let paid_amount = env.as_contract(&contract_id, || {
        DataKey::get_agreement_paid_amount(&env, agreement_id)
    });
    assert_eq!(paid_amount, salary * 2);
}

#[test]
fn test_claim_payroll_unauthorized() {
    let env = create_test_environment();
    let contract_id = env.register_contract(None, PayrollContract);
    
    let agreement_id = 1u128;
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
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let escrow_amount = 10000i128;
    
    setup_test_agreement(
        &env,
        &contract_id,
        agreement_id,
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
fn test_claim_payroll_no_periods_to_claim() {
    let env = create_test_environment();
    let contract_id = env.register_contract(None, PayrollContract);
    
    let agreement_id = 1u128;
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let escrow_amount = 10000i128;
    
    setup_test_agreement(
        &env,
        &contract_id,
        agreement_id,
        &[(employee.clone(), salary)],
        period_duration,
        &token,
        escrow_amount,
    );
    mint(&env, &token, &contract_id, escrow_amount);
    
    // Don't fast forward time - no periods elapsed
    assert!(claim(&env, &contract_id, &employee, agreement_id, 0).is_err());
}

#[test]
fn test_claim_payroll_insufficient_escrow() {
    let env = create_test_environment();
    let contract_id = env.register_contract(None, PayrollContract);
    
    let agreement_id = 1u128;
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let escrow_amount = 500i128; // Less than one period's salary
    
    setup_test_agreement(
        &env,
        &contract_id,
        agreement_id,
        &[(employee.clone(), salary)],
        period_duration,
        &token,
        escrow_amount,
    );
    mint(&env, &token, &contract_id, escrow_amount);
    
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration + 1;
    });
    
    assert!(claim(&env, &contract_id, &employee, agreement_id, 0).is_err());
}

#[test]
fn test_multiple_employees_independent_claiming() {
    let env = create_test_environment();
    let contract_id = env.register_contract(None, PayrollContract);
    
    let agreement_id = 1u128;
    let employee1 = create_test_address(&env);
    let employee2 = create_test_address(&env);
    let token = create_token(&env);
    let salary1 = 1000i128;
    let salary2 = 2000i128;
    let period_duration = 86400u64;
    let escrow_amount = 50000i128;
    
    setup_test_agreement(
        &env,
        &contract_id,
        agreement_id,
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
    
    // Verify independent tracking
    let claimed1 = get_claimed(&env, &contract_id, agreement_id, 0);
    let claimed2 = get_claimed(&env, &contract_id, agreement_id, 1);
    assert_eq!(claimed1, 2u32);
    assert_eq!(claimed2, 2u32);
    
    // Verify escrow balance
    let remaining = env.as_contract(&contract_id, || {
        DataKey::get_agreement_escrow_balance(&env, agreement_id, &token)
    });
    assert_eq!(remaining, escrow_amount - (salary1 * 2) - (salary2 * 2));
}

#[test]
fn test_get_employee_claimed_periods() {
    let env = create_test_environment();
    let contract_id = env.register_contract(None, PayrollContract);
    
    let agreement_id = 1u128;
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let escrow_amount = 10000i128;
    
    setup_test_agreement(
        &env,
        &contract_id,
        agreement_id,
        &[(employee.clone(), salary)],
        period_duration,
        &token,
        escrow_amount,
    );
    mint(&env, &token, &contract_id, escrow_amount);
    
    // Initially should be 0
    let claimed = get_claimed(&env, &contract_id, agreement_id, 0);
    assert_eq!(claimed, 0u32);
    
    // After claiming
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration + 1;
    });
    
    claim(&env, &contract_id, &employee, agreement_id, 0).unwrap();
    
    let claimed_after = get_claimed(&env, &contract_id, agreement_id, 0);
    assert_eq!(claimed_after, 1u32);
}

#[test]
fn test_claim_partial_periods() {
    let env = create_test_environment();
    let contract_id = env.register_contract(None, PayrollContract);
    
    let agreement_id = 1u128;
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let escrow_amount = 10000i128;
    
    setup_test_agreement(
        &env,
        &contract_id,
        agreement_id,
        &[(employee.clone(), salary)],
        period_duration,
        &token,
        escrow_amount,
    );
    mint(&env, &token, &contract_id, escrow_amount);
    
    // Fast forward 3.5 periods (should claim 3 periods)
    env.ledger().with_mut(|li| {
        li.timestamp = (period_duration * 3) + (period_duration / 2) + 1;
    });
    
    claim(&env, &contract_id, &employee, agreement_id, 0).unwrap();
    // Should succeed
    
    let claimed = get_claimed(&env, &contract_id, agreement_id, 0);
    assert_eq!(claimed, 3u32);
}

#[test]
fn test_claim_multiple_times_accumulative() {
    let env = create_test_environment();
    let contract_id = env.register_contract(None, PayrollContract);
    
    let agreement_id = 1u128;
    let employee = create_test_address(&env);
    let token = create_token(&env);
    let salary = 1000i128;
    let period_duration = 86400u64;
    let escrow_amount = 10000i128;
    
    setup_test_agreement(
        &env,
        &contract_id,
        agreement_id,
        &[(employee.clone(), salary)],
        period_duration,
        &token,
        escrow_amount,
    );
    mint(&env, &token, &contract_id, escrow_amount);
    
    // First claim after 1 period
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration + 1;
    });
    
    claim(&env, &contract_id, &employee, agreement_id, 0).unwrap();
    assert_eq!(get_claimed(&env, &contract_id, agreement_id, 0), 1u32);
    
    // Second claim after 2 more periods (total 3)
    env.ledger().with_mut(|li| {
        li.timestamp = period_duration * 3 + 1;
    });
    
    claim(&env, &contract_id, &employee, agreement_id, 0).unwrap();
    assert_eq!(get_claimed(&env, &contract_id, agreement_id, 0), 3u32);
}
