#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{testutils::Address as _, Address, Env};

use rbac::{RbacContract, RbacContractClient, Role};

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
fn test_admin_can_grant_and_revoke_roles() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let user = Address::generate(&env);

    // Grant Employer and Arbiter roles.
    client.grant_role(&admin, &user, &Role::Employer);
    client.grant_role(&admin, &user, &Role::Arbiter);

    let roles = client.get_roles(&user);
    assert_eq!(roles.len(), 2);
    assert!(client.has_role(&user, &Role::Employer));
    assert!(client.has_role(&user, &Role::Arbiter));

    // Revoke Arbiter.
    client.revoke_role(&admin, &user, &Role::Arbiter);
    let roles_after = client.get_roles(&user);
    assert_eq!(roles_after.len(), 1);
    assert_eq!(roles_after.get(0).unwrap(), Role::Employer);
    assert!(!client.has_role(&user, &Role::Arbiter));
}

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
#[should_panic(expected = "Only admin can revoke roles")]
fn test_non_admin_cannot_revoke_roles() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let non_admin = Address::generate(&env);
    let user = Address::generate(&env);

    // Admin grants a role first.
    client.grant_role(&admin, &user, &Role::Employee);

    // Non-admin tries to revoke.
    client.revoke_role(&non_admin, &user, &Role::Employee);
}

#[test]
fn test_role_inheritance_admin_implies_all() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);

    assert!(client.has_role(&admin, &Role::Admin));
    assert!(client.has_role(&admin, &Role::Employer));
    assert!(client.has_role(&admin, &Role::Employee));
    assert!(client.has_role(&admin, &Role::Arbiter));
}

#[test]
fn test_role_inheritance_employer_implies_employee() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let employer = Address::generate(&env);

    client.grant_role(&admin, &employer, &Role::Employer);

    assert!(client.has_role(&employer, &Role::Employer));
    assert!(client.has_role(&employer, &Role::Employee));
    assert!(!client.has_role(&employer, &Role::Arbiter));
}

#[test]
fn test_role_inheritance_employee_only_employee() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let employee = Address::generate(&env);

    client.grant_role(&admin, &employee, &Role::Employee);

    assert!(client.has_role(&employee, &Role::Employee));
    assert!(!client.has_role(&employee, &Role::Employer));
    assert!(!client.has_role(&employee, &Role::Arbiter));
}

#[test]
fn test_require_role_enforces_auth_and_role() {
    let env = create_env();
    let (_cid, client, admin) = setup_contract(&env);
    let arbiter = Address::generate(&env);

    client.grant_role(&admin, &arbiter, &Role::Arbiter);

    // Should not panic when role is present.
    client.require_role(&arbiter, &Role::Arbiter);
}

#[test]
#[should_panic(expected = "Missing required role")]
fn test_require_role_panics_when_missing() {
    let env = create_env();
    let (_cid, client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);

    client.require_role(&user, &Role::Employer);
}

