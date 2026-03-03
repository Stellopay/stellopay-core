//! Payment failure handling test suite (#222).
//!
//! Validates contract behavior under payment failure conditions and verifies
//! that recovery mechanisms restore the system to a consistent state.
//!
//! # Coverage
//!
//! | Section | Scenario |
//! |---------|----------|
//! | 1 | Insufficient escrow balance — payroll claim rejected |
//! | 2 | Insufficient escrow balance — time-based claim rejected |
//! | 3 | Insufficient escrow balance — batch payroll partial failure |
//! | 4 | Token transfer failure — contract has no on-chain token balance |
//! | 5 | Recovery — pause and resume preserves claimable state |
//! | 6 | Recovery — grace period claim after cancellation |
//! | 7 | Recovery — fund and retry after insufficient balance |
//! | 8 | Failure notifications — batch payroll error codes |
//! | 9 | Failure notifications — batch milestone error codes |
//! | 10 | State consistency — escrow balance unchanged after failed claim |
//! | 11 | State consistency — claimed periods unchanged after failure |
//! | 12 | State consistency — agreement status unchanged after failed claim |
//! | 13 | State consistency — paid amount unchanged after failed claim |
//! | 14 | Claim on non-activated agreement rejected |
//! | 15 | Claim on paused agreement rejected |
//! | 16 | Claim with wrong agreement mode rejected |
//! | 17 | Claim by unauthorized caller rejected |
//! | 18 | Claim with invalid employee index rejected |
//! | 19 | Double claim for same period rejected |
//! | 20 | All periods claimed — subsequent claim rejected |
//! | 21 | Batch payroll — mixed success and failure preserves state |
//! | 22 | Milestone claim on paused agreement rejected |
//! | 23 | Milestone claim without approval rejected |
//! | 24 | Grace period expired — claim rejected |

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, Vec,
};
use stello_pay_contract::storage::{DataKey, PayrollError};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ============================================================================
// CONSTANTS
// ============================================================================

const ONE_DAY: u64 = 86400;
const ONE_WEEK: u64 = 604800;
const STANDARD_SALARY: i128 = 1000;

// ============================================================================
// HELPERS
// ============================================================================

/// Creates a fresh test environment with all auths mocked.
fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Generates a random test address.
fn create_address(env: &Env) -> Address {
    Address::generate(env)
}

/// Deploys a Stellar Asset Contract and returns its address.
fn create_token(env: &Env) -> Address {
    let admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

/// Mints tokens to a given address.
fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

/// Registers the contract, initializes it, and returns (contract_id, client).
fn setup_contract(env: &Env) -> (Address, PayrollContractClient<'static>) {
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = create_address(env);
    client.initialize(&owner);
    (contract_id, client)
}

/// Seeds the DataKey-based storage that `claim_payroll` reads at runtime.
///
/// This mirrors the setup pattern used across the existing test suite.
/// The payroll claiming path reads activation time, period duration, token,
/// employee addresses, salaries, and claimed period counters from DataKey
/// storage, which must be populated independently of the Agreement struct.
fn seed_payroll_claim_storage(
    env: &Env,
    contract_id: &Address,
    agreement_id: u128,
    token: &Address,
    employees: &[(Address, i128)],
    escrow_balance: i128,
) {
    env.as_contract(contract_id, || {
        DataKey::set_agreement_activation_time(env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(env, agreement_id, token);
        DataKey::set_agreement_escrow_balance(env, agreement_id, token, escrow_balance);
        DataKey::set_employee_count(env, agreement_id, employees.len() as u32);

        for (index, (addr, salary)) in employees.iter().enumerate() {
            DataKey::set_employee(env, agreement_id, index as u32, addr);
            DataKey::set_employee_salary(env, agreement_id, index as u32, *salary);
            DataKey::set_employee_claimed_periods(env, agreement_id, index as u32, 0);
        }
    });
}

/// Advances the ledger timestamp by the given number of seconds.
fn advance_time(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| {
        li.timestamp += seconds;
    });
}

/// Creates a payroll agreement, adds one employee, activates it, and seeds
/// the DataKey storage with the specified escrow balance. Returns the
/// agreement ID.
fn setup_funded_payroll(
    env: &Env,
    contract_id: &Address,
    client: &PayrollContractClient,
    employer: &Address,
    employee: &Address,
    token: &Address,
    salary: i128,
    escrow_balance: i128,
) -> u128 {
    let agreement_id = client.create_payroll_agreement(employer, token, &ONE_WEEK);
    client.add_employee_to_agreement(&agreement_id, employee, &salary);
    client.activate_agreement(&agreement_id);

    seed_payroll_claim_storage(
        env,
        contract_id,
        agreement_id,
        token,
        &[(employee.clone(), salary)],
        escrow_balance,
    );

    // Mint actual tokens to the contract so transfers can succeed when balance allows.
    if escrow_balance > 0 {
        mint(env, token, contract_id, escrow_balance);
    }

    agreement_id
}

/// Creates a funded escrow (time-based) agreement. Returns the agreement ID.
fn setup_funded_escrow(
    env: &Env,
    client: &PayrollContractClient,
    employer: &Address,
    contributor: &Address,
    token: &Address,
    amount_per_period: i128,
    period_seconds: u64,
    num_periods: u32,
    fund: bool,
) -> u128 {
    let agreement_id = client.create_escrow_agreement(
        employer,
        contributor,
        token,
        &amount_per_period,
        &period_seconds,
        &num_periods,
    );

    let total = amount_per_period * (num_periods as i128);

    if fund {
        mint(env, token, &client.address, total);
        env.as_contract(&client.address, || {
            DataKey::set_agreement_escrow_balance(env, agreement_id, token, total);
        });
    }

    client.activate_agreement(&agreement_id);
    agreement_id
}

// ============================================================================
// SECTION 1: INSUFFICIENT ESCROW BALANCE — PAYROLL CLAIM
// ============================================================================

/// Payroll claim must return InsufficientEscrowBalance when the tracked escrow
/// balance is lower than the owed salary, even if time periods have elapsed.
#[test]
fn test_payroll_claim_insufficient_escrow_balance() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    // Fund with 500 but salary is 1000 per period.
    let agreement_id = setup_funded_payroll(
        &env,
        &contract_id,
        &client,
        &employer,
        &employee,
        &token,
        STANDARD_SALARY,
        500, // less than one period salary
    );

    advance_time(&env, ONE_DAY + 1);

    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert_eq!(result, Err(Ok(PayrollError::InsufficientEscrowBalance)));
}

/// Payroll claim must fail when escrow balance is exactly zero.
#[test]
fn test_payroll_claim_zero_escrow_balance() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&agreement_id, &employee, &STANDARD_SALARY);
    client.activate_agreement(&agreement_id);

    // Seed with zero escrow balance.
    seed_payroll_claim_storage(
        &env,
        &contract_id,
        agreement_id,
        &token,
        &[(employee.clone(), STANDARD_SALARY)],
        0,
    );

    advance_time(&env, ONE_DAY + 1);

    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert_eq!(result, Err(Ok(PayrollError::InsufficientEscrowBalance)));
}

/// When escrow covers only part of the owed periods, the claim for the full
/// elapsed amount must fail (no partial payout in single claim).
#[test]
fn test_payroll_claim_partial_escrow_covers_fewer_periods() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    // Fund for 2 periods, but 3 periods will have elapsed.
    let agreement_id = setup_funded_payroll(
        &env,
        &contract_id,
        &client,
        &employer,
        &employee,
        &token,
        STANDARD_SALARY,
        STANDARD_SALARY * 2,
    );

    advance_time(&env, ONE_DAY * 3 + 1);

    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert_eq!(result, Err(Ok(PayrollError::InsufficientEscrowBalance)));
}

// ============================================================================
// SECTION 2: INSUFFICIENT ESCROW BALANCE — TIME-BASED CLAIM
// ============================================================================

/// Time-based escrow claim must return InsufficientEscrowBalance when the
/// DataKey escrow balance has been drained.
#[test]
fn test_time_based_claim_insufficient_escrow_balance() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);

    // Create escrow but do NOT fund it (fund = false).
    let agreement_id = setup_funded_escrow(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        STANDARD_SALARY,
        ONE_DAY,
        4,
        false,
    );

    advance_time(&env, ONE_DAY + 1);

    let result = client.try_claim_time_based(&agreement_id);
    assert_eq!(result, Err(Ok(PayrollError::InsufficientEscrowBalance)));
}

/// Time-based claim where escrow covers the first period but not the second,
/// after both periods have elapsed. First claim succeeds, second fails.
#[test]
fn test_time_based_claim_escrow_drains_across_periods() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);

    let amount_per_period = STANDARD_SALARY;
    let period_seconds = ONE_DAY;
    let num_periods = 4u32;

    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &amount_per_period,
        &period_seconds,
        &num_periods,
    );

    // Fund only enough for 1 period.
    mint(&env, &token, &client.address, amount_per_period);
    env.as_contract(&client.address, || {
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, amount_per_period);
    });

    client.activate_agreement(&agreement_id);

    // Claim after 1 period — should succeed.
    advance_time(&env, ONE_DAY + 1);
    let result = client.try_claim_time_based(&agreement_id);
    assert!(result.is_ok());
    assert_eq!(client.get_claimed_periods(&agreement_id), 1u32);

    // Claim after 2nd period — should fail, escrow is drained.
    advance_time(&env, ONE_DAY);
    let result = client.try_claim_time_based(&agreement_id);
    assert_eq!(result, Err(Ok(PayrollError::InsufficientEscrowBalance)));
}

// ============================================================================
// SECTION 3: INSUFFICIENT ESCROW — BATCH PAYROLL PARTIAL FAILURE
// ============================================================================

/// When one employee drains the escrow, a subsequent batch claim by the
/// second employee returns InsufficientEscrowBalance.
#[test]
fn test_batch_payroll_partial_escrow_failure() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let e1 = create_address(&env);
    let e2 = create_address(&env);
    let token = create_token(&env);
    let salary = STANDARD_SALARY;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&agreement_id, &e1, &salary);
    client.add_employee_to_agreement(&agreement_id, &e2, &salary);
    client.activate_agreement(&agreement_id);

    // Fund for only 1 employee's 1 period.
    let escrow = salary;
    mint(&env, &token, &contract_id, escrow);

    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, escrow);
        DataKey::set_employee_count(&env, agreement_id, 2);

        DataKey::set_employee(&env, agreement_id, 0, &e1);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);

        DataKey::set_employee(&env, agreement_id, 1, &e2);
        DataKey::set_employee_salary(&env, agreement_id, 1, salary);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 1, 0);
    });

    advance_time(&env, ONE_DAY + 1);

    // e1 claims successfully, draining the escrow.
    let batch_e1 = client.batch_claim_payroll(&e1, &agreement_id, &Vec::from_array(&env, [0u32]));
    assert_eq!(batch_e1.successful_claims, 1);
    assert_eq!(batch_e1.total_claimed, salary);

    // e2 attempts to claim — escrow is empty.
    let batch_e2 = client.batch_claim_payroll(&e2, &agreement_id, &Vec::from_array(&env, [1u32]));
    assert_eq!(batch_e2.successful_claims, 0);
    assert_eq!(batch_e2.failed_claims, 1);

    let r = batch_e2.results.get(0).unwrap();
    assert!(!r.success);
    assert_eq!(r.error_code, PayrollError::InsufficientEscrowBalance as u32);
}

// ============================================================================
// SECTION 4: TOKEN TRANSFER FAILURE — NO ON-CHAIN BALANCE
// ============================================================================

/// When the DataKey escrow balance is set but no actual tokens were minted to
/// the contract address, the transfer call panics. This tests the failure mode
/// for a misconfigured escrow.
#[test]
#[should_panic]
fn test_payroll_claim_panics_without_onchain_tokens() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&agreement_id, &employee, &STANDARD_SALARY);
    client.activate_agreement(&agreement_id);

    // Set escrow balance in DataKey but do NOT mint tokens to the contract.
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, 10000);
        DataKey::set_employee_count(&env, agreement_id, 1);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, STANDARD_SALARY);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
    });

    advance_time(&env, ONE_DAY + 1);

    // This panics inside the token contract because the contract address has
    // insufficient on-chain token balance despite the DataKey saying otherwise.
    let _ = client.claim_payroll(&employee, &agreement_id, &0u32);
}

/// Time-based escrow claim also panics when the on-chain token balance is zero
/// but DataKey escrow balance was set.
#[test]
#[should_panic]
fn test_time_based_claim_panics_without_onchain_tokens() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);

    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &STANDARD_SALARY,
        &ONE_DAY,
        &4u32,
    );

    // Set DataKey escrow balance but do NOT mint tokens.
    env.as_contract(&client.address, || {
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, STANDARD_SALARY * 4);
    });

    client.activate_agreement(&agreement_id);
    advance_time(&env, ONE_DAY + 1);

    // Panics on the token transfer.
    let _ = client.claim_time_based(&agreement_id);
}

// ============================================================================
// SECTION 5: RECOVERY — PAUSE AND RESUME PRESERVES CLAIMABLE STATE
// ============================================================================

/// Pausing an agreement blocks claims; resuming it allows the employee to
/// claim again for the full elapsed period.
#[test]
fn test_pause_blocks_claim_resume_allows_claim() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_funded_payroll(
        &env,
        &contract_id,
        &client,
        &employer,
        &employee,
        &token,
        STANDARD_SALARY,
        STANDARD_SALARY * 10,
    );

    advance_time(&env, ONE_DAY + 1);

    // Pause — claim should fail.
    client.pause_agreement(&agreement_id);

    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert!(
        result.is_err()
            || result
                .as_ref()
                .ok()
                .and_then(|r| r.as_ref().err())
                .is_some()
    );

    // Resume — claim should succeed.
    client.resume_agreement(&agreement_id);

    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert!(result.is_ok());
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0u32), 1);
}

/// Pausing and resuming a time-based escrow agreement preserves claimed
/// periods and allows continued claiming.
#[test]
fn test_escrow_pause_resume_preserves_state() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_funded_escrow(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        STANDARD_SALARY,
        ONE_DAY,
        4,
        true,
    );

    advance_time(&env, ONE_DAY + 1);

    // Claim first period.
    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_claimed_periods(&agreement_id), 1);

    // Pause — claim should fail.
    client.pause_agreement(&agreement_id);
    advance_time(&env, ONE_DAY);

    let result = client.try_claim_time_based(&agreement_id);
    assert_eq!(result, Err(Ok(PayrollError::AgreementPaused)));

    // Resume — claim should succeed for the period that elapsed during pause.
    client.resume_agreement(&agreement_id);
    let result = client.try_claim_time_based(&agreement_id);
    assert!(result.is_ok());
    assert_eq!(client.get_claimed_periods(&agreement_id), 2);
}

// ============================================================================
// SECTION 6: RECOVERY — GRACE PERIOD CLAIM AFTER CANCELLATION
// ============================================================================

/// After cancellation, claims succeed during the grace period and fail
/// once the grace period expires.
#[test]
fn test_time_based_claim_during_and_after_grace_period() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);

    let amount_per_period = STANDARD_SALARY;
    let period_seconds = ONE_DAY;
    let num_periods = 10u32;

    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &amount_per_period,
        &period_seconds,
        &num_periods,
    );

    let total = amount_per_period * (num_periods as i128);
    mint(&env, &token, &client.address, total);
    env.as_contract(&client.address, || {
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, total);
    });

    client.activate_agreement(&agreement_id);

    // Advance 2 periods and claim.
    advance_time(&env, period_seconds * 2 + 1);
    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_claimed_periods(&agreement_id), 2);

    // Cancel agreement — grace period starts.
    client.cancel_agreement(&agreement_id);
    assert!(client.is_grace_period_active(&agreement_id));

    // Advance 1 more period within grace window — claim should succeed.
    advance_time(&env, period_seconds);
    let result = client.try_claim_time_based(&agreement_id);
    assert!(result.is_ok());
    assert_eq!(client.get_claimed_periods(&agreement_id), 3);

    // Wait until grace period expires.
    let grace_end = client.get_grace_period_end(&agreement_id).unwrap();
    let current = env.ledger().timestamp();
    if grace_end > current {
        advance_time(&env, grace_end - current + 1);
    }

    assert!(!client.is_grace_period_active(&agreement_id));

    // Claim after grace period — should fail.
    advance_time(&env, period_seconds);
    let result = client.try_claim_time_based(&agreement_id);
    assert!(result.is_err());
}

// ============================================================================
// SECTION 7: RECOVERY — FUND AND RETRY AFTER INSUFFICIENT BALANCE
// ============================================================================

/// After a claim fails due to InsufficientEscrowBalance, topping up the escrow
/// allows the same claim to succeed on retry.
#[test]
fn test_fund_and_retry_after_insufficient_balance() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    // Start with zero escrow.
    let agreement_id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&agreement_id, &employee, &STANDARD_SALARY);
    client.activate_agreement(&agreement_id);

    seed_payroll_claim_storage(
        &env,
        &contract_id,
        agreement_id,
        &token,
        &[(employee.clone(), STANDARD_SALARY)],
        0,
    );

    advance_time(&env, ONE_DAY + 1);

    // First attempt fails.
    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert_eq!(result, Err(Ok(PayrollError::InsufficientEscrowBalance)));

    // Top up escrow.
    mint(&env, &token, &contract_id, STANDARD_SALARY * 5);
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, STANDARD_SALARY * 5);
    });

    // Retry — should succeed.
    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert!(result.is_ok());
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0u32), 1);
}

// ============================================================================
// SECTION 8: FAILURE NOTIFICATIONS — BATCH PAYROLL ERROR CODES
// ============================================================================

/// Batch payroll returns per-employee error codes for various failure reasons
/// (unauthorized, no periods, insufficient balance) while successful claims
/// report error_code 0.
#[test]
fn test_batch_payroll_error_codes() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let other = create_address(&env);
    let token = create_token(&env);
    let salary = STANDARD_SALARY;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary);
    client.add_employee_to_agreement(&agreement_id, &other, &salary);
    client.activate_agreement(&agreement_id);

    let escrow = salary * 10;
    mint(&env, &token, &contract_id, escrow);

    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, escrow);
        DataKey::set_employee_count(&env, agreement_id, 2);

        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);

        DataKey::set_employee(&env, agreement_id, 1, &other);
        DataKey::set_employee_salary(&env, agreement_id, 1, salary);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 1, 0);
    });

    advance_time(&env, ONE_DAY + 1);

    // Employee claims index 0 (self — success) and index 1 (not self — Unauthorized).
    let indices = Vec::from_array(&env, [0u32, 1u32]);
    let batch = client.batch_claim_payroll(&employee, &agreement_id, &indices);

    assert_eq!(batch.successful_claims, 1);
    assert_eq!(batch.failed_claims, 1);

    let r0 = batch.results.get(0).unwrap();
    assert!(r0.success);
    assert_eq!(r0.error_code, 0);

    let r1 = batch.results.get(1).unwrap();
    assert!(!r1.success);
    assert_eq!(r1.error_code, PayrollError::Unauthorized as u32);
}

/// Batch payroll with an out-of-bounds employee index reports
/// InvalidEmployeeIndex in the result.
#[test]
fn test_batch_payroll_invalid_index_error_code() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);
    let salary = STANDARD_SALARY;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary);
    client.activate_agreement(&agreement_id);

    let escrow = salary * 10;
    mint(&env, &token, &contract_id, escrow);

    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, escrow);
        DataKey::set_employee_count(&env, agreement_id, 1);

        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
    });

    advance_time(&env, ONE_DAY + 1);

    // Index 99 does not exist.
    let indices = Vec::from_array(&env, [0u32, 99u32]);
    let batch = client.batch_claim_payroll(&employee, &agreement_id, &indices);

    assert_eq!(batch.successful_claims, 1);
    assert_eq!(batch.failed_claims, 1);

    let r1 = batch.results.get(1).unwrap();
    assert!(!r1.success);
    assert_eq!(r1.error_code, PayrollError::InvalidEmployeeIndex as u32);
}

// ============================================================================
// SECTION 9: FAILURE NOTIFICATIONS — BATCH MILESTONE ERROR CODES
// ============================================================================

/// Batch milestone claiming returns correct error codes: unapproved (3),
/// already claimed (4), invalid ID (2), and duplicate (1).
#[test]
fn test_batch_milestone_error_codes() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    // Add 3 milestones.
    client.add_milestone(&agreement_id, &500);
    client.add_milestone(&agreement_id, &600);
    client.add_milestone(&agreement_id, &700);

    // Approve and claim milestone 1.
    client.approve_milestone(&agreement_id, &1u32);
    mint(&env, &token, &client.address, 500);
    client.claim_milestone(&agreement_id, &1u32);

    // Approve milestone 2 (unclaimed).
    client.approve_milestone(&agreement_id, &2u32);
    mint(&env, &token, &client.address, 600);

    // Milestone 3 is NOT approved.

    // Batch: already claimed (1), valid approved (2), unapproved (3), invalid (99), duplicate (2).
    let ids = Vec::from_array(&env, [1u32, 2u32, 3u32, 99u32, 2u32]);
    let batch = client.batch_claim_milestones(&agreement_id, &ids);

    assert_eq!(batch.successful_claims, 1);
    assert_eq!(batch.failed_claims, 4);

    // ID 1: already claimed.
    let r0 = batch.results.get(0).unwrap();
    assert!(!r0.success);
    assert_eq!(r0.error_code, 4); // already claimed

    // ID 2: success.
    let r1 = batch.results.get(1).unwrap();
    assert!(r1.success);
    assert_eq!(r1.error_code, 0);
    assert_eq!(r1.amount_claimed, 600);

    // ID 3: not approved.
    let r2 = batch.results.get(2).unwrap();
    assert!(!r2.success);
    assert_eq!(r2.error_code, 3); // not approved

    // ID 99: invalid ID.
    let r3 = batch.results.get(3).unwrap();
    assert!(!r3.success);
    assert_eq!(r3.error_code, 2); // invalid ID

    // ID 2 again: duplicate.
    let r4 = batch.results.get(4).unwrap();
    assert!(!r4.success);
    assert_eq!(r4.error_code, 1); // duplicate
}

// ============================================================================
// SECTION 10: STATE CONSISTENCY — ESCROW BALANCE UNCHANGED AFTER FAILURE
// ============================================================================

/// After a failed payroll claim (InsufficientEscrowBalance), the DataKey
/// escrow balance must remain unchanged.
#[test]
fn test_escrow_balance_unchanged_after_failed_claim() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);
    let initial_escrow = 200i128;

    let agreement_id = setup_funded_payroll(
        &env,
        &contract_id,
        &client,
        &employer,
        &employee,
        &token,
        STANDARD_SALARY,
        initial_escrow,
    );

    advance_time(&env, ONE_DAY + 1);

    let _ = client.try_claim_payroll(&employee, &agreement_id, &0u32);

    // Verify escrow balance is unchanged.
    let escrow_after: i128 = env.as_contract(&contract_id, || {
        DataKey::get_agreement_escrow_balance(&env, agreement_id, &token)
    });
    assert_eq!(escrow_after, initial_escrow);
}

// ============================================================================
// SECTION 11: STATE CONSISTENCY — CLAIMED PERIODS UNCHANGED AFTER FAILURE
// ============================================================================

/// After a failed claim, the employee's claimed_periods counter must remain at
/// its pre-failure value.
#[test]
fn test_claimed_periods_unchanged_after_failed_claim() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_funded_payroll(
        &env,
        &contract_id,
        &client,
        &employer,
        &employee,
        &token,
        STANDARD_SALARY,
        STANDARD_SALARY, // only 1 period funded
    );

    advance_time(&env, ONE_DAY + 1);

    // Succeed on first claim.
    let _ = client.claim_payroll(&employee, &agreement_id, &0u32);
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0u32), 1);

    // Advance another day — escrow is now empty.
    advance_time(&env, ONE_DAY);

    // Fail on second claim.
    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert!(
        result.is_err()
            || result
                .as_ref()
                .ok()
                .and_then(|r| r.as_ref().err())
                .is_some()
    );

    // Claimed periods still 1.
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0u32), 1);
}

// ============================================================================
// SECTION 12: STATE CONSISTENCY — AGREEMENT STATUS UNCHANGED AFTER FAILURE
// ============================================================================

/// A failed claim must not alter the agreement's status.
#[test]
fn test_agreement_status_unchanged_after_failed_claim() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_funded_payroll(
        &env,
        &contract_id,
        &client,
        &employer,
        &employee,
        &token,
        STANDARD_SALARY,
        0, // zero escrow
    );

    advance_time(&env, ONE_DAY + 1);

    let status_before = client.get_agreement(&agreement_id).unwrap().status;

    let _ = client.try_claim_payroll(&employee, &agreement_id, &0u32);

    let status_after = client.get_agreement(&agreement_id).unwrap().status;
    assert_eq!(status_before, status_after);
}

// ============================================================================
// SECTION 13: STATE CONSISTENCY — PAID AMOUNT UNCHANGED AFTER FAILURE
// ============================================================================

/// The paid_amount on the agreement must not change when a claim fails.
#[test]
fn test_paid_amount_unchanged_after_failed_claim() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_funded_payroll(
        &env,
        &contract_id,
        &client,
        &employer,
        &employee,
        &token,
        STANDARD_SALARY,
        0,
    );

    advance_time(&env, ONE_DAY + 1);

    let paid_before: i128 = env.as_contract(&contract_id, || {
        DataKey::get_agreement_paid_amount(&env, agreement_id)
    });

    let _ = client.try_claim_payroll(&employee, &agreement_id, &0u32);

    let paid_after: i128 = env.as_contract(&contract_id, || {
        DataKey::get_agreement_paid_amount(&env, agreement_id)
    });
    assert_eq!(paid_before, paid_after);
}

// ============================================================================
// SECTION 14: CLAIM ON NON-ACTIVATED AGREEMENT REJECTED
// ============================================================================

/// Claiming payroll on an agreement that was never activated must fail.
#[test]
fn test_payroll_claim_on_non_activated_agreement() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&agreement_id, &employee, &STANDARD_SALARY);

    // Do NOT activate.

    env.as_contract(&contract_id, || {
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_employee_count(&env, agreement_id, 1);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, STANDARD_SALARY);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        // No activation time set — claim should fail.
    });

    advance_time(&env, ONE_DAY + 1);

    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert!(
        result.is_err()
            || result
                .as_ref()
                .ok()
                .and_then(|r| r.as_ref().err())
                .is_some()
    );
}

/// Time-based claim on a non-activated escrow agreement must fail.
#[test]
fn test_time_based_claim_on_non_activated_agreement() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);

    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &STANDARD_SALARY,
        &ONE_DAY,
        &4u32,
    );

    // Do not activate.
    advance_time(&env, ONE_DAY + 1);

    let result = client.try_claim_time_based(&agreement_id);
    assert_eq!(result, Err(Ok(PayrollError::AgreementNotActivated)));
}

// ============================================================================
// SECTION 15: CLAIM ON PAUSED AGREEMENT REJECTED
// ============================================================================

/// Payroll claim on a paused agreement returns an error.
#[test]
fn test_payroll_claim_on_paused_agreement() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_funded_payroll(
        &env,
        &contract_id,
        &client,
        &employer,
        &employee,
        &token,
        STANDARD_SALARY,
        STANDARD_SALARY * 10,
    );

    client.pause_agreement(&agreement_id);
    advance_time(&env, ONE_DAY + 1);

    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert_eq!(result, Err(Ok(PayrollError::InvalidData)));
}

/// Time-based claim on a paused escrow returns AgreementPaused.
#[test]
fn test_time_based_claim_on_paused_agreement() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_funded_escrow(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        STANDARD_SALARY,
        ONE_DAY,
        4,
        true,
    );

    client.pause_agreement(&agreement_id);
    advance_time(&env, ONE_DAY + 1);

    let result = client.try_claim_time_based(&agreement_id);
    assert_eq!(result, Err(Ok(PayrollError::AgreementPaused)));
}

// ============================================================================
// SECTION 16: CLAIM WITH WRONG AGREEMENT MODE REJECTED
// ============================================================================

/// Calling claim_payroll on an Escrow-mode agreement must fail.
#[test]
fn test_payroll_claim_on_escrow_mode_agreement() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_funded_escrow(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        STANDARD_SALARY,
        ONE_DAY,
        4,
        true,
    );

    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, STANDARD_SALARY * 4);
        DataKey::set_employee_count(&env, agreement_id, 1);
        DataKey::set_employee(&env, agreement_id, 0, &contributor);
        DataKey::set_employee_salary(&env, agreement_id, 0, STANDARD_SALARY);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
    });

    advance_time(&env, ONE_DAY + 1);

    let result = client.try_claim_payroll(&contributor, &agreement_id, &0u32);
    assert_eq!(result, Err(Ok(PayrollError::InvalidAgreementMode)));
}

/// Calling claim_time_based on a Payroll-mode agreement must fail.
#[test]
fn test_time_based_claim_on_payroll_mode_agreement() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_funded_payroll(
        &env,
        &contract_id,
        &client,
        &employer,
        &employee,
        &token,
        STANDARD_SALARY,
        STANDARD_SALARY * 10,
    );

    advance_time(&env, ONE_DAY + 1);

    let result = client.try_claim_time_based(&agreement_id);
    assert_eq!(result, Err(Ok(PayrollError::InvalidAgreementMode)));
}

// ============================================================================
// SECTION 17: CLAIM BY UNAUTHORIZED CALLER REJECTED
// ============================================================================

/// A payroll claim by an address that is not the employee at the given index
/// must return Unauthorized.
#[test]
fn test_payroll_claim_unauthorized_caller() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let impostor = create_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_funded_payroll(
        &env,
        &contract_id,
        &client,
        &employer,
        &employee,
        &token,
        STANDARD_SALARY,
        STANDARD_SALARY * 10,
    );

    advance_time(&env, ONE_DAY + 1);

    let result = client.try_claim_payroll(&impostor, &agreement_id, &0u32);
    assert_eq!(result, Err(Ok(PayrollError::Unauthorized)));
}

// ============================================================================
// SECTION 18: CLAIM WITH INVALID EMPLOYEE INDEX REJECTED
// ============================================================================

/// Specifying an employee index that exceeds the employee count must return
/// InvalidEmployeeIndex.
#[test]
fn test_payroll_claim_invalid_employee_index() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_funded_payroll(
        &env,
        &contract_id,
        &client,
        &employer,
        &employee,
        &token,
        STANDARD_SALARY,
        STANDARD_SALARY * 10,
    );

    advance_time(&env, ONE_DAY + 1);

    let result = client.try_claim_payroll(&employee, &agreement_id, &99u32);
    assert_eq!(result, Err(Ok(PayrollError::InvalidEmployeeIndex)));
}

// ============================================================================
// SECTION 19: DOUBLE CLAIM FOR SAME PERIOD REJECTED
// ============================================================================

/// After claiming period 1, claiming again without advancing time must return
/// NoPeriodsToClaim.
#[test]
fn test_payroll_double_claim_same_period() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_funded_payroll(
        &env,
        &contract_id,
        &client,
        &employer,
        &employee,
        &token,
        STANDARD_SALARY,
        STANDARD_SALARY * 10,
    );

    advance_time(&env, ONE_DAY + 1);

    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert!(result.is_ok());

    // Claim again without advancing time.
    let result = client.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert_eq!(result, Err(Ok(PayrollError::NoPeriodsToClaim)));
}

/// Time-based double claim in the same period returns NoPeriodsToClaim.
#[test]
fn test_time_based_double_claim_same_period() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_funded_escrow(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        STANDARD_SALARY,
        ONE_DAY,
        4,
        true,
    );

    advance_time(&env, ONE_DAY + 1);

    let result = client.try_claim_time_based(&agreement_id);
    assert!(result.is_ok());

    // Second claim in the same interval.
    let result = client.try_claim_time_based(&agreement_id);
    assert_eq!(result, Err(Ok(PayrollError::NoPeriodsToClaim)));
}

// ============================================================================
// SECTION 20: ALL PERIODS CLAIMED — SUBSEQUENT CLAIM REJECTED
// ============================================================================

/// After all periods have been claimed, further claims return AllPeriodsClaimed.
#[test]
fn test_time_based_all_periods_claimed() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);

    let num_periods = 2u32;
    let agreement_id = setup_funded_escrow(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        STANDARD_SALARY,
        ONE_DAY,
        num_periods,
        true,
    );

    // Claim both periods.
    advance_time(&env, ONE_DAY * (num_periods as u64) + 1);
    let result = client.try_claim_time_based(&agreement_id);
    assert!(result.is_ok());
    assert_eq!(client.get_claimed_periods(&agreement_id), num_periods);

    // Advance more time and try again.
    advance_time(&env, ONE_DAY);
    let result = client.try_claim_time_based(&agreement_id);
    assert_eq!(result, Err(Ok(PayrollError::AllPeriodsClaimed)));
}

// ============================================================================
// SECTION 21: BATCH PAYROLL — MIXED SUCCESS AND FAILURE STATE CONSISTENCY
// ============================================================================

/// In a batch with 3 employees, the first two succeed and the third fails
/// (InsufficientEscrowBalance). The claimed periods of the two successful
/// employees are updated; the third is untouched.
#[test]
fn test_batch_payroll_mixed_state_consistency() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let e1 = create_address(&env);
    let e2 = create_address(&env);
    let e3 = create_address(&env);
    let token = create_token(&env);
    let salary = STANDARD_SALARY;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&agreement_id, &e1, &salary);
    client.add_employee_to_agreement(&agreement_id, &e2, &salary);
    client.add_employee_to_agreement(&agreement_id, &e3, &salary);
    client.activate_agreement(&agreement_id);

    // Fund for exactly 2 employees' 1 period.
    let escrow = salary * 2;
    mint(&env, &token, &contract_id, escrow);

    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, escrow);
        DataKey::set_employee_count(&env, agreement_id, 3);

        DataKey::set_employee(&env, agreement_id, 0, &e1);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);

        DataKey::set_employee(&env, agreement_id, 1, &e2);
        DataKey::set_employee_salary(&env, agreement_id, 1, salary);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 1, 0);

        DataKey::set_employee(&env, agreement_id, 2, &e3);
        DataKey::set_employee_salary(&env, agreement_id, 2, salary);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 2, 0);
    });

    advance_time(&env, ONE_DAY + 1);

    // Claims for all three as e1 (only index 0 matches caller).
    // e1 succeeds on index 0, fails on 1 (Unauthorized), fails on 2 (Unauthorized).
    // So use e1 for index 0, then separately use e2 for index 1.
    let batch_e1 = client.batch_claim_payroll(&e1, &agreement_id, &Vec::from_array(&env, [0u32]));
    assert_eq!(batch_e1.successful_claims, 1);
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0u32), 1);

    let batch_e2 = client.batch_claim_payroll(&e2, &agreement_id, &Vec::from_array(&env, [1u32]));
    assert_eq!(batch_e2.successful_claims, 1);
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &1u32), 1);

    // e3 tries to claim — insufficient escrow.
    let batch_e3 = client.batch_claim_payroll(&e3, &agreement_id, &Vec::from_array(&env, [2u32]));
    assert_eq!(batch_e3.successful_claims, 0);
    assert_eq!(batch_e3.failed_claims, 1);

    // e3 claimed periods unchanged.
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &2u32), 0);

    // Escrow balance should be zero after two successful claims.
    let remaining: i128 = env.as_contract(&contract_id, || {
        DataKey::get_agreement_escrow_balance(&env, agreement_id, &token)
    });
    assert_eq!(remaining, 0);
}

// ============================================================================
// SECTION 22: MILESTONE CLAIM ON PAUSED AGREEMENT REJECTED
// ============================================================================

/// Claiming an approved milestone while the agreement is paused must panic.
#[test]
#[should_panic(expected = "Cannot claim when agreement is paused")]
fn test_milestone_claim_on_paused_agreement() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1u32);
    mint(&env, &token, &client.address, 1000);

    // Pause the agreement.
    client.pause_agreement(&agreement_id);

    // Attempt claim — should panic.
    client.claim_milestone(&agreement_id, &1u32);
}

// ============================================================================
// SECTION 23: MILESTONE CLAIM WITHOUT APPROVAL REJECTED
// ============================================================================

/// Claiming a milestone that has not been approved must panic.
#[test]
#[should_panic(expected = "Milestone not approved")]
fn test_milestone_claim_without_approval() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    mint(&env, &token, &client.address, 1000);

    // Do NOT approve — claiming must fail.
    client.claim_milestone(&agreement_id, &1u32);
}

/// Claiming a milestone that was already claimed must panic.
#[test]
#[should_panic(expected = "Milestone already claimed")]
fn test_milestone_double_claim() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1u32);
    mint(&env, &token, &client.address, 2000);

    client.claim_milestone(&agreement_id, &1u32);

    // Second claim must fail.
    client.claim_milestone(&agreement_id, &1u32);
}

// ============================================================================
// SECTION 24: GRACE PERIOD EXPIRED — CLAIM REJECTED
// ============================================================================

/// After the grace period has fully elapsed, time-based claims must fail.
#[test]
fn test_claim_rejected_after_grace_period_expiry() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);

    let amount_per_period = STANDARD_SALARY;
    let period_seconds = ONE_DAY;
    let num_periods = 10u32;

    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &amount_per_period,
        &period_seconds,
        &num_periods,
    );

    let total = amount_per_period * (num_periods as i128);
    mint(&env, &token, &client.address, total);
    env.as_contract(&client.address, || {
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, total);
    });

    client.activate_agreement(&agreement_id);

    // Cancel immediately.
    client.cancel_agreement(&agreement_id);

    // Jump past the grace period.
    let grace_end = client.get_grace_period_end(&agreement_id).unwrap();
    env.ledger().with_mut(|li| {
        li.timestamp = grace_end + 1;
    });

    // Claim must fail — grace period is over.
    let result = client.try_claim_time_based(&agreement_id);
    assert!(result.is_err());
}
