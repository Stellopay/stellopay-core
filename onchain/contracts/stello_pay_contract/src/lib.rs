#![no_std]

use soroban_sdk::{contract, contractimpl, Address, Env};
use stellar_access::ownable::{self as ownable, Ownable};
use stellar_contract_utils::upgradeable::UpgradeableInternal;
use stellar_macros::Upgradeable;

/// Minimal baseline Soroban contract.
///
/// Contributors will implement all business features from scratch on top of this.
#[derive(Upgradeable)]
#[contract]
pub struct PayrollContract;

#[contractimpl]
impl PayrollContract {
    pub fn initialize(env: Env, owner: Address) {
        ownable::set_owner(&env, &owner);
        // Placeholder: any other initialization logic
    }
}

impl UpgradeableInternal for PayrollContract {
    fn _require_auth(e: &Env, _operator: &Address) {
        ownable::enforce_owner_auth(e);
    }
}

#[contractimpl(contracttrait)]
impl Ownable for PayrollContract {}

#[cfg(test)]
mod test_upgrade;
