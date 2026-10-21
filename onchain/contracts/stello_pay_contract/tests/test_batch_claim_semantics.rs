//! Partial-failure and retry semantics for the batch claim entry points (#1320).
//!
//! `batch_claim_payroll` and `batch_claim_milestones` are documented as
//! partial-success: one unclaimable element is reported per element and does not
//! abort the rest. What was not validated anywhere was the reentrancy of that
//! model from a caller's point of view — a caller that retries after a partial
//! batch must be able to tell which elements are outstanding, and the retry must
//! not pay anything twice.
//!
//! # Coverage
//!
//! | # | Scenario |
//! |---|----------|
//! | 1 | Milestones — a fully successful batch settles every element |
//! | 2 | Milestones — a failing element does not abort the elements after it |
//! | 3 | Milestones — retry of the remainder pays the remainder only |
//! | 4 | Milestones — a naive retry of the whole settled batch pays nothing again |
//! | 5 | Payroll — a single-element batch is fully successful |
//! | 6 | Payroll — a failing element does not abort the elements after it |
//! | 7 | Payroll — retry before any new period settles nothing |
//! | 8 | Payroll — retry after a new period pays that period only |
//! | 9 | The enforced batch size limit matches the documented one |

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};
use stello_pay_contract::{
    storage::{DataKey, PayrollError, MAX_BATCH_SIZE},
    PayrollContract, PayrollContractClient,
};

// ============================================================================
// CONSTANTS
// ============================================================================

const ONE_DAY: u64 = 86_400;
const ONE_WEEK: u64 = 604_800;
const SALARY: i128 = 1_000;

/// Large enough that the milestone escrow never binds, so the batch element
/// outcomes are decided by approval/claim state alone.
const MILESTONE_FUND: i128 = 1_000_000;

/// Per-element `MilestoneClaimResult::error_code` values, as documented in
/// `docs/batch-payments.md`.
const MILESTONE_OK: u32 = 0;
const MILESTONE_NOT_APPROVED: u32 = 3;
const MILESTONE_ALREADY_CLAIMED: u32 = 4;

// ============================================================================
// SHARED HELPERS
// ============================================================================

fn create_token(env: &Env) -> Address {
    env.register_stellar_asset_contract_v2(Address::generate(env))
        .address()
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

fn advance_time(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| {
        li.timestamp += seconds;
    });
}

// ============================================================================
// MILESTONE FIXTURE
// ============================================================================

/// Deploys the contract and returns `(env, employer, contributor, token, client)`.
fn milestone_env() -> (
    Env,
    Address,
    Address,
    Address,
    PayrollContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    (env, employer, contributor, token, client)
}

/// Creates and funds a milestone agreement whose first milestone is `1` (the
/// amount the agreement fixture creates it with), mirroring the setup used by
/// `tests/test_milestones.rs` so both suites build the same shape of agreement.
fn fund_milestone_agreement(
    env: &Env,
    client: &PayrollContractClient,
    employer: &Address,
    contributor: &Address,
    token: &Address,
) -> u128 {
    mint(env, token, employer, MILESTONE_FUND);
    let agreement_id = client.create_milestone_agreement(
        employer,
        contributor,
        token,
        &soroban_sdk::vec![env, 1i128],
    );
    client.fund_milestone_agreement(&agreement_id, employer, &MILESTONE_FUND);
    agreement_id
}

// ============================================================================
// PAYROLL FIXTURE
// ============================================================================

/// Registers the contract, initializes it, and returns `(contract_id, client)`.
fn setup_contract(env: &Env) -> (Address, PayrollContractClient<'static>) {
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (contract_id, client)
}

/// Seeds the `DataKey` storage the batch payroll path reads at runtime, and
/// mints the matching on-chain balance to the contract.
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

    if escrow_balance > 0 {
        mint(env, token, contract_id, escrow_balance);
    }
}

/// Builds an activated two-employee payroll agreement with the given escrow.
///
/// Returns `(client, token, agreement_id, employee)`, where `employee` is the
/// address registered at index `0` and the caller used by the tests below. The
/// address at index `1` belongs to someone else.
fn payroll_fixture(
    env: &Env,
    salary: i128,
    escrow: i128,
) -> (PayrollContractClient<'static>, Address, u128, Address) {
    let (contract_id, client) = setup_contract(env);
    let employer = Address::generate(env);
    let token = create_token(env);
    let employee = Address::generate(env);
    let other_employee = Address::generate(env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary);
    client.add_employee_to_agreement(&agreement_id, &other_employee, &salary);
    client.activate_agreement(&agreement_id);

    seed_payroll_claim_storage(
        env,
        &contract_id,
        agreement_id,
        &token,
        &[(employee.clone(), salary), (other_employee, salary)],
        escrow,
    );

    (client, token, agreement_id, employee)
}

// ============================================================================
// 1-4: MILESTONE BATCH SEMANTICS
// ============================================================================

/// A batch where every element is claimable settles every element and reports
/// one success per element. Milestone 1 is the agreement fixture's own
/// milestone (amount 1), 2 and 3 are added here (100 and 200).
#[test]
fn test_batch_milestones_fully_successful_batch_settles_every_element() {
    let (env, employer, contributor, token, client) = milestone_env();
    let agreement_id = fund_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);
    client.approve_milestone(&agreement_id, &3);

    let ids = soroban_sdk::vec![&env, 1u32, 2u32, 3u32];
    let result = client.batch_claim_milestones(&agreement_id, &ids);

    assert_eq!(result.successful_claims, 3);
    assert_eq!(result.failed_claims, 0);
    assert_eq!(result.total_claimed, 301);
    assert_eq!(result.results.len(), 3);

    let first = result.results.get(0).unwrap();
    assert_eq!(first.milestone_id, 1);
    assert!(first.success);
    assert_eq!(first.amount_claimed, 1);
    assert_eq!(first.error_code, MILESTONE_OK);

    let second = result.results.get(1).unwrap();
    assert_eq!(second.milestone_id, 2);
    assert_eq!(second.amount_claimed, 100);

    let third = result.results.get(2).unwrap();
    assert_eq!(third.milestone_id, 3);
    assert_eq!(third.amount_claimed, 200);

    assert_eq!(TokenClient::new(&env, &token).balance(&contributor), 301);
}

/// An unclaimable element at the front of the batch is reported per element and
/// the element behind it still settles, which is what proves the loop continues
/// rather than aborting the batch.
#[test]
fn test_batch_milestones_one_failing_element_does_not_abort_the_rest() {
    let (env, employer, contributor, token, client) = milestone_env();
    let agreement_id = fund_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    // Milestone 2 is approved; milestone 1 is deliberately left unapproved.
    client.approve_milestone(&agreement_id, &2);

    let ids = soroban_sdk::vec![&env, 1u32, 2u32];
    let result = client.batch_claim_milestones(&agreement_id, &ids);

    let failed = result.results.get(0).unwrap();
    assert_eq!(failed.milestone_id, 1);
    assert!(!failed.success);
    assert_eq!(failed.amount_claimed, 0);
    assert_eq!(failed.error_code, MILESTONE_NOT_APPROVED);

    let settled = result.results.get(1).unwrap();
    assert_eq!(settled.milestone_id, 2);
    assert!(settled.success);
    assert_eq!(settled.amount_claimed, 100);

    assert_eq!(result.successful_claims, 1);
    assert_eq!(result.failed_claims, 1);
    assert_eq!(result.total_claimed, 100);
    assert_eq!(TokenClient::new(&env, &token).balance(&contributor), 100);
}

/// The retry contract for a partially settled batch: the result identifies the
/// outstanding element, retrying the remainder pays exactly the remainder, and
/// retrying the whole original batch pays nothing a second time.
#[test]
fn test_batch_milestones_retry_of_remainder_does_not_double_pay() {
    let (env, employer, contributor, token, client) = milestone_env();
    let agreement_id = fund_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);
    // Milestone 3 is deliberately left unapproved, so the first batch is partial.

    let ids = soroban_sdk::vec![&env, 1u32, 2u32, 3u32];
    let first = client.batch_claim_milestones(&agreement_id, &ids);
    assert_eq!(first.successful_claims, 2);
    assert_eq!(first.failed_claims, 1);
    assert_eq!(first.total_claimed, 101);

    let token_client = TokenClient::new(&env, &token);
    assert_eq!(token_client.balance(&contributor), 101);

    // The caller can read which element is outstanding from the batch result
    // instead of re-reading per-element state.
    let outstanding = first.results.get(2).unwrap();
    assert_eq!(outstanding.milestone_id, 3);
    assert!(!outstanding.success);
    assert_eq!(outstanding.amount_claimed, 0);
    assert_eq!(outstanding.error_code, MILESTONE_NOT_APPROVED);

    // Retry only the remainder.
    client.approve_milestone(&agreement_id, &3);
    let remainder = client.batch_claim_milestones(&agreement_id, &soroban_sdk::vec![&env, 3u32]);
    assert_eq!(remainder.successful_claims, 1);
    assert_eq!(remainder.failed_claims, 0);
    assert_eq!(remainder.total_claimed, 200);
    assert_eq!(token_client.balance(&contributor), 301);

    // A naive retry of the whole original batch must be a no-op in value terms:
    // every element reports "already claimed" and nothing moves.
    let retry = client.batch_claim_milestones(&agreement_id, &ids);
    assert_eq!(retry.successful_claims, 0);
    assert_eq!(retry.failed_claims, 3);
    assert_eq!(retry.total_claimed, 0);
    assert_eq!(retry.results.len(), 3);

    let retried_first = retry.results.get(0).unwrap();
    assert!(!retried_first.success);
    assert_eq!(retried_first.amount_claimed, 0);
    assert_eq!(retried_first.error_code, MILESTONE_ALREADY_CLAIMED);

    let retried_second = retry.results.get(1).unwrap();
    assert_eq!(retried_second.error_code, MILESTONE_ALREADY_CLAIMED);

    let retried_third = retry.results.get(2).unwrap();
    assert_eq!(retried_third.error_code, MILESTONE_ALREADY_CLAIMED);

    assert_eq!(token_client.balance(&contributor), 301);
}

// ============================================================================
// 5-8: PAYROLL BATCH SEMANTICS
// ============================================================================

/// A one-element payroll batch is the only shape that can be fully successful:
/// the batch enforces `caller == employee` per element, so a caller can only
/// ever settle their own index. The success accounting and balance effect are
/// asserted here so the fully successful case is covered for payroll too.
#[test]
fn test_batch_payroll_single_element_batch_is_fully_successful() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, token, agreement_id, employee) = payroll_fixture(&env, SALARY, 2 * SALARY);
    advance_time(&env, ONE_DAY + 1);

    let batch = Vec::from_array(&env, [0u32]);
    let result = client.batch_claim_payroll(&employee, &agreement_id, &batch);

    assert_eq!(result.agreement_id, agreement_id);
    assert_eq!(result.successful_claims, 1);
    assert_eq!(result.failed_claims, 0);
    assert_eq!(result.total_claimed, SALARY);

    let only = result.results.get(0).unwrap();
    assert_eq!(only.employee_index, 0);
    assert!(only.success);
    assert_eq!(only.amount_claimed, SALARY);
    assert_eq!(only.error_code, 0);

    assert_eq!(TokenClient::new(&env, &token).balance(&employee), SALARY);
}

/// A failing element in front of a claimable one is reported per element and the
/// claimable element still settles.
///
/// Index `1` belongs to another address, so the caller is rejected for it with
/// `Unauthorized`; index `0` is the caller's own and must still be paid.
#[test]
fn test_batch_payroll_one_failing_element_does_not_abort_the_rest() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, token, agreement_id, employee) = payroll_fixture(&env, SALARY, 2 * SALARY);
    advance_time(&env, ONE_DAY + 1);

    // The failing index comes first on purpose: a successful second element is
    // what shows the batch continued past the failure.
    let batch = Vec::from_array(&env, [1u32, 0u32]);
    let result = client.batch_claim_payroll(&employee, &agreement_id, &batch);

    let rejected = result.results.get(0).unwrap();
    assert_eq!(rejected.employee_index, 1);
    assert!(!rejected.success);
    assert_eq!(rejected.amount_claimed, 0);
    assert_eq!(rejected.error_code, PayrollError::Unauthorized as u32);

    let settled = result.results.get(1).unwrap();
    assert_eq!(settled.employee_index, 0);
    assert!(settled.success);
    assert_eq!(settled.amount_claimed, SALARY);

    assert_eq!(result.successful_claims, 1);
    assert_eq!(result.failed_claims, 1);
    assert_eq!(result.total_claimed, SALARY);
    assert_eq!(TokenClient::new(&env, &token).balance(&employee), SALARY);
}

/// Retrying a payroll batch before anything new becomes claimable settles
/// nothing and moves no tokens: the already-claimed element reports
/// `NoPeriodsToClaim` rather than being paid again.
#[test]
fn test_batch_payroll_retry_before_new_period_settles_nothing() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, token, agreement_id, employee) = payroll_fixture(&env, SALARY, 2 * SALARY);
    advance_time(&env, ONE_DAY + 1);

    let token_client = TokenClient::new(&env, &token);
    let batch = Vec::from_array(&env, [0u32]);

    let first = client.batch_claim_payroll(&employee, &agreement_id, &batch);
    assert_eq!(first.successful_claims, 1);
    assert_eq!(first.total_claimed, SALARY);
    assert_eq!(token_client.balance(&employee), SALARY);

    // Same batch, same call, no time has passed.
    let retry = client.batch_claim_payroll(&employee, &agreement_id, &batch);
    assert_eq!(retry.successful_claims, 0);
    assert_eq!(retry.failed_claims, 1);
    assert_eq!(retry.total_claimed, 0);

    let only = retry.results.get(0).unwrap();
    assert_eq!(only.employee_index, 0);
    assert!(!only.success);
    assert_eq!(only.amount_claimed, 0);
    assert_eq!(only.error_code, PayrollError::NoPeriodsToClaim as u32);

    assert_eq!(token_client.balance(&employee), SALARY);
}

/// Once a new period has elapsed, retrying the same batch pays exactly the new
/// period and not the one already settled.
#[test]
fn test_batch_payroll_retry_after_new_period_pays_that_period_only() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, token, agreement_id, employee) = payroll_fixture(&env, SALARY, 2 * SALARY);
    advance_time(&env, ONE_DAY + 1);

    let token_client = TokenClient::new(&env, &token);
    let batch = Vec::from_array(&env, [0u32]);

    let first = client.batch_claim_payroll(&employee, &agreement_id, &batch);
    assert_eq!(first.total_claimed, SALARY);
    assert_eq!(token_client.balance(&employee), SALARY);

    advance_time(&env, ONE_DAY);

    let remainder = client.batch_claim_payroll(&employee, &agreement_id, &batch);
    assert_eq!(remainder.successful_claims, 1);
    assert_eq!(remainder.failed_claims, 0);
    // Only the newly elapsed period, not the one already claimed.
    assert_eq!(remainder.total_claimed, SALARY);
    assert_eq!(
        remainder.results.get(0).unwrap().amount_claimed,
        SALARY,
        "a retry must pay the remaining period only, never the settled one again"
    );
    assert_eq!(token_client.balance(&employee), 2 * SALARY);
}

/// The batch size limit is enforced at the documented boundary. Enforcement
/// itself is covered by `tests/test_payment_failures.rs`
/// (`test_batch_payroll_over_limit_rejected_before_agreement_lookup` and
/// `test_batch_milestone_over_limit_rejected_before_claims`); this keeps the
/// number stated in `docs/batch-payments.md` from drifting away from the
/// constant the code enforces.
#[test]
fn test_batch_size_limit_matches_the_documented_value() {
    assert_eq!(MAX_BATCH_SIZE, 20);
}
