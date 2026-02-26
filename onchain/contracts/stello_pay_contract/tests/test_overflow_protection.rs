//! Overflow / Underflow Protection Tests (#203).
//!
//! These tests focus specifically on the arithmetic guards used in payroll and
//! escrow flows, ensuring that operations either succeed safely or fail with a
//! well-defined `PayrollError` instead of silently overflowing.

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

use stello_pay_contract::storage::{DataKey, PayrollError};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn setup_contract(env: &Env) -> (Address, PayrollContractClient<'static>, Address) {
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (contract_id, client, owner)
}

/// Verifies that `claim_payroll` uses checked multiplication for the amount
/// calculation and returns `InvalidData` instead of overflowing when the
/// product of `salary_per_period` and `periods_to_pay` would exceed `i128`.
#[test]
fn test_claim_payroll_amount_multiplication_overflow_guard() {
    let env = Env::default();
    env.mock_all_auths();

    let (_contract_id, client, _owner) = setup_contract(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    // Create a payroll agreement; we will override the DataKey state below.
    let grace: u64 = 604_800;
    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace);

    // Seed DataKey state to force an overflow in salary_per_period * periods.
    env.as_contract(&client.address, || {
        // Activation + duration for 3 periods.
        DataKey::set_agreement_activation_time(&env, agreement_id, 0);
        DataKey::set_agreement_period_duration(&env, agreement_id, 1);
        DataKey::set_agreement_token(&env, agreement_id, &token);

        // One employee, index 0, with maximum salary.
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, i128::MAX);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee_count(&env, agreement_id, 1);

        // Generous escrow so insufficiency does not mask arithmetic errors.
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, i128::MAX);
    });

    // Advance to 3 elapsed periods so `periods_to_pay = 3` and
    // MAX * 3 would overflow without checked arithmetic.
    env.ledger().with_mut(|li| {
        li.timestamp = 3;
    });

    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert_eq!(result, Err(Ok(PayrollError::InvalidData)));
}

/// Verifies that `claim_payroll` safely rejects addition overflow when
/// updating the cumulative `AgreementPaidAmount`.
#[test]
fn test_claim_payroll_paid_amount_addition_overflow_guard() {
    let env = Env::default();
    env.mock_all_auths();

    let (_contract_id, client, _owner) = setup_contract(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let grace: u64 = 604_800;
    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace);

    env.as_contract(&client.address, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, 0);
        DataKey::set_agreement_period_duration(&env, agreement_id, 1);
        DataKey::set_agreement_token(&env, agreement_id, &token);

        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, 1);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee_count(&env, agreement_id, 1);

        // Pre‑seed paid amount to i128::MAX so any positive claim would
        // overflow without a checked_add guard.
        DataKey::set_agreement_paid_amount(&env, agreement_id, i128::MAX);

        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, i128::MAX);
    });

    env.ledger().with_mut(|li| {
        li.timestamp = 1;
    });

    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert_eq!(result, Err(Ok(PayrollError::InvalidData)));
}

