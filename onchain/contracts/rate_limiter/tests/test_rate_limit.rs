#![cfg(test)]

use rate_limiter::{ConsumptionOutcome, RateLimitError, RateLimiter, RateLimiterClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn register_contract(env: &Env) -> (Address, RateLimiterClient<'static>) {
    let id = env.register(RateLimiter, ());
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
    let outcome = client.check_and_consume(&user);
    assert_eq!(outcome.remaining, 4);
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
    assert_eq!(client.check_and_consume(&user).remaining, 1);
    assert_eq!(client.check_and_consume(&user).remaining, 0);
    assert!(client.try_check_and_consume(&user).is_err());

    // Advance 1 second -> 1 token refilled
    env.ledger().with_mut(|li| li.timestamp = 101);
    assert_eq!(client.check_and_consume(&user).remaining, 0);
    assert!(client.try_check_and_consume(&user).is_err());

    // Advance 5 seconds -> tokens = 0 + 5 = 5, but capped at burst = 2
    env.ledger().with_mut(|li| li.timestamp = 106);
    assert_eq!(client.check_and_consume(&user).remaining, 1);
    assert_eq!(client.check_and_consume(&user).remaining, 0);
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
    assert_eq!(client.check_and_consume(&admin).remaining, u32::MAX);
    assert_eq!(client.check_and_consume(&admin).remaining, u32::MAX);

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

    assert_eq!(client.check_and_consume(&user).remaining, 9);

    // Clear override
    client.clear_limit_for(&user);
    let config_reset = client.get_limit_for(&user);
    assert_eq!(config_reset.burst, 1);
}

#[test]
fn test_per_caller_override_stricter_than_default_is_enforced() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    // Default bucket is generous: 5 immediate calls, no refill.
    client.initialize(&admin, &5u32, &0u32, &false);

    // Override is stricter and must fully replace the default for this caller.
    client.set_limit_for(&user, &2u32, &0u32);

    assert_eq!(client.check_and_consume(&user).remaining, 1);
    assert_eq!(client.check_and_consume(&user).remaining, 0);
    assert!(
        client.try_check_and_consume(&user).is_err(),
        "per-caller override must stop the third call even though the default burst is 5"
    );
}

#[test]
fn test_per_caller_override_looser_than_default_is_honored() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    // Default bucket is strict: only 2 immediate calls, no refill.
    client.initialize(&admin, &2u32, &0u32, &false);

    // Override is looser and must be honored instead of the default.
    client.set_limit_for(&user, &4u32, &0u32);

    assert_eq!(client.check_and_consume(&user).remaining, 3);
    assert_eq!(client.check_and_consume(&user).remaining, 2);
    assert_eq!(client.check_and_consume(&user).remaining, 1);
    assert_eq!(client.check_and_consume(&user).remaining, 0);
    assert!(
        client.try_check_and_consume(&user).is_err(),
        "per-caller override must allow four calls before exhaustion"
    );
}

#[test]
fn test_caller_without_override_uses_default_limit() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let override_user = Address::generate(&env);

    client.initialize(&admin, &3u32, &0u32, &false);
    client.set_limit_for(&override_user, &10u32, &0u32);

    // A different address with no override must still use the default values.
    assert_eq!(client.check_and_consume(&user).remaining, 2);
    assert_eq!(client.check_and_consume(&user).remaining, 1);
    assert_eq!(client.check_and_consume(&user).remaining, 0);
    assert!(
        client.try_check_and_consume(&user).is_err(),
        "addresses without overrides must continue to be governed by the default burst"
    );
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
    assert_eq!(client.check_and_consume(&user).remaining, 0);
}

#[test]
fn test_admin_transfer_requires_acceptance() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin1 = Address::generate(&env);
    let admin2 = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin1, &1u32, &1u32, &false);
    assert_eq!(client.get_admin(), Some(admin1.clone()));

    client.transfer_admin(&admin2);
    assert_eq!(client.get_admin(), Some(admin1.clone()));

    // The current admin retains access while the transfer is pending.
    assert!(client.try_set_limit_for(&user, &2u32, &1u32).is_ok());
    client.accept_admin(&admin2);
    assert_eq!(client.get_admin(), Some(admin2));
    assert!(client.try_set_limit_for(&user, &2u32, &1u32).is_ok());
}

#[test]
fn test_non_pending_admin_cannot_accept_transfer() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let pending = Address::generate(&env);
    let attacker = Address::generate(&env);

    client.initialize(&admin, &1u32, &1u32, &false);
    client.transfer_admin(&pending);

    assert!(client.try_accept_admin(&attacker).is_err());
    assert_eq!(client.get_admin(), Some(admin));
}

#[test]
fn test_admin_can_cancel_pending_transfer() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let pending = Address::generate(&env);

    client.initialize(&admin, &1u32, &1u32, &false);
    client.transfer_admin(&pending);
    client.cancel_admin_transfer();

    assert_eq!(client.get_admin(), Some(admin));
    assert!(client.try_accept_admin(&pending).is_err());
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
    assert_eq!(client.check_and_consume(&user).remaining, 2);
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

#[test]
fn test_many_same_second_calls_do_not_receive_fractional_refill_credit() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    const START: u64 = 1_000;
    const BURST: u32 = 100;
    const REFILL_RATE: u32 = 7;
    const WHOLE_SECOND_WINDOWS: u64 = 25;
    const CALLS_PER_WINDOW: u32 = 10;

    env.ledger().with_mut(|li| li.timestamp = START);
    client.initialize(&admin, &BURST, &REFILL_RATE, &false);

    let mut allowed = 0u32;
    let mut rejected = 0u32;

    // Drain the starting bucket first. Afterwards, every group of calls below
    // represents many tiny attempts inside one whole-second refill window.
    for _ in 0..BURST {
        client.check_and_consume(&user);
        allowed += 1;
    }
    assert!(client.try_check_and_consume(&user).is_err());
    rejected += 1;

    for elapsed in 1..=WHOLE_SECOND_WINDOWS {
        env.ledger()
            .with_mut(|li| li.timestamp = START.saturating_add(elapsed));

        for _ in 0..CALLS_PER_WINDOW {
            if client.try_check_and_consume(&user).is_ok() {
                allowed += 1;
            } else {
                rejected += 1;
            }
        }
    }

    let expected_allowed = BURST + (WHOLE_SECOND_WINDOWS as u32 * REFILL_RATE);
    let expected_rejected = 1 + (CALLS_PER_WINDOW - REFILL_RATE) * WHOLE_SECOND_WINDOWS as u32;

    assert_eq!(allowed, expected_allowed);
    assert_eq!(rejected, expected_rejected);

    let usage = client.get_usage(&user).unwrap();
    assert_eq!(usage.tokens, 0);
    assert_eq!(usage.last_update, START + WHOLE_SECOND_WINDOWS);
}

#[test]
fn test_long_window_rounding_drift_never_exceeds_theoretical_capacity() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    const START: u64 = 10_000;
    const BURST: u32 = 13;
    const REFILL_RATE: u32 = 3;
    const SECONDS: u64 = 500;
    const CALLS_PER_SECOND: u32 = 5;

    env.ledger().with_mut(|li| li.timestamp = START);
    client.initialize(&admin, &BURST, &REFILL_RATE, &false);

    let mut allowed = 0u32;

    for call_index in 0..CALLS_PER_SECOND * 4 {
        if client.try_check_and_consume(&user).is_ok() {
            allowed += 1;
        }

        let theoretical_allowed = BURST;
        assert!(
            allowed <= theoretical_allowed,
            "same-second call {call_index} exceeded initial burst capacity"
        );
    }

    for elapsed in 1..=SECONDS {
        env.ledger()
            .with_mut(|li| li.timestamp = START.saturating_add(elapsed));

        for _ in 0..CALLS_PER_SECOND {
            if client.try_check_and_consume(&user).is_ok() {
                allowed += 1;
            }
        }

        let theoretical_allowed = BURST + (elapsed as u32 * REFILL_RATE);
        assert!(
            allowed <= theoretical_allowed,
            "elapsed second {elapsed} exceeded theoretical bucket capacity"
        );
    }

    let expected_allowed = BURST + (SECONDS as u32 * REFILL_RATE);
    assert_eq!(allowed, expected_allowed);
}

/// # Fallback precedence: per-address override → default (global) limit
///
/// After `clear_limit_for`, `get_limit_config` returns `DefaultBurst` /
/// `DefaultRefillRate`.  This test proves that `check_and_consume` also uses
/// those defaults — i.e. the call succeeds and draws from the correct bucket
/// — rather than leaving the address with no limit at all or a stale override.
///
/// Scenario:
/// 1. Initialize with `default_burst = 3, default_refill_rate = 0` (no refill, so any
///    over-consumption is detectable immediately).
/// 2. Set a generous per-address override (`burst = 10`) for the subject.
/// 3. Consume 3 tokens through the override to prove it is active.
/// 4. Clear the override via `clear_limit_for`.
/// 5. Assert `get_limit_for` now returns the default config.
/// 6. Reset the address's *usage* so the bucket starts fresh at the default burst capacity (the
///    usage state is orthogonal to the limit config).
/// 7. Consume exactly `default_burst` (3) tokens through `check_and_consume`. Each call must
///    succeed — proving fallback to the default limit is live.
/// 8. Assert the very next call is rejected — proving the default burst cap (not the old override
///    cap) is being enforced.
#[test]
fn test_clear_limit_falls_back_to_default_and_check_and_consume_works() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    // Step 1: default burst = 3, no refill, admin bypass off
    client.initialize(&admin, &3u32, &0u32, &false);

    // Step 2: give the user a generous override so they differ clearly from the default
    client.set_limit_for(&user, &10u32, &0u32);
    let override_config = client.get_limit_for(&user);
    assert_eq!(
        override_config.burst, 10,
        "override should be active before clear"
    );

    // Step 3: consume 3 tokens via override to build up usage state
    client.check_and_consume(&user);
    client.check_and_consume(&user);
    client.check_and_consume(&user);

    // Step 4: remove the per-address override
    client.clear_limit_for(&user);

    // Step 5: config must now reflect default values
    let default_config = client.get_limit_for(&user);
    assert_eq!(
        default_config.burst, 3,
        "burst must fall back to default after clear"
    );
    assert_eq!(
        default_config.refill_rate, 0,
        "refill_rate must fall back to default after clear"
    );

    // Step 6: reset usage so the bucket is fresh at the default burst capacity
    client.reset_usage(&user);

    // Step 7: consume exactly default_burst tokens — all must succeed
    // This is the core assertion: check_and_consume uses the fallback default,
    // not the cleared override and not a missing/zero limit.
    let r1 = client.check_and_consume(&user);
    let r2 = client.check_and_consume(&user);
    let r3 = client.check_and_consume(&user);
    assert_eq!(r1.remaining, 2, "first call should leave 2 tokens");
    assert_eq!(r2.remaining, 1, "second call should leave 1 token");
    assert_eq!(
        r3.remaining, 0,
        "third call should exhaust the default bucket"
    );

    // Step 8: one more call must be rejected — default cap is enforced
    let exhausted = client.try_check_and_consume(&user);
    assert!(
        exhausted.is_err(),
        "check_and_consume must fail when default bucket is empty"
    );
}

/// # Clearing an address that never had an override is a safe no-op
///
/// `clear_limit_for` calls `env.storage().persistent().remove(key)`.  On
/// Soroban, removing a key that does not exist is a no-op (no panic).  This
/// test proves that calling `clear_limit_for` on an address that never had an
/// explicit override does not disrupt the address's subsequent ability to
/// consume tokens under the default limit.
///
/// Scenario:
/// 1. Initialize with `default_burst = 2, default_refill_rate = 0`.
/// 2. Call `clear_limit_for` on a fresh address that has no override.
/// 3. Assert `get_limit_for` still returns the default config — the clear must not zero-out or
///    corrupt the default.
/// 4. Assert `check_and_consume` succeeds for exactly `default_burst` calls — proving the address
///    is still governed by the default limit.
/// 5. Assert the next call fails — the cap is still enforced.
#[test]
fn test_clear_limit_for_address_with_no_override_is_safe_noop() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    // Step 1: default burst = 2, no refill
    client.initialize(&admin, &2u32, &0u32, &false);

    // Step 2: clear with no prior override — must not panic
    client.clear_limit_for(&user);

    // Step 3: config is still the default
    let config = client.get_limit_for(&user);
    assert_eq!(
        config.burst, 2,
        "default burst must be intact after no-op clear"
    );
    assert_eq!(
        config.refill_rate, 0,
        "default refill_rate must be intact after no-op clear"
    );

    // Step 4: check_and_consume still works under the default limit
    let r1 = client.check_and_consume(&user);
    let r2 = client.check_and_consume(&user);
    assert_eq!(r1.remaining, 1, "first call should leave 1 token");
    assert_eq!(r2.remaining, 0, "second call should exhaust the bucket");

    // Step 5: next call must be rejected (cap is enforced)
    assert!(
        client.try_check_and_consume(&user).is_err(),
        "check_and_consume must fail when default bucket is empty after no-op clear"
    );
}

/// # Long idle gap refill is capped at burst capacity
///
/// This test verifies that after an idle gap far longer than the configured
/// refill window, the bucket refills up to its configured cap exactly, without
/// over-crediting extra tokens proportional to the excess elapsed time beyond
/// the cap.
///
/// This is a critical security property: an attacker should not be able to
/// "farm" tokens by waiting a very long time between calls. The token bucket
/// must always cap at `burst`, regardless of how much time has elapsed.
///
/// Scenario:
/// 1. Initialize with `burst = 10, refill_rate = 2` (refill window = 5 seconds to full).
/// 2. Consume 9 tokens, leaving 1 token remaining.
/// 3. Advance ledger timestamp by 100 seconds (20x the refill window).
/// 4. Assert bucket refills to exactly burst = 10 (not 1 + 100*2 = 201).
/// 5. Consume 1 token → 9 remaining.
/// 6. Consume 1 token → 8 remaining.
/// 7. Assert the bucket behavior is consistent with a capped refill.
#[test]
fn test_long_idle_gap_refill_is_capped_at_burst_capacity() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    const BURST: u32 = 10;
    const REFILL_RATE: u32 = 2;
    const START_TIMESTAMP: u64 = 1000;

    // Step 1: initialize with burst=10, refill_rate=2
    env.ledger().with_mut(|li| li.timestamp = START_TIMESTAMP);
    client.initialize(&admin, &BURST, &REFILL_RATE, &false);

    // Step 2: consume 9 tokens, leaving 1 token remaining
    for _ in 0..9 {
        client.check_and_consume(&user);
    }
    assert_eq!(client.check_and_consume(&user).remaining, 0);

    // Step 3: advance ledger by 100 seconds (20x the 5-second refill window)
    // Without capping, this would give 1 + 100*2 = 201 tokens
    // With capping, this should give exactly burst = 10 tokens
    env.ledger()
        .with_mut(|li| li.timestamp = START_TIMESTAMP + 100);

    // Step 4: assert bucket refilled to exactly burst capacity
    // First call after long idle should succeed and leave burst-1 tokens
    let outcome = client.check_and_consume(&user);
    assert_eq!(
        outcome.remaining,
        BURST - 1,
        "after long idle, bucket should be at full burst capacity, not over-credited"
    );

    // Step 5: consume another token → burst-2 remaining
    let outcome = client.check_and_consume(&user);
    assert_eq!(
        outcome.remaining,
        BURST - 2,
        "second consumption should debit from capped balance"
    );

    // Step 6: verify we can consume exactly burst-2 more tokens to empty the bucket
    for _ in 0..(BURST - 2) {
        client.check_and_consume(&user);
    }

    // Step 7: bucket should now be empty
    assert!(
        client.try_check_and_consume(&user).is_err(),
        "bucket should be empty after consuming burst tokens from capped refill"
    );
}

/// # Per-contract budget configuration
///
/// `set_limit_for_contract` stores an opt-in LimitConfig keyed by the integrating
/// contract address. Until set, `get_limit_for_contract` returns `None` and
/// `check_and_consume_for_contract` only enforces the address (+ global) budgets.
#[test]
fn test_set_and_get_limit_for_contract() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let contract = Address::generate(&env);

    client.initialize(&admin, &10u32, &1u32, &false);

    assert_eq!(client.get_limit_for_contract(&contract), None);

    client.set_limit_for_contract(&contract, &3u32, &0u32);
    let config = client.get_limit_for_contract(&contract).unwrap();
    assert_eq!(config.burst, 3);
    assert_eq!(config.refill_rate, 0);

    client.clear_limit_for_contract(&contract);
    assert_eq!(client.get_limit_for_contract(&contract), None);
}

/// # Contract budget is shared across subject addresses
///
/// Address rotation inside one integrating contract must not exceed the
/// contract-scoped burst. With contract burst = 2 and generous per-address
/// defaults, two different subjects can each consume once; a third call from
/// either subject is rejected by the shared contract bucket.
#[test]
fn test_contract_limit_blocks_address_rotation() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let contract = Address::generate(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let user3 = Address::generate(&env);

    // Generous per-address defaults so only the contract bucket can bind.
    client.initialize(&admin, &10u32, &0u32, &false);
    client.set_limit_for_contract(&contract, &2u32, &0u32);

    assert_eq!(
        client
            .check_and_consume_for_contract(&user1, &contract)
            .remaining,
        9
    );
    assert_eq!(
        client
            .check_and_consume_for_contract(&user2, &contract)
            .remaining,
        9
    );

    // Contract bucket exhausted — rotation to user3 must fail.
    assert!(client
        .try_check_and_consume_for_contract(&user3, &contract)
        .is_err());
    // Fresh address quota still remaining on user1, but contract cap binds.
    assert!(client
        .try_check_and_consume_for_contract(&user1, &contract)
        .is_err());
}

/// # Address budget still binds independently of the contract budget
///
/// With a large contract burst and a tight per-address override, the subject
/// exhausts its own bucket while the contract bucket still has capacity.
#[test]
fn test_address_limit_still_enforced_alongside_contract_limit() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let contract = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &10u32, &0u32, &false);
    client.set_limit_for_contract(&contract, &10u32, &0u32);
    client.set_limit_for(&user, &1u32, &0u32);

    assert_eq!(
        client
            .check_and_consume_for_contract(&user, &contract)
            .remaining,
        0
    );
    assert!(client
        .try_check_and_consume_for_contract(&user, &contract)
        .is_err());

    // Another subject can still consume against the remaining contract budget.
    let other = Address::generate(&env);
    assert_eq!(
        client
            .check_and_consume_for_contract(&other, &contract)
            .remaining,
        9
    );
}

/// # Either limit being hit rejects the call
///
/// Covers both failure modes in one scenario sequence:
/// 1) address bucket empty while contract has tokens → reject
/// 2) contract bucket empty while address has tokens → reject
#[test]
fn test_either_contract_or_address_limit_rejects() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let contract = Address::generate(&env);
    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);

    client.initialize(&admin, &5u32, &0u32, &false);
    client.set_limit_for_contract(&contract, &2u32, &0u32);
    client.set_limit_for(&user_a, &1u32, &0u32);

    // user_a: address burst 1 — first call ok, second fails on address limit
    assert_eq!(
        client
            .check_and_consume_for_contract(&user_a, &contract)
            .remaining,
        0
    );
    assert!(
        client
            .try_check_and_consume_for_contract(&user_a, &contract)
            .is_err(),
        "address limit must reject even when contract budget remains"
    );

    // user_b consumes the last contract token
    assert_eq!(
        client
            .check_and_consume_for_contract(&user_b, &contract)
            .remaining,
        4
    );

    // Contract empty: a third subject with full address quota is still rejected
    let user_c = Address::generate(&env);
    assert!(
        client
            .try_check_and_consume_for_contract(&user_c, &contract)
            .is_err(),
        "contract limit must reject even when address budget remains"
    );
}

/// # Existing check_and_consume address path is unchanged
///
/// Configuring a contract budget must not affect callers that still use
/// `check_and_consume` (address-only path).
#[test]
fn test_check_and_consume_ignores_contract_budget() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let contract = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &2u32, &0u32, &false);
    client.set_limit_for_contract(&contract, &1u32, &0u32);

    // Address-only path can consume full default burst despite contract burst=1
    assert_eq!(client.check_and_consume(&user).remaining, 1);
    assert_eq!(client.check_and_consume(&user).remaining, 0);
    assert!(client.try_check_and_consume(&user).is_err());
}

/// # Unconfigured contract skips contract bucket
#[test]
fn test_check_and_consume_for_contract_without_budget_is_address_only() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let contract = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &2u32, &0u32, &false);

    assert_eq!(client.get_limit_for_contract(&contract), None);
    assert_eq!(
        client
            .check_and_consume_for_contract(&user, &contract)
            .remaining,
        1
    );
    assert_eq!(
        client
            .check_and_consume_for_contract(&user, &contract)
            .remaining,
        0
    );
    assert!(client
        .try_check_and_consume_for_contract(&user, &contract)
        .is_err());
    assert_eq!(client.get_contract_usage(&contract), None);
}

/// # Contract usage reset restores the shared bucket
#[test]
fn test_reset_contract_usage() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let contract = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &10u32, &0u32, &false);
    client.set_limit_for_contract(&contract, &1u32, &0u32);

    client.check_and_consume_for_contract(&user, &contract);
    assert!(client
        .try_check_and_consume_for_contract(&user, &contract)
        .is_err());

    client.reset_contract_usage(&contract);
    assert_eq!(
        client
            .check_and_consume_for_contract(&user, &contract)
            .remaining,
        8
    );
}

// ---------------------------------------------------------------------------
// Explicit outcome and exhaustion behaviour
// ---------------------------------------------------------------------------
//
// `check_and_consume` / `check_and_consume_for_contract` return
// `Result<ConsumptionOutcome, RateLimitError>`. These tests pin the four
// boundary cases called out by the contract's documented behaviour:
// consumption below the burst, exactly at the burst, past the burst, and after
// a refill interval.

/// The outcome names what it carries. A bare integer could be the remaining
/// allowance, the consumed amount, or the wait time; the named fields cannot.
#[test]
fn test_outcome_names_remaining_allowance_and_refill_time() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    const BURST: u32 = 5;
    const REFILL_RATE: u32 = 3;

    client.initialize(&admin, &BURST, &REFILL_RATE, &false);

    let outcome: ConsumptionOutcome = client.check_and_consume(&user);

    // `remaining` is the post-debit allowance: 5 - 1 = 4.
    assert_eq!(outcome.remaining, BURST - 1);
    // It is explicitly *not* the amount consumed (1) nor the refill rate (3).
    assert_ne!(outcome.remaining, 1, "remaining is not the consumed amount");
    assert_ne!(
        outcome.remaining, REFILL_RATE,
        "remaining is not the wait time"
    );
    // A non-empty bucket is usable immediately.
    assert_eq!(outcome.refill_in_seconds, Some(0));
}

/// Below the burst: every call is served, `remaining` counts down, and no
/// rejection is possible.
#[test]
fn test_consumption_below_burst_is_served_with_decreasing_allowance() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    const BURST: u32 = 4;

    client.initialize(&admin, &BURST, &1u32, &false);

    for consumed in 1..BURST {
        let outcome = client.check_and_consume(&user);
        assert_eq!(
            outcome.remaining,
            BURST - consumed,
            "call {consumed} is below the burst and must be served"
        );
        assert_eq!(outcome.refill_in_seconds, Some(0));
    }
}

/// Exactly at the burst: the final token is served and `remaining` reaches `0`.
/// That zero is a *success*, which is precisely why exhaustion cannot be
/// signalled by an in-band sentinel.
#[test]
fn test_consumption_exactly_at_burst_is_served_with_zero_remaining() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    const BURST: u32 = 3;

    client.initialize(&admin, &BURST, &1u32, &false);

    let mut last = None;
    for _ in 0..BURST {
        last = Some(client.check_and_consume(&user));
    }

    let outcome = last.unwrap();
    assert_eq!(outcome.remaining, 0, "the burst-exhausting call is served");
    // Empty bucket with a non-zero refill rate: one whole second to the next token.
    assert_eq!(outcome.refill_in_seconds, Some(1));
}

/// Past the burst: the next call is rejected with the typed error rather than
/// a zero sentinel, and the rejection does not debit any bucket.
#[test]
fn test_consumption_past_burst_returns_typed_error_without_debiting() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    const BURST: u32 = 2;

    client.initialize(&admin, &BURST, &0u32, &false);

    // Serve exactly the burst, ending with a successful `remaining == 0`.
    assert_eq!(client.check_and_consume(&user).remaining, 1);
    assert_eq!(client.check_and_consume(&user).remaining, 0);

    // Past the burst: typed error, never an in-band zero.
    assert_eq!(
        client.try_check_and_consume(&user),
        Err(Ok(RateLimitError::RateLimitExceeded))
    );
    // Repeated rejections report the same typed error and leave the bucket as-is.
    assert_eq!(
        client.try_check_and_consume(&user),
        Err(Ok(RateLimitError::RateLimitExceeded))
    );
    assert_eq!(client.get_usage(&user).unwrap().tokens, 0);
}

/// Past the burst on the per-contract path reports the same typed error.
#[test]
fn test_contract_consumption_past_burst_returns_typed_error() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let contract = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &10u32, &0u32, &false);
    client.set_limit_for_contract(&contract, &1u32, &0u32);

    assert_eq!(
        client
            .check_and_consume_for_contract(&user, &contract)
            .remaining,
        9
    );
    assert_eq!(
        client.try_check_and_consume_for_contract(&user, &contract),
        Err(Ok(RateLimitError::RateLimitExceeded))
    );
}

/// After a refill interval: an emptied bucket is served again, and sub-second
/// repetition inside the same whole-second window still earns no credit.
#[test]
fn test_consumption_after_refill_interval_is_served_again() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    const START: u64 = 1_000;
    const BURST: u32 = 2;
    const REFILL_RATE: u32 = 1;

    env.ledger().with_mut(|li| li.timestamp = START);
    client.initialize(&admin, &BURST, &REFILL_RATE, &false);

    // Drain the burst exactly.
    assert_eq!(client.check_and_consume(&user).remaining, 1);
    assert_eq!(client.check_and_consume(&user).remaining, 0);
    assert_eq!(
        client.try_check_and_consume(&user),
        Err(Ok(RateLimitError::RateLimitExceeded))
    );

    // Same whole ledger second: still empty — no fractional refill credit.
    assert_eq!(
        client.try_check_and_consume(&user),
        Err(Ok(RateLimitError::RateLimitExceeded))
    );

    // One whole second later: exactly one token is credited.
    env.ledger().with_mut(|li| li.timestamp = START + 1);
    let after_refill = client.check_and_consume(&user);
    assert_eq!(after_refill.remaining, 0);
    assert_eq!(after_refill.refill_in_seconds, Some(1));

    // Empty again immediately: the refilled token was the only one available.
    assert_eq!(
        client.try_check_and_consume(&user),
        Err(Ok(RateLimitError::RateLimitExceeded))
    );

    // A long idle gap refills up to the burst cap, never beyond it.
    env.ledger().with_mut(|li| li.timestamp = START + 100);
    assert_eq!(client.check_and_consume(&user).remaining, BURST - 1);
}

/// A zero refill rate never refills on its own, so the outcome says so instead
/// of reporting a misleading countdown.
#[test]
fn test_refill_in_seconds_is_none_when_refill_rate_is_zero() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &1u32, &0u32, &false);

    let outcome = client.check_and_consume(&user);
    assert_eq!(outcome.remaining, 0);
    assert_eq!(outcome.refill_in_seconds, None);

    // Time passing does not change that.
    env.ledger().with_mut(|li| li.timestamp = 10_000);
    assert_eq!(
        client.try_check_and_consume(&user),
        Err(Ok(RateLimitError::RateLimitExceeded))
    );
}

/// The admin bypass is reported explicitly as an unlimited allowance instead of
/// being confused with a real remaining balance.
#[test]
fn test_admin_bypass_outcome_reports_unlimited_allowance() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);

    client.initialize(&admin, &0u32, &0u32, &true);

    let outcome = client.check_and_consume(&admin);
    assert_eq!(outcome.remaining, u32::MAX);
    assert_eq!(outcome.refill_in_seconds, Some(0));
}

/// The non-`try` accessor traps on rejection instead of returning the typed
/// error, so callers that need to branch on exhaustion must use `try_`.
#[test]
#[should_panic]
fn test_plain_accessor_traps_when_bucket_is_exhausted() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &1u32, &0u32, &false);

    // The only token is served, leaving zero allowance.
    assert_eq!(client.check_and_consume(&user).remaining, 0);

    // Nothing left to serve: the plain accessor traps.
    client.check_and_consume(&user);
}

/// A rejected call is distinguishable from a served call that happens to leave
/// zero allowance: one is `Err`, the other is `Ok` with `remaining == 0`.
#[test]
fn test_zero_remaining_is_distinguishable_from_exhaustion() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &1u32, &0u32, &false);

    // The plain accessor panics on rejection, so reaching this line at all
    // proves the call was served.
    let served = client.check_and_consume(&user);
    assert_eq!(
        served.remaining, 0,
        "consuming the last token is a served call with zero allowance left"
    );

    let rejected = client.try_check_and_consume(&user);
    assert_eq!(
        rejected,
        Err(Ok(RateLimitError::RateLimitExceeded)),
        "exhaustion is a typed error, not a zero balance"
    );
}
