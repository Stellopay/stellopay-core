#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, Vec,
};

use stello_pay_contract::storage::ExchangeRateInfo;
use stello_pay_contract::storage::{DataKey, PayrollError};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

/// Create a fresh test environment with a deployed payroll contract, owner,
/// arbiter and employer. Returns `(env, owner, employer, arbiter, client)`.
fn create_test_env() -> (
    Env,
    Address,
    Address,
    Address,
    PayrollContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    let arbiter = Address::generate(&env);
    client.set_arbiter(&owner, &arbiter);

    let employer = Address::generate(&env);

    (env, owner, employer, arbiter, client)
}

/// Simple sanity check that the FX helper round-trips a basic conversion using
/// the configured rate.
#[test]
fn test_convert_currency_basic() {
    let (env, owner, _employer, _arbiter, client) = create_test_env();

    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    // FX rate: 1 base = 2 quote (rate scaled by 1e6).
    let rate: i128 = 2_000_000;

    client.set_exchange_rate(&owner, &base, &quote, &rate);

    // Convert 10 base units → expect 20 quote units.
    let amount: i128 = 10;
    let converted = client.convert_currency(&base, &quote, &amount);
    assert_eq!(converted, 20);
}

#[test]
fn test_exchange_rate_staleness_rejected() {
    let (env, owner, _employer, _arbiter, client) = create_test_env();

    let base = Address::generate(&env);
    let quote = Address::generate(&env);
    let rate: i128 = 1_000_000;

    // configure max age to 1 second
    let contract_address = client.address.clone();
    env.as_contract(&contract_address, || {
        DataKey::set_exchange_rate_max_age_seconds(&env, 1u64);
    });

    client.set_exchange_rate(&owner, &base, &quote, &rate);

    // advance ledger far beyond max age
    env.ledger().with_mut(|li| li.timestamp += 10u64);

    let res = client.try_convert_currency(&base, &quote, &10i128);
    assert!(res.is_err());
}

#[test]
fn test_exchange_rate_deviation_rejected() {
    let (env, owner, _employer, _arbiter, client) = create_test_env();

    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    // initial rate
    let r1: i128 = 1_000_000; // 1.0
    client.set_exchange_rate(&owner, &base, &quote, &r1);

    // set max deviation to 100 bps (1%)
    let contract_address = client.address.clone();
    env.as_contract(&contract_address, || {
        DataKey::set_exchange_rate_max_deviation_bps(&env, 100u32);
    });

    // attempt a >1% update should be rejected
    let r2 = r1 + (r1 / 50); // +2%
    let res = client.try_set_exchange_rate(&owner, &base, &quote, &r2);
    assert!(res.is_err());
}

/// End‑to‑end test for `claim_payroll_in_token`:
///
/// - Agreement is denominated in `base_token`.
/// - Escrow is funded in `payout_token`.
/// - Employee claims one period and is paid in `payout_token` using FX rate.
#[test]
fn test_claim_payroll_in_different_token_uses_fx_rate() {
    let (env, owner, employer, _arbiter, client) = create_test_env();

    // ---------------------------------------------------------------------
    // Token setup
    // ---------------------------------------------------------------------
    let base_admin = Address::generate(&env);
    let base_token = env.register_stellar_asset_contract_v2(base_admin).address();

    let payout_admin = Address::generate(&env);
    let payout_token = env
        .register_stellar_asset_contract_v2(payout_admin)
        .address();

    // FX: 1 base = 2 payout.
    let fx_rate: i128 = 2_000_000;
    client.set_exchange_rate(&owner, &base_token, &payout_token, &fx_rate);

    // ---------------------------------------------------------------------
    // Agreement + employee setup
    // ---------------------------------------------------------------------
    let grace_period: u64 = 604_800; // 7 days
    let period_seconds: u64 = 86_400; // 1 day
    let salary_per_period: i128 = 1_000;

    let agreement_id = client.create_payroll_agreement(&employer, &base_token, &grace_period);

    let employee = Address::generate(&env);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary_per_period);

    // Activate agreement so claims are allowed after setup.
    client.activate_agreement(&agreement_id);

    // ---------------------------------------------------------------------
    // Seed DataKey metadata and escrow for the payout token.
    // ---------------------------------------------------------------------
    let contract_address = client.address.clone();

    // Fund payout token escrow for this agreement.
    let escrow_total: i128 = 20_000;
    let payout_client = StellarAssetClient::new(&env, &payout_token);
    payout_client.mint(&contract_address, &escrow_total);

    env.as_contract(&contract_address, || {
        let now = env.ledger().timestamp();

        DataKey::set_agreement_activation_time(&env, agreement_id, now);
        DataKey::set_agreement_period_duration(&env, agreement_id, period_seconds);
        DataKey::set_agreement_token(&env, agreement_id, &base_token);

        // Single employee at index 0
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary_per_period);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee_count(&env, agreement_id, 1);

        // Escrow funded in payout token only.
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &payout_token, escrow_total);
    });

    // Advance one full period so exactly one salary is claimable.
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds;
    });

    // ---------------------------------------------------------------------
    // Employee claims in payout_token.
    // ---------------------------------------------------------------------
    client.claim_payroll_in_token(&employee, &agreement_id, &0u32, &payout_token);

    // Employee receives salary in payout token using FX rate.
    let payout_token_client = soroban_sdk::token::Client::new(&env, &payout_token);
    let expected_payout: i128 = salary_per_period * 2; // 1_000 base × 2 = 2_000 payout
    assert_eq!(payout_token_client.balance(&employee), expected_payout);

    // Escrow balance and paid amount are updated correctly.
    env.as_contract(&contract_address, || {
        let remaining = DataKey::get_agreement_escrow_balance(&env, agreement_id, &payout_token);
        assert_eq!(remaining, escrow_total - expected_payout);

        let paid = DataKey::get_agreement_paid_amount(&env, agreement_id);
        assert_eq!(paid, salary_per_period);
    });
}

// =============================================================================
// Rounding / precision tests  (issue: convert_amount edge cases)
//
// Documented convention (see convert_amount doc comment in payroll.rs):
//
//   converted = (amount * rate) / FX_SCALE   (floor / truncation toward zero)
//   FX_SCALE  = 1_000_000
//   DUST_THRESHOLD = 1
//
// Dust guard: if converted < DUST_THRESHOLD the call returns
// ExchangeRateInvalid so no period is ever marked claimed with a zero payout.
// =============================================================================

// ---------------------------------------------------------------------------
// convert_currency (pure conversion helper) — rounding-to-zero cases
// ---------------------------------------------------------------------------

/// Smallest possible amount (1) with a rate just below parity (999_999 / 1e6)
/// produces (1 * 999_999) / 1_000_000 = 0 after floor division.
/// The dust guard MUST reject this with ExchangeRateInvalid.
#[test]
fn test_convert_amount_rounds_to_zero_is_rejected() {
    let (env, owner, _employer, _arbiter, client) = create_test_env();

    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    // rate = 999_999 / 1_000_000 = 0.999999  → just below 1:1 parity
    let rate: i128 = 999_999;
    client.set_exchange_rate(&owner, &base, &quote, &rate);

    // amount=1: (1 * 999_999) / 1_000_000 = 0  → must be rejected
    let result = client.try_convert_currency(&base, &quote, &1i128);
    assert_eq!(
        result,
        Err(Ok(PayrollError::ExchangeRateInvalid)),
        "converting amount=1 at rate=999_999 must return ExchangeRateInvalid (dust guard)"
    );
}

/// Confirms the boundary: rate=1_000_000 (exactly 1:1 parity) with amount=1
/// gives converted=1, which meets DUST_THRESHOLD and must succeed.
#[test]
fn test_convert_amount_at_parity_one_unit_succeeds() {
    let (env, owner, _employer, _arbiter, client) = create_test_env();

    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    let rate: i128 = 1_000_000; // exactly 1:1
    client.set_exchange_rate(&owner, &base, &quote, &rate);

    let converted = client.convert_currency(&base, &quote, &1i128);
    assert_eq!(converted, 1, "1 base at 1:1 rate must yield exactly 1 quote");
}

/// Any rate strictly less than FX_SCALE (< 1_000_000) applied to amount=1
/// rounds to zero.  Use a highly sub-parity rate (1) as an extreme case.
#[test]
fn test_convert_amount_extreme_low_rate_rounds_to_zero() {
    let (env, owner, _employer, _arbiter, client) = create_test_env();

    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    // rate = 1 / 1_000_000 — deeply sub-parity
    let rate: i128 = 1;
    client.set_exchange_rate(&owner, &base, &quote, &rate);

    let result = client.try_convert_currency(&base, &quote, &1i128);
    assert_eq!(
        result,
        Err(Ok(PayrollError::ExchangeRateInvalid)),
        "rate=1 with amount=1 must be caught by dust guard"
    );
}

/// With the same extreme rate, a large-enough amount eventually escapes the
/// dust guard: amount=1_000_001 * rate=1 → 1_000_001 / 1_000_000 = 1.
#[test]
fn test_convert_amount_large_amount_escapes_dust_guard_at_low_rate() {
    let (env, owner, _employer, _arbiter, client) = create_test_env();

    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    let rate: i128 = 1; // effectively 0.000001 per base unit
    client.set_exchange_rate(&owner, &base, &quote, &rate);

    // (1_000_001 * 1) / 1_000_000 = 1  →  should succeed
    let converted = client.convert_currency(&base, &quote, &1_000_001i128);
    assert_eq!(converted, 1);
}

// ---------------------------------------------------------------------------
// convert_currency — high-precision (fractional) rate cases
// ---------------------------------------------------------------------------

/// rate = 1_500_000 (1.5x).  Floor of 4.5 is 4 — confirms truncation, not
/// rounding, and that sub-unit precision isn't silently over-credited.
#[test]
fn test_convert_amount_fractional_rate_floors_correctly() {
    let (env, owner, _employer, _arbiter, client) = create_test_env();

    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    // 1 base = 1.5 quote
    let rate: i128 = 1_500_000;
    client.set_exchange_rate(&owner, &base, &quote, &rate);

    // (3 * 1_500_000) / 1_000_000 = 4_500_000 / 1_000_000 = 4  (floor, not 5)
    let converted = client.convert_currency(&base, &quote, &3i128);
    assert_eq!(converted, 4, "floor(3 * 1.5) must be 4, not 5");
}

/// rate = 1_500_000 with an even amount that divides cleanly — confirms no
/// precision loss when the product is exactly divisible.
#[test]
fn test_convert_amount_fractional_rate_exact_multiple() {
    let (env, owner, _employer, _arbiter, client) = create_test_env();

    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    let rate: i128 = 1_500_000; // 1.5x
    client.set_exchange_rate(&owner, &base, &quote, &rate);

    // (2 * 1_500_000) / 1_000_000 = 3_000_000 / 1_000_000 = 3  (exact)
    let converted = client.convert_currency(&base, &quote, &2i128);
    assert_eq!(converted, 3, "2 base at 1.5x rate must yield exactly 3 quote");
}

/// rate = 1_333_333 (≈ 1.333333x).  Verifies that sub-unit precision in the
/// rate itself is handled via floor, not accumulated rounding error.
#[test]
fn test_convert_amount_high_precision_rate_no_excess_precision_loss() {
    let (env, owner, _employer, _arbiter, client) = create_test_env();

    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    // Approximately 1.333333 (4/3)
    let rate: i128 = 1_333_333;
    client.set_exchange_rate(&owner, &base, &quote, &rate);

    // (3 * 1_333_333) / 1_000_000 = 3_999_999 / 1_000_000 = 3  (floor)
    let converted = client.convert_currency(&base, &quote, &3i128);
    assert_eq!(
        converted, 3,
        "floor(3 * 1.333333) must be 3, not 4 — no over-crediting"
    );

    // (6 * 1_333_333) / 1_000_000 = 7_999_998 / 1_000_000 = 7  (floor)
    let converted6 = client.convert_currency(&base, &quote, &6i128);
    assert_eq!(
        converted6, 7,
        "floor(6 * 1.333333) must be 7 — confirms consistent floor across larger amounts"
    );
}

// ---------------------------------------------------------------------------
// claim_payroll_in_token — end-to-end dust guard: claim is fully blocked
//
// Security requirement (from issue): a claim that rounds to zero must NOT
// mark the period as claimed.  If it did, the employee's salary would be
// burned silently with no token payout.
// ---------------------------------------------------------------------------

/// Full end-to-end test: salary=1 at a rate that converts to 0 must cause
/// claim_payroll_in_token to return ExchangeRateInvalid and leave
/// claimed_periods unchanged (the period is NOT consumed).
#[test]
fn test_claim_payroll_in_token_rounding_to_zero_rejects_and_does_not_burn_period() {
    let (env, owner, employer, _arbiter, client) = create_test_env();

    // Token setup
    let base_admin = Address::generate(&env);
    let base_token = env.register_stellar_asset_contract_v2(base_admin).address();
    let payout_admin = Address::generate(&env);
    let payout_token = env
        .register_stellar_asset_contract_v2(payout_admin)
        .address();

    // rate = 999_999 / 1_000_000 < 1  →  salary=1 converts to 0 (dust guard fires)
    let rate: i128 = 999_999;
    client.set_exchange_rate(&owner, &base_token, &payout_token, &rate);

    let grace_period: u64 = 604_800;
    let period_seconds: u64 = 86_400;
    // salary = 1  →  amount_payout = floor(1 * 999_999 / 1_000_000) = 0
    let salary_per_period: i128 = 1;

    let agreement_id =
        client.create_payroll_agreement(&employer, &base_token, &grace_period);

    let employee = Address::generate(&env);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary_per_period);
    client.activate_agreement(&agreement_id);

    let contract_address = client.address.clone();
    let escrow_total: i128 = 10_000;
    let payout_client = StellarAssetClient::new(&env, &payout_token);
    payout_client.mint(&contract_address, &escrow_total);

    env.as_contract(&contract_address, || {
        let now = env.ledger().timestamp();
        DataKey::set_agreement_activation_time(&env, agreement_id, now);
        DataKey::set_agreement_period_duration(&env, agreement_id, period_seconds);
        DataKey::set_agreement_token(&env, agreement_id, &base_token);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary_per_period);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee_count(&env, agreement_id, 1);
        DataKey::set_agreement_escrow_balance(
            &env,
            agreement_id,
            &payout_token,
            escrow_total,
        );
    });

    env.ledger().with_mut(|li| li.timestamp += period_seconds);

    // Claim must be rejected — the conversion rounds to zero.
    let result =
        client.try_claim_payroll_in_token(&employee, &agreement_id, &0u32, &payout_token);
    assert_eq!(
        result,
        Err(Ok(PayrollError::ExchangeRateInvalid)),
        "claim with salary=1 at sub-parity rate must return ExchangeRateInvalid"
    );

    // Critical security check: claimed_periods must still be 0 — the period
    // was NOT consumed.  If it were consumed the employee could never reclaim.
    env.as_contract(&contract_address, || {
        let claimed =
            DataKey::get_employee_claimed_periods(&env, agreement_id, 0);
        assert_eq!(
            claimed, 0,
            "period must NOT be marked claimed when payout converts to zero"
        );
    });

    // Employee balance must remain zero — no tokens were transferred.
    let payout_token_client = soroban_sdk::token::Client::new(&env, &payout_token);
    assert_eq!(
        payout_token_client.balance(&employee),
        0,
        "employee must receive no tokens when conversion rounds to zero"
    );

    // Escrow balance is unchanged.
    env.as_contract(&contract_address, || {
        let balance =
            DataKey::get_agreement_escrow_balance(&env, agreement_id, &payout_token);
        assert_eq!(balance, escrow_total, "escrow must be untouched on rejected claim");
    });
}

/// Accumulating two periods makes the total salary large enough to escape the
/// dust guard.  Confirms the intended workaround (accumulate periods) works.
#[test]
fn test_claim_payroll_in_token_accumulated_periods_escape_dust_guard() {
    let (env, owner, employer, _arbiter, client) = create_test_env();

    let base_admin = Address::generate(&env);
    let base_token = env.register_stellar_asset_contract_v2(base_admin).address();
    let payout_admin = Address::generate(&env);
    let payout_token = env
        .register_stellar_asset_contract_v2(payout_admin)
        .address();

    // rate = 500_001 / 1_000_000 ≈ 0.500001
    // salary=1 per period → 1 period → floor(1 * 500_001 / 1_000_000) = 0  (dust)
    // salary=1 per period → 2 periods → floor(2 * 500_001 / 1_000_000) = 1  (ok)
    let rate: i128 = 500_001;
    client.set_exchange_rate(&owner, &base_token, &payout_token, &rate);

    let grace_period: u64 = 604_800;
    let period_seconds: u64 = 86_400;
    let salary_per_period: i128 = 1;

    let agreement_id =
        client.create_payroll_agreement(&employer, &base_token, &grace_period);
    let employee = Address::generate(&env);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary_per_period);
    client.activate_agreement(&agreement_id);

    let contract_address = client.address.clone();
    let escrow_total: i128 = 10_000;
    let payout_client = StellarAssetClient::new(&env, &payout_token);
    payout_client.mint(&contract_address, &escrow_total);

    env.as_contract(&contract_address, || {
        let now = env.ledger().timestamp();
        DataKey::set_agreement_activation_time(&env, agreement_id, now);
        DataKey::set_agreement_period_duration(&env, agreement_id, period_seconds);
        DataKey::set_agreement_token(&env, agreement_id, &base_token);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary_per_period);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee_count(&env, agreement_id, 1);
        DataKey::set_agreement_escrow_balance(
            &env,
            agreement_id,
            &payout_token,
            escrow_total,
        );
    });

    // Advance TWO full periods.
    env.ledger()
        .with_mut(|li| li.timestamp += period_seconds * 2);

    // Now amount_base = 1 * 2 = 2 → floor(2 * 500_001 / 1_000_000) = 1 → ok
    client.claim_payroll_in_token(&employee, &agreement_id, &0u32, &payout_token);

    let payout_token_client = soroban_sdk::token::Client::new(&env, &payout_token);
    assert_eq!(
        payout_token_client.balance(&employee),
        1,
        "two accumulated periods must yield 1 quote token after floor division"
    );

    // Both periods are now consumed.
    env.as_contract(&contract_address, || {
        let claimed =
            DataKey::get_employee_claimed_periods(&env, agreement_id, 0);
        assert_eq!(claimed, 2, "both periods must be marked claimed after successful payout");
    });
}

/// End-to-end high-precision rate: salary=3 at rate=1_500_000 (1.5x) must pay
/// out floor(4.5) = 4 quote tokens (not 5), confirming no over-crediting.
#[test]
fn test_claim_payroll_in_token_fractional_rate_floors_not_rounds() {
    let (env, owner, employer, _arbiter, client) = create_test_env();

    let base_admin = Address::generate(&env);
    let base_token = env.register_stellar_asset_contract_v2(base_admin).address();
    let payout_admin = Address::generate(&env);
    let payout_token = env
        .register_stellar_asset_contract_v2(payout_admin)
        .address();

    // 1.5x rate: floor(3 * 1.5) = 4, NOT 5
    let rate: i128 = 1_500_000;
    client.set_exchange_rate(&owner, &base_token, &payout_token, &rate);

    let grace_period: u64 = 604_800;
    let period_seconds: u64 = 86_400;
    let salary_per_period: i128 = 3;

    let agreement_id =
        client.create_payroll_agreement(&employer, &base_token, &grace_period);
    let employee = Address::generate(&env);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary_per_period);
    client.activate_agreement(&agreement_id);

    let contract_address = client.address.clone();
    let escrow_total: i128 = 10_000;
    let payout_client = StellarAssetClient::new(&env, &payout_token);
    payout_client.mint(&contract_address, &escrow_total);

    env.as_contract(&contract_address, || {
        let now = env.ledger().timestamp();
        DataKey::set_agreement_activation_time(&env, agreement_id, now);
        DataKey::set_agreement_period_duration(&env, agreement_id, period_seconds);
        DataKey::set_agreement_token(&env, agreement_id, &base_token);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary_per_period);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee_count(&env, agreement_id, 1);
        DataKey::set_agreement_escrow_balance(
            &env,
            agreement_id,
            &payout_token,
            escrow_total,
        );
    });

    env.ledger().with_mut(|li| li.timestamp += period_seconds);

    client.claim_payroll_in_token(&employee, &agreement_id, &0u32, &payout_token);

    let payout_token_client = soroban_sdk::token::Client::new(&env, &payout_token);
    // floor(3 * 1_500_000 / 1_000_000) = floor(4.5) = 4
    assert_eq!(
        payout_token_client.balance(&employee),
        4,
        "floor(salary=3 * rate=1.5) must be 4, confirming truncation not rounding"
    );
}

/// Overflow guard: i128::MAX amount with a rate > 1 overflows
/// `amount * rate` and must return ExchangeRateOverflow (not panic).
#[test]
fn test_convert_amount_overflow_returns_error_not_panic() {
    let (env, owner, _employer, _arbiter, client) = create_test_env();

    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    // rate = 2_000_000 — any amount > i128::MAX / 2_000_000 will overflow
    let rate: i128 = 2_000_000;
    client.set_exchange_rate(&owner, &base, &quote, &rate);

    // i128::MAX overflows when multiplied by 2
    let result = client.try_convert_currency(&base, &quote, &i128::MAX);
    assert_eq!(
        result,
        Err(Ok(PayrollError::ExchangeRateOverflow)),
        "i128::MAX * rate=2 must return ExchangeRateOverflow, not panic"
    );
}
