#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, Env, Map, Symbol, TryIntoVal, Val,
};

use employee_roles::{
    BuiltInRole, EmployeeRolesContract, EmployeeRolesContractClient, PayrollAction,
    RoleChangedEvent,
};

fn setup() -> (Env, Address, EmployeeRolesContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, EmployeeRolesContract);
    let client = EmployeeRolesContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    (env, owner, client)
}

/// Setup with owner, and employees holding Employee, Manager, and Admin roles.
fn setup_with_roles() -> (
    Env,
    Address,
    Address,
    Address,
    Address,
    EmployeeRolesContractClient<'static>,
) {
    let (env, owner, client) = setup();
    let emp = Address::generate(&env);
    let mgr = Address::generate(&env);
    let adm = Address::generate(&env);

    client.assign_role(&owner, &emp, &BuiltInRole::Employee);
    client.assign_role(&owner, &mgr, &BuiltInRole::Manager);
    client.assign_role(&owner, &adm, &BuiltInRole::Admin);

    (env, owner, emp, mgr, adm, client)
}

// --- Regression: existing role APIs ---

#[test]
fn test_owner_can_assign_and_revoke_roles() {
    let (_env, owner, client) = setup();

    let employee = Address::generate(&_env);

    client.assign_role(&owner, &employee, &BuiltInRole::Manager);
    assert!(client.has_role(&employee, &BuiltInRole::Manager));

    client.revoke_role(&owner, &employee, &BuiltInRole::Manager);
    assert!(!client.has_role(&employee, &BuiltInRole::Manager));
}

#[test]
fn test_admin_can_manage_roles() {
    let (_env, owner, client) = setup();

    let admin = Address::generate(&_env);
    let employee = Address::generate(&_env);

    client.assign_role(&owner, &admin, &BuiltInRole::Admin);
    client.assign_role(&admin, &employee, &BuiltInRole::Manager);

    assert!(client.has_role(&employee, &BuiltInRole::Manager));
    assert!(client.has_role_at_least(&employee, &BuiltInRole::Employee));
}

#[test]
fn test_hierarchy_admin_satisfies_manager_and_employee() {
    let (_env, owner, client) = setup();

    let admin = Address::generate(&_env);
    client.assign_role(&owner, &admin, &BuiltInRole::Admin);

    assert!(client.has_role_at_least(&admin, &BuiltInRole::Employee));
    assert!(client.has_role_at_least(&admin, &BuiltInRole::Manager));
    assert!(client.has_role_at_least(&admin, &BuiltInRole::Admin));
}

#[test]
fn test_hierarchy_manager_satisfies_employee_only() {
    let (_env, owner, client) = setup();

    let manager = Address::generate(&_env);
    client.assign_role(&owner, &manager, &BuiltInRole::Manager);

    assert!(client.has_role_at_least(&manager, &BuiltInRole::Employee));
    assert!(client.has_role_at_least(&manager, &BuiltInRole::Manager));
    assert!(!client.has_role(&manager, &BuiltInRole::Admin));
}

#[test]
fn test_hierarchy_employee_only_employee() {
    let (_env, owner, client) = setup();

    let employee = Address::generate(&_env);
    client.assign_role(&owner, &employee, &BuiltInRole::Employee);

    assert!(client.has_role_at_least(&employee, &BuiltInRole::Employee));
    assert!(!client.has_role(&employee, &BuiltInRole::Manager));
    assert!(!client.has_role(&employee, &BuiltInRole::Admin));
}

// --- Role mutation: deny paths ---

#[test]
fn test_non_admin_cannot_assign_roles() {
    let (_env, owner, client) = setup();

    let manager = Address::generate(&_env);
    let employee = Address::generate(&_env);
    client.assign_role(&owner, &manager, &BuiltInRole::Manager);

    let result = client.try_assign_role(&manager, &employee, &BuiltInRole::Employee);
    assert!(result.is_err(), "Manager must not be able to assign roles");
}

#[test]
fn test_employee_cannot_assign_roles() {
    let (_env, owner, client) = setup();

    let emp = Address::generate(&_env);
    let other = Address::generate(&_env);
    client.assign_role(&owner, &emp, &BuiltInRole::Employee);

    let result = client.try_assign_role(&emp, &other, &BuiltInRole::Employee);
    assert!(result.is_err(), "Employee must not be able to assign roles");
}

#[test]
fn test_non_admin_cannot_revoke_roles() {
    let (_env, owner, client) = setup();

    let manager = Address::generate(&_env);
    let employee = Address::generate(&_env);
    client.assign_role(&owner, &manager, &BuiltInRole::Manager);
    client.assign_role(&owner, &employee, &BuiltInRole::Employee);

    let result = client.try_revoke_role(&manager, &employee, &BuiltInRole::Employee);
    assert!(result.is_err(), "Manager must not be able to revoke roles");
}

#[test]
fn test_employee_cannot_self_grant_admin() {
    let (_env, owner, client) = setup();

    let emp = Address::generate(&_env);
    client.assign_role(&owner, &emp, &BuiltInRole::Employee);

    let result = client.try_assign_role(&emp, &emp, &BuiltInRole::Admin);
    assert!(
        result.is_err(),
        "Employee must not be able to self-grant Admin"
    );
}

#[test]
fn test_manager_cannot_assign_admin() {
    let (_env, owner, client) = setup();

    let manager = Address::generate(&_env);
    let other = Address::generate(&_env);
    client.assign_role(&owner, &manager, &BuiltInRole::Manager);

    let result = client.try_assign_role(&manager, &other, &BuiltInRole::Admin);
    assert!(result.is_err(), "Manager must not be able to assign Admin");
}

// --- Capability matrix: ALLOW (positive) ---

const EMPLOYEE_ACTIONS: &[PayrollAction] = &[
    PayrollAction::ViewPayrollStatus,
    PayrollAction::ViewPayrollHistory,
    PayrollAction::ClaimOwnPayroll,
    PayrollAction::WithdrawOwnPayroll,
];

const MANAGER_ACTIONS: &[PayrollAction] = &[
    PayrollAction::CreatePayrollRecord,
    PayrollAction::UpdatePayrollRecord,
    PayrollAction::PauseEmployeePayroll,
    PayrollAction::ResumeEmployeePayroll,
];

const ADMIN_ACTIONS: &[PayrollAction] = &[
    PayrollAction::AssignRoles,
    PayrollAction::RevokeRoles,
    PayrollAction::EmergencyPause,
    PayrollAction::EmergencyUnpause,
];

#[test]
fn test_matrix_owner_can_perform_all_actions() {
    let (_env, owner, client) = setup();

    for action in [EMPLOYEE_ACTIONS, MANAGER_ACTIONS, ADMIN_ACTIONS]
        .into_iter()
        .flatten()
    {
        assert!(
            client.can_perform(&owner, action),
            "Owner must be able to perform {:?}",
            action
        );
    }
}

#[test]
fn test_matrix_employee_can_perform_employee_actions() {
    let (_env, _owner, emp, _mgr, _adm, client) = setup_with_roles();

    for action in EMPLOYEE_ACTIONS {
        assert!(
            client.can_perform(&emp, action),
            "Employee must be able to perform {:?}",
            action
        );
    }
}

#[test]
fn test_matrix_manager_can_perform_employee_and_manager_actions() {
    let (_env, _owner, _emp, mgr, _adm, client) = setup_with_roles();

    for action in EMPLOYEE_ACTIONS.iter().chain(MANAGER_ACTIONS) {
        assert!(
            client.can_perform(&mgr, action),
            "Manager must be able to perform {:?}",
            action
        );
    }
}

#[test]
fn test_matrix_admin_can_perform_all_actions() {
    let (_env, _owner, _emp, _mgr, adm, client) = setup_with_roles();

    for action in EMPLOYEE_ACTIONS
        .iter()
        .chain(MANAGER_ACTIONS)
        .chain(ADMIN_ACTIONS)
    {
        assert!(
            client.can_perform(&adm, action),
            "Admin must be able to perform {:?}",
            action
        );
    }
}

// --- Capability matrix: DENY (negative) ---

#[test]
fn test_matrix_employee_denied_manager_actions() {
    let (_env, _owner, emp, _mgr, _adm, client) = setup_with_roles();

    for action in MANAGER_ACTIONS {
        assert!(
            !client.can_perform(&emp, action),
            "Employee must NOT be able to perform {:?}",
            action
        );
    }
}

#[test]
fn test_matrix_employee_denied_admin_actions() {
    let (_env, _owner, emp, _mgr, _adm, client) = setup_with_roles();

    for action in ADMIN_ACTIONS {
        assert!(
            !client.can_perform(&emp, action),
            "Employee must NOT be able to perform {:?}",
            action
        );
    }
}

#[test]
fn test_matrix_manager_denied_admin_actions() {
    let (_env, _owner, _emp, mgr, _adm, client) = setup_with_roles();

    for action in ADMIN_ACTIONS {
        assert!(
            !client.can_perform(&mgr, action),
            "Manager must NOT be able to perform {:?}",
            action
        );
    }
}

#[test]
fn test_matrix_no_role_denied_all_actions() {
    let (env, _owner, _emp, _mgr, _adm, client) = setup_with_roles();
    let no_role = Address::generate(&env);

    for action in EMPLOYEE_ACTIONS
        .iter()
        .chain(MANAGER_ACTIONS)
        .chain(ADMIN_ACTIONS)
    {
        assert!(
            !client.can_perform(&no_role, action),
            "No-role address must NOT be able to perform {:?}",
            action
        );
    }
}

// --- require_capability: allow/deny ---

#[test]
fn test_require_capability_allows_employee_action() {
    let (_env, _owner, emp, _mgr, _adm, client) = setup_with_roles();

    let result = client.try_require_capability(&emp, &PayrollAction::ViewPayrollStatus);
    assert!(result.is_ok());
}

#[test]
fn test_require_capability_denies_employee_manager_action() {
    let (_env, _owner, emp, _mgr, _adm, client) = setup_with_roles();

    let result = client.try_require_capability(&emp, &PayrollAction::CreatePayrollRecord);
    assert!(
        result.is_err(),
        "Employee must not have CreatePayrollRecord capability"
    );
}

#[test]
fn test_require_capability_denies_employee_admin_action() {
    let (_env, _owner, emp, _mgr, _adm, client) = setup_with_roles();

    let result = client.try_require_capability(&emp, &PayrollAction::AssignRoles);
    assert!(
        result.is_err(),
        "Employee must not have AssignRoles capability"
    );
}

#[test]
fn test_require_capability_allows_manager_action() {
    let (_env, _owner, _emp, mgr, _adm, client) = setup_with_roles();

    let result = client.try_require_capability(&mgr, &PayrollAction::CreatePayrollRecord);
    assert!(result.is_ok());
}

#[test]
fn test_require_capability_denies_manager_admin_action() {
    let (_env, _owner, _emp, mgr, _adm, client) = setup_with_roles();

    let result = client.try_require_capability(&mgr, &PayrollAction::EmergencyPause);
    assert!(
        result.is_err(),
        "Manager must not have EmergencyPause capability"
    );
}

#[test]
fn test_require_capability_allows_admin_action() {
    let (_env, _owner, _emp, _mgr, adm, client) = setup_with_roles();

    let result = client.try_require_capability(&adm, &PayrollAction::AssignRoles);
    assert!(result.is_ok());
}

// --- Event emission tests ---

/// Helper: count RoleChanged events in the current event buffer.
fn count_role_events(env: &Env) -> usize {
    let all = env.events().all();
    let role_sym: Val = Symbol::new(env, "ROLE").into();
    all.iter()
        .filter(|e| e.1.len() >= 2 && e.1.get(0) == role_sym)
        .count()
}

/// Helper: extract a field from a RoleChangedEvent data map.
fn event_field<T: TryIntoVal<Env, Val>>(env: &Env, data: &Val, name: &str) -> T {
    let map: Map<Symbol, Val> = data.clone().try_into_val(env).unwrap();
    map.get(Symbol::new(env, name))
        .unwrap()
        .try_into_val(env)
        .unwrap()
}

/// Helper: return the n-th RoleChanged event (0‑based from oldest) and its data Val.
fn nth_role_event(env: &Env, n: usize) -> Option<(Address, Vec<Val>, Val)> {
    let all = env.events().all();
    let role_sym: Val = Symbol::new(env, "ROLE").into();
    all.iter()
        .filter(|e| e.1.len() >= 2 && e.1.get(0) == role_sym)
        .nth(n)
        .map(|e| (e.0.clone(), e.1.clone(), e.2.clone()))
}

#[test]
fn test_role_changed_event_on_assign() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    env.ledger().with_mut(|l| l.timestamp = 42_000);

    client.assign_role(&owner, &employee, &BuiltInRole::Manager);

    assert_eq!(count_role_events(&env), 1, "Expected exactly one RoleChanged event");

    let (_contract, _topics, data) = nth_role_event(&env, 0).unwrap();

    let old_role: Option<BuiltInRole> = event_field(&env, &data, "old_role");
    let new_role: BuiltInRole = event_field(&env, &data, "new_role");
    let changed_by: Address = event_field(&env, &data, "changed_by");
    let emp: Address = event_field(&env, &data, "employee");
    let ts: u64 = event_field(&env, &data, "timestamp");

    assert_eq!(old_role, None);
    assert_eq!(new_role, BuiltInRole::Manager);
    assert_eq!(changed_by, owner);
    assert_eq!(emp, employee);
    assert_eq!(ts, 42_000);
}

#[test]
fn test_role_changed_event_on_revoke() {
    let (env, owner, client) = setup();
    let employee = Address::generate(&env);

    // Give the employee a role first
    client.assign_role(&owner, &employee, &BuiltInRole::Employee);

    env.ledger().with_mut(|l| l.timestamp = 99_999);

    client.revoke_role(&owner, &employee, &BuiltInRole::Employee);

    // We now have two RoleChanged events: assign (idx 0) and revoke (idx 1).
    assert_eq!(count_role_events(&env), 2);

    let (_contract, _topics, data) = nth_role_event(&env, 1).unwrap();

    let old_role: BuiltInRole = event_field(&env, &data, "old_role");
    let new_role: Option<BuiltInRole> = event_field(&env, &data, "new_role");
    let changed_by: Address = event_field(&env, &data, "changed_by");
    let emp_field: Address = event_field(&env, &data, "employee");
    let ts: u64 = event_field(&env, &data, "timestamp");

    assert_eq!(old_role, BuiltInRole::Employee);
    assert_eq!(new_role, None);
    assert_eq!(changed_by, owner);
    assert_eq!(emp_field, employee);
    assert_eq!(ts, 99_999);
}

#[test]
fn test_no_event_on_duplicate_assign() {
    let (env, owner, client) = setup();
    let employee = Address::generate(&env);

    client.assign_role(&owner, &employee, &BuiltInRole::Employee);
    assert_eq!(count_role_events(&env), 1, "First assign must emit one event");

    // Second assign of the same role is a no-op → no new event.
    client.assign_role(&owner, &employee, &BuiltInRole::Employee);
    assert_eq!(count_role_events(&env), 1, "Duplicate assign must not emit RoleChanged event");
}

#[test]
fn test_no_event_on_revoke_nonexistent() {
    let (env, owner, client) = setup();
    let stranger = Address::generate(&env);

    let _ = client.try_revoke_role(&owner, &stranger, &BuiltInRole::Manager);

    assert_eq!(count_role_events(&env), 0, "Revoke of non‑existent role must not emit event");
}

#[test]
fn test_full_payload_roundtrip() {
    let (env, owner, client) = setup();
    let employee = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 7_000);

    client.assign_role(&owner, &employee, &BuiltInRole::Admin);

    assert_eq!(count_role_events(&env), 1);

    let (_contract, _topics, data) = nth_role_event(&env, 0).unwrap();

    // Deserialize the full event and verify every field matches.
    let event: RoleChangedEvent = data.clone().try_into_val(&env).unwrap();
    assert_eq!(event.old_role, None);
    assert_eq!(event.new_role, Some(BuiltInRole::Admin));
    assert_eq!(event.changed_by, owner);
    assert_eq!(event.employee, employee);
    assert_eq!(event.timestamp, 7_000);
}

// --- Initialization safeguard ---

#[test]
#[should_panic(expected = "Already initialized")]
fn test_initialize_twice_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, EmployeeRolesContract);
    let client = EmployeeRolesContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);

    client.initialize(&owner);
    client.initialize(&owner);
}
