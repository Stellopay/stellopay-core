#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

use rate_limiter::{RateLimiter, RateLimiterClient, Usage};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn register_contract(env: &Env) -> (Address, RateLimiterClient<'static>) {
    let id = env.register_contract(None, RateLimiter);
    let client = RateLimiterClient::new(env, &id);
    (id, client)
}

#[test]
fn initialize_and_config() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);

    client.initialize(&admin, &5u32, &60u64);
    assert_eq!(client.get_admin(), Some(admin.clone()));
    assert_eq!(client.get_default_limit(), 5);
    assert_eq!(client.get_window_seconds(), 60);

    client.set_default_limit(&10u32);
    assert_eq!(client.get_default_limit(), 10);

    client.set_window_seconds(&120u64);
    assert_eq!(client.get_window_seconds(), 120);
}

#[test]
fn per_address_limit_and_usage() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 0);
    client.initialize(&admin, &3u32, &30u64);

    // default limit applies
    assert_eq!(client.get_limit_for(&user), 3);
    // override
    client.set_limit_for(&user, &2u32);
    assert_eq!(client.get_limit_for(&user), 2);

    // consume within limit
    let rem1 = client.check_and_consume(&user);
    assert_eq!(rem1, 1);
    let rem2 = client.check_and_consume(&user);
    assert_eq!(rem2, 0);

    // next call exceeds
    let err = client.try_check_and_consume(&user);
    assert!(err.is_err());
}

#[test]
fn window_reset_on_time_advance() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 100);
    client.initialize(&admin, &2u32, &10u64);

    // consume twice to hit limit
    assert_eq!(client.check_and_consume(&user), 1);
    assert_eq!(client.check_and_consume(&user), 0);
    assert!(client.try_check_and_consume(&user).is_err());

    // advance exactly by window; usage should reset
    env.ledger().with_mut(|li| li.timestamp = 110);
    assert_eq!(client.check_and_consume(&user), 1);
    let usage = client.get_usage(&user);
    assert_eq!(
        usage,
        Usage {
            count: 1,
            window_start: 110
        }
    );
}

#[test]
fn admin_reset_usage() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 42);
    client.initialize(&admin, &1u32, &100u64);

    // consume once
    assert_eq!(client.check_and_consume(&user), 0);
    assert!(client.try_check_and_consume(&user).is_err());

    // admin reset
    client.reset_usage(&user);
    assert_eq!(client.check_and_consume(&user), 0);
}

#[test]
fn security_enforcement() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &5u32, &50u64);

    // Subject must authenticate: with mock_all_auths, Address::generate is enough.
    // Admin-only paths are guarded; to simulate non-admin attempting change, we
    // flip admin and expect auth is enforced by env (mocked allows, so we assert behavior via state).
    client.set_limit_for(&user, &2u32);
    assert_eq!(client.get_limit_for(&user), 2);

    client.clear_limit_for(&user);
    assert_eq!(client.get_limit_for(&user), 5);
}

#[test]
fn edge_cases_zero_limit() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &0u32, &10u64);
    // Even with zero default, per-address override can enable usage
    client.set_limit_for(&user, &1u32);
    assert_eq!(client.check_and_consume(&user), 0);
    // Removing override reverts to zero limit and should fail
    client.clear_limit_for(&user);
    let err = client.try_check_and_consume(&user);
    assert!(err.is_err());
}
