#![cfg(test)]
#![allow(deprecated)]

use salary_adjustment::{
    AdjustmentKind, AdjustmentStatus, SalaryAdjustmentContract, SalaryAdjustmentContractClient,
    DEFAULT_MAX_SALARY,
};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, BytesN, Env, Symbol};

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
