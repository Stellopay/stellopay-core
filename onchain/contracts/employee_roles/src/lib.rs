#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Vec};
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
///
/// ## Capability Mapping (NatSpec)
/// | Role   | Allowed Actions                                                                 |
/// |--------|----------------------------------------------------------------------------------|
/// | Employee | ViewPayrollStatus, ViewPayrollHistory, ClaimOwnPayroll, WithdrawOwnPayroll   |
/// | Manager  | + CreatePayrollRecord, UpdatePayrollRecord, PauseEmployeePayroll, ResumeEmployeePayroll |
/// | Admin    | + AssignRoles, RevokeRoles, EmergencyPause, EmergencyUnpause                   |
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq, Copy)]
pub enum BuiltInRole {
    Employee = 1,
    Manager = 2,
    Admin = 3,
}

/// Payroll actions that can be permission-checked via role hierarchy.
///
/// Each action maps to a minimum required role. Admin implicitly satisfies
/// all lower-level capabilities.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq, Copy)]
pub enum PayrollAction {
    /// View own payroll status (Employee+).
    ViewPayrollStatus = 1,
    /// View own payroll history (Employee+).
    ViewPayrollHistory = 2,
    /// Claim/withdraw own payroll (Employee+).
    ClaimOwnPayroll = 3,
    /// Withdraw own payroll funds (Employee+).
    WithdrawOwnPayroll = 4,
    /// Create payroll records for team (Manager+).
    CreatePayrollRecord = 5,
    /// Update payroll records for team (Manager+).
    UpdatePayrollRecord = 6,
    /// Pause employee payroll (Manager+).
    PauseEmployeePayroll = 7,
    /// Resume employee payroll (Manager+).
    ResumeEmployeePayroll = 8,
    /// Assign roles to employees (Admin+).
    AssignRoles = 9,
    /// Revoke roles from employees (Admin+).
    RevokeRoles = 10,
    /// Emergency pause (Admin+).
    EmergencyPause = 11,
    /// Emergency unpause (Admin+).
    EmergencyUnpause = 12,
}

/// Storage keys for the employee roles contract.
#[contracttype]
#[derive(Clone)]
pub enum StorageKey {
    /// Contract owner (top-level admin).
    Owner,
    /// Mapping: employee address -> Vec<BuiltInRole>
    EmployeeRoles(Address),
    /// Linked RBAC contract address.
    RbacAddress,
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
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn initialize(env: Env, owner: Address) {
        owner.require_auth();
        if env.storage().persistent().has(&StorageKey::Owner) {
            panic!("Already initialized");
        }
        env.storage().persistent().set(&StorageKey::Owner, &owner);
    }

    /// Sets the linked RBAC contract address for centralized role checks.
    ///
    /// # Arguments
    /// * `rbac_address` - Address of the RBAC contract.
    ///
    /// # Access Control
    /// - Caller must be the contract owner.
    pub fn set_rbac_address(env: Env, rbac_address: Address) {
        let owner: Address = env.storage().persistent().get(&StorageKey::Owner).unwrap();
        owner.require_auth();
        env.storage()
            .persistent()
            .set(&StorageKey::RbacAddress, &rbac_address);
    }

    /// Assigns a built-in role to an employee.
    ///
    /// # Access Control
    /// - Caller must be the owner or hold the `Admin` role.
    ///
    /// # Arguments
    /// * `caller` - caller parameter
    /// * `employee` - employee parameter
    /// * `role` - role parameter
    ///
    /// # Returns
    /// Result<(), RoleError>
    ///
    /// # Errors
    /// Returns an error if validation fails
    pub fn assign_role(
        env: Env,
        caller: Address,
        employee: Address,
        role: BuiltInRole,
    ) -> Result<(), RoleError> {
        Self::require_role_admin(&env, &caller)?;

        // Escalation safeguard: non-owner caller must have at least the role being assigned.
        let owner: Address = env.storage().persistent().get(&StorageKey::Owner).unwrap();
        if caller != owner && !Self::has_role_at_least(env.clone(), caller.clone(), role) {
            return Err(RoleError::Unauthorized);
        }

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
    ///
    /// # Arguments
    /// * `caller` - caller parameter
    /// * `employee` - employee parameter
    /// * `role` - role parameter
    ///
    /// # Returns
    /// Result<(), RoleError>
    ///
    /// # Errors
    /// Returns an error if validation fails
    pub fn revoke_role(
        env: Env,
        caller: Address,
        employee: Address,
        role: BuiltInRole,
    ) -> Result<(), RoleError> {
        Self::require_role_admin(&env, &caller)?;

        // Escalation safeguard: non-owner caller must have at least the role being revoked.
        let owner: Address = env.storage().persistent().get(&StorageKey::Owner).unwrap();
        if caller != owner && !Self::has_role_at_least(env.clone(), caller.clone(), role) {
            return Err(RoleError::Unauthorized);
        }

        let roles: Vec<BuiltInRole> = env
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
    ///
    /// # Arguments
    /// * `employee` - employee parameter
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_roles(env: Env, employee: Address) -> Vec<BuiltInRole> {
        env.storage()
            .persistent()
            .get(&StorageKey::EmployeeRoles(employee))
            .unwrap_or(Vec::new(&env))
    }

    /// Checks whether `employee` has a specific built-in role.
    ///
    /// # Arguments
    /// * `employee` - employee parameter
    /// * `role` - role parameter
    ///
    /// # Returns
    /// bool
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn has_role(env: Env, employee: Address, role: BuiltInRole) -> bool {
        let roles: Vec<BuiltInRole> = env
            .storage()
            .persistent()
            .get(&StorageKey::EmployeeRoles(employee.clone()))
            .unwrap_or(Vec::new(&env));

        if roles.iter().any(|r| r == role) {
            return true;
        }

        // Fallback to RBAC if linked
        if let Some(rbac_address) = env
            .storage()
            .persistent()
            .get::<_, Address>(&StorageKey::RbacAddress)
        {
            if let Some(rbac_role) = Self::map_to_rbac_role(role) {
                let rbac_client = rbac::RbacContractClient::new(&env, &rbac_address);
                // We use inheritance-aware check from RBAC if exact role not found locally
                if rbac_client.has_role(&employee, &rbac_role) {
                    return true;
                }
            }
        }

        false
    }

    /// Checks whether `employee` has at least the required role in the
    /// hierarchy (e.g. Admin satisfies Manager and Employee).
    ///
    /// # Arguments
    /// * `employee` - employee parameter
    /// * `required` - required parameter
    ///
    /// # Returns
    /// bool
    pub fn has_role_at_least(env: Env, employee: Address, required: BuiltInRole) -> bool {
        let roles: Vec<BuiltInRole> = env
            .storage()
            .persistent()
            .get(&StorageKey::EmployeeRoles(employee.clone()))
            .unwrap_or(Vec::new(&env));

        let required_level = required as u32;
        if roles.iter().any(|r| (r as u32) >= required_level) {
            return true;
        }

        // Fallback to RBAC if linked
        if let Some(rbac_address) = env
            .storage()
            .persistent()
            .get::<_, Address>(&StorageKey::RbacAddress)
        {
            if let Some(rbac_role) = Self::map_to_rbac_role(required) {
                let rbac_client = rbac::RbacContractClient::new(&env, &rbac_address);
                if rbac_client.has_role(&employee, &rbac_role) {
                    return true;
                }
            }
        }

        false
    }

    /// Checks whether `employee` can perform the given payroll action.
    ///
    /// Owner and Admin can perform all actions; Manager can perform
    /// Manager- and Employee-level actions; Employee can perform
    /// Employee-level actions only.
    ///
    /// # Arguments
    /// * `employee` - Employee address to check
    /// * `action` - Payroll action to authorize
    ///
    /// # Returns
    /// `true` if the employee has sufficient role for the action.
    pub fn can_perform(env: Env, employee: Address, action: PayrollAction) -> bool {
        let owner: Option<Address> = env.storage().persistent().get(&StorageKey::Owner);
        if owner.as_ref() == Some(&employee) {
            return true;
        }

        let required = Self::action_minimum_role(action);
        Self::has_role_at_least(env, employee, required)
    }

    /// Enforces that `employee` can perform the given action; returns error if not.
    ///
    /// Use this in integrating contracts to gate payroll operations.
    ///
    /// # Arguments
    /// * `employee` - Employee address (must be authenticated)
    /// * `action` - Payroll action to authorize
    ///
    /// # Errors
    /// Returns `RoleError::Unauthorized` if the employee lacks the required role.
    pub fn require_capability(
        env: Env,
        employee: Address,
        action: PayrollAction,
    ) -> Result<(), RoleError> {
        employee.require_auth();
        if Self::can_perform(env.clone(), employee.clone(), action) {
            Ok(())
        } else {
            Err(RoleError::Unauthorized)
        }
    }

    /// Maps a payroll action to its minimum required role.
    fn action_minimum_role(action: PayrollAction) -> BuiltInRole {
        match action {
            PayrollAction::ViewPayrollStatus
            | PayrollAction::ViewPayrollHistory
            | PayrollAction::ClaimOwnPayroll
            | PayrollAction::WithdrawOwnPayroll => BuiltInRole::Employee,
            PayrollAction::CreatePayrollRecord
            | PayrollAction::UpdatePayrollRecord
            | PayrollAction::PauseEmployeePayroll
            | PayrollAction::ResumeEmployeePayroll => BuiltInRole::Manager,
            PayrollAction::AssignRoles
            | PayrollAction::RevokeRoles
            | PayrollAction::EmergencyPause
            | PayrollAction::EmergencyUnpause => BuiltInRole::Admin,
        }
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

    /// Maps a BuiltInRole to a centralized RBAC Role.
    fn map_to_rbac_role(role: BuiltInRole) -> Option<rbac::Role> {
        match role {
            BuiltInRole::Employee => Some(rbac::Role::Employee),
            BuiltInRole::Manager => Some(rbac::Role::Employer),
            BuiltInRole::Admin => Some(rbac::Role::Admin),
        }
    }
}
