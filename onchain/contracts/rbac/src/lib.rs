#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Vec};

// ---------------------------------------------------------------------------
// Roles
// ---------------------------------------------------------------------------

/// Core roles supported by the RBAC contract.
///
/// Roles are intentionally small and composable. Access control for a given
/// function can be expressed in terms of one or more of these roles.
///
/// ## Inheritance
///
/// | Granted   | Implies                              |
/// |-----------|--------------------------------------|
/// | Admin     | Admin, Employer, Employee, Arbiter   |
/// | Employer  | Employer, Employee                   |
/// | Employee  | Employee                             |
/// | Arbiter   | Arbiter                              |
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

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Storage keys for the RBAC contract.
#[contracttype]
#[derive(Clone)]
enum StorageKey {
    /// One-time initialization flag.
    Initialized,
    /// Contract owner / bootstrap admin.
    Owner,
    /// Pending owner for two-step ownership transfer.
    PendingOwner,
    /// Roles assigned to an address: Address -> Vec<Role>.
    Roles(Address),
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// @dev Reverts if the contract has not been initialized.
fn require_initialized(env: &Env) {
    let initialized: bool = env
        .storage()
        .persistent()
        .get(&StorageKey::Initialized)
        .unwrap_or(false);
    assert!(initialized, "Contract not initialized");
}

/// @dev Reads the role vector for `addr`. Returns an empty vector when
///      no roles have been assigned.
fn read_roles(env: &Env, addr: &Address) -> Vec<Role> {
    env.storage()
        .persistent()
        .get::<_, Vec<Role>>(&StorageKey::Roles(addr.clone()))
        .unwrap_or_else(|| Vec::new(env))
}

/// @dev Persists the role vector for `addr`.
fn write_roles(env: &Env, addr: &Address, roles: &Vec<Role>) {
    env.storage()
        .persistent()
        .set(&StorageKey::Roles(addr.clone()), roles);
}

/// @dev Returns true if `role` appears directly in the vector.
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
/// - Admin: implies **all** roles.
/// - Employer: implies Employer and Employee.
/// - Employee: implies Employee only.
/// - Arbiter: implies Arbiter only.
pub fn role_implies(granted: &Role, required: &Role) -> bool {
    use Role::*;
    match (granted, required) {
        (Admin, _) => true,
        (Employer, Employer) | (Employer, Employee) => true,
        (Employee, Employee) => true,
        (Arbiter, Arbiter) => true,
        _ => false,
    }
}

/// @dev Returns true if `addr` holds any role that implies `required`.
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

/// @dev Reads the current contract owner.
fn read_owner(env: &Env) -> Address {
    env.storage()
        .persistent()
        .get::<_, Address>(&StorageKey::Owner)
        .expect("Owner not set")
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct RbacContract;

#[contractimpl]
impl RbacContract {
    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    /// @notice Initializes the RBAC contract and assigns the bootstrap admin.
    /// @dev Can only be called once. The `owner` is granted the `Admin` role.
    ///      Emits event `("RBAC", "init")` with the owner address.
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

        let mut roles = Vec::new(&env);
        roles.push_back(Role::Admin);
        write_roles(&env, &owner, &roles);

        env.events()
            .publish((symbol_short!("RBAC"), symbol_short!("init")), &owner);
    }

    // -----------------------------------------------------------------------
    // Role management
    // -----------------------------------------------------------------------

    /// @notice Grants a role to a target address.
    /// @dev Caller must have the `Admin` role. Duplicate grants are no-ops.
    ///      Emits event `("RBAC", "grant")` with `(target, role)`.
    /// @param caller Address requesting the change; must authenticate.
    /// @param target Address to receive the role.
    /// @param role   Role to grant.
    pub fn grant_role(env: Env, caller: Address, target: Address, role: Role) {
        require_initialized(&env);
        caller.require_auth();

        assert!(
            has_implied_role(&env, &caller, &Role::Admin),
            "Only admin can grant roles"
        );

        let mut roles = read_roles(&env, &target);
        if !has_exact_role(&roles, &role) {
            roles.push_back(role.clone());
            write_roles(&env, &target, &roles);
        }

        env.events().publish(
            (symbol_short!("RBAC"), symbol_short!("grant")),
            (&target, &role),
        );
    }

    /// @notice Revokes a role from a target address.
    /// @dev Caller must have the `Admin` role. The owner's `Admin` role
    ///      cannot be revoked (prevents lockout). Revoking a role the
    ///      target doesn't have is a no-op.
    ///      Emits event `("RBAC", "revoke")` with `(target, role)`.
    /// @param caller Address requesting the change; must authenticate.
    /// @param target Address losing the role.
    /// @param role   Role to revoke.
    pub fn revoke_role(env: Env, caller: Address, target: Address, role: Role) {
        require_initialized(&env);
        caller.require_auth();

        assert!(
            has_implied_role(&env, &caller, &Role::Admin),
            "Only admin can revoke roles"
        );

        // Prevent revoking Admin from the contract owner – avoids lockout.
        let owner = read_owner(&env);
        assert!(
            !(target == owner && role == Role::Admin),
            "Cannot revoke Admin from owner"
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

        env.events().publish(
            (symbol_short!("RBAC"), symbol_short!("revoke")),
            (&target, &role),
        );
    }

    /// @notice Grants multiple roles to a target in a single call.
    /// @dev Caller must have `Admin` role. Duplicates within the batch
    ///      or already-held roles are silently skipped.
    /// @param caller Address requesting the change; must authenticate.
    /// @param target Address to receive the roles.
    /// @param roles  Vector of roles to grant.
    pub fn bulk_grant(env: Env, caller: Address, target: Address, roles_to_grant: Vec<Role>) {
        require_initialized(&env);
        caller.require_auth();

        assert!(
            has_implied_role(&env, &caller, &Role::Admin),
            "Only admin can grant roles"
        );

        let mut current = read_roles(&env, &target);
        for i in 0..roles_to_grant.len() {
            let role = roles_to_grant.get(i).unwrap();
            if !has_exact_role(&current, &role) {
                current.push_back(role);
            }
        }
        write_roles(&env, &target, &current);
    }

    /// @notice Revokes all roles from a target address.
    /// @dev Caller must have `Admin` role. Cannot be used on the contract
    ///      owner (prevents lockout).
    /// @param caller Address requesting the change; must authenticate.
    /// @param target Address to strip of all roles.
    pub fn revoke_all(env: Env, caller: Address, target: Address) {
        require_initialized(&env);
        caller.require_auth();

        assert!(
            has_implied_role(&env, &caller, &Role::Admin),
            "Only admin can revoke roles"
        );

        let owner = read_owner(&env);
        assert!(target != owner, "Cannot revoke all roles from owner");

        write_roles(&env, &target, &Vec::new(&env));
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// @notice Returns all roles directly assigned to an address.
    /// @dev Does not include inherited roles; use `has_role` for
    ///      inheritance-aware checks.
    /// @param addr Address to query.
    /// @return roles Vector of directly assigned roles.
    pub fn get_roles(env: Env, addr: Address) -> Vec<Role> {
        require_initialized(&env);
        read_roles(&env, &addr)
    }

    /// @notice Checks whether `addr` has a role that implies `required`.
    /// @dev Inheritance-aware (e.g. Admin implies Arbiter).
    /// @param addr     Address to check.
    /// @param required Role requirement.
    /// @return True if the role (direct or inherited) is satisfied.
    pub fn has_role(env: Env, addr: Address, required: Role) -> bool {
        require_initialized(&env);
        has_implied_role(&env, &addr, &required)
    }

    /// @notice Reverts if `addr` does not have a role that implies `required`.
    /// @dev Helper for integrating contracts to enforce authorization inline.
    /// @param addr     Address to check; must authenticate.
    /// @param required Role requirement.
    pub fn require_role(env: Env, addr: Address, required: Role) {
        require_initialized(&env);
        addr.require_auth();
        assert!(
            has_implied_role(&env, &addr, &required),
            "Missing required role"
        );
    }

    /// @notice Returns the current contract owner.
    /// @return owner The owner address.
    pub fn owner(env: Env) -> Address {
        require_initialized(&env);
        read_owner(&env)
    }

    // -----------------------------------------------------------------------
    // Ownership transfer (two-step)
    // -----------------------------------------------------------------------

    /// @notice Proposes a new owner. Must be accepted via `accept_ownership`.
    /// @dev Only the current owner may call this. The pending owner is stored
    ///      but has no privileges until they accept.
    ///      Emits event `("RBAC", "propose")` with the new_owner address.
    /// @param caller    Current owner; must authenticate.
    /// @param new_owner Proposed new owner address.
    pub fn transfer_ownership(env: Env, caller: Address, new_owner: Address) {
        require_initialized(&env);
        caller.require_auth();

        let owner = read_owner(&env);
        assert!(caller == owner, "Only owner can transfer ownership");

        env.storage()
            .persistent()
            .set(&StorageKey::PendingOwner, &new_owner);

        env.events().publish(
            (symbol_short!("RBAC"), symbol_short!("propose")),
            &new_owner,
        );
    }

    /// @notice Accepts a pending ownership transfer.
    /// @dev The caller must be the pending owner. On acceptance:
    ///      1. Admin role is granted to the new owner.
    ///      2. Admin role is revoked from the old owner.
    ///      3. Ownership record is updated.
    ///      Emits event `("RBAC", "owner")` with the new owner address.
    /// @param caller Must be the pending owner; must authenticate.
    pub fn accept_ownership(env: Env, caller: Address) {
        require_initialized(&env);
        caller.require_auth();

        let pending: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::PendingOwner)
            .expect("No pending owner");
        assert!(caller == pending, "Caller is not pending owner");

        let old_owner = read_owner(&env);

        // Grant Admin to new owner.
        let mut new_roles = read_roles(&env, &caller);
        if !has_exact_role(&new_roles, &Role::Admin) {
            new_roles.push_back(Role::Admin);
            write_roles(&env, &caller, &new_roles);
        }

        // Revoke Admin from old owner.
        let mut old_roles = read_roles(&env, &old_owner);
        let mut i = 0u32;
        while i < old_roles.len() {
            if old_roles
                .get(i)
                .as_ref()
                .map(|r| r == &Role::Admin)
                .unwrap_or(false)
            {
                old_roles.remove(i);
                break;
            }
            i += 1;
        }
        write_roles(&env, &old_owner, &old_roles);

        // Update owner record.
        env.storage()
            .persistent()
            .set(&StorageKey::Owner, &caller);
        env.storage()
            .persistent()
            .remove(&StorageKey::PendingOwner);

        env.events()
            .publish((symbol_short!("RBAC"), symbol_short!("owner")), &caller);
    }
}
