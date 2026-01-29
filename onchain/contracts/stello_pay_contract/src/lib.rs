#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Env};

/// Minimal baseline Soroban contract.
///
/// Contributors will implement all business features from scratch on top of this.
#[contract]
pub struct PayrollContract;

#[contractimpl]
impl PayrollContract {
    /// One-time initialization hook.
    pub fn initialize(env: Env, owner: Address) {
        owner.require_auth();
        // Placeholder: store `owner` in persistent storage when implementing access control.
        let _ = env;
    }
}

pub mod events;
pub mod storage;
pub mod payroll;

#[cfg(test)]
pub mod mock_contract;

#[cfg(test)]
mod tests;
