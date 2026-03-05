#![cfg(test)]

use price_oracle::{
    OracleError, PairConfig, PairState, PriceOracleContract, PriceOracleContractClient,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_payroll(env: &Env) -> (Address, Address, PayrollContractClient<'static>) {
    #[allow(deprecated)]
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
    #[allow(deprecated)]
    let oracle_id = env.register_contract(None, PriceOracleContract);
    let client = PriceOracleContractClient::new(env, &oracle_id);
    let owner = Address::generate(env);

    client.initialize(&owner, payroll_id);

    (oracle_id, client, owner)
}

#[test]
fn initialize_and_configure_pair() {
    let env = create_env();

    let (payroll_id, payroll_owner, payroll_client) = setup_payroll(&env);
    let (oracle_id, oracle_client, oracle_owner) = setup_oracle(&env, &payroll_id);

    // Designate the oracle contract as FX admin in the payroll contract.
    payroll_client
        .set_exchange_rate_admin(&payroll_owner, &oracle_id)
        .unwrap();

    // Configure an oracle source and a pair.
    let source = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source).unwrap();
    assert!(oracle_client.is_source_address(&source));

    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    let min_rate: i128 = 500_000; // 0.5
    let max_rate: i128 = 5_000_000; // 5.0
    let max_staleness: u64 = 600;

    oracle_client
        .configure_pair(
            &oracle_owner,
            &base,
            &quote,
            &min_rate,
            &max_rate,
            &max_staleness,
        )
        .unwrap();

    let cfg: PairConfig = oracle_client.get_pair_config(&base, &quote).unwrap();
    assert!(cfg.enabled);
    assert_eq!(cfg.min_rate, min_rate);
    assert_eq!(cfg.max_rate, max_rate);

    // Push a fresh price well within bounds.
    env.ledger().with_mut(|li| {
        li.timestamp = 1_000;
    });

    let rate: i128 = 2_000_000; // 2.0
    let source_ts: u64 = 1_000;

    oracle_client
        .push_price(&source, &base, &quote, &rate, &source_ts)
        .unwrap();

    // Pair state updated.
    let state: PairState = oracle_client.get_pair_state(&base, &quote).unwrap();
    assert_eq!(state.rate, rate);
    assert_eq!(state.last_source, source);
    assert_eq!(state.last_updated_ts, source_ts);

    // Payroll contract should now reflect the FX rate.
    let amount: i128 = 10;
    let converted = payroll_client.convert_currency(&base, &quote, &amount);
    assert_eq!(converted, Ok(20));
}

#[test]
fn unauthorized_and_invalid_updates_rejected() {
    let env = create_env();

    let (payroll_id, payroll_owner, payroll_client) = setup_payroll(&env);
    let (_oracle_id, oracle_client, oracle_owner) = setup_oracle(&env, &payroll_id);

    payroll_client
        .set_exchange_rate_admin(&payroll_owner, &_oracle_id)
        .unwrap();

    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    let min_rate: i128 = 1_000_000;
    let max_rate: i128 = 3_000_000;
    let max_staleness: u64 = 300;

    oracle_client
        .configure_pair(
            &oracle_owner,
            &base,
            &quote,
            &min_rate,
            &max_rate,
            &max_staleness,
        )
        .unwrap();

    // Unregistered source is rejected.
    let unknown_source = Address::generate(&env);
    let res = oracle_client.try_push_price(&unknown_source, &base, &quote, &2_000_000i128, &0u64);
    assert_eq!(res, Err(Ok(OracleError::InvalidSource)));

    // Register source.
    let source = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &source).unwrap();

    env.ledger().with_mut(|li| {
        li.timestamp = 1_000;
    });

    // Out-of-bounds rate.
    let res = oracle_client.try_push_price(&source, &base, &quote, &10_000_000i128, &1_000u64);
    assert_eq!(res, Err(Ok(OracleError::RateOutOfBounds)));

    // Stale rate.
    let res = oracle_client.try_push_price(&source, &base, &quote, &2_000_000i128, &100u64);
    assert_eq!(res, Err(Ok(OracleError::RateStale)));
}

#[test]
fn monotonic_updates_and_fallback_behavior() {
    let env = create_env();

    let (payroll_id, payroll_owner, payroll_client) = setup_payroll(&env);
    let (_oracle_id, oracle_client, oracle_owner) = setup_oracle(&env, &payroll_id);

    payroll_client
        .set_exchange_rate_admin(&payroll_owner, &_oracle_id)
        .unwrap();

    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    oracle_client
        .configure_pair(
            &oracle_owner,
            &base,
            &quote,
            &1_000_000i128,
            &4_000_000i128,
            &600u64,
        )
        .unwrap();

    let primary = Address::generate(&env);
    let backup = Address::generate(&env);
    oracle_client.add_source(&oracle_owner, &primary).unwrap();
    oracle_client.add_source(&oracle_owner, &backup).unwrap();

    // Primary reports first.
    env.ledger().with_mut(|li| {
        li.timestamp = 2_000;
    });

    oracle_client
        .push_price(&primary, &base, &quote, &2_000_000i128, &2_000u64)
        .unwrap();

    // Backup reports a newer, valid rate.
    oracle_client
        .push_price(&backup, &base, &quote, &3_000_000i128, &2_100u64)
        .unwrap();

    // Older updates are ignored.
    let _ = oracle_client.push_price(&primary, &base, &quote, &1_500_000i128, &1_900u64);

    let state: PairState = oracle_client.get_pair_state(&base, &quote).unwrap();
    assert_eq!(state.rate, 3_000_000i128);
    assert_eq!(state.last_source, backup);

    // Payroll sees the latest rate.
    let amount: i128 = 10;
    let converted = payroll_client.convert_currency(&base, &quote, &amount);
    assert_eq!(converted, Ok(30));
}
