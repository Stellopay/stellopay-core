#![cfg(test)]
//
// Tests for bounded on-chain audit retention (issue: unbounded persistent state growth).
//
// Acceptance criteria verified:
//   1. A configurable maximum number of retained entries is enforced.
//   2. Writing past the maximum evicts the oldest entry rather than failing.
//   3. Every entry is still emitted as an event regardless of on-chain retention.
//   4. Retention configuration is readable on chain.
//   5. Eviction is oldest-first.
//   6. Events are emitted for evicted entries (`audit_entry_evicted`).

use crate::{PayrollContract, PayrollContractClient};
use soroban_sdk::{
    testutils::Address as _, token::StellarAssetClient, Address, Env, Symbol,
};

// ── helpers ───────────────────────────────────────────────────────────────────

fn setup() -> (Env, Address, PayrollContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    (env, owner, client)
}

/// Creates `n` payroll agreements (one audit entry each).
fn create_n_agreements(env: &Env, client: &PayrollContractClient, n: u32) {
    let employer = Address::generate(env);
    let token_admin = Address::generate(env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    StellarAssetClient::new(env, &token).mint(&employer, &1_000_000i128);

    for _ in 0..n {
        client.create_payroll_agreement(&employer, &token, &3600u64);
    }
}

// ── AC4: retention is readable on chain ──────────────────────────────────────

#[test]
fn retention_defaults_to_zero_unlimited() {
    let (_env, _owner, client) = setup();
    assert_eq!(client.get_audit_retention(), 0u64);
}

#[test]
fn set_and_get_retention() {
    let (_env, owner, client) = setup();
    client.set_audit_retention(&owner, &5u64);
    assert_eq!(client.get_audit_retention(), 5u64);
}

// ── AC1: configurable maximum is enforced ────────────────────────────────────

#[test]
fn retention_holds_at_boundary() {
    let (env, owner, client) = setup();
    client.set_audit_retention(&owner, &3u64);

    // Write exactly 3 entries — all should still be on chain.
    create_n_agreements(&env, &client, 3);

    assert_eq!(client.get_audit_entry_count(), 3u64);
    assert!(client.get_audit_entry(&1u64).is_some());
    assert!(client.get_audit_entry(&2u64).is_some());
    assert!(client.get_audit_entry(&3u64).is_some());
}

// ── AC2: write past maximum evicts oldest, does not fail ─────────────────────

#[test]
fn write_past_limit_does_not_fail() {
    let (env, owner, client) = setup();
    client.set_audit_retention(&owner, &2u64);

    // 4 writes with limit=2 — must not panic.
    create_n_agreements(&env, &client, 4);

    assert_eq!(client.get_audit_entry_count(), 4u64);
}

// ── AC5: eviction is oldest-first ────────────────────────────────────────────

#[test]
fn eviction_is_oldest_first() {
    let (env, owner, client) = setup();
    client.set_audit_retention(&owner, &2u64);

    // ids 1..4 written; limit=2 means ids 1 and 2 get evicted, 3 and 4 survive.
    create_n_agreements(&env, &client, 4);

    assert!(
        client.get_audit_entry(&1u64).is_none(),
        "entry 1 should be evicted"
    );
    assert!(
        client.get_audit_entry(&2u64).is_none(),
        "entry 2 should be evicted"
    );
    assert!(
        client.get_audit_entry(&3u64).is_some(),
        "entry 3 should be retained"
    );
    assert!(
        client.get_audit_entry(&4u64).is_some(),
        "entry 4 should be retained"
    );
}

#[test]
fn limit_one_keeps_only_latest() {
    let (env, owner, client) = setup();
    client.set_audit_retention(&owner, &1u64);

    create_n_agreements(&env, &client, 3);

    assert!(client.get_audit_entry(&1u64).is_none());
    assert!(client.get_audit_entry(&2u64).is_none());
    assert!(client.get_audit_entry(&3u64).is_some());
}

// ── AC3 & AC6: events emitted for every entry, including evicted ones ─────────

#[test]
fn events_emitted_for_all_entries_including_evicted() {
    let (env, owner, client) = setup();
    client.set_audit_retention(&owner, &2u64);

    let employer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    StellarAssetClient::new(&env, &token).mint(&employer, &1_000_000i128);

    // id=1 and id=2 written; id=3 triggers eviction of id=1.
    client.create_payroll_agreement(&employer, &token, &3600u64);
    client.create_payroll_agreement(&employer, &token, &3600u64);
    client.create_payroll_agreement(&employer, &token, &3600u64);

    let all_events = env.events().all();

    let audit_entry_sym = Symbol::new(&env, "audit_entry");
    let evicted_sym = Symbol::new(&env, "audit_entry_evicted");

    let has_entry_event = all_events
        .iter()
        .any(|(topics, _)| topics.iter().any(|t| t == audit_entry_sym.clone().into()));
    assert!(has_entry_event, "audit_entry events should be emitted");

    let has_evicted_event = all_events
        .iter()
        .any(|(topics, _)| topics.iter().any(|t| t == evicted_sym.clone().into()));
    assert!(
        has_evicted_event,
        "audit_entry_evicted event should be emitted for evicted entries"
    );

    // id=1 must be gone from persistent storage.
    assert!(
        client.get_audit_entry(&1u64).is_none(),
        "evicted entry should be removed from storage"
    );
}

// ── unlimited retention (limit=0) leaves all entries on chain ────────────────

#[test]
fn zero_limit_means_unlimited() {
    let (env, _owner, client) = setup();

    let employer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    StellarAssetClient::new(&env, &token).mint(&employer, &1_000_000i128);

    for _ in 0..10 {
        client.create_payroll_agreement(&employer, &token, &3600u64);
    }

    // All 10 entries should remain with the default unlimited retention.
    for i in 1u64..=10 {
        assert!(client.get_audit_entry(&i).is_some());
    }
}

// ── raising the limit after eviction does not resurrect evicted entries ───────

#[test]
fn raising_limit_does_not_resurrect_evicted_entries() {
    let (env, owner, client) = setup();
    client.set_audit_retention(&owner, &2u64);

    create_n_agreements(&env, &client, 4); // evicts ids 1 and 2

    // Raise the limit — evicted entries stay gone.
    client.set_audit_retention(&owner, &100u64);

    assert!(client.get_audit_entry(&1u64).is_none());
    assert!(client.get_audit_entry(&2u64).is_none());
    assert!(client.get_audit_entry(&3u64).is_some());
    assert!(client.get_audit_entry(&4u64).is_some());
}
