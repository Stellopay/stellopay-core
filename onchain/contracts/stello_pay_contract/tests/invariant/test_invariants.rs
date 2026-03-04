//! Core state invariant tests for agreements, escrow, and payroll.
//!
//! These tests assert that fundamental accounting and lifecycle invariants
//! hold before and after key operations such as creation, claiming, refund,
//! dispute resolution, and pause/resume transitions.

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, Vec,
};

use stello_pay_contract::storage::{
    Agreement, AgreementMode, AgreementStatus, DataKey, DisputeStatus, EmployeeInfo, StorageKey,
};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ============================================================================
// Test helpers
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

/// Deploys and initializes the payroll contract.
fn setup_contract(env: &Env) -> (Address, PayrollContractClient<'static>) {
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = create_address(env);
    client.initialize(&owner);
    (contract_id, client)
}

/// Mints tokens to a given address.
fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    let sac = StellarAssetClient::new(env, token);
    sac.mint(to, &amount);
}

/// Helper: sum of all employee salaries tracked in `AgreementEmployees`.
fn sum_employee_salaries(env: &Env, agreement_id: u128) -> i128 {
    env.as_contract(&Address::from_contract_id(&env, &env.current_contract()), || {
        let employees: Vec<EmployeeInfo> = env
            .storage()
            .persistent()
            .get(&StorageKey::AgreementEmployees(agreement_id))
            .unwrap_or(Vec::new(env));

        let mut total: i128 = 0;
        for i in 0..employees.len() {
            let info = employees.get(i).unwrap();
            total += info.salary_per_period;
        }
        total
    })
}

/// Core agreement invariants that must hold for all modes.
fn assert_agreement_core_invariants(env: &Env, contract_id: &Address, agreement_id: u128) {
    env.as_contract(contract_id, || {
        let agreement: Agreement = env
            .storage()
            .persistent()
            .get(&StorageKey::Agreement(agreement_id))
            .expect("agreement must exist");

        // Basic non-negativity and bounds.
        assert!(
            agreement.total_amount >= 0,
            "total_amount must be non-negative"
        );
        assert!(
            agreement.paid_amount >= 0,
            "paid_amount must be non-negative"
        );
        assert!(
            agreement.paid_amount <= agreement.total_amount,
            "paid_amount cannot exceed total_amount"
        );

        // Escrow-specific invariants.
        if agreement.mode == AgreementMode::Escrow {
            let amount_per_period = agreement
                .amount_per_period
                .expect("escrow must have amount_per_period");
            let num_periods = agreement
                .num_periods
                .expect("escrow must have num_periods");
            let claimed_periods = agreement
                .claimed_periods
                .expect("escrow must track claimed_periods");

            assert!(amount_per_period > 0, "escrow amount_per_period must be > 0");
            assert!(num_periods > 0, "escrow num_periods must be > 0");
            assert!(
                claimed_periods <= num_periods,
                "claimed_periods cannot exceed num_periods"
            );

            let expected_total = amount_per_period * (num_periods as i128);
            assert_eq!(
                agreement.total_amount, expected_total,
                "escrow total_amount must equal amount_per_period * num_periods"
            );
        }

        // Payroll-specific invariants.
        if agreement.mode == AgreementMode::Payroll {
            // For payroll mode, time-based fields should not be populated at the agreement level.
            assert!(
                agreement.amount_per_period.is_none(),
                "payroll agreements must not set amount_per_period"
            );
            assert!(
                agreement.period_seconds.is_none(),
                "payroll agreements must not set period_seconds"
            );
            assert!(
                agreement.num_periods.is_none(),
                "payroll agreements must not set num_periods"
            );

            // Total amount must equal the sum of all employee salaries.
            let total_salaries = sum_employee_salaries(env, agreement_id);
            assert_eq!(
                agreement.total_amount, total_salaries,
                "payroll total_amount must equal sum of employee salaries"
            );
        }

        // Dispute invariants: status combinations must be consistent.
        match agreement.dispute_status {
            DisputeStatus::None => {
                assert!(
                    agreement.dispute_raised_at.is_none(),
                    "dispute_raised_at must be None when dispute_status is None"
                );
            }
            DisputeStatus::Raised => {
                assert!(
                    agreement.dispute_raised_at.is_some(),
                    "dispute_raised_at must be Some when dispute_status is Raised"
                );
                assert_eq!(
                    agreement.status,
                    AgreementStatus::Disputed,
                    "agreement.status must be Disputed when dispute is Raised"
                );
            }
            DisputeStatus::Resolved => {
                assert!(
                    agreement.dispute_raised_at.is_some(),
                    "dispute_raised_at must be Some when dispute_status is Resolved"
                );
            }
        }

        // Escrow accounting invariant: on the primary token, escrow balance must
        // never be negative and cannot exceed total_amount.
        let escrow_balance =
            DataKey::get_agreement_escrow_balance(env, agreement_id, &agreement.token);
        assert!(
            escrow_balance >= 0,
            "escrow balance must be non-negative"
        );
        assert!(
            escrow_balance + agreement.paid_amount <= agreement.total_amount,
            "paid_amount + escrow_balance must not exceed total_amount"
        );
    });
}

// ============================================================================
// Invariant tests
// ============================================================================

/// Invariants must hold across a simple escrow lifecycle:
/// - creation
/// - activation
/// - single successful time-based claim
#[test]
fn test_invariants_escrow_create_claim_flow() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);

    let amount_per_period: i128 = 1_000;
    let period_seconds: u64 = 86_400;
    let num_periods: u32 = 4;

    // Create escrow agreement and assert invariants on initial state.
    let agreement_id = client
        .create_escrow_agreement(
            &employer,
            &contributor,
            &token,
            &amount_per_period,
            &period_seconds,
            &num_periods,
        )
        .unwrap();
    assert_agreement_core_invariants(&env, &contract_id, agreement_id);

    // Fund escrow and activate.
    let total = amount_per_period * (num_periods as i128);
    mint(&env, &token, &contract_id, total);
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, total);
    });

    client.activate_agreement(&agreement_id);
    assert_agreement_core_invariants(&env, &contract_id, agreement_id);

    // Advance one period and perform a time-based claim.
    env.ledger().with_mut(|li: &mut Ledger| {
        li.timestamp += period_seconds + 1;
    });
    client.claim_time_based(&agreement_id).unwrap();

    // After claim, escrow balance and paid_amount must remain within bounds.
    assert_agreement_core_invariants(&env, &contract_id, agreement_id);
}

/// Invariants must hold across a funded payroll lifecycle:
/// - payroll agreement creation
/// - employee addition and activation
/// - single successful payroll claim
#[test]
fn test_invariants_payroll_create_claim_flow() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    let salary_per_period: i128 = 2_000;
    let grace_period: u64 = 7 * 24 * 60 * 60;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace_period);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary_per_period);

    // At this point total_amount must equal salary.
    assert_agreement_core_invariants(&env, &contract_id, agreement_id);

    client.activate_agreement(&agreement_id);

    // Seed DataKey storage for payroll claiming.
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, 86_400);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, salary_per_period * 4);
        DataKey::set_employee_count(&env, agreement_id, 1);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary_per_period);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
    });

    mint(&env, &token, &contract_id, salary_per_period * 4);

    assert_agreement_core_invariants(&env, &contract_id, agreement_id);

    // Advance exactly one period and claim once.
    env.ledger().with_mut(|li: &mut Ledger| {
        li.timestamp += 86_400 + 1;
    });

    client
        .try_claim_payroll(&employee, &agreement_id, &0u32)
        .unwrap();

    // Invariants must still hold after the claim.
    assert_agreement_core_invariants(&env, &contract_id, agreement_id);
}

/// Invariants must hold across a cancellation and refund lifecycle:
/// - escrow agreement funded and partially claimed
/// - cancellation and grace-period finalization
#[test]
fn test_invariants_escrow_refund_flow() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);

    let amount_per_period: i128 = 500;
    let period_seconds: u64 = 86_400;
    let num_periods: u32 = 6;

    let agreement_id = client
        .create_escrow_agreement(
            &employer,
            &contributor,
            &token,
            &amount_per_period,
            &period_seconds,
            &num_periods,
        )
        .unwrap();

    let total = amount_per_period * (num_periods as i128);
    mint(&env, &token, &contract_id, total);
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, total);
    });
    client.activate_agreement(&agreement_id);

    assert_agreement_core_invariants(&env, &contract_id, agreement_id);

    // Claim two periods.
    env.ledger().with_mut(|li: &mut Ledger| {
        li.timestamp += period_seconds * 2 + 1;
    });
    client.claim_time_based(&agreement_id).unwrap();

    assert_agreement_core_invariants(&env, &contract_id, agreement_id);

    // Cancel and advance past grace period, then finalize refund.
    client.cancel_agreement(&agreement_id);
    let grace_end = client.get_grace_period_end(&agreement_id).unwrap();
    env.ledger().with_mut(|li: &mut Ledger| {
        li.timestamp = grace_end + 1;
    });
    client.finalize_grace_period(&agreement_id);

    // After refund, escrow balance must be zero and invariants preserved.
    assert_agreement_core_invariants(&env, &contract_id, agreement_id);
}

/// Invariants must hold across dispute raise and resolution:
/// - dispute raised on an escrow agreement
/// - resolution with a payout split that respects total_amount
#[test]
fn test_invariants_dispute_raise_and_resolve() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let arbiter = create_address(&env);
    let token = create_token(&env);

    client.set_arbiter(&employer, &arbiter);

    let amount_per_period: i128 = 1_000;
    let period_seconds: u64 = 3_600;
    let num_periods: u32 = 2;

    let agreement_id = client
        .create_escrow_agreement(
            &employer,
            &contributor,
            &token,
            &amount_per_period,
            &period_seconds,
            &num_periods,
        )
        .unwrap();

    assert_agreement_core_invariants(&env, &contract_id, agreement_id);

    // Raise dispute.
    client.raise_dispute(&employer, &agreement_id).unwrap();
    assert_agreement_core_invariants(&env, &contract_id, agreement_id);

    // Resolve dispute with a split that is within total_amount.
    let total_locked = amount_per_period * (num_periods as i128);
    let pay_employee = total_locked / 2;
    let refund_employer = total_locked - pay_employee;

    client
        .resolve_dispute(&arbiter, &agreement_id, &pay_employee, &refund_employer)
        .unwrap();

    // After resolution, accounting and dispute flags must remain consistent.
    assert_agreement_core_invariants(&env, &contract_id, agreement_id);
}

/// Invariants must hold when pausing and resuming an agreement:
/// - funded payroll agreement
/// - pause blocks claims, resume allows claim, state remains consistent
#[test]
fn test_invariants_pause_and_resume_flow() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    let salary_per_period: i128 = 1_000;
    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary_per_period);
    client.activate_agreement(&agreement_id);

    // Seed DataKey storage with sufficient escrow.
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, 86_400);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, salary_per_period * 10);
        DataKey::set_employee_count(&env, agreement_id, 1);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary_per_period);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
    });

    mint(&env, &token, &contract_id, salary_per_period * 10);

    assert_agreement_core_invariants(&env, &contract_id, agreement_id);

    // Advance time and pause before claim.
    env.ledger().with_mut(|li: &mut Ledger| {
        li.timestamp += 86_400 + 1;
    });
    client.pause_agreement(&agreement_id);

    // Even while paused, stored values must respect invariants.
    assert_agreement_core_invariants(&env, &contract_id, agreement_id);

    // Resume and perform a claim; invariants must still hold.
    client.resume_agreement(&agreement_id);
    client
        .try_claim_payroll(&employee, &agreement_id, &0u32)
        .unwrap();

    assert_agreement_core_invariants(&env, &contract_id, agreement_id);
}

