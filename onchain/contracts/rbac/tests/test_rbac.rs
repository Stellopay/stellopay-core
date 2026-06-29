#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

use rbac::{RbacContract, RbacContractClient, Role};

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
    assert_eq!(roles.len(), 1, "Duplicate grant should not add a second entry");
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
        [true, true, true, true],     // Admin grants
        [false, true, true, false],   // Employer grants
        [false, false, true, false],  // Employee grants
        [false, false, false, true],  // Arbiter grants
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
