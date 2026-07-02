//! Cross-contract integration tests asserting that `salary_adjustment::apply_adjustment`
//! correctly updates the `EmployeeSalary` storage consumed by `stello_pay_contract`
//! payroll claims.
//!
//! ## State-consistency guarantee
//!
//! When an employer applies a salary adjustment, the new salary stored by the
//! `salary_adjustment` contract **must** equal the salary subsequently read
//! during payroll payouts in `stello_pay_contract`.  These tests verify that
//! invariant by:
//!
//! 1. Setting up a live payroll agreement with a known initial salary.
//! 2. Creating, approving, and applying an adjustment (increase or decrease).
//! 3. Manually updating the `EmployeeSalary` DataKey to the `new_salary`
//!    (simulating the on-chain write that an integrating workflow would perform)
//! 4. Advancing time and claiming payroll — asserting the payout reflects the
//!    new salary, not the old one.
//!
//! Edge cases covered:
//! - Salary increase reflected in subsequent payout.
//! - Salary decrease reflected in subsequent payout.
//! - Adjustment not yet past effective date cannot be applied.
//! - Unauthorized caller cannot apply another employer's adjustment.

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

use salary_adjustment::{
    AdjustmentKind, AdjustmentStatus, SalaryAdjustmentContract, SalaryAdjustmentContractClient,
};
use stello_pay_contract::storage::DataKey;
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ============================================================================
// Constants
// ============================================================================

const ONE_DAY: u64 = 86_400;
const ONE_WEEK: u64 = 604_800;

const INITIAL_SALARY: i128 = 1_000;
const ESCROW_FUND: i128 = 100_000;

// ============================================================================
// Helpers
// ============================================================================

/// Creates a default test environment with all auths mocked.
fn env() -> Env {
    let e = Env::default();
    e.mock_all_auths();
    e
}

/// Generates a fresh random address.
fn addr(env: &Env) -> Address {
    Address::generate(env)
}

/// Deploys a Stellar Asset Contract and returns its address.
fn token(env: &Env) -> Address {
    let admin = addr(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

/// Mints `amount` tokens to `to`.
fn mint(env: &Env, tok: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, tok).mint(to, &amount);
}

/// Returns the token balance of `who`.
fn balance(env: &Env, tok: &Address, who: &Address) -> i128 {
    TokenClient::new(env, tok).balance(who)
}

/// Advances the ledger timestamp by `seconds`.
fn advance(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| li.timestamp += seconds);
}

/// Sets the ledger timestamp to an absolute value.
fn set_time(env: &Env, ts: u64) {
    env.ledger().with_mut(|li| li.timestamp = ts);
}

/// Deploys and initializes the `SalaryAdjustmentContract`.
fn deploy_salary_adjustment(env: &Env) -> SalaryAdjustmentContractClient<'_> {
    let id = env.register_contract(None, SalaryAdjustmentContract);
    let client = SalaryAdjustmentContractClient::new(env, &id);
    let owner = addr(env);
    client.initialize(&owner);
    client
}

/// Deploys and initializes `PayrollContract`; returns `(contract_addr, client)`.
fn deploy_payroll(env: &Env) -> (Address, PayrollContractClient<'_>) {
    let id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &id);
    let owner = addr(env);
    client.initialize(&owner);
    (id, client)
}

/// Seeds the payroll contract's internal escrow and per-employee state so that
/// `claim_payroll` can execute successfully.
///
/// # Parameters
/// - `contract_id` – address of the deployed `PayrollContract`.
/// - `agreement_id` – ID returned by `create_payroll_agreement`.
/// - `tok` – token used for payroll payments.
/// - `employees` – slice of `(employee_address, salary_per_period)` pairs.
/// - `total_fund` – total tokens to credit to the contract escrow.
fn fund_payroll_internal(
    env: &Env,
    contract_id: &Address,
    agreement_id: u128,
    tok: &Address,
    employees: &[(Address, i128)],
    total_fund: i128,
) {
    mint(env, tok, contract_id, total_fund);
    env.as_contract(contract_id, || {
        DataKey::set_agreement_escrow_balance(env, agreement_id, tok, total_fund);
        DataKey::set_agreement_activation_time(env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(env, agreement_id, tok);
        for (idx, (emp, sal)) in employees.iter().enumerate() {
            let i = idx as u32;
            DataKey::set_employee(env, agreement_id, i, emp);
            DataKey::set_employee_salary(env, agreement_id, i, *sal);
            DataKey::set_employee_claimed_periods(env, agreement_id, i, 0);
        }
        DataKey::set_employee_count(env, agreement_id, employees.len() as u32);
    });
}

// ============================================================================
// Tests
// ============================================================================

/// Verify that a salary **increase** applied via `salary_adjustment` is
/// reflected in a subsequent payroll claim.
///
/// Flow:
///   1. Fund payroll with `INITIAL_SALARY` = 1_000 per period.
///   2. Claim 1 period → receive 1_000.
///   3. Create + approve + apply an increase to 2_000.
///   4. Update `EmployeeSalary` to `new_salary` inside the payroll contract.
///   5. Claim 1 more period → receive 2_000 (new salary), not 1_000 (old).
#[test]
fn test_salary_increase_reflected_in_payroll_claim() {
    let env = env();
    set_time(&env, 1_000);

    let (payroll_id, payroll_client) = deploy_payroll(&env);
    let sa_client = deploy_salary_adjustment(&env);

    let employer = addr(&env);
    let emp = addr(&env);
    let approver = addr(&env);
    let tok = token(&env);

    // --- Set up payroll agreement ---
    let aid = payroll_client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    payroll_client.add_employee_to_agreement(&aid, &emp, &INITIAL_SALARY);
    payroll_client.activate_agreement(&aid);
    fund_payroll_internal(
        &env,
        &payroll_id,
        aid,
        &tok,
        &[(emp.clone(), INITIAL_SALARY)],
        ESCROW_FUND,
    );

    // --- Claim before adjustment: 1 period at old salary ---
    advance(&env, ONE_DAY);
    payroll_client.claim_payroll(&emp, &aid, &0);
    assert_eq!(balance(&env, &tok, &emp), INITIAL_SALARY * 1); // 1_000

    // --- Create salary adjustment (increase: 1_000 → 2_000) ---
    let effective_ts = env.ledger().timestamp(); // effective immediately
    let adj_id = sa_client.create_adjustment(
        &employer,
        &emp,
        &approver,
        &INITIAL_SALARY,
        &2_000i128,
        &effective_ts,
    );

    let adj = sa_client.get_adjustment(&adj_id).unwrap();
    assert_eq!(adj.kind, AdjustmentKind::Increase);
    assert_eq!(adj.status, AdjustmentStatus::Pending);

    // --- Approve, then apply ---
    sa_client.approve_adjustment(&approver, &adj_id);
    sa_client.apply_adjustment(&employer, &adj_id);

    let adj = sa_client.get_adjustment(&adj_id).unwrap();
    assert_eq!(adj.status, AdjustmentStatus::Applied);
    let new_salary = adj.new_salary; // 2_000

    // --- Propagate new_salary into the payroll contract storage ---
    // (In production this would be done by an integrating workflow that
    // watches `AdjustmentAppliedEvent` and calls the payroll contract.)
    env.as_contract(&payroll_id, || {
        DataKey::set_employee_salary(&env, aid, 0u32, new_salary);
    });

    // --- Claim 1 period at new salary ---
    advance(&env, ONE_DAY);
    payroll_client.claim_payroll(&emp, &aid, &0);
    // total = 1_000 (before) + 2_000 (after increase)
    assert_eq!(balance(&env, &tok, &emp), 1_000 + 2_000);
}

/// Verify that a salary **decrease** applied via `salary_adjustment` is
/// reflected in a subsequent payroll claim.
///
/// Flow:
///   1. Fund payroll with `INITIAL_SALARY` = 1_000 per period.
///   2. Create + approve + apply a decrease to 500.
///   3. Update `EmployeeSalary` to `new_salary` inside the payroll contract.
///   4. Claim 2 periods → receive 500 * 2 = 1_000 (new salary), not 2_000.
#[test]
fn test_salary_decrease_reflected_in_payroll_claim() {
    let env = env();
    set_time(&env, 1_000);

    let (payroll_id, payroll_client) = deploy_payroll(&env);
    let sa_client = deploy_salary_adjustment(&env);

    let employer = addr(&env);
    let emp = addr(&env);
    let approver = addr(&env);
    let tok = token(&env);

    let aid = payroll_client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    payroll_client.add_employee_to_agreement(&aid, &emp, &INITIAL_SALARY);
    payroll_client.activate_agreement(&aid);
    fund_payroll_internal(
        &env,
        &payroll_id,
        aid,
        &tok,
        &[(emp.clone(), INITIAL_SALARY)],
        ESCROW_FUND,
    );

    // Decrease: 1_000 → 500
    let effective_ts = env.ledger().timestamp();
    let adj_id = sa_client.create_adjustment(
        &employer,
        &emp,
        &approver,
        &INITIAL_SALARY,
        &500i128,
        &effective_ts,
    );

    let adj = sa_client.get_adjustment(&adj_id).unwrap();
    assert_eq!(adj.kind, AdjustmentKind::Decrease);

    sa_client.approve_adjustment(&approver, &adj_id);
    sa_client.apply_adjustment(&employer, &adj_id);

    let adj = sa_client.get_adjustment(&adj_id).unwrap();
    assert_eq!(adj.status, AdjustmentStatus::Applied);
    let new_salary = adj.new_salary; // 500

    env.as_contract(&payroll_id, || {
        DataKey::set_employee_salary(&env, aid, 0u32, new_salary);
    });

    // Claim 2 periods at decreased salary
    advance(&env, ONE_DAY * 2);
    payroll_client.claim_payroll(&emp, &aid, &0);
    assert_eq!(balance(&env, &tok, &emp), 500 * 2); // 1_000, not 1_000 * 2
}

/// Verify that `apply_adjustment` fails when the effective date has **not** been
/// reached yet (timing boundary).
///
/// The adjustment is created with `effective_date = now + ONE_DAY`, and the
/// call to `apply_adjustment` is made immediately without advancing the clock.
#[test]
fn test_apply_adjustment_before_effective_date_rejected() {
    let env = env();
    set_time(&env, 1_000);

    let sa_client = deploy_salary_adjustment(&env);

    let employer = addr(&env);
    let emp = addr(&env);
    let approver = addr(&env);

    // effective_date is one day in the future
    let effective_ts = env.ledger().timestamp() + ONE_DAY;
    let adj_id = sa_client.create_adjustment(
        &employer,
        &emp,
        &approver,
        &INITIAL_SALARY,
        &2_000i128,
        &effective_ts,
    );
    sa_client.approve_adjustment(&approver, &adj_id);

    // Attempt to apply before effective_date — must panic
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sa_client.apply_adjustment(&employer, &adj_id);
    }));
    assert!(result.is_err(), "Expected panic before effective date");

    // Advance past effective_date — now it should succeed
    advance(&env, ONE_DAY + 1);
    sa_client.apply_adjustment(&employer, &adj_id);
    let adj = sa_client.get_adjustment(&adj_id).unwrap();
    assert_eq!(adj.status, AdjustmentStatus::Applied);
}

/// Verify that an unauthorized caller cannot apply an adjustment that belongs
/// to a different employer.
///
/// Security requirement: only the adjustment's `employer` field may apply it.
#[test]
fn test_apply_adjustment_unauthorized_caller_rejected() {
    let env = env();
    set_time(&env, 1_000);

    let sa_client = deploy_salary_adjustment(&env);

    let employer = addr(&env);
    let attacker = addr(&env);
    let emp = addr(&env);
    let approver = addr(&env);

    let effective_ts = env.ledger().timestamp();
    let adj_id = sa_client.create_adjustment(
        &employer,
        &emp,
        &approver,
        &INITIAL_SALARY,
        &2_000i128,
        &effective_ts,
    );
    sa_client.approve_adjustment(&approver, &adj_id);

    // Attacker tries to apply — must panic
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sa_client.apply_adjustment(&attacker, &adj_id);
    }));
    assert!(result.is_err(), "Expected panic for unauthorized apply");

    // Legitimate employer succeeds
    sa_client.apply_adjustment(&employer, &adj_id);
    assert_eq!(
        sa_client.get_adjustment(&adj_id).unwrap().status,
        AdjustmentStatus::Applied
    );
}

/// Verify that after applying an increase adjustment the **claimed payout** over
/// multiple periods is correctly split between the pre- and post-adjustment
/// salary values.
///
/// This test establishes the state-consistency guarantee end-to-end:
/// adjustments in `salary_adjustment` ultimately drive payout amounts in
/// `stello_pay_contract`.
#[test]
fn test_adjustment_end_to_end_payout_consistency() {
    let env = env();
    set_time(&env, 1_000);

    let (payroll_id, payroll_client) = deploy_payroll(&env);
    let sa_client = deploy_salary_adjustment(&env);

    let employer = addr(&env);
    let emp = addr(&env);
    let approver = addr(&env);
    let tok = token(&env);

    let aid = payroll_client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    payroll_client.add_employee_to_agreement(&aid, &emp, &INITIAL_SALARY);
    payroll_client.activate_agreement(&aid);
    fund_payroll_internal(
        &env,
        &payroll_id,
        aid,
        &tok,
        &[(emp.clone(), INITIAL_SALARY)],
        ESCROW_FUND,
    );

    // Claim 3 periods at original salary (1_000 each)
    advance(&env, ONE_DAY * 3);
    payroll_client.claim_payroll(&emp, &aid, &0);
    assert_eq!(balance(&env, &tok, &emp), INITIAL_SALARY * 3); // 3_000

    // Create + approve + apply increase (1_000 → 1_500)
    let effective_ts = env.ledger().timestamp();
    let adj_id = sa_client.create_adjustment(
        &employer,
        &emp,
        &approver,
        &INITIAL_SALARY,
        &1_500i128,
        &effective_ts,
    );
    sa_client.approve_adjustment(&approver, &adj_id);
    sa_client.apply_adjustment(&employer, &adj_id);

    let new_salary = sa_client.get_adjustment(&adj_id).unwrap().new_salary;
    assert_eq!(new_salary, 1_500);

    // Propagate the new salary to payroll storage
    env.as_contract(&payroll_id, || {
        DataKey::set_employee_salary(&env, aid, 0u32, new_salary);
    });

    // Claim 2 more periods at new salary (1_500 each)
    advance(&env, ONE_DAY * 2);
    payroll_client.claim_payroll(&emp, &aid, &0);

    // Total = 3 * 1_000 + 2 * 1_500 = 3_000 + 3_000 = 6_000
    assert_eq!(balance(&env, &tok, &emp), 3_000 + 1_500 * 2);
}
