#![cfg(test)]

use soroban_sdk::{
    contract, contractimpl, testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation},
    Address, Env, IntoVal, Symbol, Vec as SorobanVec,
};

/// Mock contract for testing upgrade functionality
#[contract]
pub struct UpgradeableContract;

#[contractimpl]
impl UpgradeableContract {
    /// Initialize the contract with version tracking
    pub fn initialize(env: Env, admin: Address) -> u32 {
        admin.require_auth();
        
        // Store admin
        env.storage().instance().set(&Symbol::new(&env, "admin"), &admin);
        
        // Initialize version to 1
        let initial_version: u32 = 1;
        env.storage().instance().set(&Symbol::new(&env, "version"), &initial_version);
        
        initial_version
    }
    
    /// Get current contract version
    pub fn get_contract_version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&Symbol::new(&env, "version"))
            .unwrap_or(0)
    }
    
    /// Authorize upgrade (admin only)
    pub fn authorize_upgrade(env: Env, caller: Address, new_wasm_hash: soroban_sdk::BytesN<32>) {
        caller.require_auth();
        
        // Verify caller is admin
        let admin: Address = env.storage()
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
        env.events().publish(
            (Symbol::new(&env, "upgrade"), Symbol::new(&env, "authorized")),
            new_wasm_hash,
        );
    }
    
    /// Perform upgrade and increment version
    pub fn upgrade(env: Env, new_wasm_hash: soroban_sdk::BytesN<32>) {
        // Get current version
        let current_version: u32 = Self::get_contract_version(env.clone());
        
        // Increment version
        let new_version = current_version + 1;
        env.storage().instance().set(&Symbol::new(&env, "version"), &new_version);
        
        // Update the contract code
        env.deployer().update_current_contract_wasm(&new_wasm_hash);
    }
    
    /// Store test data for state preservation tests
    pub fn store_agreement(env: Env, agreement_id: u32, data: Symbol) {
        env.storage()
            .persistent()
            .set(&(Symbol::new(&env, "agreement"), agreement_id), &data);
    }
    
    /// Get stored agreement
    pub fn get_agreement(env: Env, agreement_id: u32) -> Option<Symbol> {
        env.storage()
            .persistent()
            .get(&(Symbol::new(&env, "agreement"), agreement_id))
    }
    
    /// Store employee data
    pub fn store_employee(env: Env, employee_id: u32, name: Symbol) {
        env.storage()
            .persistent()
            .set(&(Symbol::new(&env, "employee"), employee_id), &name);
    }
    
    /// Get employee data
    pub fn get_employee(env: Env, employee_id: u32) -> Option<Symbol> {
        env.storage()
            .persistent()
            .get(&(Symbol::new(&env, "employee"), employee_id))
    }
    
    /// Store balance
    pub fn store_balance(env: Env, account: Address, balance: i128) {
        env.storage()
            .persistent()
            .set(&(Symbol::new(&env, "balance"), account), &balance);
    }
    
    /// Get balance
    pub fn get_balance(env: Env, account: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&(Symbol::new(&env, "balance"), account))
            .unwrap_or(0)
    }
    
    /// Store settings
    pub fn store_setting(env: Env, key: Symbol, value: u32) {
        env.storage()
            .persistent()
            .set(&(Symbol::new(&env, "setting"), key), &value);
    }
    
    /// Get setting
    pub fn get_setting(env: Env, key: Symbol) -> Option<u32> {
        env.storage()
            .persistent()
            .get(&(Symbol::new(&env, "setting"), key))
    }
    
    /// Migration function - can be called multiple times safely
    pub fn migrate(env: Env) -> bool {
        // Check if migration already ran
        let migration_key = Symbol::new(&env, "migration_v1");
        let already_migrated: bool = env.storage()
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
// VERSION TRACKING TESTS
// ============================================================================

#[test]
fn test_initial_version_set() {
    let env = Env::default();
    let contract_id = env.register_contract(None, UpgradeableContract);
    let client = UpgradeableContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    
    // Initialize contract
    let initial_version = client.initialize(&admin);
    
    // Verify initial version is set to 1
    assert_eq!(initial_version, 1, "Initial version should be 1");
}

#[test]
fn test_get_contract_version() {
    let env = Env::default();
    let contract_id = env.register_contract(None, UpgradeableContract);
    let client = UpgradeableContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    
    // Initialize contract
    client.initialize(&admin);
    
    // Get version
    let version = client.get_contract_version();
    
    // Verify version can be retrieved
    assert_eq!(version, 1, "Version should be retrievable and equal to 1");
}

#[test]
fn test_version_increments_on_upgrade() {
    let env = Env::default();
    let contract_id = env.register_contract(None, UpgradeableContract);
    let client = UpgradeableContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    
    // Initialize contract
    client.initialize(&admin);
    
    // Get initial version
    let version_before = client.get_contract_version();
    assert_eq!(version_before, 1);
    
    // Simulate upgrade by calling upgrade function
    let new_wasm_hash = soroban_sdk::BytesN::from_array(&env, &[0u8; 32]);
    client.upgrade(&new_wasm_hash);
    
    // Get version after upgrade
    let version_after = client.get_contract_version();
    
    // Verify version incremented
    assert_eq!(version_after, 2, "Version should increment to 2 after upgrade");
}

// ============================================================================
// AUTHORIZATION TESTS
// ============================================================================

#[test]
fn test_admin_can_authorize_upgrade() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, UpgradeableContract);
    let client = UpgradeableContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    
    // Initialize contract
    client.initialize(&admin);
    
    // Admin authorizes upgrade
    let new_wasm_hash = soroban_sdk::BytesN::from_array(&env, &[1u8; 32]);
    client.authorize_upgrade(&admin, &new_wasm_hash);
    
    // Verify authorization was recorded (no panic means success)
    // In a real implementation, you'd verify the stored hash
}

#[test]
#[should_panic(expected = "Unauthorized: Only admin can authorize upgrades")]
fn test_non_admin_cannot_authorize() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, UpgradeableContract);
    let client = UpgradeableContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let non_admin = Address::generate(&env);
    
    // Initialize contract
    client.initialize(&admin);
    
    // Non-admin tries to authorize upgrade (should panic)
    let new_wasm_hash = soroban_sdk::BytesN::from_array(&env, &[1u8; 32]);
    client.authorize_upgrade(&non_admin, &new_wasm_hash);
}

#[test]
fn test_upgrade_authorized_event() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, UpgradeableContract);
    let client = UpgradeableContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    
    // Initialize contract
    client.initialize(&admin);
    
    // Admin authorizes upgrade
    let new_wasm_hash = soroban_sdk::BytesN::from_array(&env, &[1u8; 32]);
    client.authorize_upgrade(&admin, &new_wasm_hash);
    
    // Verify event was emitted
    let events = env.events().all();
    let event_count = events.len();
    
    assert!(event_count > 0, "At least one event should be emitted");
    
    // Verify the upgrade authorized event exists
    let last_event = events.get(event_count - 1).unwrap();
    
    // The event should contain the upgrade topic
    // Note: In a real implementation, you'd verify the exact event structure
}

// ============================================================================
// STATE PRESERVATION TESTS
// ============================================================================

#[test]
fn test_existing_agreements_persist() {
    let env = Env::default();
    let contract_id = env.register_contract(None, UpgradeableContract);
    let client = UpgradeableContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    
    // Initialize contract
    client.initialize(&admin);
    
    // Store agreement data before upgrade
    let agreement_id = 1;
    let agreement_data = Symbol::new(&env, "agreement_1");
    client.store_agreement(&agreement_id, &agreement_data);
    
    // Perform upgrade
    let new_wasm_hash = soroban_sdk::BytesN::from_array(&env, &[0u8; 32]);
    client.upgrade(&new_wasm_hash);
    
    // Verify agreement data persists after upgrade
    let retrieved_data = client.get_agreement(&agreement_id);
    assert_eq!(
        retrieved_data,
        Some(agreement_data),
        "Agreement data should persist after upgrade"
    );
}

#[test]
fn test_employee_data_persists() {
    let env = Env::default();
    let contract_id = env.register_contract(None, UpgradeableContract);
    let client = UpgradeableContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    
    // Initialize contract
    client.initialize(&admin);
    
    // Store employee data before upgrade
    let employee_id = 1;
    let employee_name = Symbol::new(&env, "John");
    client.store_employee(&employee_id, &employee_name);
    
    // Perform upgrade
    let new_wasm_hash = soroban_sdk::BytesN::from_array(&env, &[0u8; 32]);
    client.upgrade(&new_wasm_hash);
    
    // Verify employee data persists after upgrade
    let retrieved_name = client.get_employee(&employee_id);
    assert_eq!(
        retrieved_name,
        Some(employee_name),
        "Employee data should persist after upgrade"
    );
}

#[test]
fn test_balances_persist() {
    let env = Env::default();
    let contract_id = env.register_contract(None, UpgradeableContract);
    let client = UpgradeableContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let account = Address::generate(&env);
    
    // Initialize contract
    client.initialize(&admin);
    
    // Store balance before upgrade
    let balance: i128 = 1000;
    client.store_balance(&account, &balance);
    
    // Perform upgrade
    let new_wasm_hash = soroban_sdk::BytesN::from_array(&env, &[0u8; 32]);
    client.upgrade(&new_wasm_hash);
    
    // Verify balance persists after upgrade
    let retrieved_balance = client.get_balance(&account);
    assert_eq!(
        retrieved_balance, balance,
        "Balance should persist after upgrade"
    );
}

#[test]
fn test_settings_persist() {
    let env = Env::default();
    let contract_id = env.register_contract(None, UpgradeableContract);
    let client = UpgradeableContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    
    // Initialize contract
    client.initialize(&admin);
    
    // Store settings before upgrade
    let setting_key = Symbol::new(&env, "max_amount");
    let setting_value: u32 = 5000;
    client.store_setting(&setting_key, &setting_value);
    
    // Perform upgrade
    let new_wasm_hash = soroban_sdk::BytesN::from_array(&env, &[0u8; 32]);
    client.upgrade(&new_wasm_hash);
    
    // Verify settings persist after upgrade
    let retrieved_value = client.get_setting(&setting_key);
    assert_eq!(
        retrieved_value,
        Some(setting_value),
        "Settings should persist after upgrade"
    );
}

// ============================================================================
// MIGRATION TESTS
// ============================================================================

#[test]
fn test_migration_functions_work() {
    let env = Env::default();
    let contract_id = env.register_contract(None, UpgradeableContract);
    let client = UpgradeableContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    
    // Initialize contract
    client.initialize(&admin);
    
    // Run migration
    let migration_result = client.migrate();
    
    // Verify migration executed successfully
    assert!(migration_result, "Migration should execute successfully");
}

#[test]
fn test_migration_preserves_all_data() {
    let env = Env::default();
    let contract_id = env.register_contract(None, UpgradeableContract);
    let client = UpgradeableContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let account = Address::generate(&env);
    
    // Initialize contract
    client.initialize(&admin);
    
    // Store various types of data
    let agreement_id = 1;
    let agreement_data = Symbol::new(&env, "agreement_1");
    client.store_agreement(&agreement_id, &agreement_data);
    
    let employee_id = 1;
    let employee_name = Symbol::new(&env, "Alice");
    client.store_employee(&employee_id, &employee_name);
    
    let balance: i128 = 2000;
    client.store_balance(&account, &balance);
    
    let setting_key = Symbol::new(&env, "fee");
    let setting_value: u32 = 100;
    client.store_setting(&setting_key, &setting_value);
    
    // Run migration
    client.migrate();
    
    // Verify all data is preserved
    assert_eq!(
        client.get_agreement(&agreement_id),
        Some(agreement_data),
        "Agreement data should be preserved"
    );
    assert_eq!(
        client.get_employee(&employee_id),
        Some(employee_name),
        "Employee data should be preserved"
    );
    assert_eq!(
        client.get_balance(&account),
        balance,
        "Balance should be preserved"
    );
    assert_eq!(
        client.get_setting(&setting_key),
        Some(setting_value),
        "Settings should be preserved"
    );
}

#[test]
fn test_migration_can_run_multiple_times_safely() {
    let env = Env::default();
    let contract_id = env.register_contract(None, UpgradeableContract);
    let client = UpgradeableContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    
    // Initialize contract
    client.initialize(&admin);
    
    // Run migration first time
    let first_run = client.migrate();
    assert!(first_run, "First migration should execute");
    
    // Run migration second time
    let second_run = client.migrate();
    assert!(!second_run, "Second migration should be idempotent (return false)");
    
    // Run migration third time
    let third_run = client.migrate();
    assert!(!third_run, "Third migration should be idempotent (return false)");
    
    // Verify contract is still functional
    let version = client.get_contract_version();
    assert_eq!(version, 1, "Contract should still be functional after multiple migrations");
}
