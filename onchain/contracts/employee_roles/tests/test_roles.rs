#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, Env, Map, Symbol, TryFromVal, TryIntoVal, Val, Vec,
};

use employee_roles::{
    BuiltInRole, EmployeeRolesContract, EmployeeRolesContractClient, PayrollAction,
    RoleChangedEvent, RoleError, RoleGrant,
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
    all.iter()
        .filter(|e| {
            if e.1.len() < 2 {
                return false;
            }
            Symbol::try_from_val(env, &e.1.get(0).unwrap())
                .map_or(false, |s| s == Symbol::new(env, "ROLE"))
        })
        .count()
}

/// Helper: extract a field from a RoleChangedEvent data map.
fn event_field<T: TryFromVal<Env, Val>>(env: &Env, data: &Val, name: &str) -> T {
    let map: Map<Symbol, Val> = data.clone().try_into_val(env).unwrap();
    map.get(Symbol::new(env, name))
        .unwrap()
        .try_into_val(env)
        .unwrap()
}

/// Helper: return the n-th RoleChanged event (0‑based from oldest) and its data Val.
fn nth_role_event(env: &Env, n: usize) -> Option<(Address, Vec<Val>, Val)> {
    let all = env.events().all();
    let role_sym: Symbol = Symbol::new(env, "ROLE");
    all.iter()
        .filter(|e| {
            if e.1.len() < 2 {
                return false;
            }
            Symbol::try_from_val(env, &e.1.get(0).unwrap()).map_or(false, |s| s == role_sym)
        })
        .nth(n)
        .map(|e| (e.0.clone(), e.1.clone(), e.2.clone()))
}

#[test]
fn test_role_changed_event_on_assign() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    env.ledger().with_mut(|l| l.timestamp = 42_000);

    client.assign_role(&owner, &employee, &BuiltInRole::Manager);

    assert_eq!(
        count_role_events(&env),
        1,
        "Expected exactly one RoleChanged event"
    );

    let (_contract, _topics, data) = nth_role_event(&env, 0).unwrap();

    let old_role: Option<u32> = event_field(&env, &data, "old_role");
    let new_role: u32 = event_field(&env, &data, "new_role");
    let changed_by: Address = event_field(&env, &data, "changed_by");
    let emp: Address = event_field(&env, &data, "employee");
    let ts: u64 = event_field(&env, &data, "timestamp");

    assert_eq!(old_role, None);
    assert_eq!(new_role, BuiltInRole::Manager as u32);
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

    // Events are scoped per invocation. The revoke call emitted exactly one event.
    assert_eq!(count_role_events(&env), 1);

    let (_contract, _topics, data) = nth_role_event(&env, 0).unwrap();

    let old_role: u32 = event_field(&env, &data, "old_role");
    let new_role: Option<u32> = event_field(&env, &data, "new_role");
    let changed_by: Address = event_field(&env, &data, "changed_by");
    let emp_field: Address = event_field(&env, &data, "employee");
    let ts: u64 = event_field(&env, &data, "timestamp");

    assert_eq!(old_role, BuiltInRole::Employee as u32);
    assert_eq!(new_role, None);
    assert_eq!(changed_by, owner);
    assert_eq!(emp_field, employee);
    assert_eq!(ts, 99_999);
}

#[test]
fn test_no_event_on_duplicate_assign() {
    let (env, owner, client) = setup();
    let employee = Address::generate(&env);

    // First assign emits exactly one RoleChanged event
    client.assign_role(&owner, &employee, &BuiltInRole::Employee);
    assert_eq!(
        count_role_events(&env),
        1,
        "First assign must emit one event"
    );

    // Second assign of the same role is a no-op → no new event.
    // Events are scoped per invocation, so the buffer is fresh.
    client.assign_role(&owner, &employee, &BuiltInRole::Employee);
    assert_eq!(
        count_role_events(&env),
        0,
        "Duplicate assign must not emit RoleChanged event"
    );
}

#[test]
fn test_no_event_on_revoke_nonexistent() {
    let (env, owner, client) = setup();
    let stranger = Address::generate(&env);

    let _ = client.try_revoke_role(&owner, &stranger, &BuiltInRole::Manager);

    assert_eq!(
        count_role_events(&env),
        0,
        "Revoke of non‑existent role must not emit event"
    );
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
    assert_eq!(event.new_role, Some(BuiltInRole::Admin as u32));
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

// --- Time-Bound Role Grant Tests ---

#[test]
fn test_time_bound_role_grant_lifecycle() {
    let (env, owner, client) = setup();
    let employee = Address::generate(&env);

    // Initial ledger timestamp: 1000
    env.ledger().set_timestamp(1000);
    let expires_at = 1500;

    // Grant Manager role with expiration
    client.assign_role_with_expiration(&owner, &employee, &BuiltInRole::Manager, &Some(expires_at));

    // Before expiry: timestamp 1200
    env.ledger().set_timestamp(1200);
    assert!(client.has_role(&employee, &BuiltInRole::Manager));
    assert!(client.has_role_at_least(&employee, &BuiltInRole::Employee));
    assert!(client.can_perform(&employee, &PayrollAction::CreatePayrollRecord));
    assert_eq!(
        client.get_roles(&employee),
        soroban_sdk::vec![&env, BuiltInRole::Manager]
    );

    // Check detailed role grants query
    let grants = client.get_role_grants(&employee);
    assert_eq!(grants.len(), 1);
    assert_eq!(
        grants.get(0).unwrap(),
        RoleGrant {
            role: BuiltInRole::Manager,
            expires_at: Some(expires_at),
        }
    );

    // After expiry: timestamp 1500
    env.ledger().set_timestamp(1500);
    assert!(!client.has_role(&employee, &BuiltInRole::Manager));
    assert!(!client.has_role_at_least(&employee, &BuiltInRole::Employee));
    assert!(!client.can_perform(&employee, &PayrollAction::CreatePayrollRecord));
    assert_eq!(client.get_roles(&employee), soroban_sdk::vec![&env]);

    // No explicit revoke transaction required, but explicitly revoking works cleanly
    client.revoke_role(&owner, &employee, &BuiltInRole::Manager);
    assert_eq!(client.get_role_grants(&employee).len(), 0);
}

#[test]
fn test_non_expiring_role_grant_longevity() {
    let (env, owner, client) = setup();
    let employee = Address::generate(&env);

    env.ledger().set_timestamp(1000);

    // Grant role without expiration
    client.assign_role(&owner, &employee, &BuiltInRole::Employee);

    // Advance timestamp far into the future
    env.ledger().set_timestamp(1_000_000_000);

    assert!(client.has_role(&employee, &BuiltInRole::Employee));
    assert!(client.can_perform(&employee, &PayrollAction::ClaimOwnPayroll));
    assert_eq!(
        client.get_role_grants(&employee),
        soroban_sdk::vec![
            &env,
            RoleGrant {
                role: BuiltInRole::Employee,
                expires_at: None,
            }
        ]
    );
}

#[test]
fn test_expiration_boundary_conditions() {
    let (env, owner, client) = setup();
    let employee = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    let expires_at = 2000;

    client.assign_role_with_expiration(&owner, &employee, &BuiltInRole::Admin, &Some(expires_at));

    // Immediately before expiry (1999) -> Authorized
    env.ledger().set_timestamp(1999);
    assert!(client.has_role(&employee, &BuiltInRole::Admin));
    assert!(client.can_perform(&employee, &PayrollAction::AssignRoles));

    // Exactly at expiry (2000) -> Unauthorized
    env.ledger().set_timestamp(2000);
    assert!(!client.has_role(&employee, &BuiltInRole::Admin));
    assert!(!client.can_perform(&employee, &PayrollAction::AssignRoles));

    // After expiry (2001) -> Unauthorized
    env.ledger().set_timestamp(2001);
    assert!(!client.has_role(&employee, &BuiltInRole::Admin));
    assert!(!client.can_perform(&employee, &PayrollAction::AssignRoles));
}

#[test]
fn test_invalid_expiration_rejected() {
    let (env, owner, client) = setup();
    let employee = Address::generate(&env);

    env.ledger().set_timestamp(1000);

    // Expiry equal to current timestamp -> InvalidExpiration
    let res_equal = client.try_assign_role_with_expiration(
        &owner,
        &employee,
        &BuiltInRole::Manager,
        &Some(1000),
    );
    assert_eq!(res_equal, Err(Ok(RoleError::InvalidExpiration)));

    // Expiry in the past -> InvalidExpiration
    let res_past = client.try_assign_role_with_expiration(
        &owner,
        &employee,
        &BuiltInRole::Manager,
        &Some(500),
    );
    assert_eq!(res_past, Err(Ok(RoleError::InvalidExpiration)));
}

#[test]
fn test_grant_update_and_extension() {
    let (env, owner, client) = setup();
    let employee = Address::generate(&env);

    env.ledger().set_timestamp(1000);

    // 1. Assign role expiring at 1500
    client.assign_role_with_expiration(&owner, &employee, &BuiltInRole::Manager, &Some(1500));

    // 2. Extend expiration to 2500
    client.assign_role_with_expiration(&owner, &employee, &BuiltInRole::Manager, &Some(2500));

    env.ledger().set_timestamp(1800);
    assert!(client.has_role(&employee, &BuiltInRole::Manager));

    // 3. Make non-expiring (None)
    client.assign_role_with_expiration(&owner, &employee, &BuiltInRole::Manager, &None);

    env.ledger().set_timestamp(5000);
    assert!(client.has_role(&employee, &BuiltInRole::Manager));
}

#[test]
fn test_legacy_storage_compatibility() {
    let (env, _owner, client) = setup();
    let employee = Address::generate(&env);

    // Simulate existing storage holding Vec<RoleGrant> with no expiration (non-expiring grant).
    // This represents grants created before time-bound support, which are stored as None expires_at.
    let stored_grants: soroban_sdk::Vec<RoleGrant> = soroban_sdk::vec![
        &env,
        RoleGrant {
            role: BuiltInRole::Manager,
            expires_at: None,
        }
    ];
    env.as_contract(&client.address, || {
        env.storage().persistent().set(
            &employee_roles::StorageKey::EmployeeRoles(employee.clone()),
            &stored_grants,
        );
    });

    // Non-expiring grants written directly to storage are correctly read as active
    assert!(client.has_role(&employee, &BuiltInRole::Manager));
    assert!(client.can_perform(&employee, &PayrollAction::CreatePayrollRecord));
    assert_eq!(
        client.get_roles(&employee),
        soroban_sdk::vec![&env, BuiltInRole::Manager]
    );
    assert_eq!(
        client.get_role_grants(&employee),
        soroban_sdk::vec![
            &env,
            RoleGrant {
                role: BuiltInRole::Manager,
                expires_at: None,
            }
        ]
    );

    // Advance time significantly — non-expiring grants are still active
    env.ledger().set_timestamp(999_999_999);
    assert!(client.has_role(&employee, &BuiltInRole::Manager));
}

// --- get_effective_permissions: union-without-duplicates ---

fn init_default_implies(env: &Env, client: &EmployeeRolesContractClient, owner: &Address) {
    // Admin -> Manager -> Employee (standard hierarchy)
    client.set_role_implies(
        owner,
        &BuiltInRole::Admin,
        &soroban_sdk::vec![&env, BuiltInRole::Manager],
    );
    client.set_role_implies(
        owner,
        &BuiltInRole::Manager,
        &soroban_sdk::vec![&env, BuiltInRole::Employee],
    );
    client.set_role_implies(owner, &BuiltInRole::Employee, &soroban_sdk::vec![&env]);
}

#[test]
fn test_effective_permissions_owner_all_actions() {
    let (env, owner, client) = setup();

    init_default_implies(&env, &client, &owner);

    // Owner should get all 12 actions
    let perms = client.get_effective_permissions(&owner);
    let expected = all_payroll_actions(&env);
    assert_eq!(perms.len(), 12);
    for a in expected.iter() {
        assert!(perms.iter().any(|p| p == a), "Owner must have {:?}", a);
    }
}

#[test]
fn test_effective_permissions_admin_all_actions() {
    let (env, owner, _emp, _mgr, adm, client) = setup_with_roles();
    init_default_implies(&env, &client, &owner);

    // Admin -> Manager -> Employee implies all 12 actions
    let perms = client.get_effective_permissions(&adm);
    assert_eq!(perms.len(), 12, "Admin should have all 12 actions");
    for a in all_payroll_actions(&env).iter() {
        assert!(perms.iter().any(|p| p == a), "Admin must have {:?}", a);
    }
}

#[test]
fn test_effective_permissions_manager_actions() {
    let (env, owner, _emp, mgr, _adm, client) = setup_with_roles();
    init_default_implies(&env, &client, &owner);

    // Manager -> Employee implies 8 actions (4 manager + 4 employee)
    let perms = client.get_effective_permissions(&mgr);
    assert_eq!(perms.len(), 8, "Manager should have 8 actions");
}

#[test]
fn test_effective_permissions_employee_actions() {
    let (env, owner, emp, _mgr, _adm, client) = setup_with_roles();
    init_default_implies(&env, &client, &owner);

    // Employee alone implies 4 actions
    let perms = client.get_effective_permissions(&emp);
    assert_eq!(perms.len(), 4, "Employee should have 4 actions");
}

#[test]
fn test_effective_permissions_no_duplicate_when_action_both_direct_and_inherited() {
    let (env, owner, _client) = setup();
    let client = &_client;

    // Setup: Employee is granted both Admin and Employee roles.
    // Admin implies Manager -> Employee, so Employee-level actions come from:
    // 1) direct Employee role, and 2) inherited via Admin -> Manager -> Employee.
    // The union must still have each action exactly once.
    let employee = Address::generate(&env);
    client.assign_role(&owner, &employee, &BuiltInRole::Admin);
    client.assign_role(&owner, &employee, &BuiltInRole::Employee);

    init_default_implies(&env, client, &owner);

    let perms = client.get_effective_permissions(&employee);
    assert_eq!(
        perms.len(),
        12,
        "Admin+Employee should have 12 actions (no dupes)"
    );

    // Verify dedup: same action appears only once
    let mut seen = soroban_sdk::Map::new(&env);
    for i in 0..perms.len() {
        let action = perms.get(i).unwrap();
        let key: u32 = action as u32;
        assert!(
            !seen.contains_key(key),
            "Duplicate action {:?} found",
            action
        );
        seen.set(key, true);
    }
}

#[test]
fn test_effective_permissions_no_duplicate_converging_inheritance_paths() {
    let (env, owner, _client) = setup();
    let client = &_client;

    // Setup: role graph with two inheritance paths converging on Employee:
    //   Admin -> [SpecialManager, Employee]
    //   SpecialManager -> [Employee]
    // Admin's effective roles: [Admin, SpecialManager, Employee] (3 unique)
    // Employee-level actions should NOT appear twice despite being reachable
    // via two paths: Admin -> Employee and Admin -> SpecialManager -> Employee.
    let employee = Address::generate(&env);

    // We use Admin as a "VirtualAdmin" that implies both SpecialManager and Employee
    client.assign_role(&owner, &employee, &BuiltInRole::Admin);

    // Set converging implies: Admin -> [Manager, Employee], Manager -> [Employee]
    client.set_role_implies(
        &owner,
        &BuiltInRole::Admin,
        &soroban_sdk::vec![&env, BuiltInRole::Manager, BuiltInRole::Employee],
    );
    client.set_role_implies(
        &owner,
        &BuiltInRole::Manager,
        &soroban_sdk::vec![&env, BuiltInRole::Employee],
    );
    client.set_role_implies(&owner, &BuiltInRole::Employee, &soroban_sdk::vec![&env]);

    env.ledger().set_timestamp(1000);

    let perms = client.get_effective_permissions(&employee);
    // Expected: Admin directly -> Admin actions (4)
    //           Admin -> Manager -> Manager actions (4)
    //           Admin -> Employee -> Employee actions (4)
    // All unique = 12
    assert_eq!(
        perms.len(),
        12,
        "Converging inheritance paths must not produce duplicates"
    );

    // Verify dedup with Map
    let mut seen = soroban_sdk::Map::new(&env);
    for i in 0..perms.len() {
        let action = perms.get(i).unwrap();
        let key: u32 = action as u32;
        assert!(
            !seen.contains_key(key),
            "Duplicate action {:?} found in converging paths test",
            action
        );
        seen.set(key, true);
    }
}

#[test]
fn test_effective_permissions_no_role_returns_empty() {
    let (env, _owner, client) = setup();
    let stranger = Address::generate(&env);

    let perms = client.get_effective_permissions(&stranger);
    assert_eq!(
        perms.len(),
        0,
        "No-role address should have empty permissions"
    );
}

#[test]
fn test_effective_permissions_union_length_matches_expected() {
    let (env, owner, _client) = setup();
    let client = &_client;

    // Set up only Partial implies: Manager -> [Employee] without Admin
    let employee = Address::generate(&env);
    client.assign_role(&owner, &employee, &BuiltInRole::Manager);

    client.set_role_implies(
        &owner,
        &BuiltInRole::Manager,
        &soroban_sdk::vec![&env, BuiltInRole::Employee],
    );
    client.set_role_implies(&owner, &BuiltInRole::Employee, &soroban_sdk::vec![&env]);

    let perms = client.get_effective_permissions(&employee);
    // Manager directly (4) + Employee implied (4) = 8 actions
    assert_eq!(perms.len(), 8);

    // Now add Admin role to same employee
    client.assign_role(&owner, &employee, &BuiltInRole::Admin);
    client.set_role_implies(
        &owner,
        &BuiltInRole::Admin,
        &soroban_sdk::vec![&env, BuiltInRole::Manager],
    );

    let perms = client.get_effective_permissions(&employee);
    // Admin (4) + Manager (4) + Employee (4) = 12, not duplicate Manager/Employee
    assert_eq!(perms.len(), 12);

    // Explicitly verify each expected action appears exactly once
    let all = all_payroll_actions(&env);
    for expected in all.iter() {
        let count = perms.iter().filter(|a| *a == expected).count();
        assert_eq!(
            count, 1,
            "Action {:?} should appear exactly once, got {}",
            expected, count
        );
    }
}

/// Helper: returns all 12 payroll actions.
fn all_payroll_actions(env: &Env) -> Vec<PayrollAction> {
    soroban_sdk::vec![
        &env,
        PayrollAction::ViewPayrollStatus,
        PayrollAction::ViewPayrollHistory,
        PayrollAction::ClaimOwnPayroll,
        PayrollAction::WithdrawOwnPayroll,
        PayrollAction::CreatePayrollRecord,
        PayrollAction::UpdatePayrollRecord,
        PayrollAction::PauseEmployeePayroll,
        PayrollAction::ResumeEmployeePayroll,
        PayrollAction::AssignRoles,
        PayrollAction::RevokeRoles,
        PayrollAction::EmergencyPause,
        PayrollAction::EmergencyUnpause,
    ]
}
