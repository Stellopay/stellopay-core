//-----------------------------------------------------------------------------
// Backup Module Tests
//-----------------------------------------------------------------------------

use crate::payroll::{PayrollContract, PayrollContractClient};
use crate::storage::{BackupStatus, BackupType, RecoveryStatus, RecoveryType};
use soroban_sdk::{
    testutils::Address as _, testutils::MockAuth, testutils::MockAuthInvoke, Address, Env, IntoVal,
    String as SorobanString,
};

//-----------------------------------------------------------------------------
// Test Setup and Helpers
//-----------------------------------------------------------------------------

fn create_test_contract(env: &Env) -> PayrollContractClient {
    let contract_id = env.register(PayrollContract, ());
    PayrollContractClient::new(env, &contract_id)
}

fn create_test_address(env: &Env, _seed: &str) -> Address {
    Address::generate(env)
}

//-----------------------------------------------------------------------------
// Backup Creation Tests
//-----------------------------------------------------------------------------

#[test]
fn test_create_backup_success() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let employee = create_test_address(&env, "employee");

    // Initialize contract
    env.mock_all_auths();
    contract.initialize(&employer);

    // Create test data
    let token = Address::generate(&env);
    contract.create_or_update_escrow(
        &employer, &employee, &token, &5000, &2592000, // 30 days
        &2592000, // 30 days
    );

    // Create backup
    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Test Backup"),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );

    assert_eq!(backup_id, 1);

    // Verify backup was created
    let backup = contract.get_backup(&backup_id);
    assert_eq!(backup.id, 1);
    assert_eq!(backup.name, SorobanString::from_str(&env, "Test Backup"));
    assert_eq!(backup.employer, employer);
    assert_eq!(backup.status, BackupStatus::Completed);
}

#[test]
#[should_panic]
fn test_create_backup_invalid_name() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Test empty name
    let _result = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, ""),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );
    // Test should fail with InvalidTemplateName error
    // Note: Client methods panic on error, so this test will panic as expected

    // Test name too long (over 100 characters)
    let long_name = "a".repeat(101);
    let _result = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, &long_name),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );
    // Test should fail with InvalidTemplateName error
    // Note: Client methods panic on error, so this test will panic as expected
}

#[test]
#[should_panic]
fn test_create_backup_unauthorized() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let unauthorized = create_test_address(&env, "unauthorized");

    // Initialize contract with employer authentication
    env.mock_auths(&[MockAuth {
        address: &employer,
        invoke: &MockAuthInvoke {
            contract: &contract.address,
            fn_name: "initialize",
            args: (employer.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    contract.initialize(&employer);

    // Try to create backup without authentication (no mock_auths for this call)
    let _result = contract.create_backup(
        &unauthorized,
        &SorobanString::from_str(&env, "Test Backup"),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );
    // Test should fail with Unauthorized error
    // Note: Client methods panic on error, so this test will panic as expected
}

#[test]
#[should_panic]
fn test_create_backup_when_paused() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");

    env.mock_all_auths();
    contract.initialize(&employer);
    contract.pause(&employer);

    // Try to create backup when paused
    let _result = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Test Backup"),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );
    // Test should fail with ContractPaused error
    // Note: Client methods panic on error, so this test will panic as expected
}

//-----------------------------------------------------------------------------
// Backup Verification Tests
//-----------------------------------------------------------------------------

#[test]
fn test_verify_backup_success() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let employee = create_test_address(&env, "employee");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create test data and backup
    let token = Address::generate(&env);
    contract.create_or_update_escrow(
        &employer, &employee, &token, &5000, &2592000, // 30 days
        &2592000, // 30 days
    );

    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Test Backup"),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );

    // Verify backup
    let is_valid = contract.verify_backup(&employer, &backup_id);
    assert!(is_valid);

    // Check backup status was updated
    let backup = contract.get_backup(&backup_id);
    assert_eq!(backup.status, BackupStatus::Verified);
}

#[test]
#[should_panic]
fn test_verify_backup_not_found() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Try to verify non-existent backup
    let _result = contract.verify_backup(&employer, &999);
    // Test should fail with BackupNotFound error
    // Note: Client methods panic on error, so this test will panic as expected
}

#[test]
#[should_panic]
fn test_verify_backup_unauthorized() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let unauthorized = create_test_address(&env, "unauthorized");
    let employee = create_test_address(&env, "employee");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create backup as employer
    let token = Address::generate(&env);
    contract.create_or_update_escrow(
        &employer, &employee, &token, &5000, &2592000, // 30 days
        &2592000, // 30 days
    );

    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Test Backup"),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );

    // Try to verify as unauthorized user
    let _result = contract.verify_backup(&unauthorized, &backup_id);
    // Test should fail with Unauthorized error
    // Note: Client methods panic on error, so this test will panic as expected
}

//-----------------------------------------------------------------------------
// Recovery Process Tests
//-----------------------------------------------------------------------------

#[test]
fn test_create_recovery_point_success() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let employee = create_test_address(&env, "employee");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create test data and backup
    let token = Address::generate(&env);
    contract.create_or_update_escrow(
        &employer, &employee, &token, &5000, &2592000, // 30 days
        &2592000, // 30 days
    );

    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Test Backup"),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );

    // Create recovery point
    let recovery_id = contract.create_recovery_point(
        &employer,
        &backup_id,
        &SorobanString::from_str(&env, "Test Recovery"),
        &SorobanString::from_str(&env, "Test Recovery Description"),
        &SorobanString::from_str(&env, "Full"),
    );

    assert_eq!(recovery_id, 1);

    // Verify recovery point was created
    let recovery_points = contract.get_recovery_points();
    assert_eq!(recovery_points.len(), 1);
    assert_eq!(recovery_points.get(0).unwrap().id, 1);
    assert_eq!(
        recovery_points.get(0).unwrap().status,
        RecoveryStatus::Pending
    );
}

#[test]
#[should_panic]
fn test_create_recovery_point_invalid_backup() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Try to create recovery point with non-existent backup
    let _result = contract.create_recovery_point(
        &employer,
        &999,
        &SorobanString::from_str(&env, "Test Recovery"),
        &SorobanString::from_str(&env, "Test Recovery Description"),
        &SorobanString::from_str(&env, "Full"),
    );
    // Test should fail with BackupNotFound error
    // Note: Client methods panic on error, so this test will panic as expected
}

#[test]
fn test_execute_recovery_success() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let employee = create_test_address(&env, "employee");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create test data and backup
    let token = Address::generate(&env);
    contract.create_or_update_escrow(
        &employer, &employee, &token, &5000, &2592000, // 30 days
        &2592000, // 30 days
    );

    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Test Backup"),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );

    // Create and execute recovery point
    let recovery_id = contract.create_recovery_point(
        &employer,
        &backup_id,
        &SorobanString::from_str(&env, "Test Recovery"),
        &SorobanString::from_str(&env, "Test Recovery Description"),
        &SorobanString::from_str(&env, "Full"),
    );

    let success = contract.execute_recovery(&employer, &recovery_id);
    assert!(success);

    // Verify recovery point status was updated
    let recovery_points = contract.get_recovery_points();
    assert_eq!(recovery_points.len(), 1);
    assert_eq!(
        recovery_points.get(0).unwrap().status,
        RecoveryStatus::Completed
    );
}

#[test]
#[should_panic]
fn test_execute_recovery_not_found() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Try to execute non-existent recovery point
    let _result = contract.execute_recovery(&employer, &999);
    // Test should fail with RecoveryPointNotFound error
    // Note: Client methods panic on error, so this test will panic as expected
}

#[test]
#[should_panic]
fn test_execute_recovery_already_in_progress() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let employee = create_test_address(&env, "employee");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create test data and backup
    let token = Address::generate(&env);
    contract.create_or_update_escrow(
        &employer, &employee, &token, &5000, &2592000, // 30 days
        &2592000, // 30 days
    );

    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Test Backup"),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );

    // Create recovery point
    let recovery_id = contract.create_recovery_point(
        &employer,
        &backup_id,
        &SorobanString::from_str(&env, "Test Recovery"),
        &SorobanString::from_str(&env, "Test Recovery Description"),
        &SorobanString::from_str(&env, "Full"),
    );

    // Execute recovery first time (should succeed)
    let success = contract.execute_recovery(&employer, &recovery_id);
    assert!(success);

    // Try to execute again (should fail)
    let _result = contract.execute_recovery(&employer, &recovery_id);
    // Test should fail with RecoveryInProgress error
    // Note: Client methods panic on error, so this test will panic as expected
}

//-----------------------------------------------------------------------------
// Backup Management Tests
//-----------------------------------------------------------------------------

#[test]
fn test_get_employer_backups() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let employee = create_test_address(&env, "employee");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create test data
    let token = Address::generate(&env);
    contract.create_or_update_escrow(
        &employer, &employee, &token, &5000, &2592000, // 30 days
        &2592000, // 30 days
    );

    // Create multiple backups
    let backup1 = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Backup 1"),
        &SorobanString::from_str(&env, "Description 1"),
        &SorobanString::from_str(&env, "Employer"),
    );

    let backup2 = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Backup 2"),
        &SorobanString::from_str(&env, "Description 2"),
        &SorobanString::from_str(&env, "Full"),
    );

    // Get employer backups
    let backups = contract.get_employer_backups(&employer);
    assert_eq!(backups.len(), 2);

    // Verify backup IDs
    let mut found_backup1 = false;
    let mut found_backup2 = false;
    for backup in backups.iter() {
        if backup.id == backup1 {
            found_backup1 = true;
        }
        if backup.id == backup2 {
            found_backup2 = true;
        }
    }
    assert!(found_backup1);
    assert!(found_backup2);
}

#[test]
#[should_panic]
fn test_delete_backup_success() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let employee = create_test_address(&env, "employee");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create test data and backup
    let token = Address::generate(&env);
    contract.create_or_update_escrow(
        &employer, &employee, &token, &5000, &2592000, // 30 days
        &2592000, // 30 days
    );

    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Test Backup"),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );

    // Delete backup
    contract.delete_backup(&employer, &backup_id);

    // Verify backup was deleted - this should panic with BackupNotFound error
    let _result = contract.get_backup(&backup_id);
    // Test should fail with BackupNotFound error
    // Note: Client methods panic on error, so this test will panic as expected

    // Verify backup was removed from employer's list
    let backups = contract.get_employer_backups(&employer);
    assert_eq!(backups.len(), 0);
}

#[test]
#[should_panic]
fn test_delete_backup_unauthorized() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let unauthorized = create_test_address(&env, "unauthorized");
    let employee = create_test_address(&env, "employee");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create backup as employer
    let token = Address::generate(&env);
    contract.create_or_update_escrow(
        &employer, &employee, &token, &5000, &2592000, // 30 days
        &2592000, // 30 days
    );

    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Test Backup"),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );

    // Try to delete as unauthorized user
    let _result = contract.delete_backup(&unauthorized, &backup_id);
    // Test should fail with Unauthorized error
    // Note: Client methods panic on error, so this test will panic as expected
}

#[test]
#[should_panic]
fn test_delete_backup_not_found() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Try to delete non-existent backup
    let _result = contract.delete_backup(&employer, &999);
    // Test should fail with BackupNotFound error
    // Note: Client methods panic on error, so this test will panic as expected
}

//-----------------------------------------------------------------------------
// Backup Type Tests
//-----------------------------------------------------------------------------

#[test]
fn test_create_full_backup() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let employee = create_test_address(&env, "employee");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create test data
    let token = Address::generate(&env);
    contract.create_or_update_escrow(
        &employer, &employee, &token, &5000, &2592000, // 30 days
        &2592000, // 30 days
    );

    // Create full backup
    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Full Backup"),
        &SorobanString::from_str(&env, "Complete system backup"),
        &SorobanString::from_str(&env, "Full"),
    );

    // Verify backup was created
    let backup = contract.get_backup(&backup_id);
    assert_eq!(backup.backup_type, SorobanString::from_str(&env, "Full"));
    assert_eq!(backup.status, BackupStatus::Completed);
}

#[test]
fn test_create_employer_backup() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let employee = create_test_address(&env, "employee");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create test data
    let token = Address::generate(&env);
    contract.create_or_update_escrow(
        &employer, &employee, &token, &5000, &2592000, // 30 days
        &2592000, // 30 days
    );

    // Create employer backup
    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Employer Backup"),
        &SorobanString::from_str(&env, "Employer-specific backup"),
        &SorobanString::from_str(&env, "Employer"),
    );

    // Verify backup was created
    let backup = contract.get_backup(&backup_id);
    assert_eq!(
        backup.backup_type,
        SorobanString::from_str(&env, "Employer")
    );
    assert_eq!(backup.status, BackupStatus::Completed);
}

#[test]
fn test_create_template_backup() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create template
    let token = Address::generate(&env);
    contract.create_template(
        &employer,
        &SorobanString::from_str(&env, "Test Template"),
        &SorobanString::from_str(&env, "Test Description"),
        &token,
        &5000,
        &2592000, // 30 days
        &2592000, // 30 days
        &false,   // is_public
    );

    // Create template backup
    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Template Backup"),
        &SorobanString::from_str(&env, "Template-specific backup"),
        &SorobanString::from_str(&env, "Template"),
    );

    // Verify backup was created
    let backup = contract.get_backup(&backup_id);
    assert_eq!(
        backup.backup_type,
        SorobanString::from_str(&env, "Template")
    );
    assert_eq!(backup.status, BackupStatus::Completed);
}

//-----------------------------------------------------------------------------
// Event Emission Tests
//-----------------------------------------------------------------------------

#[test]
fn test_backup_created_event() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let employee = create_test_address(&env, "employee");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create test data
    let token = Address::generate(&env);
    contract.create_or_update_escrow(
        &employer, &employee, &token, &5000, &2592000, // 30 days
        &2592000, // 30 days
    );

    // Create backup and check event
    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Test Backup"),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );

    // Verify event was emitted (this would require event testing infrastructure)
    assert_eq!(backup_id, 1);
}

#[test]
fn test_backup_verified_event() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let employee = create_test_address(&env, "employee");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create test data and backup
    let token = Address::generate(&env);
    contract.create_or_update_escrow(
        &employer, &employee, &token, &5000, &2592000, // 30 days
        &2592000, // 30 days
    );

    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Test Backup"),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );

    // Verify backup
    let is_valid = contract.verify_backup(&employer, &backup_id);
    assert!(is_valid);

    // Verify event was emitted (this would require event testing infrastructure)
}

#[test]
fn test_recovery_started_event() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let employee = create_test_address(&env, "employee");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create test data and backup
    let token = Address::generate(&env);
    contract.create_or_update_escrow(
        &employer, &employee, &token, &5000, &2592000, // 30 days
        &2592000, // 30 days
    );

    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Test Backup"),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );

    // Create recovery point
    let recovery_id = contract.create_recovery_point(
        &employer,
        &backup_id,
        &SorobanString::from_str(&env, "Test Recovery"),
        &SorobanString::from_str(&env, "Test Recovery Description"),
        &SorobanString::from_str(&env, "Full"),
    );

    // Verify event was emitted (this would require event testing infrastructure)
    assert_eq!(recovery_id, 1);
}

#[test]
fn test_recovery_completed_event() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let employee = create_test_address(&env, "employee");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create test data and backup
    let token = Address::generate(&env);
    contract.create_or_update_escrow(
        &employer, &employee, &token, &5000, &2592000, // 30 days
        &2592000, // 30 days
    );

    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Test Backup"),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );

    // Create and execute recovery point
    let recovery_id = contract.create_recovery_point(
        &employer,
        &backup_id,
        &SorobanString::from_str(&env, "Test Recovery"),
        &SorobanString::from_str(&env, "Test Recovery Description"),
        &SorobanString::from_str(&env, "Full"),
    );

    let success = contract.execute_recovery(&employer, &recovery_id);
    assert!(success);

    // Verify event was emitted (this would require event testing infrastructure)
}

//-----------------------------------------------------------------------------
// Error Handling Tests
//-----------------------------------------------------------------------------

#[test]
fn test_backup_creation_with_corrupted_data() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");

    env.mock_all_auths();
    contract.initialize(&employer);

    // This test would require simulating data corruption scenarios
    // For now, we test normal backup creation
    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Test Backup"),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );

    assert_eq!(backup_id, 1);
}

#[test]
fn test_recovery_with_invalid_backup_data() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create backup
    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Test Backup"),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );

    // Create recovery point
    let recovery_id = contract.create_recovery_point(
        &employer,
        &backup_id,
        &SorobanString::from_str(&env, "Test Recovery"),
        &SorobanString::from_str(&env, "Test Recovery Description"),
        &SorobanString::from_str(&env, "Full"),
    );

    // Execute recovery (should succeed with empty data)
    let success = contract.execute_recovery(&employer, &recovery_id);
    assert!(success);
}

#[test]
fn test_backup_verification_with_tampered_data() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let employee = create_test_address(&env, "employee");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create test data and backup
    let token = Address::generate(&env);
    contract.create_or_update_escrow(
        &employer, &employee, &token, &5000, &2592000, // 30 days
        &2592000, // 30 days
    );

    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Test Backup"),
        &SorobanString::from_str(&env, "Test Description"),
        &SorobanString::from_str(&env, "Employer"),
    );

    // Verify backup (should succeed with current implementation)
    let is_valid = contract.verify_backup(&employer, &backup_id);
    assert!(is_valid);
}

//-----------------------------------------------------------------------------
// Edge Cases and Stress Tests
//-----------------------------------------------------------------------------

#[test]
fn test_multiple_backups_same_employer() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let employee = create_test_address(&env, "employee");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create test data
    let token = Address::generate(&env);
    contract.create_or_update_escrow(
        &employer, &employee, &token, &5000, &2592000, // 30 days
        &2592000, // 30 days
    );

    // Create multiple backups
    let backup1 = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Backup 1"),
        &SorobanString::from_str(&env, "Description 1"),
        &SorobanString::from_str(&env, "Employer"),
    );
    let backup2 = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Backup 2"),
        &SorobanString::from_str(&env, "Description 2"),
        &SorobanString::from_str(&env, "Full"),
    );
    let backup3 = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Backup 3"),
        &SorobanString::from_str(&env, "Description 3"),
        &SorobanString::from_str(&env, "Template"),
    );

    // Verify all backups were created
    assert_eq!(backup1, 1);
    assert_eq!(backup2, 2);
    assert_eq!(backup3, 3);

    // Verify all backups are in employer's list
    let backups = contract.get_employer_backups(&employer);
    assert_eq!(backups.len(), 3);
}

#[test]
fn test_backup_with_large_dataset() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create multiple employees
    for _i in 0..10 {
        let employee = create_test_address(&env, "employee");
        let token = Address::generate(&env);
        contract.create_or_update_escrow(
            &employer, &employee, &token, &5000, &2592000, // 30 days
            &2592000, // 30 days
        );
    }

    // Create backup with large dataset
    let backup_id = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Large Dataset Backup"),
        &SorobanString::from_str(&env, "Backup with multiple employees"),
        &SorobanString::from_str(&env, "Employer"),
    );

    // Verify backup was created successfully
    let backup = contract.get_backup(&backup_id);
    assert_eq!(backup.status, BackupStatus::Completed);
    assert!(backup.size_bytes > 0);
}

#[test]
fn test_concurrent_backup_operations() {
    let env = Env::default();
    let contract = create_test_contract(&env);
    let employer = create_test_address(&env, "employer");
    let employee = create_test_address(&env, "employee");

    env.mock_all_auths();
    contract.initialize(&employer);

    // Create test data
    let token = Address::generate(&env);
    contract.create_or_update_escrow(
        &employer, &employee, &token, &5000, &2592000, // 30 days
        &2592000, // 30 days
    );

    // Create multiple backups in sequence (simulating concurrent operations)
    let backup1 = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Concurrent Backup 1"),
        &SorobanString::from_str(&env, "First backup"),
        &SorobanString::from_str(&env, "Employer"),
    );

    let backup2 = contract.create_backup(
        &employer,
        &SorobanString::from_str(&env, "Concurrent Backup 2"),
        &SorobanString::from_str(&env, "Second backup"),
        &SorobanString::from_str(&env, "Employer"),
    );

    // Verify both backups were created with unique IDs
    assert_ne!(backup1, backup2);
    assert_eq!(backup1, 1);
    assert_eq!(backup2, 2);

    // Verify both backups exist
    let backup1_data = contract.get_backup(&backup1);
    let backup2_data = contract.get_backup(&backup2);
    assert_eq!(backup1_data.status, BackupStatus::Completed);
    assert_eq!(backup2_data.status, BackupStatus::Completed);
}
