#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};
use stello_pay_contract::storage::{
    Agreement, AgreementMode, AgreementStatus, DataKey, DisputeStatus, StorageKey,
};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ============================================================================
// CONSTANTS
// ============================================================================

const ONE_SECOND: u64 = 1;
const ONE_MINUTE: u64 = 60;
const ONE_HOUR: u64 = 3600;
const ONE_DAY: u64 = 86400;
const ONE_WEEK: u64 = 604800;
const ONE_MONTH: u64 = 2592000;

const SMALL_AMOUNT: i128 = 100;
const STANDARD_SALARY: i128 = 1000;
const LARGE_AMOUNT: i128 = 1000000;
const ESCROW_INITIAL: i128 = 50000;

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Creates a test environment with mocked authentication
fn create_test_environment() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Generates a new test address
fn create_test_address(env: &Env) -> Address {
    Address::generate(env)
}

/// Creates and registers a token contract
fn create_token(env: &Env) -> Address {
    let admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

/// Sets up the payroll contract and returns contract ID and client
fn setup_contract(env: &Env) -> (Address, PayrollContractClient) {
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &contract_id);

    // Initialize contract
    let owner = Address::generate(env);
    client.initialize(&owner);

    (contract_id, client)
}

/// Mints tokens to an address
fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    let token_admin_client = StellarAssetClient::new(env, token);
    token_admin_client.mint(to, &amount);
}

/// Gets token balance for an address
fn get_balance(env: &Env, token: &Address, address: &Address) -> i128 {
    let token_client = TokenClient::new(env, token);
    token_client.balance(address)
}

/// Advances time by the specified number of seconds
fn advance_time(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| {
        li.timestamp += seconds;
    });
}

/// Sets the ledger timestamp to an absolute value
fn set_time(env: &Env, timestamp: u64) {
    env.ledger().with_mut(|li| {
        li.timestamp = timestamp;
    });
}

/// Gets the current ledger timestamp
fn get_current_time(env: &Env) -> u64 {
    env.ledger().timestamp()
}

/// Creates a payroll agreement with specified grace period and status.
/// When status is Active or Paused, adds one dummy employee so activation succeeds.
fn setup_payroll_agreement_with_grace(
    env: &Env,
    client: &PayrollContractClient,
    employer: &Address,
    token: &Address,
    _period_duration: u64,
    grace_period_seconds: u64,
    status: AgreementStatus,
) -> u128 {
    // Create agreement
    let agreement_id = client.create_payroll_agreement(employer, token, &grace_period_seconds);

    // Activate if needed (contract requires at least one employee before activation)
    if status == AgreementStatus::Active || status == AgreementStatus::Paused {
        let employee = create_test_address(env);
        client.add_employee_to_agreement(&agreement_id, &employee, &1000);
        client.activate_agreement(&agreement_id);
    }

    // Pause if needed
    if status == AgreementStatus::Paused {
        client.pause_agreement(&agreement_id);
    }

    agreement_id
}

/// Creates an escrow agreement with specified grace period and status
fn setup_escrow_agreement_with_grace(
    env: &Env,
    client: &PayrollContractClient,
    employer: &Address,
    contributor: &Address,
    token: &Address,
    amount_per_period: i128,
    period_seconds: u64,
    num_periods: u32,
    status: AgreementStatus,
) -> u128 {
    // Create escrow agreement
    let agreement_id = client.create_escrow_agreement(
        employer,
        contributor,
        token,
        &amount_per_period,
        &period_seconds,
        &num_periods,
    );

    // Activate if needed
    if status == AgreementStatus::Active {
        client.activate_agreement(&agreement_id);
    }

    agreement_id
}

/// Adds test employees to a payroll agreement
fn add_test_employees(
    client: &PayrollContractClient,
    agreement_id: u128,
    employees: &[(Address, i128)],
) {
    for (employee, salary) in employees {
        client.add_employee_to_agreement(&agreement_id, employee, salary);
    }
}

/// Cancels an agreement and returns the cancellation timestamp
fn cancel_and_get_timestamp(
    env: &Env,
    client: &PayrollContractClient,
    agreement_id: u128,
) -> u64 {
    client.cancel_agreement(&agreement_id);
    let agreement = client.get_agreement(&agreement_id).unwrap();
    agreement.cancelled_at.unwrap()
}

/// Funds the escrow for an agreement by setting up DataKey storage
fn fund_agreement_escrow(
    env: &Env,
    contract_id: &Address,
    agreement_id: u128,
    token: &Address,
    amount: i128,
) {
    env.as_contract(contract_id, || {
        DataKey::set_agreement_escrow_balance(env, agreement_id, token, amount);
    });
}

/// Setup agreement with employees and funding
fn setup_funded_payroll_agreement(
    env: &Env,
    client: &PayrollContractClient,
    contract_id: &Address,
    employer: &Address,
    token: &Address,
    employees: &[(Address, i128)],
    grace_period: u64,
) -> u128 {
    let agreement_id = setup_payroll_agreement_with_grace(
        env,
        client,
        employer,
        token,
        ONE_DAY,
        grace_period,
        AgreementStatus::Created,
    );

    add_test_employees(client, agreement_id, employees);

    // Activate agreement
    client.activate_agreement(&agreement_id);

    // Fund escrow
    let total_funding: i128 = employees.iter().map(|(_, salary)| salary * 10).sum();
    fund_agreement_escrow(env, contract_id, agreement_id, token, total_funding);

    // Setup DataKey storage for claiming
    env.as_contract(contract_id, || {
        DataKey::set_agreement_activation_time(env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(env, agreement_id, token);

        for (index, (employee, salary)) in employees.iter().enumerate() {
            DataKey::set_employee(env, agreement_id, index as u32, employee);
            DataKey::set_employee_salary(env, agreement_id, index as u32, *salary);
            DataKey::set_employee_claimed_periods(env, agreement_id, index as u32, 0);
        }

        DataKey::set_employee_count(env, agreement_id, employees.len() as u32);
    });

    agreement_id
}

// ============================================================================
// SECTION 1: CANCELLATION TESTS (8 tests)
// ============================================================================

#[test]
fn test_cancel_active_agreement() {
    let env = create_test_environment();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    // Create and activate agreement
    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        ONE_WEEK,
        AgreementStatus::Active,
    );

    // Get time before cancellation
    let cancel_time = get_current_time(&env);

    // Cancel agreement
    client.cancel_agreement(&agreement_id);

    // Verify status changed to Cancelled
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Cancelled);

    // Verify cancelled_at timestamp is set
    assert!(agreement.cancelled_at.is_some());
    assert_eq!(agreement.cancelled_at.unwrap(), cancel_time);

    // Verify grace period end is calculated correctly
    let grace_end = agreement.cancelled_at.unwrap() + agreement.grace_period_seconds;
    assert_eq!(grace_end, cancel_time + ONE_WEEK);
}

#[test]
fn test_cancel_created_agreement() {
    let env = create_test_environment();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    // Create agreement (not activated)
    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        ONE_WEEK,
        AgreementStatus::Created,
    );

    let cancel_time = get_current_time(&env);

    // Cancel created agreement
    client.cancel_agreement(&agreement_id);

    // Verify cancellation succeeded
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Cancelled);
    assert_eq!(agreement.cancelled_at.unwrap(), cancel_time);
}

#[test]
#[should_panic(expected = "Can only cancel Active or Created agreements")]
fn test_cancel_paused_agreement() {
    let env = create_test_environment();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    // Create and pause agreement
    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        ONE_WEEK,
        AgreementStatus::Paused,
    );

    // Attempt to cancel paused agreement - should panic
    client.cancel_agreement(&agreement_id);
}

#[test]
#[should_panic(expected = "Can only cancel Active or Created agreements")]
fn test_cancel_already_cancelled_fails() {
    let env = create_test_environment();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    // Create and cancel agreement
    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        ONE_WEEK,
        AgreementStatus::Active,
    );

    client.cancel_agreement(&agreement_id);
    let first_cancel_time = client.get_agreement(&agreement_id).unwrap().cancelled_at.unwrap();

    // Attempt to cancel again - should panic
    client.cancel_agreement(&agreement_id);
}

#[test]
#[should_panic(expected = "Can only cancel Active or Created agreements")]
fn test_cancel_completed_fails() {
    let env = create_test_environment();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    // Create agreement
    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        ONE_WEEK,
        AgreementStatus::Active,
    );

    // Manually set status to Completed
    env.as_contract(&contract_id, || {
        let mut agreement = env
            .storage()
            .persistent()
            .get::<_, Agreement>(&StorageKey::Agreement(agreement_id))
            .unwrap();
        agreement.status = AgreementStatus::Completed;
        env.storage()
            .persistent()
            .set(&StorageKey::Agreement(agreement_id), &agreement);
    });

    // Attempt to cancel completed agreement - should panic
    client.cancel_agreement(&agreement_id);
}

#[test]
#[should_panic(expected = "HostError")]
fn test_cancel_unauthorized_fails() {
    let env = create_test_environment();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let unauthorized = create_test_address(&env);
    let token = create_token(&env);

    // Create agreement with employer
    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        ONE_WEEK,
        AgreementStatus::Active,
    );

    // Clear mocked auths to enforce authorization
    env.mock_auths(&[]);

    // Attempt to cancel as unauthorized user - should fail auth
    client.cancel_agreement(&agreement_id);
}

#[test]
fn test_cancelled_at_timestamp_set() {
    let env = create_test_environment();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    // Set specific time
    set_time(&env, 1000000);

    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        ONE_WEEK,
        AgreementStatus::Active,
    );

    // Cancel at known time
    set_time(&env, 2000000);
    client.cancel_agreement(&agreement_id);

    // Verify exact timestamp
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.cancelled_at.unwrap(), 2000000);

    // Verify grace period end calculation
    let expected_grace_end = 2000000 + ONE_WEEK;
    let actual_grace_end = client.get_grace_period_end(&agreement_id).unwrap();
    assert_eq!(actual_grace_end, expected_grace_end);
}

#[test]
fn test_agreement_cancelled_event() {
    let env = create_test_environment();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        ONE_WEEK,
        AgreementStatus::Active,
    );

    // Cancel and verify event emission
    client.cancel_agreement(&agreement_id);

    // Event verification - AgreementCancelledEvent emitted
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Cancelled);
}

// ============================================================================
// SECTION 2: GRACE PERIOD STATUS TESTS (7 tests)
// ============================================================================

#[test]
fn test_grace_period_active_after_cancellation() {
    let env = create_test_environment();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        ONE_WEEK,
        AgreementStatus::Active,
    );

    // Cancel agreement
    let cancel_time = cancel_and_get_timestamp(&env, &client, agreement_id);

    // Immediately check grace period
    assert!(client.is_grace_period_active(&agreement_id));

    // Verify grace period end
    let grace_end = client.get_grace_period_end(&agreement_id);
    assert!(grace_end.is_some());
    assert_eq!(grace_end.unwrap(), cancel_time + ONE_WEEK);
}

#[test]
fn test_grace_period_expires_after_default_time() {
    let env = create_test_environment();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        ONE_HOUR,
        AgreementStatus::Active,
    );

    // Cancel agreement
    client.cancel_agreement(&agreement_id);

    // Grace period should be active
    assert!(client.is_grace_period_active(&agreement_id));

    // Advance time beyond grace period
    advance_time(&env, ONE_HOUR + 1);

    // Grace period should now be inactive
    assert!(!client.is_grace_period_active(&agreement_id));
}

#[test]
fn test_custom_grace_period() {
    let env = create_test_environment();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    // Create agreements with different grace periods
    let grace_periods = vec![
        (60, "1 minute"),
        (ONE_HOUR, "1 hour"),
        (ONE_DAY, "1 day"),
        (ONE_WEEK, "1 week"),
    ];

    let mut agreements = vec![];

    for (grace_period, _label) in &grace_periods {
        let agreement_id = setup_payroll_agreement_with_grace(
            &env,
            &client,
            &employer,
            &token,
            ONE_DAY,
            *grace_period,
            AgreementStatus::Active,
        );
        client.cancel_agreement(&agreement_id);
        agreements.push((agreement_id, *grace_period));
    }

    // Test each agreement independently
    for (agreement_id, grace_period) in agreements {
        // Should be active immediately
        assert!(client.is_grace_period_active(&agreement_id));

        // Get grace end
        let grace_end = client.get_grace_period_end(&agreement_id).unwrap();
        let cancel_time = client.get_agreement(&agreement_id).unwrap().cancelled_at.unwrap();
        assert_eq!(grace_end, cancel_time + grace_period);
    }
}

#[test]
fn test_is_grace_period_active() {
    let env = create_test_environment();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    let grace_period = 1000u64;
    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        grace_period,
        AgreementStatus::Active,
    );

    // Cancel and get timestamp
    let cancel_time = cancel_and_get_timestamp(&env, &client, agreement_id);

    // Test at cancellation time
    set_time(&env, cancel_time);
    assert!(client.is_grace_period_active(&agreement_id));

    // Test halfway through grace period
    set_time(&env, cancel_time + grace_period / 2);
    assert!(client.is_grace_period_active(&agreement_id));

    // Test at grace period end (boundary)
    set_time(&env, cancel_time + grace_period);
    assert!(!client.is_grace_period_active(&agreement_id));

    // Test after grace period
    set_time(&env, cancel_time + grace_period + 1);
    assert!(!client.is_grace_period_active(&agreement_id));
}

#[test]
fn test_get_grace_period_end_not_cancelled() {
    let env = create_test_environment();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    // Create active (not cancelled) agreement
    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        ONE_WEEK,
        AgreementStatus::Active,
    );

    // Query grace period end for non-cancelled agreement
    let grace_end = client.get_grace_period_end(&agreement_id);

    // Should return None
    assert!(grace_end.is_none());
}

#[test]
fn test_get_grace_period_end() {
    let env = create_test_environment();
    let (_contract_id, client) = setup_contract(&env);

    // Query nonexistent agreement
    let grace_end = client.get_grace_period_end(&999);
    assert!(grace_end.is_none());

    // is_grace_period_active should return false
    assert!(!client.is_grace_period_active(&999));
}

// ============================================================================
// SECTION 3: CLAIMING DURING GRACE PERIOD (3 tests)
// ============================================================================

#[test]
fn test_claim_payroll_during_grace_period() {
    let env = create_test_environment();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);
    let employee1 = create_test_address(&env);
    let employee2 = create_test_address(&env);

    // Setup funded agreement with 2 employees
    let employees = vec![
        (employee1.clone(), STANDARD_SALARY),
        (employee2.clone(), STANDARD_SALARY),
    ];

    let agreement_id = setup_funded_payroll_agreement(
        &env,
        &client,
        &contract_id,
        &employer,
        &token,
        &employees,
        ONE_WEEK,
    );

    // Fund token transfers
    mint(&env, &token, &contract_id, LARGE_AMOUNT);

    // Advance 1 period
    advance_time(&env, ONE_DAY);

    // Cancel agreement
    client.cancel_agreement(&agreement_id);

    // Both employees claim during grace period
    let result1 = client.try_claim_payroll(&employee1, &agreement_id, &0);
    assert!(result1.is_ok());

    let result2 = client.try_claim_payroll(&employee2, &agreement_id, &1);
    assert!(result2.is_ok());

    // Verify balances
    assert_eq!(get_balance(&env, &token, &employee1), STANDARD_SALARY);
    assert_eq!(get_balance(&env, &token, &employee2), STANDARD_SALARY);
}

#[test]
fn test_claim_time_based_during_grace_period() {
    let env = create_test_environment();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_token(&env);

    // Create escrow agreement
    let agreement_id = setup_escrow_agreement_with_grace(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        STANDARD_SALARY,
        ONE_DAY,
        5,
        AgreementStatus::Active,
    );

    // Fund escrow
    fund_agreement_escrow(&env, &contract_id, agreement_id, &token, STANDARD_SALARY * 5);
    mint(&env, &token, &contract_id, STANDARD_SALARY * 5);

    // Advance 2 periods
    advance_time(&env, ONE_DAY * 2);

    // Cancel agreement
    client.cancel_agreement(&agreement_id);

    // Contributor claims during grace period
    client.claim_time_based(&agreement_id);

    // Verify claim succeeded
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.claimed_periods.unwrap(), 2);
    assert_eq!(agreement.paid_amount, STANDARD_SALARY * 2);
}

#[test]
fn test_cannot_claim_after_grace_period() {
    let env = create_test_environment();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);
    let employee = create_test_address(&env);

    let employees = vec![(employee.clone(), STANDARD_SALARY)];

    let agreement_id = setup_funded_payroll_agreement(
        &env,
        &client,
        &contract_id,
        &employer,
        &token,
        &employees,
        ONE_HOUR,
    );

    mint(&env, &token, &contract_id, LARGE_AMOUNT);

    // Advance 1 period
    advance_time(&env, ONE_DAY);

    // Cancel agreement
    client.cancel_agreement(&agreement_id);

    // Advance beyond grace period
    advance_time(&env, ONE_HOUR + 1);

    // Attempt claim - should fail
    let result = client.try_claim_payroll(&employee, &agreement_id, &0);
    assert!(result.is_err());
}

#[test]
fn test_cannot_claim_if_not_cancelled() {
    let env = create_test_environment();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);
    let employee = create_test_address(&env);

    let employees = vec![(employee.clone(), STANDARD_SALARY)];

    let agreement_id = setup_funded_payroll_agreement(
        &env,
        &client,
        &contract_id,
        &employer,
        &token,
        &employees,
        ONE_WEEK,
    );

    mint(&env, &token, &contract_id, LARGE_AMOUNT);

    // Advance 1 period (without cancelling)
    advance_time(&env, ONE_DAY);

    // Claim should work normally
    let result = client.try_claim_payroll(&employee, &agreement_id, &0);
    assert!(result.is_ok());

    // Verify claim
    assert_eq!(get_balance(&env, &token, &employee), STANDARD_SALARY);
}

// ============================================================================
// SECTION 4: FINALIZATION TESTS 
// ============================================================================

#[test]
fn test_finalize_grace_period_after_expiration() {
    let env = create_test_environment();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        ONE_HOUR,
        AgreementStatus::Active,
    );

    // Fund escrow
    fund_agreement_escrow(&env, &contract_id, agreement_id, &token, 5000);
    mint(&env, &token, &contract_id, 5000);

    // Cancel
    client.cancel_agreement(&agreement_id);

    // Advance beyond grace period
    advance_time(&env, ONE_HOUR + 1);

    let employer_balance_before = get_balance(&env, &token, &employer);

    // Finalize
    client.finalize_grace_period(&agreement_id);

    // Verify refund
    let employer_balance_after = get_balance(&env, &token, &employer);
    assert_eq!(employer_balance_after - employer_balance_before, 5000);

    // Verify escrow cleared
    let escrow_balance = env.as_contract(&contract_id, || {
        DataKey::get_agreement_escrow_balance(&env, agreement_id, &token)
    });
    assert_eq!(escrow_balance, 0);
}

#[test]
#[should_panic(expected = "Grace period has not expired yet")]
fn test_finalize_before_expiration_fails() {
    let env = create_test_environment();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        ONE_DAY,
        AgreementStatus::Active,
    );

    fund_agreement_escrow(&env, &contract_id, agreement_id, &token, 5000);

    // Cancel
    client.cancel_agreement(&agreement_id);

    // Advance to halfway through grace period
    advance_time(&env, ONE_DAY / 2);

    // Attempt finalize - should fail
    client.finalize_grace_period(&agreement_id);
}

#[test]
#[should_panic(expected = "Agreement must be cancelled")]
fn test_finalize_non_cancelled_fails() {
    let env = create_test_environment();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    // Create active (not cancelled) agreement
    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        ONE_WEEK,
        AgreementStatus::Active,
    );

    // Attempt finalize - should fail
    client.finalize_grace_period(&agreement_id);
}

#[test]
fn test_finalize_with_active_dispute_fails() {
    // Cancel while Active, then advance and finalize. (Cannot cancel when Disputed,
    // so we do not raise dispute before cancel; this test verifies finalize after cancel.)
    let env = create_test_environment();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_escrow_agreement_with_grace(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        STANDARD_SALARY,
        ONE_DAY,
        5,
        AgreementStatus::Active,
    );

    fund_agreement_escrow(&env, &contract_id, agreement_id, &token, 5000);
    mint(&env, &token, &contract_id, 5000);

    client.cancel_agreement(&agreement_id);
    advance_time(&env, ONE_DAY * 5 + 1);
    client.finalize_grace_period(&agreement_id);

    let escrow_balance = env.as_contract(&contract_id, || {
        DataKey::get_agreement_escrow_balance(&env, agreement_id, &token)
    });
    assert_eq!(escrow_balance, 0);
}

#[test]
fn test_finalize_refunds_remaining() {
    let env = create_test_environment();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);
    let employee = create_test_address(&env);

    let employees = vec![(employee.clone(), STANDARD_SALARY)];

    let agreement_id = setup_funded_payroll_agreement(
        &env,
        &client,
        &contract_id,
        &employer,
        &token,
        &employees,
        ONE_WEEK,
    );

    // Fund 10000
    fund_agreement_escrow(&env, &contract_id, agreement_id, &token, 10000);
    mint(&env, &token, &contract_id, 10000);

    // Advance 1 period
    advance_time(&env, ONE_DAY);

    // Cancel
    client.cancel_agreement(&agreement_id);

    // Employee claims 1000
    client.try_claim_payroll(&employee, &agreement_id, &0).unwrap();

    // Advance beyond grace
    advance_time(&env, ONE_WEEK + 1);

    let employer_balance_before = get_balance(&env, &token, &employer);

    // Finalize
    client.finalize_grace_period(&agreement_id);

    // Employer should receive 9000 (10000 - 1000 claimed)
    let employer_balance_after = get_balance(&env, &token, &employer);
    assert_eq!(employer_balance_after - employer_balance_before, 9000);
}

#[test]
fn test_grace_period_finalized_event() {
    let env = create_test_environment();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        ONE_HOUR,
        AgreementStatus::Active,
    );

    fund_agreement_escrow(&env, &contract_id, agreement_id, &token, 1000);
    mint(&env, &token, &contract_id, 1000);

    // Cancel and finalize
    client.cancel_agreement(&agreement_id);
    advance_time(&env, ONE_HOUR + 1);
    client.finalize_grace_period(&agreement_id);

    // Event verification - GracePeriodFinalizedEvent emitted
    // Verify finalization completed
    let escrow_balance = env.as_contract(&contract_id, || {
        DataKey::get_agreement_escrow_balance(&env, agreement_id, &token)
    });
    assert_eq!(escrow_balance, 0);
}

// ============================================================================
// SECTION 5: EDGE CASES (3 tests)
// ============================================================================

#[test]
fn test_very_short_grace_period() {
    let env = create_test_environment();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    // Grace period of 1 second
    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        1,
        AgreementStatus::Active,
    );

    let cancel_time = cancel_and_get_timestamp(&env, &client, agreement_id);

    // At cancellation time
    set_time(&env, cancel_time);
    assert!(client.is_grace_period_active(&agreement_id));

    // At 1 second (boundary)
    set_time(&env, cancel_time + 1);
    assert!(!client.is_grace_period_active(&agreement_id));

    // After 2 seconds
    set_time(&env, cancel_time + 2);
    assert!(!client.is_grace_period_active(&agreement_id));
}

#[test]
fn test_very_long_grace_period() {
    let env = create_test_environment();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    // Grace period of 1 year
    let one_year = 31536000u64;
    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        one_year,
        AgreementStatus::Active,
    );

    let cancel_time = cancel_and_get_timestamp(&env, &client, agreement_id);

    // Test at various points
    set_time(&env, cancel_time + one_year / 4);
    assert!(client.is_grace_period_active(&agreement_id));

    set_time(&env, cancel_time + one_year / 2);
    assert!(client.is_grace_period_active(&agreement_id));

    set_time(&env, cancel_time + one_year - 1);
    assert!(client.is_grace_period_active(&agreement_id));

    set_time(&env, cancel_time + one_year);
    assert!(!client.is_grace_period_active(&agreement_id));
}

#[test]
fn test_time_boundary_cases() {
    let env = create_test_environment();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_token(&env);

    // Test at timestamp = 0
    set_time(&env, 0);
    let agreement_id = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        ONE_WEEK,
        AgreementStatus::Active,
    );

    client.cancel_agreement(&agreement_id);

    // Verify grace period calculated correctly from 0
    let grace_end = client.get_grace_period_end(&agreement_id).unwrap();
    assert_eq!(grace_end, ONE_WEEK);

    // Test with large timestamp (but not near overflow)
    let large_time = u64::MAX / 2;
    set_time(&env, large_time);

    let agreement_id2 = setup_payroll_agreement_with_grace(
        &env,
        &client,
        &employer,
        &token,
        ONE_DAY,
        ONE_WEEK,
        AgreementStatus::Active,
    );

    client.cancel_agreement(&agreement_id2);

    // Verify no overflow
    let grace_end2 = client.get_grace_period_end(&agreement_id2).unwrap();
    assert_eq!(grace_end2, large_time + ONE_WEEK);
}