//! Comprehensive tests for Department Manager Contract (Issue #326).
//!
//! Covers:
//! - Initialization (once, twice-fails)
//! - Organization creation (sequential IDs, retrieval)
//! - Department creation (top-level, nested hierarchy, 3-level deep, sequential IDs)
//! - Employee assignment (single, multiple, reassignment, cross-org)
//! - Employee removal (public remove_employee_from_department)
//! - Reporting (get_department_report, get_child_departments, get_org_departments)
//! - Access control (non-owner attempts all fail)
//! - Edge cases (uninitialized contract, bad IDs, dept in wrong org, parent in wrong org)

#![cfg(test)]
#![allow(deprecated)]

use department_manager::{
    Department, DepartmentManagerContract, DepartmentManagerContractClient, Organization,
};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Registers and initializes the contract, returns (contract_id, client).
fn setup_contract(env: &Env) -> (Address, DepartmentManagerContractClient<'_>) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, DepartmentManagerContract);
    let client = DepartmentManagerContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (contract_id, client)
}

// ---------------------------------------------------------------------------
// Initialization tests
// ---------------------------------------------------------------------------

#[test]
fn test_initialize() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    // If initialized, creating an org should work and return ID = 1.
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
#[should_panic(expected = "Contract not initialized")]
fn test_create_org_before_init_fails() {
    let env = create_env();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, DepartmentManagerContract);
    let client = DepartmentManagerContractClient::new(&env, &contract_id);
    // Never called initialize
    client.create_organization(&Address::generate(&env), &symbol_short!("Acme"));
}

#[test]
#[should_panic(expected = "Contract not initialized")]
fn test_create_dept_before_init_fails() {
    let env = create_env();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, DepartmentManagerContract);
    let client = DepartmentManagerContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    client.create_department(&owner, &1u128, &symbol_short!("Eng"), &None);
}

// ---------------------------------------------------------------------------
// Organization tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_organization_fields() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let name = symbol_short!("Acme");
    let org_id = client.create_organization(&owner, &name);
    assert_eq!(org_id, 1);
    let org: Organization = client.get_organization(&org_id);
    assert_eq!(org.id, 1);
    assert_eq!(org.owner, owner);
    assert_eq!(org.name, name);
}

#[test]
fn test_multiple_organizations_sequential_ids() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner1 = Address::generate(&env);
    let owner2 = Address::generate(&env);
    let id1 = client.create_organization(&owner1, &symbol_short!("OrgA"));
    let id2 = client.create_organization(&owner2, &symbol_short!("OrgB"));
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    // Each org has its own owner
    let org1: Organization = client.get_organization(&id1);
    let org2: Organization = client.get_organization(&id2);
    assert_eq!(org1.owner, owner1);
    assert_eq!(org2.owner, owner2);
}

#[test]
#[should_panic(expected = "Organization not found")]
fn test_get_organization_not_found() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let _ = client.get_organization(&999u128);
}

#[test]
fn test_org_departments_initially_empty() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let org_id = client.create_organization(&Address::generate(&env), &symbol_short!("Acme"));
    let depts = client.get_org_departments(&org_id);
    assert_eq!(depts.len(), 0);
}

// ---------------------------------------------------------------------------
// Department creation tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_department_top_level() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Engnrng"), &None);
    assert_eq!(dept_id, 1);
    let dept: Department = client.get_department(&dept_id);
    assert_eq!(dept.org_id, org_id);
    assert_eq!(dept.parent_id, None);
    assert_eq!(dept.name, symbol_short!("Engnrng"));
}

#[test]
fn test_departments_sequential_ids() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let d1 = client.create_department(&owner, &org_id, &symbol_short!("A"), &None);
    let d2 = client.create_department(&owner, &org_id, &symbol_short!("B"), &None);
    assert_eq!(d1, 1);
    assert_eq!(d2, 2);
}

#[test]
fn test_create_department_hierarchy_two_levels() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let eng_id = client.create_department(&owner, &org_id, &symbol_short!("Engnrng"), &None);
    let backend_id =
        client.create_department(&owner, &org_id, &symbol_short!("Backend"), &Some(eng_id));
    let dept: Department = client.get_department(&backend_id);
    assert_eq!(dept.parent_id, Some(eng_id));

    let (count, children, _emp) = client.get_department_report(&eng_id);
    assert_eq!(count, 0);
    assert_eq!(children.len(), 1);
    assert_eq!(children.get(0), Some(backend_id));
}

#[test]
fn test_create_department_hierarchy_three_levels() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("MegaCorp"));
    // Level 1
    let eng_id = client.create_department(&owner, &org_id, &symbol_short!("Engnrng"), &None);
    // Level 2
    let backend_id =
        client.create_department(&owner, &org_id, &symbol_short!("Backend"), &Some(eng_id));
    // Level 3
    let rust_id =
        client.create_department(&owner, &org_id, &symbol_short!("Rust"), &Some(backend_id));

    let d = client.get_department(&rust_id);
    assert_eq!(d.parent_id, Some(backend_id));
    assert_eq!(d.org_id, org_id);

    // backend has rust as child
    let children_of_backend = client.get_child_departments(&backend_id);
    assert_eq!(children_of_backend.len(), 1);
    assert_eq!(children_of_backend.get(0), Some(rust_id));

    // eng has backend as child only (rust is not a direct child of eng)
    let children_of_eng = client.get_child_departments(&eng_id);
    assert_eq!(children_of_eng.len(), 1);
    assert_eq!(children_of_eng.get(0), Some(backend_id));
}

#[test]
fn test_multiple_departments_returned_by_get_org_departments() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let d1 = client.create_department(&owner, &org_id, &symbol_short!("A"), &None);
    let d2 = client.create_department(&owner, &org_id, &symbol_short!("B"), &None);
    let d3 = client.create_department(&owner, &org_id, &symbol_short!("C"), &Some(d1));
    let depts = client.get_org_departments(&org_id);
    assert_eq!(depts.len(), 3);
    assert_eq!(depts.get(0), Some(d1));
    assert_eq!(depts.get(1), Some(d2));
    assert_eq!(depts.get(2), Some(d3));
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
fn test_create_department_bad_org_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    client.create_department(&owner, &999u128, &symbol_short!("Eng"), &None);
}

#[test]
#[should_panic(expected = "Parent department not found")]
fn test_create_department_bad_parent_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    client.create_department(&owner, &org_id, &symbol_short!("Eng"), &Some(999u128));
}

#[test]
#[should_panic(expected = "Parent must be in same org")]
fn test_create_department_parent_in_wrong_org_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org1 = client.create_organization(&owner, &symbol_short!("OrgA"));
    let org2 = client.create_organization(&owner, &symbol_short!("OrgB"));
    let dept_in_org1 = client.create_department(&owner, &org1, &symbol_short!("Eng"), &None);
    // Try to use a dept from org1 as parent for a dept in org2
    client.create_department(&owner, &org2, &symbol_short!("Dev"), &Some(dept_in_org1));
}

#[test]
#[should_panic(expected = "Department not found")]
fn test_get_department_not_found() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let _ = client.get_department(&999u128);
}

// ---------------------------------------------------------------------------
// Employee assignment tests
// ---------------------------------------------------------------------------

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
    assert_eq!(employees.get(0), Some(emp.clone()));

    let emp_dept = client.get_employee_department(&emp, &org_id);
    assert_eq!(emp_dept, Some(dept_id));

    let (count, _children, addrs) = client.get_department_report(&dept_id);
    assert_eq!(count, 1);
    assert_eq!(addrs.len(), 1);
    assert_eq!(addrs.get(0), Some(emp));
}

#[test]
fn test_multiple_employees_in_department() {
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

    let employees = client.get_department_employees(&dept_id);
    assert_eq!(employees.len(), 3);

    let (count, _, _) = client.get_department_report(&dept_id);
    assert_eq!(count, 3);
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
    assert_eq!(client.get_department_employees(&dept_a).len(), 1);

    // Re-assign to dept_b
    client.assign_employee_to_department(&owner, &org_id, &dept_b, &emp);
    assert_eq!(client.get_employee_department(&emp, &org_id), Some(dept_b));
    // Removed from dept_a
    assert_eq!(client.get_department_employees(&dept_a).len(), 0);
    // Now in dept_b
    assert_eq!(client.get_department_employees(&dept_b).len(), 1);
}

#[test]
fn test_employee_assignment_across_two_orgs_independent() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org1 = client.create_organization(&owner, &symbol_short!("OrgA"));
    let org2 = client.create_organization(&owner, &symbol_short!("OrgB"));
    let d1 = client.create_department(&owner, &org1, &symbol_short!("Eng"), &None);
    let d2 = client.create_department(&owner, &org2, &symbol_short!("Mktg"), &None);
    let emp = Address::generate(&env);

    // Same employee can be in different departments in different orgs independently
    client.assign_employee_to_department(&owner, &org1, &d1, &emp);
    client.assign_employee_to_department(&owner, &org2, &d2, &emp);

    assert_eq!(client.get_employee_department(&emp, &org1), Some(d1));
    assert_eq!(client.get_employee_department(&emp, &org2), Some(d2));
    // Assigning in org2 does NOT remove from org1
    assert_eq!(client.get_department_employees(&d1).len(), 1);
    assert_eq!(client.get_department_employees(&d2).len(), 1);
}

#[test]
fn test_employee_department_none_when_not_assigned() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let (_cid2, _) = setup_contract(&env); // unused but tests multiple setups
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let emp = Address::generate(&env);
    // Employee was never assigned
    assert_eq!(client.get_employee_department(&emp, &org_id), None);
}

#[test]
#[should_panic(expected = "Not organization owner")]
fn test_assign_employee_non_owner_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let other = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);
    let emp = Address::generate(&env);
    client.assign_employee_to_department(&other, &org_id, &dept_id, &emp);
}

#[test]
#[should_panic(expected = "Department not in this org")]
fn test_assign_employee_dept_in_wrong_org_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org1 = client.create_organization(&owner, &symbol_short!("OrgA"));
    let org2 = client.create_organization(&owner, &symbol_short!("OrgB"));
    let dept_in_org1 = client.create_department(&owner, &org1, &symbol_short!("Eng"), &None);
    let emp = Address::generate(&env);
    // Trying to assign using org2 but dept belongs to org1
    client.assign_employee_to_department(&owner, &org2, &dept_in_org1, &emp);
}

// ---------------------------------------------------------------------------
// Employee removal tests
// ---------------------------------------------------------------------------

#[test]
fn test_remove_employee_from_department() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);
    let emp = Address::generate(&env);

    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp);
    assert_eq!(client.get_department_employees(&dept_id).len(), 1);
    assert_eq!(client.get_employee_department(&emp, &org_id), Some(dept_id));

    // Remove the employee
    client.remove_employee_from_department(&owner, &org_id, &emp);

    assert_eq!(client.get_department_employees(&dept_id).len(), 0);
    assert_eq!(client.get_employee_department(&emp, &org_id), None);
}

#[test]
fn test_remove_one_leaves_others() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);
    let emp1 = Address::generate(&env);
    let emp2 = Address::generate(&env);

    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp1);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp2);
    assert_eq!(client.get_department_employees(&dept_id).len(), 2);

    client.remove_employee_from_department(&owner, &org_id, &emp1);
    let remaining = client.get_department_employees(&dept_id);
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining.get(0), Some(emp2.clone()));
    assert_eq!(client.get_employee_department(&emp2, &org_id), Some(dept_id));
}

#[test]
#[should_panic(expected = "Not organization owner")]
fn test_remove_employee_non_owner_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let other = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);
    let emp = Address::generate(&env);
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp);
    client.remove_employee_from_department(&other, &org_id, &emp);
}

#[test]
#[should_panic(expected = "Employee not assigned in this org")]
fn test_remove_unassigned_employee_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let emp = Address::generate(&env);
    // emp was never assigned
    client.remove_employee_from_department(&owner, &org_id, &emp);
}

// ---------------------------------------------------------------------------
// Reporting tests
// ---------------------------------------------------------------------------

#[test]
fn test_get_department_report_with_children_and_employees() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));
    let parent = client.create_department(&owner, &org_id, &symbol_short!("Tech"), &None);
    let child1 = client.create_department(&owner, &org_id, &symbol_short!("Web"), &Some(parent));
    let child2 = client.create_department(&owner, &org_id, &symbol_short!("Mobile"), &Some(parent));
    let emp1 = Address::generate(&env);
    let emp2 = Address::generate(&env);
    client.assign_employee_to_department(&owner, &org_id, &parent, &emp1);
    client.assign_employee_to_department(&owner, &org_id, &parent, &emp2);

    let (count, children, addrs) = client.get_department_report(&parent);
    assert_eq!(count, 2);
    assert_eq!(children.len(), 2);
    assert_eq!(addrs.len(), 2);
    // Children are stored in insertion order: child1 first, child2 second
    assert_eq!(children.get(0), Some(child1));
    assert_eq!(children.get(1), Some(child2));
}

#[test]
fn test_get_child_departments_empty_for_leaf() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);
    // No children created
    let children = client.get_child_departments(&dept_id);
    assert_eq!(children.len(), 0);
}

#[test]
fn test_get_department_employees_empty_initial() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);
    let employees = client.get_department_employees(&dept_id);
    assert_eq!(employees.len(), 0);
}

// ---------------------------------------------------------------------------
// Hierarchical constraint tests
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "Max hierarchy depth exceeded")]
fn test_depth_limit_enforced() {
    use department_manager::MAX_DEPTH;
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Deep"));
    // Build a chain of MAX_DEPTH+1 departments (depths 0..MAX_DEPTH are valid)
    let mut parent: Option<u128> = None;
    for _ in 0..=MAX_DEPTH {
        let id = client.create_department(&owner, &org_id, &symbol_short!("D"), &parent);
        parent = Some(id);
    }
    // This one would be at depth MAX_DEPTH+1 — must panic
    client.create_department(&owner, &org_id, &symbol_short!("D"), &parent);
}

#[test]
fn test_depth_limit_boundary_ok() {
    use department_manager::MAX_DEPTH;
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Deep"));
    let mut parent: Option<u128> = None;
    // MAX_DEPTH+1 departments: depths 0..MAX_DEPTH (all valid)
    for _ in 0..=MAX_DEPTH {
        let id = client.create_department(&owner, &org_id, &symbol_short!("D"), &parent);
        parent = Some(id);
    }
    // Verify the last created dept exists
    let last_id = parent.unwrap();
    let dept = client.get_department(&last_id);
    assert_eq!(dept.org_id, org_id);
}

// ---------------------------------------------------------------------------
// update_department (reparent) tests
// ---------------------------------------------------------------------------

#[test]
fn test_reparent_department() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));
    let a = client.create_department(&owner, &org_id, &symbol_short!("A"), &None);
    let b = client.create_department(&owner, &org_id, &symbol_short!("B"), &None);
    let c = client.create_department(&owner, &org_id, &symbol_short!("C"), &Some(a));

    // Move C from under A to under B
    client.update_department(&owner, &c, &Some(b));

    let dept_c = client.get_department(&c);
    assert_eq!(dept_c.parent_id, Some(b));
    // A no longer has C as child
    assert_eq!(client.get_child_departments(&a).len(), 0);
    // B now has C as child
    let b_children = client.get_child_departments(&b);
    assert_eq!(b_children.len(), 1);
    assert_eq!(b_children.get(0), Some(c));
}

#[test]
fn test_reparent_to_top_level() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));
    let a = client.create_department(&owner, &org_id, &symbol_short!("A"), &None);
    let b = client.create_department(&owner, &org_id, &symbol_short!("B"), &Some(a));

    client.update_department(&owner, &b, &None);

    let dept_b = client.get_department(&b);
    assert_eq!(dept_b.parent_id, None);
    assert_eq!(client.get_child_departments(&a).len(), 0);
}

#[test]
#[should_panic(expected = "Cycle detected")]
fn test_reparent_direct_cycle_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));
    let a = client.create_department(&owner, &org_id, &symbol_short!("A"), &None);
    let b = client.create_department(&owner, &org_id, &symbol_short!("B"), &Some(a));
    // A -> B exists; making A a child of B would create A -> B -> A
    client.update_department(&owner, &a, &Some(b));
}

#[test]
#[should_panic(expected = "Cycle detected")]
fn test_reparent_indirect_cycle_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));
    let a = client.create_department(&owner, &org_id, &symbol_short!("A"), &None);
    let b = client.create_department(&owner, &org_id, &symbol_short!("B"), &Some(a));
    let c = client.create_department(&owner, &org_id, &symbol_short!("C"), &Some(b));
    // Chain: A -> B -> C; making A a child of C would create A -> B -> C -> A
    client.update_department(&owner, &a, &Some(c));
}

#[test]
#[should_panic(expected = "Cycle detected")]
fn test_reparent_self_cycle_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));
    let a = client.create_department(&owner, &org_id, &symbol_short!("A"), &None);
    // A cannot be its own parent
    client.update_department(&owner, &a, &Some(a));
}

#[test]
#[should_panic(expected = "Not organization owner")]
fn test_reparent_non_owner_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let other = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));
    let a = client.create_department(&owner, &org_id, &symbol_short!("A"), &None);
    let b = client.create_department(&owner, &org_id, &symbol_short!("B"), &None);
    client.update_department(&other, &a, &Some(b));
}

#[test]
#[should_panic(expected = "Max hierarchy depth exceeded")]
fn test_reparent_exceeds_depth_fails() {
    use department_manager::MAX_DEPTH;
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));
    // Build a chain of MAX_DEPTH+1 depts (depths 0..MAX_DEPTH — all valid)
    let mut parent: Option<u128> = None;
    let mut last = 0u128;
    for _ in 0..=MAX_DEPTH {
        last = client.create_department(&owner, &org_id, &symbol_short!("D"), &parent);
        parent = Some(last);
    }
    // Create a standalone dept and try to attach it under `last` (depth MAX_DEPTH)
    // That would place standalone at depth MAX_DEPTH+1 — must panic
    let standalone = client.create_department(&owner, &org_id, &symbol_short!("S"), &None);
    client.update_department(&owner, &standalone, &Some(last));
}

// ---------------------------------------------------------------------------
// Property / fuzz-style tests
// ---------------------------------------------------------------------------

/// Property: a linear chain of N departments always has correct parent links.
#[test]
fn prop_linear_chain_parent_links() {
    use department_manager::MAX_DEPTH;
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Prop"));

    // Build the maximum valid chain: MAX_DEPTH+1 nodes (depths 0..MAX_DEPTH)
    let mut ids: soroban_sdk::Vec<u128> = soroban_sdk::Vec::new(&env);
    let mut parent: Option<u128> = None;
    for _ in 0..=MAX_DEPTH {
        let id = client.create_department(&owner, &org_id, &symbol_short!("D"), &parent);
        ids.push_back(id);
        parent = Some(id);
    }

    // Verify each dept's parent_id matches the previous dept
    for i in 0..ids.len() {
        let id = ids.get(i).unwrap();
        let dept = client.get_department(&id);
        if i == 0 {
            assert_eq!(dept.parent_id, None);
        } else {
            assert_eq!(dept.parent_id, Some(ids.get(i - 1).unwrap()));
        }
    }
}

/// Property: after a series of reparent operations the tree remains acyclic.
/// Simulates a sequence of valid moves and verifies no cycle is introduced.
#[test]
fn prop_reparent_sequence_no_cycle() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Prop"));

    // Create 5 top-level departments
    let a = client.create_department(&owner, &org_id, &symbol_short!("A"), &None);
    let b = client.create_department(&owner, &org_id, &symbol_short!("B"), &None);
    let c = client.create_department(&owner, &org_id, &symbol_short!("C"), &None);
    let d = client.create_department(&owner, &org_id, &symbol_short!("D"), &None);
    let e = client.create_department(&owner, &org_id, &symbol_short!("E"), &None);

    // Valid reparent sequence: build A -> B -> C -> D -> E
    client.update_department(&owner, &b, &Some(a));
    client.update_department(&owner, &c, &Some(b));
    client.update_department(&owner, &d, &Some(c));
    client.update_department(&owner, &e, &Some(d));

    // Verify the chain
    assert_eq!(client.get_department(&b).parent_id, Some(a));
    assert_eq!(client.get_department(&c).parent_id, Some(b));
    assert_eq!(client.get_department(&d).parent_id, Some(c));
    assert_eq!(client.get_department(&e).parent_id, Some(d));

    // Flatten back: move E to top-level, then D under E
    client.update_department(&owner, &e, &None);
    client.update_department(&owner, &d, &Some(e));

    assert_eq!(client.get_department(&e).parent_id, None);
    assert_eq!(client.get_department(&d).parent_id, Some(e));
    // C no longer has D as child
    assert_eq!(client.get_child_departments(&c).len(), 0);
}

/// Property: all attempted cycle-creating reparents are rejected.
/// Exhaustively tries to create cycles in a 4-node chain.
#[test]
fn prop_all_cycle_attempts_rejected() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Prop"));

    // Build chain: root -> n1 -> n2 -> n3
    let root = client.create_department(&owner, &org_id, &symbol_short!("R"), &None);
    let n1 = client.create_department(&owner, &org_id, &symbol_short!("N1"), &Some(root));
    let n2 = client.create_department(&owner, &org_id, &symbol_short!("N2"), &Some(n1));
    let n3 = client.create_department(&owner, &org_id, &symbol_short!("N3"), &Some(n2));

    // Each of these would create a cycle; verify they all panic
    let cycle_attempts: &[(u128, u128)] = &[
        (root, n1), // root -> n1 -> root
        (root, n2), // root -> n1 -> n2 -> root
        (root, n3), // root -> n1 -> n2 -> n3 -> root
        (n1, n2),   // n1 -> n2 -> n1
        (n1, n3),   // n1 -> n2 -> n3 -> n1
        (n2, n3),   // n2 -> n3 -> n2
    ];

    for &(ancestor, descendant) in cycle_attempts {
        // We need a fresh env per attempt since panics unwind the test
        let env2 = create_env();
        let (_cid2, client2) = setup_contract(&env2);
        let owner2 = Address::generate(&env2);
        let org2 = client2.create_organization(&owner2, &symbol_short!("P"));
        let r = client2.create_department(&owner2, &org2, &symbol_short!("R"), &None);
        let x1 = client2.create_department(&owner2, &org2, &symbol_short!("N1"), &Some(r));
        let x2 = client2.create_department(&owner2, &org2, &symbol_short!("N2"), &Some(x1));
        let x3 = client2.create_department(&owner2, &org2, &symbol_short!("N3"), &Some(x2));

        // Map original IDs to new IDs
        let map_id = |id: u128| -> u128 {
            if id == root { r }
            else if id == n1 { x1 }
            else if id == n2 { x2 }
            else { x3 }
        };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client2.update_department(&owner2, &map_id(ancestor), &Some(map_id(descendant)));
        }));
        assert!(
            result.is_err(),
            "Expected cycle detection to panic for ({ancestor}, {descendant})"
        );
    }
}

/// Property: subtree move preserves all descendant relationships.
#[test]
fn prop_subtree_move_preserves_descendants() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Prop"));

    // Tree: root -> [a -> [a1, a2], b]
    let root = client.create_department(&owner, &org_id, &symbol_short!("R"), &None);
    let a = client.create_department(&owner, &org_id, &symbol_short!("A"), &Some(root));
    let a1 = client.create_department(&owner, &org_id, &symbol_short!("A1"), &Some(a));
    let a2 = client.create_department(&owner, &org_id, &symbol_short!("A2"), &Some(a));
    let b = client.create_department(&owner, &org_id, &symbol_short!("B"), &Some(root));

    // Move subtree A (with children a1, a2) under B
    client.update_department(&owner, &a, &Some(b));

    // A is now under B
    assert_eq!(client.get_department(&a).parent_id, Some(b));
    // A's children are unchanged
    let a_children = client.get_child_departments(&a);
    assert_eq!(a_children.len(), 2);
    // root no longer has A as direct child
    let root_children = client.get_child_departments(&root);
    assert_eq!(root_children.len(), 1);
    assert_eq!(root_children.get(0), Some(b));
    // B now has A as child
    let b_children = client.get_child_departments(&b);
    assert_eq!(b_children.len(), 1);
    assert_eq!(b_children.get(0), Some(a));
    // a1 and a2 still point to a
    assert_eq!(client.get_department(&a1).parent_id, Some(a));
    assert_eq!(client.get_department(&a2).parent_id, Some(a));
}
