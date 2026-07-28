//! Shared RBAC types and client for cross-contract calls.
//!
//! Depend on this crate (rlib only) from other contracts. Deploy the `rbac`
//! contract crate separately — do not link `rbac` as a cdylib dependency.

#![no_std]

use soroban_sdk::{contractclient, contracttype, Address, Env, Vec};

/// Core roles supported by the RBAC contract (must stay in sync with `rbac`).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Role {
    Admin,
    Employer,
    Employee,
    Arbiter,
}

/// Client for the deployed RBAC contract.
#[contractclient(name = "RbacContractClient")]
pub trait RbacContractInterface {
    /// One-time initialization. Assigns `owner` the `Admin` role.
    ///
    /// # Caller authorization
    /// - `owner` must sign (caller authenticates themselves).
    ///
    /// # Panics
    /// - `"Already initialized"` — if `initialize` has already been called.
    fn initialize(env: Env, owner: Address);

    /// Returns `true` when `addr` holds a role that implies `required`
    /// (inheritance-aware: `Admin` implies all roles; `Employer` implies
    /// `Employer` and `Employee`; other roles imply only themselves).
    ///
    /// # Caller authorization
    /// - Unauthenticated — any address may call.
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    fn has_role(env: Env, addr: Address, required: Role) -> bool;

    /// Returns all roles directly assigned to `addr` (no inheritance).
    ///
    /// # Caller authorization
    /// - Unauthenticated — any address may call.
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    fn get_roles(env: Env, addr: Address) -> Vec<Role>;

    /// Returns the current contract owner.
    ///
    /// # Caller authorization
    /// - Unauthenticated — any address may call.
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    /// - `"Owner not set"` — if no owner has been stored (defensive; should
    ///   not occur after a successful `initialize`).
    fn owner(env: Env) -> Address;

    /// Grants `role` to `target`.
    ///
    /// Duplicate grants are silently skipped.
    ///
    /// # Caller authorization
    /// - `caller` must sign and must hold a role that implies `Admin`
    ///   (i.e. must have the `Admin` role).
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    /// - `"Only admin can grant roles"` — if `caller` does not have a role
    ///   implying `Admin`.
    fn grant_role(env: Env, caller: Address, target: Address, role: Role);

    /// Revokes `role` from `target`.
    ///
    /// Revoking a role the target does not hold is a silent no-op. The
    /// contract owner's `Admin` role is protected and cannot be revoked
    /// (prevents lockout).
    ///
    /// # Caller authorization
    /// - `caller` must sign and must hold a role that implies `Admin`.
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    /// - `"Only admin can revoke roles"` — if `caller` does not have a role
    ///   implying `Admin`.
    /// - `"Cannot revoke Admin from owner"` — if `target` is the contract
    ///   owner and `role` is `Admin`.
    fn revoke_role(env: Env, caller: Address, target: Address, role: Role);

    /// Grants multiple roles to `target` in a single call.
    ///
    /// Duplicates within the batch or roles already held are silently
    /// skipped.
    ///
    /// # Caller authorization
    /// - `caller` must sign and must hold a role that implies `Admin`.
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    /// - `"Only admin can grant roles"` — if `caller` does not have a role
    ///   implying `Admin`.
    fn bulk_grant(env: Env, caller: Address, target: Address, roles: Vec<Role>);

    /// Revokes every role from `target`.
    ///
    /// The contract owner is protected — calling `revoke_all` on the owner
    /// address is forbidden (prevents lockout).
    ///
    /// # Caller authorization
    /// - `caller` must sign and must hold a role that implies `Admin`.
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    /// - `"Only admin can revoke roles"` — if `caller` does not have a role
    ///   implying `Admin`.
    /// - `"Cannot revoke all roles from owner"` — if `target` is the contract
    ///   owner.
    fn revoke_all(env: Env, caller: Address, target: Address);

    /// Reverts unless `addr` holds a role that implies `required`.
    ///
    /// Designed as an inline guard for integrating contracts.
    ///
    /// # Caller authorization
    /// - `addr` must sign (the address being checked authenticates
    ///   themselves).
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    /// - `"Missing required role"` — if `addr` does not hold a role implying
    ///   `required`.
    fn require_role(env: Env, addr: Address, required: Role);

    /// Proposes a new contract owner (two-step transfer, step 1).
    ///
    /// The pending owner has no privileges until they call
    /// `accept_ownership`.
    ///
    /// # Caller authorization
    /// - `caller` must sign and must be the current contract owner.
    ///   Holding the `Admin` role alone is **not** sufficient.
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    /// - `"Only owner can transfer ownership"` — if `caller` is not the
    ///   current owner.
    fn transfer_ownership(env: Env, caller: Address, new_owner: Address);

    /// Accepts a pending ownership transfer (two-step transfer, step 2).
    ///
    /// On acceptance:
    /// 1. The `Admin` role is granted to the new owner.
    /// 2. The `Admin` role is revoked from the old owner.
    /// 3. The ownership record is updated.
    ///
    /// # Caller authorization
    /// - `caller` must sign and must be the pending owner set by a prior
    ///   `transfer_ownership` call.
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    /// - `"No pending owner"` — if `transfer_ownership` has not been called
    ///   first (no pending proposal exists).
    /// - `"Caller is not pending owner"` — if `caller` does not match the
    ///   stored pending owner.
    fn accept_ownership(env: Env, caller: Address);
}
