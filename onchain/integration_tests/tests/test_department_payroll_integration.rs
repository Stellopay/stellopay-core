//! Cross-contract integration tests: `department_manager` × `stello_pay_contract`
//!
//! # Purpose
//! These tests exercise the **organizational offboarding workflow** — removing an
//! employee from a department via `department_manager` — and assert the documented
//! behaviour of `stello_pay_contract::claim_payroll` before and after that removal.
//!
//! # Key finding (codified as tests)
//! The two contracts are **intentionally decoupled**.  Department membership is an
//! organizational concept managed entirely by `department_manager`; payroll
//! eligibility is governed solely by:
//!   1. The agreement's `AgreementStatus` (`Active`, or `Cancelled` + active grace).
//!   2. The elapsed time since activation relative to the period duration.
//!   3. The caller matching the employee stored at `employee_index` in the agreement.
//!
//! `remove_employee_from_department` writes only to `department_manager` storage and
//! emits the `emp_rmvd` event.  It does **not** touch any key in `stello_pay_contract`
//! storage, so a previously eligible employee **retains full claim eligibility** after
//! being removed from every department.
//!
//! # Test inventory
//! | # | Name | Behaviour exercised |
//! |---|------|---------------------|
//! | 1 | `claim_succeeds_before_any_department_assignment` | Payroll is independent of dept membership from the start |
//! | 2 | `claim_succeeds_after_department_assignment` | Assigning to a dept does not gate or alter payroll |
//! | 3 | `claim_still_succeeds_after_department_removal` | **Core assertion**: removal does not revoke eligibility |
//! | 4 | `sequential_claim_after_removal_accumulates_correctly` | Multiple claim rounds across the removal boundary |
//! | 5 | `multiple_employees_removal_of_one_does_not_affect_other` | Removal is scoped to one employee |
//! | 6 | `stranger_cannot_claim_regardless_of_dept_membership` | Auth boundary: dept membership ≠ payroll auth |
//! | 7 | `claim_during_grace_period_after_dept_removal` | Cancelled agreement grace path is unaffected |
//! | 8 | `employee_reassigned_to_new_dept_can_still_claim` | Re-assignment (dept change) has no payroll side-effect |
//! | 9 | `dept_removal_event_is_emitted_payroll_state_unchanged` | `emp_rmvd` event fires; payroll storage is untouched |
//! | 10 | `fully_removed_employee_loses_dept_membership_only` | `get_employee_department` returns None; claim still works |

#![cfg(test)]
#![allow(deprecated)]

use department_manager::{DepartmentManagerContract, DepartmentManagerContractClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, TryFromVal,
};
use stello_pay_contract::{
    storage::{AgreementStatus, DataKey},
    PayrollContract, PayrollContractClient,
};

// ─────────────────────────────────────────────────────────────────────────────
// Shared constants
// ─────────────────────────────────────────────────────────────────────────────

/// One 24-hour period in seconds — the payroll claim granularity used in all tests.
const ONE_DAY: u64 = 86_400;

/// Grace period after agreement cancellation during which employees may still
/// claim outstanding periods (one week).
const GRACE: u64 = ONE_DAY * 7;

/// Salary credited to the employee per period (in token base units).
const SALARY: i128 = 1_000;

/// Total escrow pre-funded into the payroll contract (enough for 30 days).
const FUND: i128 = 30_000;

/// Generous token float given to the employer so minting never fails.
const EMPLOYER_FLOAT: i128 = 1_000_000;

// ─────────────────────────────────────────────────────────────────────────────
// Test environment helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Creates a default Soroban test environment with all auth mocked.
fn env() -> Env {
    let e = Env::default();
    e.mock_all_auths();
    e
}

/// Generates a fresh random address in the given environment.
fn addr(e: &Env) -> Address {
    Address::generate(e)
}

/// Sets the ledger timestamp to an absolute value.
fn set_time(e: &Env, ts: u64) {
    e.ledger().with_mut(|li| li.timestamp = ts);
}

/// Advances the ledger timestamp by `delta` seconds.
fn advance(e: &Env, delta: u64) {
    e.ledger().with_mut(|li| li.timestamp += delta);
}

/// Registers a new Stellar Asset Contract and returns its address.
fn create_token(e: &Env) -> Address {
    let admin = addr(e);
    e.register_stellar_asset_contract_v2(admin).address()
}

/// Mints `amount` tokens to `to` via the Stellar Asset Client.
fn mint(e: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(e, token).mint(to, &amount);
}

/// Returns the token balance of `who`.
fn balance(e: &Env, token: &Address, who: &Address) -> i128 {
    TokenClient::new(e, token).balance(who)
}

// ─────────────────────────────────────────────────────────────────────────────
// Contract deployment helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Thin wrapper carrying a deployed `DepartmentManagerContract`.
struct DeptManager<'a> {
    id: Address,
    client: DepartmentManagerContractClient<'a>,
}

/// Deploys and initialises `DepartmentManagerContract`.
fn deploy_dept_manager(e: &Env) -> DeptManager<'_> {
    let id = e.register_contract(None, DepartmentManagerContract);
    let client = DepartmentManagerContractClient::new(e, &id);
    let admin = addr(e);
    client.initialize(&admin);
    DeptManager { id, client }
}

/// Thin wrapper carrying a deployed `PayrollContract`.
struct Payroll<'a> {
    id: Address,
    client: PayrollContractClient<'a>,
}

/// Deploys and initialises `PayrollContract` with a throwaway owner.
fn deploy_payroll(e: &Env) -> Payroll<'_> {
    let id = e.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(e, &id);
    let owner = addr(e);
    client.initialize(&owner);
    Payroll { id, client }
}

// ─────────────────────────────────────────────────────────────────────────────
// Payroll seeding helper
// ─────────────────────────────────────────────────────────────────────────────

/// Seeds all payroll storage keys needed for `claim_payroll` to succeed for a
/// single employee at index 0.  This mirrors the approach used in other
/// integration tests (e.g. `test_token_vesting_payroll_integration.rs`) and
/// avoids needing the employer to hold tokens at setup time.
///
/// Call this inside `env.as_contract(&payroll.id, || { … })` — it is a
/// pure storage write with no cross-contract calls.
fn seed_payroll(
    e: &Env,
    agreement_id: u128,
    token: &Address,
    employee: &Address,
    activation_time: u64,
) {
    DataKey::set_agreement_escrow_balance(e, agreement_id, token, FUND);
    DataKey::set_agreement_activation_time(e, agreement_id, activation_time);
    DataKey::set_agreement_period_duration(e, agreement_id, ONE_DAY);
    DataKey::set_agreement_token(e, agreement_id, token);
    DataKey::set_employee(e, agreement_id, 0, employee);
    DataKey::set_employee_salary(e, agreement_id, 0, SALARY);
    DataKey::set_employee_claimed_periods(e, agreement_id, 0, 0);
    DataKey::set_employee_count(e, agreement_id, 1);
}

/// Seeds payroll storage for *two* employees (indices 0 and 1) in the same
/// agreement.  Used by multi-employee tests.
fn seed_payroll_two_employees(
    e: &Env,
    agreement_id: u128,
    token: &Address,
    employee_a: &Address,
    employee_b: &Address,
    activation_time: u64,
) {
    DataKey::set_agreement_escrow_balance(e, agreement_id, token, FUND * 2);
    DataKey::set_agreement_activation_time(e, agreement_id, activation_time);
    DataKey::set_agreement_period_duration(e, agreement_id, ONE_DAY);
    DataKey::set_agreement_token(e, agreement_id, token);
    DataKey::set_employee(e, agreement_id, 0, employee_a);
    DataKey::set_employee_salary(e, agreement_id, 0, SALARY);
    DataKey::set_employee_claimed_periods(e, agreement_id, 0, 0);
    DataKey::set_employee(e, agreement_id, 1, employee_b);
    DataKey::set_employee_salary(e, agreement_id, 1, SALARY);
    DataKey::set_employee_claimed_periods(e, agreement_id, 1, 0);
    DataKey::set_employee_count(e, agreement_id, 2);
}

// ─────────────────────────────────────────────────────────────────────────────
// Full workflow setup: department + payroll side-by-side
// ─────────────────────────────────────────────────────────────────────────────

/// Bundles all context returned by the common setup routine.
struct Setup<'a> {
    payroll: Payroll<'a>,
    dept: DeptManager<'a>,
    token: Address,
    employer: Address,
    employee: Address,
    org_id: u128,
    dept_id: u128,
    agreement_id: u128,
}

/// Creates and activates one payroll agreement and one org/department, assigns
/// the employee to the department, pre-funds escrow, and seeds payroll storage.
/// Time starts at `t = 1_000`.
fn setup<'a>(e: &'a Env) -> Setup<'a> {
    set_time(e, 1_000);

    let payroll = deploy_payroll(e);
    let dept = deploy_dept_manager(e);
    let token = create_token(e);
    let employer = addr(e);
    let employee = addr(e);

    // Mint tokens to employer so the SAC transfer in claim_payroll can succeed.
    mint(e, &token, &employer, EMPLOYER_FLOAT);

    // ── Payroll side ──────────────────────────────────────────────────────────
    let agreement_id = payroll
        .client
        .create_payroll_agreement(&employer, &token, &GRACE);
    payroll
        .client
        .add_employee_to_agreement(&agreement_id, &employee, &SALARY);
    payroll.client.activate_agreement(&agreement_id);

    // Transfer tokens into the payroll contract and seed the escrow keys.
    TokenClient::new(e, &token).transfer(&employer, &payroll.id, &FUND);
    let activation_time = e.ledger().timestamp();
    e.as_contract(&payroll.id, || {
        seed_payroll(e, agreement_id, &token, &employee, activation_time);
    });

    // ── Department side ───────────────────────────────────────────────────────
    let org_id = dept
        .client
        .create_organization(&employer, &symbol_short!("Acme"));
    let dept_id = dept
        .client
        .create_department(&employer, &org_id, &symbol_short!("Eng"), &None);
    dept.client
        .assign_employee_to_department(&employer, &org_id, &dept_id, &employee);

    Setup {
        payroll,
        dept,
        token,
        employer,
        employee,
        org_id,
        dept_id,
        agreement_id,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — Payroll works with no department assignment whatsoever
// ─────────────────────────────────────────────────────────────────────────────

/// Establishes the baseline: an employee who has never been added to *any*
/// department can claim payroll normally.  Department membership is irrelevant.
#[test]
fn claim_succeeds_before_any_department_assignment() {
    let e = env();
    set_time(&e, 1_000);

    let payroll = deploy_payroll(&e);
    let token = create_token(&e);
    let employer = addr(&e);
    let employee = addr(&e);
    mint(&e, &token, &employer, EMPLOYER_FLOAT);

    // --- payroll setup (no department at all) ---
    let agreement_id = payroll
        .client
        .create_payroll_agreement(&employer, &token, &GRACE);
    payroll
        .client
        .add_employee_to_agreement(&agreement_id, &employee, &SALARY);
    payroll.client.activate_agreement(&agreement_id);
    TokenClient::new(&e, &token).transfer(&employer, &payroll.id, &FUND);
    let activation_time = e.ledger().timestamp();
    e.as_contract(&payroll.id, || {
        seed_payroll(&e, agreement_id, &token, &employee, activation_time);
    });

    // advance 3 periods
    advance(&e, ONE_DAY * 3);

    // employee has no department — should still claim successfully
    payroll.client.claim_payroll(&employee, &agreement_id, &0);

    assert_eq!(
        payroll
            .client
            .get_employee_claimed_periods(&agreement_id, &0),
        3,
        "should have claimed 3 periods"
    );
    assert_eq!(
        balance(&e, &token, &employee),
        SALARY * 3,
        "employee should hold 3 days of salary"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — Payroll works after department assignment (no change)
// ─────────────────────────────────────────────────────────────────────────────

/// Assigning an employee to a department must not alter their payroll
/// eligibility in any way — positively or negatively.
#[test]
fn claim_succeeds_after_department_assignment() {
    let e = env();
    let s = setup(&e);

    // employee is already assigned to s.dept_id by setup()
    assert_eq!(
        s.dept
            .client
            .get_employee_department(&s.employee, &s.org_id),
        Some(s.dept_id),
        "employee should be in dept after setup"
    );

    advance(&e, ONE_DAY * 5);

    s.payroll
        .client
        .claim_payroll(&s.employee, &s.agreement_id, &0);

    assert_eq!(
        s.payroll
            .client
            .get_employee_claimed_periods(&s.agreement_id, &0),
        5
    );
    assert_eq!(balance(&e, &s.token, &s.employee), SALARY * 5);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — Core assertion: payroll claim still works after dept removal
// ─────────────────────────────────────────────────────────────────────────────

/// **This is the primary test for issue #946.**
///
/// Workflow:
/// 1. Employee is assigned to a department.
/// 2. Three periods elapse; employee claims normally.
/// 3. HR removes the employee from the department (`remove_employee_from_department`).
/// 4. Two more periods elapse.
/// 5. Employee claims again — must succeed unchanged.
///
/// The department-manager and payroll contracts share no state.  Step 3 writes
/// only to `department_manager` storage and must have zero effect on the
/// payroll agreement.
#[test]
fn claim_still_succeeds_after_department_removal() {
    let e = env();
    let s = setup(&e);

    // ── Phase 1: claim while in department ───────────────────────────────────
    advance(&e, ONE_DAY * 3);
    s.payroll
        .client
        .claim_payroll(&s.employee, &s.agreement_id, &0);
    assert_eq!(
        s.payroll
            .client
            .get_employee_claimed_periods(&s.agreement_id, &0),
        3,
        "should have claimed 3 periods before removal"
    );

    // ── Offboarding: remove from department ──────────────────────────────────
    s.dept
        .client
        .remove_employee_from_department(&s.employer, &s.org_id, &s.employee);

    // Verify the department record reflects the removal.
    assert_eq!(
        s.dept
            .client
            .get_employee_department(&s.employee, &s.org_id),
        None,
        "employee should have no department after removal"
    );
    assert!(
        s.dept
            .client
            .get_department_employees(&s.dept_id)
            .is_empty(),
        "department employee list should be empty after removal"
    );

    // ── Phase 2: claim after department removal ───────────────────────────────
    advance(&e, ONE_DAY * 2);
    s.payroll
        .client
        .claim_payroll(&s.employee, &s.agreement_id, &0);

    assert_eq!(
        s.payroll
            .client
            .get_employee_claimed_periods(&s.agreement_id, &0),
        5,
        "should have claimed 2 more periods after removal (total 5)"
    );
    assert_eq!(
        balance(&e, &s.token, &s.employee),
        SALARY * 5,
        "employee balance should reflect 5 total claimed periods"
    );

    // Payroll agreement status must be unaffected.
    assert_eq!(
        s.payroll
            .client
            .get_agreement(&s.agreement_id)
            .unwrap()
            .status,
        AgreementStatus::Active,
        "agreement must remain Active after department removal"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — Sequential claims across the removal boundary accumulate correctly
// ─────────────────────────────────────────────────────────────────────────────

/// Exercises three claim rounds: before, during (at removal boundary), and after
/// removal, verifying period accounting is strictly cumulative and correct.
#[test]
fn sequential_claim_after_removal_accumulates_correctly() {
    let e = env();
    let s = setup(&e);

    // Round 1: 2 days before removal
    advance(&e, ONE_DAY * 2);
    s.payroll
        .client
        .claim_payroll(&s.employee, &s.agreement_id, &0);
    assert_eq!(
        s.payroll
            .client
            .get_employee_claimed_periods(&s.agreement_id, &0),
        2
    );

    // Remove from department
    s.dept
        .client
        .remove_employee_from_department(&s.employer, &s.org_id, &s.employee);

    // Round 2: 3 more days pass while no dept membership
    advance(&e, ONE_DAY * 3);
    s.payroll
        .client
        .claim_payroll(&s.employee, &s.agreement_id, &0);
    assert_eq!(
        s.payroll
            .client
            .get_employee_claimed_periods(&s.agreement_id, &0),
        5,
        "cumulative claimed periods should be 5 after round 2"
    );

    // Round 3: 4 more days — still no dept
    advance(&e, ONE_DAY * 4);
    s.payroll
        .client
        .claim_payroll(&s.employee, &s.agreement_id, &0);
    assert_eq!(
        s.payroll
            .client
            .get_employee_claimed_periods(&s.agreement_id, &0),
        9,
        "cumulative claimed periods should be 9 after round 3"
    );

    // Total payout = 9 × SALARY
    assert_eq!(balance(&e, &s.token, &s.employee), SALARY * 9);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — Removing one employee does not affect co-workers in the same agreement
// ─────────────────────────────────────────────────────────────────────────────

/// Two employees share one payroll agreement and one department.  Removing
/// employee A from the department must leave employee B's claim eligibility
/// completely intact, and vice-versa.
#[test]
fn multiple_employees_removal_of_one_does_not_affect_other() {
    let e = env();
    set_time(&e, 2_000);

    let payroll = deploy_payroll(&e);
    let dept = deploy_dept_manager(&e);
    let token = create_token(&e);
    let employer = addr(&e);
    let employee_a = addr(&e);
    let employee_b = addr(&e);
    mint(&e, &token, &employer, EMPLOYER_FLOAT);

    // ── Payroll: two-employee agreement ──────────────────────────────────────
    let agreement_id = payroll
        .client
        .create_payroll_agreement(&employer, &token, &GRACE);
    payroll
        .client
        .add_employee_to_agreement(&agreement_id, &employee_a, &SALARY);
    payroll
        .client
        .add_employee_to_agreement(&agreement_id, &employee_b, &SALARY);
    payroll.client.activate_agreement(&agreement_id);
    TokenClient::new(&e, &token).transfer(&employer, &payroll.id, &(FUND * 2));
    let activation_time = e.ledger().timestamp();
    e.as_contract(&payroll.id, || {
        seed_payroll_two_employees(
            &e,
            agreement_id,
            &token,
            &employee_a,
            &employee_b,
            activation_time,
        );
    });

    // ── Department: same org, both assigned ──────────────────────────────────
    let org_id = dept
        .client
        .create_organization(&employer, &symbol_short!("Corp"));
    let dept_id = dept
        .client
        .create_department(&employer, &org_id, &symbol_short!("HR"), &None);
    dept.client
        .assign_employee_to_department(&employer, &org_id, &dept_id, &employee_a);
    dept.client
        .assign_employee_to_department(&employer, &org_id, &dept_id, &employee_b);

    advance(&e, ONE_DAY * 4);

    // Remove only employee A from the department
    dept.client
        .remove_employee_from_department(&employer, &org_id, &employee_a);

    // Employee A: removed from dept but can still claim
    payroll.client.claim_payroll(&employee_a, &agreement_id, &0);
    assert_eq!(
        payroll
            .client
            .get_employee_claimed_periods(&agreement_id, &0),
        4,
        "employee A should claim 4 periods after removal"
    );

    // Employee B: still in dept AND still can claim
    assert_eq!(
        dept.client.get_employee_department(&employee_b, &org_id),
        Some(dept_id),
        "employee B should still be in dept"
    );
    payroll.client.claim_payroll(&employee_b, &agreement_id, &1);
    assert_eq!(
        payroll
            .client
            .get_employee_claimed_periods(&agreement_id, &1),
        4,
        "employee B should also claim 4 periods"
    );

    // Both received the same salary
    assert_eq!(balance(&e, &token, &employee_a), SALARY * 4);
    assert_eq!(balance(&e, &token, &employee_b), SALARY * 4);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — Stranger cannot claim regardless of department membership
// ─────────────────────────────────────────────────────────────────────────────

/// Department membership has zero bearing on who is authorised to call
/// `claim_payroll`.  A stranger assigned to the same department as the employee
/// must be rejected, and the real employee must succeed.
///
/// This guards the security boundary: dept membership ≠ payroll authorisation.
#[test]
fn stranger_cannot_claim_regardless_of_dept_membership() {
    let e = env();
    let s = setup(&e);

    // Assign a stranger to the *same* department as the employee.
    let stranger = addr(&e);
    s.dept
        .client
        .assign_employee_to_department(&s.employer, &s.org_id, &s.dept_id, &stranger);

    advance(&e, ONE_DAY * 3);

    // Stranger tries to claim at index 0 (where s.employee lives) — must fail.
    let stranger_result = s
        .payroll
        .client
        .try_claim_payroll(&stranger, &s.agreement_id, &0);
    assert!(
        stranger_result.is_err(),
        "stranger must not claim another employee's payroll even if in same dept"
    );

    // The real employee must still succeed.
    s.payroll
        .client
        .claim_payroll(&s.employee, &s.agreement_id, &0);
    assert_eq!(balance(&e, &s.token, &s.employee), SALARY * 3);

    // Stranger balance must remain zero.
    assert_eq!(
        balance(&e, &s.token, &stranger),
        0,
        "stranger should have received nothing"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7 — Claim during grace period after department removal
// ─────────────────────────────────────────────────────────────────────────────

/// When an agreement is cancelled and the employee has been removed from the
/// department, claims submitted within the grace window must still succeed.
/// Department state is irrelevant to grace-period eligibility.
///
/// Workflow:
/// 1. Employee claims 2 periods while in dept.
/// 2. Employee is removed from dept.
/// 3. Agreement is cancelled (grace period starts).
/// 4. One more period elapses within the grace window.
/// 5. Employee claims — must succeed.
/// 6. Grace window expires; subsequent claim must fail.
#[test]
fn claim_during_grace_period_after_dept_removal() {
    let e = env();
    let s = setup(&e);

    // Step 1 — initial claim while in dept
    advance(&e, ONE_DAY * 2);
    s.payroll
        .client
        .claim_payroll(&s.employee, &s.agreement_id, &0);
    assert_eq!(
        s.payroll
            .client
            .get_employee_claimed_periods(&s.agreement_id, &0),
        2
    );

    // Step 2 — remove from department
    s.dept
        .client
        .remove_employee_from_department(&s.employer, &s.org_id, &s.employee);
    assert_eq!(
        s.dept
            .client
            .get_employee_department(&s.employee, &s.org_id),
        None
    );

    // Step 3 — cancel agreement (grace = GRACE seconds from now)
    advance(&e, ONE_DAY);
    s.payroll.client.cancel_agreement(&s.agreement_id);
    assert_eq!(
        s.payroll
            .client
            .get_agreement(&s.agreement_id)
            .unwrap()
            .status,
        AgreementStatus::Cancelled
    );
    assert!(
        s.payroll.client.is_grace_period_active(&s.agreement_id),
        "grace period should be active immediately after cancellation"
    );

    // Step 4 & 5 — one more period elapses, employee claims inside grace window
    advance(&e, ONE_DAY);
    s.payroll
        .client
        .claim_payroll(&s.employee, &s.agreement_id, &0);
    assert_eq!(
        s.payroll
            .client
            .get_employee_claimed_periods(&s.agreement_id, &0),
        4,
        "should have claimed 4 periods total (2 + 1 cancelled-day + 1 grace)"
    );

    // Step 6 — after grace expires, claim must fail
    advance(&e, GRACE + ONE_DAY);
    assert!(
        !s.payroll.client.is_grace_period_active(&s.agreement_id),
        "grace period should have expired"
    );
    let late_result = s
        .payroll
        .client
        .try_claim_payroll(&s.employee, &s.agreement_id, &0);
    assert!(
        late_result.is_err(),
        "claim after grace expiry must be rejected"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 8 — Reassigning to a different department does not affect payroll
// ─────────────────────────────────────────────────────────────────────────────

/// Moving an employee from one department to another (re-assignment) must not
/// change their payroll eligibility or accumulated periods.
///
/// This exercises `assign_employee_to_department` on an already-assigned
/// employee (which internally calls `remove_employee_from_dept_internal` first).
#[test]
fn employee_reassigned_to_new_dept_can_still_claim() {
    let e = env();
    let s = setup(&e);

    // Create a second department in the same org
    let dept_b =
        s.dept
            .client
            .create_department(&s.employer, &s.org_id, &symbol_short!("Ops"), &None);

    // Claim 2 periods before reassignment
    advance(&e, ONE_DAY * 2);
    s.payroll
        .client
        .claim_payroll(&s.employee, &s.agreement_id, &0);
    assert_eq!(
        s.payroll
            .client
            .get_employee_claimed_periods(&s.agreement_id, &0),
        2
    );

    // Reassign employee to dept_b (dept_manager moves them automatically)
    s.dept
        .client
        .assign_employee_to_department(&s.employer, &s.org_id, &dept_b, &s.employee);
    assert_eq!(
        s.dept
            .client
            .get_employee_department(&s.employee, &s.org_id),
        Some(dept_b),
        "employee should now be in dept_b"
    );

    // Claim 3 more periods after reassignment
    advance(&e, ONE_DAY * 3);
    s.payroll
        .client
        .claim_payroll(&s.employee, &s.agreement_id, &0);
    assert_eq!(
        s.payroll
            .client
            .get_employee_claimed_periods(&s.agreement_id, &0),
        5,
        "should have claimed 5 periods total across dept change"
    );
    assert_eq!(balance(&e, &s.token, &s.employee), SALARY * 5);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 9 — emp_rmvd event fires; payroll storage is completely untouched
// ─────────────────────────────────────────────────────────────────────────────

/// Verifies two things in combination:
///   (a) `remove_employee_from_department` emits the expected `emp_rmvd` event.
///   (b) All payroll-relevant state (agreement status, escrow balance, employee
///       address, claimed periods) is byte-for-byte identical before and after
///       the department removal call.
#[test]
fn dept_removal_event_is_emitted_payroll_state_unchanged() {
    let e = env();
    let s = setup(&e);

    advance(&e, ONE_DAY * 2);

    // Snapshot payroll state before removal
    let agreement_before = s.payroll.client.get_agreement(&s.agreement_id).unwrap();
    let employee_addr_before = e.as_contract(&s.payroll.id, || {
        DataKey::get_employee(&e, s.agreement_id, 0)
    });
    let claimed_before = s
        .payroll
        .client
        .get_employee_claimed_periods(&s.agreement_id, &0);

    // Remove from department
    s.dept
        .client
        .remove_employee_from_department(&s.employer, &s.org_id, &s.employee);

    // ── Assert `emp_rmvd` event was emitted ───────────────────────────────────
    let all_events = e.events().all();
    let removal_event = all_events.iter().find(|(contract_id, topics, _data)| {
        if *contract_id != s.dept.id {
            return false;
        }
        if topics.is_empty() {
            return false;
        }
        let topic = topics.get(0).unwrap();
        soroban_sdk::Symbol::try_from_val(&e, &topic)
            .map(|sym| sym.to_string() == "emp_rmvd")
            .unwrap_or(false)
    });
    assert!(
        removal_event.is_some(),
        "emp_rmvd event must be emitted by department_manager on removal"
    );

    // ── Assert payroll state is identical after removal ───────────────────────
    let agreement_after = s.payroll.client.get_agreement(&s.agreement_id).unwrap();
    let employee_addr_after = e.as_contract(&s.payroll.id, || {
        DataKey::get_employee(&e, s.agreement_id, 0)
    });
    let claimed_after = s
        .payroll
        .client
        .get_employee_claimed_periods(&s.agreement_id, &0);

    assert_eq!(
        agreement_before.status, agreement_after.status,
        "agreement status must not change after dept removal"
    );
    assert_eq!(
        employee_addr_before, employee_addr_after,
        "employee address stored in payroll must be unchanged"
    );
    assert_eq!(
        claimed_before, claimed_after,
        "claimed periods must be unchanged after dept removal"
    );

    // Employee can still claim after the event check
    s.payroll
        .client
        .claim_payroll(&s.employee, &s.agreement_id, &0);
    assert_eq!(
        s.payroll
            .client
            .get_employee_claimed_periods(&s.agreement_id, &0),
        2
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 10 — get_employee_department returns None; payroll claim still works
// ─────────────────────────────────────────────────────────────────────────────

/// Confirms the two state machines are truly orthogonal by asserting that
/// `get_employee_department` returns `None` (no dept) at the same instant
/// `claim_payroll` succeeds — both outcomes must hold simultaneously.
#[test]
fn fully_removed_employee_loses_dept_membership_only() {
    let e = env();
    let s = setup(&e);

    advance(&e, ONE_DAY * 4);

    // Remove from department
    s.dept
        .client
        .remove_employee_from_department(&s.employer, &s.org_id, &s.employee);

    // Department state: no membership
    assert_eq!(
        s.dept
            .client
            .get_employee_department(&s.employee, &s.org_id),
        None,
        "department membership should be None after removal"
    );
    assert!(
        s.dept
            .client
            .get_department_employees(&s.dept_id)
            .is_empty(),
        "department employee list should be empty"
    );

    // Payroll state: eligibility unaffected
    s.payroll
        .client
        .claim_payroll(&s.employee, &s.agreement_id, &0);

    assert_eq!(
        s.payroll
            .client
            .get_employee_claimed_periods(&s.agreement_id, &0),
        4,
        "employee with no dept membership should claim 4 periods"
    );
    assert_eq!(
        balance(&e, &s.token, &s.employee),
        SALARY * 4,
        "employee balance must reflect 4 claimed periods"
    );
    assert_eq!(
        s.payroll
            .client
            .get_agreement(&s.agreement_id)
            .unwrap()
            .status,
        AgreementStatus::Active,
        "agreement must still be Active"
    );
}
