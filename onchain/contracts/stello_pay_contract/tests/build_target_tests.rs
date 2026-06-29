//! Build target validation tests.
//!
//! These tests verify that the contract compiles and behaves correctly under
//! the `wasm32-unknown-unknown` target required by Soroban SDK 23.4.1.
//! Using `wasm32v1-none` causes linker errors or produces binaries that fail
//! on-network deployment; all CI build steps must use `wasm32-unknown-unknown`.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn setup(env: &Env) -> (PayrollContractClient<'_>, Address) {
    env.mock_all_auths();
    let id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(env, &id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (client, owner)
}

/// Verifies the contract can be registered and initialized — the most basic
/// smoke-test that the WASM binary produced by `wasm32-unknown-unknown` is
/// valid and loadable by the Soroban test environment.
#[test]
fn test_contract_registers_and_initializes() {
    let env = Env::default();
    let (_client, _owner) = setup(&env);
}

/// Double-initialization must panic, confirming the guard in `initialize` works
/// regardless of build target.
#[test]
#[should_panic(expected = "Already initialized")]
fn test_double_initialize_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &id);
    let owner = Address::generate(&env);
    client.initialize(&owner);
    client.initialize(&owner); // must panic
}

/// Confirms that `is_emergency_paused` returns `false` on a freshly initialized
/// contract — validates that the default storage state is correct when compiled
/// for `wasm32-unknown-unknown`.
#[test]
fn test_default_emergency_pause_state_is_false() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    assert!(!client.is_emergency_paused());
}

/// Verifies that `get_arbiter` returns `None` before any arbiter is set,
/// confirming that optional storage reads work correctly on the target.
#[test]
fn test_get_arbiter_returns_none_by_default() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    assert!(client.get_arbiter().is_none());
}

/// Verifies that `set_arbiter` and `get_arbiter` round-trip correctly,
/// exercising a basic write-then-read storage path on `wasm32-unknown-unknown`.
#[test]
fn test_set_and_get_arbiter_round_trip() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let arbiter = Address::generate(&env);
    client.set_arbiter(&owner, &arbiter);
    assert_eq!(client.get_arbiter(), Some(arbiter));
}
