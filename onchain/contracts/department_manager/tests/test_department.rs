//! Comprehensive tests for Department/Organization Management (#237).

#![cfg(test)]
#![allow(deprecated)]

use department_manager::{
    Department, DepartmentManagerContract, DepartmentManagerContractClient, Organization,
};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

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

// ── Paginated get_department_employees_paged tests ────────────────────────────

#[test]
fn test_paged_empty_department() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);

    let (page, next) = client.get_department_employees_paged(&dept_id, &0, &10);
    assert_eq!(page.len(), 0);
    assert_eq!(next, None);
}

#[test]
fn test_paged_single_full_page() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);

    let emp1 = Address::generate(&env);
    let emp2 = Address::generate(&env);
    let emp3 = Address::generate(&env);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp1);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp2);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp3);

    let (page, next) = client.get_department_employees_paged(&dept_id, &0, &10);
    assert_eq!(page.len(), 3);
    assert_eq!(page.get(0), emp1);
    assert_eq!(page.get(1), emp2);
    assert_eq!(page.get(2), emp3);
    assert_eq!(next, None);
}

#[test]
fn test_paged_exact_page_boundary() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);

    let emp1 = Address::generate(&env);
    let emp2 = Address::generate(&env);
    let emp3 = Address::generate(&env);
    let emp4 = Address::generate(&env);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp1);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp2);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp3);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp4);

    // First page of 2
    let (page1, next1) = client.get_department_employees_paged(&dept_id, &0, &2);
    assert_eq!(page1.len(), 2);
    assert_eq!(page1.get(0), emp1);
    assert_eq!(page1.get(1), emp2);
    assert_eq!(next1, Some(2));

    // Second page of 2
    let (page2, next2) = client.get_department_employees_paged(&dept_id, &2, &2);
    assert_eq!(page2.len(), 2);
    assert_eq!(page2.get(0), emp3);
    assert_eq!(page2.get(1), emp4);
    assert_eq!(next2, None);
}

#[test]
fn test_paged_oversized_limit_clamped_to_max() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);

    // Add 3 employees but request 9999 – should still return only 3
    let emp1 = Address::generate(&env);
    let emp2 = Address::generate(&env);
    let emp3 = Address::generate(&env);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp1);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp2);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp3);

    let (page, next) = client.get_department_employees_paged(&dept_id, &0, &9999);
    assert_eq!(page.len(), 3);
    assert_eq!(next, None);
}

#[test]
fn test_paged_start_beyond_total_returns_empty() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);

    let emp = Address::generate(&env);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp);

    let (page, next) = client.get_department_employees_paged(&dept_id, &100, &10);
    assert_eq!(page.len(), 0);
    assert_eq!(next, None);
}

#[test]
fn test_paged_partial_last_page() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);

    let emp1 = Address::generate(&env);
    let emp2 = Address::generate(&env);
    let emp3 = Address::generate(&env);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp1);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp2);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp3);

    // Request page of 2 starting at index 2 – only 1 employee remains
    let (page, next) = client.get_department_employees_paged(&dept_id, &2, &2);
    assert_eq!(page.len(), 1);
    assert_eq!(page.get(0), emp3);
    assert_eq!(next, None);
}

#[test]
fn test_paged_zero_limit_uses_max_page_size() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);

    let emp1 = Address::generate(&env);
    let emp2 = Address::generate(&env);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp1);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp2);

    // limit=0 should default to MAX_PAGE_SIZE (50), returning all 2 employees
    let (page, next) = client.get_department_employees_paged(&dept_id, &0, &0);
    assert_eq!(page.len(), 2);
    assert_eq!(next, None);
}
