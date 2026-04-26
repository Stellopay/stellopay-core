#![cfg(test)]

use soroban_sdk::{
    testutils::Address as _,
    Address, Env,
};

use employee_roles::{EmployeeRolesContract, EmployeeRolesContractClient, BuiltInRole, PayrollAction};
use rbac::{RbacContract, RbacContractClient, Role};

/// Creates a test environment with all auths mocked.
fn env() -> Env {
    let e = Env::default();
    e.mock_all_auths();
    e
}

/// Generates a fresh test address.
fn addr(env: &Env) -> Address {
    Address::generate(env)
}

/// Deploys and initializes the RbacContract.
fn deploy_rbac(env: &Env) -> (Address, RbacContractClient<'_>) {
    let id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(env, &id);
    let owner = addr(env);
    client.initialize(&owner);
    (id, client)
}

/// Deploys and initializes the EmployeeRolesContract.
fn deploy_employee_roles(env: &Env) -> (Address, EmployeeRolesContractClient<'_>) {
    let id = env.register_contract(None, EmployeeRolesContract);
    let client = EmployeeRolesContractClient::new(env, &id);
    let owner = addr(env);
    client.initialize(&owner);
    (id, client)
}

// Refined helpers for better control
fn setup(env: &Env) -> (RbacContractClient<'_>, EmployeeRolesContractClient<'_>, Address, Address) {
    let rbac_id = env.register_contract(None, RbacContract);
    let rbac_client = RbacContractClient::new(env, &rbac_id);
    let rbac_owner = addr(env);
    rbac_client.initialize(&rbac_owner);
    
    let er_id = env.register_contract(None, EmployeeRolesContract);
    let er_client = EmployeeRolesContractClient::new(env, &er_id);
    let er_owner = addr(env);
    er_client.initialize(&er_owner);
    
    (rbac_client, er_client, rbac_owner, er_owner)
}

#[test]
fn test_rbac_integration_immediate_effect() {
    let env = env();
    let (rbac_client, er_client, rbac_owner, er_owner) = setup(&env);
    
    let user = addr(&env);
    
    // Link ER to RBAC
    er_client.set_rbac_address(&rbac_client.address);
    
    // Initially, user has no roles
    assert!(!er_client.can_perform(&user, &PayrollAction::ClaimOwnPayroll));
    assert!(!er_client.can_perform(&user, &PayrollAction::CreatePayrollRecord));
    assert!(!er_client.can_perform(&user, &PayrollAction::AssignRoles));
    
    // 1. Grant Employee in RBAC
    rbac_client.grant_role(&rbac_owner, &user, &Role::Employee);
    
    // Verify immediate effect in EmployeeRoles
    assert!(er_client.can_perform(&user, &PayrollAction::ClaimOwnPayroll));
    assert!(!er_client.can_perform(&user, &PayrollAction::CreatePayrollRecord));
    
    // 2. Grant Employer in RBAC (implies Employee)
    let user2 = addr(&env);
    rbac_client.grant_role(&rbac_owner, &user2, &Role::Employer);
    
    assert!(er_client.can_perform(&user2, &PayrollAction::ClaimOwnPayroll));
    assert!(er_client.can_perform(&user2, &PayrollAction::CreatePayrollRecord));
    assert!(!er_client.can_perform(&user2, &PayrollAction::AssignRoles));
    
    // 3. Grant Admin in RBAC (implies all)
    let user3 = addr(&env);
    rbac_client.grant_role(&rbac_owner, &user3, &Role::Admin);
    
    assert!(er_client.can_perform(&user3, &PayrollAction::ClaimOwnPayroll));
    assert!(er_client.can_perform(&user3, &PayrollAction::CreatePayrollRecord));
    assert!(er_client.can_perform(&user3, &PayrollAction::AssignRoles));
    
    // 4. Revoke role in RBAC
    rbac_client.revoke_role(&rbac_owner, &user, &Role::Employee);
    assert!(!er_client.can_perform(&user, &PayrollAction::ClaimOwnPayroll));
}

#[test]
fn test_rbac_hierarchy_inheritance() {
    let env = env();
    let (rbac_client, er_client, rbac_owner, _er_owner) = setup(&env);
    let user = addr(&env);
    
    er_client.set_rbac_address(&rbac_client.address);
    
    // Admin in RBAC should satisfy all checks in EmployeeRoles
    rbac_client.grant_role(&rbac_owner, &user, &Role::Admin);
    
    assert!(er_client.has_role_at_least(&user, &BuiltInRole::Employee));
    assert!(er_client.has_role_at_least(&user, &BuiltInRole::Manager));
    assert!(er_client.has_role_at_least(&user, &BuiltInRole::Admin));
    
    assert!(er_client.can_perform(&user, &PayrollAction::ClaimOwnPayroll));
    assert!(er_client.can_perform(&user, &PayrollAction::CreatePayrollRecord));
    assert!(er_client.can_perform(&user, &PayrollAction::AssignRoles));
}

#[test]
fn test_local_role_override() {
    let env = env();
    let (rbac_client, er_client, _rbac_owner, er_owner) = setup(&env);
    let user = addr(&env);
    
    er_client.set_rbac_address(&rbac_client.address);
    
    // User has no role in RBAC, but has role locally in ER
    er_client.assign_role(&er_owner, &user, &BuiltInRole::Manager);
    
    assert!(er_client.can_perform(&user, &PayrollAction::CreatePayrollRecord));
    assert!(er_client.has_role(&user, &BuiltInRole::Manager));
}

#[test]
fn test_unlinked_behavior() {
    let env = env();
    let (rbac_client, er_client, rbac_owner, _er_owner) = setup(&env);
    let user = addr(&env);
    
    // RBAC has the role, but ER is NOT linked yet
    rbac_client.grant_role(&rbac_owner, &user, &Role::Admin);
    
    assert!(!er_client.can_perform(&user, &PayrollAction::ClaimOwnPayroll));
    
    // Now link it
    er_client.set_rbac_address(&rbac_client.address);
    assert!(er_client.can_perform(&user, &PayrollAction::ClaimOwnPayroll));
}
