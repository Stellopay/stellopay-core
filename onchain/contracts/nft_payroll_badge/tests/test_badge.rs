#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Bytes, Env,
};

use nft_payroll_badge::{Badge, BadgeError, BadgeKind, BadgeState, NftPayrollBadge, NftPayrollBadgeClient};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    
    // Set a non-zero timestamp. In Soroban tests, the default timestamp is 0.
    // Because our contract treats `expires_at == 0` as "never expires",
    // calling `expire()` when the ledger time is 0 creates a logical conflict.
    env.ledger().with_mut(|li| {
        li.timestamp = 1_000_000;
    });

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
        &0,
    );

    let employee_badge_id = client.mint(
        &admin,
        &employee,
        &BadgeKind::Employee,
        &bytes_from_str(&env, "employee-meta"),
        &true,
        &0,
    );

    assert!(employer_badge_id != employee_badge_id);

    let employer_badge: Badge = client.get_badge(&employer_badge_id).unwrap();
    assert_eq!(employer_badge.owner, employer);
    assert_eq!(employer_badge.kind, BadgeKind::Employer);
    assert!(!employer_badge.transferable);
    assert!(!employer_badge.metadata_frozen);

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
        &0,
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
        &0,
    );

    let transferable_id = client.mint(
        &admin,
        &employee,
        &BadgeKind::Employee,
        &bytes_from_str(&env, "t"),
        &true,
        &0,
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
        &0,
    );

    let owner_minted_id = client.mint(
        &admin,
        &employee,
        &BadgeKind::Employee,
        &bytes_from_str(&env, "owner-minted"),
        &false,
        &0,
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
    let res = client.try_mint(
        &employer,
        &employee,
        &BadgeKind::Employee,
        &bytes_from_str(&env, "x"),
        &true,
        &0,
    );
    assert!(res.is_err());

    // Admin mints a badge for employee.
    let badge_id = client.mint(
        &admin,
        &employee,
        &BadgeKind::Employee,
        &bytes_from_str(&env, "y"),
        &true,
        &0,
    );

    // Employer (not admin, not owner) cannot burn.
    let res = client.try_burn(&employer, &badge_id);
    assert_eq!(res, Err(Ok(BadgeError::NotOwnerOrAdmin)));
}

#[test]
fn metadata_too_long() {
    let env = create_env();
    let (client, admin, employee, _) = setup_initialized(&env);

    // Create 1025 bytes of metadata (limit is 1024).
    let mut data = [0u8; 1025];
    for i in 0..1025 {
        data[i] = (i % 256) as u8;
    }
    let metadata = Bytes::from_slice(&env, &data);

    let res = client.try_mint(
        &admin,
        &employee,
        &BadgeKind::Employee,
        &metadata,
        &true,
        &0,
    );
    assert_eq!(res, Err(Ok(BadgeError::MetadataTooLong)));

    // Exactly 1024 bytes should work.
    let mut data2 = [0u8; 1024];
    for i in 0..1024 {
        data2[i] = (i % 256) as u8;
    }
    let metadata2 = Bytes::from_slice(&env, &data2);
    let res2 = client.try_mint(
        &admin,
        &employee,
        &BadgeKind::Employee,
        &metadata2,
        &true,
        &0,
    );
    assert!(res2.is_ok());
}

#[test]
fn metadata_update_and_freeze() {
    let env = create_env();
    let (client, admin, employer, _) = setup_initialized(&env);

    let badge_id = client.mint(
        &admin,
        &employer,
        &BadgeKind::Employer,
        &bytes_from_str(&env, "meta1"),
        &true,
        &0,
    );

    // Update metadata
    client.update_metadata(&admin, &badge_id, &bytes_from_str(&env, "meta2"));
    let badge = client.get_badge(&badge_id).unwrap();
    assert_eq!(badge.metadata, bytes_from_str(&env, "meta2"));

    // Freeze
    client.freeze_metadata(&admin, &badge_id);
    let frozen_badge = client.get_badge(&badge_id).unwrap();
    assert!(frozen_badge.metadata_frozen);

    // Try update again -> error
    let res = client.try_update_metadata(&admin, &badge_id, &bytes_from_str(&env, "meta3"));
    assert_eq!(res, Err(Ok(BadgeError::MetadataFrozen)));
    
    // Try freeze again -> error
    let res2 = client.try_freeze_metadata(&admin, &badge_id);
    assert_eq!(res2, Err(Ok(BadgeError::MetadataFrozen)));
}

#[test]
fn revocation_and_expiry_flows() {
    let env = create_env();
    let (client, admin, _, employee) = setup_initialized(&env);

    let badge_id = client.mint(
        &admin,
        &employee,
        &BadgeKind::Employee,
        &bytes_from_str(&env, "employee"),
        &true,
        &0,
    );

    assert_eq!(client.get_state(&badge_id), BadgeState::Active);

    // Revoke
    client.revoke(&admin, &badge_id);
    assert_eq!(client.get_state(&badge_id), BadgeState::Revoked);

    // Repeated revoke -> error
    let res = client.try_revoke(&admin, &badge_id);
    assert_eq!(res, Err(Ok(BadgeError::AlreadyRevoked)));

    // Try transfer revoked badge -> error
    let res_transfer = client.try_transfer(&employee, &badge_id, &admin);
    assert_eq!(res_transfer, Err(Ok(BadgeError::BadgeRevoked)));
}

#[test]
fn explicit_expiry() {
    let env = create_env();
    let (client, admin, _, employee) = setup_initialized(&env);

    let badge_id = client.mint(
        &admin,
        &employee,
        &BadgeKind::Employee,
        &bytes_from_str(&env, "employee"),
        &true,
        &0,
    );

    // Expire
    client.expire(&admin, &badge_id);
    assert_eq!(client.get_state(&badge_id), BadgeState::Expired);
    
    // Repeated expire -> error
    let res = client.try_expire(&admin, &badge_id);
    assert_eq!(res, Err(Ok(BadgeError::BadgeExpired)));
    
    // Transfer expired -> error
    let res_transfer = client.try_transfer(&employee, &badge_id, &admin);
    assert_eq!(res_transfer, Err(Ok(BadgeError::BadgeExpired)));
}

#[test]
fn metadata_too_long_on_update() {
    let env = create_env();
    let (client, admin, employee, _) = setup_initialized(&env);

    let badge_id = client.mint(
        &admin,
        &employee,
        &BadgeKind::Employee,
        &bytes_from_str(&env, "short"),
        &false,
        &0,
    );

    let mut data = [0u8; 1025];
    for i in 0..1025 {
        data[i] = (i % 256) as u8;
    }
    let metadata = Bytes::from_slice(&env, &data);

    let res = client.try_update_metadata(&admin, &badge_id, &metadata);
    assert_eq!(res, Err(Ok(BadgeError::MetadataTooLong)));
}

#[test]
fn non_admin_update_revoke_fails() {
    let env = create_env();
    let (client, admin, employer, employee) = setup_initialized(&env);

    let badge_id = client.mint(
        &admin,
        &employee,
        &BadgeKind::Employee,
        &bytes_from_str(&env, "emp"),
        &true,
        &0,
    );

    // Employer is not admin, should fail to update/revoke/expire
    let res1 = client.try_update_metadata(&employer, &badge_id, &bytes_from_str(&env, "new"));
    assert!(res1.is_err());
    
    let res2 = client.try_revoke(&employer, &badge_id);
    assert!(res2.is_err());
    
    let res3 = client.try_expire(&employer, &badge_id);
    assert!(res3.is_err());
    
    let res4 = client.try_freeze_metadata(&employer, &badge_id);
    assert!(res4.is_err());
}
