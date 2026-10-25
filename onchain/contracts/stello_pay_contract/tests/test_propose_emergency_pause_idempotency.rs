//! Integration tests proving `propose_emergency_pause` is safe to call twice
//! (issue #1370).
//!
//! `propose_emergency_pause` is **idempotent** for a repeated call with the
//! same arguments in the same ledger: it overwrites the pending proposal with
//! identical values and resets the approval list to `[caller]`, so a client
//! retrying after a timeout leaves exactly the state a single call leaves. It
//! moves no tokens and emits no events. The proposer's approval is never
//! counted twice, so a retry cannot bring the pause closer to its threshold.
//!
//! A repeat call is a *restart*, not a no-op, once the proposal has moved on:
//! it discards approvals collected since the first call, and a non-zero
//! timelock is re-anchored to the ledger time of the latest call.
//!
//! Acceptance criteria verified:
//!   1. Calling twice leaves the same state as calling once.
//!   2. Token balances are asserted after both calls.
//!   3. Emitted events are asserted for both calls.
//!   4. The behaviour chosen is stated in the entry point's doc comment.
#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};
use stello_pay_contract::{
    storage::{EmergencyPause, StorageKey},
    PayrollContract, PayrollContractClient,
};

// ── helpers ──────────────────────────────────────────────────────────────────

struct Setup {
    env: Env,
    client: PayrollContractClient<'static>,
    contract_id: Address,
    guardians: std::vec::Vec<Address>,
}

/// Registers the contract with three guardians (approval threshold = 2).
fn setup() -> Setup {
    setup_with_guardians(3)
}

/// Registers the contract with `count` guardians (threshold = count / 2 + 1).
fn setup_with_guardians(count: usize) -> Setup {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = 1_000_000);

    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    let guardians: std::vec::Vec<Address> = (0..count).map(|_| Address::generate(&env)).collect();
    let mut list = Vec::new(&env);
    for g in guardians.iter() {
        list.push_back(g.clone());
    }
    client.set_emergency_guardians(&list);

    Setup {
        env,
        client,
        contract_id,
        guardians,
    }
}

fn read_pending(env: &Env, contract_id: &Address) -> Option<EmergencyPause> {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get::<_, EmergencyPause>(&StorageKey::PendingPause)
    })
}

fn read_approvals(env: &Env, contract_id: &Address) -> Option<Vec<Address>> {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get::<_, Vec<Address>>(&StorageKey::PauseApprovals)
    })
}

/// Number of events the payroll contract emitted in the last invocation.
fn contract_event_count(env: &Env, contract_id: &Address) -> usize {
    env.events()
        .all()
        .iter()
        .filter(|e| &e.0 == contract_id)
        .count()
}

// ── AC1: calling twice leaves the same state as calling once ─────────────────

#[test]
fn test_repeat_call_leaves_same_state_as_single_call() {
    let s = setup();
    let proposer = &s.guardians[0];

    s.client.propose_emergency_pause(proposer, &0);
    let pending_once = read_pending(&s.env, &s.contract_id);
    let approvals_once = read_approvals(&s.env, &s.contract_id);
    let state_once = s.client.get_emergency_pause_state();

    s.client.propose_emergency_pause(proposer, &0);
    let pending_twice = read_pending(&s.env, &s.contract_id);
    let approvals_twice = read_approvals(&s.env, &s.contract_id);
    let state_twice = s.client.get_emergency_pause_state();

    assert_eq!(
        pending_once, pending_twice,
        "pending proposal changed on retry"
    );
    assert_eq!(
        approvals_once, approvals_twice,
        "approvals changed on retry"
    );
    assert_eq!(
        state_once, state_twice,
        "active pause state changed on retry"
    );

    // And that state is exactly one un-activated proposal from `proposer`.
    assert_eq!(
        pending_twice,
        Some(EmergencyPause {
            is_paused: false,
            paused_at: None,
            paused_by: Some(proposer.clone()),
            timelock_end: None,
        })
    );
    let mut expected = Vec::new(&s.env);
    expected.push_back(proposer.clone());
    assert_eq!(approvals_twice, Some(expected));
    assert_eq!(state_twice, None);
    assert!(!s.client.is_emergency_paused());
}

#[test]
fn test_repeat_call_with_timelock_in_same_ledger_keeps_same_deadline() {
    let s = setup();
    let proposer = &s.guardians[0];

    s.client.propose_emergency_pause(proposer, &3_600);
    let first = read_pending(&s.env, &s.contract_id).unwrap();

    s.client.propose_emergency_pause(proposer, &3_600);
    let second = read_pending(&s.env, &s.contract_id).unwrap();

    assert_eq!(first, second);
    assert_eq!(second.timelock_end, Some(1_000_000 + 3_600));
}

#[test]
fn test_retry_does_not_double_count_proposer_approval() {
    // With 3 guardians the threshold is 2. If a retry counted the proposer
    // twice, the pause would already be active; it must still need exactly
    // one more distinct guardian.
    let s = setup();
    let proposer = &s.guardians[0];

    s.client.propose_emergency_pause(proposer, &0);
    s.client.propose_emergency_pause(proposer, &0);

    assert_eq!(read_approvals(&s.env, &s.contract_id).unwrap().len(), 1);
    assert!(!s.client.is_emergency_paused());

    // The proposer approving their own proposal is also not a second vote.
    s.client.approve_emergency_pause(proposer);
    assert!(!s.client.is_emergency_paused());

    s.client.approve_emergency_pause(&s.guardians[1]);
    assert!(s.client.is_emergency_paused());
}

// ── AC2: token balances are asserted after both calls ────────────────────────

#[test]
fn test_token_balances_unchanged_after_both_calls() {
    let s = setup();
    let proposer = &s.guardians[0];

    let token_admin = Address::generate(&s.env);
    let token = s
        .env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let token_client = TokenClient::new(&s.env, &token);
    let minter = StellarAssetClient::new(&s.env, &token);
    minter.mint(&s.contract_id, &50_000i128);
    minter.mint(proposer, &7_500i128);

    let contract_before = token_client.balance(&s.contract_id);
    let proposer_before = token_client.balance(proposer);
    assert_eq!(contract_before, 50_000);
    assert_eq!(proposer_before, 7_500);

    s.client.propose_emergency_pause(proposer, &0);
    assert_eq!(
        token_client.balance(&s.contract_id),
        contract_before,
        "contract balance must not change after first call"
    );
    assert_eq!(
        token_client.balance(proposer),
        proposer_before,
        "proposer balance must not change after first call"
    );

    s.client.propose_emergency_pause(proposer, &0);
    assert_eq!(
        token_client.balance(&s.contract_id),
        contract_before,
        "contract balance must not change after second call"
    );
    assert_eq!(
        token_client.balance(proposer),
        proposer_before,
        "proposer balance must not change after second call"
    );
}

// ── AC3: emitted events are asserted for both calls ──────────────────────────

#[test]
fn test_neither_call_emits_events() {
    let s = setup();
    let proposer = &s.guardians[0];

    s.client.propose_emergency_pause(proposer, &0);
    assert_eq!(
        contract_event_count(&s.env, &s.contract_id),
        0,
        "first call must not emit events"
    );

    s.client.propose_emergency_pause(proposer, &0);
    assert_eq!(
        contract_event_count(&s.env, &s.contract_id),
        0,
        "second call must not emit events"
    );
}

// ── Documented restart semantics (behaviour unchanged) ───────────────────────

#[test]
fn test_repeat_call_after_another_approval_restarts_collection() {
    // Five guardians (threshold 3) so a second approval is stored without
    // activating the pause.
    let s = setup_with_guardians(5);
    let proposer = &s.guardians[0];

    s.client.propose_emergency_pause(proposer, &0);
    s.client.approve_emergency_pause(&s.guardians[1]);
    assert_eq!(read_approvals(&s.env, &s.contract_id).unwrap().len(), 2);
    assert!(!s.client.is_emergency_paused());

    // A late retry of the original proposal discards guardian 2's approval.
    s.client.propose_emergency_pause(proposer, &0);

    let mut expected = Vec::new(&s.env);
    expected.push_back(proposer.clone());
    assert_eq!(read_approvals(&s.env, &s.contract_id), Some(expected));
    assert!(!s.client.is_emergency_paused());
}

#[test]
fn test_repeat_call_in_later_ledger_reanchors_timelock() {
    let s = setup();
    let proposer = &s.guardians[0];

    s.client.propose_emergency_pause(proposer, &3_600);
    s.env.ledger().with_mut(|li| li.timestamp += 60);
    s.client.propose_emergency_pause(proposer, &3_600);

    let pending = read_pending(&s.env, &s.contract_id).unwrap();
    assert_eq!(pending.timelock_end, Some(1_000_000 + 60 + 3_600));
}
