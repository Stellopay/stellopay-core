#![cfg(test)]

use employee_roles::{
    BuiltInRole, EmployeeRolesContract, EmployeeRolesContractClient, PayrollAction, RoleError,
    RoleGrant,
};
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env};

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
