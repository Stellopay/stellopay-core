//! Comprehensive test suite for boundary conditions (#197).
//!
//! Validates contract behavior at the edges of input domains:
//! agreement IDs, payment amounts, employee counts, and time parameters.
//!
//! Each test includes NatSpec-style documentation describing the boundary
//! condition under examination and the expected outcome.

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{testutils::Address as _, Address, Env};
use stello_pay_contract::storage::{AgreementMode, AgreementStatus, DisputeStatus};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ============================================================================
// HELPERS
// ============================================================================

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn create_test_address(env: &Env) -> Address {
    Address::generate(env)
}

fn setup_contract(env: &Env) -> (Address, PayrollContractClient<'static>) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = create_test_address(env);
    client.initialize(&owner);
    (contract_id, client)
}

/// Deploys a Stellar Asset Contract and returns its address.
///
/// Uses the Soroban SDK's built-in SAC registration for test environments.
/// The returned address can be used as a token in agreement creation and
/// with `StellarAssetClient` for minting in tests that require funded escrows.
fn create_token_contract(env: &Env, admin: &Address) -> Address {
    env.register_stellar_asset_contract_v2(admin.clone())
        .address()
}

// ============================================================================
// SECTION 1: AGREEMENT ID BOUNDARY CONDITIONS
// ============================================================================

/// Verifies that querying a non-existent agreement with ID 0 returns None.
///
/// Agreement IDs are auto-incremented starting at 1, so ID 0 should never
/// correspond to a valid agreement. The contract must handle this gracefully
/// instead of panicking.
#[test]
fn test_get_agreement_with_zero_id_returns_none() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);

    let result = client.get_agreement(&0u128);
    assert!(result.is_none());
}

/// Verifies that querying a non-existent agreement at the maximum u128 value
/// returns None.
///
/// ID u128::MAX is astronomically unlikely to exist. The contract must handle
/// this lookup gracefully without overflow or storage errors.
#[test]
fn test_get_agreement_with_max_u128_id_returns_none() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);

    let result = client.get_agreement(&u128::MAX);
    assert!(result.is_none());
}

/// Verifies that operations on a non-existent agreement ID (e.g., activate,
/// pause, cancel) fail with an appropriate error.
///
/// Passing an ID that was never created to `activate_agreement` must panic
/// or return an error — not silently succeed.
#[test]
#[should_panic(expected = "Agreement not found")]
fn test_activate_nonexistent_agreement_panics() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);

    client.activate_agreement(&999_999u128);
}

/// Verifies that querying employees for a non-existent agreement ID returns
/// an empty vector rather than panicking.
///
/// `get_agreement_employees` should be safe to call with any agreement ID.
#[test]
fn test_get_employees_for_nonexistent_agreement_returns_empty() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);

    let employees = client.get_agreement_employees(&999_999u128);
    assert_eq!(employees.len(), 0);
}

/// Verifies that sequential agreement creation produces strictly increasing IDs.
///
/// After creating N agreements, each ID should be exactly 1 greater than the
/// previous, confirming the counter has no off-by-one errors at the boundary
/// of the first few allocations.
#[test]
fn test_sequential_agreement_ids_are_strictly_increasing() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);

    let id1 = client.create_payroll_agreement(&employer, &token, &604800u64);
    let id2 = client.create_payroll_agreement(&employer, &token, &604800u64);
    let id3 = client.create_payroll_agreement(&employer, &token, &604800u64);

    assert_eq!(id2, id1 + 1);
    assert_eq!(id3, id2 + 1);
    assert!(id1 >= 1);
}

// ============================================================================
// SECTION 2: PAYMENT AMOUNT BOUNDARY CONDITIONS
// ============================================================================

// ---------- Escrow Agreement Amounts ----------

/// Verifies that creating an escrow agreement with zero amount_per_period
/// returns `Err(PayrollError::ZeroAmountPerPeriod)`.
///
/// Zero-value payments have no economic meaning and must be rejected
/// at creation time to prevent nonsensical agreements.
#[test]
#[should_panic]
fn test_escrow_zero_amount_per_period_rejected() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    // amount_per_period = 0 triggers ZeroAmountPerPeriod validation
    let _ = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &0i128,
        &86400u64,
        &4u32,
    );
}

/// Verifies that creating an escrow agreement with a negative amount_per_period
/// returns `Err(PayrollError::ZeroAmountPerPeriod)`.
///
/// Negative payment amounts could lead to reverse fund flows.
/// The guard `amount_per_period <= 0` must catch this.
#[test]
#[should_panic]
fn test_escrow_negative_amount_per_period_rejected() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    let _ = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &(-100i128),
        &86400u64,
        &4u32,
    );
}

/// Verifies that creating an escrow agreement with `i128::MIN` as
/// amount_per_period is rejected.
///
/// The most extreme negative value must be caught by the same validation
/// that rejects zero, ensuring no underflow or sign-confusion occurs.
#[test]
#[should_panic]
fn test_escrow_i128_min_amount_per_period_rejected() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    let _ = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &i128::MIN,
        &86400u64,
        &4u32,
    );
}

/// Verifies that creating an escrow agreement with the minimum valid amount
/// (1 unit) succeeds.
///
/// amount_per_period = 1 is the smallest positive value. The contract must
/// accept it and compute `total_amount = 1 * num_periods` correctly.
#[test]
fn test_escrow_minimum_valid_amount_succeeds() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &1i128,
        &86400u64,
        &4u32,
    );

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.total_amount, 4); // 1 * 4
    assert_eq!(agreement.mode, AgreementMode::Escrow);
    assert_eq!(agreement.status, AgreementStatus::Created);
}

/// Verifies behavior when amount_per_period is set to i128::MAX.
///
/// With num_periods > 1, the multiplication `i128::MAX * num_periods`
/// would overflow i128. The contract should either reject this at creation
/// or handle the overflow gracefully.
#[test]
#[should_panic]
fn test_escrow_i128_max_amount_per_period_overflow() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    // i128::MAX * 2 overflows i128 during total_amount computation
    let _ = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &i128::MAX,
        &86400u64,
        &2u32,
    );
}

/// Verifies that creating an escrow agreement with zero period_seconds
/// returns `Err(PayrollError::ZeroPeriodDuration)`.
///
/// A zero-duration period is undefined and would cause division-by-zero
/// during claim calculations.
#[test]
#[should_panic]
fn test_escrow_zero_period_seconds_rejected() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    let _ = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &100i128,
        &0u64,
        &4u32,
    );
}

/// Verifies that creating an escrow agreement with zero num_periods
/// returns `Err(PayrollError::ZeroNumPeriods)`.
///
/// An agreement with zero periods has no payment schedule and must be
/// rejected at creation time.
#[test]
#[should_panic]
fn test_escrow_zero_num_periods_rejected() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    let _ = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &100i128,
        &86400u64,
        &0u32,
    );
}

/// Verifies that creating an escrow agreement with u32::MAX num_periods
/// succeeds (or fails gracefully due to overflow in total_amount).
///
/// `total_amount = amount_per_period * (u32::MAX as i128)` may overflow
/// if amount_per_period is large. With amount_per_period = 1, the result
/// fits in i128 and should succeed.
#[test]
fn test_escrow_max_u32_num_periods() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    // total_amount = 1 * u32::MAX = 4_294_967_295 — fits comfortably in i128
    // grace_period_seconds = 1 * u32::MAX = 4_294_967_295 — fits in u64
    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &1i128,
        &1u64,
        &u32::MAX,
    );

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.total_amount, u32::MAX as i128);
    assert_eq!(agreement.num_periods, Some(u32::MAX));
}

/// Verifies that creating an escrow agreement with u64::MAX period_seconds
/// succeeds.
///
/// While impractical (billions of years), the contract should not reject
/// a technically valid u64 value. The grace_period_seconds computation
/// `period_seconds * num_periods` may overflow u64 though.
#[test]
fn test_escrow_max_u64_period_seconds() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    // With num_periods = 1: grace_period = u64::MAX * 1 = u64::MAX (no overflow)
    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &1i128,
        &u64::MAX,
        &1u32,
    );

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.period_seconds, Some(u64::MAX));
    assert_eq!(agreement.total_amount, 1);
}

// ---------- Payroll Employee Salary Amounts ----------

/// Verifies that adding an employee with zero salary panics.
///
/// The contract asserts `salary_per_period > 0`. A zero salary is
/// economically meaningless and must be rejected.
#[test]
#[should_panic(expected = "Salary must be positive")]
fn test_add_employee_zero_salary_panics() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let employee = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &employee, &0i128);
}

/// Verifies that adding an employee with negative salary panics.
///
/// Negative salaries could invert payment direction. The `> 0` guard
/// must reject all non-positive values.
#[test]
#[should_panic(expected = "Salary must be positive")]
fn test_add_employee_negative_salary_panics() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let employee = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &employee, &(-500i128));
}

/// Verifies that adding an employee with `i128::MIN` salary panics.
///
/// The most extreme negative value tests for underflow in the salary
/// validation path.
#[test]
#[should_panic(expected = "Salary must be positive")]
fn test_add_employee_i128_min_salary_panics() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let employee = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &employee, &i128::MIN);
}

/// Verifies that adding an employee with the minimum valid salary (1) succeeds.
///
/// salary_per_period = 1 is the smallest acceptable value. The employee
/// should be added and `total_amount` incremented by 1.
#[test]
fn test_add_employee_minimum_valid_salary_succeeds() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let employee = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &employee, &1i128);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.total_amount, 1);

    let employees = client.get_agreement_employees(&agreement_id);
    assert_eq!(employees.len(), 1);
}

/// Verifies that adding an employee with `i128::MAX` salary succeeds at
/// the add step, but tests that total_amount accumulation doesn't overflow.
///
/// Adding a first employee with i128::MAX succeeds (total_amount = i128::MAX).
/// Adding a second employee with salary 1 causes `total_amount` to overflow,
/// which panics in debug mode.
#[test]
#[should_panic]
fn test_add_employee_i128_max_salary_overflow_risk() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);

    // First add succeeds: total_amount = 0 + i128::MAX = i128::MAX
    let emp1 = create_test_address(&env);
    client.add_employee_to_agreement(&agreement_id, &emp1, &i128::MAX);

    // Second add overflows: total_amount = i128::MAX + 1 → panic
    let emp2 = create_test_address(&env);
    client.add_employee_to_agreement(&agreement_id, &emp2, &1i128);
}

// ---------- Milestone Amounts ----------

/// Verifies that adding a milestone with zero amount panics.
///
/// The contract asserts `amount > 0`. A zero-value milestone has no
/// economic purpose.
#[test]
#[should_panic(expected = "Amount must be positive")]
fn test_add_milestone_zero_amount_panics() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &0i128);
}

/// Verifies that adding a milestone with negative amount panics.
///
/// Negative milestone amounts could corrupt `total_amount` tracking.
#[test]
#[should_panic(expected = "Amount must be positive")]
fn test_add_milestone_negative_amount_panics() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &(-500i128));
}

/// Verifies that adding a milestone with the minimum valid amount (1) succeeds.
///
/// amount = 1 is the boundary between accepted and rejected. The milestone
/// should be created and the agreement's `total_amount` updated.
#[test]
fn test_add_milestone_minimum_valid_amount_succeeds() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1i128);

    assert_eq!(client.get_milestone_count(&agreement_id), 1);
    let milestone = client.get_milestone(&agreement_id, &1u32).unwrap();
    assert_eq!(milestone.amount, 1);
    assert!(!milestone.approved);
    assert!(!milestone.claimed);
}

/// Verifies behavior when adding a milestone with `i128::MAX` amount.
///
/// A single milestone at i128::MAX should succeed, but adding a second
/// milestone with amount 1 causes `total_amount` overflow in the
/// milestone counter's accumulation (`total + amount`).
#[test]
#[should_panic]
fn test_add_milestone_i128_max_amount_overflow_risk() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    // First milestone: total = 0 + i128::MAX = i128::MAX (OK)
    client.add_milestone(&agreement_id, &i128::MAX);

    // Second milestone: total = i128::MAX + 1 → overflow panic
    client.add_milestone(&agreement_id, &1i128);
}

// ---------- Dispute Resolution Amounts ----------

/// Verifies that resolving a dispute where `pay_employee + refund_employer`
/// exceeds `total_amount` returns `Err(PayrollError::InvalidPayout)`.
///
/// The contract checks `pay_employee + refund_employer > total_locked`.
/// This prevents distributing more funds than the agreement holds.
#[test]
fn test_resolve_dispute_payout_exceeds_total_rejected() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let arbiter = create_test_address(&env);
    let token = create_test_address(&env);

    // Set arbiter
    client.set_arbiter(&employer, &arbiter);

    // Create escrow: total_amount = 100 * 1 = 100
    // grace_period_seconds = 3600 * 1 = 3600 (dispute must be raised within this)
    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &100i128,
        &3600u64,
        &1u32,
    );

    // Raise dispute (within grace period since timestamp starts at 0)
    client.raise_dispute(&employer, &agreement_id);

    // Attempt resolve with payout sum (60 + 50 = 110) exceeding total (100)
    let result = client.try_resolve_dispute(&arbiter, &agreement_id, &60i128, &50i128);
    assert!(result.is_err());
}

/// Verifies that resolving a dispute with zero payouts (both amounts = 0)
/// succeeds.
///
/// While unusual, a zero-payout resolution is valid — it simply resolves
/// the dispute without transferring funds.
#[test]
fn test_resolve_dispute_zero_payouts_succeeds() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let arbiter = create_test_address(&env);
    let token = create_test_address(&env);

    // Set arbiter
    client.set_arbiter(&employer, &arbiter);

    // Create escrow: total_amount = 100
    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &100i128,
        &3600u64,
        &1u32,
    );

    // Raise dispute
    client.raise_dispute(&employer, &agreement_id);

    // Resolve with zero payouts — no token transfers occur
    client.resolve_dispute(&arbiter, &agreement_id, &0i128, &0i128);

    // Verify dispute is resolved and agreement completed
    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Resolved
    );
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Completed);
}

// ============================================================================
// SECTION 3: EMPLOYEE COUNT BOUNDARY CONDITIONS
// ============================================================================

/// Verifies that a payroll agreement cannot be activated with zero employees.
///
/// The contract asserts `employees.len() > 0` during activation. An
/// agreement with no employees is not operational.
#[test]
#[should_panic(expected = "Payroll agreement must have at least one employee to activate")]
fn test_activate_payroll_with_zero_employees_panics() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    // Activate without adding any employees → panics
    client.activate_agreement(&agreement_id);
}

/// Verifies that a payroll agreement with exactly one employee can be
/// activated successfully.
///
/// This is the minimum viable employee count. The agreement should
/// transition from Created -> Active.
#[test]
fn test_activate_payroll_with_one_employee_succeeds() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let employee = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000i128);
    client.activate_agreement(&agreement_id);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Active);
    assert!(agreement.activated_at.is_some());
}

/// Verifies that multiple employees can be added and the agreement
/// tracks each one correctly.
///
/// Adds exactly 10 employees in a loop and verifies the employee list
/// length and total_amount accumulation match expectations.
#[test]
fn test_add_multiple_employees_tracked_correctly() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);

    let mut expected_total: i128 = 0;
    for i in 0u32..10 {
        let employee = create_test_address(&env);
        let salary = (i as i128 + 1) * 100; // 100, 200, ..., 1000
        client.add_employee_to_agreement(&agreement_id, &employee, &salary);
        expected_total += salary;
    }

    let employees = client.get_agreement_employees(&agreement_id);
    assert_eq!(employees.len(), 10);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.total_amount, expected_total); // 5500
}

/// Verifies that claiming payroll with an invalid employee_index
/// (u32::MAX) returns `Err(PayrollError::InvalidEmployeeIndex)`.
///
/// u32::MAX is far beyond any realistic employee list. The index
/// bounds check must catch this before any storage access.
#[test]
fn test_claim_payroll_invalid_employee_index_max_u32() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let caller = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);

    // DataKey employee count defaults to 0; u32::MAX >= 0 → InvalidEmployeeIndex
    let result = client.try_claim_payroll(&caller, &agreement_id, &u32::MAX);
    assert!(result.is_err());
}

/// Verifies that claiming payroll with employee_index = 0 for an
/// agreement with zero employees returns an appropriate error.
///
/// Even index 0 is out-of-bounds when the employee count is 0.
#[test]
fn test_claim_payroll_index_zero_with_no_employees() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);
    let caller = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);

    // DataKey employee count defaults to 0; index 0 >= 0 → InvalidEmployeeIndex
    let result = client.try_claim_payroll(&caller, &agreement_id, &0u32);
    assert!(result.is_err());
}

/// Verifies that `get_employee_claimed_periods` returns 0 for a
/// non-existent employee index.
///
/// Querying claimed periods for an index that doesn't exist should
/// return the default value (0) rather than panicking.
#[test]
fn test_get_claimed_periods_nonexistent_employee_returns_zero() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);

    // Agreement 999 doesn't exist; employee index 42 doesn't exist
    // DataKey defaults to 0 for missing keys
    let periods = client.get_employee_claimed_periods(&999u128, &42u32);
    assert_eq!(periods, 0);
}

// ============================================================================
// SECTION 4: TIME PARAMETER BOUNDARY CONDITIONS
// ============================================================================

/// Verifies that creating a payroll agreement with zero grace_period_seconds
/// succeeds.
///
/// A zero grace period means cancellation takes effect immediately.
/// This is valid and should not be rejected.
#[test]
fn test_payroll_zero_grace_period_succeeds() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &0u64);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.grace_period_seconds, 0);
    assert_eq!(agreement.status, AgreementStatus::Created);
}

/// Verifies that creating a payroll agreement with u64::MAX
/// grace_period_seconds succeeds.
///
/// While impractical, u64::MAX is a valid value and should be stored
/// correctly without overflow during creation.
#[test]
fn test_payroll_max_u64_grace_period_succeeds() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let token = create_test_address(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &u64::MAX);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.grace_period_seconds, u64::MAX);
    assert_eq!(agreement.status, AgreementStatus::Created);
}

// ============================================================================
// SECTION 5: MILESTONE ID BOUNDARY CONDITIONS
// ============================================================================

/// Verifies that approving milestone ID 0 panics with "Invalid milestone ID".
///
/// Milestone IDs are 1-based. ID 0 is always invalid and the contract
/// explicitly checks `milestone_id > 0`.
#[test]
#[should_panic(expected = "Invalid milestone ID")]
fn test_approve_milestone_id_zero_panics() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000i128);

    // Milestone IDs are 1-based; 0 is always invalid
    client.approve_milestone(&agreement_id, &0u32);
}

/// Verifies that approving a milestone ID beyond the count panics.
///
/// If only 2 milestones exist, approving milestone ID 3 must fail
/// with "Invalid milestone ID".
#[test]
#[should_panic(expected = "Invalid milestone ID")]
fn test_approve_milestone_id_beyond_count_panics() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &500i128);
    client.add_milestone(&agreement_id, &500i128);

    // Only 2 milestones exist; ID 3 is out of range
    client.approve_milestone(&agreement_id, &3u32);
}

/// Verifies that claiming milestone ID 0 panics with "Invalid milestone ID".
///
/// Same 1-based ID invariant applies to claiming. ID 0 is never valid.
#[test]
#[should_panic(expected = "Invalid milestone ID")]
fn test_claim_milestone_id_zero_panics() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000i128);
    client.approve_milestone(&agreement_id, &1u32);

    // Attempt to claim milestone ID 0 — always invalid
    client.claim_milestone(&agreement_id, &0u32);
}

/// Verifies that `get_milestone` with ID 0 returns None.
///
/// The read path also validates `milestone_id == 0 || milestone_id > count`
/// and returns None for out-of-range IDs.
#[test]
fn test_get_milestone_id_zero_returns_none() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let employer = create_test_address(&env);
    let contributor = create_test_address(&env);
    let token = create_test_address(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000i128);

    // ID 0 is explicitly handled: returns None
    assert!(client.get_milestone(&agreement_id, &0u32).is_none());

    // ID 1 should exist for comparison
    assert!(client.get_milestone(&agreement_id, &1u32).is_some());
}

/// Verifies that `get_milestone_count` for a non-existent agreement returns 0.
///
/// Querying milestone count on an ID that was never created should return
/// the default (0) without panicking.
#[test]
fn test_get_milestone_count_nonexistent_agreement_returns_zero() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);

    assert_eq!(client.get_milestone_count(&999_999u128), 0);
}
