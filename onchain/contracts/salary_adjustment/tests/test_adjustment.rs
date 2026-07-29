#![cfg(test)]
#![allow(deprecated)]

use salary_adjustment::{
    AdjustmentKind, AdjustmentStatus, AdjustmentType, SalaryAdjustmentContract,
    SalaryAdjustmentContractClient, DEFAULT_MAX_SALARY,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env, Symbol,
};

// ============================================================================
// TEST HELPERS
// ============================================================================

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn create_contract(env: &Env) -> SalaryAdjustmentContractClient<'_> {
    let contract_id = env.register_contract(None, SalaryAdjustmentContract);
    SalaryAdjustmentContractClient::new(env, &contract_id)
}

fn set_time(env: &Env, timestamp: u64) {
    env.ledger().with_mut(|ledger| {
        ledger.timestamp = timestamp;
    });
}

fn reason_hash(env: &Env, marker: u8) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[0] = marker;
    BytesN::from_array(env, &bytes)
}

// ============================================================================
// INITIALIZATION TESTS
// ============================================================================

#[test]
fn test_initialize_stores_owner() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);

    client.initialize(&owner);

    assert_eq!(client.get_owner(), Some(owner));
}

#[test]
#[should_panic(expected = "Contract already initialized")]
fn test_double_initialization() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);

    client.initialize(&owner);
    client.initialize(&owner);
}

#[test]
#[should_panic(expected = "Contract not initialized")]
fn test_contract_not_initialized_create_panics() {
    let env = create_env();
    let client = create_contract(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &100);
}

#[test]
#[should_panic(expected = "Contract not initialized")]
fn test_contract_not_initialized_approve_panics() {
    let env = create_env();
    let client = create_contract(&env);
    let approver = Address::generate(&env);

    client.approve_adjustment(&approver, &1);
}

// ============================================================================
// CREATE ADJUSTMENT TESTS
// ============================================================================

#[test]
fn test_create_salary_increase() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &7_000, &1_000);

    let stored = client.get_adjustment(&id).unwrap();
    assert_eq!(stored.kind, AdjustmentKind::Increase);
    assert_eq!(stored.status, AdjustmentStatus::Pending);
    assert_eq!(stored.current_salary, 5_000);
    assert_eq!(stored.new_salary, 7_000);
    assert_eq!(stored.effective_date, 1_000);
}

#[test]
fn test_create_salary_decrease() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &10_000, &8_000, &500);

    let stored = client.get_adjustment(&id).unwrap();
    assert_eq!(stored.kind, AdjustmentKind::Decrease);
    assert_eq!(stored.status, AdjustmentStatus::Pending);
    assert_eq!(stored.current_salary, 10_000);
    assert_eq!(stored.new_salary, 8_000);
}

#[test]
fn test_create_records_creation_timestamp() {
    let env = create_env();
    set_time(&env, 500);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &600);

    let stored = client.get_adjustment(&id).unwrap();
    assert_eq!(stored.created_at, 500);
    assert_eq!(stored.effective_date, 600);
}

#[test]
fn test_create_multiple_adjustments_increments_id() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id1 = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &100);
    let id2 = client.create_adjustment(&employer, &employee, &approver, &5_000, &7_000, &200);

    assert_eq!(id2, id1 + 1);
    assert!(client.get_adjustment(&id1).is_some());
    assert!(client.get_adjustment(&id2).is_some());
}

#[test]
#[should_panic(expected = "Current salary must be positive")]
fn test_zero_current_salary_rejected() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);
    client.create_adjustment(&employer, &employee, &approver, &0, &5_000, &100);
}

#[test]
#[should_panic(expected = "New salary must be positive")]
fn test_zero_new_salary_rejected() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);
    client.create_adjustment(&employer, &employee, &approver, &5_000, &0, &100);
}

#[test]
#[should_panic(expected = "New salary must differ from current salary")]
fn test_same_salary_rejected() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);
    client.create_adjustment(&employer, &employee, &approver, &5_000, &5_000, &100);
}

#[test]
#[should_panic(expected = "Effective date cannot be in the past")]
fn test_retroactive_adjustment_rejected() {
    let env = create_env();
    set_time(&env, 1_000);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);
    // effective_date (500) is before current ledger time (1_000)
    client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &500);
}

#[test]
fn test_effective_date_equals_current_time_allowed() {
    let env = create_env();
    set_time(&env, 1_000);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);
    // effective_date == now is valid (boundary)
    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &1_000);
    assert!(client.get_adjustment(&id).is_some());
}

#[test]
fn test_retroactive_adjustment_blocked_by_default() {
    let env = create_env();
    set_time(&env, 1_000);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let result =
        client.try_create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &999);
    assert!(result.is_err());
    assert_eq!(client.get_audit_log_count(), 0);
}

#[test]
fn test_authorized_retroactive_adjustment_works_and_is_logged() {
    let env = create_env();
    set_time(&env, 1_000);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let raw_reason_hash = reason_hash(&env, 7);

    client.initialize(&owner);

    let id = client.create_retroactive_adjustment(
        &owner,
        &employer,
        &employee,
        &approver,
        &5_000,
        &6_000,
        &500,
        &raw_reason_hash,
    );

    let stored = client.get_adjustment(&id).unwrap();
    assert!(stored.retroactive);
    assert_eq!(stored.retroactive_approved_by, Some(owner.clone()));
    assert!(stored.reason_hash.is_some());
    assert_ne!(stored.reason_hash.clone().unwrap(), raw_reason_hash);
    assert_eq!(stored.created_at, 1_000);
    assert_eq!(stored.effective_date, 500);

    assert_eq!(client.get_audit_log_count(), 1);
    let audit = client.get_audit_log(&1).unwrap();
    assert_eq!(audit.adjustment_id, Some(id));
    assert_eq!(audit.actor, employer.clone());
    assert_eq!(audit.action, Symbol::new(&env, "adjustment_created"));
    assert_eq!(audit.employee, Some(employee.clone()));
    assert_eq!(audit.amount, Some(6_000));
    assert_eq!(audit.reason_hash, stored.reason_hash.clone());

    client.approve_adjustment(&approver, &id);
    client.apply_adjustment(&employer, &id);

    let applied = client.get_adjustment(&id).unwrap();
    assert_eq!(applied.status, AdjustmentStatus::Applied);
    assert_eq!(applied.reason_hash, stored.reason_hash);
    assert_eq!(client.get_employee_salary(&employee), Some(6_000));
    assert_eq!(client.get_audit_log_count(), 3);
}

#[test]
fn test_non_owner_cannot_authorize_retroactive_adjustment() {
    let env = create_env();
    set_time(&env, 1_000);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let result = client.try_create_retroactive_adjustment(
        &attacker,
        &employer,
        &employee,
        &approver,
        &5_000,
        &6_000,
        &500,
        &reason_hash(&env, 8),
    );
    assert!(result.is_err());
    assert_eq!(client.get_audit_log_count(), 0);
}

#[test]
fn test_zero_retroactive_reason_hash_rejected() {
    let env = create_env();
    set_time(&env, 1_000);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let zero_hash = BytesN::from_array(&env, &[0; 32]);

    client.initialize(&owner);

    let result = client.try_create_retroactive_adjustment(
        &owner, &employer, &employee, &approver, &5_000, &6_000, &500, &zero_hash,
    );
    assert!(result.is_err());
    assert_eq!(client.get_audit_log_count(), 0);
}

#[test]
fn test_conflicting_same_employee_effective_date_rejected() {
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let first = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &200);
    assert_eq!(first, 1);

    let result =
        client.try_create_adjustment(&employer, &employee, &approver, &6_000, &7_000, &200);
    assert!(result.is_err());
}

#[test]
fn test_same_employee_distinct_effective_dates_allowed() {
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let first = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &200);
    let second = client.create_adjustment(&employer, &employee, &approver, &6_000, &7_000, &201);

    assert_eq!(second, first + 1);
    assert_eq!(client.get_audit_log_count(), 2);
}

// ============================================================================
// SALARY CAP TESTS
// ============================================================================

#[test]
fn test_get_salary_cap_returns_default() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);

    client.initialize(&owner);

    assert_eq!(client.get_salary_cap(), DEFAULT_MAX_SALARY);
}

#[test]
fn test_set_salary_cap_and_get() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);

    client.initialize(&owner);
    client.set_salary_cap(&owner, &500_000);

    assert_eq!(client.get_salary_cap(), 500_000);
}

#[test]
#[should_panic(expected = "Only owner can set salary cap")]
fn test_non_owner_cannot_set_salary_cap() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);

    client.initialize(&owner);
    client.set_salary_cap(&attacker, &500_000);
}

#[test]
#[should_panic(expected = "Salary cap must be positive")]
fn test_zero_salary_cap_rejected() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);

    client.initialize(&owner);
    client.set_salary_cap(&owner, &0);
}

#[test]
#[should_panic(expected = "New salary exceeds salary cap")]
fn test_salary_cap_enforced_on_increase() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);
    client.set_salary_cap(&owner, &10_000);
    // new_salary (15_000) > cap (10_000)
    client.create_adjustment(&employer, &employee, &approver, &5_000, &15_000, &100);
}

#[test]
fn test_new_salary_at_cap_boundary_allowed() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);
    client.set_salary_cap(&owner, &10_000);
    // new_salary == cap is valid
    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &10_000, &100);
    assert!(client.get_adjustment(&id).is_some());
}

#[test]
fn test_decrease_below_cap_allowed() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);
    client.set_salary_cap(&owner, &10_000);
    // decreases are always within cap
    let id = client.create_adjustment(&employer, &employee, &approver, &8_000, &6_000, &100);
    let stored = client.get_adjustment(&id).unwrap();
    assert_eq!(stored.kind, AdjustmentKind::Decrease);
}

#[test]
fn test_updated_cap_is_respected() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);
    client.set_salary_cap(&owner, &20_000);

    // 15_000 is within first cap
    client.create_adjustment(&employer, &employee, &approver, &5_000, &15_000, &100);

    // Tighten cap
    client.set_salary_cap(&owner, &12_000);

    // 15_000 now exceeds new cap — must fail
    let result =
        client.try_create_adjustment(&employer, &employee, &approver, &5_000, &15_000, &200);
    assert!(result.is_err());
}

// ============================================================================
// APPROVAL / REJECTION TESTS
// ============================================================================

#[test]
fn test_approve_adjustment_changes_status() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_500, &1_000);

    client.approve_adjustment(&approver, &id);
    let stored = client.get_adjustment(&id).unwrap();
    assert_eq!(stored.status, AdjustmentStatus::Approved);
}

#[test]
#[should_panic(expected = "Only approver can approve")]
fn test_only_approver_can_approve() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let attacker = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &100);
    client.approve_adjustment(&attacker, &id);
}

#[test]
#[should_panic(expected = "Only approver can reject")]
fn test_only_approver_can_reject() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let attacker = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &100);
    client.reject_adjustment(&attacker, &id);
}

#[test]
#[should_panic(expected = "Adjustment is not pending")]
fn test_cannot_approve_already_approved() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &100);
    client.approve_adjustment(&approver, &id);
    client.approve_adjustment(&approver, &id); // second approve must fail
}

#[test]
#[should_panic(expected = "Adjustment is not pending")]
fn test_cannot_reject_after_approval() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &100);
    client.approve_adjustment(&approver, &id);
    client.reject_adjustment(&approver, &id);
}

#[test]
fn test_reject_adjustment_changes_status() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &100);
    client.reject_adjustment(&approver, &id);

    let stored = client.get_adjustment(&id).unwrap();
    assert_eq!(stored.status, AdjustmentStatus::Rejected);
}

// ============================================================================
// APPLY ADJUSTMENT TESTS
// ============================================================================

#[test]
fn test_approve_and_apply_adjustment() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_500, &1_000);

    client.approve_adjustment(&approver, &id);
    set_time(&env, 1_200);
    client.apply_adjustment(&employer, &id);

    let applied = client.get_adjustment(&id).unwrap();
    assert_eq!(applied.status, AdjustmentStatus::Applied);
}

#[test]
fn test_apply_at_exact_effective_date() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &8_000, &1_000);

    client.approve_adjustment(&approver, &id);
    set_time(&env, 1_000);
    client.apply_adjustment(&employer, &id);

    let stored = client.get_adjustment(&id).unwrap();
    assert_eq!(stored.status, AdjustmentStatus::Applied);
}

#[test]
#[should_panic(expected = "Effective date not reached")]
fn test_apply_before_effective_date() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &7_000, &2_000);

    client.approve_adjustment(&approver, &id);
    set_time(&env, 500);
    client.apply_adjustment(&employer, &id);
}

#[test]
#[should_panic(expected = "Adjustment is not approved")]
fn test_apply_unapproved_adjustment() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &7_000, &100);
    set_time(&env, 200);
    client.apply_adjustment(&employer, &id);
}

#[test]
#[should_panic(expected = "Only employer can apply")]
fn test_non_employer_cannot_apply() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let attacker = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &7_000, &100);
    client.approve_adjustment(&approver, &id);
    set_time(&env, 200);
    client.apply_adjustment(&attacker, &id);
}

// ============================================================================
// CANCEL ADJUSTMENT TESTS
// ============================================================================

#[test]
fn test_cancel_pending_adjustment() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &100);
    client.cancel_adjustment(&employer, &id);

    let stored = client.get_adjustment(&id).unwrap();
    assert_eq!(stored.status, AdjustmentStatus::Cancelled);
}

#[test]
fn test_reject_then_cancel() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &100);
    client.reject_adjustment(&approver, &id);

    let rejected = client.get_adjustment(&id).unwrap();
    assert_eq!(rejected.status, AdjustmentStatus::Rejected);

    client.cancel_adjustment(&employer, &id);
    let cancelled = client.get_adjustment(&id).unwrap();
    assert_eq!(cancelled.status, AdjustmentStatus::Cancelled);
}

#[test]
#[should_panic(expected = "Adjustment cannot be cancelled")]
fn test_cannot_cancel_approved_adjustment() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &100);
    client.approve_adjustment(&approver, &id);
    client.cancel_adjustment(&employer, &id);
}

#[test]
#[should_panic(expected = "Adjustment cannot be cancelled")]
fn test_cannot_cancel_applied_adjustment() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &100);
    client.approve_adjustment(&approver, &id);
    set_time(&env, 200);
    client.apply_adjustment(&employer, &id);
    client.cancel_adjustment(&employer, &id);
}

#[test]
#[should_panic(expected = "Only employer can cancel")]
fn test_non_employer_cannot_cancel() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let attacker = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &100);
    client.cancel_adjustment(&attacker, &id);
}

// ============================================================================
// PAYROLL VISIBILITY TESTS
// ============================================================================

#[test]
fn test_get_employee_salary_returns_none_before_apply() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employee = Address::generate(&env);

    client.initialize(&owner);

    assert_eq!(client.get_employee_salary(&employee), None);
}

#[test]
fn test_get_employee_salary_after_apply() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &8_000, &100);
    client.approve_adjustment(&approver, &id);
    set_time(&env, 200);
    client.apply_adjustment(&employer, &id);

    assert_eq!(client.get_employee_salary(&employee), Some(8_000));
}

#[test]
fn test_multiple_applied_adjustments_salary_tracks_latest() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    // First adjustment: 5_000 -> 8_000
    let id1 = client.create_adjustment(&employer, &employee, &approver, &5_000, &8_000, &100);
    client.approve_adjustment(&approver, &id1);
    set_time(&env, 200);
    client.apply_adjustment(&employer, &id1);

    assert_eq!(client.get_employee_salary(&employee), Some(8_000));

    // Second adjustment: 8_000 -> 10_000
    let id2 = client.create_adjustment(&employer, &employee, &approver, &8_000, &10_000, &300);
    client.approve_adjustment(&approver, &id2);
    set_time(&env, 400);
    client.apply_adjustment(&employer, &id2);

    assert_eq!(client.get_employee_salary(&employee), Some(10_000));
}

#[test]
fn test_employee_salaries_are_independent() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee_a = Address::generate(&env);
    let employee_b = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id_a = client.create_adjustment(&employer, &employee_a, &approver, &5_000, &7_000, &100);
    client.approve_adjustment(&approver, &id_a);
    set_time(&env, 200);
    client.apply_adjustment(&employer, &id_a);

    // employee_b has no adjustments yet
    assert_eq!(client.get_employee_salary(&employee_a), Some(7_000));
    assert_eq!(client.get_employee_salary(&employee_b), None);
}

// ============================================================================
// CONCURRENT PENDING PROPOSAL TESTS
// ============================================================================

#[test]
fn test_concurrent_pending_proposals_same_effective_date_rejected() {
    // Verifies that a second proposal for the same employee with the same
    // effective date is rejected while the first proposal is still pending.
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    // Create first pending proposal
    let id1 = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &200);
    let adj1 = client.get_adjustment(&id1).unwrap();
    assert_eq!(adj1.status, AdjustmentStatus::Pending);

    // Attempt to create second proposal for same employee and effective date
    let result =
        client.try_create_adjustment(&employer, &employee, &approver, &5_000, &7_000, &200);
    assert!(
        result.is_err(),
        "Second proposal with same effective date must be rejected"
    );
}

#[test]
fn test_concurrent_pending_proposals_different_effective_dates_allowed() {
    // Verifies that multiple pending proposals for the same employee are
    // allowed as long as they have different effective dates.
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    // Create first pending proposal with effective_date=200
    let id1 = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &200);
    let adj1 = client.get_adjustment(&id1).unwrap();
    assert_eq!(adj1.status, AdjustmentStatus::Pending);

    // Create second pending proposal with effective_date=300
    let id2 = client.create_adjustment(&employer, &employee, &approver, &6_000, &7_000, &300);
    let adj2 = client.get_adjustment(&id2).unwrap();
    assert_eq!(adj2.status, AdjustmentStatus::Pending);

    // Verify both proposals exist independently
    assert_eq!(id2, id1 + 1);
    assert_eq!(adj1.effective_date, 200);
    assert_eq!(adj2.effective_date, 300);
    assert_eq!(adj1.new_salary, 6_000);
    assert_eq!(adj2.new_salary, 7_000);
}

#[test]
fn test_approve_adjustment_targets_specific_proposal_id() {
    // Verifies that approve_adjustment acts on the specific proposal id,
    // not an ambiguous "latest" pointer, when multiple pending proposals exist.
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    // Create two pending proposals with different effective dates
    let id1 = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &200);
    let id2 = client.create_adjustment(&employer, &employee, &approver, &6_000, &7_500, &300);

    // Approve only the first proposal
    client.approve_adjustment(&approver, &id1);

    // Verify first proposal is approved
    let adj1 = client.get_adjustment(&id1).unwrap();
    assert_eq!(adj1.status, AdjustmentStatus::Approved);
    assert_eq!(adj1.new_salary, 6_000);

    // Verify second proposal remains pending
    let adj2 = client.get_adjustment(&id2).unwrap();
    assert_eq!(adj2.status, AdjustmentStatus::Pending);
    assert_eq!(adj2.new_salary, 7_500);
}

#[test]
fn test_apply_adjustment_targets_specific_proposal_id() {
    // Verifies that apply_adjustment acts on the specific proposal id,
    // ensuring the correct salary change is applied when multiple proposals
    // exist for the same employee.
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    // Create and approve two proposals with different effective dates
    let id1 = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &200);
    let id2 = client.create_adjustment(&employer, &employee, &approver, &6_000, &8_000, &300);

    client.approve_adjustment(&approver, &id1);
    client.approve_adjustment(&approver, &id2);

    // Apply only the first proposal
    set_time(&env, 250);
    client.apply_adjustment(&employer, &id1);

    // Verify employee salary reflects the first proposal
    assert_eq!(client.get_employee_salary(&employee), Some(6_000));

    // Verify first proposal is applied
    let adj1 = client.get_adjustment(&id1).unwrap();
    assert_eq!(adj1.status, AdjustmentStatus::Applied);

    // Verify second proposal is still approved but not applied
    let adj2 = client.get_adjustment(&id2).unwrap();
    assert_eq!(adj2.status, AdjustmentStatus::Approved);

    // Apply the second proposal
    set_time(&env, 350);
    client.apply_adjustment(&employer, &id2);

    // Verify employee salary now reflects the second proposal
    assert_eq!(client.get_employee_salary(&employee), Some(8_000));

    let adj2_applied = client.get_adjustment(&id2).unwrap();
    assert_eq!(adj2_applied.status, AdjustmentStatus::Applied);
}

#[test]
fn test_cancel_one_pending_proposal_does_not_affect_other() {
    // Verifies that cancelling one pending proposal does not affect
    // other pending proposals for the same employee.
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    // Create two pending proposals
    let id1 = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &200);
    let id2 = client.create_adjustment(&employer, &employee, &approver, &6_000, &7_000, &300);

    // Cancel the first proposal
    client.cancel_adjustment(&employer, &id1);

    // Verify first proposal is cancelled
    let adj1 = client.get_adjustment(&id1).unwrap();
    assert_eq!(adj1.status, AdjustmentStatus::Cancelled);

    // Verify second proposal remains pending
    let adj2 = client.get_adjustment(&id2).unwrap();
    assert_eq!(adj2.status, AdjustmentStatus::Pending);
}

#[test]
fn test_reject_one_pending_proposal_does_not_affect_other() {
    // Verifies that rejecting one pending proposal does not affect
    // other pending proposals for the same employee.
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    // Create two pending proposals
    let id1 = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &200);
    let id2 = client.create_adjustment(&employer, &employee, &approver, &6_000, &7_000, &300);

    // Reject the first proposal
    client.reject_adjustment(&approver, &id1);

    // Verify first proposal is rejected
    let adj1 = client.get_adjustment(&id1).unwrap();
    assert_eq!(adj1.status, AdjustmentStatus::Rejected);

    // Verify second proposal remains pending
    let adj2 = client.get_adjustment(&id2).unwrap();
    assert_eq!(adj2.status, AdjustmentStatus::Pending);

    // Approve and apply the second proposal
    client.approve_adjustment(&approver, &id2);
    set_time(&env, 350);
    client.apply_adjustment(&employer, &id2);

    assert_eq!(client.get_employee_salary(&employee), Some(7_000));
}

#[test]
fn test_reuse_effective_date_after_cancellation() {
    // Verifies that the same effective date can be reused for a new proposal
    // after the original proposal is cancelled.
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    // Create first proposal
    let id1 = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &200);

    // Cancel it
    client.cancel_adjustment(&employer, &id1);

    // Attempt to create a new proposal with the same effective date
    // This should still fail because the slot reservation is not cleared
    let result =
        client.try_create_adjustment(&employer, &employee, &approver, &5_000, &7_000, &200);

    // The current implementation does NOT clear the reservation slot on cancellation,
    // so this will fail. This test documents the current behavior.
    assert!(
        result.is_err(),
        "Effective date slot is not freed on cancellation in current implementation"
    );
}

// ============================================================================
// QUERY TESTS
// ============================================================================

#[test]
fn test_get_nonexistent_adjustment() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);

    client.initialize(&owner);

    assert!(client.get_adjustment(&999).is_none());
}

#[test]
fn test_get_owner() {
    let env = create_env();
    let client = create_contract(&env);
    let owner = Address::generate(&env);

    client.initialize(&owner);

    assert_eq!(client.get_owner(), Some(owner));
}

// ============================================================================
// RETROACTIVE ADJUSTMENT AFTER CLAIMED PERIODS
//
// These tests verify that a retroactive adjustment:
//   1. Updates the salary going forward when applied.
//   2. Does NOT retroactively alter already-processed payroll periods. The salary_adjustment
//      contract has no claw-back or top-up mechanism for past claims. The effective_date is a
//      constraint on when the adjustment can be applied, not a trigger for retroactive
//      recalculation.
// ============================================================================

#[test]
fn test_retroactive_adjustment_updates_salary_going_forward() {
    let env = create_env();
    set_time(&env, 1000);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    // Step 1: Create and apply a normal forward adjustment at T=1000.
    //         This simulates the initial salary being updated to 8_000.
    let id1 = client.create_adjustment(&employer, &employee, &approver, &5_000, &8_000, &1000);
    client.approve_adjustment(&approver, &id1);
    client.apply_adjustment(&employer, &id1);
    assert_eq!(client.get_employee_salary(&employee), Some(8_000));

    // Step 2: Time passes. In a real system, payroll periods are claimed at
    //         salary 8_000 during this interval. The salary_adjustment contract
    //         does not track individual claims — it only stores the latest
    //         applied salary for future visibility.
    set_time(&env, 2000);

    // Step 3: Create a retroactive adjustment with effective_date=500,
    //         which is before the first adjustment was applied. The owner
    //         must authorize retroactive adjustments explicitly.
    let id2 = client.create_retroactive_adjustment(
        &owner,
        &employer,
        &employee,
        &approver,
        &8_000,
        &10_000,
        &500,
        &reason_hash(&env, 42),
    );
    let adj = client.get_adjustment(&id2).unwrap();
    assert!(adj.retroactive, "adjustment must be marked retroactive");

    client.approve_adjustment(&approver, &id2);
    client.apply_adjustment(&employer, &id2);

    // Step 4: The salary is updated going forward. The new salary takes
    //         effect from the time `apply_adjustment` was called (T=2000).
    assert_eq!(
        client.get_employee_salary(&employee),
        Some(10_000),
        "salary must reflect the new value after apply"
    );

    // Step 5: Verify the retroactive adjustment is correctly recorded.
    let applied = client.get_adjustment(&id2).unwrap();
    assert_eq!(applied.status, AdjustmentStatus::Applied);
    assert_eq!(applied.effective_date, 500);
    assert_eq!(applied.current_salary, 8_000);
    assert_eq!(applied.new_salary, 10_000);
    assert_eq!(
        applied.retroactive_approved_by,
        Some(owner.clone()),
        "retroactive approval must be recorded"
    );

    // Step 6: The earlier salary (8_000) is no longer directly accessible
    //         via get_employee_salary — only the latest applied salary is
    //         stored. This is the expected forward-only behavior. If a
    //         period was already claimed at 8_000, it is NOT retroactively
    //         topped up to 10_000 by this contract.
    assert_eq!(
        client.get_employee_salary(&employee),
        Some(10_000),
        "only the latest salary is visible; no retroactive recalculation"
    );
}

#[test]
fn test_retroactive_adjustment_does_not_affect_prior_normal_adjustment() {
    // Verifies that a retroactive adjustment with effective_date before an
    // earlier normal adjustment does not retroactively change the salary
    // that was in effect when the earlier adjustment was applied.
    let env = create_env();
    set_time(&env, 1000);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    // Apply normal adjustment at T=1000: 5_000 -> 8_000
    let id1 = client.create_adjustment(&employer, &employee, &approver, &5_000, &8_000, &1000);
    client.approve_adjustment(&approver, &id1);
    client.apply_adjustment(&employer, &id1);
    assert_eq!(client.get_employee_salary(&employee), Some(8_000));

    // Time passes, then create a retroactive adjustment to 10_000
    // with effective_date=200 (way before the first adjustment).
    set_time(&env, 3000);
    let id2 = client.create_retroactive_adjustment(
        &owner,
        &employer,
        &employee,
        &approver,
        &8_000,
        &10_000,
        &200,
        &reason_hash(&env, 99),
    );
    client.approve_adjustment(&approver, &id2);
    client.apply_adjustment(&employer, &id2);

    // The retroactive adjustment's new salary is visible going forward.
    assert_eq!(client.get_employee_salary(&employee), Some(10_000));

    // The audit trail records the retroactive nature of the adjustment.
    let audit_count = client.get_audit_log_count();
    let audit = client.get_audit_log(&audit_count).unwrap();
    assert_eq!(audit.action, Symbol::new(&env, "adjustment_applied"));
}

// ============================================================================
// PROPOSE_ADJUSTMENT — PERCENTAGE VS FIXED-AMOUNT TYPE VALIDATION
//
// `propose_adjustment` accepts either a percentage (basis points) or a fixed
// stroop delta, computes the absolute new salary, and then follows the same
// approve → apply path as `create_adjustment`. These tests lock in correct
// math for both modes and reject out-of-range inputs.
// ============================================================================

#[test]
fn test_propose_percentage_increase_applies_correct_salary() {
    // 10% increase: 10_000 + floor(10_000 * 1_000 / 10_000) = 11_000
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let current_salary: i128 = 10_000;
    let percentage_bps: i128 = 1_000; // 10%
    let expected_new_salary = current_salary
        + (current_salary * percentage_bps) / salary_adjustment::BPS_DENOMINATOR;
    assert_eq!(expected_new_salary, 11_000);

    let id = client.propose_adjustment(
        &employer,
        &employee,
        &approver,
        &current_salary,
        &AdjustmentType::Percentage,
        &percentage_bps,
        &AdjustmentKind::Increase,
        &200,
    );

    let stored = client.get_adjustment(&id).unwrap();
    assert_eq!(stored.kind, AdjustmentKind::Increase);
    assert_eq!(stored.status, AdjustmentStatus::Pending);
    assert_eq!(stored.current_salary, current_salary);
    assert_eq!(stored.new_salary, expected_new_salary);

    client.approve_adjustment(&approver, &id);
    set_time(&env, 250);
    client.apply_adjustment(&employer, &id);

    let applied = client.get_adjustment(&id).unwrap();
    assert_eq!(applied.status, AdjustmentStatus::Applied);
    assert_eq!(applied.new_salary, expected_new_salary);
    assert_eq!(
        client.get_employee_salary(&employee),
        Some(expected_new_salary),
        "apply_adjustment must persist the mathematically correct percentage result"
    );
}

#[test]
fn test_propose_fixed_amount_increase_applies_correct_salary() {
    // Fixed +2_500 on 10_000 → 12_500
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let current_salary: i128 = 10_000;
    let fixed_delta: i128 = 2_500;
    let expected_new_salary = current_salary + fixed_delta;

    let id = client.propose_adjustment(
        &employer,
        &employee,
        &approver,
        &current_salary,
        &AdjustmentType::FixedAmount,
        &fixed_delta,
        &AdjustmentKind::Increase,
        &200,
    );

    let stored = client.get_adjustment(&id).unwrap();
    assert_eq!(stored.kind, AdjustmentKind::Increase);
    assert_eq!(stored.current_salary, current_salary);
    assert_eq!(stored.new_salary, expected_new_salary);

    client.approve_adjustment(&approver, &id);
    set_time(&env, 250);
    client.apply_adjustment(&employer, &id);

    assert_eq!(
        client.get_employee_salary(&employee),
        Some(expected_new_salary),
        "apply_adjustment must persist the absolute fixed-amount result"
    );
    let applied = client.get_adjustment(&id).unwrap();
    assert_eq!(applied.status, AdjustmentStatus::Applied);
    assert_eq!(applied.new_salary, 12_500);
}

#[test]
fn test_propose_fixed_amount_decrease_applies_correct_salary() {
    // Fixed -1_500 on 10_000 → 8_500
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id = client.propose_adjustment(
        &employer,
        &employee,
        &approver,
        &10_000,
        &AdjustmentType::FixedAmount,
        &1_500,
        &AdjustmentKind::Decrease,
        &200,
    );

    let stored = client.get_adjustment(&id).unwrap();
    assert_eq!(stored.kind, AdjustmentKind::Decrease);
    assert_eq!(stored.new_salary, 8_500);

    client.approve_adjustment(&approver, &id);
    set_time(&env, 250);
    client.apply_adjustment(&employer, &id);

    assert_eq!(client.get_employee_salary(&employee), Some(8_500));
}

#[test]
fn test_propose_percentage_decrease_applies_correct_salary() {
    // 25% decrease: 8_000 - floor(8_000 * 2_500 / 10_000) = 6_000
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let id = client.propose_adjustment(
        &employer,
        &employee,
        &approver,
        &8_000,
        &AdjustmentType::Percentage,
        &2_500,
        &AdjustmentKind::Decrease,
        &200,
    );

    let stored = client.get_adjustment(&id).unwrap();
    assert_eq!(stored.kind, AdjustmentKind::Decrease);
    assert_eq!(stored.new_salary, 6_000);

    client.approve_adjustment(&approver, &id);
    set_time(&env, 250);
    client.apply_adjustment(&employer, &id);

    assert_eq!(client.get_employee_salary(&employee), Some(6_000));
}

#[test]
#[should_panic(expected = "Percentage must be positive")]
fn test_propose_negative_percentage_rejected() {
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    client.propose_adjustment(
        &employer,
        &employee,
        &approver,
        &10_000,
        &AdjustmentType::Percentage,
        &-500, // negative percentage must be rejected
        &AdjustmentKind::Increase,
        &200,
    );
}

#[test]
#[should_panic(expected = "Percentage must be positive")]
fn test_propose_zero_percentage_rejected() {
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    client.propose_adjustment(
        &employer,
        &employee,
        &approver,
        &10_000,
        &AdjustmentType::Percentage,
        &0,
        &AdjustmentKind::Increase,
        &200,
    );
}

#[test]
#[should_panic(expected = "Fixed amount must be positive")]
fn test_propose_negative_fixed_amount_rejected() {
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    client.propose_adjustment(
        &employer,
        &employee,
        &approver,
        &10_000,
        &AdjustmentType::FixedAmount,
        &-1_000, // fixed amount below zero must be rejected
        &AdjustmentKind::Increase,
        &200,
    );
}

#[test]
#[should_panic(expected = "Fixed amount must be positive")]
fn test_propose_zero_fixed_amount_rejected() {
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    client.propose_adjustment(
        &employer,
        &employee,
        &approver,
        &10_000,
        &AdjustmentType::FixedAmount,
        &0,
        &AdjustmentKind::Decrease,
        &200,
    );
}

#[test]
#[should_panic(expected = "Adjustment would result in non-positive salary")]
fn test_propose_fixed_amount_driving_salary_below_zero_rejected() {
    // current 5_000, decrease by 5_000 → 0 (non-positive) must be rejected
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    client.propose_adjustment(
        &employer,
        &employee,
        &approver,
        &5_000,
        &AdjustmentType::FixedAmount,
        &5_000,
        &AdjustmentKind::Decrease,
        &200,
    );
}

#[test]
#[should_panic(expected = "Adjustment would result in non-positive salary")]
fn test_propose_fixed_amount_exceeding_current_salary_rejected() {
    // current 5_000, decrease by 6_000 → underflow / non-positive
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    client.propose_adjustment(
        &employer,
        &employee,
        &approver,
        &5_000,
        &AdjustmentType::FixedAmount,
        &6_000,
        &AdjustmentKind::Decrease,
        &200,
    );
}

#[test]
#[should_panic(expected = "Adjustment would result in non-positive salary")]
fn test_propose_percentage_decrease_to_zero_rejected() {
    // 100% decrease → salary 0 must be rejected
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    client.propose_adjustment(
        &employer,
        &employee,
        &approver,
        &10_000,
        &AdjustmentType::Percentage,
        &10_000, // 100%
        &AdjustmentKind::Decrease,
        &200,
    );
}

#[test]
fn test_propose_negative_percentage_try_path_leaves_no_state() {
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let result = client.try_propose_adjustment(
        &employer,
        &employee,
        &approver,
        &10_000,
        &AdjustmentType::Percentage,
        &-100,
        &AdjustmentKind::Increase,
        &200,
    );
    assert!(result.is_err());
    assert!(client.get_adjustment(&1).is_none());
    assert_eq!(client.get_audit_log_count(), 0);
}

#[test]
fn test_propose_fixed_amount_below_zero_salary_try_path_leaves_no_state() {
    let env = create_env();
    set_time(&env, 100);
    let client = create_contract(&env);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);

    client.initialize(&owner);

    let result = client.try_propose_adjustment(
        &employer,
        &employee,
        &approver,
        &5_000,
        &AdjustmentType::FixedAmount,
        &5_500,
        &AdjustmentKind::Decrease,
        &200,
    );
    assert!(result.is_err());
    assert!(client.get_adjustment(&1).is_none());
    assert_eq!(client.get_employee_salary(&employee), None);
    assert_eq!(client.get_audit_log_count(), 0);
}
