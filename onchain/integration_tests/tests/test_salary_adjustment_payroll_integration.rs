//! Cross-contract integration test verifying that salary_adjustment's
//! apply_adjustment updates the salary value read by payroll claims.
//!
//! This test deploys both SalaryAdjustmentContract and PayrollContract,
//! creates a payroll agreement with an employee, then creates, approves,
//! and applies a salary adjustment, and verifies that the payroll claim
//! logic can read the updated salary via get_employee_salary.
//!
//! Scope: test only — no runtime logic, storage schema, or APIs are changed.
#![cfg(test)]

use salary_adjustment::{SalaryAdjustmentContract, SalaryAdjustmentContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};
use stello_pay_contract::{storage::DataKey, PayrollContract, PayrollContractClient};

// ============================================================================
// CONSTANTS
// ============================================================================

const ONE_DAY: u64 = 86_400;
const ONE_WEEK: u64 = 604_800;
const INITIAL_SALARY: i128 = 1_000;
const NEW_SALARY: i128 = 2_500;
const PAYROLL_FUND: i128 = 50_000;
const EMPLOYER_FLOAT: i128 = 100_000;

// ============================================================================
// HELPERS
// ============================================================================

fn env() -> Env {
    let e = Env::default();
    e.mock_all_auths();
    e
}

fn addr(env: &Env) -> Address {
    Address::generate(env)
}

fn token(env: &Env) -> Address {
    let admin = addr(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

fn mint(env: &Env, tok: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, tok).mint(to, &amount);
}

fn set_time(env: &Env, ts: u64) {
    env.ledger().with_mut(|li| li.timestamp = ts);
}

fn advance(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| li.timestamp += seconds);
}

fn balance(env: &Env, tok: &Address, who: &Address) -> i128 {
    TokenClient::new(env, tok).balance(who)
}

/// Seeds payroll internal storage so the employee can claim.
fn seed_payroll(
    env: &Env,
    payroll_id: &Address,
    agreement_id: u128,
    tok: &Address,
    employee: &Address,
    salary: i128,
    total_fund: i128,
) {
    env.as_contract(payroll_id, || {
        DataKey::set_agreement_escrow_balance(env, agreement_id, tok, total_fund);
        DataKey::set_agreement_activation_time(env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(env, agreement_id, tok);
        DataKey::set_employee(env, agreement_id, 0, employee);
        DataKey::set_employee_salary(env, agreement_id, 0, salary);
        DataKey::set_employee_claimed_periods(env, agreement_id, 0, 0);
        DataKey::set_employee_count(env, agreement_id, 1);
    });
}

// ============================================================================
// TESTS
// ============================================================================

/// Verifies that a salary adjustment application updates the employee salary
/// visible to the payroll contract, and that subsequent payroll claims reflect
/// the new salary rate.
#[test]
fn test_salary_adjustment_apply_updates_payroll_salary() {
    let env = env();
    set_time(&env, 1_000);

    // Deploy contracts
    let salary_id = env.register_contract(None, SalaryAdjustmentContract);
    let salary_client = SalaryAdjustmentContractClient::new(&env, &salary_id);
    let payroll_id = env.register_contract(None, PayrollContract);
    let payroll_client = PayrollContractClient::new(&env, &payroll_id);

    // Setup addresses and tokens
    let salary_owner = addr(&env);
    let employer = addr(&env);
    let employee = addr(&env);
    let approver = addr(&env);
    let tok = token(&env);
    mint(&env, &tok, &employer, EMPLOYER_FLOAT);

    // Initialize contracts
    salary_client.initialize(&salary_owner);
    payroll_client.initialize(&employer);
    payroll_client.set_salary_adjustment_contract(&employer, &salary_id);

    // Step 1: Create payroll agreement with employee at INITIAL_SALARY
    let agreement_id = payroll_client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    payroll_client.add_employee_to_agreement(&agreement_id, &employee, &INITIAL_SALARY);
    payroll_client.activate_agreement(&agreement_id);

    // Fund payroll contract and seed internal storage
    mint(&env, &tok, &payroll_id, PAYROLL_FUND);
    seed_payroll(
        &env,
        &payroll_id,
        agreement_id,
        &tok,
        &employee,
        INITIAL_SALARY,
        PAYROLL_FUND,
    );

    // Step 2: Employee claims payroll at INITIAL_SALARY
    advance(&env, ONE_DAY * 3);
    payroll_client.claim_payroll(&employee, &agreement_id, &0);
    assert_eq!(balance(&env, &tok, &employee), INITIAL_SALARY * 3);

    // Step 3: Create salary adjustment (employer raises salary to NEW_SALARY)
    let effective_date = env.ledger().timestamp() + ONE_DAY;
    let adjustment_id = salary_client.create_adjustment(
        &employer,
        &employee,
        &approver,
        &INITIAL_SALARY,
        &NEW_SALARY,
        &effective_date,
    );

    // Step 4: Approve the adjustment
    salary_client.approve_adjustment(&approver, &adjustment_id);
    let adj = salary_client.get_adjustment(&adjustment_id).unwrap();
    assert_eq!(adj.status, salary_adjustment::AdjustmentStatus::Approved);

    // Step 5: Advance to effective date and apply
    advance(&env, ONE_DAY * 1); // exactly at effective_date
    salary_client.apply_adjustment(&employer, &adjustment_id);
    let adj = salary_client.get_adjustment(&adjustment_id).unwrap();
    assert_eq!(adj.status, salary_adjustment::AdjustmentStatus::Applied);

    // Step 6: Verify salary tracker reflects new salary
    let tracked_salary = salary_client.get_employee_salary(&employee).unwrap();
    assert_eq!(tracked_salary, NEW_SALARY);

    // Step 7: Claim additional payroll — should use NEW_SALARY for new periods
    // The employee already claimed 3 periods.
    // At this point (time 260,200 + 86,400 = 346,600), 4 periods have elapsed since 1,000.
    // 3 were already claimed, so 1 more is available.
    payroll_client.claim_payroll(&employee, &agreement_id, &0);

    // 3 periods at INITIAL_SALARY (3,000) + 1 period at NEW_SALARY (2,500) = 5,500
    assert_eq!(
        balance(&env, &tok, &employee),
        INITIAL_SALARY * 3 + NEW_SALARY * 1
    );
    assert_eq!(
        payroll_client.get_employee_claimed_periods(&agreement_id, &0),
        4
    );
}

/// Verifies that a decrease adjustment is also reflected in the payroll claim.
#[test]
fn test_salary_decrease_affects_payroll_claim() {
    let env = env();
    set_time(&env, 1_000);

    let salary_id = env.register_contract(None, SalaryAdjustmentContract);
    let salary_client = SalaryAdjustmentContractClient::new(&env, &salary_id);
    let payroll_id = env.register_contract(None, PayrollContract);
    let payroll_client = PayrollContractClient::new(&env, &payroll_id);

    let salary_owner = addr(&env);
    let employer = addr(&env);
    let employee = addr(&env);
    let approver = addr(&env);
    let tok = token(&env);
    mint(&env, &tok, &employer, EMPLOYER_FLOAT);

    salary_client.initialize(&salary_owner);
    payroll_client.initialize(&employer);
    payroll_client.set_salary_adjustment_contract(&employer, &salary_id);

    let agreement_id = payroll_client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    payroll_client.add_employee_to_agreement(&agreement_id, &employee, &INITIAL_SALARY);
    payroll_client.activate_agreement(&agreement_id);
    mint(&env, &tok, &payroll_id, PAYROLL_FUND);
    seed_payroll(
        &env,
        &payroll_id,
        agreement_id,
        &tok,
        &employee,
        INITIAL_SALARY,
        PAYROLL_FUND,
    );

    // Claim some at initial salary
    advance(&env, ONE_DAY * 2);
    payroll_client.claim_payroll(&employee, &agreement_id, &0);
    assert_eq!(balance(&env, &tok, &employee), INITIAL_SALARY * 2);

    // Create and apply a DECREASE adjustment
    let decreased: i128 = 500;
    let effective_date = env.ledger().timestamp() + ONE_DAY;
    let adjustment_id = salary_client.create_adjustment(
        &employer,
        &employee,
        &approver,
        &INITIAL_SALARY,
        &decreased,
        &effective_date,
    );
    salary_client.approve_adjustment(&approver, &adjustment_id);
    advance(&env, ONE_DAY * 1); // exactly at effective_date
    salary_client.apply_adjustment(&employer, &adjustment_id);

    let tracked = salary_client.get_employee_salary(&employee).unwrap();
    assert_eq!(tracked, decreased);

    // Claim — should use decreased salary for new periods
    // At this point (time 173,800 + 86,400 = 260,200), 3 periods have elapsed since 1,000.
    // 2 were already claimed, so 1 more is available.
    payroll_client.claim_payroll(&employee, &agreement_id, &0);

    // 2 periods at 1,000 + 1 period at 500 = 2,500
    assert_eq!(
        balance(&env, &tok, &employee),
        INITIAL_SALARY * 2 + decreased * 1
    );
}

/// Verifies that get_employee_salary returns None before any adjustment is applied.
#[test]
fn test_no_adjustment_yet_returns_none() {
    let env = env();
    set_time(&env, 1_000);

    let salary_id = env.register_contract(None, SalaryAdjustmentContract);
    let salary_client = SalaryAdjustmentContractClient::new(&env, &salary_id);
    let salary_owner = addr(&env);
    salary_client.initialize(&salary_owner);

    let employee = addr(&env);
    let result = salary_client.get_employee_salary(&employee);
    assert!(result.is_none(), "Expected None before any adjustment");
}

/// Verifies that the very next `claim_payroll` call after `apply_adjustment`
/// disburses the updated post-adjustment amount, not the stale pre-adjustment
/// figure.
///
/// Flow:
///   1. Deploy both contracts and link them via `set_salary_adjustment_contract`.
///   2. Create a payroll agreement, add employee at `INITIAL_SALARY = 1_000`.
///   3. Fund internal escrow and claim 2 periods at the original salary.
///   4. Create, approve, and apply an increase to `NEW_SALARY = 2_500`.
///   5. Claim the next single period — it must pay at the new rate.
#[test]
fn test_applied_adjustment_reflected_in_next_claim() {
    let env = env();
    set_time(&env, 1_000);

    let salary_id = env.register_contract(None, SalaryAdjustmentContract);
    let salary_client = SalaryAdjustmentContractClient::new(&env, &salary_id);
    let payroll_id = env.register_contract(None, PayrollContract);
    let payroll_client = PayrollContractClient::new(&env, &payroll_id);

    let salary_owner = addr(&env);
    let employer = addr(&env);
    let employee = addr(&env);
    let approver = addr(&env);
    let tok = token(&env);
    mint(&env, &tok, &employer, EMPLOYER_FLOAT);

    salary_client.initialize(&salary_owner);
    payroll_client.initialize(&employer);
    payroll_client.set_salary_adjustment_contract(&employer, &salary_id);

    // --- Set up payroll agreement ---
    let agreement_id = payroll_client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    payroll_client.add_employee_to_agreement(&agreement_id, &employee, &INITIAL_SALARY);
    payroll_client.activate_agreement(&agreement_id);
    mint(&env, &tok, &payroll_id, PAYROLL_FUND);
    seed_payroll(
        &env,
        &payroll_id,
        agreement_id,
        &tok,
        &employee,
        INITIAL_SALARY,
        PAYROLL_FUND,
    );

    // --- Claim 2 periods at pre-adjustment salary ---
    advance(&env, ONE_DAY * 2);
    payroll_client.claim_payroll(&employee, &agreement_id, &0);
    assert_eq!(balance(&env, &tok, &employee), INITIAL_SALARY * 2);

    // --- Create + approve + apply a salary increase ---
    let effective_ts = env.ledger().timestamp();
    let adj_id = salary_client.create_adjustment(
        &employer,
        &employee,
        &approver,
        &INITIAL_SALARY,
        &NEW_SALARY,
        &effective_ts,
    );
    salary_client.approve_adjustment(&approver, &adj_id);
    salary_client.apply_adjustment(&employer, &adj_id);

    // Confirm salary adjustment contract reflects the new salary
    let tracked_salary = salary_client.get_employee_salary(&employee).unwrap();
    assert_eq!(tracked_salary, NEW_SALARY);

    // --- Claim the very next period — must use NEW_SALARY ---
    advance(&env, ONE_DAY * 1);
    payroll_client.claim_payroll(&employee, &agreement_id, &0);

    // Total = 2 * 1_000 + 1 * 2_500 = 4_500
    assert_eq!(
        balance(&env, &tok, &employee),
        INITIAL_SALARY * 2 + NEW_SALARY * 1
    );
}

/// Verifies that a still-pending (or approved-but-unapplied) adjustment does
/// NOT affect `claim_payroll` amounts. Only an explicitly `apply_adjustment`'d
/// salary override is visible to the payroll contract.
///
/// Flow:
///   1. Deploy both contracts and link them.
///   2. Create a payroll agreement, fund, claim 1 period at `INITIAL_SALARY`.
///   3. Create + approve an increase to `NEW_SALARY` but **do not apply** it.
///   4. Advance time and claim another period.
///   5. Assert the second claim still uses `INITIAL_SALARY`, not `NEW_SALARY`.
#[test]
fn test_pending_adjustment_does_not_affect_claim_amount() {
    let env = env();
    set_time(&env, 1_000);

    let salary_id = env.register_contract(None, SalaryAdjustmentContract);
    let salary_client = SalaryAdjustmentContractClient::new(&env, &salary_id);
    let payroll_id = env.register_contract(None, PayrollContract);
    let payroll_client = PayrollContractClient::new(&env, &payroll_id);

    let salary_owner = addr(&env);
    let employer = addr(&env);
    let employee = addr(&env);
    let approver = addr(&env);
    let tok = token(&env);
    mint(&env, &tok, &employer, EMPLOYER_FLOAT);

    salary_client.initialize(&salary_owner);
    payroll_client.initialize(&employer);
    payroll_client.set_salary_adjustment_contract(&employer, &salary_id);

    // --- Set up payroll agreement ---
    let agreement_id = payroll_client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    payroll_client.add_employee_to_agreement(&agreement_id, &employee, &INITIAL_SALARY);
    payroll_client.activate_agreement(&agreement_id);
    mint(&env, &tok, &payroll_id, PAYROLL_FUND);
    seed_payroll(
        &env,
        &payroll_id,
        agreement_id,
        &tok,
        &employee,
        INITIAL_SALARY,
        PAYROLL_FUND,
    );

    // --- Claim 1 period at initial salary ---
    advance(&env, ONE_DAY * 1);
    payroll_client.claim_payroll(&employee, &agreement_id, &0);
    assert_eq!(balance(&env, &tok, &employee), INITIAL_SALARY * 1);

    // --- Create + approve adjustment but do NOT apply it ---
    let effective_ts = env.ledger().timestamp();
    let adj_id = salary_client.create_adjustment(
        &employer,
        &employee,
        &approver,
        &INITIAL_SALARY,
        &NEW_SALARY,
        &effective_ts,
    );
    salary_client.approve_adjustment(&approver, &adj_id);

    // Confirm the adjustment exists and is Approved, NOT Applied
    let adj = salary_client.get_adjustment(&adj_id).unwrap();
    assert_eq!(adj.status, salary_adjustment::AdjustmentStatus::Approved);

    // Confirm get_employee_salary still returns None (no Applied adjustment yet)
    assert!(salary_client.get_employee_salary(&employee).is_none());

    // --- Claim another period — must still use INITIAL_SALARY ---
    advance(&env, ONE_DAY * 1);
    payroll_client.claim_payroll(&employee, &agreement_id, &0);

    // Total = 1 * 1_000 + 1 * 1_000 = 2_000 (NOT 1_000 + 2_500)
    assert_eq!(balance(&env, &tok, &employee), INITIAL_SALARY * 2);
    assert_eq!(
        payroll_client.get_employee_claimed_periods(&agreement_id, &0),
        2
    );
}

/// Verifies that after an adjustment is applied, the next claim uses the new
/// rate, and that a subsequent (second) pending adjustment that has NOT yet
/// been applied is correctly ignored — proving per-adjustment granularity.
///
/// This is a regression guard against any coarse "latest adjustment wins"
/// logic that might leak unapplied state into payroll.
#[test]
fn test_second_pending_adjustment_ignored_after_first_applied() {
    let env = env();
    set_time(&env, 1_000);

    let salary_id = env.register_contract(None, SalaryAdjustmentContract);
    let salary_client = SalaryAdjustmentContractClient::new(&env, &salary_id);
    let payroll_id = env.register_contract(None, PayrollContract);
    let payroll_client = PayrollContractClient::new(&env, &payroll_id);

    let salary_owner = addr(&env);
    let employer = addr(&env);
    let employee = addr(&env);
    let approver = addr(&env);
    let tok = token(&env);
    mint(&env, &tok, &employer, EMPLOYER_FLOAT);

    salary_client.initialize(&salary_owner);
    payroll_client.initialize(&employer);
    payroll_client.set_salary_adjustment_contract(&employer, &salary_id);

    // --- Set up payroll agreement ---
    let agreement_id = payroll_client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    payroll_client.add_employee_to_agreement(&agreement_id, &employee, &INITIAL_SALARY);
    payroll_client.activate_agreement(&agreement_id);
    mint(&env, &tok, &payroll_id, PAYROLL_FUND);
    seed_payroll(
        &env,
        &payroll_id,
        agreement_id,
        &tok,
        &employee,
        INITIAL_SALARY,
        PAYROLL_FUND,
    );

    // --- Claim 1 period at initial salary ---
    advance(&env, ONE_DAY * 1);
    payroll_client.claim_payroll(&employee, &agreement_id, &0);
    assert_eq!(balance(&env, &tok, &employee), INITIAL_SALARY * 1);

    // --- Adjustment A: create, approve, and apply (1_000 → 2_000) ---
    let effective_ts_a = env.ledger().timestamp();
    let adj_a = salary_client.create_adjustment(
        &employer,
        &employee,
        &approver,
        &INITIAL_SALARY,
        &2_000i128,
        &effective_ts_a,
    );
    salary_client.approve_adjustment(&approver, &adj_a);
    salary_client.apply_adjustment(&employer, &adj_a);

    let tracked = salary_client.get_employee_salary(&employee).unwrap();
    assert_eq!(tracked, 2_000);

    // --- Advance to a new period, then claim at 2_000 ---
    advance(&env, ONE_DAY * 1);
    payroll_client.claim_payroll(&employee, &agreement_id, &0);
    assert_eq!(
        balance(&env, &tok, &employee),
        INITIAL_SALARY * 1 + 2_000 * 1
    );

    // --- Adjustment B: create, approve but do NOT apply (2_000 → 3_000) ---
    // The effective_date must be AFTER adjustment A's to avoid the
    // "Conflicting adjustment exists" guard.
    let effective_ts_b = env.ledger().timestamp();
    let adj_b = salary_client.create_adjustment(
        &employer,
        &employee,
        &approver,
        &2_000i128,
        &3_000i128,
        &effective_ts_b,
    );
    salary_client.approve_adjustment(&approver, &adj_b);

    // Confirm adjustment B is still Approved (not Applied)
    let adj_b_status = salary_client.get_adjustment(&adj_b).unwrap();
    assert_eq!(
        adj_b_status.status,
        salary_adjustment::AdjustmentStatus::Approved
    );

    // Employee still sees 2_000 from the applied adjustment A
    assert_eq!(salary_client.get_employee_salary(&employee).unwrap(), 2_000);

    // --- Claim another period — must use 2_000 (adjustment A), not 3_000 ---
    advance(&env, ONE_DAY * 1);
    payroll_client.claim_payroll(&employee, &agreement_id, &0);

    // Total = 1 * 1_000 + 1 * 2_000 + 1 * 2_000 = 5_000 (NOT 1_000 + 2_000 + 3_000)
    assert_eq!(
        balance(&env, &tok, &employee),
        INITIAL_SALARY * 1 + 2_000 * 2
    );
    assert_eq!(
        payroll_client.get_employee_claimed_periods(&agreement_id, &0),
        3
    );

    // Now apply adjustment B and verify the next claim uses 3_000
    advance(&env, ONE_DAY * 1);
    salary_client.apply_adjustment(&employer, &adj_b);
    assert_eq!(salary_client.get_employee_salary(&employee).unwrap(), 3_000);

    payroll_client.claim_payroll(&employee, &agreement_id, &0);

    // Total = 1_000 + 2_000 + 2_000 + 3_000 = 8_000
    assert_eq!(
        balance(&env, &tok, &employee),
        INITIAL_SALARY * 1 + 2_000 * 2 + 3_000 * 1
    );
    assert_eq!(
        payroll_client.get_employee_claimed_periods(&agreement_id, &0),
        4
    );
}
