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
//! - **Org Owner**: Any authenticated address that creates an organization.
//!   Only the org owner may create departments within the org and manage
//!   all employee assignments within that org.
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

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Vec};

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
    /// # Arguments
    /// * `owner` - Caller (must authenticate); becomes org owner.
    /// * `name`  - Symbol name for the organization.
    ///
    /// # Returns
    /// The new organization ID (starts at 1, increments by 1).
    ///
    /// # Events
    /// Publishes `("org_created", org_id)` on success.
    pub fn create_organization(env: Env, owner: Address, name: soroban_sdk::Symbol) -> u128 {
        owner.require_auth();
        Self::require_initialized(&env);
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
            name,
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

    /// Returns a department-level report:
    /// `(employee_count, child_department_ids, employee_addresses)`.
    ///
    /// # Arguments
    /// * `department_id` - The department ID.
    pub fn get_department_report(env: Env, department_id: u128) -> (u32, Vec<u128>, Vec<Address>) {
        let employees: Vec<Address> = env
            .storage()
            .persistent()
            .get(&StorageKey::DepartmentEmployees(department_id))
            .unwrap_or_else(|| Vec::new(&env));
        let children: Vec<u128> = env
            .storage()
            .persistent()
            .get(&StorageKey::DepartmentChildren(department_id))
            .unwrap_or_else(|| Vec::new(&env));
        (employees.len(), children, employees)
    }

    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

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
