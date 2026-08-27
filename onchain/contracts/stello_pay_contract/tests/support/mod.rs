//! Contracts used only by integration tests.
//!
//! Integration tests are separate crates, so they cannot access library items
//! compiled with `cfg(test)`. Keeping this hook here lets the production crate
//! gate its mock module with `cfg(test)` while preserving the reentrancy test's
//! end-to-end callback coverage.

use soroban_sdk::{contract, contractimpl, Address, Env, Symbol};

/// Recording milestone hook used by the reentrancy regression tests.
///
/// This contract deliberately records that the callback ran instead of
/// performing a cross-contract re-entry. A failing re-entry would roll back
/// the entire Soroban transaction, which would also erase the evidence the
/// test needs to inspect. The payroll contract's persisted terminal state is
/// tested separately before this callback path is exercised.
#[contract]
pub struct MaliciousMilestoneHook;

#[contractimpl]
impl MaliciousMilestoneHook {
    /// Configure the addresses that are useful when inspecting a test run.
    pub fn initialize(env: Env, payroll_contract: Address, contributor: Address) {
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "payroll"), &payroll_contract);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "contributor"), &contributor);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "hook_calls"), &0u32);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "attempted_reentry"), &false);
    }

    /// Return the number of callback invocations recorded by this hook.
    pub fn get_hook_call_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get::<_, u32>(&Symbol::new(&env, "hook_calls"))
            .unwrap_or(0)
    }

    /// Return whether the callback path was reached.
    pub fn attempted_reentry(env: Env) -> bool {
        env.storage()
            .instance()
            .get::<_, bool>(&Symbol::new(&env, "attempted_reentry"))
            .unwrap_or(false)
    }

    /// Record a callback from `expire_milestone`.
    pub fn on_milestone_expired(env: Env, _agreement_id: u128, _milestone_id: u32) {
        let previous: u32 = env
            .storage()
            .instance()
            .get::<_, u32>(&Symbol::new(&env, "hook_calls"))
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "hook_calls"), &(previous + 1));
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "attempted_reentry"), &true);
    }

    /// Return the configured payroll contract for test diagnostics.
    pub fn get_payroll_contract(env: Env) -> Option<Address> {
        env.storage()
            .instance()
            .get::<_, Address>(&Symbol::new(&env, "payroll"))
    }
}
