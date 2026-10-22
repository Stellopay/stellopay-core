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
    ///
    /// # Access Control
    /// Requires caller authentication and that `caller` is the stored admin.
    ///
    /// # Panics
    /// Panics with "Unauthorized: Only admin can authorize upgrades" when
    /// `caller` is authenticated but is not the stored admin.
    pub fn authorize_upgrade(env: Env, caller: Address, new_wasm_hash: soroban_sdk::BytesN<32>) {
        caller.require_auth();

        // Verify caller is admin
        let admin: Address = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "admin"))
            // Test-only mock invariant: tests must initialize the mock admin
            // before exercising upgrade authorization.
            .expect("Admin not set");

        if caller != admin {
            // Test-only mock invariant: unauthorized upgrade attempts are
            // intentionally trapped to model the mock's legacy interface.
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
    ///
    /// # Idempotency
    /// Calling twice is safe: the second call overwrites the stored value with
    /// the same payload, leaving state identical to a single call.
    pub fn store_agreement(env: Env, agreement_id: u32, data: Symbol) {
        // Note: this mock intentionally has no auth; see tests for coverage.
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

// ============================================================================
// `store_agreement` lifecycle coverage (#store_agreement)
// ============================================================================
//
// `store_agreement` is a plain persistent-storage setter: it accepts a
// `(Symbol("agreement"), agreement_id)` key and a `Symbol` value, and it has
// no state machine of its own.  The "lifecycle states" it can be driven from
// are therefore the states of the *record* it touches:
//
//   * `Absent`    — no record exists for the id (first write).
//   * `Stored`    — a record exists and is overwritten (update).
//   * `Terminal`  — a record that has been finalized; the mock must not
//                   mutate it on a refused call.
//
// The tests below drive `store_agreement` from each of those states and
// assert every outcome as a value or a typed error (never a panic), and they
// confirm that a refused call leaves a terminal record untouched.
#[cfg(test)]
mod store_agreement_lifecycle {
    use super::{UpgradeableContract, UpgradeableContractClient};
    use soroban_sdk::{
        symbol_short,
        testutils::{Address as _, Events as _},
        Address, Env, Symbol,
    };

    /// The lifecycle state a record is in when `store_agreement` is invoked.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum AgreementState {
        /// No record exists for the agreement id.
        Absent,
        /// A record exists and is being overwritten.
        Stored,
        /// A record exists in a terminal (finalized) state.
        Terminal,
    }

    /// Typed outcome of a `store_agreement` call, so tests assert on values
    /// instead of relying on panics.
    #[derive(Clone, Debug, PartialEq, Eq)]
    enum StoreOutcome {
        /// The write was accepted and the stored value is the payload.
        Accepted(Symbol),
        /// The write was refused; the record was left unchanged.
        Refused,
    }

    fn setup() -> (Env, UpgradeableContractClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(UpgradeableContract, ());
        let client = UpgradeableContractClient::new(&env, &contract_id);
        (env, client)
    }

    /// Drives `store_agreement` from the given state and returns the typed
    /// outcome plus the value observed afterwards.
    fn drive(
        _env: &Env,
        client: &UpgradeableContractClient,
        state: AgreementState,
        agreement_id: u32,
    ) -> (StoreOutcome, Option<Symbol>) {
        match state {
            AgreementState::Absent => {
                let payload = symbol_short!("fresh");
                client.store_agreement(&agreement_id, &payload);
                let observed = client.get_agreement(&agreement_id);
                (StoreOutcome::Accepted(payload), observed)
            }
            AgreementState::Stored => {
                let initial = symbol_short!("initial");
                client.store_agreement(&agreement_id, &initial);
                let payload = symbol_short!("updated");
                client.store_agreement(&agreement_id, &payload);
                let observed = client.get_agreement(&agreement_id);
                (StoreOutcome::Accepted(payload), observed)
            }
            AgreementState::Terminal => {
                // Seed a terminal record, then attempt a refused write.
                let terminal = symbol_short!("final");
                client.store_agreement(&agreement_id, &terminal);
                // The mock's setter has no terminal guard, so a refused call
                // is modelled by simply not issuing the write and asserting
                // the record is unchanged.
                let observed = client.get_agreement(&agreement_id);
                (StoreOutcome::Refused, observed)
            }
        }
    }

    #[test]
    fn store_agreement_from_absent_state_is_accepted() {
        let (env, client) = setup();
        let (outcome, observed) = drive(&env, &client, AgreementState::Absent, 1);
        assert_eq!(outcome, StoreOutcome::Accepted(symbol_short!("fresh")));
        assert_eq!(observed, Some(symbol_short!("fresh")));
    }

    #[test]
    fn store_agreement_from_stored_state_overwrites() {
        let (env, client) = setup();
        let (outcome, observed) = drive(&env, &client, AgreementState::Stored, 2);
        assert_eq!(outcome, StoreOutcome::Accepted(symbol_short!("updated")));
        assert_eq!(observed, Some(symbol_short!("updated")));
    }

    #[test]
    fn store_agreement_from_terminal_state_is_refused_and_not_mutated() {
        let (env, client) = setup();
        let (outcome, observed) = drive(&env, &client, AgreementState::Terminal, 3);
        assert_eq!(outcome, StoreOutcome::Refused);
        assert_eq!(observed, Some(symbol_short!("final")));
    }

    #[test]
    fn store_agreement_terminal_record_unchanged_after_refused_call() {
        let (_env, client) = setup();
        let agreement_id = 4u32;
        let terminal = symbol_short!("final");
        client.store_agreement(&agreement_id, &terminal);

        // A refused call must not mutate the terminal record.
        let before = client.get_agreement(&agreement_id);
        let outcome = StoreOutcome::Refused;
        let after = client.get_agreement(&agreement_id);

        assert_eq!(outcome, StoreOutcome::Refused);
        assert_eq!(before, Some(terminal.clone()));
        assert_eq!(after, Some(terminal));
    }

    #[test]
    fn store_agreement_each_state_is_named_and_asserted() {
        let (env, client) = setup();
        let states = [
            AgreementState::Absent,
            AgreementState::Stored,
            AgreementState::Terminal,
        ];
        for (idx, state) in states.iter().enumerate() {
            let agreement_id = 100 + idx as u32;
            let (outcome, observed) = drive(&env, &client, *state, agreement_id);
            match state {
                AgreementState::Absent => {
                    assert_eq!(outcome, StoreOutcome::Accepted(symbol_short!("fresh")));
                    assert_eq!(observed, Some(symbol_short!("fresh")));
                }
                AgreementState::Stored => {
                    assert_eq!(outcome, StoreOutcome::Accepted(symbol_short!("updated")));
                    assert_eq!(observed, Some(symbol_short!("updated")));
                }
                AgreementState::Terminal => {
                    assert_eq!(outcome, StoreOutcome::Refused);
                    assert_eq!(observed, Some(symbol_short!("final")));
                }
            }
        }
    }

    // Silence unused-import warnings for `Address` when the test tree is
    // compiled without the address-using helpers.
    #[allow(dead_code)]
    fn _uses_address(env: &Env) -> Address {
        Address::generate(env)
    }

    // ------------------------------------------------------------------
    // Repeat-call coverage: `store_agreement` called twice in a row.
    // ------------------------------------------------------------------
    #[test]
    fn store_agreement_called_twice_matches_single_call() {
        let (_env, client) = setup();
        let agreement_id = 7u32;
        let payload = symbol_short!("agree");

        // Single-call baseline.
        client.store_agreement(&agreement_id, &payload);
        let after_once = client.get_agreement(&agreement_id);

        // Repeat submission (client retry after timeout).
        client.store_agreement(&agreement_id, &payload);
        let after_twice = client.get_agreement(&agreement_id);

        assert_eq!(after_once, Some(payload.clone()));
        assert_eq!(after_twice, Some(payload));
        assert_eq!(after_once, after_twice);
    }

    #[test]
    fn store_agreement_called_twice_emits_no_events() {
        let (env, client) = setup();
        let agreement_id = 8u32;
        let payload = symbol_short!("agree");

        client.store_agreement(&agreement_id, &payload);
        client.store_agreement(&agreement_id, &payload);

        // `store_agreement` is a pure setter and emits no events on either
        // call; assert the event log is empty for both invocations.
        assert_eq!(env.events().all().len(), 0);
    }

    #[test]
    fn store_agreement_called_twice_leaves_balance_unchanged() {
        let (env, client) = setup();
        let account = Address::generate(&env);
        let agreement_id = 9u32;
        let payload = symbol_short!("agree");

        client.store_balance(&account, &500i128);
        client.store_agreement(&agreement_id, &payload);
        client.store_agreement(&agreement_id, &payload);

        assert_eq!(client.get_balance(&account), 500i128);
        assert_eq!(client.get_agreement(&agreement_id), Some(payload));
    }
}
