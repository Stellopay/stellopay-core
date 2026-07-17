//! Shared RBAC types and client for cross-contract calls.
//!
//! Depend on this crate (rlib only) from other contracts. Deploy the `rbac`
//! contract crate separately — do not link `rbac` as a cdylib dependency.

#![no_std]

use soroban_sdk::{contractclient, contracttype, Address, Env};

/// Core roles supported by the RBAC contract (must stay in sync with `rbac`).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Role {
    Admin,
    Employer,
    Employee,
    Arbiter,
}

/// Client for the deployed RBAC contract (`has_role` and other exports).
#[contractclient(name = "RbacContractClient")]
pub trait RbacContractInterface {
    /// Returns whether `addr` satisfies the `required` role according to the
    /// deployed RBAC contract. Resolution is **inheritance-aware** — e.g. holding
    /// `Role::Admin` also satisfies a `Role::Arbiter` requirement.
    ///
    /// # Caller authorization
    /// Read-only view. **No `require_auth` is performed** and the caller is not
    /// restricted: any contract or external address may call it. It only queries
    /// on-chain RBAC state.
    ///
    /// # Panics
    /// Panics with the RBAC contract's `"Contract not initialized"`-style error
    /// if the RBAC contract has not been initialized (state precondition, not a
    /// caller-authorization failure). It does **not** panic for a valid `Role`
    /// value — an unknown/unsupported role simply resolves to `false`.
    ///
    /// # Returns
    /// `true` if `addr` holds `required` (directly or by inheritance),
    /// otherwise `false`.
    fn has_role(env: Env, addr: Address, required: Role) -> bool;
}
