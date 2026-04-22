#![cfg(test)]
#![allow(deprecated)]

use price_oracle::{
    OracleError, PriceOracleContract, PriceOracleContractClient,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

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

/// Full setup: payroll + oracle + FX admin registered + source + pair configured.
fn full_setup(
    env: &Env,
) -> (
    PriceOracleContractClient<'static>,
    PayrollContractClient<'static>,
    Address,       // oracle owner
    Address,       // source
    Address,       // base
    Address,       // quote
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
        &500_000i128,   // min 0.5
        &5_000_000i128, // max 5.0
        &600u64,        // 10 min staleness
        &1u32,          // quorum
    );

    (oracle_client, payroll_client, oracle_owner, source, base, quote)
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
    );

    let cfg = oracle_client.get_pair_config(&base2, &quote2).unwrap();
    assert_eq!(cfg.min_rate, 1_000_000);
    assert_eq!(cfg.max_rate, 3_000_000);
    assert_eq!(cfg.max_staleness_seconds, 300);
    assert!(cfg.enabled);
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
        &3_000_000i128, // min > max
        &1_000_000i128,
        &300u64,
        &1u32,
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

// ===========================================================================
// 5. Push price – happy path
// ===========================================================================

#[test]
fn test_push_price_success_and_payroll_integration() {
    let env = create_env();
    let (oracle_client, payroll_client, _, source, base, quote) = full_setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);

    let state = oracle_client.get_pair_state(&base, &quote).unwrap();
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

    let state = oracle_client.get_pair_state(&base, &quote).unwrap();
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

    let state = oracle_client.get_pair_state(&base, &quote).unwrap();
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
    let res =
        oracle_client.try_push_price(&source, &unknown_base, &unknown_quote, &2_000_000i128, &1_000u64);
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

    let state = oracle_client.get_pair_state(&base, &quote).unwrap();
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

    let state = oracle_client.get_pair_state(&base, &quote).unwrap();
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

    let state = oracle_client.get_pair_state(&base, &quote).unwrap();
    assert_eq!(state.rate, 3_000_000);
    assert_eq!(state.last_source, backup);

    let converted = payroll_client.convert_currency(&base, &quote, &10i128);
    assert_eq!(converted, 30);
}

// ===========================================================================
// 8. Ownership transfer
// ===========================================================================

#[test]
fn test_transfer_ownership_success() {
    let env = create_env();
    let (payroll_id, _, _) = setup_payroll(&env);
    let (_, client, owner) = setup_oracle(&env, &payroll_id);
    let new_owner = Address::generate(&env);

    client.transfer_ownership(&owner, &new_owner);
    assert_eq!(client.get_owner().unwrap(), new_owner);
}

#[test]
fn test_new_owner_can_add_source() {
    let env = create_env();
    let (payroll_id, _, _) = setup_payroll(&env);
    let (_, client, owner) = setup_oracle(&env, &payroll_id);
    let new_owner = Address::generate(&env);

    client.transfer_ownership(&owner, &new_owner);

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

    client.transfer_ownership(&owner, &new_owner);

    let source = Address::generate(&env);
    let res = client.try_add_source(&owner, &source);
    assert_eq!(res, Err(Ok(OracleError::NotAuthorized)));
}

#[test]
fn test_non_owner_cannot_transfer_ownership() {
    let env = create_env();
    let (payroll_id, _, _) = setup_payroll(&env);
    let (_, client, _owner) = setup_oracle(&env, &payroll_id);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);

    let res = client.try_transfer_ownership(&attacker, &target);
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

    let res = client.try_configure_pair(&a, &b, &c, &1i128, &2i128, &1u64, &1u32);
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
fn test_transfer_ownership_before_init_fails() {
    let env = create_env();
    let oracle_id = env.register_contract(None, PriceOracleContract);
    let client = PriceOracleContractClient::new(&env, &oracle_id);
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    let res = client.try_transfer_ownership(&a, &b);
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
    let (oracle_client, _, oracle_owner, source, base, quote) = full_setup(&env);

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
    );
    assert_eq!(res, Err(Ok(OracleError::NotAuthorized)));

    // Source cannot transfer ownership.
    let res = oracle_client.try_transfer_ownership(&source, &source);
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
    );

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    oracle_client.push_price(&source, &base, &quote, &2_000_000i128, &1_000u64);

    // Second pair has no state yet.
    assert!(oracle_client.get_pair_state(&base2, &quote2).is_none());

    // Push to second pair.
    oracle_client.push_price(&source, &base2, &quote2, &4_000_000i128, &1_000u64);

    // Each pair has its own state.
    let s1 = oracle_client.get_pair_state(&base, &quote).unwrap();
    let s2 = oracle_client.get_pair_state(&base2, &quote2).unwrap();
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
    );

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let rate = 2_000_000i128;

    // Source 1 submits.
    oracle_client.push_price(&source1, &base, &quote, &rate, &1_000u64);

    // State should NOT be updated yet (quorum = 2).
    assert!(oracle_client.get_pair_state(&base, &quote).is_none());

    // Source 2 submits the SAME rate and timestamp.
    oracle_client.push_price(&source2, &base, &quote, &rate, &1_000u64);

    // Now quorum is met!
    let state = oracle_client.get_pair_state(&base, &quote).unwrap();
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
    );

    env.ledger().with_mut(|li| li.timestamp = 1_000);

    // Source 1 submits rate A.
    oracle_client.push_price(&source1, &base, &quote, &2_000_000i128, &1_000u64);

    // Source 2 submits rate B (different).
    oracle_client.push_price(&source2, &base, &quote, &2_100_000i128, &1_000u64);

    // Neither reached quorum of 2.
    assert!(oracle_client.get_pair_state(&base, &quote).is_none());
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
    );
    assert_eq!(res, Err(Ok(OracleError::InvalidPairConfig)));
}
