#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Bytes, Env};

use nft_payroll_badge::{
    Badge, BadgeError, BadgeKind, NftPayrollBadge, NftPayrollBadgeClient,
};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_initialized(env: &Env) -> (NftPayrollBadgeClient<'static>, Address, Address, Address) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, NftPayrollBadge);
    let client = NftPayrollBadgeClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let employer = Address::generate(env);
    let employee = Address::generate(env);

    client.initialize(&admin);

    (client, admin, employer, employee)
}

fn bytes_from_str(env: &Env, s: &str) -> Bytes {
    Bytes::from_slice(env, s.as_bytes())
}

#[test]
fn initialize_and_get_admin() {
    let env = create_env();
    let (client, admin, _, _) = setup_initialized(&env);

    let stored = client.get_admin().unwrap();
    assert_eq!(stored, admin);
}

#[test]
fn mint_employer_and_employee_badges() {
    let env = create_env();
    let (client, admin, employer, employee) = setup_initialized(&env);

    let employer_badge_id = client.mint(
            &admin,
            &employer,
            &BadgeKind::Employer,
            &bytes_from_str(&env, "employer-meta"),
            &false,
        );

    let employee_badge_id = client.mint(
            &admin,
            &employee,
            &BadgeKind::Employee,
            &bytes_from_str(&env, "employee-meta"),
            &true,
        );

    assert!(employer_badge_id != employee_badge_id);

    let employer_badge: Badge = client.get_badge(&employer_badge_id).unwrap();
    assert_eq!(employer_badge.owner, employer);
    assert_eq!(employer_badge.kind, BadgeKind::Employer);
    assert!(!employer_badge.transferable);

    let employee_badge: Badge = client.get_badge(&employee_badge_id).unwrap();
    assert_eq!(employee_badge.owner, employee);
    assert_eq!(employee_badge.kind, BadgeKind::Employee);
    assert!(employee_badge.transferable);

    let employer_badges = client.badges_of(&employer);
    assert_eq!(employer_badges.len(), 1);
    assert_eq!(employer_badges.get(0).unwrap(), employer_badge_id);

    let employee_badges = client.badges_of(&employee);
    assert_eq!(employee_badges.len(), 1);
    assert_eq!(employee_badges.get(0).unwrap(), employee_badge_id);
}

#[test]
fn custom_badge_and_transferable_flag() {
    let env = create_env();
    let (client, admin, employer, _) = setup_initialized(&env);

    let badge_id = client.mint(
            &admin,
            &employer,
            &BadgeKind::Custom(42),
            &bytes_from_str(&env, "custom"),
            &true,
        );

    let badge = client.get_badge(&badge_id).unwrap();
    assert_eq!(badge.kind, BadgeKind::Custom(42));
    assert!(badge.transferable);
}

#[test]
fn transfer_only_allowed_for_transferable_badges() {
    let env = create_env();
    let (client, admin, employer, employee) = setup_initialized(&env);

    let non_transferable_id = client.mint(
            &admin,
            &employer,
            &BadgeKind::Employer,
            &bytes_from_str(&env, "nt"),
            &false,
        );

    let transferable_id = client.mint(
            &admin,
            &employee,
            &BadgeKind::Employee,
            &bytes_from_str(&env, "t"),
            &true,
        );

    // Non-transferable: transfer should fail.
    let res = client.try_transfer(&employer, &non_transferable_id, &employee);
    assert_eq!(res, Err(Ok(BadgeError::TransferNotAllowed)));

    // Transferable: transfer succeeds when called by owner.
    client.transfer(&employee, &transferable_id, &employer);

    let owner = client.owner_of(&transferable_id).unwrap();
    assert_eq!(owner, employer);
}

#[test]
fn burn_by_admin_or_owner() {
    let env = create_env();
    let (client, admin, employer, employee) = setup_initialized(&env);

    let admin_minted_id = client.mint(
            &admin,
            &employer,
            &BadgeKind::Employer,
            &bytes_from_str(&env, "admin-minted"),
            &false,
        );

    let owner_minted_id = client.mint(
            &admin,
            &employee,
            &BadgeKind::Employee,
            &bytes_from_str(&env, "owner-minted"),
            &false,
        );

    // Admin can burn employer badge.
    client.burn(&admin, &admin_minted_id);
    assert!(client.get_badge(&admin_minted_id).is_none());

    // Owner can burn their own badge.
    client.burn(&employee, &owner_minted_id);
    assert!(client.get_badge(&owner_minted_id).is_none());
}

#[test]
fn non_admin_cannot_mint_or_burn_others() {
    let env = create_env();
    let (client, admin, employer, employee) = setup_initialized(&env);

    // Non-admin mint must fail.
    // Non-admin mint panics with NotAdmin.
    let res = client.try_mint(
        &employer,
        &employee,
        &BadgeKind::Employee,
        &bytes_from_str(&env, "x"),
        &true,
    );
    assert!(res.is_err());

    // Admin mints a badge for employee.
    let badge_id = client.mint(
            &admin,
            &employee,
            &BadgeKind::Employee,
            &bytes_from_str(&env, "y"),
            &true,
        );

    // Employer (not admin, not owner) cannot burn.
    let res = client.try_burn(&employer, &badge_id);
    assert_eq!(res, Err(Ok(BadgeError::NotOwnerOrAdmin)));
}

