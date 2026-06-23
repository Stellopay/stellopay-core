#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

use rate_limiter::{RateLimiter, RateLimiterClient};

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
fn test_initialize_and_basic_quota() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &5u32, &1u32, &true);
    assert_eq!(client.get_admin(), Some(admin.clone()));
    
    let config = client.get_limit_for(&user);
    assert_eq!(config.burst, 5);
    assert_eq!(config.refill_rate, 1);

    // Consume 1 token
    let remaining = client.check_and_consume(&user);
    assert_eq!(remaining, 4);
}

#[test]
fn test_token_bucket_refill_logic() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    // Start at T=100
    env.ledger().with_mut(|li| li.timestamp = 100);
    
    // Burst 2, Refill 1 per second
    client.initialize(&admin, &2u32, &1u32, &false);

    // Use burst
    assert_eq!(client.check_and_consume(&user), 1);
    assert_eq!(client.check_and_consume(&user), 0);
    assert!(client.try_check_and_consume(&user).is_err());

    // Advance 1 second -> 1 token refilled
    env.ledger().with_mut(|li| li.timestamp = 101);
    assert_eq!(client.check_and_consume(&user), 0);
    assert!(client.try_check_and_consume(&user).is_err());

    // Advance 5 seconds -> tokens = 0 + 5 = 5, but capped at burst = 2
    env.ledger().with_mut(|li| li.timestamp = 106);
    assert_eq!(client.check_and_consume(&user), 1);
    assert_eq!(client.check_and_consume(&user), 0);
    assert!(client.try_check_and_consume(&user).is_err());
}

#[test]
fn test_global_limit_enforcement() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);

    client.initialize(&admin, &10u32, &10u32, &false);

    // Enable global limit: only 1 request allowed globally, no refill
    client.set_global_limit(&true, &1u32, &0u32);

    // User 1 consumes the global token
    client.check_and_consume(&user1);
    
    // User 2 fails because global bucket is empty
    let result = client.try_check_and_consume(&user2);
    assert!(result.is_err());
}

#[test]
fn test_admin_bypass_security() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);

    // Admin bypass enabled, strict limit for others
    client.initialize(&admin, &0u32, &0u32, &true);

    // Admin is exempt
    assert_eq!(client.check_and_consume(&admin), u32::MAX);
    assert_eq!(client.check_and_consume(&admin), u32::MAX);
    
    // User is blocked
    let user = Address::generate(&env);
    assert!(client.try_check_and_consume(&user).is_err());
}

#[test]
fn test_per_address_overrides() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &1u32, &0u32, &false);
    
    // User override
    client.set_limit_for(&user, &10u32, &5u32);
    let config = client.get_limit_for(&user);
    assert_eq!(config.burst, 10);
    assert_eq!(config.refill_rate, 5);

    assert_eq!(client.check_and_consume(&user), 9);
    
    // Clear override
    client.clear_limit_for(&user);
    let config_reset = client.get_limit_for(&user);
    assert_eq!(config_reset.burst, 1);
}

#[test]
fn test_admin_usage_reset() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &1u32, &0u32, &false);
    
    client.check_and_consume(&user);
    assert!(client.try_check_and_consume(&user).is_err());
    
    // Admin resets user usage
    client.reset_usage(&user);
    assert_eq!(client.check_and_consume(&user), 0);
}

#[test]
fn test_admin_transfer() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin1 = Address::generate(&env);
    let admin2 = Address::generate(&env);

    client.initialize(&admin1, &1u32, &1u32, &false);
    assert_eq!(client.get_admin(), Some(admin1.clone()));
    
    client.transfer_admin(&admin2);
    assert_eq!(client.get_admin(), Some(admin2.clone()));
}

#[test]
fn test_get_usage_returns_none_for_unused_address() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &10u32, &1u32, &false);

    // No usage record yet → None
    assert_eq!(client.get_usage(&user), None);
}

#[test]
fn test_get_usage_after_consumption() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 100);
    client.initialize(&admin, &5u32, &1u32, &false);

    // Consume 2 tokens
    client.check_and_consume(&user);
    client.check_and_consume(&user);

    // get_usage should show 3 tokens remaining at time 100
    let usage = client.get_usage(&user).unwrap();
    assert_eq!(usage.tokens, 3);
    assert_eq!(usage.last_update, 100);
}

#[test]
fn test_get_usage_shows_refill_without_mutation() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 100);
    client.initialize(&admin, &5u32, &1u32, &false);

    // Consume all 5 tokens
    for _ in 0..5 {
        client.check_and_consume(&user);
    }
    assert!(client.try_check_and_consume(&user).is_err());

    // Advance 3 seconds → refill gives 3 tokens
    env.ledger().with_mut(|li| li.timestamp = 103);
    let usage = client.get_usage(&user).unwrap();
    assert_eq!(usage.tokens, 3);
    assert_eq!(usage.last_update, 103);

    // Call again — result is identical (no mutation)
    let usage2 = client.get_usage(&user).unwrap();
    assert_eq!(usage2.tokens, 3);
    assert_eq!(usage2.last_update, 103);

    // check_and_consume should also see 3 tokens (not double-refilled)
    assert_eq!(client.check_and_consume(&user), 2);
}

#[test]
fn test_get_usage_no_refill_with_zero_rate() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 100);
    client.initialize(&admin, &3u32, &0u32, &false);

    client.check_and_consume(&user);

    // Advance time — no refill because rate is 0, but last_update advances
    env.ledger().with_mut(|li| li.timestamp = 200);
    let usage = client.get_usage(&user).unwrap();
    assert_eq!(usage.tokens, 2);
    assert_eq!(usage.last_update, 200);
}

#[test]
fn test_get_usage_with_per_address_override() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 100);
    client.initialize(&admin, &1u32, &0u32, &false);

    // Override: burst 10, refill 2/sec
    client.set_limit_for(&user, &10u32, &2u32);

    // Consume 5 tokens → 5 remain
    for _ in 0..5 {
        client.check_and_consume(&user);
    }

    // Advance 2 seconds → refill 4 tokens → 5 + 4 = 9
    env.ledger().with_mut(|li| li.timestamp = 102);
    let usage = client.get_usage(&user).unwrap();
    assert_eq!(usage.tokens, 9);
    assert_eq!(usage.last_update, 102);
}

#[test]
fn test_get_usage_caps_at_burst() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 100);
    client.initialize(&admin, &5u32, &1u32, &false);

    // Consume 1 token → 4 remain
    client.check_and_consume(&user);

    // Advance 10 seconds → would refill 10 tokens, but capped at burst 5
    env.ledger().with_mut(|li| li.timestamp = 110);
    let usage = client.get_usage(&user).unwrap();
    assert_eq!(usage.tokens, 5);
    assert_eq!(usage.last_update, 110);
}

