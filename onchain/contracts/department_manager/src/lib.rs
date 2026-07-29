#![no_std]
#![allow(deprecated)]

//! Department and Organization Management Contract
//!
//! Provides hierarchical structures for organizing employees into departments
//! and organizations. Supports department creation, employee assignment,
//! employee removal, and department-level reporting.
//!
//! # Role Model
//! - **Admin**: Deploys and initializes the contract (one-time).
//! - **Org Owner**: Any authenticated address that creates an organization. Only the org owner may
//!   create departments within the org and manage all employee assignments within that org.
//!
//! # Storage Layout (for integrators)
//! | Key                                  | Value               | Description                       |
//! |--------------------------------------|---------------------|-----------------------------------|
//! | `Admin`                              | `Address`           | Contract administrator            |
//! | `Initialized`                        | `bool`              | One-time init guard               |
//! | `NextOrgId`                          | `u128`              | Auto-increment org ID counter     |
//! | `NextDeptId`                         | `u128`              | Auto-increment dept ID counter    |
//! | `Organization(org_id)`               | `Organization`      | Org record                        |
//! | `Department(dept_id)`                | `Department`        | Department record                 |
//! | `OrgDepartments(org_id)`             | `Vec<u128>`         | All dept IDs in an org            |
//! | `DepartmentChildren(parent_dept_id)` | `Vec<u128>`         | Child dept IDs                    |
//! | `EmployeeInDepartment(dept_id, addr)`| `()`               | Membership flag                   |
//! | `EmployeeDepartment(addr, org_id)`   | `u128`              | Employee → current dept in org    |
//! | `DepartmentEmployees(dept_id)`       | `Vec<Address>`      | All employees in a dept           |
//! | `OrgByName(name)`                    | `u128`              | Reverse index: name → org_id (enforces name uniqueness) |

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Vec};

/// Maximum allowed depth of the department hierarchy (root = depth 0).
/// A department at depth MAX_DEPTH cannot have children.
pub const MAX_DEPTH: u32 = 10;

#[contract]
pub struct DepartmentManagerContract;

/// Storage keys for the contract
#[contracttype]
#[derive(Clone)]
enum StorageKey {
    /// Admin address (authorized during initialization)
    Admin,
    /// Initialization flag
    Initialized,
    /// Next organization ID counter
    NextOrgId,
    /// Next department ID counter (global)
    NextDeptId,
    /// Organization data: org_id -> Organization
    Organization(u128),
    /// Department data: dept_id -> Department
    Department(u128),
    /// All department IDs under an organization: org_id -> Vec<u128>
    OrgDepartments(u128),
    /// Child department IDs: parent_dept_id -> Vec<u128>
    DepartmentChildren(u128),
    /// Employee membership flag: (dept_id, employee_address) -> ()
    EmployeeInDepartment(u128, Address),
    /// Current department for an employee in an org: (employee, org_id) -> dept_id
    EmployeeDepartment(Address, u128),
    /// List of employee addresses in a department: dept_id -> Vec<Address>
    DepartmentEmployees(u128),
    /// Reverse index enforcing organization name uniqueness: name -> org_id.
    /// Presence of this entry under `name` indicates the name is already claimed
    /// by the org whose ID matches the stored value. Acts as the unique-organization-id
    /// guard for `create_organization`.
    OrgByName(soroban_sdk::Symbol),
}

/// Organization record
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Organization {
    pub id: u128,
    pub name: soroban_sdk::Symbol,
    pub owner: Address,
    pub created_at: u64,
}

/// Department record with optional parent for hierarchy
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Department {
    pub id: u128,
    pub org_id: u128,
    pub name: soroban_sdk::Symbol,
    pub parent_id: Option<u128>,
    pub created_at: u64,
}

#[contractimpl]
impl DepartmentManagerContract {
    // -------------------------------------------------------------------------
    // Initialization (Admin only)
    // -------------------------------------------------------------------------

    /// Initializes the contract. **Callable once** by the deployer.
    ///
    /// # Arguments
    /// * `admin` - Address that will be the admin (must authenticate).
    ///
    /// # Panics
    /// Panics with `"Already initialized"` if called more than once.
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        let initialized: bool = env
            .storage()
            .persistent()
            .get(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(!initialized, "Already initialized");
        env.storage().persistent().set(&StorageKey::Admin, &admin);
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);
        env.storage()
            .persistent()
            .set(&StorageKey::NextOrgId, &1u128);
        env.storage()
            .persistent()
            .set(&StorageKey::NextDeptId, &1u128);
    }

    // -------------------------------------------------------------------------
    // Organizations (Org Owner operations)
    // -------------------------------------------------------------------------

    /// Creates a new organization. The caller becomes the **org owner**.
    ///
    /// # Uniqueness Invariant (Issue #917)
    /// Each `name` is **globally unique** across the entire contract. If `name`
    /// is already claimed by an existing organization (regardless of owner), the
    /// call is rejected and **all state mutations are rolled back**, so the
    /// original organization's record and its `OrgDepartments` tree are left
    /// fully intact. This guards against a re-creation that would otherwise
    /// reset or overwrite the org's department tree.
    ///
    /// Symbols are case-sensitive in Soroban, so `"Acme"` and `"acme"` are
    /// distinct identifiers. There is no `delete_organization`, so claiming a
    /// name locks it permanently for the lifetime of the contract.
    ///
    /// # Arguments
    /// * `owner` - Caller (must authenticate); becomes org owner.
    /// * `name`  - Symbol name for the organization (must be unique).
    ///
    /// # Returns
    /// The new organization ID (starts at 1, increments by 1).
    ///
    /// # Panics
    /// - `"Organization name already in use"` – `name` already maps to an existing organization via
    ///   the `OrgByName` reverse index.
    /// - `"Contract not initialized"` – `initialize` was not called.
    ///
    /// # Events
    /// Publishes `("org_created", org_id)` on success.
    pub fn create_organization(env: Env, owner: Address, name: soroban_sdk::Symbol) -> u128 {
        owner.require_auth();
        Self::require_initialized(&env);

        // === Unique-organization-id guard ===
        // Reject BEFORE allocating any state (NextOrgId, Organization record,
        // OrgDepartments Vec). On panic, Soroban rolls back storage writes from
        // this call, so the check is also safe if placed later; doing it first
        // keeps the invariant explicit and prevents wasting an org_id on a
        // duplicate-name attempt.
        assert!(
            env.storage()
                .persistent()
                .get::<_, u128>(&StorageKey::OrgByName(name.clone()))
                .is_none(),
            "Organization name already in use"
        );

        let next_id: u128 = env
            .storage()
            .persistent()
            .get(&StorageKey::NextOrgId)
            .unwrap_or(1);
        env.storage()
            .persistent()
            .set(&StorageKey::NextOrgId, &(next_id + 1));
        let org = Organization {
            id: next_id,
            name: name.clone(),
            owner: owner.clone(),
            created_at: env.ledger().timestamp(),
        };
        env.storage()
            .persistent()
            .set(&StorageKey::Organization(next_id), &org);
        let empty: Vec<u128> = Vec::new(&env);
        env.storage()
            .persistent()
            .set(&StorageKey::OrgDepartments(next_id), &empty);

        // Register the name in the reverse index so future calls with the same
        // name are rejected by the guard above.
        env.storage()
            .persistent()
            .set(&StorageKey::OrgByName(name), &next_id);

        env.events()
            .publish((symbol_short!("org_crtd"), next_id), next_id);

        next_id
    }

    /// Returns the organization record.
    ///
    /// # Arguments
    /// * `org_id` - The organization ID.
    ///
    /// # Panics
    /// Panics with `"Organization not found"` if the ID does not exist.
    pub fn get_organization(env: Env, org_id: u128) -> Organization {
        env.storage()
            .persistent()
            .get(&StorageKey::Organization(org_id))
            .expect("Organization not found")
    }

    // -------------------------------------------------------------------------
    // Departments (Org Owner operations)
    // -------------------------------------------------------------------------

    /// Creates a department under an organization.
    ///
    /// The department can be:
    /// - **Top-level**: `parent_id = None`
    /// - **Nested**: `parent_id = Some(parent_dept_id)` (parent must be in same org)
    ///
    /// # Arguments
    /// * `caller`    - Must be the **org owner** (must authenticate).
    /// * `org_id`    - Organization ID.
    /// * `name`      - Symbol name for the department.
    /// * `parent_id` - Optional parent department ID; `None` for top-level.
    ///
    /// # Returns
    /// The new department ID (global counter, starts at 1).
    ///
    /// # Panics
    /// - `"Organization not found"` – org_id does not exist.
    /// - `"Not organization owner"` – caller is not the org owner.
    /// - `"Parent department not found"` – parent_id does not exist.
    /// - `"Parent must be in same org"` – parent belongs to a different org.
    ///
    /// # Events
    /// Publishes `("dept_crtd", dept_id)` on success.
    pub fn create_department(
        env: Env,
        caller: Address,
        org_id: u128,
        name: soroban_sdk::Symbol,
        parent_id: Option<u128>,
    ) -> u128 {
        caller.require_auth();
        Self::require_initialized(&env);
        let org: Organization = env
            .storage()
            .persistent()
            .get(&StorageKey::Organization(org_id))
            .expect("Organization not found");
        assert!(org.owner == caller, "Not organization owner");

        if let Some(pid) = parent_id {
            let parent: Department = env
                .storage()
                .persistent()
                .get(&StorageKey::Department(pid))
                .expect("Parent department not found");
            assert!(parent.org_id == org_id, "Parent must be in same org");
            // child depth = parent depth + 1; must not exceed MAX_DEPTH
            assert!(
                Self::dept_depth(&env, pid) + 1 <= MAX_DEPTH,
                "Max hierarchy depth exceeded"
            );
        }

        let next_id: u128 = env
            .storage()
            .persistent()
            .get(&StorageKey::NextDeptId)
            .unwrap_or(1);
        env.storage()
            .persistent()
            .set(&StorageKey::NextDeptId, &(next_id + 1));

        let dept = Department {
            id: next_id,
            org_id,
            name,
            parent_id,
            created_at: env.ledger().timestamp(),
        };
        env.storage()
            .persistent()
            .set(&StorageKey::Department(next_id), &dept);

        // Register dept under org
        let mut org_depts: Vec<u128> = env
            .storage()
            .persistent()
            .get(&StorageKey::OrgDepartments(org_id))
            .unwrap_or_else(|| Vec::new(&env));
        org_depts.push_back(next_id);
        env.storage()
            .persistent()
            .set(&StorageKey::OrgDepartments(org_id), &org_depts);

        // Register as child of parent dept if nested
        if let Some(pid) = parent_id {
            let mut children: Vec<u128> = env
                .storage()
                .persistent()
                .get(&StorageKey::DepartmentChildren(pid))
                .unwrap_or_else(|| Vec::new(&env));
            children.push_back(next_id);
            env.storage()
                .persistent()
                .set(&StorageKey::DepartmentChildren(pid), &children);
        }

        // Initialize empty employee list for this department
        let empty_employees: Vec<Address> = Vec::new(&env);
        env.storage()
            .persistent()
            .set(&StorageKey::DepartmentEmployees(next_id), &empty_employees);

        env.events()
            .publish((symbol_short!("dept_crtd"), next_id), next_id);

        next_id
    }

    /// Returns the department record.
    ///
    /// # Arguments
    /// * `department_id` - The department ID.
    ///
    /// # Panics
    /// Panics with `"Department not found"` if the ID does not exist.
    pub fn get_department(env: Env, department_id: u128) -> Department {
        env.storage()
            .persistent()
            .get(&StorageKey::Department(department_id))
            .expect("Department not found")
    }

    /// Returns all department IDs (top-level and nested) under an organization.
    ///
    /// # Arguments
    /// * `org_id` - The organization ID.
    pub fn get_org_departments(env: Env, org_id: u128) -> Vec<u128> {
        env.storage()
            .persistent()
            .get(&StorageKey::OrgDepartments(org_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns the direct child department IDs of a given department.
    ///
    /// # Arguments
    /// * `department_id` - The parent department ID.
    pub fn get_child_departments(env: Env, department_id: u128) -> Vec<u128> {
        env.storage()
            .persistent()
            .get(&StorageKey::DepartmentChildren(department_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    // -------------------------------------------------------------------------
    // Employee Assignment (Org Owner operations)
    // -------------------------------------------------------------------------

    /// Assigns an employee address to a department within an organization.
    ///
    /// If the employee is already assigned to another department in the same
    /// org, they are **automatically removed** from the old department first
    /// (re-assignment/move semantics).
    ///
    /// # Arguments
    /// * `caller`        - Must be the **org owner** (must authenticate).
    /// * `org_id`        - Organization ID.
    /// * `department_id` - Target department ID (must belong to `org_id`).
    /// * `employee`      - Employee address to assign.
    ///
    /// # Panics
    /// - `"Organization not found"` – org_id does not exist.
    /// - `"Not organization owner"` – caller is not the org owner.
    /// - `"Department not found"` – department_id does not exist.
    /// - `"Department not in this org"` – dept belongs to a different org.
    ///
    /// # Events
    /// Publishes `("emp_asgnd", employee)` on success.
    pub fn assign_employee_to_department(
        env: Env,
        caller: Address,
        org_id: u128,
        department_id: u128,
        employee: Address,
    ) {
        caller.require_auth();
        Self::require_initialized(&env);
        let org: Organization = env
            .storage()
            .persistent()
            .get(&StorageKey::Organization(org_id))
            .expect("Organization not found");
        assert!(org.owner == caller, "Not organization owner");

        let dept: Department = env
            .storage()
            .persistent()
            .get(&StorageKey::Department(department_id))
            .expect("Department not found");
        assert!(dept.org_id == org_id, "Department not in this org");

        // Remove from previous department in this org, if any
        if let Some(old_dept) = env
            .storage()
            .persistent()
            .get::<_, u128>(&StorageKey::EmployeeDepartment(employee.clone(), org_id))
        {
            Self::remove_employee_from_dept_internal(&env, old_dept, &employee);
        }

        env.storage().persistent().set(
            &StorageKey::EmployeeInDepartment(department_id, employee.clone()),
            &(),
        );
        env.storage().persistent().set(
            &StorageKey::EmployeeDepartment(employee.clone(), org_id),
            &department_id,
        );

        let mut employees: Vec<Address> = env
            .storage()
            .persistent()
            .get(&StorageKey::DepartmentEmployees(department_id))
            .unwrap_or_else(|| Vec::new(&env));
        employees.push_back(employee.clone());
        env.storage()
            .persistent()
            .set(&StorageKey::DepartmentEmployees(department_id), &employees);

        env.events()
            .publish((symbol_short!("emp_asgnd"), department_id), employee);
    }

    /// Removes (un-assigns) an employee from their current department in an org.
    ///
    /// After calling this, `get_employee_department` returns `None` for the
    /// employee in that org.
    ///
    /// # Arguments
    /// * `caller`   - Must be the **org owner** (must authenticate).
    /// * `org_id`   - Organization ID.
    /// * `employee` - Employee address to remove.
    ///
    /// # Panics
    /// - `"Organization not found"` – org_id does not exist.
    /// - `"Not organization owner"` – caller is not the org owner.
    /// - `"Employee not assigned in this org"` – employee has no assignment.
    ///
    /// # Events
    /// Publishes `("emp_rmvd", employee)` on success.
    pub fn remove_employee_from_department(
        env: Env,
        caller: Address,
        org_id: u128,
        employee: Address,
    ) {
        caller.require_auth();
        Self::require_initialized(&env);
        let org: Organization = env
            .storage()
            .persistent()
            .get(&StorageKey::Organization(org_id))
            .expect("Organization not found");
        assert!(org.owner == caller, "Not organization owner");

        let dept_id: u128 = env
            .storage()
            .persistent()
            .get::<_, u128>(&StorageKey::EmployeeDepartment(employee.clone(), org_id))
            .expect("Employee not assigned in this org");

        Self::remove_employee_from_dept_internal(&env, dept_id, &employee);

        env.storage()
            .persistent()
            .remove(&StorageKey::EmployeeDepartment(employee.clone(), org_id));

        env.events()
            .publish((symbol_short!("emp_rmvd"), dept_id), employee);
    }

    // -------------------------------------------------------------------------
    // Reporting (read-only, no auth required)
    // -------------------------------------------------------------------------

    /// Returns the list of employee addresses assigned to a department.
    ///
    /// # Arguments
    /// * `department_id` - The department ID.
    pub fn get_department_employees(env: Env, department_id: u128) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&StorageKey::DepartmentEmployees(department_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns the department ID the employee is currently in within an org,
    /// or `None` if they are not assigned to any department in that org.
    ///
    /// # Arguments
    /// * `employee` - Employee address.
    /// * `org_id`   - Organization ID.
    pub fn get_employee_department(env: Env, employee: Address, org_id: u128) -> Option<u128> {
        env.storage()
            .persistent()
            .get(&StorageKey::EmployeeDepartment(employee, org_id))
    }

    /// Returns a department-level report that **recursively** aggregates
    /// employee figures across the full descendant tree:
    /// `(total_employee_count, direct_child_department_ids, all_employee_addresses)`.
    ///
    /// The returned `employee_count` and `employee_addresses` include employees
    /// from the queried department **and** every descendant department (children,
    /// grandchildren, …).  `child_department_ids` contains only **direct**
    /// children (one level) so callers can still traverse the tree structure.
    ///
    /// # Arguments
    /// * `department_id` - The department ID.
    pub fn get_department_report(env: Env, department_id: u128) -> (u32, Vec<u128>, Vec<Address>) {
        Self::collect_descendant_employees(&env, department_id)
    }

    /// Recursively collects all employee addresses from `dept_id` and its
    /// full descendant subtree.  Returns `(count, direct_children, employees)`.
    fn collect_descendant_employees(env: &Env, dept_id: u128) -> (u32, Vec<u128>, Vec<Address>) {
        let children: Vec<u128> = env
            .storage()
            .persistent()
            .get(&StorageKey::DepartmentChildren(dept_id))
            .unwrap_or_else(|| Vec::new(env));

        let mut all_employees: Vec<Address> = Vec::new(env);

        let direct_employees: Vec<Address> = env
            .storage()
            .persistent()
            .get(&StorageKey::DepartmentEmployees(dept_id))
            .unwrap_or_else(|| Vec::new(env));
        for emp in direct_employees.iter() {
            all_employees.push_back(emp);
        }

        for child_id in children.iter() {
            let (_, _, child_employees) = Self::collect_descendant_employees(env, child_id);
            for emp in child_employees.iter() {
                all_employees.push_back(emp);
            }
        }

        (all_employees.len(), children, all_employees)
    }

    /// Reparents a department to a new parent (or makes it top-level).
    ///
    /// # Arguments
    /// * `caller`     - Must be the **org owner** (must authenticate).
    /// * `dept_id`    - Department to move.
    /// * `new_parent` - `Some(parent_dept_id)` or `None` for top-level.
    ///
    /// # Panics
    /// - `"Organization not found"` – org not found.
    /// - `"Not organization owner"` – caller is not the org owner.
    /// - `"Department not found"` – dept_id does not exist.
    /// - `"Parent department not found"` – new_parent does not exist.
    /// - `"Parent must be in same org"` – new parent is in a different org.
    /// - `"Max hierarchy depth exceeded"` – new depth would exceed MAX_DEPTH.
    /// - `"Cycle detected"` – new parent is a descendant of dept_id.
    ///
    /// # Events
    /// Publishes `("dept_mvd", dept_id)` on success.
    pub fn update_department(env: Env, caller: Address, dept_id: u128, new_parent: Option<u128>) {
        caller.require_auth();
        Self::require_initialized(&env);

        let mut dept: Department = env
            .storage()
            .persistent()
            .get(&StorageKey::Department(dept_id))
            .expect("Department not found");

        let org: Organization = env
            .storage()
            .persistent()
            .get(&StorageKey::Organization(dept.org_id))
            .expect("Organization not found");
        assert!(org.owner == caller, "Not organization owner");

        if let Some(pid) = new_parent {
            let parent: Department = env
                .storage()
                .persistent()
                .get(&StorageKey::Department(pid))
                .expect("Parent department not found");
            assert!(parent.org_id == dept.org_id, "Parent must be in same org");
            // child depth = parent depth + 1; must not exceed MAX_DEPTH
            assert!(
                Self::dept_depth(&env, pid) + 1 <= MAX_DEPTH,
                "Max hierarchy depth exceeded"
            );
            assert!(!Self::has_cycle(&env, dept_id, pid), "Cycle detected");
        }

        // Remove dept_id from old parent's children list
        if let Some(old_pid) = dept.parent_id {
            let mut old_children: Vec<u128> = env
                .storage()
                .persistent()
                .get(&StorageKey::DepartmentChildren(old_pid))
                .unwrap_or_else(|| Vec::new(&env));
            let mut i = 0u32;
            while i < old_children.len() {
                if old_children.get(i) == Some(dept_id) {
                    old_children.remove(i);
                    break;
                }
                i += 1;
            }
            env.storage()
                .persistent()
                .set(&StorageKey::DepartmentChildren(old_pid), &old_children);
        }

        // Add dept_id to new parent's children list
        if let Some(pid) = new_parent {
            let mut new_children: Vec<u128> = env
                .storage()
                .persistent()
                .get(&StorageKey::DepartmentChildren(pid))
                .unwrap_or_else(|| Vec::new(&env));
            new_children.push_back(dept_id);
            env.storage()
                .persistent()
                .set(&StorageKey::DepartmentChildren(pid), &new_children);
        }

        dept.parent_id = new_parent;
        env.storage()
            .persistent()
            .set(&StorageKey::Department(dept_id), &dept);

        env.events()
            .publish((symbol_short!("dept_mvd"), dept_id), dept_id);
    }

    /// Renames a department while preserving all employee associations and hierarchy.
    ///
    /// This operation updates only the department's name field. All employee
    /// assignments (forward index `EmployeeDepartment` and reverse index
    /// `DepartmentEmployees`) remain completely unchanged, ensuring that employee
    /// department lookups continue to resolve correctly after the rename.
    ///
    /// # Arguments
    /// * `caller`    - Must be the **org owner** (must authenticate).
    /// * `dept_id`   - Department to rename.
    /// * `new_name`  - New symbol name for the department.
    ///
    /// # Panics
    /// - `"Organization not found"` – org not found.
    /// - `"Not organization owner"` – caller is not the org owner.
    /// - `"Department not found"` – dept_id does not exist.
    ///
    /// # Events
    /// Publishes `("dept_renamed", dept_id)` on success with the new name as data.
    ///
    /// # Security & Correctness
    /// **Employee Association Preservation**: The department name is a metadata field
    /// stored separately from employee indexes. Renaming does not modify:
    /// - Forward index: `EmployeeDepartment(emp, org_id) → dept_id`
    /// - Reverse index: `DepartmentEmployees(dept_id) → Vec<Address>`
    ///
    /// Therefore, all employee lookups remain valid after rename.
    pub fn rename_department(
        env: Env,
        caller: Address,
        dept_id: u128,
        new_name: soroban_sdk::Symbol,
    ) {
        caller.require_auth();
        Self::require_initialized(&env);

        let mut dept: Department = env
            .storage()
            .persistent()
            .get(&StorageKey::Department(dept_id))
            .expect("Department not found");

        let org: Organization = env
            .storage()
            .persistent()
            .get(&StorageKey::Organization(dept.org_id))
            .expect("Organization not found");
        assert!(org.owner == caller, "Not organization owner");

        // Update the department name
        dept.name = new_name.clone();
        env.storage()
            .persistent()
            .set(&StorageKey::Department(dept_id), &dept);

        env.events()
            .publish((symbol_short!("dpt_rn"), dept_id), new_name);
    }

    /// Merges two departments within the same organization.
    ///
    /// Moves all members and child departments from `source` into `target`,
    /// then removes `source`.
    ///
    /// # Arguments
    /// * `caller` - Must be the **org owner** (must authenticate).
    /// * `org_id`  - Organization ID.
    /// * `source`  - Department to merge (will be removed).
    /// * `target`  - Department to merge into (will absorb members/children).
    ///
    /// # Panics
    /// - `"Organization not found"` – org_id does not exist.
    /// - `"Not organization owner"` – caller is not the org owner.
    /// - `"Department not found"` – source or target does not exist.
    /// - `"Department not in this org"` – source or target belongs to a different org.
    /// - `"Cannot merge a department into itself"` – source == target.
    /// - `"Cycle detected"` – target is a descendant of source.
    ///
    /// # Events
    /// Publishes `("dept_merged", source)` with target as data on success.
    pub fn merge_departments(env: Env, caller: Address, org_id: u128, source: u128, target: u128) {
        caller.require_auth();
        Self::require_initialized(&env);

        let source_dept: Department = env
            .storage()
            .persistent()
            .get(&StorageKey::Department(source))
            .expect("Department not found");
        assert!(source_dept.org_id == org_id, "Department not in this org");

        let target_dept: Department = env
            .storage()
            .persistent()
            .get(&StorageKey::Department(target))
            .expect("Department not found");
        assert!(target_dept.org_id == org_id, "Department not in this org");

        let org: Organization = env
            .storage()
            .persistent()
            .get(&StorageKey::Organization(org_id))
            .expect("Organization not found");
        assert!(org.owner == caller, "Not organization owner");

        assert!(source != target, "Cannot merge a department into itself");
        assert!(!Self::has_cycle(&env, source, target), "Cycle detected");

        // Move all members from source to target
        let source_employees: Vec<Address> = env
            .storage()
            .persistent()
            .get(&StorageKey::DepartmentEmployees(source))
            .unwrap_or_else(|| Vec::new(&env));
        for emp in source_employees.iter() {
            Self::remove_employee_from_dept_internal(&env, source, &emp);
            env.storage()
                .persistent()
                .set(&StorageKey::EmployeeInDepartment(target, emp.clone()), &());
            env.storage().persistent().set(
                &StorageKey::EmployeeDepartment(emp.clone(), org_id),
                &target,
            );
            let mut target_employees: Vec<Address> = env
                .storage()
                .persistent()
                .get(&StorageKey::DepartmentEmployees(target))
                .unwrap_or_else(|| Vec::new(&env));
            target_employees.push_back(emp.clone());
            env.storage()
                .persistent()
                .set(&StorageKey::DepartmentEmployees(target), &target_employees);
        }

        // Move all child departments from source to target
        let source_children: Vec<u128> = env
            .storage()
            .persistent()
            .get(&StorageKey::DepartmentChildren(source))
            .unwrap_or_else(|| Vec::new(&env));
        for child_id in source_children.iter() {
            let mut child: Department = env
                .storage()
                .persistent()
                .get(&StorageKey::Department(child_id))
                .expect("Department not found");
            child.parent_id = Some(target);
            env.storage()
                .persistent()
                .set(&StorageKey::Department(child_id), &child);

            let mut target_children: Vec<u128> = env
                .storage()
                .persistent()
                .get(&StorageKey::DepartmentChildren(target))
                .unwrap_or_else(|| Vec::new(&env));
            target_children.push_back(child_id);
            env.storage()
                .persistent()
                .set(&StorageKey::DepartmentChildren(target), &target_children);
        }

        // Remove source from its parent's children list
        if let Some(parent_id) = source_dept.parent_id {
            let mut parent_children: Vec<u128> = env
                .storage()
                .persistent()
                .get(&StorageKey::DepartmentChildren(parent_id))
                .unwrap_or_else(|| Vec::new(&env));
            let mut i = 0u32;
            while i < parent_children.len() {
                if parent_children.get(i) == Some(source) {
                    parent_children.remove(i);
                    break;
                }
                i += 1;
            }
            env.storage()
                .persistent()
                .set(&StorageKey::DepartmentChildren(parent_id), &parent_children);
        }

        // Remove source from org's department list
        let mut org_depts: Vec<u128> = env
            .storage()
            .persistent()
            .get(&StorageKey::OrgDepartments(org_id))
            .unwrap_or_else(|| Vec::new(&env));
        let mut i = 0u32;
        while i < org_depts.len() {
            if org_depts.get(i) == Some(source) {
                org_depts.remove(i);
                break;
            }
            i += 1;
        }
        env.storage()
            .persistent()
            .set(&StorageKey::OrgDepartments(org_id), &org_depts);

        // Clean up source storage entries
        env.storage()
            .persistent()
            .remove(&StorageKey::Department(source));
        env.storage()
            .persistent()
            .remove(&StorageKey::DepartmentChildren(source));
        env.storage()
            .persistent()
            .remove(&StorageKey::DepartmentEmployees(source));

        env.events()
            .publish((symbol_short!("dept_mrg"), source), target);
    }

    /// Removes a department from an organization.
    ///
    /// # Arguments
    /// * `caller`   - Must be the **org owner** (must authenticate).
    /// * `dept_id`  - Department ID to remove.
    ///
    /// # Panics
    /// - `"Organization not found"` – org not found.
    /// - `"Not organization owner"` – caller is not the org owner.
    /// - `"Department not found"` – dept_id does not exist.
    /// - `"Cannot remove department with active employees"` – department has employees assigned.
    /// - `"Cannot remove department with child departments"` – department has child departments.
    ///
    /// # Events
    /// Publishes `("dept_rmvd", dept_id)` on success.
    pub fn remove_department(env: Env, caller: Address, dept_id: u128) {
        caller.require_auth();
        Self::require_initialized(&env);

        let dept: Department = env
            .storage()
            .persistent()
            .get(&StorageKey::Department(dept_id))
            .expect("Department not found");

        let org: Organization = env
            .storage()
            .persistent()
            .get(&StorageKey::Organization(dept.org_id))
            .expect("Organization not found");
        assert!(org.owner == caller, "Not organization owner");

        // Check for active employees
        let employees: Vec<Address> = env
            .storage()
            .persistent()
            .get(&StorageKey::DepartmentEmployees(dept_id))
            .unwrap_or_else(|| Vec::new(&env));
        assert!(
            employees.is_empty(),
            "Cannot remove department with active employees"
        );

        // Check for child departments
        let children: Vec<u128> = env
            .storage()
            .persistent()
            .get(&StorageKey::DepartmentChildren(dept_id))
            .unwrap_or_else(|| Vec::new(&env));
        assert!(
            children.is_empty(),
            "Cannot remove department with child departments"
        );

        // Remove dept from its parent's children list (if it has a parent)
        if let Some(parent_id) = dept.parent_id {
            let mut parent_children: Vec<u128> = env
                .storage()
                .persistent()
                .get(&StorageKey::DepartmentChildren(parent_id))
                .unwrap_or_else(|| Vec::new(&env));
            let mut i = 0u32;
            while i < parent_children.len() {
                if parent_children.get(i) == Some(dept_id) {
                    parent_children.remove(i);
                    break;
                }
                i += 1;
            }
            env.storage()
                .persistent()
                .set(&StorageKey::DepartmentChildren(parent_id), &parent_children);
        }

        // Remove dept from org's department list
        let mut org_depts: Vec<u128> = env
            .storage()
            .persistent()
            .get(&StorageKey::OrgDepartments(dept.org_id))
            .unwrap_or_else(|| Vec::new(&env));
        let mut i = 0u32;
        while i < org_depts.len() {
            if org_depts.get(i) == Some(dept_id) {
                org_depts.remove(i);
                break;
            }
            i += 1;
        }
        env.storage()
            .persistent()
            .set(&StorageKey::OrgDepartments(dept.org_id), &org_depts);

        // Clean up department storage entries
        env.storage()
            .persistent()
            .remove(&StorageKey::Department(dept_id));
        env.storage()
            .persistent()
            .remove(&StorageKey::DepartmentChildren(dept_id));
        env.storage()
            .persistent()
            .remove(&StorageKey::DepartmentEmployees(dept_id));

        env.events()
            .publish((symbol_short!("dept_rmvd"), dept_id), dept_id);
    }

    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

    /// Returns the depth of `dept_id` in the hierarchy (root = 0).
    fn dept_depth(env: &Env, dept_id: u128) -> u32 {
        let mut depth = 0u32;
        let mut current = dept_id;
        loop {
            let dept: Department = match env
                .storage()
                .persistent()
                .get(&StorageKey::Department(current))
            {
                Some(d) => d,
                None => break,
            };
            match dept.parent_id {
                None => break,
                Some(pid) => {
                    depth += 1;
                    current = pid;
                }
            }
        }
        depth
    }

    /// Returns `true` if making `candidate` the parent of `dept_id` would
    /// create a cycle (i.e., `candidate` is already a descendant of `dept_id`).
    fn has_cycle(env: &Env, dept_id: u128, candidate: u128) -> bool {
        // Walk up from candidate; if we reach dept_id, it's a cycle.
        let mut current = candidate;
        loop {
            if current == dept_id {
                return true;
            }
            let dept: Department = match env
                .storage()
                .persistent()
                .get(&StorageKey::Department(current))
            {
                Some(d) => d,
                None => return false,
            };
            match dept.parent_id {
                None => return false,
                Some(pid) => current = pid,
            }
        }
    }

    /// Removes an employee from a department's employee list and membership flag.
    /// Does NOT update `EmployeeDepartment` – caller must handle that.
    fn remove_employee_from_dept_internal(env: &Env, department_id: u128, employee: &Address) {
        env.storage()
            .persistent()
            .remove(&StorageKey::EmployeeInDepartment(
                department_id,
                employee.clone(),
            ));

        let mut employees: Vec<Address> = env
            .storage()
            .persistent()
            .get(&StorageKey::DepartmentEmployees(department_id))
            .unwrap_or_else(|| Vec::new(env));

        let mut i = 0u32;
        while i < employees.len() {
            if employees.get(i).map(|a| a == *employee).unwrap_or(false) {
                employees.remove(i);
                break;
            }
            i += 1;
        }
        env.storage()
            .persistent()
            .set(&StorageKey::DepartmentEmployees(department_id), &employees);
    }

    /// Returns a bounded page of employee addresses in a department.
    ///
    /// Ordering is stable (insertion order) so callers can use the returned
    /// `next_start` cursor in subsequent calls without risk of skipping or
    /// duplicating members.
    ///
    /// # Arguments
    /// * `department_id` - Department to query
    /// * `start`         - Zero-based index of the first employee to return
    /// * `limit`         - Maximum number of employees to return; clamped to [`MAX_PAGE_SIZE`] (50)
    ///   to bound instruction usage
    ///
    /// # Returns
    /// A tuple `(page, next_start)` where:
    /// - `page`       – the slice of employee addresses for this page
    /// - `next_start` – the index to pass as `start` on the next call, or `None` when the last page
    ///   has been reached
    ///
    /// # Security
    /// Clamping `limit` to `MAX_PAGE_SIZE` prevents unbounded-return DoS.
    /// Cursor arithmetic uses simple addition, making skip/duplicate
    /// impossible for a stable list.
    pub fn get_department_employees_paged(
        env: Env,
        department_id: u128,
        start: u32,
        limit: u32,
    ) -> (Vec<Address>, Option<u32>) {
        /// Hard upper bound on employees returned per call.
        const MAX_PAGE_SIZE: u32 = 50;

        let effective_limit = if limit == 0 || limit > MAX_PAGE_SIZE {
            MAX_PAGE_SIZE
        } else {
            limit
        };

        let all_employees: Vec<Address> = env
            .storage()
            .persistent()
            .get(&StorageKey::DepartmentEmployees(department_id))
            .unwrap_or_else(|| Vec::new(&env));

        let total = all_employees.len();
        let mut page: Vec<Address> = Vec::new(&env);

        if start >= total {
            return (page, None);
        }

        let end = {
            let candidate = start + effective_limit;
            if candidate > total {
                total
            } else {
                candidate
            }
        };

        let mut i = start;
        while i < end {
            if let Some(addr) = all_employees.get(i) {
                page.push_back(addr);
            }
            i += 1;
        }

        let next_start = if end < total { Some(end) } else { None };
        (page, next_start)
    }

    /// Asserts the contract has been initialized.
    ///
    /// # Panics
    /// Panics with `"Contract not initialized"` if `initialize` was never called.
    fn require_initialized(env: &Env) {
        let initialized: bool = env
            .storage()
            .persistent()
            .get(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(initialized, "Contract not initialized");
    }
}
