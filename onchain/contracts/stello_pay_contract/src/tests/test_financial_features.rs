#![cfg(test)]

use crate::payroll::PayrollContractClient;
use soroban_sdk::token::StellarAssetClient as TokenAdmin;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, String,
};

fn setup_token(env: &Env) -> (Address, TokenAdmin) {
    let token_admin = Address::generate(env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone());
    (
        token_contract_id.address(),
        TokenAdmin::new(&env, &token_contract_id.address()),
    )
}

fn setup_payroll_with_deposit(env: &Env, client: &PayrollContractClient, employer: &Address, employee: &Address, token: &Address, token_admin: &TokenAdmin, amount: i128) {
    let interval = 86400u64; // 1 day
    let recurrence_frequency = 2592000u64; // 30 days

    env.mock_all_auths();

    // Mint tokens to employer first
    token_admin.mint(employer, &(amount * 10));

    client.initialize(employer);
    client.create_or_update_escrow(
        employer,
        employee,
        token,
        &amount,
        &interval,
        &recurrence_frequency,
    );

    // Deposit funds for employer
    client.deposit_tokens(employer, token, &(amount * 10));
}

#[test]
fn test_disburse_partial_salary_success() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let (token, token_admin) = setup_token(&env);
    let amount = 1000i128;

    setup_payroll_with_deposit(&env, &client, &employer, &employee, &token, &token_admin, amount);

    // Test 50% partial payment (5000 basis points)
    let percentage = 5000u32;
    let result = client.try_disburse_partial_salary(&employer, &employee, &percentage);
    assert!(result.is_ok());
}

#[test]
fn test_disburse_partial_salary_invalid_percentage() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let (token, token_admin) = setup_token(&env);
    let amount = 1000i128;

    setup_payroll_with_deposit(&env, &client, &employer, &employee, &token, &token_admin, amount);

    // Test invalid percentage (over 100%)
    let percentage = 15000u32; // 150%
    let result = client.try_disburse_partial_salary(&employer, &employee, &percentage);
    assert!(result.is_err());
}

#[test]
fn test_disburse_partial_salary_unauthorized() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let (token, token_admin) = setup_token(&env);
    let amount = 1000i128;

    setup_payroll_with_deposit(&env, &client, &employer, &employee, &token, &token_admin, amount);

    // Test unauthorized caller
    let percentage = 5000u32;
    let result = client.try_disburse_partial_salary(&unauthorized, &employee, &percentage);
    assert!(result.is_err());
}

#[test]
fn test_pay_overtime_success() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let (token, token_admin) = setup_token(&env);
    let amount = 1000i128;

    setup_payroll_with_deposit(&env, &client, &employer, &employee, &token, &token_admin, amount);

    // Test overtime payment
    let hours = 10u32;
    let hourly_rate = Some(50i128);
    let result = client.try_pay_overtime(&employer, &employee, &hours, &hourly_rate);
    assert!(result.is_ok());
}

#[test]
fn test_pay_overtime_zero_hours() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let (token, token_admin) = setup_token(&env);
    let amount = 1000i128;

    setup_payroll_with_deposit(&env, &client, &employer, &employee, &token, &token_admin, amount);

    // Test zero hours
    let hours = 0u32;
    let hourly_rate = Some(50i128);
    let result = client.try_pay_overtime(&employer, &employee, &hours, &hourly_rate);
    assert!(result.is_err());
}

#[test]
fn test_pay_overtime_unauthorized() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let (token, token_admin) = setup_token(&env);
    let amount = 1000i128;

    setup_payroll_with_deposit(&env, &client, &employer, &employee, &token, &token_admin, amount);

    // Test unauthorized caller
    let hours = 10u32;
    let hourly_rate = Some(50i128);
    let result = client.try_pay_overtime(&unauthorized, &employee, &hours, &hourly_rate);
    assert!(result.is_err());
}

#[test]
fn test_pay_bonus_success() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let (token, token_admin) = setup_token(&env);
    let amount = 1000i128;

    setup_payroll_with_deposit(&env, &client, &employer, &employee, &token, &token_admin, amount);

    // Test bonus payment
    let bonus_amount = 500i128;
    let bonus_reason = String::from_str(&env, "Performance bonus");
    let result = client.try_pay_bonus(&employer, &employee, &bonus_amount, &bonus_reason);
    assert!(result.is_ok());
}

#[test]
fn test_pay_bonus_zero_amount() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let (token, token_admin) = setup_token(&env);
    let amount = 1000i128;

    setup_payroll_with_deposit(&env, &client, &employer, &employee, &token, &token_admin, amount);

    // Test zero bonus amount
    let bonus_amount = 0i128;
    let bonus_reason = String::from_str(&env, "No bonus");
    let result = client.try_pay_bonus(&employer, &employee, &bonus_amount, &bonus_reason);
    assert!(result.is_err());
}

#[test]
fn test_pay_bonus_negative_amount() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let (token, token_admin) = setup_token(&env);
    let amount = 1000i128;

    setup_payroll_with_deposit(&env, &client, &employer, &employee, &token, &token_admin, amount);

    // Test negative bonus amount
    let bonus_amount = -100i128;
    let bonus_reason = String::from_str(&env, "Negative bonus");
    let result = client.try_pay_bonus(&employer, &employee, &bonus_amount, &bonus_reason);
    assert!(result.is_err());
}

#[test]
fn test_disburse_salary_with_tax_success() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let (token, token_admin) = setup_token(&env);
    let amount = 1000i128;

    setup_payroll_with_deposit(&env, &client, &employer, &employee, &token, &token_admin, amount);

    // Advance time to make payout eligible
    env.ledger().with_mut(|l| l.timestamp = 2592001); // Past the 30-day window

    // Test tax withholding (20% tax rate = 2000 basis points)
    let tax_rate = 2000u32;
    let result = client.try_disburse_salary_with_tax(&employer, &employee, &tax_rate);
    assert!(result.is_ok());
}

#[test]
fn test_disburse_salary_with_tax_invalid_rate() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let (token, token_admin) = setup_token(&env);
    let amount = 1000i128;

    setup_payroll_with_deposit(&env, &client, &employer, &employee, &token, &token_admin, amount);

    // Test invalid tax rate (over 100%)
    let tax_rate = 15000u32; // 150%
    let result = client.try_disburse_salary_with_tax(&employer, &employee, &tax_rate);
    assert!(result.is_err());
}

#[test]
fn test_get_payment_summary_success() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let (token, token_admin) = setup_token(&env);
    let amount = 1000i128;

    setup_payroll_with_deposit(&env, &client, &employer, &employee, &token, &token_admin, amount);

    // Test payment summary
    let summary = client.get_payment_summary(&employee);
    assert!(summary.len() > 0);
}

#[test]
fn test_export_payment_data_success() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let (token, token_admin) = setup_token(&env);
    let amount = 1000i128;

    setup_payroll_with_deposit(&env, &client, &employer, &employee, &token, &token_admin, amount);

    // Test payment data export
    let export_data = client.try_export_payment_data(&employer, &employee);
    assert!(export_data.is_ok());
}

#[test]
fn test_set_currency_preference_success() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let (token, token_admin) = setup_token(&env);
    let amount = 1000i128;

    setup_payroll_with_deposit(&env, &client, &employer, &employee, &token, &token_admin, amount);

    // Test setting currency preference
    let preferred_token = Address::generate(&env);
    let result = client.try_set_currency_preference(&employer, &employee, &preferred_token);
    assert!(result.is_ok());
}

#[test]
fn test_get_payment_types_success() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    // Test get payment types
    let payment_types = client.get_payment_types();
    assert!(payment_types.len() > 0);
}

#[test]
fn test_boundary_values_partial_salary() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let (token, token_admin) = setup_token(&env);
    let amount = 1000i128;

    setup_payroll_with_deposit(&env, &client, &employer, &employee, &token, &token_admin, amount);

    // Test minimum valid percentage (1 basis point = 0.01%)
    let min_percentage = 1u32;
    let result = client.try_disburse_partial_salary(&employer, &employee, &min_percentage);
    assert!(result.is_ok());

    // Test maximum valid percentage (10000 basis points = 100%)
    let max_percentage = 10000u32;
    let result = client.try_disburse_partial_salary(&employer, &employee, &max_percentage);
    assert!(result.is_ok());
}

#[test]
fn test_get_payment_summary_nonexistent_employee() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let nonexistent_employee = Address::generate(&env);

    // Test payment summary for nonexistent employee
    let summary = client.get_payment_summary(&nonexistent_employee);
    assert_eq!(summary.len(), 0);
}

#[test]
fn test_boundary_values_tax_rate() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let (token, token_admin) = setup_token(&env);
    let amount = 1000i128;

    setup_payroll_with_deposit(&env, &client, &employer, &employee, &token, &token_admin, amount);

    // Advance time to make payout eligible
    env.ledger().with_mut(|l| l.timestamp = 2592001); // Past the 30-day window

    // Test minimum valid tax rate (0%)
    let min_tax_rate = 0u32;
    let result = client.try_disburse_salary_with_tax(&employer, &employee, &min_tax_rate);
    assert!(result.is_ok());

    // Need to create a new payroll for the second test since the first already disbursed
    let employee2 = Address::generate(&env);
    client.create_or_update_escrow(
        &employer,
        &employee2,
        &token,
        &amount,
        &86400u64,
        &2592000u64,
    );

    // Test maximum valid tax rate (50% = 5000 basis points - adjusted per implementation)
    let max_tax_rate = 5000u32;
    let result = client.try_disburse_salary_with_tax(&employer, &employee2, &max_tax_rate);
    assert!(result.is_ok());
}


