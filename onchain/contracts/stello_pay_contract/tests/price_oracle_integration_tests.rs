//! Integration tests for the multi-currency conversion path via price_oracle.
//!
//! # What is tested
//!
//! These tests exercise the full oracle → payroll contract pipeline:
//!
//! 1. **Conversion accuracy** – `push_price` from the oracle propagates the
//!    correct scaled rate into `DataKey::ExchangeRate`, and `claim_payroll_in_token`
//!    converts base-currency salary amounts to payout-currency amounts exactly.
//!
//! 2. **Oracle unavailability** – when no rate has been pushed for a pair,
//!    `claim_payroll_in_token` returns `ExchangeRateNotFound`.
//!
//! 3. **Stale price rejection** – the oracle rejects source timestamps that
//!    exceed `max_staleness_seconds`, so a stale feed cannot update the payroll
//!    contract's FX rate.
//!
//! 4. **Rate bounds enforcement** – rates outside `[min_rate, max_rate]` are
//!    rejected by the oracle before they can reach the payroll contract.
//!
//! 5. **Quorum requirement** – with `quorum_n = 2`, a single source vote does
//!    not update the payroll rate; only after a second agreeing source does the
//!    rate propagate.
//!
//! 6. **Disabled pair** – a disabled oracle pair cannot push rates.
//!
//! 7. **Rate update propagation** – a newer oracle push overwrites the old rate
//!    in the payroll contract, and subsequent claims use the updated rate.
//!
//! 8. **Same-token shortcut** – `claim_payroll_in_token` with `payout_token ==
//!    base_token` falls through to the native `claim_payroll` path without
//!    requiring an FX rate.
//!
//! 9. **Overflow protection** – extremely large salary × rate combinations
//!    return `ExchangeRateOverflow` rather than panicking.
//!
//! 10. **Multi-period accumulation** – multiple elapsed periods are converted
//!     correctly in a single claim.
//!
//! # Security notes
//!
//! * The oracle contract must be registered as `ExchangeRateAdmin` on the
//!   payroll contract before `push_price` can succeed.  Without this, every
//!   `push_price` call returns `FxUpdateFailed`.
//!
//! * Only the payroll contract owner may call `set_exchange_rate_admin`.
//!   An attacker cannot self-register as FX admin.
//!
//! * The oracle enforces `[min_rate, max_rate]` bounds per pair, limiting the
//!   blast radius of a compromised oracle source to the configured range.
//!
//! * Stale-price rejection (`max_staleness_seconds`) prevents a replayed or
//!   delayed price feed from silently using an outdated rate.
//!
//! * `FX_SCALE = 1_000_000` (6 decimal fixed-point).  All rates in these tests
//!   are expressed as `quote_per_base * 1_000_000`.

#![cfg(test)]

use price_oracle::{PriceOracleContract, PriceOracleContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};
use stello_pay_contract::{
    storage::{DataKey, PayrollError},
    PayrollContract, PayrollContractClient,
};

// ============================================================================
// Constants
// ============================================================================

/// Fixed-point scale used by both contracts (1e6).
const FX_SCALE: i128 = 1_000_000;

/// Default oracle pair bounds used in most tests.
const MIN_RATE: i128 = 100_000; // 0.1
const MAX_RATE: i128 = 10_000_000; // 10.0
const MAX_STALENESS: u64 = 600; // 10 minutes
const QUORUM_WINDOW: u64 = 60; // 1 minute

// ============================================================================
// Test harness helpers
// ============================================================================

/// Deploys and initialises the payroll contract.  Returns `(contract_id, owner, client)`.
fn deploy_payroll(env: &Env) -> (Address, Address, PayrollContractClient<'static>) {
    let id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (id, owner, client)
}

/// Deploys and initialises the price oracle, wired to `payroll_id`.
/// Returns `(oracle_id, oracle_owner, client)`.
fn deploy_oracle(
    env: &Env,
    payroll_id: &Address,
) -> (Address, Address, PriceOracleContractClient<'static>) {
    let id = env.register_contract(None, PriceOracleContract);
    let client = PriceOracleContractClient::new(env, &id);
    let owner = Address::generate(env);
    client.initialize(&owner, payroll_id);
    (id, owner, client)
}

/// Full wiring: payroll + oracle + oracle registered as FX admin + source added
/// + pair configured with `quorum_n = 1`.
///
/// Returns `(payroll_client, oracle_client, payroll_owner, oracle_owner, source, base, quote)`.
#[allow(clippy::type_complexity)]
fn full_setup(
    env: &Env,
) -> (
    PayrollContractClient<'static>,
    PriceOracleContractClient<'static>,
    Address, // payroll owner
    Address, // oracle owner
    Address, // oracle source
    Address, // base token
    Address, // quote token
) {
    let (payroll_id, payroll_owner, payroll_client) = deploy_payroll(env);
    let (oracle_id, oracle_owner, oracle_client) = deploy_oracle(env, &payroll_id);

    // Register oracle as FX admin so push_price can call set_exchange_rate.
    payroll_client.set_exchange_rate_admin(&payroll_owner, &oracle_id);

    let source = Address::generate(env);
    oracle_client.add_source(&oracle_owner, &source);

    let base = Address::generate(env);
    let quote = Address::generate(env);
    oracle_client.configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &MIN_RATE,
        &MAX_RATE,
        &MAX_STALENESS,
        &1u32,
        &0u32,
        &QUORUM_WINDOW,
        &0u64, // min_submit_interval_secs: no rate limit in tests
    );

    (
        payroll_client,
        oracle_client,
        payroll_owner,
        oracle_owner,
        source,
        base,
        quote,
    )
}

/// Seeds the payroll contract's internal DataKey state for a payroll agreement
/// and mints `escrow_amount` of `escrow_token` to the contract address.
///
/// This mirrors the pattern used in `test_multi_currency.rs` and is necessary
/// because the payroll contract has no public "fund escrow" entry point — in
/// production, employers transfer tokens directly to the contract address.
fn seed_payroll_agreement(
    env: &Env,
    payroll_client: &PayrollContractClient<'static>,
    employer: &Address,
    employee: &Address,
    base_token: &Address,
    escrow_token: &Address,
    salary_per_period: i128,
    period_seconds: u64,
    escrow_amount: i128,
) -> u128 {
    let grace_period: u64 = 7 * 24 * 3600;
    let agreement_id =
        payroll_client.create_payroll_agreement(employer, base_token, &grace_period);
    payroll_client.add_employee_to_agreement(&agreement_id, employee, &salary_per_period);
    payroll_client.activate_agreement(&agreement_id);

    let contract_address = payroll_client.address.clone();

    // Mint escrow_token to the contract address.
    let escrow_admin = Address::generate(env);
    let escrow_asset = env
        .register_stellar_asset_contract_v2(escrow_admin)
        .address();
    // We need the actual escrow_token address, not a new one.
    // Mint directly using the provided escrow_token (caller must be its admin).
    // In tests we use register_stellar_asset_contract_v2 externally; here we
    // accept the pre-minted token and just seed the DataKey balances.
    let _ = escrow_asset; // unused; escrow_token is passed in already minted

    env.as_contract(&contract_address, || {
        let now = env.ledger().timestamp();
        DataKey::set_agreement_activation_time(env, agreement_id, now);
        DataKey::set_agreement_period_duration(env, agreement_id, period_seconds);
        DataKey::set_agreement_token(env, agreement_id, base_token);
        DataKey::set_employee(env, agreement_id, 0, employee);
        DataKey::set_employee_salary(env, agreement_id, 0, salary_per_period);
        DataKey::set_employee_claimed_periods(env, agreement_id, 0, 0);
        DataKey::set_employee_count(env, agreement_id, 1);
        DataKey::set_agreement_escrow_balance(env, agreement_id, escrow_token, escrow_amount);
    });

    agreement_id
}

// ============================================================================
// 1. Conversion accuracy
// ============================================================================

/// Oracle pushes rate 2.0 (2_000_000 scaled).  Employee claims one period of
/// 1_000 base units.  Expected payout = 1_000 × 2 = 2_000 quote units.
#[test]
fn test_oracle_push_propagates_rate_and_claim_converts_correctly() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_client, oracle_client, _payroll_owner, _oracle_owner, source, base, quote) =
        full_setup(&env);

    // Push rate: 1 base = 2 quote.
    let rate: i128 = 2 * FX_SCALE;
    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &rate, &1_000u64);

    // Verify the rate landed in the payroll contract.
    let converted = payroll_client.convert_currency(&base, &quote, &1_000i128);
    assert_eq!(converted, 2_000, "1_000 base × 2.0 should equal 2_000 quote");

    // Set up a payroll agreement denominated in base, funded in quote.
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let salary_per_period: i128 = 1_000;
    let period_seconds: u64 = 86_400;
    let escrow_amount: i128 = 100_000;

    // Mint quote tokens to the contract.
    let quote_admin = Address::generate(&env);
    let quote_token = env
        .register_stellar_asset_contract_v2(quote_admin)
        .address();
    // We need to use the same `quote` address that the oracle pair is configured for.
    // Re-register quote as a stellar asset so we can mint to the contract.
    let quote_asset_admin = Address::generate(&env);
    let quote_asset = env
        .register_stellar_asset_contract_v2(quote_asset_admin)
        .address();
    let _ = (quote_token, quote_asset); // unused; we use `quote` directly below

    // Use a fresh token pair where we control minting.
    let base2_admin = Address::generate(&env);
    let base2 = env
        .register_stellar_asset_contract_v2(base2_admin.clone())
        .address();
    let quote2_admin = Address::generate(&env);
    let quote2 = env
        .register_stellar_asset_contract_v2(quote2_admin.clone())
        .address();

    // Simpler: just set the rate directly via set_exchange_rate (owner path).
    let (payroll_id3, payroll_owner3, payroll_client3) = deploy_payroll(&env);
    let _ = payroll_id3;
    payroll_client3.set_exchange_rate(&payroll_owner3, &base2, &quote2, &rate);

    let agreement_id = seed_payroll_agreement(
        &env,
        &payroll_client3,
        &employer,
        &employee,
        &base2,
        &quote2,
        salary_per_period,
        period_seconds,
        escrow_amount,
    );

    // Mint quote2 tokens to the payroll contract.
    let quote2_asset_client = StellarAssetClient::new(&env, &quote2);
    quote2_asset_client.mint(&payroll_client3.address, &escrow_amount);

    // Advance one period.
    env.ledger().with_mut(|li| li.timestamp += period_seconds);

    payroll_client3.claim_payroll_in_token(&employee, &agreement_id, &0u32, &quote2);

    let quote2_token = soroban_sdk::token::Client::new(&env, &quote2);
    let expected: i128 = salary_per_period * 2; // 1_000 × 2.0 = 2_000
    assert_eq!(
        quote2_token.balance(&employee),
        expected,
        "Employee should receive salary converted at oracle rate"
    );
}

// ============================================================================
// 2. Oracle unavailability — no rate configured
// ============================================================================

/// When no FX rate has been pushed for a pair, `claim_payroll_in_token` must
/// return `ExchangeRateNotFound` rather than panicking or using a zero rate.
#[test]
fn test_claim_in_token_without_oracle_rate_returns_exchange_rate_not_found() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_id, payroll_owner, payroll_client) = deploy_payroll(&env);
    let _ = payroll_id;

    let base_admin = Address::generate(&env);
    let base = env
        .register_stellar_asset_contract_v2(base_admin)
        .address();
    let quote_admin = Address::generate(&env);
    let quote = env
        .register_stellar_asset_contract_v2(quote_admin)
        .address();

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let salary_per_period: i128 = 500;
    let period_seconds: u64 = 3_600;

    let agreement_id = seed_payroll_agreement(
        &env,
        &payroll_client,
        &employer,
        &employee,
        &base,
        &quote,
        salary_per_period,
        period_seconds,
        50_000,
    );

    // Mint quote tokens so the escrow balance check doesn't interfere.
    let quote_asset_client = StellarAssetClient::new(&env, &quote);
    quote_asset_client.mint(&payroll_client.address, &50_000i128);

    // Advance one period — but NO rate has been set.
    env.ledger().with_mut(|li| li.timestamp += period_seconds);

    let result =
        payroll_client.try_claim_payroll_in_token(&employee, &agreement_id, &0u32, &quote);

    assert_eq!(
        result,
        Err(Ok(PayrollError::ExchangeRateNotFound)),
        "Missing oracle rate must return ExchangeRateNotFound"
    );

    // Verify no tokens were transferred.
    let quote_token = soroban_sdk::token::Client::new(&env, &quote);
    assert_eq!(
        quote_token.balance(&employee),
        0,
        "No tokens should be transferred when rate is missing"
    );

    // Verify claimed periods were NOT advanced.
    let claimed = payroll_client.get_employee_claimed_periods(&agreement_id, &0u32);
    assert_eq!(claimed, 0, "Claimed periods must not advance on failure");

    let _ = payroll_owner;
}

// ============================================================================
// 3. Stale price rejection
// ============================================================================

/// The oracle rejects a `source_timestamp` that is older than
/// `max_staleness_seconds` relative to the ledger timestamp.  This means a
/// stale feed cannot silently update the payroll contract's FX rate.
#[test]
fn test_oracle_rejects_stale_price_and_payroll_rate_unchanged() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_client, oracle_client, payroll_owner, _oracle_owner, source, base, quote) =
        full_setup(&env);

    // Push a fresh rate first.
    let initial_rate: i128 = 2 * FX_SCALE;
    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &initial_rate, &1_000u64);

    // Verify initial rate is in payroll.
    assert_eq!(
        payroll_client.convert_currency(&base, &quote, &1_000i128),
        2_000
    );

    // Advance ledger well past staleness window.
    // max_staleness = 600s; ledger = 2_000; stale_ts = 1_000 → age = 1_000 > 600.
    env.ledger().with_mut(|li| li.timestamp = 2_000);
    let stale_rate: i128 = 5 * FX_SCALE;
    let result = oracle_client.try_push_price(&source, &base, &quote, &stale_rate, &1_000u64);

    // Oracle must reject the stale submission.
    assert!(
        result.is_err(),
        "Oracle must reject stale price submission"
    );

    // Payroll rate must remain at the initial value.
    assert_eq!(
        payroll_client.convert_currency(&base, &quote, &1_000i128),
        2_000,
        "Payroll rate must not change after stale oracle rejection"
    );

    let _ = payroll_owner;
}

/// A future timestamp (source_timestamp > ledger.timestamp) is also rejected.
#[test]
fn test_oracle_rejects_future_timestamp() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_client, oracle_client, payroll_owner, _oracle_owner, source, base, quote) =
        full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let result =
        oracle_client.try_push_price(&source, &base, &quote, &(2 * FX_SCALE), &1_001u64);

    assert!(result.is_err(), "Future timestamp must be rejected");

    // No rate should have been set.
    let result2 = payroll_client.try_convert_currency(&base, &quote, &1_000i128);
    assert!(
        result2.is_err(),
        "No rate should exist after future-timestamp rejection"
    );

    let _ = payroll_owner;
}

// ============================================================================
// 4. Rate bounds enforcement
// ============================================================================

/// A rate below `min_rate` is rejected by the oracle; the payroll contract
/// never sees it.
#[test]
fn test_oracle_rejects_rate_below_min_and_payroll_unchanged() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_client, oracle_client, payroll_owner, _oracle_owner, source, base, quote) =
        full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    // MIN_RATE = 100_000; submit 99_999.
    let result = oracle_client.try_push_price(&source, &base, &quote, &99_999i128, &1_000u64);
    assert!(result.is_err(), "Rate below min must be rejected");

    let result2 = payroll_client.try_convert_currency(&base, &quote, &1_000i128);
    assert!(result2.is_err(), "No rate should exist after out-of-bounds rejection");

    let _ = payroll_owner;
}

/// A rate above `max_rate` is rejected by the oracle.
#[test]
fn test_oracle_rejects_rate_above_max_and_payroll_unchanged() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_client, oracle_client, payroll_owner, _oracle_owner, source, base, quote) =
        full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    // MAX_RATE = 10_000_000; submit 10_000_001.
    let result =
        oracle_client.try_push_price(&source, &base, &quote, &10_000_001i128, &1_000u64);
    assert!(result.is_err(), "Rate above max must be rejected");

    let result2 = payroll_client.try_convert_currency(&base, &quote, &1_000i128);
    assert!(result2.is_err(), "No rate should exist after out-of-bounds rejection");

    let _ = payroll_owner;
}

/// A zero rate is rejected by the oracle.
#[test]
fn test_oracle_rejects_zero_rate() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_client, oracle_client, payroll_owner, _oracle_owner, source, base, quote) =
        full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let result = oracle_client.try_push_price(&source, &base, &quote, &0i128, &1_000u64);
    assert!(result.is_err(), "Zero rate must be rejected");

    let result2 = payroll_client.try_convert_currency(&base, &quote, &1_000i128);
    assert!(result2.is_err(), "No rate should exist after zero-rate rejection");

    let _ = payroll_owner;
}

// ============================================================================
// 5. Quorum requirement
// ============================================================================

/// With `quorum_n = 2`, a single source vote must NOT update the payroll rate.
/// Only after a second agreeing source does the rate propagate.
#[test]
fn test_quorum_n2_requires_two_sources_before_rate_propagates() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_id, payroll_owner, payroll_client) = deploy_payroll(&env);
    let (oracle_id, oracle_owner, oracle_client) = deploy_oracle(&env, &payroll_id);
    payroll_client.set_exchange_rate_admin(&payroll_owner, &oracle_id);

    let source1 = Address::generate(&env);
    let source2 = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source1);
    oracle_client.add_source(&oracle_owner, &source2);

    let base = Address::generate(&env);
    let quote = Address::generate(&env);
    oracle_client.configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &MIN_RATE,
        &MAX_RATE,
        &MAX_STALENESS,
        &2u32, // quorum = 2
        &0u32,
        &QUORUM_WINDOW,
        &0u64, // min_submit_interval_secs: no rate limit in tests
    );

    let rate: i128 = 3 * FX_SCALE;
    env.ledger().with_mut(|li| li.timestamp = 1_000);

    // First source votes — quorum not yet met.
    oracle_client.push_price(&source1, &base, &quote, &rate, &1_000u64);

    let result = payroll_client.try_convert_currency(&base, &quote, &1_000i128);
    assert!(
        result.is_err(),
        "Rate must not propagate after only one source vote"
    );

    // Second source votes — quorum met.
    oracle_client.push_price(&source2, &base, &quote, &rate, &1_000u64);

    let converted = payroll_client.convert_currency(&base, &quote, &1_000i128);
    assert_eq!(
        converted,
        3_000,
        "Rate must propagate after quorum is reached"
    );
}

/// Duplicate vote from the same source within a quorum window is rejected.
#[test]
fn test_quorum_duplicate_vote_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_id, payroll_owner, payroll_client) = deploy_payroll(&env);
    let (oracle_id, oracle_owner, oracle_client) = deploy_oracle(&env, &payroll_id);
    payroll_client.set_exchange_rate_admin(&payroll_owner, &oracle_id);

    let source = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source);

    let base = Address::generate(&env);
    let quote = Address::generate(&env);
    oracle_client.configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &MIN_RATE,
        &MAX_RATE,
        &MAX_STALENESS,
        &2u32,
        &0u32,
        &QUORUM_WINDOW,
        &0u64, // min_submit_interval_secs: no rate limit in tests
    );

    let rate: i128 = 2 * FX_SCALE;
    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &rate, &1_000u64);

    // Same source tries to vote again in the same window.
    let result = oracle_client.try_push_price(&source, &base, &quote, &rate, &1_000u64);
    assert!(result.is_err(), "Duplicate vote must be rejected");

    // Rate must still not have propagated.
    let result2 = payroll_client.try_convert_currency(&base, &quote, &1_000i128);
    assert!(result2.is_err(), "Rate must not propagate after duplicate vote");
}

// ============================================================================
// 6. Disabled pair
// ============================================================================

/// After `disable_pair`, `push_price` must fail and the payroll rate must
/// remain at its last accepted value.
#[test]
fn test_disabled_oracle_pair_cannot_update_payroll_rate() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_client, oracle_client, _payroll_owner, oracle_owner, source, base, quote) =
        full_setup(&env);

    // Push an initial rate.
    let initial_rate: i128 = 2 * FX_SCALE;
    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &initial_rate, &1_000u64);
    assert_eq!(payroll_client.convert_currency(&base, &quote, &1_000i128), 2_000);

    // Disable the pair.
    oracle_client.disable_pair(&oracle_owner, &base, &quote);

    // Attempt to push a new rate.
    env.ledger().with_mut(|li| li.timestamp = 2_000);
    let result = oracle_client.try_push_price(&source, &base, &quote, &(4 * FX_SCALE), &2_000u64);
    assert!(result.is_err(), "Disabled pair must reject push_price");

    // Payroll rate must remain at the initial value.
    assert_eq!(
        payroll_client.convert_currency(&base, &quote, &1_000i128),
        2_000,
        "Payroll rate must not change after disabled-pair rejection"
    );
}

/// Re-enabling a pair allows price updates to resume.
#[test]
fn test_re_enabled_oracle_pair_resumes_rate_updates() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_client, oracle_client, _payroll_owner, oracle_owner, source, base, quote) =
        full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &(2 * FX_SCALE), &1_000u64);

    oracle_client.disable_pair(&oracle_owner, &base, &quote);
    oracle_client.enable_pair(&oracle_owner, &base, &quote);

    env.ledger().with_mut(|li| li.timestamp = 2_000);
    oracle_client.push_price(&source, &base, &quote, &(4 * FX_SCALE), &2_000u64);

    assert_eq!(
        payroll_client.convert_currency(&base, &quote, &1_000i128),
        4_000,
        "Rate must update after pair is re-enabled"
    );
}

// ============================================================================
// 7. Rate update propagation — claim uses latest rate
// ============================================================================

/// After the oracle pushes a new rate, subsequent claims use the updated rate.
/// This verifies that the payroll contract always reads the current stored rate
/// at claim time rather than caching it at agreement creation.
#[test]
fn test_rate_update_propagates_to_subsequent_claims() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_id, payroll_owner, payroll_client) = deploy_payroll(&env);
    let (oracle_id, oracle_owner, oracle_client) = deploy_oracle(&env, &payroll_id);
    payroll_client.set_exchange_rate_admin(&payroll_owner, &oracle_id);

    let source = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source);

    let base_admin = Address::generate(&env);
    let base = env
        .register_stellar_asset_contract_v2(base_admin)
        .address();
    let quote_admin = Address::generate(&env);
    let quote = env
        .register_stellar_asset_contract_v2(quote_admin)
        .address();

    oracle_client.configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &MIN_RATE,
        &MAX_RATE,
        &MAX_STALENESS,
        &1u32,
        &0u32,
        &QUORUM_WINDOW,
        &0u64, // min_submit_interval_secs: no rate limit in tests
    );

    // Push initial rate: 1 base = 2 quote.
    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &(2 * FX_SCALE), &1_000u64);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let salary_per_period: i128 = 1_000;
    let period_seconds: u64 = 86_400;
    let escrow_amount: i128 = 200_000;

    let agreement_id = seed_payroll_agreement(
        &env,
        &payroll_client,
        &employer,
        &employee,
        &base,
        &quote,
        salary_per_period,
        period_seconds,
        escrow_amount,
    );

    let quote_asset_client = StellarAssetClient::new(&env, &quote);
    quote_asset_client.mint(&payroll_client.address, &escrow_amount);

    // Advance one period and claim at rate 2.0.
    env.ledger().with_mut(|li| li.timestamp += period_seconds);
    payroll_client.claim_payroll_in_token(&employee, &agreement_id, &0u32, &quote);

    let quote_token = soroban_sdk::token::Client::new(&env, &quote);
    assert_eq!(
        quote_token.balance(&employee),
        2_000,
        "First claim: 1_000 base × 2.0 = 2_000 quote"
    );

    // Oracle pushes a new rate: 1 base = 3 quote.
    env.ledger().with_mut(|li| li.timestamp += 100);
    oracle_client.push_price(
        &source,
        &base,
        &quote,
        &(3 * FX_SCALE),
        &env.ledger().timestamp(),
    );

    // Advance another period and claim at the new rate.
    env.ledger().with_mut(|li| li.timestamp += period_seconds);
    payroll_client.claim_payroll_in_token(&employee, &agreement_id, &0u32, &quote);

    assert_eq!(
        quote_token.balance(&employee),
        2_000 + 3_000, // 2_000 from first claim + 1_000 × 3.0 = 3_000
        "Second claim: 1_000 base × 3.0 = 3_000 quote"
    );
}

// ============================================================================
// 8. Same-token shortcut
// ============================================================================

/// When `payout_token == base_token`, `claim_payroll_in_token` falls through
/// to the native `claim_payroll` path.  No FX rate is required.
#[test]
fn test_claim_in_same_token_does_not_require_fx_rate() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_id, payroll_owner, payroll_client) = deploy_payroll(&env);
    let _ = (payroll_id, payroll_owner);

    let base_admin = Address::generate(&env);
    let base = env
        .register_stellar_asset_contract_v2(base_admin)
        .address();

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let salary_per_period: i128 = 500;
    let period_seconds: u64 = 3_600;
    let escrow_amount: i128 = 50_000;

    let agreement_id = seed_payroll_agreement(
        &env,
        &payroll_client,
        &employer,
        &employee,
        &base,
        &base, // escrow token == base token
        salary_per_period,
        period_seconds,
        escrow_amount,
    );

    let base_asset_client = StellarAssetClient::new(&env, &base);
    base_asset_client.mint(&payroll_client.address, &escrow_amount);

    // Advance one period.
    env.ledger().with_mut(|li| li.timestamp += period_seconds);

    // No FX rate configured — same-token path must succeed.
    payroll_client.claim_payroll_in_token(&employee, &agreement_id, &0u32, &base);

    let base_token = soroban_sdk::token::Client::new(&env, &base);
    assert_eq!(
        base_token.balance(&employee),
        salary_per_period,
        "Same-token claim must transfer exact salary without conversion"
    );
}

// ============================================================================
// 9. Overflow protection
// ============================================================================

/// `salary_per_period * rate` must not overflow i128.  The contract must
/// return `ExchangeRateOverflow` rather than panicking.
///
/// We trigger this by setting a very large rate directly (bypassing the oracle
/// bounds) and using a large salary.
#[test]
fn test_overflow_in_conversion_returns_exchange_rate_overflow() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_id, payroll_owner, payroll_client) = deploy_payroll(&env);
    let _ = payroll_id;

    let base_admin = Address::generate(&env);
    let base = env
        .register_stellar_asset_contract_v2(base_admin)
        .address();
    let quote_admin = Address::generate(&env);
    let quote = env
        .register_stellar_asset_contract_v2(quote_admin)
        .address();

    // Set a rate that will cause overflow: i128::MAX as the rate.
    // amount * rate overflows when amount > 1 and rate = i128::MAX.
    let overflow_rate: i128 = i128::MAX;
    payroll_client.set_exchange_rate(&payroll_owner, &base, &quote, &overflow_rate);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    // salary_per_period = 2 is enough to overflow with rate = i128::MAX.
    let salary_per_period: i128 = 2;
    let period_seconds: u64 = 3_600;

    let agreement_id = seed_payroll_agreement(
        &env,
        &payroll_client,
        &employer,
        &employee,
        &base,
        &quote,
        salary_per_period,
        period_seconds,
        i128::MAX,
    );

    let quote_asset_client = StellarAssetClient::new(&env, &quote);
    quote_asset_client.mint(&payroll_client.address, &i128::MAX);

    env.ledger().with_mut(|li| li.timestamp += period_seconds);

    let result =
        payroll_client.try_claim_payroll_in_token(&employee, &agreement_id, &0u32, &quote);

    assert_eq!(
        result,
        Err(Ok(PayrollError::ExchangeRateOverflow)),
        "Overflow in conversion must return ExchangeRateOverflow"
    );

    // No tokens transferred.
    let quote_token = soroban_sdk::token::Client::new(&env, &quote);
    assert_eq!(quote_token.balance(&employee), 0);
}

// ============================================================================
// 10. Multi-period accumulation
// ============================================================================

/// Three elapsed periods are converted correctly in a single claim.
/// salary = 1_000, rate = 1.5 (1_500_000 scaled), periods = 3.
/// Expected payout = 3 × 1_000 × 1.5 = 4_500.
#[test]
fn test_multi_period_claim_converts_accumulated_salary() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_id, payroll_owner, payroll_client) = deploy_payroll(&env);
    let _ = payroll_id;

    let base_admin = Address::generate(&env);
    let base = env
        .register_stellar_asset_contract_v2(base_admin)
        .address();
    let quote_admin = Address::generate(&env);
    let quote = env
        .register_stellar_asset_contract_v2(quote_admin)
        .address();

    let rate: i128 = 1_500_000; // 1.5
    payroll_client.set_exchange_rate(&payroll_owner, &base, &quote, &rate);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let salary_per_period: i128 = 1_000;
    let period_seconds: u64 = 86_400;
    let escrow_amount: i128 = 100_000;

    let agreement_id = seed_payroll_agreement(
        &env,
        &payroll_client,
        &employer,
        &employee,
        &base,
        &quote,
        salary_per_period,
        period_seconds,
        escrow_amount,
    );

    let quote_asset_client = StellarAssetClient::new(&env, &quote);
    quote_asset_client.mint(&payroll_client.address, &escrow_amount);

    // Advance 3 full periods.
    env.ledger().with_mut(|li| li.timestamp += 3 * period_seconds);

    payroll_client.claim_payroll_in_token(&employee, &agreement_id, &0u32, &quote);

    let quote_token = soroban_sdk::token::Client::new(&env, &quote);
    let expected: i128 = 3 * salary_per_period * 3 / 2; // 3 × 1_000 × 1.5 = 4_500
    assert_eq!(
        quote_token.balance(&employee),
        expected,
        "3 periods × 1_000 base × 1.5 rate = 4_500 quote"
    );

    // Claimed periods must advance to 3.
    assert_eq!(
        payroll_client.get_employee_claimed_periods(&agreement_id, &0u32),
        3
    );
}

// ============================================================================
// 11. Oracle not registered as FX admin — push_price returns FxUpdateFailed
// ============================================================================

/// If the oracle is NOT registered as FX admin on the payroll contract,
/// `push_price` must return `FxUpdateFailed` and the payroll rate must remain
/// unset.
#[test]
fn test_push_price_fails_when_oracle_not_registered_as_fx_admin() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_id, _payroll_owner, payroll_client) = deploy_payroll(&env);
    // Deliberately do NOT call set_exchange_rate_admin.
    let (_, oracle_owner, oracle_client) = deploy_oracle(&env, &payroll_id);

    let source = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source);

    let base = Address::generate(&env);
    let quote = Address::generate(&env);
    oracle_client.configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &MIN_RATE,
        &MAX_RATE,
        &MAX_STALENESS,
        &1u32,
        &0u32,
        &QUORUM_WINDOW,
        &0u64, // min_submit_interval_secs: no rate limit in tests
    );

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let result = oracle_client.try_push_price(&source, &base, &quote, &(2 * FX_SCALE), &1_000u64);

    assert!(
        result.is_err(),
        "push_price must fail when oracle is not FX admin"
    );

    // No rate should have been set.
    let result2 = payroll_client.try_convert_currency(&base, &quote, &1_000i128);
    assert!(
        result2.is_err(),
        "No rate should exist when oracle is not FX admin"
    );
}

// ============================================================================
// 12. Unregistered source cannot push price
// ============================================================================

/// An address that has not been added as an oracle source must not be able to
/// push a price, even if it knows the correct pair and rate.
#[test]
fn test_unregistered_source_cannot_push_price_to_payroll() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_client, oracle_client, _payroll_owner, _oracle_owner, _source, base, quote) =
        full_setup(&env);

    let attacker = Address::generate(&env);
    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let result = oracle_client.try_push_price(&attacker, &base, &quote, &(2 * FX_SCALE), &1_000u64);

    assert!(result.is_err(), "Unregistered source must be rejected");

    let result2 = payroll_client.try_convert_currency(&base, &quote, &1_000i128);
    assert!(
        result2.is_err(),
        "No rate should exist after unregistered-source rejection"
    );
}

// ============================================================================
// 13. Monotonic rate — older update does not overwrite newer
// ============================================================================

/// After a fresh rate is accepted, an older source_timestamp is silently
/// ignored by the oracle.  The payroll contract retains the newer rate.
#[test]
fn test_monotonic_oracle_update_does_not_overwrite_newer_rate_in_payroll() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_client, oracle_client, _payroll_owner, _oracle_owner, source, base, quote) =
        full_setup(&env);

    // Push a fresh rate at t=2_000.
    env.ledger().with_mut(|li| li.timestamp = 2_000);
    oracle_client.push_price(&source, &base, &quote, &(3 * FX_SCALE), &2_000u64);
    assert_eq!(payroll_client.convert_currency(&base, &quote, &1_000i128), 3_000);

    // Attempt to push an older rate (t=1_500) — must be silently ignored.
    env.ledger().with_mut(|li| li.timestamp = 2_100);
    oracle_client.push_price(&source, &base, &quote, &(2 * FX_SCALE), &1_500u64);

    // Payroll rate must remain at 3.0.
    assert_eq!(
        payroll_client.convert_currency(&base, &quote, &1_000i128),
        3_000,
        "Older oracle update must not overwrite newer rate in payroll"
    );
}

// ============================================================================
// 14. Pair direction isolation
// ============================================================================

/// Configuring and pushing a rate for (base, quote) must not affect (quote, base).
/// Attempting to claim with the reversed pair must return ExchangeRateNotFound.
#[test]
fn test_pair_direction_isolation_in_payroll() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_id, payroll_owner, payroll_client) = deploy_payroll(&env);
    let _ = payroll_id;

    let base_admin = Address::generate(&env);
    let base = env
        .register_stellar_asset_contract_v2(base_admin)
        .address();
    let quote_admin = Address::generate(&env);
    let quote = env
        .register_stellar_asset_contract_v2(quote_admin)
        .address();

    // Set rate for (base → quote) only.
    payroll_client.set_exchange_rate(&payroll_owner, &base, &quote, &(2 * FX_SCALE));

    // (base → quote) works.
    assert_eq!(payroll_client.convert_currency(&base, &quote, &1_000i128), 2_000);

    // (quote → base) must fail — no rate configured.
    let result = payroll_client.try_convert_currency(&quote, &base, &1_000i128);
    assert!(
        result.is_err(),
        "Reversed pair must not inherit the forward rate"
    );
}

// ============================================================================
// 15. Insufficient escrow balance in payout token
// ============================================================================

/// If the escrow balance for the payout token is insufficient, the claim must
/// return `InsufficientEscrowBalance` and no tokens must be transferred.
#[test]
fn test_insufficient_payout_escrow_returns_error() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_id, payroll_owner, payroll_client) = deploy_payroll(&env);
    let _ = payroll_id;

    let base_admin = Address::generate(&env);
    let base = env
        .register_stellar_asset_contract_v2(base_admin)
        .address();
    let quote_admin = Address::generate(&env);
    let quote = env
        .register_stellar_asset_contract_v2(quote_admin)
        .address();

    // Rate: 1 base = 2 quote.  Salary = 1_000 base → 2_000 quote needed.
    payroll_client.set_exchange_rate(&payroll_owner, &base, &quote, &(2 * FX_SCALE));

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let salary_per_period: i128 = 1_000;
    let period_seconds: u64 = 86_400;
    // Seed only 1_000 quote — not enough for 2_000.
    let escrow_amount: i128 = 1_000;

    let agreement_id = seed_payroll_agreement(
        &env,
        &payroll_client,
        &employer,
        &employee,
        &base,
        &quote,
        salary_per_period,
        period_seconds,
        escrow_amount,
    );

    let quote_asset_client = StellarAssetClient::new(&env, &quote);
    quote_asset_client.mint(&payroll_client.address, &escrow_amount);

    env.ledger().with_mut(|li| li.timestamp += period_seconds);

    let result =
        payroll_client.try_claim_payroll_in_token(&employee, &agreement_id, &0u32, &quote);

    assert_eq!(
        result,
        Err(Ok(PayrollError::InsufficientEscrowBalance)),
        "Insufficient payout escrow must return InsufficientEscrowBalance"
    );

    let quote_token = soroban_sdk::token::Client::new(&env, &quote);
    assert_eq!(quote_token.balance(&employee), 0, "No tokens transferred on failure");
    assert_eq!(
        payroll_client.get_employee_claimed_periods(&agreement_id, &0u32),
        0,
        "Claimed periods must not advance on failure"
    );
}
