#![no_std]

//! Department and Organization Management Contract
//!
//! Provides hierarchical structures for organizing employees into departments
//! and organizations. Supports department creation, employee assignment, and
//! department-level reporting.

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Vec};

#[contract]
pub struct DepartmentManagerContract;

/// Storage keys for the contract
#[contracttype]
#[derive(Clone)]
enum StorageKey {
    /// Admin address (authorized to create orgs/departments)
    Admin,
    /// Initialization flag
    Initialized,
    /// Next organization ID
    NextOrgId,
    /// Next department ID (global)
    NextDeptId,
    /// Organization data: org_id -> Organization
    Organization(u128),
    /// Department data: dept_id -> Department
    Department(u128),
    /// Department IDs under an organization: org_id -> Vec<u128>
    OrgDepartments(u128),
    /// Department children: parent_dept_id -> Vec<u128>
    DepartmentChildren(u128),
    /// Employee assignments: (dept_id, employee_address) -> ()
    EmployeeInDepartment(u128, Address),
    /// Department ID for an employee in an org: (org_id, employee) -> dept_id
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
    /// Initializes the contract. Callable once by the deployer.
    ///
    /// # Arguments
    /// * `admin` - Address that will be the admin (must authenticate)
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        let initialized: bool = env
            .storage()
            .persistent()
            .get(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(!initialized, "Already initialized");
        env.storage().persistent().set(&StorageKey::Admin, &admin);
        env.storage().persistent().set(&StorageKey::Initialized, &true);
        env.storage().persistent().set(&StorageKey::NextOrgId, &1u128);
        env.storage().persistent().set(&StorageKey::NextDeptId, &1u128);
    }

    /// Creates a new organization.
    ///
    /// # Arguments
    /// * `owner` - Caller (must authenticate); becomes org owner
    /// * `name` - Symbol name for the organization
    /// # Returns
    /// The new organization ID
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
        next_id
    }

    /// Creates a department under an organization (top-level or under a parent department).
    ///
    /// # Arguments
    /// * `caller` - Must be org owner (must authenticate)
    /// * `org_id` - Organization ID
    /// * `name` - Symbol name for the department
    /// * `parent_id` - Optional parent department ID for hierarchy; None for top-level
    /// # Returns
    /// The new department ID
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

        let mut org_depts: Vec<u128> = env
            .storage()
            .persistent()
            .get(&StorageKey::OrgDepartments(org_id))
            .unwrap_or_else(|| Vec::new(&env));
        org_depts.push_back(next_id);
        env.storage()
            .persistent()
            .set(&StorageKey::OrgDepartments(org_id), &org_depts);

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

        let empty_employees: Vec<Address> = Vec::new(&env);
        env.storage()
            .persistent()
            .set(&StorageKey::DepartmentEmployees(next_id), &empty_employees);

        next_id
    }

    /// Assigns an employee (address) to a department. Re-assigning overwrites previous department in same org.
    ///
    /// # Arguments
    /// * `caller` - Must be org owner (must authenticate)
    /// * `org_id` - Organization ID
    /// * `department_id` - Department ID
    /// * `employee` - Employee address to assign
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

        // Remove from previous department in this org if any
        if let Some(old_dept) = env
            .storage()
            .persistent()
            .get::<_, u128>(&StorageKey::EmployeeDepartment(employee.clone(), org_id))
        {
            Self::remove_employee_from_department_internal(&env, old_dept, &employee);
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
        employees.push_back(employee);
        env.storage()
            .persistent()
            .set(&StorageKey::DepartmentEmployees(department_id), &employees);
    }

    fn remove_employee_from_department_internal(env: &Env, department_id: u128, employee: &Address) {
        let key = StorageKey::EmployeeInDepartment(department_id, employee.clone());
        env.storage().persistent().remove(&key);

        let mut employees: Vec<Address> = env
            .storage()
            .persistent()
            .get(&StorageKey::DepartmentEmployees(department_id))
            .unwrap_or_else(|| Vec::new(env));
        let mut i = 0u32;
        while i < employees.len() {
            if employees.get(i).as_ref().map(|a| a == employee).unwrap_or(false) {
                employees.remove(i);
                break;
            }
            i += 1;
        }
        env.storage()
            .persistent()
            .set(&StorageKey::DepartmentEmployees(department_id), &employees);
    }

    /// Returns the organization record.
    pub fn get_organization(env: Env, org_id: u128) -> Organization {
        env.storage()
            .persistent()
            .get(&StorageKey::Organization(org_id))
            .expect("Organization not found")
    }

    /// Returns the department record.
    pub fn get_department(env: Env, department_id: u128) -> Department {
        env.storage()
            .persistent()
            .get(&StorageKey::Department(department_id))
            .expect("Department not found")
    }

    /// Returns department IDs for an organization (top-level only or all if flatten not set).
    pub fn get_org_departments(env: Env, org_id: u128) -> Vec<u128> {
        env.storage()
            .persistent()
            .get(&StorageKey::OrgDepartments(org_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns employee addresses in a department (department-level reporting).
    pub fn get_department_employees(env: Env, department_id: u128) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&StorageKey::DepartmentEmployees(department_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns the department ID for an employee in an org, or None if not assigned.
    pub fn get_employee_department(env: Env, employee: Address, org_id: u128) -> Option<u128> {
        env.storage()
            .persistent()
            .get(&StorageKey::EmployeeDepartment(employee, org_id))
    }

    /// Department-level report: employee count and child department IDs.
    pub fn get_department_report(
        env: Env,
        department_id: u128,
    ) -> (u32, Vec<u128>, Vec<Address>) {
        let employees: Vec<Address> = Self::get_department_employees(env.clone(), department_id);
        let children: Vec<u128> = env
            .storage()
            .persistent()
            .get(&StorageKey::DepartmentChildren(department_id))
            .unwrap_or_else(|| Vec::new(&env));
        (employees.len(), children, employees)
    }

    fn require_initialized(env: &Env) {
        let initialized: bool = env
            .storage()
            .persistent()
            .get(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(initialized, "Contract not initialized");
    }
}
