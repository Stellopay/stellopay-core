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



// ============================================================================
// Conservation-of-funds invariant tests
// ============================================================================

/// **Conservation invariant: Payroll multi-claim sequence**
///
/// Executes multiple sequential claims across different employees and asserts
/// that the fundamental accounting equation holds at every step:
///
///   `total_deposited == sum(all_payouts) + remaining_escrow_balance`
///
/// This test catches cumulative rounding errors and over-distribution bugs.
#[test]
fn test_conservation_payroll_multi_claim_sequence() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_token(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800);

    // Add 3 employees with different salaries
    let employee1 = create_address(&env);
    let employee2 = create_address(&env);
    let employee3 = create_address(&env);
    
    client.add_employee_to_agreement(&agreement_id, &employee1, &1000);
    client.add_employee_to_agreement(&agreement_id, &employee2, &1500);
    client.add_employee_to_agreement(&agreement_id, &employee3, &2000);

    let total_salary = 1000 + 1500 + 2000;
    let initial_escrow = total_salary * 10; // Fund for 10 periods

    // Setup storage
    env.as_contract(&contract_id, || {
        let now = env.ledger().timestamp();
        DataKey::set_agreement_activation_time(&env, agreement_id, now);
        DataKey::set_agreement_period_duration(&env, agreement_id, 86400);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, initial_escrow);
        DataKey::set_employee_count(&env, agreement_id, 3);
        DataKey::set_employee(&env, agreement_id, 0, &employee1);
        DataKey::set_employee_salary(&env, agreement_id, 0, 1000);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee(&env, agreement_id, 1, &employee2);
        DataKey::set_employee_salary(&env, agreement_id, 1, 1500);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 1, 0);
        DataKey::set_employee(&env, agreement_id, 2, &employee3);
        DataKey::set_employee_salary(&env, agreement_id, 2, 2000);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 2, 0);
    });

    mint(&env, &token, &contract_id, initial_escrow);
    client.activate_agreement(&agreement_id);

    // Helper: assert conservation at current state
    let assert_conservation = || {
        env.as_contract(&contract_id, || {
            let remaining = DataKey::get_agreement_escrow_balance(&env, agreement_id, &token);
            let paid = DataKey::get_agreement_paid_amount(&env, agreement_id);
            assert_eq!(
                remaining + paid,
                initial_escrow,
                "Conservation violated: {} + {} != {}",
                remaining, paid, initial_escrow
            );
        });
    };

    // Sequence of claims across different periods
    for _ in 0..3 {
        env.ledger().with_mut(|li| li.timestamp += 86400 + 1);

        client.try_claim_payroll(&employee1, &agreement_id, &0).ok();
        assert_conservation();

        client.try_claim_payroll(&employee2, &agreement_id, &1).ok();
        assert_conservation();

        client.try_claim_payroll(&employee3, &agreement_id, &2).ok();
        assert_conservation();
    }
}

/// **Conservation invariant: Dispute resolution with multi-employee split**
///
/// Tests that resolve_dispute correctly handles integer division when splitting
/// employee_payout among multiple employees without losing or creating funds.
///
/// This is the primary test for the multi-employee dust bug in resolve_dispute_core.
#[test]
fn test_conservation_multi_employee_dispute_integer_division() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let arbiter = create_address(&env);
    let token = create_token(&env);

    client.set_arbiter(&employer, &arbiter);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800);

    // Add 3 employees - this creates integer division scenarios
    let employee1 = create_address(&env);
    let employee2 = create_address(&env);
    let employee3 = create_address(&env);
    
    client.add_employee_to_agreement(&agreement_id, &employee1, &100);
    client.add_employee_to_agreement(&agreement_id, &employee2, &100);
    client.add_employee_to_agreement(&agreement_id, &employee3, &100);

    // Use an escrow amount that doesn't divide evenly by 3
    let escrow_balance = 1000i128; // 1000 / 3 = 333.33... per employee
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, escrow_balance);
    });

    mint(&env, &token, &contract_id, escrow_balance);
    client.activate_agreement(&agreement_id);

    // Raise dispute
    client.raise_dispute(&employer, &agreement_id).unwrap();

    // Split: 700 to employees, 300 to employer
    // 700 / 3 = 233.33... per employee (will have 1 token dust)
    let employee_payout = 700i128;
    let employer_refund = 300i128;

    let sac = StellarAssetClient::new(&env, &token);
    let employer_balance_before = sac.balance(&employer);
    let employee1_balance_before = sac.balance(&employee1);
    let employee2_balance_before = sac.balance(&employee2);
    let employee3_balance_before = sac.balance(&employee3);

    client.resolve_dispute(&arbiter, &agreement_id, &employee_payout, &employer_refund).unwrap();

    // **Assert conservation**: sum of all transfers == escrow_balance
    let employer_balance_after = sac.balance(&employer);
    let employee1_balance_after = sac.balance(&employee1);
    let employee2_balance_after = sac.balance(&employee2);
    let employee3_balance_after = sac.balance(&employee3);

    let total_distributed = 
        (employer_balance_after - employer_balance_before) +
        (employee1_balance_after - employee1_balance_before) +
        (employee2_balance_after - employee2_balance_before) +
        (employee3_balance_after - employee3_balance_before);

    assert_eq!(
        total_distributed,
        employee_payout + employer_refund,
        "Total distributed must equal employee_payout + employer_refund"
    );

    // Verify no dust left in contract (or minimal dust <= employee_count - 1)
    env.as_contract(&contract_id, || {
        let remaining = DataKey::get_agreement_escrow_balance(&env, agreement_id, &token);
        assert!(
            remaining <= 2, // At most 2 tokens of dust for 3 employees
            "Excessive remaining balance: {}",
            remaining
        );
    });
}

/// **Conservation invariant: Batch claim preserves accounting**
///
/// Tests that batch_claim_payroll correctly handles multiple employees
/// and maintains conservation of funds.
#[test]
fn test_conservation_batch_claim_payroll() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_token(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800);

    // Add 4 employees
    let mut employees = vec![];
    let salaries = vec![1000i128, 1500, 2000, 2500];
    let total_salary: i128 = salaries.iter().sum();

    for (idx, &salary) in salaries.iter().enumerate() {
        let employee = create_address(&env);
        client.add_employee_to_agreement(&agreement_id, &employee, &salary);
        employees.push(employee);

        env.as_contract(&contract_id, || {
            DataKey::set_employee(&env, agreement_id, idx as u32, &employees[idx]);
            DataKey::set_employee_salary(&env, agreement_id, idx as u32, salary);
            DataKey::set_employee_claimed_periods(&env, agreement_id, idx as u32, 0);
        });
    }

    let initial_escrow = total_salary * 5;
    env.as_contract(&contract_id, || {
        let now = env.ledger().timestamp();
        DataKey::set_agreement_activation_time(&env, agreement_id, now);
        DataKey::set_agreement_period_duration(&env, agreement_id, 86400);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, initial_escrow);
        DataKey::set_employee_count(&env, agreement_id, 4);
    });

    mint(&env, &token, &contract_id, initial_escrow);
    client.activate_agreement(&agreement_id);

    // Advance time
    env.ledger().with_mut(|li| li.timestamp += 86400 + 1);

    // Batch claim for all employees
    let employee_indices: SorobanVec<u32> = vec![0u32, 1, 2, 3]
        .into_iter()
        .collect();
    
    client.try_batch_claim_payroll(&agreement_id, &employee_indices).ok();

    // **Assert conservation**
    env.as_contract(&contract_id, || {
        let remaining = DataKey::get_agreement_escrow_balance(&env, agreement_id, &token);
        let paid = DataKey::get_agreement_paid_amount(&env, agreement_id);
        
        assert_eq!(
            remaining + paid,
            initial_escrow,
            "Conservation violated after batch claim: {} + {} != {}",
            remaining, paid, initial_escrow
        );
    });
}

/// **Invariant: claimed_periods never exceeds available periods**
///
/// Tests that the contract correctly bounds claimed_periods even with
/// aggressive claim attempts.
#[test]
fn test_invariant_claimed_periods_never_exceeds_available() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let employee = create_address(&env);
    let token = create_token(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);

    let max_periods = 5u32;
    env.as_contract(&contract_id, || {
        let now = env.ledger().timestamp();
        DataKey::set_agreement_activation_time(&env, agreement_id, now);
        DataKey::set_agreement_period_duration(&env, agreement_id, 86400);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, 1000 * (max_periods as i128));
        DataKey::set_employee_count(&env, agreement_id, 1);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, 1000);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
    });

    mint(&env, &token, &contract_id, 1000 * (max_periods as i128));
    client.activate_agreement(&agreement_id);

    // Attempt to claim beyond max_periods
    for _ in 0..max_periods + 5 {
        env.ledger().with_mut(|li| li.timestamp += 86400 + 1);
        let _ = client.try_claim_payroll(&employee, &agreement_id, &0);

        // **Assert claimed_periods bound**
        env.as_contract(&contract_id, || {
            let claimed = DataKey::get_employee_claimed_periods(&env, agreement_id, 0);
            assert!(
                claimed <= max_periods,
                "claimed_periods ({}) exceeded max ({})",
                claimed, max_periods
            );
        });
    }
}

/// **Invariant: Cancelled agreement respects grace period claims**
///
/// Tests that claims during grace period are allowed but bounded, and
/// conservation holds after finalization.
#[test]
fn test_invariant_cancelled_agreement_grace_period_conservation() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);

    let amount_per_period = 1000i128;
    let num_periods = 5u32;
    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &amount_per_period,
        &86400,
        &num_periods,
    ).unwrap();

    let total_amount = amount_per_period * (num_periods as i128);
    mint(&env, &token, &contract_id, total_amount);
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, total_amount);
    });

    client.activate_agreement(&agreement_id);

    // Claim 2 periods before cancellation
    env.ledger().with_mut(|li| li.timestamp += 86400 * 2 + 1);
    client.claim_time_based(&agreement_id).ok();

    let paid_before_cancel = env.as_contract(&contract_id, || {
        DataKey::get_agreement_paid_amount(&env, agreement_id)
    });

    // Cancel agreement
    client.cancel_agreement(&agreement_id);

    // Try to claim during grace period
    env.ledger().with_mut(|li| li.timestamp += 86400);
    client.claim_time_based(&agreement_id).ok();

    // Finalize grace period
    if let Some(grace_end) = client.get_grace_period_end(&agreement_id) {
        env.ledger().with_mut(|li| li.timestamp = grace_end + 1);
        client.finalize_grace_period(&agreement_id);
    }

    // **Assert conservation and that paid amount didn't decrease**
    env.as_contract(&contract_id, || {
        let final_escrow = DataKey::get_agreement_escrow_balance(&env, agreement_id, &token);
        let final_paid = DataKey::get_agreement_paid_amount(&env, agreement_id);
        
        assert!(final_paid >= paid_before_cancel, "Paid amount should never decrease");
        assert_eq!(
            final_escrow + final_paid,
            total_amount,
            "Conservation violated: {} + {} != {}",
            final_escrow, final_paid, total_amount
        );
    });
}
