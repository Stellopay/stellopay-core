use salary_adjustment::{
    AdjustmentKind, AdjustmentStatus, SalaryAdjustmentContract, SalaryAdjustmentContractClient,
};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env};

fn create_contract<'a>(env: &Env) -> SalaryAdjustmentContractClient<'a> {
    let contract_id = env.register_contract(None, SalaryAdjustmentContract);
    SalaryAdjustmentContractClient::new(env, &contract_id)
}

fn set_time(env: &Env, timestamp: u64) {
    env.ledger().with_mut(|ledger| {
        ledger.timestamp = timestamp;
    });
}

#[test]
fn test_create_salary_increase() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let client = create_contract(&env);

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
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &10_000, &8_000, &500);

    let stored = client.get_adjustment(&id).unwrap();
    assert_eq!(stored.kind, AdjustmentKind::Decrease);
    assert_eq!(stored.status, AdjustmentStatus::Pending);
    assert_eq!(stored.current_salary, 10_000);
    assert_eq!(stored.new_salary, 8_000);
}

#[test]
fn test_approve_and_apply_adjustment() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_500, &1_000);

    client.approve_adjustment(&approver, &id);
    let approved = client.get_adjustment(&id).unwrap();
    assert_eq!(approved.status, AdjustmentStatus::Approved);

    set_time(&env, 1_200);
    client.apply_adjustment(&employer, &id);
    let applied = client.get_adjustment(&id).unwrap();
    assert_eq!(applied.status, AdjustmentStatus::Applied);
}

#[test]
#[should_panic(expected = "Effective date not reached")]
fn test_apply_before_effective_date() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &7_000, &2_000);

    client.approve_adjustment(&approver, &id);
    set_time(&env, 500);
    client.apply_adjustment(&employer, &id);
}

#[test]
#[should_panic(expected = "Only approver can approve")]
fn test_only_approver_can_approve() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let attacker = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &100);

    client.approve_adjustment(&attacker, &id);
}

#[test]
#[should_panic(expected = "Only approver can reject")]
fn test_only_approver_can_reject() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let attacker = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &100);

    client.reject_adjustment(&attacker, &id);
}

#[test]
#[should_panic(expected = "Adjustment is not approved")]
fn test_apply_unapproved_adjustment() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &7_000, &100);

    set_time(&env, 200);
    client.apply_adjustment(&employer, &id);
}

#[test]
fn test_cancel_pending_adjustment() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &100);

    client.cancel_adjustment(&employer, &id);
    let stored = client.get_adjustment(&id).unwrap();
    assert_eq!(stored.status, AdjustmentStatus::Cancelled);
}

#[test]
#[should_panic(expected = "Adjustment cannot be cancelled")]
fn test_cannot_cancel_approved_adjustment() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &100);

    client.approve_adjustment(&approver, &id);
    client.cancel_adjustment(&employer, &id);
}

#[test]
fn test_reject_then_cancel() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let client = create_contract(&env);

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
#[should_panic(expected = "New salary must differ from current salary")]
fn test_same_salary_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);

    client.create_adjustment(&employer, &employee, &approver, &5_000, &5_000, &100);
}

#[test]
#[should_panic(expected = "Current salary must be positive")]
fn test_zero_current_salary_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);

    client.create_adjustment(&employer, &employee, &approver, &0, &5_000, &100);
}

#[test]
#[should_panic(expected = "New salary must be positive")]
fn test_zero_new_salary_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);

    client.create_adjustment(&employer, &employee, &approver, &5_000, &0, &100);
}

#[test]
#[should_panic(expected = "Contract already initialized")]
fn test_double_initialization() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);
    client.initialize(&owner);
}

#[test]
fn test_get_owner() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);

    let stored_owner = client.get_owner().unwrap();
    assert_eq!(stored_owner, owner);
}

#[test]
fn test_get_nonexistent_adjustment() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);

    assert!(client.get_adjustment(&999).is_none());
}

#[test]
fn test_apply_at_exact_effective_date() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &8_000, &1_000);

    client.approve_adjustment(&approver, &id);
    set_time(&env, 1_000);
    client.apply_adjustment(&employer, &id);

    let stored = client.get_adjustment(&id).unwrap();
    assert_eq!(stored.status, AdjustmentStatus::Applied);
}

#[test]
#[should_panic(expected = "Only employer can cancel")]
fn test_non_employer_cannot_cancel() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let attacker = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &6_000, &100);

    client.cancel_adjustment(&attacker, &id);
}

#[test]
#[should_panic(expected = "Only employer can apply")]
fn test_non_employer_cannot_apply() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let attacker = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);

    let id = client.create_adjustment(&employer, &employee, &approver, &5_000, &7_000, &100);

    client.approve_adjustment(&approver, &id);
    set_time(&env, 200);
    client.apply_adjustment(&attacker, &id);
}
