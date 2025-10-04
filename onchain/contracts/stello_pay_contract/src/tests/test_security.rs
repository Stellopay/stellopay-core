#![cfg(test)]

use crate::events::{PAUSED_EVENT, UNPAUSED_EVENT};
use crate::payroll::{PayrollContract, PayrollContractClient};
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, Symbol, TryFromVal,
};

// Helper to setup env + contract
fn setup() -> (Env, Address, Address, PayrollContractClient<'static>) {
    let env = Env::default();
    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    (env, owner, attacker, client)
}

//--- AUTHENTICATION ---

// Initialization requires owner auth
#[test]
#[should_panic(expected = "Error(Auth")]
fn initialize_requires_auth() {
    let (env, _owner, attacker, client) = setup();
    // No mock auth => should panic
    client.initialize(&attacker);
}

// Owner can initialize successfully
#[test]
fn initialize_with_owner_succeeds() {
    let (env, owner, _attacker, client) = setup();
    env.mock_all_auths();
    client.initialize(&owner);
    assert_eq!(client.get_owner(), Some(owner));
}

// --- AUTHORIZATION ---

// Only owner can pause
#[test]
#[should_panic(expected = "Error(Contract")]
fn pause_requires_owner() {
    let (env, owner, attacker, client) = setup();
    env.mock_all_auths();
    client.initialize(&owner);
    // attacker should panic
    client.pause(&attacker);
}

// Owner can pause and unpause
#[test]
fn pause_unpause_with_owner() {
    let (env, owner, _attacker, client) = setup();
    env.mock_all_auths();
    client.initialize(&owner);
    client.pause(&owner);
    assert!(client.is_paused());
    client.unpause(&owner);
    assert!(!client.is_paused());
}

// Ownership transfer restricted to current owner
#[test]
#[should_panic(expected = "Error(Contract")]
fn transfer_ownership_requires_owner() {
    let (env, owner, attacker, client) = setup();
    env.mock_all_auths();
    client.initialize(&owner);
    let new_owner = Address::generate(&env);
    // attacker should panic
    client.transfer_ownership(&attacker, &new_owner);
}

// Owner can transfer ownership and new owner takes effect
#[test]
fn transfer_ownership_succeeds() {
    let (env, owner, _attacker, client) = setup();
    env.mock_all_auths();
    client.initialize(&owner);
    let new_owner = Address::generate(&env);
    client.transfer_ownership(&owner, &new_owner);
    assert_eq!(client.get_owner(), Some(new_owner));
}

// --- EVENT HANDLING ---

#[test]
fn pause_emits_event() {
    let (env, owner, _attacker, client) = setup();
    env.mock_all_auths();
    client.initialize(&owner);
    client.pause(&owner);

    let events = env.events().all();
    let has_paused = events.iter().any(|e| {
        e.1.iter().any(|val| {
            // Try to convert the raw Val into a Symbol
            if let Ok(sym) = Symbol::try_from_val(&env, &val) {
                sym == PAUSED_EVENT
            } else {
                false
            }
        })
    });

    assert!(has_paused, "Paused event not emitted");
}

#[test]
fn unpause_emits_event() {
    let (env, owner, _attacker, client) = setup();
    env.mock_all_auths();
    client.initialize(&owner);
    client.pause(&owner); // First pause to set state
    client.unpause(&owner); // Then unpause

    let events = env.events().all();
    let has_unpaused = events.iter().any(|e| {
        e.1.iter().any(|val| {
            if let Ok(sym) = Symbol::try_from_val(&env, &val) {
                sym == UNPAUSED_EVENT
            } else {
                false
            }
        })
    });

    assert!(has_unpaused, "Unpaused event not emitted");
}

// Additional security edge case tests

// Note: pause/unpause functions don't check current state, they just set the state
// So pausing when already paused or unpausing when not paused is allowed

#[test]
fn test_ownership_transfer_to_same_address() {
    let (env, owner, _attacker, client) = setup();
    env.mock_all_auths();
    client.initialize(&owner);

    // Transfer ownership to same address
    client.transfer_ownership(&owner, &owner);

    // Should still be the owner
    assert_eq!(client.get_owner(), Some(owner));
}

#[test]
#[should_panic(expected = "Error(Contract")]
fn test_operations_after_ownership_transfer() {
    let (env, owner, new_owner, client) = setup();
    env.mock_all_auths();
    client.initialize(&owner);

    // Transfer ownership
    client.transfer_ownership(&owner, &new_owner);

    // Old owner should not be able to pause
    client.pause(&owner);
}
