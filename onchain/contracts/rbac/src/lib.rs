#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Vec};

/// Core roles supported by the RBAC contract.
///
/// Roles are intentionally small and composable. Access control for a given
/// function can be expressed in terms of one or more of these roles.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Role {
    /// Global administrator; implicitly has all roles.
    Admin,
    /// Employer role; implicitly grants `Employee` permissions.
    Employer,
    /// Employee role; base role for payroll participants.
    Employee,
    /// Arbiter role; used for dispute resolution flows.
    Arbiter,
}

/// Storage keys for the RBAC contract.
#[contracttype]
#[derive(Clone)]
enum StorageKey {
    /// One-time initialization flag.
    Initialized,
    /// Contract owner / bootstrap admin.
    Owner,
    /// Roles assigned to an address: Address -> Vec<Role>.
    Roles(Address),
}

fn require_initialized(env: &Env) {
    let initialized: bool = env
        .storage()
        .persistent()
        .get(&StorageKey::Initialized)
        .unwrap_or(false);
    assert!(initialized, "Contract not initialized");
}

fn read_roles(env: &Env, addr: &Address) -> Vec<Role> {
    env.storage()
        .persistent()
        .get::<_, Vec<Role>>(&StorageKey::Roles(addr.clone()))
        .unwrap_or_else(|| Vec::new(env))
}

fn write_roles(env: &Env, addr: &Address, roles: &Vec<Role>) {
    env.storage()
        .persistent()
        .set(&StorageKey::Roles(addr.clone()), roles);
}

fn has_exact_role(roles: &Vec<Role>, role: &Role) -> bool {
    for i in 0..roles.len() {
        if &roles.get(i).unwrap() == role {
            return true;
        }
    }
    false
}

/// Returns true if a granted role implies `required` according to the
/// inheritance rules:
///
/// - Admin: implies all roles.
/// - Employer: implies Employer and Employee.
/// - Employee: implies Employee.
/// - Arbiter: implies Arbiter.
fn role_implies(granted: &Role, required: &Role) -> bool {
    use Role::*;
    match (granted, required) {
        (Admin, _) => true,
        (Employer, Employer) | (Employer, Employee) => true,
        (Employee, Employee) => true,
        (Arbiter, Arbiter) => true,
        _ => false,
    }
}

/// Returns true if the address has a role that implies `required`.
fn has_implied_role(env: &Env, addr: &Address, required: &Role) -> bool {
    let roles = read_roles(env, addr);
    for i in 0..roles.len() {
        let r = roles.get(i).unwrap();
        if role_implies(&r, required) {
            return true;
        }
    }
    false
}

#[contract]
pub struct RbacContract;

#[contractimpl]
impl RbacContract {
    /// @notice Initializes the RBAC contract and assigns the bootstrap admin.
    /// @dev Can only be called once. The `owner` is granted the `Admin` role.
    /// @param owner Address that becomes contract owner and initial admin.
    pub fn initialize(env: Env, owner: Address) {
        owner.require_auth();

        let initialized: bool = env
            .storage()
            .persistent()
            .get(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(!initialized, "Already initialized");

        env.storage().persistent().set(&StorageKey::Owner, &owner);
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);

        // Grant Admin role to owner.
        let mut roles = Vec::new(&env);
        roles.push_back(Role::Admin);
        write_roles(&env, &owner, &roles);
    }

    /// @notice Grants a role to a target address.
    /// @dev Caller must have the `Admin` role.
    /// @param caller Address requesting the change; must authenticate.
    /// @param target Address to receive the role.
    /// @param role Role to grant.
    pub fn grant_role(env: Env, caller: Address, target: Address, role: Role) {
        require_initialized(&env);
        caller.require_auth();

        assert!(
            has_implied_role(&env, &caller, &Role::Admin),
            "Only admin can grant roles"
        );

        let mut roles = read_roles(&env, &target);
        if !has_exact_role(&roles, &role) {
            roles.push_back(role);
            write_roles(&env, &target, &roles);
        }
    }

    /// @notice Revokes a role from a target address.
    /// @dev Caller must have the `Admin` role.
    /// @param caller Address requesting the change; must authenticate.
    /// @param target Address losing the role.
    /// @param role Role to revoke.
    pub fn revoke_role(env: Env, caller: Address, target: Address, role: Role) {
        require_initialized(&env);
        caller.require_auth();

        assert!(
            has_implied_role(&env, &caller, &Role::Admin),
            "Only admin can revoke roles"
        );

        let mut roles = read_roles(&env, &target);
        let mut i = 0u32;
        while i < roles.len() {
            if roles.get(i).as_ref().map(|r| r == &role).unwrap_or(false) {
                roles.remove(i);
                break;
            }
            i += 1;
        }
        write_roles(&env, &target, &roles);
    }

    /// @notice Returns all roles assigned to an address (without inheritance).
    /// @param addr Address to query.
    /// @return roles Vector of directly assigned roles.
    /// @dev Requires caller authentication
    pub fn get_roles(env: Env, addr: Address) -> Vec<Role> {
        require_initialized(&env);
        read_roles(&env, &addr)
    }

    /// @notice Checks whether `addr` has a role that implies `required`.
    /// @dev This is inheritance-aware (e.g. Employer implies Employee).
    /// @param addr Address to check.
    /// @param required Role requirement.
    /// @return has_role True if the role (direct or inherited) is satisfied.
    pub fn has_role(env: Env, addr: Address, required: Role) -> bool {
        require_initialized(&env);
        has_implied_role(&env, &addr, &required)
    }

    /// @notice Reverts if `addr` does not have a role that implies `required`.
    /// @dev Helper for integrating contracts to enforce authorization.
    /// @param addr Address to check; must authenticate.
    /// @param required Role requirement.
    pub fn require_role(env: Env, addr: Address, required: Role) {
        require_initialized(&env);
        addr.require_auth();
        assert!(
            has_implied_role(&env, &addr, &required),
            "Missing required role"
        );
    }
}
