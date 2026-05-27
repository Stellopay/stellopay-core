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
    fn has_role(env: Env, addr: Address, required: Role) -> bool;
}
