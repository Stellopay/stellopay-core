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
    /// Configures the addresses used to verify the reentrancy hook behavior.
    ///
    /// @param env The contract environment.
    /// @param payroll_contract The payroll contract address under test.
    /// @param contributor The contributor address used for the reentry simulation.
    /// @access Requires the test harness to invoke this entrypoint with a valid environment.
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

    /// Returns the number of callback invocations recorded by this hook.
    ///
    /// @param env The contract environment.
    /// @return The number of times the milestone-expired hook was invoked.
    /// @access This is a read-only test helper and does not require special authorization.
    pub fn get_hook_call_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get::<_, u32>(&Symbol::new(&env, "hook_calls"))
            .unwrap_or(0)
    }

    /// Returns whether the callback path was reached.
    ///
    /// @param env The contract environment.
    /// @return True when the hook marked a reentry attempt as observed.
    /// @access This is a read-only test helper and does not require special authorization.
    pub fn attempted_reentry(env: Env) -> bool {
        env.storage()
            .instance()
            .get::<_, bool>(&Symbol::new(&env, "attempted_reentry"))
            .unwrap_or(false)
    }

    /// Records a callback from `expire_milestone`.
    ///
    /// @param env The contract environment.
    /// @param _agreement_id The agreement ID passed to the hook.
    /// @param _milestone_id The milestone ID passed to the hook.
    /// @access This test hook is intentionally callable without additional auth checks.
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

    /// Returns the configured payroll contract for test diagnostics.
    ///
    /// @param env The contract environment.
    /// @return The configured payroll contract address, if any.
    /// @access This is a read-only test helper and does not require special authorization.
    pub fn get_payroll_contract(env: Env) -> Option<Address> {
        env.storage()
            .instance()
            .get::<_, Address>(&Symbol::new(&env, "payroll"))
    }
}
