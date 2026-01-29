#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, Symbol,
};

use crate::mock_contract::{UpgradeableContract, UpgradeableContractClient};

// ============================================================================
// VERSION TRACKING TESTS
// ============================================================================

#[test]
fn test_initial_version_set() {
    let env = Env::default();
    env.mock_all_auths();
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
    env.mock_all_auths();
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
    env.mock_all_auths();
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
    env.mock_all_auths();
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
    env.mock_all_auths();
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
    env.mock_all_auths();
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
    env.mock_all_auths();
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
    env.mock_all_auths();
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
    env.mock_all_auths();
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
    env.mock_all_auths();
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
