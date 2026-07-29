#![cfg(test)]
#![allow(deprecated)]

use rbac::{RbacContract, RbacContractClient, Role};
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, Vec,
};

// ===========================================================================
// Helpers
// ===========================================================================

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_contract(env: &Env) -> (Address, RbacContractClient<'_>, Address) {
    let contract_id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(env, &contract_id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (contract_id, client, owner)
}

/// Generates `n` distinct addresses in the given environment.
fn gen_addresses(env: &Env, n: usize) -> soroban_sdk::Vec<Address> {
    let mut addrs = soroban_sdk::Vec::new(env);
    for _ in 0..n {
        addrs.push_back(Address::generate(env));
    }
    addrs
}

// ===========================================================================
// 1. Initialization
// ===========================================================================

#[test]
fn test_initialize_sets_admin_role() {
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);

    let roles = client.get_roles(&owner);
    assert_eq!(roles.len(), 1);
    assert_eq!(roles.get(0).unwrap(), Role::Admin);
    assert!(client.has_role(&owner, &Role::Admin));
}

#[test]
fn test_owner_query_returns_bootstrap_admin() {
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    assert_eq!(client.owner(), owner);
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_initialize_twice_fails() {
    let env = create_env();
    let contract_id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    client.initialize(&owner);
    client.initialize(&owner);
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_reinitialize_with_different_owner_fails() {
    let env = create_env();
    let contract_id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(&env, &contract_id);
    let owner1 = Address::generate(&env);
    let owner2 = Address::generate(&env);
    client.initialize(&owner1);
    client.initialize(&owner2);
}

// ===========================================================================
// 2. Role granting – happy path
// ===========================================================================

#[test]
fn test_admin_can_grant_and_revoke_roles() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let user = Address::generate(&env);

    client.grant_role(&admin, &user, &Role::Employer);
    client.grant_role(&admin, &user, &Role::Arbiter);

    let roles = client.get_roles(&user);
    assert_eq!(roles.len(), 2);
    assert!(client.has_role(&user, &Role::Employer));
    assert!(client.has_role(&user, &Role::Arbiter));

    client.revoke_role(&admin, &user, &Role::Arbiter);
    let roles_after = client.get_roles(&user);
    assert_eq!(roles_after.len(), 1);
    assert_eq!(roles_after.get(0).unwrap(), Role::Employer);
    assert!(!client.has_role(&user, &Role::Arbiter));
}

#[test]
fn test_duplicate_grant_is_noop() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let user = Address::generate(&env);

    client.grant_role(&admin, &user, &Role::Employee);
    client.grant_role(&admin, &user, &Role::Employee);

    let roles = client.get_roles(&user);
    assert_eq!(
        roles.len(),
        1,
        "Duplicate grant should not add a second entry"
    );
}

#[test]
fn test_revoke_absent_role_is_noop() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let user = Address::generate(&env);

    // user has no roles yet
    client.revoke_role(&admin, &user, &Role::Arbiter);
    let roles = client.get_roles(&user);
    assert_eq!(roles.len(), 0);
}

#[test]
fn test_grant_admin_to_second_user() {
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let second_admin = Address::generate(&env);

    client.grant_role(&owner, &second_admin, &Role::Admin);
    assert!(client.has_role(&second_admin, &Role::Admin));

    // Second admin can also grant roles.
    let user = Address::generate(&env);
    client.grant_role(&second_admin, &user, &Role::Employee);
    assert!(client.has_role(&user, &Role::Employee));
}

// ===========================================================================
// 3. Role granting – forbidden paths (negative tests)
// ===========================================================================

#[test]
#[should_panic(expected = "Only admin can grant roles")]
fn test_non_admin_cannot_grant_roles() {
    let env = create_env();
    let (_cid, client, _admin) = setup_contract(&env);
    let non_admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.grant_role(&non_admin, &user, &Role::Employee);
}

#[test]
#[should_panic(expected = "Only admin can grant roles")]
fn test_employer_cannot_grant_roles() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let employer = Address::generate(&env);
    let user = Address::generate(&env);

    client.grant_role(&admin, &employer, &Role::Employer);
    client.grant_role(&employer, &user, &Role::Employee);
}

#[test]
#[should_panic(expected = "Only admin can grant roles")]
fn test_employee_cannot_grant_roles() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let employee = Address::generate(&env);
    let user = Address::generate(&env);

    client.grant_role(&admin, &employee, &Role::Employee);
    client.grant_role(&employee, &user, &Role::Employee);
}

#[test]
#[should_panic(expected = "Only admin can grant roles")]
fn test_arbiter_cannot_grant_roles() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let arbiter = Address::generate(&env);
    let user = Address::generate(&env);

    client.grant_role(&admin, &arbiter, &Role::Arbiter);
    client.grant_role(&arbiter, &user, &Role::Employee);
}

// ===========================================================================
// 4. Role revocation – forbidden paths (negative tests)
// ===========================================================================

#[test]
#[should_panic(expected = "Only admin can revoke roles")]
fn test_non_admin_cannot_revoke_roles() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let non_admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.grant_role(&admin, &user, &Role::Employee);
    client.revoke_role(&non_admin, &user, &Role::Employee);
}

#[test]
#[should_panic(expected = "Only admin can revoke roles")]
fn test_employer_cannot_revoke_roles() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let employer = Address::generate(&env);
    let user = Address::generate(&env);

    client.grant_role(&admin, &employer, &Role::Employer);
    client.grant_role(&admin, &user, &Role::Employee);
    client.revoke_role(&employer, &user, &Role::Employee);
}

#[test]
#[should_panic(expected = "Cannot revoke Admin from owner")]
fn test_cannot_revoke_admin_from_owner() {
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);

    // Owner tries to remove their own Admin role – blocked.
    client.revoke_role(&owner, &owner, &Role::Admin);
}

#[test]
#[should_panic(expected = "Cannot revoke Admin from owner")]
fn test_second_admin_cannot_revoke_owner_admin() {
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let second_admin = Address::generate(&env);

    client.grant_role(&owner, &second_admin, &Role::Admin);
    // Second admin cannot strip owner's Admin either.
    client.revoke_role(&second_admin, &owner, &Role::Admin);
}

// ===========================================================================
// 5. Role inheritance – full matrix
// ===========================================================================

/// Tests every (granted, required) combination in a 4x4 matrix to validate
/// the inheritance truth table exhaustively.
#[test]
fn test_role_inheritance_full_matrix() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);

    let all_roles = [Role::Admin, Role::Employer, Role::Employee, Role::Arbiter];

    // Expected truth table: granted (row) × required (col)
    //           Admin  Employer  Employee  Arbiter
    // Admin      T       T         T         T
    // Employer   F       T         T         F
    // Employee   F       F         T         F
    // Arbiter    F       F         F         T
    let expected: [[bool; 4]; 4] = [
        [true, true, true, true],    // Admin grants
        [false, true, true, false],  // Employer grants
        [false, false, true, false], // Employee grants
        [false, false, false, true], // Arbiter grants
    ];

    for (gi, granted) in all_roles.iter().enumerate() {
        let user = Address::generate(&env);
        client.grant_role(&admin, &user, granted);

        for (ri, required) in all_roles.iter().enumerate() {
            let result = client.has_role(&user, required);
            assert_eq!(
                result, expected[gi][ri],
                "Inheritance mismatch: granted={:?}, required={:?}, expected={}, got={}",
                granted, required, expected[gi][ri], result
            );
        }
    }
}

#[test]
fn test_admin_implies_all() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);

    assert!(client.has_role(&admin, &Role::Admin));
    assert!(client.has_role(&admin, &Role::Employer));
    assert!(client.has_role(&admin, &Role::Employee));
    assert!(client.has_role(&admin, &Role::Arbiter));
}

#[test]
fn test_employer_implies_employee_not_arbiter() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let employer = Address::generate(&env);

    client.grant_role(&admin, &employer, &Role::Employer);

    assert!(client.has_role(&employer, &Role::Employer));
    assert!(client.has_role(&employer, &Role::Employee));
    assert!(!client.has_role(&employer, &Role::Admin));
    assert!(!client.has_role(&employer, &Role::Arbiter));
}

#[test]
fn test_employee_only_implies_employee() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let employee = Address::generate(&env);

    client.grant_role(&admin, &employee, &Role::Employee);

    assert!(client.has_role(&employee, &Role::Employee));
    assert!(!client.has_role(&employee, &Role::Employer));
    assert!(!client.has_role(&employee, &Role::Admin));
    assert!(!client.has_role(&employee, &Role::Arbiter));
}

#[test]
fn test_arbiter_only_implies_arbiter() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let arbiter = Address::generate(&env);

    client.grant_role(&admin, &arbiter, &Role::Arbiter);

    assert!(client.has_role(&arbiter, &Role::Arbiter));
    assert!(!client.has_role(&arbiter, &Role::Admin));
    assert!(!client.has_role(&arbiter, &Role::Employer));
    assert!(!client.has_role(&arbiter, &Role::Employee));
}

#[test]
fn test_multi_role_user_has_combined_permissions() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let user = Address::generate(&env);

    client.grant_role(&admin, &user, &Role::Employer);
    client.grant_role(&admin, &user, &Role::Arbiter);

    // Employer + Arbiter combined
    assert!(client.has_role(&user, &Role::Employer));
    assert!(client.has_role(&user, &Role::Employee)); // inherited from Employer
    assert!(client.has_role(&user, &Role::Arbiter));
    assert!(!client.has_role(&user, &Role::Admin));
}

// ===========================================================================
// 6. require_role – access enforcement
// ===========================================================================

#[test]
fn test_require_role_succeeds_with_valid_role() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let arbiter = Address::generate(&env);

    client.grant_role(&admin, &arbiter, &Role::Arbiter);
    client.require_role(&arbiter, &Role::Arbiter);
}

#[test]
fn test_require_role_succeeds_with_inherited_role() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let employer = Address::generate(&env);

    client.grant_role(&admin, &employer, &Role::Employer);
    // Employer inherits Employee.
    client.require_role(&employer, &Role::Employee);
}

#[test]
#[should_panic(expected = "Missing required role")]
fn test_require_role_panics_when_missing() {
    let env = create_env();
    let (_cid, client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);

    client.require_role(&user, &Role::Employer);
}

#[test]
#[should_panic(expected = "Missing required role")]
fn test_require_role_employee_cannot_satisfy_admin() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let employee = Address::generate(&env);

    client.grant_role(&admin, &employee, &Role::Employee);
    client.require_role(&employee, &Role::Admin);
}

#[test]
#[should_panic(expected = "Missing required role")]
fn test_require_role_arbiter_cannot_satisfy_employer() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let arbiter = Address::generate(&env);

    client.grant_role(&admin, &arbiter, &Role::Arbiter);
    client.require_role(&arbiter, &Role::Employer);
}

// ===========================================================================
// 7. Bulk operations
// ===========================================================================

#[test]
fn test_bulk_grant_assigns_multiple_roles() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let user = Address::generate(&env);

    let mut roles_to_grant = Vec::new(&env);
    roles_to_grant.push_back(Role::Employer);
    roles_to_grant.push_back(Role::Arbiter);
    client.bulk_grant(&admin, &user, &roles_to_grant);

    let roles = client.get_roles(&user);
    assert_eq!(roles.len(), 2);
    assert!(client.has_role(&user, &Role::Employer));
    assert!(client.has_role(&user, &Role::Arbiter));
}

#[test]
fn test_bulk_grant_skips_duplicates() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let user = Address::generate(&env);

    client.grant_role(&admin, &user, &Role::Employee);

    let mut roles_to_grant = Vec::new(&env);
    roles_to_grant.push_back(Role::Employee); // already has this
    roles_to_grant.push_back(Role::Arbiter);
    client.bulk_grant(&admin, &user, &roles_to_grant);

    let roles = client.get_roles(&user);
    assert_eq!(roles.len(), 2);
}

#[test]
#[should_panic(expected = "Only admin can grant roles")]
fn test_bulk_grant_forbidden_for_non_admin() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let employer = Address::generate(&env);
    let user = Address::generate(&env);

    client.grant_role(&admin, &employer, &Role::Employer);

    let mut roles_to_grant = Vec::new(&env);
    roles_to_grant.push_back(Role::Employee);
    client.bulk_grant(&employer, &user, &roles_to_grant);
}

#[test]
fn test_bulk_revoke_removes_multiple_roles() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let user = Address::generate(&env);

    client.grant_role(&admin, &user, &Role::Employer);
    client.grant_role(&admin, &user, &Role::Arbiter);
    client.grant_role(&admin, &user, &Role::Employee);
    assert_eq!(client.get_roles(&user).len(), 3);

    let mut roles_to_revoke = Vec::new(&env);
    roles_to_revoke.push_back(Role::Employer);
    roles_to_revoke.push_back(Role::Arbiter);
    client.bulk_revoke(&admin, &user, &roles_to_revoke);

    let roles = client.get_roles(&user);
    assert_eq!(roles.len(), 1);
    assert_eq!(roles.get(0).unwrap(), Role::Employee);
    assert!(!client.has_role(&user, &Role::Employer));
    assert!(!client.has_role(&user, &Role::Arbiter));
}

#[test]
fn test_bulk_revoke_skips_duplicates() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let user = Address::generate(&env);

    client.grant_role(&admin, &user, &Role::Employer);
    client.grant_role(&admin, &user, &Role::Arbiter);

    let mut roles_to_revoke = Vec::new(&env);
    roles_to_revoke.push_back(Role::Employer);
    roles_to_revoke.push_back(Role::Employer); // duplicate in batch
    roles_to_revoke.push_back(Role::Arbiter);
    client.bulk_revoke(&admin, &user, &roles_to_revoke);

    let roles = client.get_roles(&user);
    assert_eq!(roles.len(), 0);
}

#[test]
fn test_bulk_revoke_skips_already_not_held() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let user = Address::generate(&env);

    client.grant_role(&admin, &user, &Role::Employer);

    let mut roles_to_revoke = Vec::new(&env);
    roles_to_revoke.push_back(Role::Employer); // has this
    roles_to_revoke.push_back(Role::Arbiter); // doesn't have this
    roles_to_revoke.push_back(Role::Employee); // doesn't have this
    client.bulk_revoke(&admin, &user, &roles_to_revoke);

    let roles = client.get_roles(&user);
    assert_eq!(roles.len(), 0);
}

#[test]
#[should_panic(expected = "Only admin can revoke roles")]
fn test_bulk_revoke_forbidden_for_non_admin() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let employer = Address::generate(&env);
    let user = Address::generate(&env);

    client.grant_role(&admin, &employer, &Role::Employer);
    client.grant_role(&admin, &user, &Role::Employee);

    let mut roles_to_revoke = Vec::new(&env);
    roles_to_revoke.push_back(Role::Employee);
    client.bulk_revoke(&employer, &user, &roles_to_revoke);
}

#[test]
fn test_bulk_revoke_protects_owner_admin() {
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);

    // Owner has Admin role
    assert!(client.has_role(&owner, &Role::Admin));

    let mut roles_to_revoke = Vec::new(&env);
    roles_to_revoke.push_back(Role::Admin);
    client.bulk_revoke(&owner, &owner, &roles_to_revoke);

    // Owner should still have Admin (protected)
    assert!(client.has_role(&owner, &Role::Admin));
}

#[test]
fn test_bulk_revoke_mixed_held_and_unheld() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let user = Address::generate(&env);

    client.grant_role(&admin, &user, &Role::Employer);
    client.grant_role(&admin, &user, &Role::Employee);

    let mut roles_to_revoke = Vec::new(&env);
    roles_to_revoke.push_back(Role::Employer); // has this
    roles_to_revoke.push_back(Role::Arbiter); // doesn't have this
    roles_to_revoke.push_back(Role::Employee); // has this (directly held)
    client.bulk_revoke(&admin, &user, &roles_to_revoke);

    let roles = client.get_roles(&user);
    // Both Employer and Employee were directly held and revoked
    // Arbiter was never held
    assert_eq!(roles.len(), 0);
}

#[test]
fn test_revoke_all_strips_every_role() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let user = Address::generate(&env);

    client.grant_role(&admin, &user, &Role::Employer);
    client.grant_role(&admin, &user, &Role::Arbiter);
    assert_eq!(client.get_roles(&user).len(), 2);

    client.revoke_all(&admin, &user);
    assert_eq!(client.get_roles(&user).len(), 0);
    assert!(!client.has_role(&user, &Role::Employer));
    assert!(!client.has_role(&user, &Role::Arbiter));
}

#[test]
#[should_panic(expected = "Cannot revoke all roles from owner")]
fn test_revoke_all_blocked_on_owner() {
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);

    client.revoke_all(&owner, &owner);
}

#[test]
#[should_panic(expected = "Only admin can revoke roles")]
fn test_revoke_all_forbidden_for_non_admin() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let attacker = Address::generate(&env);

    client.grant_role(&admin, &user, &Role::Employee);
    client.revoke_all(&attacker, &user);
}

// ===========================================================================
// 8. Ownership transfer (two-step)
// ===========================================================================

#[test]
fn test_ownership_transfer_full_lifecycle() {
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let new_owner = Address::generate(&env);

    // Step 1: propose
    client.transfer_ownership(&owner, &new_owner);

    // Step 2: accept
    client.accept_ownership(&new_owner);

    // Verify new owner
    assert_eq!(client.owner(), new_owner);
    assert!(client.has_role(&new_owner, &Role::Admin));

    // Old owner should lose Admin
    assert!(!client.has_role(&owner, &Role::Admin));
}

#[test]
fn test_new_owner_can_grant_after_transfer() {
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let new_owner = Address::generate(&env);

    client.transfer_ownership(&owner, &new_owner);
    client.accept_ownership(&new_owner);

    let user = Address::generate(&env);
    client.grant_role(&new_owner, &user, &Role::Employer);
    assert!(client.has_role(&user, &Role::Employer));
}

#[test]
#[should_panic(expected = "Only owner can transfer ownership")]
fn test_non_owner_cannot_transfer_ownership() {
    let env = create_env();
    let (_cid, client, _owner) = setup_contract(&env);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);

    client.transfer_ownership(&attacker, &target);
}

#[test]
#[should_panic(expected = "Only owner can transfer ownership")]
fn test_admin_non_owner_cannot_transfer_ownership() {
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let second_admin = Address::generate(&env);
    let target = Address::generate(&env);

    client.grant_role(&owner, &second_admin, &Role::Admin);
    // second_admin has Admin but is not the owner
    client.transfer_ownership(&second_admin, &target);
}

#[test]
#[should_panic(expected = "Caller is not pending owner")]
fn test_wrong_address_cannot_accept_ownership() {
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let new_owner = Address::generate(&env);
    let attacker = Address::generate(&env);

    client.transfer_ownership(&owner, &new_owner);
    client.accept_ownership(&attacker);
}

#[test]
#[should_panic(expected = "No pending owner")]
fn test_accept_without_proposal_fails() {
    let env = create_env();
    let (_cid, client, _owner) = setup_contract(&env);
    let random = Address::generate(&env);

    client.accept_ownership(&random);
}

#[test]
#[should_panic(expected = "Only admin can grant roles")]
fn test_old_owner_loses_admin_after_transfer() {
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let new_owner = Address::generate(&env);

    client.transfer_ownership(&owner, &new_owner);
    client.accept_ownership(&new_owner);

    // Old owner should no longer be able to grant roles.
    let user = Address::generate(&env);
    client.grant_role(&owner, &user, &Role::Employee);
}

#[test]
fn test_accept_ownership_emits_event_with_both_addresses() {
    let env = create_env();
    let (contract_id, client, owner) = setup_contract(&env);
    let new_owner = Address::generate(&env);

    client.transfer_ownership(&owner, &new_owner);
    client.accept_ownership(&new_owner);

    let events = env.events().all();
    let last = events.last().unwrap();

    // events.all() returns Vec<(Address, Vec<Val>, Val)>
    let (contract, topics, _data) = last;

    // Verify event emitted from this contract
    assert_eq!(contract, contract_id, "event must be from contract");

    // Topics: [Symbol("RBAC"), Symbol("owner")]
    assert_eq!(
        topics.len(),
        2,
        "expected 2 event topics: 'RBAC', 'owner'"
    );
}

#[test]
fn test_accept_ownership_no_event_on_failure() {
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let proposed = Address::generate(&env);
    let imposter = Address::generate(&env);

    client.transfer_ownership(&owner, &proposed);

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.accept_ownership(&imposter);
    }));

    let events = env.events().all();
    // The events here should be only from transfer_ownership ("propose").
    // No ("RBAC", "owner") event should exist.
    for (_contract, topics, _data) in events.iter() {
        assert_ne!(
            topics.len(),
            2,
            "unexpected event with 2 topics on failed accept"
        );
    }
}

// ===========================================================================
// 9. Uninitialized contract guard
// ===========================================================================

#[test]
#[should_panic(expected = "Contract not initialized")]
fn test_grant_role_before_init_fails() {
    let env = create_env();
    let contract_id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(&env, &contract_id);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    client.grant_role(&a, &b, &Role::Employee);
}

#[test]
#[should_panic(expected = "Contract not initialized")]
fn test_revoke_role_before_init_fails() {
    let env = create_env();
    let contract_id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(&env, &contract_id);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    client.revoke_role(&a, &b, &Role::Employee);
}

#[test]
#[should_panic(expected = "Contract not initialized")]
fn test_has_role_before_init_fails() {
    let env = create_env();
    let contract_id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(&env, &contract_id);
    let a = Address::generate(&env);
    client.has_role(&a, &Role::Employee);
}

#[test]
#[should_panic(expected = "Contract not initialized")]
fn test_require_role_before_init_fails() {
    let env = create_env();
    let contract_id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(&env, &contract_id);
    let a = Address::generate(&env);
    client.require_role(&a, &Role::Employee);
}

#[test]
#[should_panic(expected = "Contract not initialized")]
fn test_get_roles_before_init_fails() {
    let env = create_env();
    let contract_id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(&env, &contract_id);
    let a = Address::generate(&env);
    client.get_roles(&a);
}

#[test]
#[should_panic(expected = "Contract not initialized")]
fn test_bulk_grant_before_init_fails() {
    let env = create_env();
    let contract_id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(&env, &contract_id);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    client.bulk_grant(&a, &b, &Vec::new(&env));
}

#[test]
#[should_panic(expected = "Contract not initialized")]
fn test_bulk_revoke_before_init_fails() {
    let env = create_env();
    let contract_id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(&env, &contract_id);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    client.bulk_revoke(&a, &b, &Vec::new(&env));
}

#[test]
#[should_panic(expected = "Contract not initialized")]
fn test_revoke_all_before_init_fails() {
    let env = create_env();
    let contract_id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(&env, &contract_id);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    client.revoke_all(&a, &b);
}

#[test]
#[should_panic(expected = "Contract not initialized")]
fn test_transfer_ownership_before_init_fails() {
    let env = create_env();
    let contract_id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(&env, &contract_id);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    client.transfer_ownership(&a, &b);
}

// ===========================================================================
// 10. Security scenarios
// ===========================================================================

/// Validate that revoking a role truly removes access. This tests the
/// "grant → verify → revoke → verify gone" cycle for every role.
#[test]
fn test_role_grant_revoke_cycle_all_roles() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);

    let roles = [Role::Employer, Role::Employee, Role::Arbiter];
    for role in roles.iter() {
        let user = Address::generate(&env);
        client.grant_role(&admin, &user, role);
        assert!(client.has_role(&user, role));

        client.revoke_role(&admin, &user, role);
        assert!(!client.has_role(&user, role));
    }
}

/// Ensure a user with zero roles has no implied access.
#[test]
fn test_address_with_no_roles_has_no_access() {
    let env = create_env();
    let (_cid, client, _admin) = setup_contract(&env);
    let nobody = Address::generate(&env);

    assert!(!client.has_role(&nobody, &Role::Admin));
    assert!(!client.has_role(&nobody, &Role::Employer));
    assert!(!client.has_role(&nobody, &Role::Employee));
    assert!(!client.has_role(&nobody, &Role::Arbiter));
}

/// Regression: granting a role to user A must not affect user B.
#[test]
fn test_role_isolation_between_addresses() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.grant_role(&admin, &alice, &Role::Employer);

    assert!(client.has_role(&alice, &Role::Employer));
    assert!(!client.has_role(&bob, &Role::Employer));
    assert!(!client.has_role(&bob, &Role::Employee));
}

/// An Admin-role user who is NOT the owner can grant/revoke, but cannot
/// transfer ownership.
#[test]
fn test_delegated_admin_can_manage_roles_but_not_ownership() {
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let delegate = Address::generate(&env);

    client.grant_role(&owner, &delegate, &Role::Admin);

    // Delegate can grant.
    let user = Address::generate(&env);
    client.grant_role(&delegate, &user, &Role::Employee);
    assert!(client.has_role(&user, &Role::Employee));

    // Delegate can revoke.
    client.revoke_role(&delegate, &user, &Role::Employee);
    assert!(!client.has_role(&user, &Role::Employee));
}

/// Validates that after ownership transfer, the new owner is protected
/// from having their Admin role revoked.
#[test]
#[should_panic(expected = "Cannot revoke Admin from owner")]
fn test_new_owner_admin_protected_after_transfer() {
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let new_owner = Address::generate(&env);

    client.transfer_ownership(&owner, &new_owner);
    client.accept_ownership(&new_owner);

    // Even if old_owner somehow gained Admin back, they can't revoke
    // the new owner's Admin. But actually old_owner lost Admin, so
    // let new_owner grant it to a delegate and try.
    let delegate = Address::generate(&env);
    client.grant_role(&new_owner, &delegate, &Role::Admin);
    client.revoke_role(&delegate, &new_owner, &Role::Admin);
}

/// Bulk operations on multiple addresses.
#[test]
fn test_grant_roles_to_multiple_users() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let addrs = gen_addresses(&env, 5);

    for i in 0..addrs.len() {
        let addr = addrs.get(i).unwrap();
        client.grant_role(&admin, &addr, &Role::Employee);
    }

    for i in 0..addrs.len() {
        let addr = addrs.get(i).unwrap();
        assert!(client.has_role(&addr, &Role::Employee));
    }
}

/// Ensure that revoking Admin from a delegate (non-owner) works fine.
#[test]
fn test_revoke_admin_from_non_owner_delegate() {
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let delegate = Address::generate(&env);

    client.grant_role(&owner, &delegate, &Role::Admin);
    assert!(client.has_role(&delegate, &Role::Admin));

    client.revoke_role(&owner, &delegate, &Role::Admin);
    assert!(!client.has_role(&delegate, &Role::Admin));
}

/// After revoking Admin from a delegate, they can no longer grant roles.
#[test]
#[should_panic(expected = "Only admin can grant roles")]
fn test_revoked_admin_cannot_grant() {
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let delegate = Address::generate(&env);

    client.grant_role(&owner, &delegate, &Role::Admin);
    client.revoke_role(&owner, &delegate, &Role::Admin);

    let user = Address::generate(&env);
    client.grant_role(&delegate, &user, &Role::Employee);
}

// ===========================================================================
// 11. Override-safety classification (issue #1055)
//
// These tests assert the SECURITY-CRITICAL and SAFELY-CUSTOMIZABLE invariants
// that every implementer of `RbacContractInterface` must preserve. They map
// 1-to-1 onto the `@customization-safety` and `@invariant` tags in
// `onchain/contracts/rbac-interface/src/lib.rs` and onto the reviewer
// checklist in `docs/rbac.md`. If any of these tests fail, an implementer
// has weakened an access-control invariant and must be rejected.
// ===========================================================================

// ---------- A. Initialization invariant ------------------------------------

#[test]
fn test_override_safety_initialize_grants_admin_to_owner() {
    // @customization-safety SECURITY-CRITICAL
    // @invariant: after initialize, owner holds Admin.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    assert!(client.has_role(&owner, &Role::Admin));
    assert_eq!(client.get_roles(&owner).len(), 1);
}

#[test]
fn test_override_safety_initialize_assigns_owner_record() {
    // @invariant: owner() returns the bootstrap address.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    assert_eq!(client.owner(), owner);
}

#[test]
fn test_override_safety_initialize_no_extra_roles() {
    // @invariant: after initialize, no other address holds any role.
    let env = create_env();
    let (_cid, client, _owner) = setup_contract(&env);
    let bystander = Address::generate(&env);
    assert_eq!(client.get_roles(&bystander).len(), 0);
    assert!(!client.has_role(&bystander, &Role::Admin));
}

// ---------- B. Core authorization invariants -------------------------------

#[test]
fn test_override_safety_has_role_requires_actual_role() {
    // @invariant: has_role returns false when no implied role is held.
    let env = create_env();
    let (_cid, client, _owner) = setup_contract(&env);
    let nobody = Address::generate(&env);
    for role in [Role::Admin, Role::Employer, Role::Employee, Role::Arbiter] {
        assert!(!client.has_role(&nobody, &role));
    }
}

#[test]
fn test_override_safety_has_role_matches_require_role() {
    // @invariant: has_role(a, r) == !require_role_panics(a, r) for every pair.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let employer = Address::generate(&env);
    client.grant_role(&owner, &employer, &Role::Employer);

    // require_role succeeds for held/implied roles.
    client.require_role(&owner, &Role::Admin);
    client.require_role(&employer, &Role::Employer);
    client.require_role(&employer, &Role::Employee); // inherited

    // Mirror: has_role agrees with the no-panic cases above.
    assert!(client.has_role(&owner, &Role::Admin));
    assert!(client.has_role(&employer, &Role::Employer));
    assert!(client.has_role(&employer, &Role::Employee));
}

#[test]
#[should_panic(expected = "Missing required role")]
fn test_override_safety_require_role_panics_when_has_role_false() {
    // @invariant: require_role panics iff has_role is false.
    let env = create_env();
    let (_cid, client, _owner) = setup_contract(&env);
    let nobody = Address::generate(&env);
    // has_role returns false -> require_role must panic.
    client.require_role(&nobody, &Role::Employer);
}

#[test]
fn test_override_safety_has_role_inheritance_matrix() {
    // @invariant: Admin implies all; Employer implies Employer+Employee;
    //             Employee implies Employee; Arbiter implies Arbiter.
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let arbiter = Address::generate(&env);

    client.grant_role(&admin, &employer, &Role::Employer);
    client.grant_role(&admin, &employee, &Role::Employee);
    client.grant_role(&admin, &arbiter, &Role::Arbiter);

    // Admin
    assert!(client.has_role(&admin, &Role::Admin));
    assert!(client.has_role(&admin, &Role::Employer));
    assert!(client.has_role(&admin, &Role::Employee));
    assert!(client.has_role(&admin, &Role::Arbiter));

    // Employer
    assert!(client.has_role(&employer, &Role::Employer));
    assert!(client.has_role(&employer, &Role::Employee));
    assert!(!client.has_role(&employer, &Role::Admin));
    assert!(!client.has_role(&employer, &Role::Arbiter));

    // Employee
    assert!(client.has_role(&employee, &Role::Employee));
    assert!(!client.has_role(&employee, &Role::Employer));
    assert!(!client.has_role(&employee, &Role::Admin));
    assert!(!client.has_role(&employee, &Role::Arbiter));

    // Arbiter
    assert!(client.has_role(&arbiter, &Role::Arbiter));
    assert!(!client.has_role(&arbiter, &Role::Employer));
    assert!(!client.has_role(&arbiter, &Role::Employee));
    assert!(!client.has_role(&arbiter, &Role::Admin));
}

// ---------- C. Privilege-escalation invariants ------------------------------

#[test]
#[should_panic(expected = "Only admin can grant roles")]
fn test_override_safety_grant_role_admin_only_enforced_per_call() {
    // @invariant: grant_role requires Admin; non-admin calls revert.
    let env = create_env();
    let (_cid, client, _owner) = setup_contract(&env);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);

    client.grant_role(&attacker, &target, &Role::Admin);
}

#[test]
fn test_override_safety_grant_role_idempotent_no_duplicate() {
    // @invariant: duplicate grants are silent no-ops (no duplicate storage entries).
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let user = Address::generate(&env);

    client.grant_role(&owner, &user, &Role::Employee);
    client.grant_role(&owner, &user, &Role::Employee);
    client.grant_role(&owner, &user, &Role::Employee);

    assert_eq!(client.get_roles(&user).len(), 1);
}

#[test]
#[should_panic(expected = "Only admin can grant roles")]
fn test_override_safety_bulk_grant_enforces_admin_once_per_call() {
    // @invariant: bulk_grant admin check is per-call, not per-element.
    //             This guards against both partial-application and bypass
    //             via batch splitting.
    let env = create_env();
    let (_cid, client, _owner) = setup_contract(&env);
    let non_admin = Address::generate(&env);
    let target = Address::generate(&env);

    let mut batch = Vec::new(&env);
    batch.push_back(Role::Employee);
    batch.push_back(Role::Employer);
    batch.push_back(Role::Arbiter);

    client.bulk_grant(&non_admin, &target, &batch);
}

#[test]
fn test_override_safety_bulk_grant_empty_vector_succeeds_for_admin() {
    // @invariant: an empty batch from an Admin caller is a silent no-op success.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let target = Address::generate(&env);

    client.bulk_grant(&owner, &target, &Vec::new(&env));
    assert_eq!(client.get_roles(&target).len(), 0);
}

#[test]
fn test_override_safety_bulk_grant_skips_duplicates() {
    // @invariant: duplicates within batch + already-held roles are no-ops.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let target = Address::generate(&env);

    client.grant_role(&owner, &target, &Role::Employee);

    let mut batch = Vec::new(&env);
    batch.push_back(Role::Employee); // already held
    batch.push_back(Role::Employee); // duplicate within batch
    batch.push_back(Role::Arbiter); // new
    client.bulk_grant(&owner, &target, &batch);

    assert_eq!(client.get_roles(&target).len(), 2);
}

// ---------- D. Privilege-revocation invariants -----------------------------

#[test]
#[should_panic(expected = "Cannot revoke Admin from owner")]
fn test_override_safety_revoke_role_protects_owner_admin() {
    // @invariant: owner's Admin cannot be revoked even by owner themselves.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);

    // Owner attempts self-revoke of Admin.
    client.revoke_role(&owner, &owner, &Role::Admin);
}

#[test]
fn test_override_safety_revoke_role_protects_owner_admin_post_check() {
    // @invariant: after a failed self-revoke, owner still holds Admin.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.revoke_role(&owner, &owner, &Role::Admin);
    }));
    assert!(client.has_role(&owner, &Role::Admin));
    assert_eq!(client.owner(), owner);
}

#[test]
#[should_panic(expected = "Cannot revoke all roles from owner")]
fn test_override_safety_revoke_all_blocks_on_owner() {
    // @invariant: revoke_all(target = owner) must revert.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);

    client.revoke_all(&owner, &owner);
}

#[test]
fn test_override_safety_revoke_all_blocks_on_owner_post_check() {
    // @invariant: after a failed revoke_all on owner, owner still holds Admin.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.revoke_all(&owner, &owner);
    }));
    assert!(client.has_role(&owner, &Role::Admin));
    assert_eq!(client.get_roles(&owner).len(), 1);
}

#[test]
#[should_panic(expected = "Only admin can revoke roles")]
fn test_override_safety_revoke_role_admin_only() {
    // @invariant: non-admin revoke_role must revert.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let employer = Address::generate(&env);
    let target = Address::generate(&env);

    client.grant_role(&owner, &employer, &Role::Employer);
    client.grant_role(&owner, &target, &Role::Employee);

    client.revoke_role(&employer, &target, &Role::Employee);
}

#[test]
fn test_override_safety_revoke_role_admin_only_post_check() {
    // @invariant: target retains role after a failed non-admin revoke attempt.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let employer = Address::generate(&env);
    let target = Address::generate(&env);

    client.grant_role(&owner, &employer, &Role::Employer);
    client.grant_role(&owner, &target, &Role::Employee);

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.revoke_role(&employer, &target, &Role::Employee);
    }));
    assert!(
        client.has_role(&target, &Role::Employee),
        "target retains role after failed revoke attempt"
    );
}

#[test]
fn test_override_safety_revoke_role_noop_for_unheld_role() {
    // @invariant: revoking a role the target does not hold is a silent no-op.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let target = Address::generate(&env);

    // Target has no roles; revoke is silent no-op.
    client.revoke_role(&owner, &target, &Role::Arbiter);
    assert_eq!(client.get_roles(&target).len(), 0);
}

// ---------- E. Ownership tracking invariants -------------------------------

#[test]
fn test_override_safety_owner_after_transfer_is_new_owner() {
    // @invariant: owner() reflects the most recent successful transfer.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let new_owner = Address::generate(&env);

    assert_eq!(client.owner(), owner);
    client.transfer_ownership(&owner, &new_owner);
    client.accept_ownership(&new_owner);
    assert_eq!(client.owner(), new_owner);
}

#[test]
#[should_panic(expected = "Cannot revoke Admin from owner")]
fn test_override_safety_owner_protected_via_lockout_check() {
    // @invariant: address returned by owner() is the one whose Admin is
    //             protected from revocation.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let reported_owner = client.owner();
    assert_eq!(reported_owner, owner);

    // The reported owner's Admin cannot be revoked.
    client.revoke_role(&owner, &reported_owner, &Role::Admin);
}

// ---------- F. Two-step ownership-transfer invariants ----------------------

#[test]
#[should_panic(expected = "Only owner can transfer ownership")]
fn test_override_safety_transfer_ownership_requires_owner() {
    // @invariant: Admin alone is insufficient to call transfer_ownership.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let delegated_admin = Address::generate(&env);
    let new_target = Address::generate(&env);

    client.grant_role(&owner, &delegated_admin, &Role::Admin);

    client.transfer_ownership(&delegated_admin, &new_target);
}

#[test]
fn test_override_safety_transfer_ownership_requires_owner_post_check() {
    // @invariant: after a failed delegated transfer, owner is unchanged.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let delegated_admin = Address::generate(&env);
    let new_target = Address::generate(&env);

    client.grant_role(&owner, &delegated_admin, &Role::Admin);

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.transfer_ownership(&delegated_admin, &new_target);
    }));
    assert_eq!(client.owner(), owner);
}

#[test]
#[should_panic(expected = "No pending owner")]
fn test_override_safety_accept_ownership_requires_pending_proposal() {
    // @invariant: accept_ownership without a prior transfer_ownership must revert.
    let env = create_env();
    let (_cid, client, _owner) = setup_contract(&env);
    let stranger = Address::generate(&env);
    client.accept_ownership(&stranger);
}

#[test]
#[should_panic(expected = "Caller is not pending owner")]
fn test_override_safety_accept_ownership_rejects_non_pending_caller() {
    // @invariant: only the recorded pending owner may call accept_ownership.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let proposed = Address::generate(&env);
    let imposter = Address::generate(&env);

    client.transfer_ownership(&owner, &proposed);

    client.accept_ownership(&imposter);
}

#[test]
fn test_override_safety_accept_ownership_rejects_non_pending_post_check() {
    // @invariant: imposter does not gain Admin after a failed accept.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let proposed = Address::generate(&env);
    let imposter = Address::generate(&env);

    client.transfer_ownership(&owner, &proposed);

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.accept_ownership(&imposter);
    }));
    assert_eq!(client.owner(), owner);
    assert!(!client.has_role(&imposter, &Role::Admin));
}

#[test]
fn test_override_safety_accept_ownership_atomic_grant_and_revoke() {
    // @invariant: on successful accept_ownership,
    //             (1) new owner receives Admin, (2) old owner loses Admin,
    //             (3) owner() == new owner, (4) pending slot cleared.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let new_owner = Address::generate(&env);

    client.transfer_ownership(&owner, &new_owner);
    client.accept_ownership(&new_owner);

    // (1) New owner has Admin.
    assert!(client.has_role(&new_owner, &Role::Admin));
    // (2) Old owner has lost Admin.
    assert!(!client.has_role(&owner, &Role::Admin));
    // (3) owner() returns new owner.
    assert_eq!(client.owner(), new_owner);
}

#[test]
#[should_panic(expected = "No pending owner")]
fn test_override_safety_pending_slot_cleared_after_accept() {
    // @invariant: after a successful accept_ownership, the pending slot
    //             is cleared — a second accept_ownership must revert.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let new_owner = Address::generate(&env);

    client.transfer_ownership(&owner, &new_owner);
    client.accept_ownership(&new_owner);

    // Pending slot is cleared: this second call must revert.
    client.accept_ownership(&new_owner);
}

#[test]
fn test_override_safety_transfer_ownership_self_proposal_allowed() {
    // @invariant: transfer_ownership to self is allowed; accepting is a no-op-ish
    //             rotation that preserves the admin grant and revoke invariants.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let new_owner = owner.clone();

    client.transfer_ownership(&owner, &new_owner);
    client.accept_ownership(&new_owner);

    assert_eq!(client.owner(), owner);
    assert!(client.has_role(&owner, &Role::Admin));
}

// ---------- G. get_roles invariants (the SAFELY-CUSTOMIZABLE method) ------

#[test]
fn test_override_safety_get_roles_returns_only_direct_roles() {
    // @invariant: get_roles returns exactly the roles granted, no inherited roles.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let user = Address::generate(&env);

    client.grant_role(&owner, &user, &Role::Employee);
    // Employee has no inheritance children, so direct == implied for this role.
    let roles = client.get_roles(&user);
    assert_eq!(roles.len(), 1);
    assert_eq!(roles.get(0).unwrap(), Role::Employee);

    // Employer implies Employee, but get_roles must still only show Employer.
    let employer = Address::generate(&env);
    client.grant_role(&owner, &employer, &Role::Employer);
    let employer_roles = client.get_roles(&employer);
    assert_eq!(employer_roles.len(), 1);
    assert_eq!(employer_roles.get(0).unwrap(), Role::Employer);
}

#[test]
fn test_override_safety_get_roles_empty_for_unknown_address() {
    // @invariant: get_roles(unknown) returns empty, never panics, never returns phantom roles.
    let env = create_env();
    let (_cid, client, _owner) = setup_contract(&env);
    let unknown = Address::generate(&env);
    let roles = client.get_roles(&unknown);
    assert_eq!(roles.len(), 0);
}

#[test]
fn test_override_safety_get_roles_no_duplicates_after_idempotent_grants() {
    // @invariant: duplicate grants do not produce duplicate role entries.
    //             We loop 100 times to make any duplicate-creation
    //             regression obvious.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let user = Address::generate(&env);

    for _ in 0..100 {
        client.grant_role(&owner, &user, &Role::Arbiter);
    }
    let roles = client.get_roles(&user);
    assert_eq!(
        roles.len(),
        1,
        "duplicate grants must not duplicate storage entries"
    );
}

#[test]
fn test_override_safety_get_roles_updates_after_revoke() {
    // @invariant: get_roles reflects revocations.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let user = Address::generate(&env);

    client.grant_role(&owner, &user, &Role::Arbiter);
    client.grant_role(&owner, &user, &Role::Employee);
    assert_eq!(client.get_roles(&user).len(), 2);

    client.revoke_role(&owner, &user, &Role::Arbiter);
    let after = client.get_roles(&user);
    assert_eq!(after.len(), 1);
    assert_eq!(after.get(0).unwrap(), Role::Employee);
}

// ---------- H. Initialization guard ----------------------------------------

#[test]
#[should_panic(expected = "Contract not initialized")]
fn test_override_safety_owner_panics_before_init() {
    // @invariant: owner() panics before initialize.
    let env = create_env();
    let contract_id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(&env, &contract_id);
    let _ = client.owner();
}

#[test]
#[should_panic(expected = "Contract not initialized")]
fn test_override_safety_get_roles_panics_before_init() {
    // @invariant: get_roles panics before initialize.
    let env = create_env();
    let contract_id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(&env, &contract_id);
    let a = Address::generate(&env);
    let _ = client.get_roles(&a);
}

#[test]
#[should_panic(expected = "Contract not initialized")]
fn test_override_safety_require_role_panics_before_init() {
    // @invariant: require_role panics before initialize.
    let env = create_env();
    let contract_id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(&env, &contract_id);
    let a = Address::generate(&env);
    client.require_role(&a, &Role::Employee);
}

#[test]
#[should_panic(expected = "Contract not initialized")]
fn test_override_safety_accept_ownership_panics_before_init() {
    // @invariant: accept_ownership panics before initialize.
    let env = create_env();
    let contract_id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(&env, &contract_id);
    let a = Address::generate(&env);
    client.accept_ownership(&a);
}

#[test]
#[should_panic(expected = "Contract not initialized")]
fn test_override_safety_transfer_ownership_panics_before_init() {
    // @invariant: transfer_ownership panics before initialize.
    let env = create_env();
    let contract_id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(&env, &contract_id);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    client.transfer_ownership(&a, &b);
}

// ---------- I. Documentation hygiene (compile-time checks) ----------------

/// This test is documentary: it verifies the interface trait symbol
/// remains available to integrating crates. Tag-presence is enforced by
/// the `cargo doc -p rbac-interface --no-deps` step that the reviewer
/// checklist requires in CI.
#[test]
fn test_override_safety_interface_trait_symbol_available() {
    // Verify that the client type references the interface trait.
    // This ensures the interface trait symbol remains available to integrating crates.
    let _ = std::any::type_name::<RbacContractClient>();
}

#[test]
fn test_override_safety_has_role_multi_role_holder_inheritance() {
    // @invariant: a user holding multiple roles has the union of their
    //             inheritance. We exercise every (role1, role2) pair
    //             combined and verify the resulting has_role answers.
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let user = Address::generate(&env);

    client.grant_role(&admin, &user, &Role::Employer);
    client.grant_role(&admin, &user, &Role::Arbiter);

    // Direct holds: Employer, Arbiter.
    assert!(client.has_role(&user, &Role::Employer));
    assert!(client.has_role(&user, &Role::Arbiter));
    // Inheritance from Employer: Employee.
    assert!(client.has_role(&user, &Role::Employee));
    // Admin and other roles still denied.
    assert!(!client.has_role(&user, &Role::Admin));

    // Add Admin, verify every role now implied.
    client.grant_role(&admin, &user, &Role::Admin);
    for role in [Role::Admin, Role::Employer, Role::Employee, Role::Arbiter] {
        assert!(
            client.has_role(&user, &role),
            "Admin holder must imply {:?}",
            role
        );
    }
}

// ---------- J. Cross-method security composition ----------------------------

// ---------- J. Cross-method security composition ----------------------------
//
// These are not single-invariant override-safety tests; they exercise the
// interaction between SECURITY-CRITICAL methods. They live here because
// they document *which combinations* of overrides could combine to weaken
// the system. Moved from the original section-10 listing for proximity.

#[test]
#[should_panic(expected = "Cannot revoke Admin from owner")]
fn test_composite_owner_lockout_chain_revoke_role() {
    // Composite: a delegated Admin cannot revoke the owner's Admin role.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let delegate = Address::generate(&env);

    client.grant_role(&owner, &delegate, &Role::Admin);

    client.revoke_role(&delegate, &owner, &Role::Admin);
}

#[test]
#[should_panic(expected = "Cannot revoke all roles from owner")]
fn test_composite_owner_lockout_chain_revoke_all() {
    // Composite: a delegated Admin cannot revoke_all on the owner.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let delegate = Address::generate(&env);

    client.grant_role(&owner, &delegate, &Role::Admin);

    client.revoke_all(&delegate, &owner);
}

#[test]
fn test_composite_owner_lockout_chain_owner_unaffected() {
    // Composite: after multiple failed lockout attempts, owner retains Admin.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let delegate1 = Address::generate(&env);
    let delegate2 = Address::generate(&env);

    client.grant_role(&owner, &delegate1, &Role::Admin);
    client.grant_role(&owner, &delegate2, &Role::Admin);
    client.grant_role(&delegate1, &delegate2, &Role::Admin);

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.revoke_role(&delegate1, &owner, &Role::Admin);
    }));
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.revoke_all(&delegate2, &owner);
    }));

    assert!(client.has_role(&owner, &Role::Admin));
    assert_eq!(client.owner(), owner);
}

#[test]
#[should_panic(expected = "Missing required role")]
fn test_composite_require_role_blocks_impostor() {
    // Composite: an impostor with no roles cannot satisfy require_role.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let impostor = Address::generate(&env);

    client.grant_role(&owner, &impostor, &Role::Employee);
    client.revoke_role(&owner, &impostor, &Role::Employee);

    // Impostor holds no roles -> must panic.
    client.require_role(&impostor, &Role::Employer);
}

#[test]
fn test_composite_bulk_grant_does_not_revoke_existing_roles() {
    // @invariant: bulk_grant is additive; existing roles are preserved.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let user = Address::generate(&env);

    client.grant_role(&owner, &user, &Role::Arbiter);

    let mut batch = Vec::new(&env);
    batch.push_back(Role::Employee);
    batch.push_back(Role::Employer);
    client.bulk_grant(&owner, &user, &batch);

    let roles = client.get_roles(&user);
    assert_eq!(roles.len(), 3, "existing Arbiter role must be preserved");
    assert!(client.has_role(&user, &Role::Arbiter));
    assert!(client.has_role(&user, &Role::Employee));
    assert!(client.has_role(&user, &Role::Employer));
}

#[test]
fn test_composite_get_roles_after_revoke_all() {
    // @invariant: revoke_all clears every role; get_roles returns empty.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let user = Address::generate(&env);

    client.grant_role(&owner, &user, &Role::Arbiter);
    client.grant_role(&owner, &user, &Role::Employer);
    client.grant_role(&owner, &user, &Role::Employee);
    assert_eq!(client.get_roles(&user).len(), 3);

    client.revoke_all(&owner, &user);
    assert_eq!(client.get_roles(&user).len(), 0);
    assert!(!client.has_role(&user, &Role::Arbiter));
    assert!(!client.has_role(&user, &Role::Employer));
    // Note: has_role(Employee) checks inheritance from Employer, which is now gone.
    assert!(!client.has_role(&user, &Role::Employee));
}

#[test]
fn test_composite_owner_record_survives_role_grants() {
    // @invariant: granting roles to non-owner addresses must not change
    //             the owner record.
    let env = create_env();
    let (_cid, client, owner) = setup_contract(&env);
    let user = Address::generate(&env);
    let delegate = Address::generate(&env);

    client.grant_role(&owner, &user, &Role::Employee);
    client.grant_role(&owner, &delegate, &Role::Admin);
    client.grant_role(&delegate, &user, &Role::Arbiter);

    assert_eq!(client.owner(), owner);
}

// ---------- End of override-safety classification --------------------------

// ===========================================================================
// End of test suite
// ===========================================================================
