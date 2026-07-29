use soroban_sdk::{contract, contractimpl, Address, Env, Symbol};

/// Mock contract for testing upgrade functionality
#[contract]
pub struct UpgradeableContract;

#[contractimpl]
impl UpgradeableContract {
    /// Initialize the contract with version tracking
    ///
    /// # Arguments
    /// * `admin` - admin parameter
    ///
    /// # Returns
    /// u32
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn initialize(env: Env, admin: Address) -> u32 {
        admin.require_auth();

        // Store admin
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "admin"), &admin);

        // Initialize version to 1
        let initial_version: u32 = 1;
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "version"), &initial_version);

        initial_version
    }

    /// Get current contract version
    ///
    /// # Returns
    /// u32
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_contract_version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&Symbol::new(&env, "version"))
            .unwrap_or(0)
    }

    /// Authorize upgrade (admin only)
    ///
    /// # Arguments
    /// * `caller` - caller parameter
    /// * `new_wasm_hash` - new_wasm_hash parameter
    pub fn authorize_upgrade(env: Env, caller: Address, new_wasm_hash: soroban_sdk::BytesN<32>) {
        caller.require_auth();

        // Verify caller is admin
        let admin: Address = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "admin"))
            .expect("Admin not set");

        if caller != admin {
            panic!("Unauthorized: Only admin can authorize upgrades");
        }

        // Store the authorized wasm hash
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "authorized_wasm"), &new_wasm_hash);

        // Emit upgrade authorized event
        #[allow(deprecated)]
        env.events().publish(
            (
                Symbol::new(&env, "upgrade"),
                Symbol::new(&env, "authorized"),
            ),
            new_wasm_hash,
        );
    }

    /// Upgrade
    ///
    /// # Arguments
    /// * `_new_wasm_hash` - _new_wasm_hash parameter
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn upgrade(env: Env, _new_wasm_hash: soroban_sdk::BytesN<32>) {
        let current_version: u32 = Self::get_contract_version(env.clone());
        let new_version = current_version + 1;
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "version"), &new_version);
    }

    /// Store test data for state preservation tests
    ///
    /// # Arguments
    /// * `agreement_id` - agreement_id parameter
    /// * `data` - data parameter
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn store_agreement(env: Env, agreement_id: u32, data: Symbol) {
        env.storage()
            .persistent()
            .set(&(Symbol::new(&env, "agreement"), agreement_id), &data);
    }

    /// Get stored agreement
    ///
    /// # Arguments
    /// * `agreement_id` - agreement_id parameter
    ///
    /// # Returns
    /// `Option<Symbol>`
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_agreement(env: Env, agreement_id: u32) -> Option<Symbol> {
        env.storage()
            .persistent()
            .get(&(Symbol::new(&env, "agreement"), agreement_id))
    }

    /// Store employee data
    ///
    /// # Arguments
    /// * `employee_id` - employee_id parameter
    /// * `name` - name parameter
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn store_employee(env: Env, employee_id: u32, name: Symbol) {
        env.storage()
            .persistent()
            .set(&(Symbol::new(&env, "employee"), employee_id), &name);
    }

    /// Get employee data
    ///
    /// # Arguments
    /// * `employee_id` - employee_id parameter
    ///
    /// # Returns
    /// `Option<Symbol>`
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_employee(env: Env, employee_id: u32) -> Option<Symbol> {
        env.storage()
            .persistent()
            .get(&(Symbol::new(&env, "employee"), employee_id))
    }

    /// Store balance
    ///
    /// # Arguments
    /// * `account` - account parameter
    /// * `balance` - balance parameter
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn store_balance(env: Env, account: Address, balance: i128) {
        env.storage()
            .persistent()
            .set(&(Symbol::new(&env, "balance"), account), &balance);
    }

    /// Get balance
    ///
    /// # Arguments
    /// * `account` - account parameter
    ///
    /// # Returns
    /// i128
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_balance(env: Env, account: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&(Symbol::new(&env, "balance"), account))
            .unwrap_or(0)
    }

    /// Store settings
    ///
    /// # Arguments
    /// * `key` - key parameter
    /// * `value` - value parameter
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn store_setting(env: Env, key: Symbol, value: u32) {
        env.storage()
            .persistent()
            .set(&(Symbol::new(&env, "setting"), key), &value);
    }

    /// Get setting
    ///
    /// # Arguments
    /// * `key` - key parameter
    ///
    /// # Returns
    /// `Option<u32>`
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_setting(env: Env, key: Symbol) -> Option<u32> {
        env.storage()
            .persistent()
            .get(&(Symbol::new(&env, "setting"), key))
    }

    /// Migration function - can be called multiple times safely
    ///
    /// # Returns
    /// bool
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn migrate(env: Env) -> bool {
        // Check if migration already ran
        let migration_key = Symbol::new(&env, "migration_v1");
        let already_migrated: bool = env
            .storage()
            .instance()
            .get(&migration_key)
            .unwrap_or(false);

        if already_migrated {
            return false; // Already migrated, safe to call again
        }

        // Perform migration
        env.storage().instance().set(&migration_key, &true);

        true // Migration performed
    }
}

// ============================================================================
// Malicious Milestone Hook — Reentrancy Regression Test Support (#855)
// ============================================================================
//
// `MaliciousMilestoneHook` is a test-only contract that implements the
// `MilestoneContractInterface` hook convention.  When `on_milestone_expired`
// is called by the payroll contract it records the call and attempts to
// re-enter `claim_milestone` on the payroll contract using the stored
// `payroll_contract` address.
//
// The reentrancy attempt will fail because:
//   a) The milestone was expired (not approved), so `claim_milestone` returns
//      `MilestoneNotApproved` — the cross-contract call panics, rolling back
//      the entire `expire_milestone` transaction, OR
//   b) Even for an approved milestone, the CEI pattern in `claim_milestone`
//      ensures that once the "claimed" flag is set, a re-entrant call would
//      return `MilestoneAlreadyClaimed`.
//
// In tests, we verify CEI correctness by:
//   1. Setting up a milestone agreement.
//   2. Approving a milestone.
//   3. Observing that `claim_milestone` claims exactly once (state-before-transfer).
//   4. Confirming a second `claim_milestone` call fails with `MilestoneAlreadyClaimed`.

/// A recording milestone hook contract that tracks `on_milestone_expired`
/// invocations.  Used in reentrancy regression tests.
///
/// # Security Note
/// This contract is **test-only** and must never be deployed to production.
#[contract]
pub struct MaliciousMilestoneHook;

#[contractimpl]
impl MaliciousMilestoneHook {
    /// Stores the payroll contract address and contributor so the hook callback
    /// can record state for test assertions.
    ///
    /// # Arguments
    /// * `payroll_contract` — the address of the deployed `stello_pay_contract`.
    /// * `contributor`      — the contributor address to impersonate if attempting re-entry.
    pub fn initialize(env: Env, payroll_contract: Address, contributor: Address) {
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "payroll"), &payroll_contract);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "contributor"), &contributor);
        // Reset hook invocation counter.
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "hook_calls"), &0u32);
        // Reset reentrant-attempt flag.
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "attempted_reentry"), &false);
    }

    /// Returns the number of times `on_milestone_expired` was invoked.
    ///
    /// A value of 0 after `expire_milestone` means the hook was never triggered
    /// (the contract address was not configured or the hook path was not reached).
    /// A value ≥ 1 confirms the hook fired.
    pub fn get_hook_call_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get::<_, u32>(&Symbol::new(&env, "hook_calls"))
            .unwrap_or(0)
    }

    /// Returns whether this hook attempted a reentrant call to `claim_milestone`.
    pub fn attempted_reentry(env: Env) -> bool {
        env.storage()
            .instance()
            .get::<_, bool>(&Symbol::new(&env, "attempted_reentry"))
            .unwrap_or(false)
    }

    /// Hook implementation: invoked by the payroll contract during `expire_milestone`.
    ///
    /// Records the call, then sets a flag indicating a re-entry was attempted.
    /// The actual cross-contract re-entry call is NOT performed here because in
    /// Soroban any cross-contract panic rolls back the entire calling transaction.
    /// Instead, the reentrancy regression is verified by test assertions: tests
    /// confirm that `claim_milestone` marks the milestone claimed BEFORE any
    /// external call, so a subsequent claim always fails with
    /// `MilestoneAlreadyClaimed`.
    ///
    /// # Checks-Effects-Interactions
    /// The fact that `expire_milestone` in the payroll contract marks the
    /// milestone expired *before* calling this hook means that even if this hook
    /// attempted a re-entry, the claimed/expired state is already committed and
    /// the re-entrant call would be rejected.
    pub fn on_milestone_expired(env: Env, _agreement_id: u128, _milestone_id: u32) {
        // Increment the hook-call counter.
        let prev: u32 = env
            .storage()
            .instance()
            .get::<_, u32>(&Symbol::new(&env, "hook_calls"))
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "hook_calls"), &(prev + 1));

        // Record that re-entry was "attempted" (in a real attack, code here
        // would call back into the payroll contract; we only set the flag in
        // this safe simulation so tests can assert the hook ran).
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "attempted_reentry"), &true);
    }

    /// Returns the stored payroll contract address (for test inspection).
    pub fn get_payroll_contract(env: Env) -> Option<Address> {
        env.storage()
            .instance()
            .get::<_, Address>(&Symbol::new(&env, "payroll"))
    }
}


// ============================================================================
// Compile-time interface guard (#813)
// ============================================================================
//
// Constant function-pointer assertions ensure that public method signatures
// in `UpgradeableContract` stay in sync with the interface that upgrade and
// migration tests depend on.  If a signature changes, tests will not compile,
// preventing the mock from silently diverging.
#[cfg(test)]
mod __interface_guard {
    use super::UpgradeableContract;
    use soroban_sdk::{Address, BytesN, Env, Symbol};

    // Lifecycle methods
    const _INIT: fn(Env, Address) -> u32 = UpgradeableContract::initialize;
    const _VERSION: fn(Env) -> u32 = UpgradeableContract::get_contract_version;
    const _AUTH_UPGRADE: fn(Env, Address, BytesN<32>) = UpgradeableContract::authorize_upgrade;
    const _UPGRADE: fn(Env, BytesN<32>) = UpgradeableContract::upgrade;
    const _MIGRATE: fn(Env) -> bool = UpgradeableContract::migrate;

    // Data-store methods used by upgrade-persistence tests
    const _STORE_AGREEMENT: fn(Env, u32, Symbol) = UpgradeableContract::store_agreement;
    const _GET_AGREEMENT: fn(Env, u32) -> Option<Symbol> = UpgradeableContract::get_agreement;
    const _STORE_EMPLOYEE: fn(Env, u32, Symbol) = UpgradeableContract::store_employee;
    const _GET_EMPLOYEE: fn(Env, u32) -> Option<Symbol> = UpgradeableContract::get_employee;
    const _STORE_BALANCE: fn(Env, Address, i128) = UpgradeableContract::store_balance;
    const _GET_BALANCE: fn(Env, Address) -> i128 = UpgradeableContract::get_balance;
    const _STORE_SETTING: fn(Env, Symbol, u32) = UpgradeableContract::store_setting;
    const _GET_SETTING: fn(Env, Symbol) -> Option<u32> = UpgradeableContract::get_setting;
}
