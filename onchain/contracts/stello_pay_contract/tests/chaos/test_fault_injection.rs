#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};

use stello_pay_contract::storage::{AgreementStatus, DataKey, PayrollError};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

const ONE_DAY: u64 = 86400;
const ONE_WEEK: u64 = 604800;

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn create_address(env: &Env) -> Address {
    Address::generate(env)
}

fn create_token(env: &Env) -> Address {
    let admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

fn setup_contract(env: &Env) -> (Address, PayrollContractClient<'static>) {
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = create_address(env);
    client.initialize(&owner);
    (contract_id, client)
}

fn advance_time(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| {
        li.timestamp += seconds;
    });
}

/// Chaos test: simulate token transfer failures by deliberately not minting
/// on-chain tokens while keeping escrow metadata non-zero.
///
/// Verifies that:
/// - payment flows surface `TransferFailed`-style behavior via `PayrollError`
/// - agreement state, claimed periods, and escrow balance remain unchanged.
#[test]
fn chaos_token_transfer_failure_does_not_corrupt_state() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&agreement_id, &employee, &1_000i128);
    client.activate_agreement(&agreement_id);

    // Set up DataKey-based escrow tracking but do NOT mint any tokens to the contract.
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, 10_000);
        DataKey::set_employee_count(&env, agreement_id, 1);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, 1_000);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
    });

    let status_before = client.get_agreement(&agreement_id).unwrap().status;
    let claimed_before = client.get_employee_claimed_periods(&agreement_id, &0u32);
    let escrow_before: i128 = env.as_contract(&contract_id, || {
        DataKey::get_agreement_escrow_balance(&env, agreement_id, &token)
    });

    advance_time(&env, ONE_DAY + 1);

    // Attempting a claim should panic inside the token contract due to missing
    // on-chain balance; catch the error via try_ wrapper.
    let res = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert!(res.is_err());

    let status_after = client.get_agreement(&agreement_id).unwrap().status;
    let claimed_after = client.get_employee_claimed_periods(&agreement_id, &0u32);
    let escrow_after: i128 = env.as_contract(&contract_id, || {
        DataKey::get_agreement_escrow_balance(&env, agreement_id, &token)
    });

    assert_eq!(status_before, AgreementStatus::Active);
    assert_eq!(status_after, AgreementStatus::Active);
    assert_eq!(claimed_before, 0);
    assert_eq!(claimed_after, 0);
    assert_eq!(escrow_before, escrow_after);
}

/// Chaos test: simulate storage write failures by injecting an inconsistent
/// escrow balance mid-execution and verifying that retrying after correction
/// leads to a clean success path.
#[test]
fn chaos_escrow_misconfiguration_then_recovery() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&agreement_id, &employee, &1_000i128);
    client.activate_agreement(&agreement_id);

    // Misconfigured storage: escrow balance says 0 while contract has tokens.
    mint(&env, &token, &contract_id, 10_000);
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, 0);
        DataKey::set_employee_count(&env, agreement_id, 1);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, 1_000);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
    });

    advance_time(&env, ONE_DAY + 1);

    let first = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert_eq!(first, Err(Ok(PayrollError::InsufficientEscrowBalance)));

    // "Recover" by correcting escrow balance and retrying.
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, 10_000);
    });

    let second = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert!(second.is_ok());
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0u32), 1);
}

/// Chaos test: inject a fault mid-batch where one employee's claim drains
/// escrow and a second employee's claim observes a failure. Verifies that
/// partial completion is reported correctly and state remains consistent.
#[test]
fn chaos_batch_partial_completion_and_rollback() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let e1 = create_address(&env);
    let e2 = create_address(&env);
    let token = create_token(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&agreement_id, &e1, &1_000i128);
    client.add_employee_to_agreement(&agreement_id, &e2, &1_000i128);
    client.activate_agreement(&agreement_id);

    // Fund for only one employee period and set DataKey storage accordingly.
    mint(&env, &token, &contract_id, 1_000);
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, 1_000);
        DataKey::set_employee_count(&env, agreement_id, 2);
        DataKey::set_employee(&env, agreement_id, 0, &e1);
        DataKey::set_employee_salary(&env, agreement_id, 0, 1_000);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee(&env, agreement_id, 1, &e2);
        DataKey::set_employee_salary(&env, agreement_id, 1, 1_000);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 1, 0);
    });

    advance_time(&env, ONE_DAY + 1);

    let indices = soroban_sdk::Vec::from_array(&env, [0u32, 1u32]);
    let batch = client
        .batch_claim_payroll(&e1, &agreement_id, &indices)
        .unwrap();

    assert_eq!(batch.successful_claims, 1);
    assert_eq!(batch.failed_claims, 1);

    let r0 = batch.results.get(0).unwrap();
    let r1 = batch.results.get(1).unwrap();

    assert!(r0.success);
    assert!(!r1.success);
    assert_eq!(r1.error_code, PayrollError::InsufficientEscrowBalance as u32);

    // State: e1 has claimed one period, e2 still at zero.
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0u32), 1);
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &1u32), 0);
}

