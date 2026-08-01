#![cfg(test)]
#![allow(deprecated)]

use price_oracle::{OracleError, PriceOracleContract, PriceOracleContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// Helper constant — halt disabled in all legacy tests.
const NO_HALT: u32 = 0;

const DEFAULT_TOLERANCE_BPS: u32 = 0;
const DEFAULT_QUORUM_WINDOW_SECONDS: u64 = 60;

// ===========================================================================
// Helpers
// ===========================================================================

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_payroll(env: &Env) -> (Address, Address, PayrollContractClient<'static>) {
    let payroll_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &payroll_id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (payroll_id, owner, client)
}

fn setup_oracle(
    env: &Env,
    payroll_id: &Address,
) -> (Address, PriceOracleContractClient<'static>, Address) {
    let oracle_id = env.register_contract(None, PriceOracleContract);
    let client = PriceOracleContractClient::new(env, &oracle_id);
    let owner = Address::generate(env);
    client.initialize(&owner, payroll_id);
    (oracle_id, client, owner)
}

fn configure_pair_with_settings(
    oracle_client: &PriceOracleContractClient<'static>,
    oracle_owner: &Address,
    base: &Address,
    quote: &Address,
    min_rate: i128,
    max_rate: i128,
    max_staleness_seconds: u64,
    quorum_n: u32,
    tolerance_bps: u32,
    quorum_window_seconds: u64,
) {
    oracle_client.configure_pair(
        oracle_owner,
        base,
        quote,
        &min_rate,
        &max_rate,
        &max_staleness_seconds,
        &quorum_n,
        &tolerance_bps,
        &quorum_window_seconds,
        &0u64,    // no rate limit by default in tests
        &NO_HALT, // halt disabled by default in tests
    );
}

/// Full setup: payroll + oracle + FX admin registered + source + pair configured.
fn full_setup(
    env: &Env,
) -> (
    PriceOracleContractClient<'static>,
    PayrollContractClient<'static>,
    Address, // oracle owner
    Address, // source
    Address, // base
    Address, // quote
) {
    let (payroll_id, payroll_owner, payroll_client) = setup_payroll(env);
    let (oracle_id, oracle_client, oracle_owner) = setup_oracle(env, &payroll_id);
    payroll_client.set_exchange_rate_admin(&payroll_owner, &oracle_id);

    let source = Address::generate(env);
    oracle_client.add_source(&oracle_owner, &source);

    let base = Address::generate(env);
    let quote = Address::generate(env);
    configure_pair_with_settings(
        &oracle_client,
        &oracle_owner,
        &base,
        &quote,
        500_000i128,   // min 0.5
        5_000_000i128, // max 5.0
        600u64,        // 10 min staleness
        1u32,          // quorum
        DEFAULT_TOLERANCE_BPS,
        DEFAULT_QUORUM_WINDOW_SECONDS,
    );
    // NB: min_submit_interval_secs = 0, max_consecutive_stale_before_halt = 0 (both disabled).

    (
        oracle_client,
        payroll_client,
        oracle_owner,
        source,
        base,
        quote,
    )
}

// ===========================================================================
// 1. Initialization
// ===========================================================================

#[test]
fn test_initialize_sets_owner() {
    let env = create_env();
    let (payroll_id, _, _) = setup_payroll(&env);
    let (_, oracle_client, oracle_owner) = setup_oracle(&env, &payroll_id);

    assert_eq!(oracle_client.get_owner().unwrap(), oracle_owner);
}

#[test]
fn test_initialize_twice_returns_error() {
    let env = create_env();
    let (payroll_id, _, _) = setup_payroll(&env);
    let oracle_id = env.register_contract(None, PriceOracleContract);
    let client = PriceOracleContractClient::new(&env, &oracle_id);
    let owner = Address::generate(&env);

    client.initialize(&owner, &payroll_id);
    let res = client.try_initialize(&owner, &payroll_id);
    assert_eq!(res, Err(Ok(OracleError::AlreadyInitialized)));
}

// ===========================================================================
// 2. Source management
// ===========================================================================

#[test]
fn test_add_and_remove_source() {
    let env = create_env();
    let (payroll_id, _, _) = setup_payroll(&env);
    let (_, client, owner) = setup_oracle(&env, &payroll_id);
    let source = Address::generate(&env);

    client.add_source(&owner, &source);
    assert!(client.is_source_address(&source));

    client.remove_source(&owner, &source);
    assert!(!client.is_source_address(&source));
}

#[test]
fn test_non_owner_cannot_add_source() {
    let env = create_env();
    let (payroll_id, _, _) = setup_payroll(&env);
    let (_, client, _owner) = setup_oracle(&env, &payroll_id);
    let attacker = Address::generate(&env);
    let source = Address::generate(&env);

    let res = client.try_add_source(&attacker, &source);
    assert_eq!(res, Err(Ok(OracleError::NotAuthorized)));
}

#[test]
fn test_non_owner_cannot_remove_source() {
    let env = create_env();
    let (payroll_id, _, _) = setup_payroll(&env);
    let (_, client, owner) = setup_oracle(&env, &payroll_id);
    let attacker = Address::generate(&env);
    let source = Address::generate(&env);

    client.add_source(&owner, &source);

    let res = client.try_remove_source(&attacker, &source);
    assert_eq!(res, Err(Ok(OracleError::NotAuthorized)));
}

#[test]
fn test_removed_source_cannot_push_price() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source, base, quote) = full_setup(&env);

    oracle_client.remove_source(&oracle_owner, &source);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let res = oracle_client.try_push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);
    assert_eq!(res, Err(Ok(OracleError::InvalidSource)));
}

// ===========================================================================
// 3. Pair configuration
// ===========================================================================

#[test]
fn test_configure_pair_and_read_config() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, _, _, _) = full_setup(&env);
    let base2 = Address::generate(&env);
    let quote2 = Address::generate(&env);

    oracle_client.configure_pair(
        &oracle_owner,
        &base2,
        &quote2,
        &1_000_000i128,
        &3_000_000i128,
        &300u64,
        &1u32,
        &DEFAULT_TOLERANCE_BPS,
        &DEFAULT_QUORUM_WINDOW_SECONDS,
        &0u64,
        &NO_HALT,
    );

    let cfg = oracle_client.get_pair_config(&base2, &quote2).unwrap();
    assert_eq!(cfg.min_rate, 1_000_000);
    assert_eq!(cfg.max_rate, 3_000_000);
    assert_eq!(cfg.max_staleness_seconds, 300);
    assert!(cfg.enabled);
    assert_eq!(cfg.quorum_n, 1);
    assert_eq!(cfg.tolerance_bps, DEFAULT_TOLERANCE_BPS);
    assert_eq!(cfg.quorum_window_seconds, DEFAULT_QUORUM_WINDOW_SECONDS);
    assert_eq!(cfg.min_submit_interval_secs, 0);
}

#[test]
fn test_configure_pair_same_base_quote_rejected() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, _, _, _) = full_setup(&env);
    let token = Address::generate(&env);

    let res = oracle_client.try_configure_pair(
        &oracle_owner,
        &token,
        &token,
        &1_000_000i128,
        &2_000_000i128,
        &300u64,
        &1u32,
        &DEFAULT_TOLERANCE_BPS,
        &DEFAULT_QUORUM_WINDOW_SECONDS,
        &0u64,
        &NO_HALT,
    );
    assert_eq!(res, Err(Ok(OracleError::InvalidPairConfig)));
}

#[test]
fn test_configure_pair_min_greater_than_max_rejected() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, _, _, _) = full_setup(&env);
    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    let res = oracle_client.try_configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &3_000_000i128,
        &1_000_000i128,
        &300u64,
        &1u32,
        &DEFAULT_TOLERANCE_BPS,
        &DEFAULT_QUORUM_WINDOW_SECONDS,
        &0u64,
        &NO_HALT,
    );
    assert_eq!(res, Err(Ok(OracleError::InvalidPairConfig)));
}

#[test]
fn test_configure_pair_zero_min_rate_rejected() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, _, _, _) = full_setup(&env);
    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    let res = oracle_client.try_configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &0i128,
        &2_000_000i128,
        &300u64,
        &1u32,
        &DEFAULT_TOLERANCE_BPS,
        &DEFAULT_QUORUM_WINDOW_SECONDS,
        &0u64,
        &NO_HALT,
    );
    assert_eq!(res, Err(Ok(OracleError::InvalidPairConfig)));
}

#[test]
fn test_configure_pair_negative_rate_rejected() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, _, _, _) = full_setup(&env);
    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    let res = oracle_client.try_configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &-1i128,
        &2_000_000i128,
        &300u64,
        &1u32,
        &DEFAULT_TOLERANCE_BPS,
        &DEFAULT_QUORUM_WINDOW_SECONDS,
        &0u64,
        &NO_HALT,
    );
    assert_eq!(res, Err(Ok(OracleError::InvalidPairConfig)));
}

#[test]
fn test_configure_pair_zero_staleness_rejected() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, _, _, _) = full_setup(&env);
    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    let res = oracle_client.try_configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &1_000_000i128,
        &2_000_000i128,
        &0u64,
        &1u32,
        &DEFAULT_TOLERANCE_BPS,
        &DEFAULT_QUORUM_WINDOW_SECONDS,
        &0u64,
        &NO_HALT,
    );
    assert_eq!(res, Err(Ok(OracleError::InvalidPairConfig)));
}

#[test]
fn test_configure_pair_zero_quorum_window_rejected() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, _, _, _) = full_setup(&env);
    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    let res = oracle_client.try_configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &1_000_000i128,
        &2_000_000i128,
        &300u64,
        &1u32,
        &DEFAULT_TOLERANCE_BPS,
        &0u64,
        &0u64,
        &NO_HALT,
    );
    assert_eq!(res, Err(Ok(OracleError::InvalidPairConfig)));
}

#[test]
fn test_non_owner_cannot_configure_pair() {
    let env = create_env();
    let (oracle_client, _, _, _, _, _) = full_setup(&env);
    let attacker = Address::generate(&env);
    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    let res = oracle_client.try_configure_pair(
        &attacker,
        &base,
        &quote,
        &1_000_000i128,
        &2_000_000i128,
        &300u64,
        &1u32,
        &DEFAULT_TOLERANCE_BPS,
        &DEFAULT_QUORUM_WINDOW_SECONDS,
        &0u64,
        &NO_HALT,
    );
    assert_eq!(res, Err(Ok(OracleError::NotAuthorized)));
}

// ===========================================================================
// 4. Disable / enable pair
// ===========================================================================

#[test]
fn test_disable_pair_blocks_updates() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source, base, quote) = full_setup(&env);

    oracle_client.disable_pair(&oracle_owner, &base, &quote);

    let cfg = oracle_client.get_pair_config(&base, &quote).unwrap();
    assert!(!cfg.enabled);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let res = oracle_client.try_push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);
    assert_eq!(res, Err(Ok(OracleError::PairNotConfigured)));
}

#[test]
fn test_enable_pair_resumes_updates() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source, base, quote) = full_setup(&env);

    oracle_client.disable_pair(&oracle_owner, &base, &quote);
    oracle_client.enable_pair(&oracle_owner, &base, &quote);

    let cfg = oracle_client.get_pair_config(&base, &quote).unwrap();
    assert!(cfg.enabled);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let res = oracle_client.try_push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);
    assert!(res.is_ok());
}

#[test]
fn test_disable_unconfigured_pair_returns_error() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, _, _, _) = full_setup(&env);
    let b = Address::generate(&env);
    let q = Address::generate(&env);

    let res = oracle_client.try_disable_pair(&oracle_owner, &b, &q);
    assert_eq!(res, Err(Ok(OracleError::PairNotConfigured)));
}

#[test]
fn test_non_owner_cannot_disable_pair() {
    let env = create_env();
    let (oracle_client, _, _, _, base, quote) = full_setup(&env);
    let attacker = Address::generate(&env);

    let res = oracle_client.try_disable_pair(&attacker, &base, &quote);
    assert_eq!(res, Err(Ok(OracleError::NotAuthorized)));
}

/// Disabling a pair clears the stored PairState, making get_pair_state
/// return PairNotConfigured and push_price reject new submissions.
#[test]
fn test_disable_pair_clears_state_and_blocks_reads() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source, base, quote) = full_setup(&env);

    // Push a fresh price.
    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);
    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 2_000_000);

    // Disable the pair.
    oracle_client.disable_pair(&oracle_owner, &base, &quote);
    let cfg = oracle_client.get_pair_config(&base, &quote).unwrap();
    assert!(!cfg.enabled);

    // get_pair_state must NOT return the old cached state — pair is disabled.
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PairNotConfigured))
    );

    // push_price must also be rejected for a disabled pair.
    assert_eq!(
        oracle_client.try_push_price(&source, &base, &quote, &3_000_000i128, &1_000u64),
        Err(Ok(OracleError::PairNotConfigured))
    );
}

/// A disabled pair that still holds a fresh (non-stale) cached price is
/// clearly distinguishable from an enabled pair with a fresh price.
/// get_pair_state returns PairNotConfigured for the disabled pair even
/// though the cached rate has not yet aged past max_staleness_seconds.
#[test]
fn test_disabled_pair_get_pair_state_distinguishable_from_fresh() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source, base, quote) = full_setup(&env);

    // Push a fresh price.
    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);

    // Configure and push a fresh price to a second (reference) pair.
    let base2 = Address::generate(&env);
    let quote2 = Address::generate(&env);
    oracle_client.configure_pair(
        &oracle_owner,
        &base2,
        &quote2,
        &500_000i128,
        &5_000_000i128,
        &600u64,
        &1u32,
        &DEFAULT_TOLERANCE_BPS,
        &DEFAULT_QUORUM_WINDOW_SECONDS,
        &0u64,
        &NO_HALT,
    );
    let source2 = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source2);
    oracle_client.push_price(&source2, &base2, &quote2, &2_500_000i128, &1_000u64);

    // Disable the first pair while both are still fresh.
    oracle_client.disable_pair(&oracle_owner, &base, &quote);

    // Enabled reference pair: succeeds, returns the fresh state.
    let state = oracle_client.get_pair_state(&base2, &quote2);
    assert_eq!(state.rate, 2_500_000);
    assert_eq!(state.last_updated_ts, 1_000);

    // Disabled pair: returns PairNotConfigured even though its cached
    // price is equally fresh (age is the same).
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PairNotConfigured))
    );
}

/// Re-enabling a previously disabled pair does NOT resurrect the old
/// pre-disable price.  A caller must push a new price before
/// get_pair_state will succeed again.
#[test]
fn test_enable_pair_does_not_resurrect_stale_price() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source, base, quote) = full_setup(&env);

    // Push a price, then disable the pair immediately.
    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);
    oracle_client.disable_pair(&oracle_owner, &base, &quote);

    // Re-enable.
    oracle_client.enable_pair(&oracle_owner, &base, &quote);
    assert!(
        oracle_client
            .get_pair_config(&base, &quote)
            .unwrap()
            .enabled
    );

    // get_pair_state still returns an error — the old price was cleared
    // on disable and no fresh push has happened since re-enable.
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PairNotConfigured))
    );

    // Push a brand-new price after re-enable.
    env.ledger().with_mut(|li| li.timestamp = 2_000);
    oracle_client.push_price(&source, &base, &quote, &3_000_000i128, &2_000u64);

    // Now get_pair_state succeeds with the NEW price.
    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 3_000_000);
    assert_eq!(state.last_updated_ts, 2_000);
    assert_eq!(state.last_source, source);
}

// ===========================================================================
// 5. Push price – happy path
// ===========================================================================

#[test]
fn test_push_price_success_and_payroll_integration() {
    let env = create_env();
    let (oracle_client, payroll_client, _, source, base, quote) = full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);

    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 2_000_000);
    assert_eq!(state.last_updated_ts, 1_000);
    assert_eq!(state.last_source, source);

    // Payroll contract should reflect the FX rate.
    let converted = payroll_client.convert_currency(&base, &quote, &10i128);
    assert_eq!(converted, 20);
}

#[test]
fn test_push_price_at_min_boundary() {
    let env = create_env();
    let (oracle_client, _, _, source, base, quote) = full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    // min_rate = 500_000
    let res = oracle_client.try_push_price(&source, &base, &quote, &500_000i128, &1_000u64);
    assert!(res.is_ok());

    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 500_000);
}

#[test]
fn test_push_price_at_max_boundary() {
    let env = create_env();
    let (oracle_client, _, _, source, base, quote) = full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    // max_rate = 5_000_000
    let res = oracle_client.try_push_price(&source, &base, &quote, &5_000_000i128, &1_000u64);
    assert!(res.is_ok());

    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 5_000_000);
}

#[test]
fn test_push_price_at_max_staleness_boundary() {
    let env = create_env();
    let (oracle_client, _, _, source, base, quote) = full_setup(&env);

    // max_staleness = 600s
    env.ledger().with_mut(|li| li.timestamp = 1_600);
    // source_ts = 1000, age = 600 => exactly at boundary
    let res = oracle_client.try_push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);
    assert!(res.is_ok());
}

// ===========================================================================
// 6. Push price – forbidden paths
// ===========================================================================

#[test]
fn test_unregistered_source_rejected() {
    let env = create_env();
    let (oracle_client, _, _, _, base, quote) = full_setup(&env);
    let unknown = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let res = oracle_client.try_push_price(&unknown, &base, &quote, &2_000_000i128, &1_000u64);
    assert_eq!(res, Err(Ok(OracleError::InvalidSource)));
}

#[test]
fn test_zero_rate_rejected() {
    let env = create_env();
    let (oracle_client, _, _, source, base, quote) = full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let res = oracle_client.try_push_price(&source, &base, &quote, &0i128, &1_000u64);
    assert_eq!(res, Err(Ok(OracleError::ZeroRate)));
}

#[test]
fn test_negative_rate_rejected() {
    let env = create_env();
    let (oracle_client, _, _, source, base, quote) = full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let res = oracle_client.try_push_price(&source, &base, &quote, &-1i128, &1_000u64);
    assert_eq!(res, Err(Ok(OracleError::ZeroRate)));
}

#[test]
fn test_rate_below_min_rejected() {
    let env = create_env();
    let (oracle_client, _, _, source, base, quote) = full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    // min_rate = 500_000, submit 499_999
    let res = oracle_client.try_push_price(&source, &base, &quote, &499_999i128, &1_000u64);
    assert_eq!(res, Err(Ok(OracleError::RateOutOfBounds)));
}

#[test]
fn test_rate_above_max_rejected() {
    let env = create_env();
    let (oracle_client, _, _, source, base, quote) = full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    // max_rate = 5_000_000, submit 5_000_001
    let res = oracle_client.try_push_price(&source, &base, &quote, &5_000_001i128, &1_000u64);
    assert_eq!(res, Err(Ok(OracleError::RateOutOfBounds)));
}

#[test]
fn test_future_timestamp_rejected() {
    let env = create_env();
    let (oracle_client, _, _, source, base, quote) = full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let res = oracle_client.try_push_price(&source, &base, &quote, &2_000_000i128, &1_001u64);
    assert_eq!(res, Err(Ok(OracleError::RateStale)));
}

#[test]
fn test_stale_timestamp_rejected() {
    let env = create_env();
    let (oracle_client, _, _, source, base, quote) = full_setup(&env);

    // max_staleness = 600, ledger = 1000, source_ts = 399 => age = 601
    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let res = oracle_client.try_push_price(&source, &base, &quote, &2_000_000i128, &399u64);
    assert_eq!(res, Err(Ok(OracleError::RateStale)));
}

#[test]
fn test_unconfigured_pair_rejected() {
    let env = create_env();
    let (oracle_client, _, _, source, _, _) = full_setup(&env);
    let unknown_base = Address::generate(&env);
    let unknown_quote = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let res = oracle_client.try_push_price(
        &source,
        &unknown_base,
        &unknown_quote,
        &2_000_000i128,
        &1_000u64,
    );
    assert_eq!(res, Err(Ok(OracleError::PairNotConfigured)));
}

// ===========================================================================
// 7. Monotonic updates and multi-source
// ===========================================================================

#[test]
fn test_monotonic_ignores_older_update() {
    let env = create_env();
    let (oracle_client, _, _, source, base, quote) = full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 2_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &2_000u64);

    // Older timestamp is silently ignored.
    env.ledger().with_mut(|li| li.timestamp = 2_100);
    oracle_client.push_price(&source, &base, &quote, &1_500_000i128, &1_900u64);

    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 2_000_000);
    assert_eq!(state.last_updated_ts, 2_000);
}

#[test]
fn test_monotonic_ignores_equal_timestamp() {
    let env = create_env();
    let (oracle_client, _, _, source, base, quote) = full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 2_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &2_000u64);

    // Same timestamp with different rate — ignored.
    oracle_client.push_price(&source, &base, &quote, &3_000_000i128, &2_000u64);

    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 2_000_000);
}

#[test]
fn test_multi_source_latest_wins() {
    let env = create_env();
    let (oracle_client, payroll_client, oracle_owner, source, base, quote) = full_setup(&env);

    let backup = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &backup);

    // Primary reports.
    env.ledger().with_mut(|li| li.timestamp = 2_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &2_000u64);

    // Backup reports newer.
    env.ledger().with_mut(|li| li.timestamp = 2_100);
    oracle_client.push_price(&backup, &base, &quote, &3_000_000i128, &2_100u64);

    // Older primary update ignored.
    let _ = oracle_client.push_price(&source, &base, &quote, &1_500_000i128, &1_900u64);

    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 3_000_000);
    assert_eq!(state.last_source, backup);

    let converted = payroll_client.convert_currency(&base, &quote, &10i128);
    assert_eq!(converted, 30);
}

// ===========================================================================
// 8. Ownership transfer (two-step)
// ===========================================================================

#[test]
fn test_transfer_ownership_success() {
    let env = create_env();
    let (payroll_id, _, _) = setup_payroll(&env);
    let (_, client, owner) = setup_oracle(&env, &payroll_id);
    let new_owner = Address::generate(&env);

    client.propose_ownership(&owner, &new_owner);
    assert_eq!(client.get_owner().unwrap(), owner); // Still old owner

    client.accept_ownership(&new_owner);
    assert_eq!(client.get_owner().unwrap(), new_owner);
}

#[test]
fn test_new_owner_can_add_source() {
    let env = create_env();
    let (payroll_id, _, _) = setup_payroll(&env);
    let (_, client, owner) = setup_oracle(&env, &payroll_id);
    let new_owner = Address::generate(&env);

    client.propose_ownership(&owner, &new_owner);
    client.accept_ownership(&new_owner);

    let source = Address::generate(&env);
    client.add_source(&new_owner, &source);
    assert!(client.is_source_address(&source));
}

#[test]
fn test_old_owner_loses_admin_after_transfer() {
    let env = create_env();
    let (payroll_id, _, _) = setup_payroll(&env);
    let (_, client, owner) = setup_oracle(&env, &payroll_id);
    let new_owner = Address::generate(&env);

    client.propose_ownership(&owner, &new_owner);
    client.accept_ownership(&new_owner);

    let source = Address::generate(&env);
    let res = client.try_add_source(&owner, &source);
    assert_eq!(res, Err(Ok(OracleError::NotAuthorized)));
}

#[test]
fn test_non_owner_cannot_propose_ownership() {
    let env = create_env();
    let (payroll_id, _, _) = setup_payroll(&env);
    let (_, client, _owner) = setup_oracle(&env, &payroll_id);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);

    let res = client.try_propose_ownership(&attacker, &target);
    assert_eq!(res, Err(Ok(OracleError::NotAuthorized)));
}

#[test]
fn test_unauthorized_accept_rejection() {
    let env = create_env();
    let (payroll_id, _, _) = setup_payroll(&env);
    let (_, client, owner) = setup_oracle(&env, &payroll_id);
    let new_owner = Address::generate(&env);
    let attacker = Address::generate(&env);

    client.propose_ownership(&owner, &new_owner);

    let res = client.try_accept_ownership(&attacker);
    assert_eq!(res, Err(Ok(OracleError::NotAuthorized)));
    assert_eq!(client.get_owner().unwrap(), owner);
}

#[test]
fn test_accept_without_propose_fails() {
    let env = create_env();
    let (payroll_id, _, _) = setup_payroll(&env);
    let (_, client, owner) = setup_oracle(&env, &payroll_id);
    let attacker = Address::generate(&env);

    let res = client.try_accept_ownership(&attacker);
    assert_eq!(res, Err(Ok(OracleError::NotAuthorized)));
    assert_eq!(client.get_owner().unwrap(), owner);
}

#[test]
fn test_cancel_ownership_transfer() {
    let env = create_env();
    let (payroll_id, _, _) = setup_payroll(&env);
    let (_, client, owner) = setup_oracle(&env, &payroll_id);
    let new_owner = Address::generate(&env);

    client.propose_ownership(&owner, &new_owner);
    client.cancel_ownership_transfer(&owner);

    // After cancel, the pending owner cannot accept.
    let res = client.try_accept_ownership(&new_owner);
    assert_eq!(res, Err(Ok(OracleError::NotAuthorized)));
    assert_eq!(client.get_owner().unwrap(), owner);
}

#[test]
fn test_non_owner_cannot_cancel_transfer() {
    let env = create_env();
    let (payroll_id, _, _) = setup_payroll(&env);
    let (_, client, owner) = setup_oracle(&env, &payroll_id);
    let new_owner = Address::generate(&env);
    let attacker = Address::generate(&env);

    client.propose_ownership(&owner, &new_owner);

    let res = client.try_cancel_ownership_transfer(&attacker);
    assert_eq!(res, Err(Ok(OracleError::NotAuthorized)));
}

// ===========================================================================
// 9. Uninitialized guards
// ===========================================================================

#[test]
fn test_push_price_before_init_fails() {
    let env = create_env();
    let oracle_id = env.register_contract(None, PriceOracleContract);
    let client = PriceOracleContractClient::new(&env, &oracle_id);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);

    let res = client.try_push_price(&a, &b, &c, &1_000_000i128, &0u64);
    assert_eq!(res, Err(Ok(OracleError::NotInitialized)));
}

#[test]
fn test_add_source_before_init_fails() {
    let env = create_env();
    let oracle_id = env.register_contract(None, PriceOracleContract);
    let client = PriceOracleContractClient::new(&env, &oracle_id);
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    let res = client.try_add_source(&a, &b);
    assert_eq!(res, Err(Ok(OracleError::NotInitialized)));
}

#[test]
fn test_configure_pair_before_init_fails() {
    let env = create_env();
    let oracle_id = env.register_contract(None, PriceOracleContract);
    let client = PriceOracleContractClient::new(&env, &oracle_id);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);

    let res = client.try_configure_pair(
        &a,
        &b,
        &c,
        &1i128,
        &2i128,
        &1u64,
        &1u32,
        &DEFAULT_TOLERANCE_BPS,
        &DEFAULT_QUORUM_WINDOW_SECONDS,
        &0u64,
        &NO_HALT,
    );
    assert_eq!(res, Err(Ok(OracleError::NotInitialized)));
}

#[test]
fn test_disable_pair_before_init_fails() {
    let env = create_env();
    let oracle_id = env.register_contract(None, PriceOracleContract);
    let client = PriceOracleContractClient::new(&env, &oracle_id);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);

    let res = client.try_disable_pair(&a, &b, &c);
    assert_eq!(res, Err(Ok(OracleError::NotInitialized)));
}

#[test]
fn test_propose_ownership_before_init_fails() {
    let env = create_env();
    let oracle_id = env.register_contract(None, PriceOracleContract);
    let client = PriceOracleContractClient::new(&env, &oracle_id);
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    let res = client.try_propose_ownership(&a, &b);
    assert_eq!(res, Err(Ok(OracleError::NotInitialized)));
}

#[test]
fn test_accept_ownership_before_init_fails() {
    let env = create_env();
    let oracle_id = env.register_contract(None, PriceOracleContract);
    let client = PriceOracleContractClient::new(&env, &oracle_id);
    let a = Address::generate(&env);

    let res = client.try_accept_ownership(&a);
    assert_eq!(res, Err(Ok(OracleError::NotInitialized)));
}

// ===========================================================================
// 10. Security scenarios
// ===========================================================================

/// Oracle compromise blast radius: a compromised source can only push rates
/// within configured bounds. It cannot modify config, add sources, or
/// transfer ownership.
#[test]
fn test_compromised_source_blast_radius() {
    let env = create_env();
    let (oracle_client, _, _oracle_owner, source, base, quote) = full_setup(&env);

    // Source cannot add another source.
    let evil_source = Address::generate(&env);
    let res = oracle_client.try_add_source(&source, &evil_source);
    assert_eq!(res, Err(Ok(OracleError::NotAuthorized)));

    // Source cannot reconfigure pair bounds to widen them.
    let res = oracle_client.try_configure_pair(
        &source,
        &base,
        &quote,
        &1i128,
        &999_000_000i128,
        &86400u64,
        &1u32,
        &DEFAULT_TOLERANCE_BPS,
        &DEFAULT_QUORUM_WINDOW_SECONDS,
        &0u64,
        &NO_HALT,
    );
    assert_eq!(res, Err(Ok(OracleError::NotAuthorized)));

    // Source cannot transfer ownership.
    let res = oracle_client.try_propose_ownership(&source, &source);
    assert_eq!(res, Err(Ok(OracleError::NotAuthorized)));

    // Source cannot disable pair.
    let res = oracle_client.try_disable_pair(&source, &base, &quote);
    assert_eq!(res, Err(Ok(OracleError::NotAuthorized)));

    // Source CAN push a rate within bounds.
    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let res = oracle_client.try_push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);
    assert!(res.is_ok());

    // But cannot push outside bounds.
    env.ledger().with_mut(|li| li.timestamp = 2_000);
    let res = oracle_client.try_push_price(&source, &base, &quote, &50_000_000i128, &2_000u64);
    assert_eq!(res, Err(Ok(OracleError::RateOutOfBounds)));
}

/// Pair isolation: updating one pair does not affect another.
#[test]
fn test_pair_isolation() {
    let env = create_env();
    let (oracle_client, _, _oracle_owner, source, base, quote) = full_setup(&env);

    let base2 = Address::generate(&env);
    let quote2 = Address::generate(&env);
    oracle_client.configure_pair(
        &_oracle_owner,
        &base2,
        &quote2,
        &100_000i128,
        &9_000_000i128,
        &600u64,
        &1u32,
        &DEFAULT_TOLERANCE_BPS,
        &DEFAULT_QUORUM_WINDOW_SECONDS,
        &0u64,
        &NO_HALT,
    );

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);

    // Second pair has no state yet.
    assert_eq!(
        oracle_client.try_get_pair_state(&base2, &quote2),
        Err(Ok(OracleError::PairNotConfigured))
    );

    // Push to second pair.
    oracle_client.push_price(&source, &base2, &quote2, &4_000_000i128, &1_000u64);

    // Each pair has its own state.
    let s1 = oracle_client.get_pair_state(&base, &quote);
    let s2 = oracle_client.get_pair_state(&base2, &quote2);
    assert_eq!(s1.rate, 2_000_000);
    assert_eq!(s2.rate, 4_000_000);
}

/// Reconfigure pair: tightening bounds rejects previously valid rates.
#[test]
fn test_reconfigure_pair_tightens_bounds() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source, base, quote) = full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    // 4_500_000 is within [500_000, 5_000_000]
    oracle_client.push_price(&source, &base, &quote, &4_500_000i128, &1_000u64);

    // Tighten bounds.
    oracle_client.configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &1_000_000i128,
        &3_000_000i128,
        &600u64,
        &1u32,
        &DEFAULT_TOLERANCE_BPS,
        &DEFAULT_QUORUM_WINDOW_SECONDS,
        &0u64,
        &NO_HALT,
    );

    // Same rate now rejected.
    env.ledger().with_mut(|li| li.timestamp = 2_000);
    let res = oracle_client.try_push_price(&source, &base, &quote, &4_500_000i128, &2_000u64);
    assert_eq!(res, Err(Ok(OracleError::RateOutOfBounds)));
}

/// Direction matters: pair (A, B) is independent from pair (B, A).
#[test]
fn test_pair_direction_matters() {
    let env = create_env();
    let (oracle_client, _, _oracle_owner, source, base, quote) = full_setup(&env);

    // (base, quote) is configured; (quote, base) is not.
    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let res = oracle_client.try_push_price(&source, &quote, &base, &2_000_000i128, &1_000u64);
    assert_eq!(res, Err(Ok(OracleError::PairNotConfigured)));
}

// ===========================================================================
// 11. Multi-source quorum mode
// ===========================================================================

#[test]
fn test_multi_source_quorum_success() {
    let env = create_env();
    let (oracle_client, payroll_client, oracle_owner, source1, base, quote) = full_setup(&env);

    let source2 = Address::generate(&env);
    let source3 = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source2);
    oracle_client.add_source(&oracle_owner, &source3);

    // Reconfigure for quorum = 2.
    oracle_client.configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &500_000i128,
        &5_000_000i128,
        &600u64,
        &2u32,
        &50u32,
        &60u64,
        &0u64,
        &NO_HALT,
    );

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let rate = 2_000_000i128;

    // Source 1 submits.
    oracle_client.push_price(&source1, &base, &quote, &rate, &1_000u64);

    // State should NOT be updated yet (quorum = 2).
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PairNotConfigured))
    );

    // Source 2 submits the SAME rate and timestamp.
    oracle_client.push_price(&source2, &base, &quote, &rate, &1_000u64);

    // Now quorum is met!
    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, rate);
    assert_eq!(state.last_source, source2); // The one that completed the quorum

    // Payroll should be updated.
    let converted = payroll_client.convert_currency(&base, &quote, &10i128);
    assert_eq!(converted, 20);
}

#[test]
fn test_multi_source_quorum_different_rates_do_not_count() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source1, base, quote) = full_setup(&env);

    let source2 = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source2);

    oracle_client.configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &500_000i128,
        &5_000_000i128,
        &600u64,
        &2u32,
        &50u32,
        &60u64,
        &0u64,
        &NO_HALT,
    );

    env.ledger().with_mut(|li| li.timestamp = 1_000);

    // Source 1 submits rate A.
    oracle_client.push_price(&source1, &base, &quote, &2_000_000i128, &1_000u64);

    // Source 2 submits rate B (different).
    oracle_client.push_price(&source2, &base, &quote, &2_100_000i128, &1_000u64);

    // Neither reached quorum of 2.
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PairNotConfigured))
    );
}

#[test]
fn test_multi_source_quorum_tolerance_boundary_accepts() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source1, base, quote) = full_setup(&env);
    let source2 = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source2);

    configure_pair_with_settings(
        &oracle_client,
        &oracle_owner,
        &base,
        &quote,
        500_000i128,
        5_000_000i128,
        600u64,
        2u32,
        50u32,
        60u64,
    );

    env.ledger().with_mut(|li| li.timestamp = 1_005);
    oracle_client.push_price(&source1, &base, &quote, &2_000_000i128, &1_000u64);
    oracle_client.push_price(&source2, &base, &quote, &2_010_000i128, &1_005u64);

    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 2_010_000);
    assert_eq!(state.last_updated_ts, 1_005u64);
    assert_eq!(state.last_source, source2);
}

#[test]
fn test_multi_source_quorum_duplicate_vote_rejected() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source1, base, quote) = full_setup(&env);
    let source2 = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source2);

    configure_pair_with_settings(
        &oracle_client,
        &oracle_owner,
        &base,
        &quote,
        500_000i128,
        5_000_000i128,
        600u64,
        2u32,
        0u32,
        60u64,
    );

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source1, &base, &quote, &2_000_000i128, &1_000u64);

    let res = oracle_client.try_push_price(&source1, &base, &quote, &2_000_000i128, &1_000u64);
    assert_eq!(res, Err(Ok(OracleError::DuplicateVote)));
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PairNotConfigured))
    );

    oracle_client.push_price(&source2, &base, &quote, &2_000_000i128, &1_000u64);
    assert_eq!(oracle_client.get_pair_state(&base, &quote).rate, 2_000_000);
}

#[test]
fn test_multi_source_quorum_dissenting_source_does_not_block_matching_cluster() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source1, base, quote) = full_setup(&env);
    let source2 = Address::generate(&env);
    let source3 = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source2);
    oracle_client.add_source(&oracle_owner, &source3);

    configure_pair_with_settings(
        &oracle_client,
        &oracle_owner,
        &base,
        &quote,
        500_000i128,
        5_000_000i128,
        600u64,
        2u32,
        50u32,
        60u64,
    );

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source1, &base, &quote, &2_000_000i128, &1_000u64);
    oracle_client.push_price(&source2, &base, &quote, &2_100_000i128, &1_000u64);
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PairNotConfigured))
    );

    oracle_client.push_price(&source3, &base, &quote, &2_000_000i128, &1_000u64);
    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 2_000_000);
    assert_eq!(state.last_source, source3);
}

#[test]
fn test_multi_source_quorum_window_rollover_resets_pending_votes() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source1, base, quote) = full_setup(&env);
    let source2 = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source2);

    configure_pair_with_settings(
        &oracle_client,
        &oracle_owner,
        &base,
        &quote,
        500_000i128,
        5_000_000i128,
        600u64,
        2u32,
        0u32,
        60u64,
    );

    env.ledger().with_mut(|li| li.timestamp = 1_060);
    oracle_client.push_price(&source1, &base, &quote, &2_000_000i128, &1_000u64);
    oracle_client.push_price(&source2, &base, &quote, &2_000_000i128, &1_060u64);
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PairNotConfigured))
    );

    oracle_client.push_price(&source1, &base, &quote, &2_000_000i128, &1_060u64);
    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 2_000_000);
    assert_eq!(state.last_updated_ts, 1_060u64);
}

#[test]
fn test_multi_source_quorum_older_bucket_vote_is_ignored_after_rollover() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source1, base, quote) = full_setup(&env);
    let source2 = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source2);

    configure_pair_with_settings(
        &oracle_client,
        &oracle_owner,
        &base,
        &quote,
        500_000i128,
        5_000_000i128,
        600u64,
        2u32,
        0u32,
        60u64,
    );

    env.ledger().with_mut(|li| li.timestamp = 1_060);
    oracle_client.push_price(&source1, &base, &quote, &2_000_000i128, &1_060u64);

    let res = oracle_client.try_push_price(&source2, &base, &quote, &2_000_000i128, &1_000u64);
    assert!(res.is_ok());
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PairNotConfigured))
    );
}

#[test]
fn test_multi_source_quorum_uses_max_supporting_timestamp() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source1, base, quote) = full_setup(&env);
    let source2 = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source2);

    configure_pair_with_settings(
        &oracle_client,
        &oracle_owner,
        &base,
        &quote,
        500_000i128,
        5_000_000i128,
        600u64,
        2u32,
        100u32,
        60u64,
    );

    env.ledger().with_mut(|li| li.timestamp = 1_005);
    oracle_client.push_price(&source1, &base, &quote, &2_000_000i128, &1_005u64);
    oracle_client.push_price(&source2, &base, &quote, &2_000_500i128, &1_000u64);

    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 2_000_500i128);
    assert_eq!(state.last_updated_ts, 1_005u64);
}

#[test]
fn test_removed_source_pending_vote_no_longer_counts_toward_quorum() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source1, base, quote) = full_setup(&env);
    let source2 = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source2);

    configure_pair_with_settings(
        &oracle_client,
        &oracle_owner,
        &base,
        &quote,
        500_000i128,
        5_000_000i128,
        600u64,
        2u32,
        0u32,
        60u64,
    );

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source1, &base, &quote, &2_000_000i128, &1_000u64);
    oracle_client.remove_source(&oracle_owner, &source1);

    oracle_client.push_price(&source2, &base, &quote, &2_000_000i128, &1_000u64);
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PairNotConfigured))
    );
}

#[test]
fn test_push_price_returns_fx_update_failed_when_payroll_rejects_update() {
    let env = create_env();
    let (payroll_id, _, _) = setup_payroll(&env);
    let (_, oracle_client, oracle_owner) = setup_oracle(&env, &payroll_id);
    let source = Address::generate(&env);
    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    oracle_client.add_source(&oracle_owner, &source);
    configure_pair_with_settings(
        &oracle_client,
        &oracle_owner,
        &base,
        &quote,
        500_000i128,
        5_000_000i128,
        600u64,
        1u32,
        DEFAULT_TOLERANCE_BPS,
        DEFAULT_QUORUM_WINDOW_SECONDS,
    );

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let res = oracle_client.try_push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);
    assert_eq!(res, Err(Ok(OracleError::FxUpdateFailed)));
}

#[test]
fn test_quorum_rejection_on_zero_quorum() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, _, base, quote) = full_setup(&env);

    let res = oracle_client.try_configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &500_000i128,
        &5_000_000i128,
        &600u64,
        &0u32, // Invalid
        &DEFAULT_TOLERANCE_BPS,
        &DEFAULT_QUORUM_WINDOW_SECONDS,
        &0u64,
        &NO_HALT,
    );
    assert_eq!(res, Err(Ok(OracleError::InvalidPairConfig)));
}

/// A publisher submission that diverges from the current quorum-cluster
/// beyond the configured tolerance band is excluded from aggregation.
/// The outlier does not skew the accepted price or the supporting-vote
/// cluster.
#[test]
fn test_multi_source_quorum_outlier_beyond_tolerance_excluded() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source1, base, quote) = full_setup(&env);

    let source2 = Address::generate(&env);
    let source3 = Address::generate(&env);
    let source4 = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source2);
    oracle_client.add_source(&oracle_owner, &source3);
    oracle_client.add_source(&oracle_owner, &source4);

    // quorum=3, tolerance=25bps (0.25 %).
    configure_pair_with_settings(
        &oracle_client,
        &oracle_owner,
        &base,
        &quote,
        500_000i128,
        5_000_000i128,
        600u64,
        3u32,
        25u32,
        60u64,
    );

    env.ledger().with_mut(|li| li.timestamp = 1_000);

    // source1 — cluster anchor.
    oracle_client.push_price(&source1, &base, &quote, &1_000_000i128, &1_000u64);

    // source2 — OUTLIER, far outside 25 bps tolerance (~100 % diff).
    oracle_client.push_price(&source2, &base, &quote, &1_999_999i128, &1_000u64);

    // source3 — within 25 bps of source1 (~0.05 % = 5 bps diff).
    oracle_client.push_price(&source3, &base, &quote, &1_000_500i128, &1_000u64);

    // Quorum not yet reached — only 2 matching votes (source1 + source3).
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PairNotConfigured))
    );

    // source4 — within 25 bps of the cluster (~0.1 % = 10 bps from source1).
    // Completes quorum: {source1, source3, source4} all within tolerance.
    oracle_client.push_price(&source4, &base, &quote, &1_001_000i128, &1_000u64);

    let state = oracle_client.get_pair_state(&base, &quote);
    // Accepted rate is source4's (the completing vote), NOT the outlier.
    assert_eq!(state.rate, 1_001_000);
    assert_ne!(state.rate, 1_999_999);
    assert_eq!(state.last_source, source4);
}

/// All publisher submissions within the configured tolerance band are
/// accepted — each vote contributes to quorum and the price is correctly
/// aggregated at the completing vote's rate.
#[test]
fn test_multi_source_quorum_within_tolerance_accepted() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source1, base, quote) = full_setup(&env);

    let source2 = Address::generate(&env);
    let source3 = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source2);
    oracle_client.add_source(&oracle_owner, &source3);

    // quorum=3, tolerance=30bps (0.3 %) — all three rates fit comfortably.
    configure_pair_with_settings(
        &oracle_client,
        &oracle_owner,
        &base,
        &quote,
        500_000i128,
        5_000_000i128,
        600u64,
        3u32,
        30u32,
        60u64,
    );

    env.ledger().with_mut(|li| li.timestamp = 2_000);

    oracle_client.push_price(&source1, &base, &quote, &2_000_000i128, &2_000u64);
    oracle_client.push_price(&source2, &base, &quote, &2_005_000i128, &2_000u64);

    // Only 2 supporting votes so far — quorum=3 not yet met.
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PairNotConfigured))
    );

    // source3 completes the cluster — all three rates within 30 bps.
    oracle_client.push_price(&source3, &base, &quote, &2_006_000i128, &2_000u64);

    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 2_006_000);
    assert_eq!(state.last_source, source3);
}

// ===========================================================================

// ===========================================================================
// 12. Per-source submission rate limiting
// ===========================================================================

/// A source configured with a 30-second interval cannot resubmit within that window.
#[test]
fn test_rate_limit_rejects_rapid_resubmission() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source, base, quote) = full_setup(&env);

    oracle_client.configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &500_000i128,
        &5_000_000i128,
        &600u64,
        &1u32,
        &0u32,
        &60u64,
        &30u64, // 30-second min interval
        &NO_HALT,
    );

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);

    // Only 10 seconds later — rejected.
    env.ledger().with_mut(|li| li.timestamp = 1_010);
    let res = oracle_client.try_push_price(&source, &base, &quote, &2_100_000i128, &1_010u64);
    assert_eq!(res, Err(Ok(OracleError::SubmissionRateLimited)));

    // State unchanged.
    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 2_000_000);
}

/// After the interval expires the source can submit again.
#[test]
fn test_rate_limit_allows_submission_after_interval() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source, base, quote) = full_setup(&env);

    oracle_client.configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &500_000i128,
        &5_000_000i128,
        &600u64,
        &1u32,
        &0u32,
        &60u64,
        &30u64,
        &NO_HALT,
    );

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);

    // Exactly 30 seconds later — at the boundary — allowed.
    env.ledger().with_mut(|li| li.timestamp = 1_030);
    let res = oracle_client.try_push_price(&source, &base, &quote, &2_100_000i128, &1_030u64);
    assert!(res.is_ok());

    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 2_100_000);
}

/// Rate limit is per-source: a rate-limited source does not block distinct sources.
#[test]
fn test_rate_limit_does_not_affect_other_sources() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source1, base, quote) = full_setup(&env);
    let source2 = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source2);

    oracle_client.configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &500_000i128,
        &5_000_000i128,
        &600u64,
        &1u32,
        &0u32,
        &60u64,
        &30u64,
        &NO_HALT,
    );

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source1, &base, &quote, &2_000_000i128, &1_000u64);

    // source1 is rate-limited within the interval.
    env.ledger().with_mut(|li| li.timestamp = 1_010);
    let res = oracle_client.try_push_price(&source1, &base, &quote, &2_100_000i128, &1_010u64);
    assert_eq!(res, Err(Ok(OracleError::SubmissionRateLimited)));

    // source2 is a distinct key — submits freely.
    let res = oracle_client.try_push_price(&source2, &base, &quote, &2_100_000i128, &1_010u64);
    assert!(res.is_ok());

    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 2_100_000);
    assert_eq!(state.last_source, source2);
}

/// A single source cannot spam near-duplicate prices to dominate quorum clustering.
/// The rate limit ensures one source contributes at most one effective vote per bucket.
#[test]
fn test_rate_limit_single_source_counts_once_in_quorum() {
    let env = create_env();
    let (oracle_client, _, oracle_owner, source1, base, quote) = full_setup(&env);
    let source2 = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source2);

    // quorum=2, 30-second rate limit per source.
    oracle_client.configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &500_000i128,
        &5_000_000i128,
        &600u64,
        &2u32,
        &100u32,
        &60u64,
        &30u64,
        &NO_HALT,
    );

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source1, &base, &quote, &2_000_000i128, &1_000u64);

    // Rapid follow-up from source1 — blocked by rate limit before reaching the bucket.
    env.ledger().with_mut(|li| li.timestamp = 1_005);
    let res = oracle_client.try_push_price(&source1, &base, &quote, &2_005_000i128, &1_005u64);
    assert_eq!(res, Err(Ok(OracleError::SubmissionRateLimited)));

    // Quorum not met — only source1's single vote is in the bucket.
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PairNotConfigured))
    );

    // source2 completes quorum.
    oracle_client.push_price(&source2, &base, &quote, &2_000_000i128, &1_000u64);
    assert!(oracle_client.try_get_pair_state(&base, &quote).is_ok());
}

/// min_submit_interval_secs = 0 disables the rate limit entirely.
#[test]
fn test_rate_limit_zero_interval_is_disabled() {
    let env = create_env();
    // full_setup configures the pair with interval = 0.
    let (oracle_client, _, _, source, base, quote) = full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);

    // Immediate resubmission with a newer timestamp — allowed.
    env.ledger().with_mut(|li| li.timestamp = 1_001);
    let res = oracle_client.try_push_price(&source, &base, &quote, &2_100_000i128, &1_001u64);
    assert!(res.is_ok());
}

// ===========================================================================
// 13. get_pair_state freshness checks
// ===========================================================================

/// Fresh price within max_age is returned successfully.
#[test]
fn test_get_pair_state_fresh_price_succeeds() {
    let env = create_env();
    let (oracle_client, _, _, source, base, quote) = full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);

    // 60 seconds later — price is 60s old, max_age = 600s -> fresh (full_setup configures
    // max_staleness=600)
    env.ledger().with_mut(|li| li.timestamp = 1_060);
    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 2_000_000);
    assert_eq!(state.last_updated_ts, 1_000);
}

/// Price age exactly equal to max_age is still accepted.
#[test]
fn test_get_pair_state_at_exact_max_age_succeeds() {
    let env = create_env();
    let (oracle_client, _, _, source, base, quote) = full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);

    env.ledger().with_mut(|li| li.timestamp = 1_600); // age = 600
    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 2_000_000);
}

/// Price age one second beyond max_age is rejected with PriceTooOld.
#[test]
fn test_get_pair_state_one_second_past_max_age_rejected() {
    let env = create_env();
    let (oracle_client, _, _, source, base, quote) = full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);

    // age = 601 > max_age = 600 -> stale
    env.ledger().with_mut(|li| li.timestamp = 1_601);
    let res = oracle_client.try_get_pair_state(&base, &quote);
    assert_eq!(res, Err(Ok(OracleError::PriceTooOld)));
}

/// After a stale price is refreshed, it succeeds again.
#[test]
fn test_get_pair_state_recovers_after_fresh_update() {
    let env = create_env();
    let (oracle_client, _, _, source, base, quote) = full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);

    // Price goes stale.
    env.ledger().with_mut(|li| li.timestamp = 2_000); // age = 1000 > max_age = 600
    let res = oracle_client.try_get_pair_state(&base, &quote);
    assert_eq!(res, Err(Ok(OracleError::PriceTooOld)));

    // Source pushes a fresh price.
    oracle_client.push_price(&source, &base, &quote, &2_500_000i128, &2_000u64);

    // Checked read recovers.
    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 2_500_000);
    assert_eq!(state.last_updated_ts, 2_000);
}

// ===========================================================================
// 14. Consecutive stale-price automatic halt
// ===========================================================================

/// Helper: configure a pair with a specific halt threshold and return fresh
/// oracle + source + pair tokens.
fn setup_halt_pair(
    env: &Env,
    halt_threshold: u32,
) -> (
    PriceOracleContractClient<'static>,
    Address, // oracle_owner
    Address, // source
    Address, // base
    Address, // quote
) {
    let (payroll_id, payroll_owner, payroll_client) = setup_payroll(env);
    let (oracle_id, oracle_client, oracle_owner) = setup_oracle(env, &payroll_id);
    payroll_client.set_exchange_rate_admin(&payroll_owner, &oracle_id);

    let source = Address::generate(env);
    oracle_client.add_source(&oracle_owner, &source);

    let base = Address::generate(env);
    let quote = Address::generate(env);

    oracle_client.configure_pair(
        &oracle_owner,
        &base,
        &quote,
        &500_000i128,
        &5_000_000i128,
        &300u64, // max_staleness = 300 s — easy to exceed in tests
        &1u32,
        &0u32,
        &DEFAULT_QUORUM_WINDOW_SECONDS,
        &0u64,
        &halt_threshold,
    );

    (oracle_client, oracle_owner, source, base, quote)
}

/// After exactly `max_consecutive_stale_before_halt` consecutive stale reads,
/// `get_pair_state` must return `PairHalted` instead of `PriceTooOld`.
///
/// # Security note
/// `PairHalted` is a distinct error so off-chain consumers can differentiate
/// between a briefly stale price (transient) and a pair that has been
/// persistently stale (requires intervention).
#[test]
fn test_consecutive_stale_halt_triggers_after_threshold() {
    let env = create_env();
    // Threshold = 3: reads 1 and 2 return PriceTooOld; read 3 returns PairHalted.
    let (oracle_client, _, source, base, quote) = setup_halt_pair(&env, 3);

    // Push a fresh price, then advance time past max_staleness.
    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);

    // Advance time so the stored price is older than max_staleness (300 s).
    env.ledger().with_mut(|li| li.timestamp = 2_000); // age = 1000 > 300

    // Read 1 — counter becomes 1; still PriceTooOld.
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PriceTooOld)),
        "first stale read must return PriceTooOld (counter=1, threshold=3)"
    );

    // Read 2 — counter becomes 2; still PriceTooOld.
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PriceTooOld)),
        "second stale read must return PriceTooOld (counter=2, threshold=3)"
    );

    // Read 3 — counter reaches threshold (3 >= 3); PairHalted.
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PairHalted)),
        "third stale read must return PairHalted (counter=3 >= threshold=3)"
    );

    // Subsequent reads also return PairHalted until a fresh push clears the state.
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PairHalted)),
        "reads beyond threshold must keep returning PairHalted"
    );
}

/// Reads that stay below the consecutive-stale threshold must still return
/// `PriceTooOld`, NOT `PairHalted`. The halt only fires at the exact threshold.
#[test]
fn test_consecutive_stale_below_threshold_returns_price_too_old() {
    let env = create_env();
    // High threshold: 10. We will only perform 5 stale reads.
    let (oracle_client, _, source, base, quote) = setup_halt_pair(&env, 10);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);
    env.ledger().with_mut(|li| li.timestamp = 2_000); // stale

    for read_n in 1..=5u32 {
        assert_eq!(
            oracle_client.try_get_pair_state(&base, &quote),
            Err(Ok(OracleError::PriceTooOld)),
            "read {} of 5 (threshold=10) must return PriceTooOld, not PairHalted",
            read_n
        );
    }
}

/// A fresh, valid `push_price` clears the consecutive-stale counter so that
/// subsequent reads of a non-stale price succeed, and the halt counter starts
/// from zero again.
///
/// # Security note
/// Only a genuine price update (which passes all validation checks including
/// freshness, bounds, and source authorization) resets the halt state.
/// An attacker cannot fake-reset it by any other means.
#[test]
fn test_fresh_push_clears_halt_and_resumes_serving() {
    let env = create_env();
    // Threshold = 2: two stale reads trigger PairHalted.
    let (oracle_client, _, source, base, quote) = setup_halt_pair(&env, 2);

    // Push initial price and then let it go stale.
    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);
    env.ledger().with_mut(|li| li.timestamp = 2_000); // age = 1000 > max_staleness=300

    // Read 1 → PriceTooOld (counter=1).
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PriceTooOld))
    );
    // Read 2 → PairHalted (counter=2 >= threshold=2).
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PairHalted))
    );

    // Push a fresh price — this must clear the counter.
    // max_staleness=300, so source_timestamp=2_000 with ledger=2_000 is fresh.
    oracle_client.push_price(&source, &base, &quote, &3_000_000i128, &2_000u64);

    // Immediately after a fresh push, get_pair_state must succeed again.
    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 3_000_000, "fresh push rate must be served");
    assert_eq!(
        state.last_updated_ts, 2_000,
        "timestamp must reflect the new push"
    );

    // The halt counter is zero again; a subsequent non-stale read also succeeds.
    let state2 = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state2.rate, 3_000_000);
}

/// When `max_consecutive_stale_before_halt = 0`, the halt mechanism is disabled
/// entirely. Any number of stale reads must keep returning `PriceTooOld`, never
/// `PairHalted`.
#[test]
fn test_halt_disabled_when_threshold_zero() {
    let env = create_env();
    // Threshold = 0 → halt disabled.
    let (oracle_client, _, source, base, quote) = setup_halt_pair(&env, 0);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);
    env.ledger().with_mut(|li| li.timestamp = 2_000); // stale

    // Many stale reads — all must return PriceTooOld, never PairHalted.
    for _ in 0..20 {
        assert_eq!(
            oracle_client.try_get_pair_state(&base, &quote),
            Err(Ok(OracleError::PriceTooOld)),
            "with threshold=0 every stale read must return PriceTooOld"
        );
    }
}

/// A fresh push mid-way through accumulating stale detections resets the counter
/// to zero, requiring the full threshold to be reached again before halting.
#[test]
fn test_fresh_push_before_halt_resets_counter() {
    let env = create_env();
    // Threshold = 3.
    let (oracle_client, _, source, base, quote) = setup_halt_pair(&env, 3);

    // Initial fresh push.
    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);

    // Advance time: price is stale.
    env.ledger().with_mut(|li| li.timestamp = 2_000);

    // Two stale reads (counter = 2; threshold not yet reached).
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PriceTooOld)),
        "read 1 (counter=1) must be PriceTooOld"
    );
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PriceTooOld)),
        "read 2 (counter=2) must be PriceTooOld"
    );

    // Push a fresh price BEFORE the third stale read resets the counter.
    oracle_client.push_price(&source, &base, &quote, &2_500_000i128, &2_000u64);

    // Fresh read immediately succeeds (counter is now 0).
    let state = oracle_client.get_pair_state(&base, &quote);
    assert_eq!(state.rate, 2_500_000, "fresh push must be served");

    // Let the new price go stale again.
    env.ledger().with_mut(|li| li.timestamp = 3_000); // age from ts=2000 is 1000 > 300

    // The full threshold (3) must be accumulated again from scratch.
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PriceTooOld)),
        "post-reset read 1 (counter=1) must be PriceTooOld"
    );
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PriceTooOld)),
        "post-reset read 2 (counter=2) must be PriceTooOld"
    );
    // Third stale read — threshold reached again.
    assert_eq!(
        oracle_client.try_get_pair_state(&base, &quote),
        Err(Ok(OracleError::PairHalted)),
        "post-reset read 3 (counter=3 >= threshold=3) must be PairHalted"
    );
}
