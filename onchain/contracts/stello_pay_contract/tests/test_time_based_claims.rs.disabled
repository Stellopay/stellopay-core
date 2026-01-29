#![cfg(test)]

use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::{testutils::Address as _, testutils::Ledger, Address, Env};
use stello_pay_contract::storage::{AgreementMode, AgreementStatus, DataKey, PayrollError};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// =============================================================================
// Test Helpers
// =============================================================================

fn create_test_environment() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn create_token(env: &Env) -> Address {
    let admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    let token_admin_client = StellarAssetClient::new(env, token);
    token_admin_client.mint(to, &amount);
}

fn setup_escrow_agreement(
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

    // Mint tokens to fund the escrow
    let total_amount = amount_per_period * (num_periods as i128);
    mint(env, token, &client.address, total_amount);

    // Set the escrow balance in storage
    env.as_contract(&client.address, || {
        DataKey::set_agreement_escrow_balance(env, agreement_id, token, total_amount);
    });

    agreement_id
}

// =============================================================================
// Agreement Creation Tests
// =============================================================================

#[test]
fn test_create_time_based_agreement() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let amount_per_period = 1000i128;
    let period_seconds = 86400u64; // 1 day
    let num_periods = 10u32;

    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &amount_per_period,
        &period_seconds,
        &num_periods,
    );

    let agreement = client.get_agreement(&agreement_id).unwrap();

    assert_eq!(agreement.id, agreement_id);
    assert_eq!(agreement.employer, employer);
    assert_eq!(agreement.token, token);
    assert_eq!(agreement.mode, AgreementMode::Escrow);
    assert_eq!(agreement.status, AgreementStatus::Created);
    assert_eq!(agreement.amount_per_period, Some(amount_per_period));
    assert_eq!(agreement.period_seconds, Some(period_seconds));
    assert_eq!(agreement.num_periods, Some(num_periods));
    assert_eq!(agreement.claimed_periods, Some(0));
}

#[test]
fn test_create_zero_amount_fails() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    let result = client.try_create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &0i128, // Zero amount
        &86400u64,
        &10u32,
    );

    assert_eq!(result, Err(Ok(PayrollError::ZeroAmountPerPeriod)));
}

#[test]
fn test_create_zero_period_fails() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    let result = client.try_create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &1000i128,
        &0u64, // Zero period duration
        &10u32,
    );

    assert_eq!(result, Err(Ok(PayrollError::ZeroPeriodDuration)));
}

#[test]
fn test_create_zero_periods_fails() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    let result = client.try_create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &1000i128,
        &86400u64,
        &0u32, // Zero number of periods
    );

    assert_eq!(result, Err(Ok(PayrollError::ZeroNumPeriods)));
}

#[test]
fn test_total_amount_calculated_correctly() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let amount_per_period = 1500i128;
    let num_periods = 12u32;
    let expected_total = amount_per_period * (num_periods as i128);

    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &amount_per_period,
        &86400u64,
        &num_periods,
    );

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.total_amount, expected_total);
}

// =============================================================================
// Claiming Tests
// =============================================================================

#[test]
fn test_claim_before_activation_fails() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1000i128,
        86400u64,
        10u32,
    );

    // Don't activate - try to claim immediately
    let result = client.try_claim_time_based(&agreement_id);
    assert_eq!(result, Err(Ok(PayrollError::AgreementNotActivated)));
}

#[test]
fn test_claim_after_one_period() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let amount_per_period = 1000i128;
    let period_seconds = 86400u64;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount_per_period,
        period_seconds,
        10u32,
    );

    client.activate_agreement(&agreement_id);

    // Fast forward 1 period
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds;
    });

    client.claim_time_based(&agreement_id);

    let claimed = client.get_claimed_periods(&agreement_id);
    assert_eq!(claimed, 1u32);

    // Verify contributor received payment
    let token_client = TokenClient::new(&env, &token);
    let balance = token_client.balance(&contributor);
    assert_eq!(balance, amount_per_period);
}

#[test]
fn test_claim_after_multiple_periods() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let amount_per_period = 1000i128;
    let period_seconds = 86400u64;
    let periods_elapsed = 5u64;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount_per_period,
        period_seconds,
        10u32,
    );

    client.activate_agreement(&agreement_id);

    // Fast forward 5 periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * periods_elapsed;
    });

    client.claim_time_based(&agreement_id);

    let claimed = client.get_claimed_periods(&agreement_id);
    assert_eq!(claimed, 5u32);

    // Verify contributor received payment for all 5 periods
    let token_client = TokenClient::new(&env, &token);
    let balance = token_client.balance(&contributor);
    assert_eq!(balance, amount_per_period * (periods_elapsed as i128));
}

#[test]
fn test_claim_all_periods_at_once() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let amount_per_period = 1000i128;
    let period_seconds = 86400u64;
    let num_periods = 10u32;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount_per_period,
        period_seconds,
        num_periods,
    );

    client.activate_agreement(&agreement_id);

    // Fast forward past all periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * (num_periods as u64) + 1;
    });

    client.claim_time_based(&agreement_id);

    let claimed = client.get_claimed_periods(&agreement_id);
    assert_eq!(claimed, num_periods);

    // Verify agreement is completed
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Completed);

    // Verify contributor received full payment
    let token_client = TokenClient::new(&env, &token);
    let balance = token_client.balance(&contributor);
    assert_eq!(balance, amount_per_period * (num_periods as i128));
}

#[test]
fn test_claim_wrong_status_fails() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let period_seconds = 86400u64;
    let num_periods = 10u32;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1000i128,
        period_seconds,
        num_periods,
    );

    client.activate_agreement(&agreement_id);

    // Fast forward 1 period
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds;
    });

    // Cancel the agreement
    client.cancel_agreement(&agreement_id);

    // Fast forward past grace period
    let grace_period = period_seconds * (num_periods as u64);
    env.ledger().with_mut(|li| {
        li.timestamp += grace_period + 1;
    });

    // Try to claim - should fail because grace period expired
    let result = client.try_claim_time_based(&agreement_id);
    assert_eq!(result, Err(Ok(PayrollError::NotInGracePeriod)));
}

#[test]
fn test_claim_during_grace_period() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let amount_per_period = 1000i128;
    let period_seconds = 86400u64;
    let num_periods = 10u32;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount_per_period,
        period_seconds,
        num_periods,
    );

    client.activate_agreement(&agreement_id);

    // Fast forward 3 periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 3;
    });

    // Cancel the agreement (initiates grace period)
    client.cancel_agreement(&agreement_id);

    // Verify agreement is cancelled
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Cancelled);

    // Verify grace period is active
    assert!(client.is_grace_period_active(&agreement_id));

    // Should still be able to claim during grace period
    client.claim_time_based(&agreement_id);

    let claimed = client.get_claimed_periods(&agreement_id);
    assert_eq!(claimed, 3u32);

    // Verify contributor received payment
    let token_client = TokenClient::new(&env, &token);
    let balance = token_client.balance(&contributor);
    assert_eq!(balance, amount_per_period * 3);
}

#[test]
fn test_claim_after_grace_period_fails() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let period_seconds = 86400u64;
    let num_periods = 10u32;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1000i128,
        period_seconds,
        num_periods,
    );

    client.activate_agreement(&agreement_id);

    // Fast forward 2 periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 2;
    });

    // Cancel the agreement
    client.cancel_agreement(&agreement_id);

    // Fast forward past the grace period
    // Grace period = period_seconds * num_periods
    let grace_period = period_seconds * (num_periods as u64);
    env.ledger().with_mut(|li| {
        li.timestamp += grace_period + 1;
    });

    // Verify grace period is no longer active
    assert!(!client.is_grace_period_active(&agreement_id));

    // Should fail to claim after grace period
    let result = client.try_claim_time_based(&agreement_id);
    assert_eq!(result, Err(Ok(PayrollError::NotInGracePeriod)));
}

// =============================================================================
// Period Calculation Tests
// =============================================================================

#[test]
fn test_periods_calculation_exact_boundaries() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let period_seconds = 86400u64;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1000i128,
        period_seconds,
        10u32,
    );

    client.activate_agreement(&agreement_id);

    // At exactly 1 period boundary
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds;
    });

    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_claimed_periods(&agreement_id), 1u32);

    // At exactly 2 more periods (total 3)
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 2;
    });

    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_claimed_periods(&agreement_id), 3u32);
}

#[test]
fn test_periods_calculation_partial_rounds_down() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let period_seconds = 86400u64;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1000i128,
        period_seconds,
        10u32,
    );

    client.activate_agreement(&agreement_id);

    // At 2.5 periods - should round down to 2
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 2 + period_seconds / 2;
    });

    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_claimed_periods(&agreement_id), 2u32);
}

#[test]
fn test_periods_calculation_more_elapsed_than_total() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let period_seconds = 86400u64;
    let num_periods = 5u32;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1000i128,
        period_seconds,
        num_periods,
    );

    client.activate_agreement(&agreement_id);

    // Fast forward 100 periods (way more than num_periods)
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 100;
    });

    client.claim_time_based(&agreement_id);

    // Should cap at num_periods
    assert_eq!(client.get_claimed_periods(&agreement_id), num_periods);
}

#[test]
fn test_claimed_periods_updates() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let period_seconds = 86400u64;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1000i128,
        period_seconds,
        10u32,
    );

    client.activate_agreement(&agreement_id);

    // Initial claimed periods should be 0
    assert_eq!(client.get_claimed_periods(&agreement_id), 0u32);

    // Claim after 2 periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 2;
    });
    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_claimed_periods(&agreement_id), 2u32);

    // Claim after 3 more periods (5 total)
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 3;
    });
    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_claimed_periods(&agreement_id), 5u32);

    // Verify agreement tracks updates
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.claimed_periods, Some(5u32));
}

#[test]
fn test_cannot_claim_same_period_twice() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let period_seconds = 86400u64;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1000i128,
        period_seconds,
        10u32,
    );

    client.activate_agreement(&agreement_id);

    // Fast forward 2 periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 2;
    });

    // First claim succeeds
    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_claimed_periods(&agreement_id), 2u32);

    // Second claim at same time fails - no new periods
    let result = client.try_claim_time_based(&agreement_id);
    assert_eq!(result, Err(Ok(PayrollError::NoPeriodsToClaim)));
}

// =============================================================================
// Payment Amount Tests
// =============================================================================

#[test]
fn test_payment_amount_single_period() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let amount_per_period = 1500i128;
    let period_seconds = 86400u64;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount_per_period,
        period_seconds,
        10u32,
    );

    client.activate_agreement(&agreement_id);

    // Fast forward 1 period
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds;
    });

    client.claim_time_based(&agreement_id);

    let token_client = TokenClient::new(&env, &token);
    assert_eq!(token_client.balance(&contributor), amount_per_period);
}

#[test]
fn test_payment_amount_multiple_periods() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let amount_per_period = 2000i128;
    let period_seconds = 86400u64;
    let periods_to_claim = 4u64;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount_per_period,
        period_seconds,
        10u32,
    );

    client.activate_agreement(&agreement_id);

    // Fast forward 4 periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * periods_to_claim;
    });

    client.claim_time_based(&agreement_id);

    let token_client = TokenClient::new(&env, &token);
    assert_eq!(
        token_client.balance(&contributor),
        amount_per_period * (periods_to_claim as i128)
    );
}

#[test]
fn test_payment_amount_calculation() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let amount_per_period = 1234i128;
    let period_seconds = 3600u64; // 1 hour
    let num_periods = 10u32;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount_per_period,
        period_seconds,
        num_periods,
    );

    client.activate_agreement(&agreement_id);

    let token_client = TokenClient::new(&env, &token);

    // Claim 3 periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 3;
    });
    client.claim_time_based(&agreement_id);
    assert_eq!(token_client.balance(&contributor), amount_per_period * 3);

    // Claim 2 more periods (5 total)
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 2;
    });
    client.claim_time_based(&agreement_id);
    assert_eq!(token_client.balance(&contributor), amount_per_period * 5);

    // Claim remaining 5 periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 5;
    });
    client.claim_time_based(&agreement_id);
    assert_eq!(
        token_client.balance(&contributor),
        amount_per_period * (num_periods as i128)
    );
}

#[test]
fn test_funds_released_from_escrow() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let amount_per_period = 1000i128;
    let period_seconds = 86400u64;
    let num_periods = 5u32;
    let total_amount = amount_per_period * (num_periods as i128);

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount_per_period,
        period_seconds,
        num_periods,
    );

    client.activate_agreement(&agreement_id);

    let token_client = TokenClient::new(&env, &token);

    // Initial contract balance
    let initial_contract_balance = token_client.balance(&contract_id);
    assert_eq!(initial_contract_balance, total_amount);

    // Claim 2 periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 2;
    });
    client.claim_time_based(&agreement_id);

    // Contract balance decreased
    let contract_balance_after = token_client.balance(&contract_id);
    assert_eq!(
        contract_balance_after,
        total_amount - (amount_per_period * 2)
    );

    // Contributor received funds
    assert_eq!(token_client.balance(&contributor), amount_per_period * 2);
}

// =============================================================================
// Agreement Completion Tests
// =============================================================================

#[test]
fn test_agreement_completes_all_periods_claimed() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let period_seconds = 86400u64;
    let num_periods = 3u32;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1000i128,
        period_seconds,
        num_periods,
    );

    client.activate_agreement(&agreement_id);

    // Claim all periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * (num_periods as u64);
    });
    client.claim_time_based(&agreement_id);

    // Verify agreement is completed
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Completed);
    assert_eq!(agreement.claimed_periods, Some(num_periods));
}

#[test]
fn test_cannot_claim_after_completion() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let period_seconds = 86400u64;
    let num_periods = 3u32;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1000i128,
        period_seconds,
        num_periods,
    );

    client.activate_agreement(&agreement_id);

    // Claim all periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * (num_periods as u64);
    });
    client.claim_time_based(&agreement_id);

    // Verify completed
    assert_eq!(
        client.get_agreement(&agreement_id).unwrap().status,
        AgreementStatus::Completed
    );

    // Fast forward more time
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 10;
    });

    // Try to claim again - should fail with AllPeriodsClaimed
    let result = client.try_claim_time_based(&agreement_id);
    assert_eq!(result, Err(Ok(PayrollError::AllPeriodsClaimed)));
}

#[test]
fn test_agreement_completed_event() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let period_seconds = 86400u64;
    let num_periods = 2u32;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1000i128,
        period_seconds,
        num_periods,
    );

    client.activate_agreement(&agreement_id);

    // Claim all periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * (num_periods as u64);
    });
    client.claim_time_based(&agreement_id);

    // Verify agreement status changed to Completed
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Completed);

    // Verify paid_amount equals total_amount
    assert_eq!(agreement.paid_amount, agreement.total_amount);
}

// =============================================================================
// Edge Cases
// =============================================================================

#[test]
fn test_very_short_periods() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let amount_per_period = 100i128;
    let period_seconds = 1u64; // 1 second periods
    let num_periods = 100u32;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount_per_period,
        period_seconds,
        num_periods,
    );

    client.activate_agreement(&agreement_id);

    // Fast forward 50 seconds
    env.ledger().with_mut(|li| {
        li.timestamp += 50;
    });

    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_claimed_periods(&agreement_id), 50u32);

    // Fast forward remaining time
    env.ledger().with_mut(|li| {
        li.timestamp += 50;
    });

    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_claimed_periods(&agreement_id), num_periods);

    // Verify completion
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Completed);
}

#[test]
fn test_very_long_periods() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let amount_per_period = 10_000i128;
    let period_seconds = 31_536_000u64; // 1 year in seconds
    let num_periods = 2u32;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount_per_period,
        period_seconds,
        num_periods,
    );

    client.activate_agreement(&agreement_id);

    // Fast forward 1 year
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds;
    });

    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_claimed_periods(&agreement_id), 1u32);

    // Fast forward another year
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds;
    });

    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_claimed_periods(&agreement_id), 2u32);

    // Verify completion
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Completed);
}

#[test]
fn test_very_large_num_periods() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let amount_per_period = 1i128;
    let period_seconds = 1u64;
    let num_periods = 10_000u32;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount_per_period,
        period_seconds,
        num_periods,
    );

    client.activate_agreement(&agreement_id);

    // Fast forward all periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * (num_periods as u64);
    });

    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_claimed_periods(&agreement_id), num_periods);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Completed);
}

#[test]
fn test_claiming_at_exact_boundaries() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let amount_per_period = 1000i128;
    let period_seconds = 3600u64; // 1 hour
    let num_periods = 5u32;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount_per_period,
        period_seconds,
        num_periods,
    );

    client.activate_agreement(&agreement_id);
    let token_client = TokenClient::new(&env, &token);

    // At exactly 1 period boundary
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds;
    });
    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_claimed_periods(&agreement_id), 1u32);
    assert_eq!(token_client.balance(&contributor), amount_per_period);

    // At exactly 2 period boundary (1 more period)
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds;
    });
    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_claimed_periods(&agreement_id), 2u32);
    assert_eq!(token_client.balance(&contributor), amount_per_period * 2);

    // At exactly 5 period boundary (3 more periods)
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 3;
    });
    client.claim_time_based(&agreement_id);
    assert_eq!(client.get_claimed_periods(&agreement_id), 5u32);
    assert_eq!(token_client.balance(&contributor), amount_per_period * 5);

    // Verify completion
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Completed);
}

#[test]
fn test_rapid_successive_claims() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let amount_per_period = 500i128;
    let period_seconds = 60u64; // 1 minute periods
    let num_periods = 10u32;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount_per_period,
        period_seconds,
        num_periods,
    );

    client.activate_agreement(&agreement_id);
    let token_client = TokenClient::new(&env, &token);

    // Claim every period rapidly
    for i in 1..=num_periods {
        env.ledger().with_mut(|li| {
            li.timestamp += period_seconds;
        });
        client.claim_time_based(&agreement_id);
        assert_eq!(client.get_claimed_periods(&agreement_id), i);
        assert_eq!(
            token_client.balance(&contributor),
            amount_per_period * (i as i128)
        );
    }

    // Verify completion
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Completed);
}

// =============================================================================
// Additional Edge Cases
// =============================================================================

#[test]
fn test_claim_when_paused_fails() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let period_seconds = 86400u64;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1000i128,
        period_seconds,
        10u32,
    );

    client.activate_agreement(&agreement_id);

    // Fast forward 2 periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 2;
    });

    // Pause the agreement
    client.pause_agreement(&agreement_id);

    // Verify agreement is paused
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Paused);

    // Try to claim - should fail with AgreementPaused
    let result = client.try_claim_time_based(&agreement_id);
    assert_eq!(result, Err(Ok(PayrollError::AgreementPaused)));
}

#[test]
fn test_claim_after_resume() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let amount_per_period = 1000i128;
    let period_seconds = 86400u64;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount_per_period,
        period_seconds,
        10u32,
    );

    client.activate_agreement(&agreement_id);

    // Fast forward 2 periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 2;
    });

    // Pause
    client.pause_agreement(&agreement_id);

    // Fast forward while paused
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 3;
    });

    // Resume
    client.resume_agreement(&agreement_id);

    // Verify active
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Active);

    // Now claim - should work and include time elapsed while paused
    client.claim_time_based(&agreement_id);

    // Total elapsed: 5 periods
    assert_eq!(client.get_claimed_periods(&agreement_id), 5u32);

    let token_client = TokenClient::new(&env, &token);
    assert_eq!(token_client.balance(&contributor), amount_per_period * 5);
}

#[test]
fn test_claim_immediately_after_activation_fails() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1000i128,
        86400u64,
        10u32,
    );

    client.activate_agreement(&agreement_id);

    // Try to claim immediately - no time has passed
    let result = client.try_claim_time_based(&agreement_id);
    assert_eq!(result, Err(Ok(PayrollError::NoPeriodsToClaim)));
}

#[test]
fn test_grace_period_end_timestamp() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let period_seconds = 86400u64;
    let num_periods = 10u32;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1000i128,
        period_seconds,
        num_periods,
    );

    client.activate_agreement(&agreement_id);

    // Before cancellation, grace period end should be None
    assert!(client.get_grace_period_end(&agreement_id).is_none());

    // Fast forward and cancel
    let cancel_time = period_seconds * 3;
    env.ledger().with_mut(|li| {
        li.timestamp += cancel_time;
    });
    client.cancel_agreement(&agreement_id);

    // Grace period end should be set
    let grace_end = client.get_grace_period_end(&agreement_id);
    assert!(grace_end.is_some());

    // Grace period = period_seconds * num_periods
    let expected_grace_period = period_seconds * (num_periods as u64);
    let expected_grace_end = cancel_time + expected_grace_period;
    assert_eq!(grace_end.unwrap(), expected_grace_end);
}

#[test]
fn test_incremental_claims_tracking() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let amount_per_period = 1000i128;
    let period_seconds = 86400u64;
    let num_periods = 10u32;

    let agreement_id = setup_escrow_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        amount_per_period,
        period_seconds,
        num_periods,
    );

    client.activate_agreement(&agreement_id);
    let token_client = TokenClient::new(&env, &token);

    // Track paid_amount incrementally
    let mut total_paid = 0i128;

    // Claim 2 periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 2;
    });
    client.claim_time_based(&agreement_id);
    total_paid += amount_per_period * 2;

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.paid_amount, total_paid);
    assert_eq!(token_client.balance(&contributor), total_paid);

    // Claim 3 more periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 3;
    });
    client.claim_time_based(&agreement_id);
    total_paid += amount_per_period * 3;

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.paid_amount, total_paid);
    assert_eq!(token_client.balance(&contributor), total_paid);

    // Claim remaining 5 periods
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 5;
    });
    client.claim_time_based(&agreement_id);
    total_paid += amount_per_period * 5;

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.paid_amount, total_paid);
    assert_eq!(agreement.paid_amount, agreement.total_amount);
    assert_eq!(token_client.balance(&contributor), total_paid);
}

#[test]
fn test_insufficient_escrow_balance_fails() {
    let env = create_test_environment();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);
    let amount_per_period = 1000i128;
    let period_seconds = 86400u64;
    let num_periods = 10u32;

    // Create agreement but don't fund properly
    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &amount_per_period,
        &period_seconds,
        &num_periods,
    );

    // Only mint half the required tokens
    let partial_amount = (amount_per_period * (num_periods as i128)) / 2;
    mint(&env, &token, &client.address, partial_amount);

    // Set only partial escrow balance
    env.as_contract(&client.address, || {
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, partial_amount);
    });

    client.activate_agreement(&agreement_id);

    // Try to claim more than available
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds * 8; // 8 periods worth
    });

    let result = client.try_claim_time_based(&agreement_id);
    assert_eq!(result, Err(Ok(PayrollError::InsufficientEscrowBalance)));
}
