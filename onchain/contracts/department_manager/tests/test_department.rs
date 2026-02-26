//! Comprehensive tests for Department/Organization Management (#237).

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};
use department_manager::{
    Department, DepartmentManagerContract, DepartmentManagerContractClient, Organization,
};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_contract(env: &Env) -> (Address, DepartmentManagerContractClient<'_>) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, DepartmentManagerContract);
    let client = DepartmentManagerContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (contract_id, client)
}

#[test]
fn test_initialize() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    // Contract is initialized in setup_contract; get_organization will fail for bad id but init succeeded
    let org_id = client.create_organization(&Address::generate(&env), &symbol_short!("Test"));
    assert_eq!(org_id, 1);
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_initialize_twice_fails() {
    let env = create_env();
    let admin = Address::generate(&env);
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, DepartmentManagerContract);
    let client = DepartmentManagerContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    client.initialize(&admin);
}

#[test]
fn test_create_organization() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let name = symbol_short!("Acme");
    let org_id = client.create_organization(&owner, &name);
    assert_eq!(org_id, 1);
    let org = client.get_organization(&org_id);
    assert_eq!(org.id, 1);
    assert_eq!(org.owner, owner);
    assert_eq!(org.name, name);
}

#[test]
fn test_create_department_top_level() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Engineering"), &None);
    assert_eq!(dept_id, 1);
    let dept = client.get_department(&dept_id);
    assert_eq!(dept.org_id, org_id);
    assert_eq!(dept.parent_id, None);
    assert_eq!(dept.name, symbol_short!("Engineering"));
}

#[test]
fn test_create_department_hierarchy() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let eng_id = client.create_department(&owner, &org_id, &symbol_short!("Engineering"), &None);
    let backend_id =
        client.create_department(&owner, &org_id, &symbol_short!("Backend"), &Some(eng_id));
    let dept = client.get_department(&backend_id);
    assert_eq!(dept.parent_id, Some(eng_id));
    let (len, children, _) = client.get_department_report(&eng_id);
    assert_eq!(len, 0);
    assert_eq!(children.len(), 1);
    assert_eq!(children.get(0), backend_id);
}

#[test]
fn test_assign_employee_and_report() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);
    let emp = Address::generate(&env);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp);

    let employees = client.get_department_employees(&dept_id);
    assert_eq!(employees.len(), 1);
    assert_eq!(employees.get(0), emp);

    let emp_dept = client.get_employee_department(&emp, &org_id);
    assert_eq!(emp_dept, Some(dept_id));

    let (count, children, addrs) = client.get_department_report(&dept_id);
    assert_eq!(count, 1);
    assert_eq!(addrs.len(), 1);
    assert_eq!(addrs.get(0), emp);
}

#[test]
fn test_reassign_employee_to_another_department() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_a = client.create_department(&owner, &org_id, &symbol_short!("A"), &None);
    let dept_b = client.create_department(&owner, &org_id, &symbol_short!("B"), &None);
    let emp = Address::generate(&env);
    client.assign_employee_to_department(&owner, &org_id, &dept_a, &emp);
    assert_eq!(client.get_employee_department(&emp, &org_id), Some(dept_a));
    client.assign_employee_to_department(&owner, &org_id, &dept_b, &emp);
    assert_eq!(client.get_employee_department(&emp, &org_id), Some(dept_b));
    assert_eq!(client.get_department_employees(&dept_a).len(), 0);
    assert_eq!(client.get_department_employees(&dept_b).len(), 1);
}

#[test]
#[should_panic(expected = "Not organization owner")]
fn test_create_department_non_owner_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let other = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    client.create_department(&other, &org_id, &symbol_short!("Eng"), &None);
}

#[test]
#[should_panic(expected = "Organization not found")]
fn test_get_organization_not_found() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let _ = client.get_organization(&999);
}
