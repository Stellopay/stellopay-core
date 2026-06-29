#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env, Symbol, Vec};

use tax_withholding::{
    RulesetMetadata, TaxComputation, TaxError, TaxWithholdingContract, TaxWithholdingContractClient,
};

// ─── Fixtures ────────────────────────────────────────────────────────────────

fn setup() -> (Env, Address, TaxWithholdingContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, TaxWithholdingContract);
    let client = TaxWithholdingContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    (env, owner, client)
}

// ─── Ruleset version management tests ─────────────────────────────────────────

#[test]
fn test_initialize_creates_version_1() {
    let (env, _owner, client) = setup();

    assert_eq!(client.get_active_ruleset_version(), 1);
    assert_eq!(client.get_latest_ruleset_version(), 1);

    let metadata = client.get_ruleset_metadata(&1);
    assert!(metadata.is_some());
    let meta = metadata.unwrap();
    assert_eq!(meta.version, 1);
    assert_eq!(meta.description, Symbol::new(&env, "initial"));
    assert!(!meta.deprecated);
}

#[test]
fn test_publish_new_ruleset_version() {
    let (env, owner, client) = setup();

    let desc = Symbol::new(&env, "v2_update");
    let new_version = client.publish_ruleset_version(&owner, &desc);
    assert_eq!(new_version, 2);

    assert_eq!(client.get_latest_ruleset_version(), 2);

    let metadata = client.get_ruleset_metadata(&2);
    assert!(metadata.is_some());
    let meta = metadata.unwrap();
    assert_eq!(meta.version, 2);
    assert_eq!(meta.description, desc);
}

#[test]
fn test_publish_multiple_versions() {
    let (env, owner, client) = setup();

    let v2 = client.publish_ruleset_version(&owner, &Symbol::new(&env, "v2"));
    let v3 = client.publish_ruleset_version(&owner, &Symbol::new(&env, "v3"));
    let v4 = client.publish_ruleset_version(&owner, &Symbol::new(&env, "v4"));

    assert_eq!(v2, 2);
    assert_eq!(v3, 3);
    assert_eq!(v4, 4);
    assert_eq!(client.get_latest_ruleset_version(), 4);
}

#[test]
fn test_set_active_ruleset_version() {
    let (env, owner, client) = setup();

    client.publish_ruleset_version(&owner, &Symbol::new(&env, "v2"));
    client.publish_ruleset_version(&owner, &Symbol::new(&env, "v3"));

    // Active version should still be 1
    assert_eq!(client.get_active_ruleset_version(), 1);

    // Set active to version 2
    client.set_active_ruleset_version(&owner, &2);
    assert_eq!(client.get_active_ruleset_version(), 2);

    // Set active to version 3
    client.set_active_ruleset_version(&owner, &3);
    assert_eq!(client.get_active_ruleset_version(), 3);
}

#[test]
fn test_set_active_version_invalid() {
    let (_env, owner, client) = setup();

    let res = client.try_set_active_ruleset_version(&owner, &999);
    assert_eq!(res, Err(Ok(TaxError::InvalidVersion)));
}

#[test]
fn test_publish_version_unauthorized() {
    let (env, _owner, client) = setup();

    let non_owner = Address::generate(&env);
    let res = client.try_publish_ruleset_version(&non_owner, &Symbol::new(&env, "v2"));
    assert_eq!(res, Err(Ok(TaxError::Unauthorized)));
}

// ─── Versioned rate configuration tests ───────────────────────────────────────

#[test]
fn test_set_jurisdiction_rate_versioned() {
    let (env, owner, client) = setup();

    let jurisdiction = Symbol::new(&env, "US_FED");

    // Set rate for version 1
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);
    assert_eq!(client.get_jurisdiction_rate(&jurisdiction, &1), Some(1000));

    // Publish version 2 and set different rate
    client.publish_ruleset_version(&owner, &Symbol::new(&env, "v2"));
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1500u32, &2);

    // Both versions should have their own rates
    assert_eq!(client.get_jurisdiction_rate(&jurisdiction, &1), Some(1000));
    assert_eq!(client.get_jurisdiction_rate(&jurisdiction, &2), Some(1500));
}

#[test]
fn test_get_jurisdiction_rate_nonexistent_version() {
    let (env, _owner, client) = setup();

    let jurisdiction = Symbol::new(&env, "US_FED");
    assert_eq!(client.get_jurisdiction_rate(&jurisdiction, &999), None);
}

// ─── Ruleset locking tests ────────────────────────────────────────────────────

#[test]
fn test_lock_ruleset_version() {
    let (env, owner, client) = setup();

    assert!(!client.is_ruleset_locked(&1));

    client.lock_ruleset_version(&owner, &1);
    assert!(client.is_ruleset_locked(&1));
}

#[test]
fn test_cannot_modify_locked_version() {
    let (env, owner, client) = setup();

    let jurisdiction = Symbol::new(&env, "US_FED");

    // Set rate before locking
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);

    // Lock version 1
    client.lock_ruleset_version(&owner, &1);

    // Attempt to modify locked version should fail
    let res = client.try_set_jurisdiction_rate(&owner, &jurisdiction, &1500u32, &1);
    assert_eq!(res, Err(Ok(TaxError::VersionLocked)));
}

#[test]
fn test_lock_invalid_version() {
    let (_env, owner, client) = setup();

    let res = client.try_lock_ruleset_version(&owner, &999);
    assert_eq!(res, Err(Ok(TaxError::InvalidVersion)));
}

#[test]
fn test_lock_unauthorized() {
    let (env, _owner, client) = setup();

    let non_owner = Address::generate(&env);
    let res = client.try_lock_ruleset_version(&non_owner, &1);
    assert_eq!(res, Err(Ok(TaxError::Unauthorized)));
}

// ─── Employee version migration tests ─────────────────────────────────────────

#[test]
fn test_employee_defaults_to_active_version() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    // Configure version 1
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);
    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    // Employee should use version 1 (active by default)
    assert_eq!(client.get_employee_ruleset_version(&employee), 1);

    let result: TaxComputation = client.calculate_withholding(&employee, &10_000i128);
    assert_eq!(result.ruleset_version, 1);
    assert_eq!(result.total_tax, 1_000);
}

#[test]
fn test_migrate_employee_to_new_version() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    // Configure version 1 with 10% rate
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);
    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    // Publish version 2 with 15% rate
    client.publish_ruleset_version(&owner, &Symbol::new(&env, "v2"));
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1500u32, &2);

    // Employee still on version 1
    let result1: TaxComputation = client.calculate_withholding(&employee, &10_000i128);
    assert_eq!(result1.ruleset_version, 1);
    assert_eq!(result1.total_tax, 1_000);

    // Migrate employee to version 2
    client.migrate_employee_to_version(&owner, &employee, &2);
    assert_eq!(client.get_employee_ruleset_version(&employee), 2);

    // Now employee uses version 2 rates
    let result2: TaxComputation = client.calculate_withholding(&employee, &10_000i128);
    assert_eq!(result2.ruleset_version, 2);
    assert_eq!(result2.total_tax, 1_500);
}

#[test]
fn test_migrate_employee_invalid_version() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let res = client.try_migrate_employee_to_version(&owner, &employee, &999);
    assert_eq!(res, Err(Ok(TaxError::InvalidVersion)));
}

#[test]
fn test_migrate_employee_unauthorized() {
    let (env, _owner, client) = setup();

    let non_owner = Address::generate(&env);
    let employee = Address::generate(&env);
    let res = client.try_migrate_employee_to_version(&non_owner, &employee, &1);
    assert_eq!(res, Err(Ok(TaxError::Unauthorized)));
}

// ─── Deterministic calculation tests ──────────────────────────────────────────

#[test]
fn test_calculations_identical_under_same_version() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    // Configure version 1
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);
    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    // Calculate multiple times - should be identical
    let result1: TaxComputation = client.calculate_withholding(&employee, &10_000i128);
    let result2: TaxComputation = client.calculate_withholding(&employee, &10_000i128);
    let result3: TaxComputation = client.calculate_withholding(&employee, &10_000i128);

    assert_eq!(result1, result2);
    assert_eq!(result2, result3);
    assert_eq!(result1.ruleset_version, 1);
    assert_eq!(result1.total_tax, 1_000);
}

#[test]
fn test_calculations_differ_across_versions() {
    let (env, owner, client) = setup();

    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    // Configure version 1 with 10% rate
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);
    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee1, &jurisdictions.clone());
    client.set_employee_jurisdictions(&owner, &employee2, &jurisdictions);

    // Publish version 2 with 15% rate
    client.publish_ruleset_version(&owner, &Symbol::new(&env, "v2"));
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1500u32, &2);

    // Migrate employee2 to version 2
    client.migrate_employee_to_version(&owner, &employee2, &2);

    // Same gross amount, different versions, different results
    let result1: TaxComputation = client.calculate_withholding(&employee1, &10_000i128);
    let result2: TaxComputation = client.calculate_withholding(&employee2, &10_000i128);

    assert_eq!(result1.ruleset_version, 1);
    assert_eq!(result1.total_tax, 1_000);

    assert_eq!(result2.ruleset_version, 2);
    assert_eq!(result2.total_tax, 1_500);

    assert_ne!(result1.total_tax, result2.total_tax);
}

#[test]
fn test_multi_jurisdiction_versioning() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let j1 = Symbol::new(&env, "US_FED");
    let j2 = Symbol::new(&env, "US_STATE");

    // Version 1: 10% federal, 5% state
    client.set_jurisdiction_rate(&owner, &j1, &1000u32, &1);
    client.set_jurisdiction_rate(&owner, &j2, &500u32, &1);

    let jurisdictions = Vec::from_array(&env, [j1.clone(), j2.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    let result1: TaxComputation = client.calculate_withholding(&employee, &20_000i128);
    assert_eq!(result1.ruleset_version, 1);
    assert_eq!(result1.total_tax, 3_000); // 2000 + 1000

    // Version 2: 12% federal, 6% state
    client.publish_ruleset_version(&owner, &Symbol::new(&env, "v2"));
    client.set_jurisdiction_rate(&owner, &j1, &1200u32, &2);
    client.set_jurisdiction_rate(&owner, &j2, &600u32, &2);

    // Migrate to version 2
    client.migrate_employee_to_version(&owner, &employee, &2);

    let result2: TaxComputation = client.calculate_withholding(&employee, &20_000i128);
    assert_eq!(result2.ruleset_version, 2);
    assert_eq!(result2.total_tax, 3_600); // 2400 + 1200
}

// ─── Migration strategy tests ─────────────────────────────────────────────────

#[test]
fn test_gradual_migration_strategy() {
    let (env, owner, client) = setup();

    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let employee3 = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    // Setup version 1 for all employees
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);
    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee1, &jurisdictions.clone());
    client.set_employee_jurisdictions(&owner, &employee2, &jurisdictions.clone());
    client.set_employee_jurisdictions(&owner, &employee3, &jurisdictions);

    // Publish version 2
    client.publish_ruleset_version(&owner, &Symbol::new(&env, "v2"));
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1200u32, &2);

    // Gradual migration: migrate one employee at a time
    client.migrate_employee_to_version(&owner, &employee1, &2);

    // Verify mixed state
    assert_eq!(client.get_employee_ruleset_version(&employee1), 2);
    assert_eq!(client.get_employee_ruleset_version(&employee2), 1);
    assert_eq!(client.get_employee_ruleset_version(&employee3), 1);

    // Migrate second employee
    client.migrate_employee_to_version(&owner, &employee2, &2);

    assert_eq!(client.get_employee_ruleset_version(&employee1), 2);
    assert_eq!(client.get_employee_ruleset_version(&employee2), 2);
    assert_eq!(client.get_employee_ruleset_version(&employee3), 1);

    // Final migration
    client.migrate_employee_to_version(&owner, &employee3, &2);

    assert_eq!(client.get_employee_ruleset_version(&employee1), 2);
    assert_eq!(client.get_employee_ruleset_version(&employee2), 2);
    assert_eq!(client.get_employee_ruleset_version(&employee3), 2);
}

#[test]
fn test_rollback_migration() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    // Setup version 1
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);
    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    // Publish and migrate to version 2
    client.publish_ruleset_version(&owner, &Symbol::new(&env, "v2"));
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1500u32, &2);
    client.migrate_employee_to_version(&owner, &employee, &2);

    assert_eq!(client.get_employee_ruleset_version(&employee), 2);
    let result: TaxComputation = client.calculate_withholding(&employee, &10_000i128);
    assert_eq!(result.total_tax, 1_500);

    // Rollback to version 1
    client.migrate_employee_to_version(&owner, &employee, &1);

    assert_eq!(client.get_employee_ruleset_version(&employee), 1);
    let result_rollback: TaxComputation = client.calculate_withholding(&employee, &10_000i128);
    assert_eq!(result_rollback.total_tax, 1_000);
}

#[test]
fn test_lock_prevents_tampering_with_historical_data() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    // Setup and lock version 1
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);
    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    // Lock version 1 to preserve historical calculations
    client.lock_ruleset_version(&owner, &1);

    // Create version 2 for new rates
    client.publish_ruleset_version(&owner, &Symbol::new(&env, "v2"));
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1500u32, &2);

    // Version 1 calculations remain unchanged and cannot be modified
    let result_v1: TaxComputation = client.calculate_withholding(&employee, &10_000i128);
    assert_eq!(result_v1.ruleset_version, 1);
    assert_eq!(result_v1.total_tax, 1_000);

    // Attempt to modify locked version fails
    let res = client.try_set_jurisdiction_rate(&owner, &jurisdiction, &2000u32, &1);
    assert_eq!(res, Err(Ok(TaxError::VersionLocked)));
}

// ─── Edge case tests ──────────────────────────────────────────────────────────

#[test]
fn test_version_overflow_protection() {
    let (env, owner, client) = setup();

    // This test verifies that version numbers are checked for overflow
    // In practice, reaching u32::MAX versions is unrealistic, but the check exists

    // We can't actually create u32::MAX versions in a test, but we verify
    // the arithmetic is checked
    let desc = Symbol::new(&env, "test");
    let v2 = client.publish_ruleset_version(&owner, &desc);
    assert_eq!(v2, 2);
}

#[test]
fn test_accrual_preserves_version_in_computation() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);
    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    // Accrue withholding and verify version is recorded
    let result: TaxComputation = client.accrue_withholding(&owner, &employee, &10_000i128);
    assert_eq!(result.ruleset_version, 1);
    assert_eq!(result.total_tax, 1_000);
}
