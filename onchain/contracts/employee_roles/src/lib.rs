#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol, Vec};
use stellar_contract_utils::upgradeable::UpgradeableInternal;
use stellar_macros::Upgradeable;

/// Role-based access control errors.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum RoleError {
    /// Caller is not authorized to modify roles.
    Unauthorized = 1,
    /// Invalid or unknown role name.
    InvalidRole = 2,
}

/// Built-in hierarchical roles.
///
/// Higher ordinal values represent strictly higher privileges:
/// Admin > Manager > Employee.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq, Copy)]
pub enum BuiltInRole {
    Employee = 1,
    Manager = 2,
    Admin = 3,
}

/// Storage keys for the employee roles contract.
#[contracttype]
#[derive(Clone)]
pub enum StorageKey {
    /// Contract owner (top-level admin).
    Owner,
    /// Mapping: employee address -> Vec<BuiltInRole>
    EmployeeRoles(Address),
}

/// Employee Roles Contract
///
/// Provides hierarchical role management for employees, with simple
/// role-based permission checks suitable for payroll and HR workflows.
#[derive(Upgradeable)]
#[contract]
pub struct EmployeeRolesContract;

impl UpgradeableInternal for EmployeeRolesContract {
    fn _require_auth(e: &Env, _operator: &Address) {
        let owner: Address = e.storage().persistent().get(&StorageKey::Owner).unwrap();
        owner.require_auth();
    }
}

#[contractimpl]
impl EmployeeRolesContract {
    /// Initializes the roles contract.
    ///
    /// # Arguments
    /// * `owner` - Initial owner account with full admin privileges.
    pub fn initialize(env: Env, owner: Address) {
        owner.require_auth();
        env.storage().persistent().set(&StorageKey::Owner, &owner);
    }

    /// Assigns a built-in role to an employee.
    ///
    /// # Access Control
    /// - Caller must be the owner or hold the `Admin` role.
    pub fn assign_role(
        env: Env,
        caller: Address,
        employee: Address,
        role: BuiltInRole,
    ) -> Result<(), RoleError> {
        Self::require_role_admin(&env, &caller)?;

        let mut roles: Vec<BuiltInRole> = env
            .storage()
            .persistent()
            .get(&StorageKey::EmployeeRoles(employee.clone()))
            .unwrap_or(Vec::new(&env));

        if !roles.iter().any(|r| r == role) {
            roles.push_back(role);
            env.storage()
                .persistent()
                .set(&StorageKey::EmployeeRoles(employee), &roles);
        }

        Ok(())
    }

    /// Revokes a built-in role from an employee.
    ///
    /// # Access Control
    /// - Caller must be the owner or hold the `Admin` role.
    pub fn revoke_role(
        env: Env,
        caller: Address,
        employee: Address,
        role: BuiltInRole,
    ) -> Result<(), RoleError> {
        Self::require_role_admin(&env, &caller)?;

        let mut roles: Vec<BuiltInRole> = env
            .storage()
            .persistent()
            .get(&StorageKey::EmployeeRoles(employee.clone()))
            .unwrap_or(Vec::new(&env));

        let mut filtered = Vec::new(&env);
        for r in roles.iter() {
            if r != role {
                filtered.push_back(r);
            }
        }

        env.storage()
            .persistent()
            .set(&StorageKey::EmployeeRoles(employee), &filtered);

        Ok(())
    }

    /// Returns all roles currently assigned to an employee.
    pub fn get_roles(env: Env, employee: Address) -> Vec<BuiltInRole> {
        env.storage()
            .persistent()
            .get(&StorageKey::EmployeeRoles(employee))
            .unwrap_or(Vec::new(&env))
    }

    /// Checks whether `employee` has a specific built-in role.
    pub fn has_role(env: Env, employee: Address, role: BuiltInRole) -> bool {
        let roles: Vec<BuiltInRole> = env
            .storage()
            .persistent()
            .get(&StorageKey::EmployeeRoles(employee))
            .unwrap_or(Vec::new(&env));

        roles.iter().any(|r| r == role)
    }

    /// Checks whether `employee` has at least the required role in the
    /// hierarchy (e.g. Admin satisfies Manager and Employee).
    pub fn has_role_at_least(env: Env, employee: Address, required: BuiltInRole) -> bool {
        let roles: Vec<BuiltInRole> = env
            .storage()
            .persistent()
            .get(&StorageKey::EmployeeRoles(employee))
            .unwrap_or(Vec::new(&env));

        let required_level = required as u32;
        roles.iter().any(|r| (r as u32) >= required_level)
    }

    /// Internal helper: require that `caller` is allowed to manage roles.
    fn require_role_admin(env: &Env, caller: &Address) -> Result<(), RoleError> {
        caller.require_auth();

        let owner: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::Owner)
            .ok_or(RoleError::Unauthorized)?;

        if *caller == owner {
            return Ok(());
        }

        let is_admin = Self::has_role(env.clone(), caller.clone(), BuiltInRole::Admin);
        if !is_admin {
            return Err(RoleError::Unauthorized);
        }

        Ok(())
    }
}

