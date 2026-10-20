//! Boundary coverage for `PayrollContract::set_rate_limiter_contract`.
//!
//! The entrypoint itself takes only two `Address` values, so it has no
//! numeric or time-valued parameter of its own. The numeric boundary that the
//! wiring exposes lives in the linked rate limiter's `burst` / `refill_rate`
//! configuration. These tests pin both:
//!
//! * the unset ("zero") state and the first ("one") assignment,
//! * a linked limiter configured at `0`, at `1`, and at `u32::MAX`,
//! * a value one past the limiter's `u32` maximum, which cannot be encoded,
//! * every refusal on the payroll entrypoint as a typed `PayrollError`.
//!
//! No behaviour is changed; each case is asserted not to panic.

#![cfg(test)]

use rate_limiter::{LimitConfig, RateLimitError, RateLimiter, RateLimiterClient};
use soroban_sdk::{testutils::Address as _, Address, Env};
use stello_pay_contract::{storage::PayrollError, PayrollContract, PayrollContractClient};

fn setup(env: &Env) -> (PayrollContractClient<'static>, Address) {
    env.mock_all_auths();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (client, owner)
}

fn register_rate_limiter(
    env: &Env,
    admin: &Address,
    burst: u32,
    refill_rate: u32,
) -> (Address, RateLimiterClient<'static>) {
    let id = env.register(RateLimiter, ());
    let client = RateLimiterClient::new(env, &id);
    client.initialize(admin, &burst, &refill_rate, &false);
    (id, client)
}

/// Zero: a fresh payroll contract has no linked limiter, and a linked limiter
/// configured with zero burst/refill refuses consumption with the rate
/// limiter's documented typed error instead of returning an in-band zero.
#[test]
fn test_zero_state_and_zero_limits_are_bounded() {
    let env = Env::default();
    let (client, owner) = setup(&env);

    assert_eq!(client.get_rate_limiter_contract(), None);

    let (rl_id, rl_client) = register_rate_limiter(&env, &owner, 0, 0);
    client.set_rate_limiter_contract(&owner, &rl_id);
    assert_eq!(client.get_rate_limiter_contract(), Some(rl_id));

    let subject = Address::generate(&env);
    assert_eq!(
        rl_client.try_check_and_consume(&subject),
        Err(Ok(RateLimitError::RateLimitExceeded))
    );
}

/// One: the first wiring assignment succeeds, and a linked limiter with a
/// burst of exactly one serves one request then refuses the next with a typed
/// error.
#[test]
fn test_one_assignment_and_one_token_burst() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let (rl_id, rl_client) = register_rate_limiter(&env, &owner, 1, 0);

    client.set_rate_limiter_contract(&owner, &rl_id);
    assert_eq!(client.get_rate_limiter_contract(), Some(rl_id));

    let subject = Address::generate(&env);
    let outcome = rl_client.check_and_consume(&subject);
    assert_eq!(outcome.remaining, 0);
    assert_eq!(
        rl_client.try_check_and_consume(&subject),
        Err(Ok(RateLimitError::RateLimitExceeded))
    );
}

/// Maximum accepted: `u32::MAX` burst and refill are stored unchanged and the
/// first consumption succeeds without overflow.
#[test]
fn test_maximum_u32_limits_are_accepted() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let (rl_id, rl_client) = register_rate_limiter(&env, &owner, u32::MAX, u32::MAX);

    client.set_rate_limiter_contract(&owner, &rl_id);
    assert_eq!(client.get_rate_limiter_contract(), Some(rl_id));

    let subject = Address::generate(&env);
    assert_eq!(
        rl_client.get_limit_for(&subject),
        LimitConfig {
            burst: u32::MAX,
            refill_rate: u32::MAX,
        }
    );
    assert_eq!(
        rl_client.check_and_consume(&subject).remaining,
        u32::MAX - 1
    );
}

/// One past maximum: the limiter's burst/refill ABI is `u32`, so
/// `u32::MAX + 1` is rejected at the type boundary before any contract call.
/// This is the only "one past maximum" that exists for this configuration.
#[test]
fn test_one_past_maximum_is_not_representable() {
    let one_past_maximum = u64::from(u32::MAX) + 1;
    assert!(u32::try_from(one_past_maximum).is_err());
    assert_eq!(u32::try_from(u64::from(u32::MAX)), Ok(u32::MAX));
}

/// A missing owner must be reported as the documented typed error, not a
/// panic escaping the contract.
#[test]
fn test_uninitialized_owner_is_typed_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let rate_limiter = Address::generate(&env);

    assert_eq!(
        client.try_set_rate_limiter_contract(&owner, &rate_limiter),
        Err(Ok(PayrollError::Unauthorized.into()))
    );
}

/// A caller other than the initialized owner is refused with the documented
/// typed error.
#[test]
fn test_non_owner_is_typed_unauthorized() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let attacker = Address::generate(&env);
    let rate_limiter = Address::generate(&env);

    assert_eq!(
        client.try_set_rate_limiter_contract(&attacker, &rate_limiter),
        Err(Ok(PayrollError::Unauthorized.into()))
    );
    assert_eq!(client.get_rate_limiter_contract(), None);
}

/// Missing authorization is surfaced as an error by the fallible client, not
/// as a panic in the test process.
#[test]
fn test_missing_auth_does_not_panic() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let rate_limiter = Address::generate(&env);

    env.mock_auths(&[]);
    assert!(client
        .try_set_rate_limiter_contract(&owner, &rate_limiter)
        .is_err());
}

/// The entrypoint permits replacing the linked limiter; the latest assignment
/// is what the getter returns and no count limit is implied.
#[test]
fn test_reassignment_keeps_latest_address() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let first = Address::generate(&env);
    let second = Address::generate(&env);

    client.set_rate_limiter_contract(&owner, &first);
    client.set_rate_limiter_contract(&owner, &second);

    assert_eq!(client.get_rate_limiter_contract(), Some(second));
}
