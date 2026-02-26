#![cfg(test)]

use soroban_sdk::{
    testutils::Address as _,
    Address, Env,
};

use employee_roles::{BuiltInRole, EmployeeRolesContract, EmployeeRolesContractClient};

fn setup() -> (Env, Address, EmployeeRolesContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, EmployeeRolesContract);
    let client = EmployeeRolesContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    (env, owner, client)
}

#[test]
fn test_owner_can_assign_and_revoke_roles() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);

    client.assign_role(&owner, &employee, &BuiltInRole::Manager);
    assert!(client.has_role(&employee, &BuiltInRole::Manager));

    client.revoke_role(&owner, &employee, &BuiltInRole::Manager);
    assert!(!client.has_role(&employee, &BuiltInRole::Manager));
}

#[test]
fn test_admin_can_manage_roles() {
    let (env, owner, client) = setup();

    let admin = Address::generate(&env);
    let employee = Address::generate(&env);

    // Owner grants Admin role.
    client.assign_role(&owner, &admin, &BuiltInRole::Admin);

    // Admin can now assign Manager role to employee.
    client.assign_role(&admin, &employee, &BuiltInRole::Manager);

    assert!(client.has_role(&employee, &BuiltInRole::Manager));
    assert!(client.has_role_at_least(&employee, &BuiltInRole::Employee));
}

#[test]
fn test_hierarchy_admin_satisfies_manager_and_employee() {
    let (env, owner, client) = setup();

    let admin = Address::generate(&env);
    client.assign_role(&owner, &admin, &BuiltInRole::Admin);

    assert!(client.has_role_at_least(&admin, &BuiltInRole::Employee));
    assert!(client.has_role_at_least(&admin, &BuiltInRole::Manager));
    assert!(client.has_role_at_least(&admin, &BuiltInRole::Admin));
}

