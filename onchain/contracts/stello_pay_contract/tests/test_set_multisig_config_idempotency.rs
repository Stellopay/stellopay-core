//! Integration tests proving `set_multisig_config` is safe to call twice
//! (issue #1362).
//!
//! `set_multisig_config` is **idempotent**: a repeated call with identical
//! arguments overwrites persistent storage with the same values and emits a
//! second `multisig_config_changed_event` whose old/new thresholds are equal
//! (a visible no-op for off-chain monitors). A call with *different*
//! arguments cleanly updates to the latest values.
//!
//! Acceptance criteria verified:
//!   1. Calling twice with the same args leaves the same state as calling once.
//!   2. Token balances are asserted after both calls.
//!   3. Emitted events are asserted for both calls.
//!   4. The behaviour chosen is stated in the entry point's doc comment.
//!   5. Calling twice with different args applies only the latest values.
#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Events},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Map, Symbol, TryFromVal, TryIntoVal, Val,
};
use stello_pay_contract::{storage::StorageKey, PayrollContract, PayrollContractClient};

// ── helpers ──────────────────────────────────────────────────────────────────

fn setup() -> (Env, Address, PayrollContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    #[allow(deprecated)]
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    (env, owner, client, contract_id)
}

/// Returns the multisig contract address stored in persistent storage.
fn read_multisig_contract(env: &Env, contract_id: &Address) -> Option<Address> {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get::<_, Address>(&StorageKey::MultisigContract)
    })
}

/// Returns the large-payment threshold stored in persistent storage.
fn read_large_threshold(env: &Env, contract_id: &Address) -> i128 {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get::<_, i128>(&StorageKey::LargePaymentThreshold)
            .unwrap_or(0)
    })
}

/// Returns the dispute-resolution threshold stored in persistent storage.
fn read_dispute_threshold(env: &Env, contract_id: &Address) -> i128 {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get::<_, i128>(&StorageKey::DisputeResolutionThreshold)
            .unwrap_or(0)
    })
}

/// Helper to check if an event with a specific topic name exists in the current invocation.
fn has_event(env: &Env, event_name: &str) -> bool {
    let events = env.events().all();
    let target_sym = Symbol::new(env, event_name);
    events.iter().any(|e| {
        if !e.1.is_empty() {
            let topic = e.1.get(0).unwrap();
            if let Ok(sym) = Symbol::try_from_val(env, &topic) {
                return sym == target_sym;
            }
        }
        false
    })
}

/// Helper to find an event by first topic name in the current invocation.
fn find_event(env: &Env, event_name: &str) -> Option<(Address, soroban_sdk::Vec<Val>, Val)> {
    let events = env.events().all();
    let target_sym = Symbol::new(env, event_name);
    events
        .iter()
        .find(|e| {
            if !e.1.is_empty() {
                let topic = e.1.get(0).unwrap();
                if let Ok(sym) = Symbol::try_from_val(env, &topic) {
                    return sym == target_sym;
                }
            }
            false
        })
        .map(|e| (e.0.clone(), e.1.clone(), e.2))
}

/// Helper to extract a field from map-based event data.
fn get_event_field<T: TryFromVal<Env, Val>>(env: &Env, event_data: &Val, field_name: &str) -> T {
    let map: Map<Symbol, Val> = event_data.try_into_val(env).unwrap();
    let key = Symbol::new(env, field_name);
    let val = map.get(key).unwrap();
    val.try_into_val(env).unwrap()
}

// ── AC1: calling twice leaves same state as calling once ─────────────────────

#[test]
fn test_idempotent_call_leaves_same_state() {
    let (env, owner, client, contract_id) = setup();
    let multisig_addr = Address::generate(&env);
    let large = 1_000i128;
    let dispute = 500i128;

    // First call
    client.set_multisig_config(&owner, &multisig_addr, &large, &dispute);

    let ms1 = read_multisig_contract(&env, &contract_id);
    let lg1 = read_large_threshold(&env, &contract_id);
    let dp1 = read_dispute_threshold(&env, &contract_id);

    // Second call with identical arguments
    client.set_multisig_config(&owner, &multisig_addr, &large, &dispute);

    let ms2 = read_multisig_contract(&env, &contract_id);
    let lg2 = read_large_threshold(&env, &contract_id);
    let dp2 = read_dispute_threshold(&env, &contract_id);

    // Calling twice must leave the exact same state as calling once.
    assert_eq!(ms1, ms2, "multisig contract changed on repeat call");
    assert_eq!(lg1, lg2, "large threshold changed on repeat call");
    assert_eq!(dp1, dp2, "dispute threshold changed on repeat call");

    // Values in storage match the configured arguments.
    assert_eq!(ms2, Some(multisig_addr.clone()));
    assert_eq!(lg2, large);
    assert_eq!(dp2, dispute);

    // Contract getter also returns the identical address.
    assert_eq!(client.get_multisig_contract(), Some(multisig_addr));
}

// ── AC2: token balances are asserted after both calls ────────────────────────

#[test]
fn test_token_balances_asserted_after_both_calls() {
    let (env, owner, client, contract_id) = setup();
    let multisig_addr = Address::generate(&env);
    let large = 2_500i128;
    let dispute = 1_250i128;

    // Setup a token and mint a balance to the contract.
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let token_client = TokenClient::new(&env, &token);
    StellarAssetClient::new(&env, &token).mint(&contract_id, &50_000i128);

    let balance_before = token_client.balance(&contract_id);
    assert_eq!(balance_before, 50_000i128);

    // First call: assert balance immediately after.
    client.set_multisig_config(&owner, &multisig_addr, &large, &dispute);
    let balance_after_first = token_client.balance(&contract_id);
    assert_eq!(
        balance_after_first, balance_before,
        "token balance must not change after first set_multisig_config call"
    );

    // Second call: assert balance immediately after.
    client.set_multisig_config(&owner, &multisig_addr, &large, &dispute);
    let balance_after_second = token_client.balance(&contract_id);
    assert_eq!(
        balance_after_second, balance_after_first,
        "token balance must not change after second set_multisig_config call"
    );
}

// ── AC3: emitted events are asserted for both calls ──────────────────────────

#[test]
fn test_emitted_events_asserted_for_both_calls() {
    let (env, owner, client, _contract_id) = setup();
    let multisig_addr = Address::generate(&env);
    let large = 1_000i128;
    let dispute = 500i128;

    // First call: event emitted reporting 0 as previous thresholds.
    client.set_multisig_config(&owner, &multisig_addr, &large, &dispute);

    assert!(
        has_event(&env, "multisig_config_changed_event"),
        "first call must emit multisig_config_changed_event"
    );
    let event1 = find_event(&env, "multisig_config_changed_event").unwrap();
    let caller1: Address = get_event_field(&env, &event1.2, "caller");
    let ms1: Address = get_event_field(&env, &event1.2, "multisig_contract");
    let old_large_1: i128 = get_event_field(&env, &event1.2, "old_large_threshold");
    let new_large_1: i128 = get_event_field(&env, &event1.2, "new_large_threshold");
    let old_dispute_1: i128 = get_event_field(&env, &event1.2, "old_dispute_threshold");
    let new_dispute_1: i128 = get_event_field(&env, &event1.2, "new_dispute_threshold");

    assert_eq!(caller1, owner);
    assert_eq!(ms1, multisig_addr);
    assert_eq!(old_large_1, 0, "first call previous large threshold is 0");
    assert_eq!(new_large_1, large, "first call new large threshold matches");
    assert_eq!(
        old_dispute_1, 0,
        "first call previous dispute threshold is 0"
    );
    assert_eq!(
        new_dispute_1, dispute,
        "first call new dispute threshold matches"
    );

    // Second call: event emitted reporting old == new (idempotent repeat).
    client.set_multisig_config(&owner, &multisig_addr, &large, &dispute);

    assert!(
        has_event(&env, "multisig_config_changed_event"),
        "second call must emit multisig_config_changed_event"
    );
    let event2 = find_event(&env, "multisig_config_changed_event").unwrap();
    let caller2: Address = get_event_field(&env, &event2.2, "caller");
    let ms2: Address = get_event_field(&env, &event2.2, "multisig_contract");
    let old_large_2: i128 = get_event_field(&env, &event2.2, "old_large_threshold");
    let new_large_2: i128 = get_event_field(&env, &event2.2, "new_large_threshold");
    let old_dispute_2: i128 = get_event_field(&env, &event2.2, "old_dispute_threshold");
    let new_dispute_2: i128 = get_event_field(&env, &event2.2, "new_dispute_threshold");

    assert_eq!(caller2, owner);
    assert_eq!(ms2, multisig_addr);
    assert_eq!(
        old_large_2, large,
        "second call previous large threshold must equal first call's configured value"
    );
    assert_eq!(
        new_large_2, large,
        "second call new large threshold must equal configured value"
    );
    assert_eq!(
        old_large_2, new_large_2,
        "idempotent call must have old_large == new_large"
    );
    assert_eq!(
        old_dispute_2, dispute,
        "second call previous dispute threshold must equal first call's configured value"
    );
    assert_eq!(
        new_dispute_2, dispute,
        "second call new dispute threshold must equal configured value"
    );
    assert_eq!(
        old_dispute_2, new_dispute_2,
        "idempotent call must have old_dispute == new_dispute"
    );
}

// ── Different arguments on second call applies latest ────────────────────────

#[test]
fn test_second_call_with_different_args_applies_latest_and_asserts_events_and_balances() {
    let (env, owner, client, contract_id) = setup();
    let ms1 = Address::generate(&env);
    let ms2 = Address::generate(&env);

    // Setup token balance to verify no tokens are touched on updates.
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let token_client = TokenClient::new(&env, &token);
    StellarAssetClient::new(&env, &token).mint(&contract_id, &10_000i128);

    let bal_0 = token_client.balance(&contract_id);

    // Call 1: initial values
    client.set_multisig_config(&owner, &ms1, &100i128, &200i128);
    let event1 = find_event(&env, "multisig_config_changed_event").unwrap();
    let bal_1 = token_client.balance(&contract_id);
    assert_eq!(bal_0, bal_1, "call 1 must not alter token balance");

    let old_large_1: i128 = get_event_field(&env, &event1.2, "old_large_threshold");
    let new_large_1: i128 = get_event_field(&env, &event1.2, "new_large_threshold");
    assert_eq!(old_large_1, 0);
    assert_eq!(new_large_1, 100);

    // Call 2: new values
    client.set_multisig_config(&owner, &ms2, &999i128, &888i128);
    let event2 = find_event(&env, "multisig_config_changed_event").unwrap();
    let bal_2 = token_client.balance(&contract_id);
    assert_eq!(bal_1, bal_2, "call 2 must not alter token balance");
    let old_large_2: i128 = get_event_field(&env, &event2.2, "old_large_threshold");
    let new_large_2: i128 = get_event_field(&env, &event2.2, "new_large_threshold");
    let old_dispute_2: i128 = get_event_field(&env, &event2.2, "old_dispute_threshold");
    let new_dispute_2: i128 = get_event_field(&env, &event2.2, "new_dispute_threshold");
    assert_eq!(old_large_2, 100, "call 2 reports call 1 threshold as old");
    assert_eq!(new_large_2, 999, "call 2 reports updated threshold as new");
    assert_eq!(old_dispute_2, 200, "call 2 reports call 1 dispute as old");
    assert_eq!(new_dispute_2, 888, "call 2 reports updated dispute as new");

    // Persistent storage reflects the second call's arguments.
    assert_eq!(
        read_multisig_contract(&env, &contract_id),
        Some(ms2.clone())
    );
    assert_eq!(read_large_threshold(&env, &contract_id), 999);
    assert_eq!(read_dispute_threshold(&env, &contract_id), 888);
    assert_eq!(client.get_multisig_contract(), Some(ms2));
}

// ── Audit entries recorded for both calls ────────────────────────────────────

#[test]
fn test_audit_entries_recorded_for_both_calls() {
    let (env, owner, client, _contract_id) = setup();
    let multisig_addr = Address::generate(&env);

    let count_before = client.get_audit_entry_count();

    // Call 1
    client.set_multisig_config(&owner, &multisig_addr, &1_000i128, &500i128);
    let count_after_1 = client.get_audit_entry_count();
    assert_eq!(
        count_after_1,
        count_before + 1,
        "first call should create exactly one audit entry"
    );

    // Call 2
    client.set_multisig_config(&owner, &multisig_addr, &1_000i128, &500i128);
    let count_after_2 = client.get_audit_entry_count();
    assert_eq!(
        count_after_2,
        count_after_1 + 1,
        "second call should create a second audit entry"
    );
}
