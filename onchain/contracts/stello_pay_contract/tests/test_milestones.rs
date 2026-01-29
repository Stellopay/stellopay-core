#![cfg(test)]

use soroban_sdk::{testutils::Address as _, token, Address, Env};
use stello_pay_contract::storage::{AgreementStatus, MilestoneKey, PaymentType};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ============================================================================
// Helpers
// ============================================================================

fn create_test_env() -> (Env, Address, Address, Address, PayrollContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    (env, employer, contributor, token, client)
}

fn create_token_contract<'a>(
    e: &'a Env,
    admin: &Address,
) -> (Address, token::Client<'a>, token::StellarAssetClient<'a>) {
    let token_id = e.register_stellar_asset_contract_v2(admin.clone());
    let token = token_id.address();
    let token_client = token::Client::new(e, &token);
    let token_admin_client = token::StellarAssetClient::new(e, &token);
    (token, token_client, token_admin_client)
}

fn get_milestone_status(env: &Env, contract: &Address, agreement_id: u128) -> AgreementStatus {
    env.as_contract(contract, || {
        env.storage()
            .instance()
            .get(&MilestoneKey::Status(agreement_id))
            .unwrap()
    })
}

fn get_milestone_payment_type(env: &Env, contract: &Address, agreement_id: u128) -> PaymentType {
    env.as_contract(contract, || {
        env.storage()
            .instance()
            .get(&MilestoneKey::PaymentType(agreement_id))
            .unwrap()
    })
}

fn get_milestone_total(env: &Env, contract: &Address, agreement_id: u128) -> i128 {
    env.as_contract(contract, || {
        env.storage()
            .instance()
            .get(&MilestoneKey::TotalAmount(agreement_id))
            .unwrap()
    })
}

fn get_milestone_paid(env: &Env, contract: &Address, agreement_id: u128) -> i128 {
    env.as_contract(contract, || {
        env.storage()
            .instance()
            .get(&MilestoneKey::PaidAmount(agreement_id))
            .unwrap()
    })
}

// ============================================================================
// Milestone agreement creation
// ============================================================================

/// test_create_milestone_agreement()
#[test]
fn test_create_milestone_agreement() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    assert_eq!(agreement_id, 1);
    assert_eq!(
        get_milestone_status(&env, &client.address, agreement_id),
        AgreementStatus::Created
    );
}

/// test_milestone_agreement_payment_type()
#[test]
fn test_milestone_agreement_payment_type() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    assert_eq!(
        get_milestone_payment_type(&env, &client.address, agreement_id),
        PaymentType::MilestoneBased
    );
}

/// test_initial_milestone_count_zero()
#[test]
fn test_initial_milestone_count_zero() {
    let (_, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    assert_eq!(client.get_milestone_count(&agreement_id), 0);
}

// ============================================================================
// Adding milestones
// ============================================================================

/// test_add_milestone()
#[test]
fn test_add_milestone() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);

    assert_eq!(client.get_milestone_count(&agreement_id), 1);
    let m1 = client.get_milestone(&agreement_id, &1).unwrap();
    assert_eq!(m1.id, 1);
    assert_eq!(m1.amount, 1000);
    assert!(!m1.approved);
    assert!(!m1.claimed);
}

/// test_add_multiple_milestones()
#[test]
fn test_add_multiple_milestones() {
    let (_, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.add_milestone(&agreement_id, &2500);
    client.add_milestone(&agreement_id, &10);
    assert_eq!(client.get_milestone_count(&agreement_id), 3);
}

/// test_add_milestone_zero_amount_fails()
#[test]
#[should_panic(expected = "Amount must be positive")]
fn test_add_milestone_zero_amount_fails() {
    let (_, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &0);
}

/// test_add_milestone_wrong_status_fails()
#[test]
#[should_panic(expected = "Agreement must be in Created status")]
fn test_add_milestone_wrong_status_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    // Force status away from Created
    env.as_contract(&client.address, || {
        env.storage()
            .instance()
            .set(&MilestoneKey::Status(agreement_id), &AgreementStatus::Active);
    });

    client.add_milestone(&agreement_id, &1000);
}

/// test_add_milestone_unauthorized_fails()
#[test]
#[should_panic]
fn test_add_milestone_unauthorized_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    env.set_auths(&[]); // Clear auth mocks
    client.add_milestone(&agreement_id, &1000);
}

/// test_add_milestone_updates_total()
#[test]
fn test_add_milestone_updates_total() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    assert_eq!(get_milestone_total(&env, &client.address, agreement_id), 0);

    client.add_milestone(&agreement_id, &1000);
    client.add_milestone(&agreement_id, &2500);
    assert_eq!(get_milestone_total(&env, &client.address, agreement_id), 3500);
}

// ============================================================================
// Approving milestones
// ============================================================================

/// test_approve_milestone()
#[test]
fn test_approve_milestone() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);

    client.approve_milestone(&agreement_id, &1);
    let m1 = client.get_milestone(&agreement_id, &1).unwrap();
    assert!(m1.approved);
    assert!(!m1.claimed);

}

/// test_approve_multiple_milestones()
#[test]
fn test_approve_multiple_milestones() {
    let (_, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.add_milestone(&agreement_id, &2500);
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);
    assert!(client.get_milestone(&agreement_id, &1).unwrap().approved);
    assert!(client.get_milestone(&agreement_id, &2).unwrap().approved);
}

/// test_approve_invalid_id_fails()
#[test]
#[should_panic(expected = "Invalid milestone ID")]
fn test_approve_invalid_id_fails() {
    let (_, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &2);
}

/// test_approve_wrong_status_fails()
#[test]
#[should_panic(expected = "Agreement status does not allow approval")]
fn test_approve_wrong_status_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);

    env.as_contract(&client.address, || {
        env.storage()
            .instance()
            .set(&MilestoneKey::Status(agreement_id), &AgreementStatus::Paused);
    });

    client.approve_milestone(&agreement_id, &1);
}

/// test_approve_unauthorized_fails()
#[test]
#[should_panic]
fn test_approve_unauthorized_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);

    env.set_auths(&[]);
    client.approve_milestone(&agreement_id, &1);
}

// ============================================================================
// Claiming milestones
// ============================================================================

/// test_claim_approved_milestone()
#[test]
fn test_claim_approved_milestone() {
    let (env, employer, contributor, _, client) = create_test_env();
    let admin = Address::generate(&env);
    let (token, token_client, token_admin_client) = create_token_contract(&env, &admin);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);

    token_admin_client.mint(&client.address, &1000);
    client.claim_milestone(&agreement_id, &1);

    let m1 = client.get_milestone(&agreement_id, &1).unwrap();
    assert!(m1.claimed);
    assert_eq!(token_client.balance(&contributor), 1000);
}

/// test_claim_unapproved_fails()
#[test]
#[should_panic(expected = "Milestone not approved")]
fn test_claim_unapproved_fails() {
    let (_, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.claim_milestone(&agreement_id, &1);
}

/// test_claim_already_claimed_fails()
#[test]
#[should_panic(expected = "Milestone already claimed")]
fn test_claim_already_claimed_fails() {
    let (env, employer, contributor, _, client) = create_test_env();
    let admin = Address::generate(&env);
    let (token, _, token_admin_client) = create_token_contract(&env, &admin);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    // Use >1 milestone so agreement doesn't complete on first claim.
    client.add_milestone(&agreement_id, &1000);
    client.add_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &1);

    token_admin_client.mint(&client.address, &2000);
    client.claim_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);
}

/// test_claim_unauthorized_fails()
#[test]
#[should_panic]
fn test_claim_unauthorized_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);

    env.set_auths(&[]);
    client.claim_milestone(&agreement_id, &1);
}

/// test_claim_releases_funds()
#[test]
fn test_claim_releases_funds() {
    let (env, employer, contributor, _, client) = create_test_env();
    let admin = Address::generate(&env);
    let (token, token_client, token_admin_client) = create_token_contract(&env, &admin);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &500);
    client.approve_milestone(&agreement_id, &1);

    token_admin_client.mint(&client.address, &500);
    assert_eq!(token_client.balance(&contributor), 0);
    client.claim_milestone(&agreement_id, &1);
    assert_eq!(token_client.balance(&contributor), 500);
}

/// test_claim_updates_paid_amount()
#[test]
fn test_claim_updates_paid_amount() {
    let (env, employer, contributor, _, client) = create_test_env();
    let admin = Address::generate(&env);
    let (token, _, token_admin_client) = create_token_contract(&env, &admin);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);

    token_admin_client.mint(&client.address, &300);
    assert_eq!(get_milestone_paid(&env, &client.address, agreement_id), 0);
    client.claim_milestone(&agreement_id, &1);
    assert_eq!(get_milestone_paid(&env, &client.address, agreement_id), 100);
    client.claim_milestone(&agreement_id, &2);
    assert_eq!(get_milestone_paid(&env, &client.address, agreement_id), 300);
}

/// test_agreement_completes_all_claimed()
#[test]
fn test_agreement_completes_all_claimed() {
    let (env, employer, contributor, _, client) = create_test_env();
    let admin = Address::generate(&env);
    let (token, _, token_admin_client) = create_token_contract(&env, &admin);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);
    token_admin_client.mint(&client.address, &300);

    client.claim_milestone(&agreement_id, &1);
    assert_eq!(
        get_milestone_status(&env, &client.address, agreement_id),
        AgreementStatus::Created
    );
    client.claim_milestone(&agreement_id, &2);
    assert_eq!(
        get_milestone_status(&env, &client.address, agreement_id),
        AgreementStatus::Completed
    );
}

// ============================================================================
// Edge cases
// ============================================================================

/// test_single_milestone_agreement()
#[test]
fn test_single_milestone_agreement() {
    let (env, employer, contributor, _, client) = create_test_env();
    let admin = Address::generate(&env);
    let (token, _, token_admin_client) = create_token_contract(&env, &admin);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &999);
    client.approve_milestone(&agreement_id, &1);
    token_admin_client.mint(&client.address, &999);
    client.claim_milestone(&agreement_id, &1);
    assert_eq!(
        get_milestone_status(&env, &client.address, agreement_id),
        AgreementStatus::Completed
    );
}

/// test_many_milestones()
#[test]
fn test_many_milestones() {
    let (env, employer, contributor, _, client) = create_test_env();
    let admin = Address::generate(&env);
    let (token, _, token_admin_client) = create_token_contract(&env, &admin);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    let n: u32 = 25;
    let mut total: i128 = 0;
    for _ in 0..n {
        client.add_milestone(&agreement_id, &10);
        total += 10;
    }
    assert_eq!(client.get_milestone_count(&agreement_id), n);

    for i in 1..=n {
        client.approve_milestone(&agreement_id, &i);
    }
    token_admin_client.mint(&client.address, &total);
    for i in 1..=n {
        client.claim_milestone(&agreement_id, &i);
    }
    assert_eq!(
        get_milestone_status(&env, &client.address, agreement_id),
        AgreementStatus::Completed
    );
}

/// test_claiming_out_of_order()
#[test]
fn test_claiming_out_of_order() {
    let (env, employer, contributor, _, client) = create_test_env();
    let admin = Address::generate(&env);
    let (token, _, token_admin_client) = create_token_contract(&env, &admin);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);
    token_admin_client.mint(&client.address, &300);

    // Claim milestone 2 first.
    client.claim_milestone(&agreement_id, &2);
    assert!(client.get_milestone(&agreement_id, &2).unwrap().claimed);
    assert_eq!(
        get_milestone_status(&env, &client.address, agreement_id),
        AgreementStatus::Created
    );

    // Then claim milestone 1.
    client.claim_milestone(&agreement_id, &1);
    assert_eq!(
        get_milestone_status(&env, &client.address, agreement_id),
        AgreementStatus::Completed
    );
}

/// test_very_large_milestone_amounts()
#[test]
fn test_very_large_milestone_amounts() {
    let (env, employer, contributor, _, client) = create_test_env();
    let admin = Address::generate(&env);
    let (token, _, token_admin_client) = create_token_contract(&env, &admin);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    let big: i128 = 9_000_000_000_000i128;
    client.add_milestone(&agreement_id, &big);
    client.approve_milestone(&agreement_id, &1);
    token_admin_client.mint(&client.address, &big);
    client.claim_milestone(&agreement_id, &1);
    assert_eq!(
        get_milestone_status(&env, &client.address, agreement_id),
        AgreementStatus::Completed
    );
}
