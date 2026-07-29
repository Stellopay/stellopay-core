#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Vec};
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
    /// Expiration timestamp must be in the future.
    InvalidExpiration = 3,
}

/// Role grant assignment with an optional expiration timestamp.
///
/// If `expires_at` is `None`, the grant remains valid indefinitely.
/// If `expires_at` is `Some(ts)`, the grant is valid while `current_timestamp < ts`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleGrant {
    pub role: BuiltInRole,
    pub expires_at: Option<u64>,
}

impl RoleGrant {
    /// Returns whether this role grant is currently active at `current_timestamp`.
    pub fn is_active(&self, current_timestamp: u64) -> bool {
        match self.expires_at {
            None => true,
            Some(exp) => current_timestamp < exp,
        }
    }
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
    /// Mapping: BuiltInRole -> Vec<BuiltInRole>
    RoleImplies(BuiltInRole),
}

/// Event emitted when an employee's role changes (assigned or revoked).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleChangedEvent {
    /// Previous role value, or None if a new role was assigned.
    pub old_role: Option<BuiltInRole>,
    /// New role value, or None if a role was revoked.
    pub new_role: Option<BuiltInRole>,
    /// Address that authorized the change (authenticated via require_auth).
    pub changed_by: Address,
    /// Employee whose role was changed.
    pub employee: Address,
    /// Ledger timestamp of the change.
    pub timestamp: u64,
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

    /// Internal helper: retrieves all role grants for `employee`.
    fn get_grants(env: &Env, employee: &Address) -> Vec<RoleGrant> {
        env.storage()
            .persistent()
            .get::<_, Vec<RoleGrant>>(&StorageKey::EmployeeRoles(employee.clone()))
            .unwrap_or(Vec::new(env))
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
        Self::assign_role_with_expiration(env, caller, employee, role, None)
    }

    /// Assigns a built-in role to an employee with an optional expiration timestamp.
    ///
    /// # Access Control
    /// - Caller must be the owner or hold the `Admin` role.
    ///
    /// # Arguments
    /// * `caller` - caller parameter
    /// * `employee` - employee parameter
    /// * `role` - role parameter
    /// * `expires_at` - optional expiration timestamp in seconds since unix epoch
    ///
    /// # Returns
    /// Result<(), RoleError>
    ///
    /// # Errors
    /// Returns `RoleError::Unauthorized` if authorization fails, or
    /// `RoleError::InvalidExpiration` if `expires_at` is in the past/present.
    pub fn assign_role_with_expiration(
        env: Env,
        caller: Address,
        employee: Address,
        role: BuiltInRole,
        expires_at: Option<u64>,
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
                .set(&StorageKey::EmployeeRoles(employee.clone()), &roles);

            let event = RoleChangedEvent {
                old_role: None,
                new_role: Some(role),
                changed_by: caller,
                employee: employee,
                timestamp: env.ledger().timestamp(),
            };
            env.events().publish(
                (symbol_short!("ROLE"), symbol_short!("chng")),
                &event,
            );
        }

        env.storage()
            .persistent()
            .set(&StorageKey::EmployeeRoles(employee), &updated_grants);

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

        let had_role = roles.iter().any(|r| r == role);

        let mut filtered = Vec::new(&env);
        for g in grants.iter() {
            if g.role != role {
                filtered.push_back(g);
            }
        }

        env.storage()
            .persistent()
            .set(&StorageKey::EmployeeRoles(employee.clone()), &filtered);

        if had_role {
            let event = RoleChangedEvent {
                old_role: Some(role),
                new_role: None,
                changed_by: caller,
                employee: employee,
                timestamp: env.ledger().timestamp(),
            };
            env.events().publish(
                (symbol_short!("ROLE"), symbol_short!("chng")),
                &event,
            );
        }

        Ok(())
    }

    /// Returns all active roles currently assigned to an employee.
    ///
    /// Expired grants are automatically filtered out.
    ///
    /// # Arguments
    /// * `employee` - employee parameter
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_roles(env: Env, employee: Address) -> Vec<BuiltInRole> {
        let grants = Self::get_grants(&env, &employee);
        let current_ts = env.ledger().timestamp();
        let mut active_roles = Vec::new(&env);
        for g in grants.iter() {
            if g.is_active(current_ts) {
                active_roles.push_back(g.role);
            }
        }
        active_roles
    }

    /// Returns all role grants (including expiration details) for an employee.
    ///
    /// # Arguments
    /// * `employee` - employee parameter
    pub fn get_role_grants(env: Env, employee: Address) -> Vec<RoleGrant> {
        Self::get_grants(&env, &employee)
    }

    /// Checks whether `employee` has a specific built-in role.
    ///
    /// Expiration is evaluated dynamically against the current ledger timestamp.
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
        let grants = Self::get_grants(&env, &employee);
        let current_ts = env.ledger().timestamp();

        if grants
            .iter()
            .any(|g| g.role == role && g.is_active(current_ts))
        {
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
    /// Expiration is evaluated dynamically against the current ledger timestamp.
    ///
    /// # Arguments
    /// * `employee` - employee parameter
    /// * `required` - required parameter
    ///
    /// # Returns
    /// bool
    pub fn has_role_at_least(env: Env, employee: Address, required: BuiltInRole) -> bool {
        let grants = Self::get_grants(&env, &employee);
        let current_ts = env.ledger().timestamp();
        let required_level = required as u32;

        if grants
            .iter()
            .any(|g| (g.role as u32) >= required_level && g.is_active(current_ts))
        {
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
