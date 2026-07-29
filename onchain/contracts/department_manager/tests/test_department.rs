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
// Unique-organization-id guard tests (Issue #917)
// ---------------------------------------------------------------------------
//
// Verifies the contract enforces that each organization `name` is globally
// unique. Re-creating an organization with a name that is already claimed by
// an existing organization must be rejected, and the rejected attempt must
// leave the original organization's department tree untouched.
//
// NOTE on test style: this section uses BOTH `#[should_panic]` (for tests
// that only need to assert the panic) and `std::panic::catch_unwind` (for
// tests that must continue execution after the panic to verify post-rejection
// state, like the tree-integrity guarantees). The existing suite uses only
// `#[should_panic]`; that pattern cannot assert post-panic storage state, so
// `catch_unwind(safe)` is necessary here.
/// @notice Duplicate name with the **same owner** is rejected. The guard fires
///         even when the same caller reuses a name they already own.
#[test]
#[should_panic(expected = "Organization name already in use")]
fn test_create_organization_rejects_duplicate_name_same_owner() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    client.create_organization(&owner, &symbol_short!("Acme"));
    // Second call with the same name + same owner must panic.
    client.create_organization(&owner, &symbol_short!("Acme"));
}

/// @notice Duplicate name with a **different owner** is also rejected. Names
///         are globally unique across the entire contract, not per-owner.
#[test]
#[should_panic(expected = "Organization name already in use")]
fn test_create_organization_rejects_duplicate_name_different_owner() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner1 = Address::generate(&env);
    let owner2 = Address::generate(&env);
    client.create_organization(&owner1, &symbol_short!("Acme"));
    // Different owner must still be rejected.
    client.create_organization(&owner2, &symbol_short!("Acme"));
}

/// @notice A rejected duplicate-create attempt leaves the original
///         organization's record and its full department tree untouched.
///         This is the security-critical guarantee from issue #917: re-creating
///         an organization could otherwise reset or overwrite its tree.
///
/// Tree built before the duplicate attempt:
/// ```text
///  Acme (root)
///   ├── Eng (no children, no members)
///   └── Sales (no children, no members)
///        └── DirectSales (no children, no members)
/// ```
#[test]
fn test_failed_duplicate_create_leaves_existing_tree_intact() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);

    // 1. Create the original organization and build a department tree.
    let acme_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let eng_id = client.create_department(&owner, &acme_id, &symbol_short!("Eng"), &None);
    let sales_id = client.create_department(&owner, &acme_id, &symbol_short!("Sales"), &None);
    let direct_id =
        client.create_department(&owner, &acme_id, &symbol_short!("Dir"), &Some(sales_id));
    let emp = Address::generate(&env);
    client.assign_employee_to_department(&owner, &acme_id, &sales_id, &emp);

    // Snapshot the original tree before the attempt.
    let before_depts = client.get_org_departments(&acme_id);
    assert_eq!(before_depts.len(), 3, "pre-condition: tree has 3 depts");
    let before_emp_in_sales = client.get_department_employees(&sales_id);
    assert_eq!(
        before_emp_in_sales.len(),
        1,
        "pre-condition: one emp in Sales"
    );

    // 2. Attempt to re-create "Acme" with a different owner — must panic.
    let stranger = Address::generate(&env);
    let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.create_organization(&stranger, &symbol_short!("Acme"));
    }));
    assert!(
        panic_result.is_err(),
        "duplicate organization create must panic"
    );

    // 3. The original organization's record is unchanged.
    let acme_after = client.get_organization(&acme_id);
    assert_eq!(acme_after.id, acme_id);
    assert_eq!(acme_after.name, symbol_short!("Acme"));
    assert_eq!(acme_after.owner, owner);

    // 4. The full department tree (from get_org_departments) is unchanged.
    let after_depts = client.get_org_departments(&acme_id);
    assert_eq!(
        after_depts.len(),
        3,
        "rejected duplicate must not alter the original org's departments"
    );
    assert_eq!(after_depts.get(0), Some(eng_id));
    assert_eq!(after_depts.get(1), Some(sales_id));
    assert_eq!(after_depts.get(2), Some(direct_id));

    // 5. Each department record and its relations are still intact.
    let eng = client.get_department(&eng_id);
    assert_eq!(eng.parent_id, None);
    let sales = client.get_department(&sales_id);
    assert_eq!(sales.parent_id, None);
    let direct = client.get_department(&direct_id);
    assert_eq!(direct.parent_id, Some(sales_id));

    // 6. Employee membership and computed report are still intact.
    assert_eq!(
        client.get_employee_department(&emp, &acme_id),
        Some(sales_id)
    );
    assert_eq!(client.get_department_employees(&sales_id).len(), 1);
    let (sales_count, sales_children, _) = client.get_department_report(&sales_id);
    assert_eq!(sales_count, 1, "Sales still reports its assigned employee");
    assert_eq!(sales_children.len(), 1);
    assert_eq!(sales_children.get(0), Some(direct_id));
}

/// @notice Soroban `Symbol` is case-sensitive, so `"Acme"` and `"acme"` are
///         distinct identifiers and BOTH may be claimed by different orgs.
///         This documents the case-sensitivity contract.
#[test]
fn test_create_organization_names_are_case_sensitive() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let id_uc = client.create_organization(&owner, &symbol_short!("Acme"));
    let id_lc = client.create_organization(&owner, &symbol_short!("acme"));
    assert_ne!(
        id_uc, id_lc,
        "case-different names must yield distinct orgs"
    );
    let uc = client.get_organization(&id_uc);
    let lc = client.get_organization(&id_lc);
    assert_eq!(uc.name, symbol_short!("Acme"));
    assert_eq!(lc.name, symbol_short!("acme"));
}

/// @notice After a rejected duplicate-name attempt, the next legitimate
///         `create_organization` call still succeeds and returns the
///         sequential id that would have been allocated.
///
/// This test FAILS if Soroban's host ever stops rolling back the storage
/// writes made by a panicked contract function call. Under the standard
/// Soroban semantics (atomic host-function execution: panic ⇒ revert ALL
/// writes from that call), the rejected duplicate-name attempt must NOT
/// consume a counter slot, so the next legitimate create gets id 2.
#[test]
fn test_failed_duplicate_does_not_increment_next_org_id() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let stranger = Address::generate(&env);
    let first = client.create_organization(&owner, &symbol_short!("Acme"));
    assert_eq!(first, 1);

    // Failed attempt must not consume an id.
    let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.create_organization(&stranger, &symbol_short!("Acme"));
    }));
    assert!(panic_result.is_err());

    // Next legitimate create gets id = 2 (sequential, no gaps).
    let second = client.create_organization(&owner, &symbol_short!("Beta"));
    assert_eq!(second, 2, "rejected duplicate must not increment NextOrgId");
}

/// @notice Sanity check: many distinct names are accepted in sequence; the
///         uniqueness guard never blocks legitimate distinct creations.
///         This exercises the happy path alongside the rejection paths above.
///         Because `create_organization` assigns sequential ids in insertion
///         order, no sort is needed; we just verify the expected range and
///         the strictly-increasing property directly.
#[test]
fn test_create_organization_many_distinct_names_ok() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    // `symbol_short!` is limited to 9 chars; use unique short names.
    let ids = [
        client.create_organization(&owner, &symbol_short!("Org01")),
        client.create_organization(&owner, &symbol_short!("Org02")),
        client.create_organization(&owner, &symbol_short!("Org03")),
        client.create_organization(&owner, &symbol_short!("Org04")),
        client.create_organization(&owner, &symbol_short!("Org05")),
    ];
    assert_eq!(ids[0], 1, "first org gets id 1");
    assert_eq!(ids[4], 5, "fifth org gets id 5");
    for i in 0..ids.len() - 1 {
        assert!(
            ids[i] < ids[i + 1],
            "ids must be strictly increasing after a sequence of distinct creates"
        );
    }
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
    assert_eq!(
        client.get_employee_department(&emp2, &org_id),
        Some(dept_id)
    );
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

/// @notice Verifies get_department_report aggregates employees across a three-level
///         department tree (root → child → grandchild), not just direct children.
///
/// Builds:
/// ```
///        Corp (root, emp A)
///        /         \
///     Eng          Sales (emp C)
///    /    \
/// Front  Back (emp B)
/// ```
///
/// Expected rollup for Corp: 4 employees (A + B + C + D in Front)
/// Expected rollup for Eng: 2 employees (D in Front + B)
/// Expected rollup for Sales: 1 employee (C)
#[test]
fn test_multi_level_department_report_aggregation() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));

    // Level 0: root
    let root_id = client.create_department(&owner, &org_id, &symbol_short!("Corp"), &None);
    // Level 1: children
    let eng_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &Some(root_id));
    let sales_id =
        client.create_department(&owner, &org_id, &symbol_short!("Sales"), &Some(root_id));
    // Level 2: grandchildren
    let frontend_id =
        client.create_department(&owner, &org_id, &symbol_short!("Front"), &Some(eng_id));
    let backend_id =
        client.create_department(&owner, &org_id, &symbol_short!("Back"), &Some(eng_id));

    // Assign employees
    let emp_a = Address::generate(&env);
    let emp_b = Address::generate(&env);
    let emp_c = Address::generate(&env);
    let emp_d = Address::generate(&env);
    client.assign_employee_to_department(&owner, &org_id, &root_id, &emp_a);
    client.assign_employee_to_department(&owner, &org_id, &backend_id, &emp_b);
    client.assign_employee_to_department(&owner, &org_id, &sales_id, &emp_c);
    client.assign_employee_to_department(&owner, &org_id, &frontend_id, &emp_d);

    // Root report should aggregate all 4 employees across the full tree
    let (root_count, root_children, root_addrs) = client.get_department_report(&root_id);
    assert_eq!(
        root_count, 4,
        "root department should report 4 employees (A + B + C + D) across all descendants"
    );
    assert_eq!(root_addrs.len(), 4);
    assert_eq!(root_children.len(), 2);
    // Direct children only (not grandchildren)
    assert_eq!(root_children.get(0), Some(eng_id));
    assert_eq!(root_children.get(1), Some(sales_id));

    // Eng report should aggregate 0 direct + 1 in Front + 1 in Back = 2
    let (eng_count, eng_children, eng_addrs) = client.get_department_report(&eng_id);
    assert_eq!(
        eng_count, 2,
        "Eng should report 2 employees (D in Front + B in Back)"
    );
    assert_eq!(eng_addrs.len(), 2);
    assert_eq!(eng_children.len(), 2);
    assert_eq!(eng_children.get(0), Some(frontend_id));
    assert_eq!(eng_children.get(1), Some(backend_id));

    // Sales leaf — direct employee only
    let (sales_count, sales_children, sales_addrs) = client.get_department_report(&sales_id);
    assert_eq!(sales_count, 1, "Sales should report 1 employee (C)");
    assert_eq!(sales_addrs.len(), 1);
    assert_eq!(sales_children.len(), 0);

    // Backend leaf — direct employee only
    let (be_count, be_children, be_addrs) = client.get_department_report(&backend_id);
    assert_eq!(be_count, 1, "Backend should report 1 employee (B)");
    assert_eq!(be_addrs.len(), 1);
    assert_eq!(be_children.len(), 0);

    // Frontend leaf — direct employee only
    let (fe_count, fe_children, fe_addrs) = client.get_department_report(&frontend_id);
    assert_eq!(fe_count, 1, "Frontend should report 1 employee (D)");
    assert_eq!(fe_addrs.len(), 1);
    assert_eq!(fe_children.len(), 0);
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
            if id == root {
                r
            } else if id == n1 {
                x1
            } else if id == n2 {
                x2
            } else {
                x3
            }
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
    assert_eq!(page.get(0), Some(emp1));
    assert_eq!(page.get(1), Some(emp2));
    assert_eq!(page.get(2), Some(emp3));
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
    assert_eq!(page1.get(0), Some(emp1));
    assert_eq!(page1.get(1), Some(emp2));
    assert_eq!(next1, Some(2));

    // Second page of 2
    let (page2, next2) = client.get_department_employees_paged(&dept_id, &2, &2);
    assert_eq!(page2.len(), 2);
    assert_eq!(page2.get(0), Some(emp3));
    assert_eq!(page2.get(1), Some(emp4));
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
    assert_eq!(page.get(0), Some(emp3));
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

// ---------------------------------------------------------------------------
// Cycle-detection tests (Issue #763)
// ---------------------------------------------------------------------------

/// Verifies that reparenting a department under its own descendant in a 4-level
/// hierarchy is rejected with a clear error. This is the core correctness guard
/// against tree corruption and infinite traversal loops.
///
/// Builds a 4-level hierarchy:
/// ```text
///     Eng (level 0)
///      └── Backend (level 1)
///           └── Rust (level 2)
///                └── Tokio (level 3)
/// ```
///
/// Attempt 1: Reparent `Eng` under `Tokio` (deepest leaf) → must be rejected.
/// Attempt 2: Reparent `Eng` under `Rust` (mid-level descendant) → must be rejected.
/// Attempt 3: Reparent `Eng` under `Backend` (direct child) → must be rejected.
///
/// After all cycle attempts, verify valid reparenting still works by moving
/// `Tokio` to be a child of `Backend` (no longer under `Rust`).
#[test]
fn test_reparent_4level_cycle_detection_rejected() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));

    // Build 4-level hierarchy: Eng → Backend → Rust → Tokio
    let eng = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);
    let backend = client.create_department(&owner, &org_id, &symbol_short!("Backend"), &Some(eng));
    let rust = client.create_department(&owner, &org_id, &symbol_short!("Rust"), &Some(backend));
    let tokio = client.create_department(&owner, &org_id, &symbol_short!("Tokio"), &Some(rust));

    // Verify the hierarchy is correct before cycle attempts
    assert_eq!(client.get_department(&eng).parent_id, None);
    assert_eq!(client.get_department(&backend).parent_id, Some(eng));
    assert_eq!(client.get_department(&rust).parent_id, Some(backend));
    assert_eq!(client.get_department(&tokio).parent_id, Some(rust));

    // Attempt 1: Reparent Eng (root of subtree) under Tokio (deepest leaf)
    // This would create: Eng → Backend → Rust → Tokio → Eng (cycle)
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.update_department(&owner, &eng, &Some(tokio));
    }));
    assert!(
        result.is_err(),
        "Reparenting Eng under Tokio (its own descendant) must be rejected"
    );

    // Attempt 2: Reparent Eng under Rust (mid-level descendant)
    // This would create: Eng → Backend → Rust → Eng (cycle)
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.update_department(&owner, &eng, &Some(rust));
    }));
    assert!(
        result.is_err(),
        "Reparenting Eng under Rust (its own descendant) must be rejected"
    );

    // Attempt 3: Reparent Eng under Backend (direct child)
    // This would create: Eng → Backend → Eng (cycle)
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.update_department(&owner, &eng, &Some(backend));
    }));
    assert!(
        result.is_err(),
        "Reparenting Eng under Backend (its own direct child) must be rejected"
    );

    // Verify hierarchy is unchanged after all cycle attempts
    assert_eq!(client.get_department(&eng).parent_id, None);
    assert_eq!(client.get_department(&backend).parent_id, Some(eng));
    assert_eq!(client.get_department(&rust).parent_id, Some(backend));
    assert_eq!(client.get_department(&tokio).parent_id, Some(rust));

    // Verify valid reparenting still works: move Tokio from under Rust to under Backend
    client.update_department(&owner, &tokio, &Some(backend));
    assert_eq!(client.get_department(&tokio).parent_id, Some(backend));
    // Rust no longer has Tokio as child
    assert_eq!(client.get_child_departments(&rust).len(), 0);
    // Backend now has both Rust and Tokio as children
    let backend_children = client.get_child_departments(&backend);
    assert_eq!(backend_children.len(), 2);
}

/// Verifies that reparenting a leaf department to a non-descendant (valid move)
/// works correctly while cycle detection is active for other operations.
#[test]
fn test_valid_reparent_amid_cycle_guard() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));

    // Build: Root → [A → A1, B]
    let root = client.create_department(&owner, &org_id, &symbol_short!("Root"), &None);
    let a = client.create_department(&owner, &org_id, &symbol_short!("A"), &Some(root));
    let a1 = client.create_department(&owner, &org_id, &symbol_short!("A1"), &Some(a));
    let b = client.create_department(&owner, &org_id, &symbol_short!("B"), &Some(root));

    // Valid: move A1 from under A to under B (A1 is not an ancestor of B)
    client.update_department(&owner, &a1, &Some(b));
    assert_eq!(client.get_department(&a1).parent_id, Some(b));
    assert_eq!(client.get_child_departments(&a).len(), 0);
    assert_eq!(client.get_child_departments(&b).get(0), Some(a1));

    // Valid: move A1 to top-level
    client.update_department(&owner, &a1, &None);
    assert_eq!(client.get_department(&a1).parent_id, None);

    // Cycle: try to move Root under A (would create Root → A → Root)
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.update_department(&owner, &root, &Some(a));
    }));
    assert!(
        result.is_err(),
        "Cycle must still be detected after valid reparents"
    );
}

// ---------------------------------------------------------------------------
// merge_departments tests
// ---------------------------------------------------------------------------

#[test]
fn test_merge_departments_moves_members() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));
    let source = client.create_department(&owner, &org_id, &symbol_short!("Src"), &None);
    let target = client.create_department(&owner, &org_id, &symbol_short!("Tgt"), &None);

    let emp = Address::generate(&env);
    client.assign_employee_to_department(&owner, &org_id, &source, &emp);
    assert_eq!(client.get_department_employees(&source).len(), 1);
    assert_eq!(client.get_department_employees(&target).len(), 0);

    client.merge_departments(&owner, &org_id, &source, &target);

    assert_eq!(client.get_department_employees(&target).len(), 1);
    assert_eq!(
        client.get_department_employees(&target).get(0),
        Some(emp.clone())
    );
    assert_eq!(client.get_employee_department(&emp, &org_id), Some(target));
}

#[test]
fn test_merge_departments_moves_children() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));
    let source = client.create_department(&owner, &org_id, &symbol_short!("Src"), &None);
    let target = client.create_department(&owner, &org_id, &symbol_short!("Tgt"), &None);
    let child = client.create_department(&owner, &org_id, &symbol_short!("Child"), &Some(source));

    assert_eq!(client.get_child_departments(&source).len(), 1);
    assert_eq!(client.get_child_departments(&target).len(), 0);
    assert_eq!(client.get_department(&child).parent_id, Some(source));

    client.merge_departments(&owner, &org_id, &source, &target);

    assert_eq!(client.get_child_departments(&target).len(), 1);
    assert_eq!(client.get_child_departments(&target).get(0), Some(child));
    assert_eq!(client.get_department(&child).parent_id, Some(target));
    // Source is removed, so it no longer appears as a child of target
    assert_eq!(client.get_child_departments(&source).len(), 0);
}

#[test]
fn test_merge_departments_removes_source() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));
    let source = client.create_department(&owner, &org_id, &symbol_short!("Src"), &None);
    let target = client.create_department(&owner, &org_id, &symbol_short!("Tgt"), &None);

    client.merge_departments(&owner, &org_id, &source, &target);

    // source no longer exists in org department list
    let org_depts = client.get_org_departments(&org_id);
    assert_eq!(org_depts.len(), 1);
    assert_eq!(org_depts.get(0), Some(target));
}

#[test]
#[should_panic(expected = "Cannot merge a department into itself")]
fn test_merge_self_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));
    let dept = client.create_department(&owner, &org_id, &symbol_short!("A"), &None);
    client.merge_departments(&owner, &org_id, &dept, &dept);
}

#[test]
#[should_panic(expected = "Cycle detected")]
fn test_merge_into_descendant_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));
    let a = client.create_department(&owner, &org_id, &symbol_short!("A"), &None);
    let b = client.create_department(&owner, &org_id, &symbol_short!("B"), &Some(a));
    // b is a descendant of a; merging a into b would create a cycle
    client.merge_departments(&owner, &org_id, &a, &b);
}

#[test]
#[should_panic(expected = "Not organization owner")]
fn test_merge_non_owner_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let other = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));
    let source = client.create_department(&owner, &org_id, &symbol_short!("Src"), &None);
    let target = client.create_department(&owner, &org_id, &symbol_short!("Tgt"), &None);
    client.merge_departments(&other, &org_id, &source, &target);
}

#[test]
#[should_panic(expected = "Department not found")]
fn test_merge_bad_source_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));
    let target = client.create_department(&owner, &org_id, &symbol_short!("Tgt"), &None);
    client.merge_departments(&owner, &org_id, &999u128, &target);
}

#[test]
#[should_panic(expected = "Department not found")]
fn test_merge_bad_target_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Corp"));
    let source = client.create_department(&owner, &org_id, &symbol_short!("Src"), &None);
    client.merge_departments(&owner, &org_id, &source, &999u128);
}

#[test]
#[should_panic(expected = "Department not in this org")]
fn test_merge_source_wrong_org_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org1 = client.create_organization(&owner, &symbol_short!("OrgA"));
    let org2 = client.create_organization(&owner, &symbol_short!("OrgB"));
    let source = client.create_department(&owner, &org1, &symbol_short!("Src"), &None);
    let target = client.create_department(&owner, &org2, &symbol_short!("Tgt"), &None);
    client.merge_departments(&owner, &org1, &source, &target);
}

// ---------------------------------------------------------------------------
// Reverse-index consistency tests
//
// These tests prove that the two storage indexes that track employee placement
// — the *forward* index `EmployeeDepartment(addr, org_id) -> dept_id` and the
// *reverse* index `DepartmentEmployees(dept_id) -> Vec<Address>` — remain
// mutually consistent through assign → remove → reassign cycles.
//
// Invariant being tested (documented in docs/department-management.md):
//   After every public operation (assign / remove) the following always holds:
//   1. `get_employee_department(emp, org)` returns `Some(dept)` iff
//      `get_department_employees(dept)` contains `emp`.
//   2. For any department `d` that is NOT the employee's current department,
//      `get_department_employees(d)` does NOT contain `emp`.
// ---------------------------------------------------------------------------

/// Verifies the full assign → remove → reassign cycle leaves both indexes in
/// a consistent state pointing exclusively to the new department.
///
/// Sequence:
/// 1. Assign `emp` to `dept_a`   → forward: dept_a, reverse: dept_a contains emp
/// 2. Remove `emp` from dept      → forward: None,   reverse: dept_a empty
/// 3. Reassign `emp` to `dept_b`  → forward: dept_b, reverse: dept_b contains emp dept_a still
///    empty
///
/// This is the primary regression guard: without proper cleanup a stale
/// `EmployeeDepartment` entry would survive step 2, leaving `get_employee_department`
/// returning `Some(dept_a)` even after step 3 replaces it.
#[test]
fn test_assign_remove_reassign_indexes_are_consistent() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_a = client.create_department(&owner, &org_id, &symbol_short!("DeptA"), &None);
    let dept_b = client.create_department(&owner, &org_id, &symbol_short!("DeptB"), &None);
    let emp = Address::generate(&env);

    // Step 1: Assign to dept_a — both indexes must reflect dept_a.
    client.assign_employee_to_department(&owner, &org_id, &dept_a, &emp);
    assert_eq!(
        client.get_employee_department(&emp, &org_id),
        Some(dept_a),
        "after assign: forward index must point to dept_a"
    );
    assert!(
        client.get_department_employees(&dept_a).contains(&emp),
        "after assign: reverse index for dept_a must contain emp"
    );
    assert_eq!(
        client.get_department_employees(&dept_b).len(),
        0,
        "after assign: dept_b reverse index must be empty"
    );

    // Step 2: Remove from org — both indexes must show no assignment.
    client.remove_employee_from_department(&owner, &org_id, &emp);
    assert_eq!(
        client.get_employee_department(&emp, &org_id),
        None,
        "after remove: forward index must return None"
    );
    assert_eq!(
        client.get_department_employees(&dept_a).len(),
        0,
        "after remove: reverse index for dept_a must be empty"
    );

    // Step 3: Reassign to dept_b — both indexes must now reflect dept_b only.
    client.assign_employee_to_department(&owner, &org_id, &dept_b, &emp);
    assert_eq!(
        client.get_employee_department(&emp, &org_id),
        Some(dept_b),
        "after reassign: forward index must point to dept_b"
    );
    assert!(
        client.get_department_employees(&dept_b).contains(&emp),
        "after reassign: reverse index for dept_b must contain emp"
    );
    assert_eq!(
        client.get_department_employees(&dept_a).len(),
        0,
        "after reassign: dept_a reverse index must remain empty (no stale entry)"
    );
}

/// Verifies that after an employee is moved away from their original department,
/// that department's employee list no longer includes them — even when other
/// employees remain in it.
///
/// Setup:
///   dept_a: [emp1, emp2]   dept_b: []
///
/// After moving emp1 to dept_b:
///   dept_a: [emp2]         dept_b: [emp1]
///
/// This guards against a regression where the reverse index (`DepartmentEmployees`)
/// retains a stale entry for the departed employee while the forward index
/// (`EmployeeDepartment`) already points to the new department.
#[test]
fn test_original_department_excludes_moved_employee() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_a = client.create_department(&owner, &org_id, &symbol_short!("DeptA"), &None);
    let dept_b = client.create_department(&owner, &org_id, &symbol_short!("DeptB"), &None);
    let emp1 = Address::generate(&env);
    let emp2 = Address::generate(&env);

    // Assign both employees to dept_a.
    client.assign_employee_to_department(&owner, &org_id, &dept_a, &emp1);
    client.assign_employee_to_department(&owner, &org_id, &dept_a, &emp2);
    assert_eq!(
        client.get_department_employees(&dept_a).len(),
        2,
        "pre-condition: dept_a must have 2 employees"
    );

    // Move emp1 from dept_a to dept_b via direct reassignment.
    client.assign_employee_to_department(&owner, &org_id, &dept_b, &emp1);

    // emp1 must no longer appear in dept_a's employee list.
    let dept_a_employees = client.get_department_employees(&dept_a);
    assert_eq!(
        dept_a_employees.len(),
        1,
        "dept_a must have exactly 1 employee after emp1 moved out"
    );
    assert!(
        !dept_a_employees.contains(&emp1),
        "dept_a reverse index must NOT contain emp1 after it was reassigned"
    );
    assert!(
        dept_a_employees.contains(&emp2),
        "dept_a reverse index must still contain emp2 (not moved)"
    );

    // emp1 must appear exclusively in dept_b's employee list.
    let dept_b_employees = client.get_department_employees(&dept_b);
    assert_eq!(
        dept_b_employees.len(),
        1,
        "dept_b must have exactly 1 employee after emp1 moved in"
    );
    assert!(
        dept_b_employees.contains(&emp1),
        "dept_b reverse index must contain emp1"
    );

    // Forward index must agree with the reverse index.
    assert_eq!(
        client.get_employee_department(&emp1, &org_id),
        Some(dept_b),
        "forward index must point to dept_b for emp1"
    );
    assert_eq!(
        client.get_employee_department(&emp2, &org_id),
        Some(dept_a),
        "forward index must still point to dept_a for emp2"
    );
}

// ---------------------------------------------------------------------------
// Department removal tests (Issue #1094)
// ---------------------------------------------------------------------------

/// @notice Removing a department with active employees must be rejected.
///         This prevents stranding employees' organizational references.
#[test]
#[should_panic(expected = "Cannot remove department with active employees")]
fn test_remove_department_with_active_employees_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);
    let emp = Address::generate(&env);

    // Assign an employee to the department
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp);

    // Attempting to remove the department should panic
    client.remove_department(&owner, &dept_id);
}

/// @notice Removing a department with child departments must be rejected.
///         This prevents orphaning the department hierarchy.
#[test]
#[should_panic(expected = "Cannot remove department with child departments")]
fn test_remove_department_with_children_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let parent_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);
    let _child_id =
        client.create_department(&owner, &org_id, &symbol_short!("Backend"), &Some(parent_id));

    // Attempting to remove the parent department should panic
    client.remove_department(&owner, &parent_id);
}

/// @notice Removing a department succeeds when it has no employees and no children.
#[test]
fn test_remove_department_empty_succeeds() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);

    // Verify department exists
    let _dept = client.get_department(&dept_id);
    assert_eq!(client.get_org_departments(&org_id).len(), 1);

    // Remove the department
    client.remove_department(&owner, &dept_id);

    // Verify department no longer exists
    let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.get_department(&dept_id);
    }));
    assert!(
        panic_result.is_err(),
        "department should not exist after removal"
    );

    // Verify it's removed from org's department list
    assert_eq!(client.get_org_departments(&org_id).len(), 0);
}

/// @notice Removing a department succeeds after all employees are reassigned.
///         This is the positive case for the active-employee rejection.
#[test]
fn test_remove_department_after_employee_reassignment_succeeds() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_a = client.create_department(&owner, &org_id, &symbol_short!("DeptA"), &None);
    let dept_b = client.create_department(&owner, &org_id, &symbol_short!("DeptB"), &None);
    let emp = Address::generate(&env);

    // Assign employee to dept_a
    client.assign_employee_to_department(&owner, &org_id, &dept_a, &emp);
    assert_eq!(client.get_department_employees(&dept_a).len(), 1);

    // Reassign employee to dept_b
    client.assign_employee_to_department(&owner, &org_id, &dept_b, &emp);
    assert_eq!(client.get_department_employees(&dept_a).len(), 0);
    assert_eq!(client.get_department_employees(&dept_b).len(), 1);

    // Now dept_a can be removed
    client.remove_department(&owner, &dept_a);

    // Verify dept_a no longer exists
    let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.get_department(&dept_a);
    }));
    assert!(panic_result.is_err());

    // Verify dept_b still exists and has the employee
    let _dept_b = client.get_department(&dept_b);
    assert_eq!(client.get_department_employees(&dept_b).len(), 1);
    assert_eq!(client.get_employee_department(&emp, &org_id), Some(dept_b));
}

/// @notice Removing a department succeeds after all employees are removed from the org.
#[test]
fn test_remove_department_after_employee_removal_succeeds() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);
    let emp = Address::generate(&env);

    // Assign employee to the department
    client.assign_employee_to_department(&owner, &org_id, &dept_id, &emp);
    assert_eq!(client.get_department_employees(&dept_id).len(), 1);

    // Remove the employee from the org
    client.remove_employee_from_department(&owner, &org_id, &emp);
    assert_eq!(client.get_department_employees(&dept_id).len(), 0);

    // Now the department can be removed
    client.remove_department(&owner, &dept_id);

    // Verify department no longer exists
    let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.get_department(&dept_id);
    }));
    assert!(panic_result.is_err());
}

/// @notice Non-owner cannot remove a department.
#[test]
#[should_panic(expected = "Not organization owner")]
fn test_remove_department_non_owner_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let other = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);

    // Non-owner attempts to remove
    client.remove_department(&other, &dept_id);
}

/// @notice Removing a non-existent department fails.
#[test]
#[should_panic(expected = "Department not found")]
fn test_remove_department_not_found_fails() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    client.remove_department(&owner, &999u128);
}

/// @notice Removing a nested department updates the parent's children list.
#[test]
fn test_remove_nested_department_updates_parent() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let parent_id = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);
    let child_id =
        client.create_department(&owner, &org_id, &symbol_short!("Backend"), &Some(parent_id));

    // Verify parent has child
    assert_eq!(client.get_child_departments(&parent_id).len(), 1);
    assert_eq!(client.get_child_departments(&parent_id).get(0), Some(child_id));

    // Remove the child department
    client.remove_department(&owner, &child_id);

    // Verify parent no longer has the child
    assert_eq!(client.get_child_departments(&parent_id).len(), 0);

    // Verify child no longer exists
    let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.get_department(&child_id);
    }));
    assert!(panic_result.is_err());
}

/// @notice Removing a department removes it from the organization's department list.
#[test]
fn test_remove_department_updates_org_department_list() {
    let env = create_env();
    let (_cid, client) = setup_contract(&env);
    let owner = Address::generate(&env);
    let org_id = client.create_organization(&owner, &symbol_short!("Acme"));
    let dept1 = client.create_department(&owner, &org_id, &symbol_short!("Eng"), &None);
    let dept2 = client.create_department(&owner, &org_id, &symbol_short!("Sales"), &None);
    let dept3 = client.create_department(&owner, &org_id, &symbol_short!("HR"), &None);

    // Verify all three departments are in the org
    assert_eq!(client.get_org_departments(&org_id).len(), 3);

    // Remove dept2
    client.remove_department(&owner, &dept2);

    // Verify org now has only 2 departments
    let org_depts = client.get_org_departments(&org_id);
    assert_eq!(org_depts.len(), 2);
    assert!(!org_depts.contains(&dept2));
    assert!(org_depts.contains(&dept1));
    assert!(org_depts.contains(&dept3));
}
