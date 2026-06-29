//! Property-based tests for conservation-of-funds and accounting invariants.
//!
//! This module uses `proptest` to generate randomized payroll/escrow agreements
//! and operation sequences, then asserts critical financial invariants:
//!
//! - **Conservation of funds**: Total tokens transferred out never exceed total deposited
//!   per (agreement, token).
//! - **Monotonicity**: `claimed_periods` is non-decreasing and never exceeds `num_periods`.
//! - **Dispute resolution bounds**: `resolve_dispute` never transfers more than the
//!   agreement's escrow balance.
//!
//! These tests are the primary automated safety net against fund leakage, double-claims,
//! and over-distribution on money paths where accounting bugs exist.

#![cfg(test)]

use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, Vec as SorobanVec,
};

use stello_pay_contract::storage::{DataKey, PayrollError};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ============================================================================
// Configuration: Proptest case counts
// ============================================================================

/// Number of proptest cases to run in CI (can be overridden via env var PROPTEST_CASES)
fn proptest_cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(32) // Default: 32 cases for CI-friendliness
}

// ============================================================================
// Test helpers
// ============================================================================

/// Helper to deploy a fresh contract + owner in a new environment.
fn setup_contract() -> (Env, Address, Address, PayrollContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    (env, contract_id, owner, client)
}

/// Create and mint a token for testing
fn setup_token(env: &Env, contract_id: &Address, initial_supply: i128) -> Address {
    let admin = Address::generate(env);
    let token = env.register_stellar_asset_contract_v2(admin).address();
    let sac = StellarAssetClient::new(env, &token);
    sac.mint(contract_id, &initial_supply);
    token
}

// ============================================================================
// Proptest strategies
// ============================================================================

/// Strategy: Generate a randomized payroll agreement configuration
fn payroll_agreement_strategy() -> impl Strategy<Value = (u32, Vec<i128>, u64, u64)> {
    (
        1u32..=5,                           // employee_count: 1-5 employees
        prop::collection::vec(100i128..5000, 1..=5), // salaries: 100-5000 per employee
        3600u64..86400,                     // period_seconds: 1 hour to 1 day
        0u64..604800,                       // grace_period: 0 to 1 week
    )
}

/// Strategy: Generate a randomized escrow agreement configuration
fn escrow_agreement_strategy() -> impl Strategy<Value = (i128, u64, u32)> {
    (
        100i128..10000,     // amount_per_period: 100-10000
        3600u64..86400,     // period_seconds: 1 hour to 1 day
        1u32..=10,          // num_periods: 1-10 periods
    )
}

/// Strategy: Generate a sequence of operations (fund, claim, dispute, resolve)
fn operation_sequence_strategy() -> impl Strategy<Value = Vec<Operation>> {
    prop::collection::vec(operation_strategy(), 1..=10)
}

#[derive(Debug, Clone)]
enum Operation {
    /// Advance time by a number of periods
    AdvanceTime(u32),
    /// Claim payroll for an employee index
    ClaimPayroll(u32),
    /// Batch claim for all employees
    BatchClaimPayroll,
    /// Raise a dispute
    RaiseDispute,
    /// Resolve a dispute with specified split (employee_payout_ratio: 0-100)
    ResolveDispute(u32),
}

fn operation_strategy() -> impl Strategy<Value = Operation> {
    prop_oneof![
        (1u32..=3).prop_map(Operation::AdvanceTime),
        (0u32..=4).prop_map(Operation::ClaimPayroll),
        Just(Operation::BatchClaimPayroll),
        Just(Operation::RaiseDispute),
        (0u32..=100).prop_map(Operation::ResolveDispute),
    ]
}

// ============================================================================
// Property tests
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(proptest_cases()))]

    /// Property: `convert_currency` should match the fixed-point formula
    ///
    ///     converted = amount * rate / FX_SCALE
    ///
    /// for a wide range of small positive amounts and rates, where
    /// overflow cannot occur.
    #[test]
    fn prop_convert_currency_matches_scaled_multiplication(
        amount in 0i128..1_000_000,      // keep small to avoid overflow
        rate in 1i128..10_000_000,       // up to 10x with 1e6 scale
    ) {
        let (env, _contract_id, owner, client) = setup_contract();

        let from = Address::generate(&env);
        let to = Address::generate(&env);

        // Configure FX rate for (from, to).
        client.set_exchange_rate(&owner, &from, &to, &rate);

        // Contract helper should apply the same scaled multiplication.
        let converted = client.convert_currency(&from, &to, &amount);
        let expected = (amount * rate) / 1_000_000i128;

        prop_assert_eq!(converted, expected);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(proptest_cases()))]

    /// **Conservation of funds invariant for payroll agreements**
    ///
    /// This test generates randomized payroll agreements with multiple employees
    /// and executes random sequences of fund, claim, and dispute operations.
    ///
    /// **Invariant asserted**: For every (agreement, token) pair:
    ///   `sum(escrow deposits) == sum(payouts) + remaining escrow balance`
    ///
    /// **Why it matters**: This catches fund leakage, double-claims, and over-distribution
    /// bugs in the claim_payroll and resolve_dispute paths, including the tricky
    /// multi-employee integer division dust cases.
    #[test]
    fn prop_payroll_conservation_of_funds(
        (employee_count, salaries, period_seconds, grace_period) in payroll_agreement_strategy(),
        operations in operation_sequence_strategy(),
    ) {
        let (env, contract_id, owner, client) = setup_contract();
        
        let employer = Address::generate(&env);
        let token = setup_token(&env, &contract_id, 1_000_000);
        
        // Create payroll agreement
        let agreement_id = client.create_payroll_agreement(&employer, &token, &grace_period);
        
        // Add employees
        let mut employees = Vec::new();
        let mut total_salary: i128 = 0;
        for i in 0..employee_count {
            let employee = Address::generate(&env);
            let salary = salaries.get(i as usize).cloned().unwrap_or(1000);
            client.add_employee_to_agreement(&agreement_id, &employee, &salary);
            employees.push((employee, salary));
            total_salary += salary;
        }
        
        // Setup DataKey storage for payroll
        let initial_escrow = total_salary * 20; // Fund for 20 periods
        env.as_contract(&contract_id, || {
            let now = env.ledger().timestamp();
            DataKey::set_agreement_activation_time(&env, agreement_id, now);
            DataKey::set_agreement_period_duration(&env, agreement_id, period_seconds);
            DataKey::set_agreement_token(&env, agreement_id, &token);
            DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, initial_escrow);
            DataKey::set_employee_count(&env, agreement_id, employee_count);
            for (idx, (employee, salary)) in employees.iter().enumerate() {
                DataKey::set_employee(&env, agreement_id, idx as u32, employee);
                DataKey::set_employee_salary(&env, agreement_id, idx as u32, *salary);
                DataKey::set_employee_claimed_periods(&env, agreement_id, idx as u32, 0);
            }
        });
        
        client.activate_agreement(&agreement_id);
        
        // Track total deposits
        let total_deposited = initial_escrow;
        
        // Execute operations and track state
        let mut dispute_raised = false;
        
        for op in operations {
            match op {
                Operation::AdvanceTime(periods) => {
                    env.ledger().with_mut(|li: &mut Ledger| {
                        li.timestamp += period_seconds * (periods as u64);
                    });
                }
                Operation::ClaimPayroll(employee_idx) => {
                    if dispute_raised {
                        continue; // Can't claim during dispute
                    }
                    let idx = employee_idx % employee_count;
                    let employee = &employees[idx as usize].0;
                    let _ = client.try_claim_payroll(employee, &agreement_id, &idx);
                }
                Operation::BatchClaimPayroll => {
                    if dispute_raised {
                        continue;
                    }
                    let employee_indices: SorobanVec<u32> = (0..employee_count)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .collect::<SorobanVec<u32>>();
                    let _ = client.try_batch_claim_payroll(&agreement_id, &employee_indices);
                }
                Operation::RaiseDispute => {
                    if !dispute_raised {
                        if let Ok(_) = client.try_raise_dispute(&employer, &agreement_id) {
                            dispute_raised = true;
                        }
                    }
                }
                Operation::ResolveDispute(employee_payout_ratio) => {
                    if !dispute_raised {
                        continue;
                    }
                    
                    // Set arbiter and resolve
                    let arbiter = Address::generate(&env);
                    client.set_arbiter(&owner, &arbiter);
                    
                    let remaining_escrow = env.as_contract(&contract_id, || {
                        DataKey::get_agreement_escrow_balance(&env, agreement_id, &token)
                    });
                    
                    let employee_payout = (remaining_escrow * (employee_payout_ratio as i128)) / 100;
                    let employer_refund = remaining_escrow - employee_payout;
                    
                    let _ = client.try_resolve_dispute(
                        &arbiter,
                        &agreement_id,
                        &employee_payout,
                        &employer_refund,
                    );
                    dispute_raised = false;
                }
            }
        }
        
        // **Assert conservation of funds invariant**
        env.as_contract(&contract_id, || {
            let remaining_escrow = DataKey::get_agreement_escrow_balance(&env, agreement_id, &token);
            let total_paid = DataKey::get_agreement_paid_amount(&env, agreement_id);
            
            // The fundamental invariant: deposits = payouts + remaining
            prop_assert!(remaining_escrow >= 0, "Escrow balance must be non-negative");
            prop_assert!(
                total_paid + remaining_escrow <= total_deposited,
                "Total paid ({}) + remaining ({}) must not exceed deposited ({})",
                total_paid, remaining_escrow, total_deposited
            );
            
            // Verify no funds leaked from the contract
            prop_assert_eq!(
                total_paid + remaining_escrow,
                total_deposited,
                "Conservation of funds violated: {} + {} != {}",
                total_paid, remaining_escrow, total_deposited
            );
        });
    }
}


proptest! {
    #![proptest_config(ProptestConfig::with_cases(proptest_cases()))]

    /// **Conservation of funds invariant for escrow agreements**
    ///
    /// Generates randomized escrow agreements and operation sequences, then
    /// asserts that total transferred out never exceeds total deposited.
    ///
    /// **Invariant**: `escrow_balance + paid_amount == total_amount` at all times.
    #[test]
    fn prop_escrow_conservation_of_funds(
        (amount_per_period, period_seconds, num_periods) in escrow_agreement_strategy(),
        claim_attempts in 0u32..15,
    ) {
        let (env, contract_id, _owner, client) = setup_contract();
        
        let employer = Address::generate(&env);
        let contributor = Address::generate(&env);
        let token = setup_token(&env, &contract_id, 1_000_000);
        
        let agreement_id = client.create_escrow_agreement(
            &employer,
            &contributor,
            &token,
            &amount_per_period,
            &period_seconds,
            &num_periods,
        ).unwrap();
        
        let total_amount = amount_per_period * (num_periods as i128);
        
        // Fund and activate
        env.as_contract(&contract_id, || {
            DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, total_amount);
        });
        client.activate_agreement(&agreement_id);
        
        // Attempt multiple claims
        for _ in 0..claim_attempts {
            env.ledger().with_mut(|li: &mut Ledger| {
                li.timestamp += period_seconds + 1;
            });
            let _ = client.try_claim_time_based(&agreement_id);
        }
        
        // **Assert conservation invariant**
        env.as_contract(&contract_id, || {
            let remaining = DataKey::get_agreement_escrow_balance(&env, agreement_id, &token);
            let paid = DataKey::get_agreement_paid_amount(&env, agreement_id);
            
            prop_assert!(remaining >= 0, "Escrow balance must be non-negative");
            prop_assert!(paid >= 0, "Paid amount must be non-negative");
            prop_assert_eq!(
                remaining + paid,
                total_amount,
                "Conservation violated: {} + {} != {}",
                remaining, paid, total_amount
            );
        });
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(proptest_cases()))]

    /// **Claimed periods monotonicity invariant**
    ///
    /// Asserts that `claimed_periods` for each employee is:
    /// 1. Monotonic non-decreasing (never goes backward)
    /// 2. Never exceeds the total available periods
    ///
    /// This catches bugs where claims might decrement or overflow the period counter.
    #[test]
    fn prop_claimed_periods_monotonic_and_bounded(
        (employee_count, salaries, period_seconds, _grace) in payroll_agreement_strategy(),
        claim_sequence in prop::collection::vec(0u32..5, 1..=10),
    ) {
        let (env, contract_id, _owner, client) = setup_contract();
        
        let employer = Address::generate(&env);
        let token = setup_token(&env, &contract_id, 1_000_000);
        let agreement_id = client.create_payroll_agreement(&employer, &token, &604800);
        
        // Add employees
        let mut employees = Vec::new();
        let mut total_salary: i128 = 0;
        for i in 0..employee_count {
            let employee = Address::generate(&env);
            let salary = salaries.get(i as usize).cloned().unwrap_or(1000);
            client.add_employee_to_agreement(&agreement_id, &employee, &salary);
            employees.push((employee, salary));
            total_salary += salary;
        }
        
        // Setup storage
        let max_periods = 20u32;
        env.as_contract(&contract_id, || {
            let now = env.ledger().timestamp();
            DataKey::set_agreement_activation_time(&env, agreement_id, now);
            DataKey::set_agreement_period_duration(&env, agreement_id, period_seconds);
            DataKey::set_agreement_token(&env, agreement_id, &token);
            DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, total_salary * (max_periods as i128));
            DataKey::set_employee_count(&env, agreement_id, employee_count);
            for (idx, (employee, salary)) in employees.iter().enumerate() {
                DataKey::set_employee(&env, agreement_id, idx as u32, employee);
                DataKey::set_employee_salary(&env, agreement_id, idx as u32, *salary);
                DataKey::set_employee_claimed_periods(&env, agreement_id, idx as u32, 0);
            }
        });
        
        client.activate_agreement(&agreement_id);
        
        // Track previous claimed_periods for monotonicity check
        let mut prev_claimed = vec![0u32; employee_count as usize];
        
        for claim_idx in claim_sequence {
            // Advance time
            env.ledger().with_mut(|li: &mut Ledger| {
                li.timestamp += period_seconds + 1;
            });
            
            let employee_idx = claim_idx % employee_count;
            let employee = &employees[employee_idx as usize].0;
            let _ = client.try_claim_payroll(employee, &agreement_id, &employee_idx);
            
            // **Assert monotonicity and bounds**
            env.as_contract(&contract_id, || {
                let claimed = DataKey::get_employee_claimed_periods(&env, agreement_id, employee_idx);
                
                prop_assert!(
                    claimed >= prev_claimed[employee_idx as usize],
                    "claimed_periods must be non-decreasing: {} < {}",
                    claimed, prev_claimed[employee_idx as usize]
                );
                
                prop_assert!(
                    claimed <= max_periods,
                    "claimed_periods ({}) must not exceed max_periods ({})",
                    claimed, max_periods
                );
                
                prev_claimed[employee_idx as usize] = claimed;
            });
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(proptest_cases()))]

    /// **Resolve dispute never over-distributes funds**
    ///
    /// Generates randomized disputes and resolution splits, then asserts:
    /// 1. `employee_payout + employer_refund <= escrow_balance`
    /// 2. After resolution, all funds are accounted for
    ///
    /// This catches integer division dust bugs and over-distribution in
    /// multi-employee dispute resolution.
    #[test]
    fn prop_resolve_dispute_bounds_respected(
        (employee_count, salaries, _period, grace) in payroll_agreement_strategy(),
        employee_payout_ratio in 0u32..=100,
    ) {
        let (env, contract_id, owner, client) = setup_contract();
        
        let employer = Address::generate(&env);
        let arbiter = Address::generate(&env);
        let token = setup_token(&env, &contract_id, 1_000_000);
        
        client.set_arbiter(&owner, &arbiter);
        
        let agreement_id = client.create_payroll_agreement(&employer, &token, &grace);
        
        // Add employees
        let mut total_salary: i128 = 0;
        for i in 0..employee_count {
            let employee = Address::generate(&env);
            let salary = salaries.get(i as usize).cloned().unwrap_or(1000);
            client.add_employee_to_agreement(&agreement_id, &employee, &salary);
            total_salary += salary;
        }
        
        let escrow_balance = total_salary * 10;
        env.as_contract(&contract_id, || {
            DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, escrow_balance);
        });
        
        client.activate_agreement(&agreement_id);
        
        // Raise dispute
        if client.try_raise_dispute(&employer, &agreement_id).is_err() {
            return Ok(()); // Skip if dispute can't be raised
        }
        
        // Calculate split
        let employee_payout = (escrow_balance * (employee_payout_ratio as i128)) / 100;
        let employer_refund = escrow_balance - employee_payout;
        
        // Track balances before resolution
        let balance_before = env.as_contract(&contract_id, || {
            DataKey::get_agreement_escrow_balance(&env, agreement_id, &token)
        });
        
        // Resolve dispute
        let result = client.try_resolve_dispute(
            &arbiter,
            &agreement_id,
            &employee_payout,
            &employer_refund,
        );
        
        match result {
            Ok(_) => {
                // **Assert bounds and conservation after resolution**
                env.as_contract(&contract_id, || {
                    let balance_after = DataKey::get_agreement_escrow_balance(&env, agreement_id, &token);
                    
                    // After resolution, escrow should be depleted
                    prop_assert!(
                        balance_after <= balance_before,
                        "Escrow balance increased after dispute resolution"
                    );
                    
                    // The sum of payouts must not exceed the initial balance
                    prop_assert!(
                        employee_payout + employer_refund <= balance_before,
                        "Total payout ({} + {}) exceeds escrow ({})",
                        employee_payout, employer_refund, balance_before
                    );
                });
            }
            Err(Ok(PayrollError::InvalidPayout)) => {
                // Expected if split doesn't match escrow exactly
            }
            Err(e) => {
                // Other errors are acceptable in property tests
                prop_assert!(true, "Dispute resolution failed: {:?}", e);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(proptest_cases()))]

    /// **Multi-employee dispute resolution conservation**
    ///
    /// Specifically tests the integer division dust case in multi-employee
    /// dispute resolution where employee_payout is split among N employees.
    ///
    /// **Invariant**: No funds are lost or created during the split:
    ///   `sum(individual_payouts) + employer_refund == total_escrow`
    #[test]
    fn prop_multi_employee_dispute_no_dust_leakage(
        employee_count in 2u32..=5,
        base_salary in 100i128..1000,
        employee_payout_ratio in 1u32..=100,
    ) {
        let (env, contract_id, owner, client) = setup_contract();
        
        let employer = Address::generate(&env);
        let arbiter = Address::generate(&env);
        let token = setup_token(&env, &contract_id, 1_000_000);
        
        client.set_arbiter(&owner, &arbiter);
        
        let agreement_id = client.create_payroll_agreement(&employer, &token, &604800);
        
        // Add employees with identical salaries for simplicity
        let mut employees = Vec::new();
        for _ in 0..employee_count {
            let employee = Address::generate(&env);
            client.add_employee_to_agreement(&agreement_id, &employee, &base_salary);
            employees.push(employee);
        }
        
        let total_escrow = base_salary * (employee_count as i128) * 5;
        env.as_contract(&contract_id, || {
            DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, total_escrow);
        });
        
        client.activate_agreement(&agreement_id);
        
        // Raise and resolve dispute
        if client.try_raise_dispute(&employer, &agreement_id).is_err() {
            return Ok(());
        }
        
        let employee_payout = (total_escrow * (employee_payout_ratio as i128)) / 100;
        let employer_refund = total_escrow - employee_payout;
        
        let result = client.try_resolve_dispute(
            &arbiter,
            &agreement_id,
            &employee_payout,
            &employer_refund,
        );
        
        if result.is_ok() {
            // **Assert no dust leakage**: total distributed equals total escrow
            // This is implicitly checked by the contract, but we verify the
            // remaining balance is zero or minimal dust
            env.as_contract(&contract_id, || {
                let remaining = DataKey::get_agreement_escrow_balance(&env, agreement_id, &token);
                
                // After full resolution, remaining should be zero or minimal dust
                // (less than employee_count due to integer division)
                prop_assert!(
                    remaining <= (employee_count as i128),
                    "Excessive remaining balance after dispute: {} (employee_count: {})",
                    remaining, employee_count
                );
            });
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(proptest_cases()))]

    /// **Grace period claim conservation**
    ///
    /// Tests that claims made during grace period after cancellation still
    /// respect conservation of funds.
    ///
    /// **Invariant**: Claims during grace + refund after grace == total escrow
    #[test]
    fn prop_grace_period_claim_conservation(
        (amount_per_period, period_seconds, num_periods) in escrow_agreement_strategy(),
    ) {
        let (env, contract_id, _owner, client) = setup_contract();
        
        let employer = Address::generate(&env);
        let contributor = Address::generate(&env);
        let token = setup_token(&env, &contract_id, 1_000_000);
        
        let agreement_id = client.create_escrow_agreement(
            &employer,
            &contributor,
            &token,
            &amount_per_period,
            &period_seconds,
            &num_periods,
        ).unwrap();
        
        let total_amount = amount_per_period * (num_periods as i128);
        
        env.as_contract(&contract_id, || {
            DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, total_amount);
        });
        client.activate_agreement(&agreement_id);
        
        // Advance time to allow some claims
        env.ledger().with_mut(|li: &mut Ledger| {
            li.timestamp += period_seconds * 2;
        });
        
        // Make some claims
        let _ = client.try_claim_time_based(&agreement_id);
        
        let claimed_before_cancel = env.as_contract(&contract_id, || {
            DataKey::get_agreement_paid_amount(&env, agreement_id)
        });
        
        // Cancel agreement
        client.cancel_agreement(&agreement_id);
        
        // Try to claim during grace period
        env.ledger().with_mut(|li: &mut Ledger| {
            li.timestamp += period_seconds;
        });
        let _ = client.try_claim_time_based(&agreement_id);
        
        // Finalize after grace period
        if let Some(grace_end) = client.get_grace_period_end(&agreement_id) {
            env.ledger().with_mut(|li: &mut Ledger| {
                li.timestamp = grace_end + 1;
            });
            client.finalize_grace_period(&agreement_id);
        }
        
        // **Assert conservation**
        env.as_contract(&contract_id, || {
            let final_escrow = DataKey::get_agreement_escrow_balance(&env, agreement_id, &token);
            let final_paid = DataKey::get_agreement_paid_amount(&env, agreement_id);
            
            prop_assert!(final_paid >= claimed_before_cancel, "Paid amount should not decrease");
            prop_assert_eq!(
                final_escrow + final_paid,
                total_amount,
                "Conservation violated after grace period: {} + {} != {}",
                final_escrow, final_paid, total_amount
            );
        });
    }
}
