//! Stress Testing Infrastructure for StelloPay Core (#228).
//!
//! This suite validates contract behavior under extreme conditions:
//! - Maximum values and overflow boundaries
//! - Rapid transaction bursts in a single ledger window
//! - Congested batch traffic with duplicates/invalid operations
//! - Explicit failure-point discovery and reporting

#![cfg(test)]
#![allow(deprecated)]

use std::time::Instant;

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, Vec,
};
use stello_pay_contract::storage::{DataKey, PayrollError};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

/// Creates a fresh test environment with mocked auth and initialized contract.
///
/// # Returns
/// `(env, employer, token, client)`
fn create_test_env() -> (Env, Address, Address, PayrollContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    let employer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    (env, employer, token, client)
}

/// Mints test tokens to `to`.
fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

/// Creates and funds an escrow agreement, then activates it.
///
/// Also seeds `AgreementEscrowBalance`, which time-based claim logic reads.
fn setup_funded_escrow(
    env: &Env,
    client: &PayrollContractClient,
    employer: &Address,
    contributor: &Address,
    token: &Address,
    amount_per_period: i128,
    period_seconds: u64,
    num_periods: u32,
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
    mint(env, token, &client.address, total);
    env.as_contract(&client.address, || {
        DataKey::set_agreement_escrow_balance(env, agreement_id, token, total);
    });
    client.activate_agreement(&agreement_id);
    agreement_id
}

/// Creates and funds a milestone agreement.
fn setup_funded_milestone(
    env: &Env,
    client: &PayrollContractClient,
    employer: &Address,
    contributor: &Address,
    token: &Address,
    amount: i128,
    milestone_count: u32,
) -> u128 {
    let agreement_id = client.create_milestone_agreement(employer, contributor, token);
    for _ in 0..milestone_count {
        client.add_milestone(&agreement_id, &amount);
    }
    mint(
        env,
        token,
        &client.address,
        amount * (milestone_count as i128),
    );
    agreement_id
}

/// Stress test for maximum-value behavior and overflow boundaries.
///
/// Verifies:
/// - Large-but-valid escrow setup (`u32::MAX` periods) succeeds.
/// - Overflowing setup (`i128::MAX * 2`) fails cleanly.
#[test]
fn stress_max_values_and_overflow_boundaries() {
    let (env, employer, token, client) = create_test_env();
    let contributor = Address::generate(&env);

    let started = Instant::now();
    let max_safe_id =
        client.create_escrow_agreement(&employer, &contributor, &token, &1i128, &1u64, &u32::MAX);
    let max_safe_elapsed = started.elapsed();

    let max_safe_agreement = client.get_agreement(&max_safe_id).unwrap();
    assert_eq!(max_safe_agreement.total_amount, u32::MAX as i128);
    assert_eq!(max_safe_agreement.num_periods, Some(u32::MAX));

    let overflow_result = client.try_create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &i128::MAX,
        &1u64,
        &2u32,
    );
    assert!(overflow_result.is_err());

    println!(
        "[stress][max-values] max_safe_create_us={} overflow_rejected=true",
        max_safe_elapsed.as_micros()
    );
}

/// Stress test for rapid transactions submitted in one ledger window.
///
/// Verifies:
/// - First claim succeeds once a period elapses.
/// - A burst of immediate repeated claims fails consistently.
/// - Failure mode is measured and reported.
#[test]
fn stress_rapid_transactions_single_window() {
    let (env, employer, token, client) = create_test_env();
    let contributor = Address::generate(&env);
    let agreement_id =
        setup_funded_escrow(&env, &client, &employer, &contributor, &token, 1000, 1, 10);

    env.ledger().with_mut(|li| li.timestamp += 1);
    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_claimed_periods(&agreement_id), 1);

    let rapid_attempts = 300u32;
    let started = Instant::now();
    let mut no_period_errors = 0u32;
    let mut all_periods_errors = 0u32;
    for _ in 0..rapid_attempts {
        match client.try_claim_time_based(&agreement_id) {
            Err(Ok(PayrollError::NoPeriodsToClaim)) => no_period_errors += 1,
            Err(Ok(PayrollError::AllPeriodsClaimed)) => all_periods_errors += 1,
            other => panic!("unexpected rapid-claim result: {:?}", other),
        }
    }
    let elapsed = started.elapsed();

    assert_eq!(no_period_errors + all_periods_errors, rapid_attempts);
    assert_eq!(client.get_claimed_periods(&agreement_id), 1);

    println!(
        "[stress][rapid] attempts={} duration_ms={} no_period_errors={} all_periods_errors={}",
        rapid_attempts,
        elapsed.as_millis(),
        no_period_errors,
        all_periods_errors
    );
}

/// Stress test that simulates network congestion via a large mixed batch.
///
/// Batch composition:
/// - Approved valid IDs (successful)
/// - Duplicate IDs (fail)
/// - Unapproved IDs (fail)
/// - Out-of-range IDs (fail)
#[test]
fn stress_network_congestion_mixed_batch() {
    let (env, employer, token, client) = create_test_env();
    let contributor = Address::generate(&env);
    let agreement_id =
        setup_funded_milestone(&env, &client, &employer, &contributor, &token, 100, 100);

    for id in 1..=60u32 {
        client.approve_milestone(&agreement_id, &id);
    }

    let mut ids = Vec::new(&env);
    for id in 1..=60u32 {
        ids.push_back(id); // approved -> success
    }
    for id in 1..=60u32 {
        ids.push_back(id); // duplicate -> fail
    }
    for id in 61..=100u32 {
        ids.push_back(id); // unapproved -> fail
    }
    for id in 101..=140u32 {
        ids.push_back(id); // invalid -> fail
    }

    let started = Instant::now();
    let result = client.batch_claim_milestones(&agreement_id, &ids);
    let elapsed = started.elapsed();

    assert_eq!(result.successful_claims, 60);
    assert_eq!(result.failed_claims, 140);
    assert_eq!(result.total_claimed, 6000);

    println!(
        "[stress][congestion] batch_size={} duration_ms={} success={} failed={}",
        ids.len(),
        elapsed.as_millis(),
        result.successful_claims,
        result.failed_claims
    );
}

/// Stress test that explicitly measures the first failure point.
///
/// Claims are attempted repeatedly after full accrual:
/// - Attempt 1 should consume all available periods.
/// - Attempt 2 should fail (`AllPeriodsClaimed` or `NoPeriodsToClaim`).
#[test]
fn stress_failure_point_detection() {
    let (env, employer, token, client) = create_test_env();
    let contributor = Address::generate(&env);
    let agreement_id =
        setup_funded_escrow(&env, &client, &employer, &contributor, &token, 500, 1, 12);

    env.ledger().with_mut(|li| li.timestamp += 12);

    let mut first_failure_attempt: Option<u32> = None;
    let mut failure_code: Option<PayrollError> = None;
    for attempt in 1..=20u32 {
        let result = client.try_claim_time_based(&agreement_id);
        if let Err(Ok(code)) = result {
            first_failure_attempt = Some(attempt);
            failure_code = Some(code);
            break;
        }
    }

    assert_eq!(first_failure_attempt, Some(2));
    let code = failure_code.expect("failure code must be present");
    assert!(matches!(
        code,
        PayrollError::AllPeriodsClaimed | PayrollError::NoPeriodsToClaim
    ));

    println!(
        "[stress][failure-point] first_failure_attempt={} error={:?}",
        first_failure_attempt.unwrap(),
        code
    );
}
