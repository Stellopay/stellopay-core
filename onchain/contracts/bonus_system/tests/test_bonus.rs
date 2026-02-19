use bonus_system::{ApprovalStatus, BonusSystemContract, BonusSystemContractClient, IncentiveKind};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{token, Address, Env};

fn create_token<'a>(env: &Env, admin: &Address) -> token::Client<'a> {
    let token_address = env.register_stellar_asset_contract(admin.clone());
    token::Client::new(env, &token_address)
}

fn create_contract<'a>(env: &Env) -> BonusSystemContractClient<'a> {
    let contract_id = env.register_contract(None, BonusSystemContract);
    BonusSystemContractClient::new(env, &contract_id)
}

fn set_time(env: &Env, timestamp: u64) {
    env.ledger().with_mut(|ledger| {
        ledger.timestamp = timestamp;
    });
}

#[test]
fn test_create_and_approve_one_time_bonus() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &1_000);

    client.initialize(&owner);

    let incentive_id = client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &250,
        &100,
    );

    let stored = client.get_incentive(&incentive_id).unwrap();
    assert_eq!(stored.kind, IncentiveKind::OneTime);
    assert_eq!(stored.status, ApprovalStatus::Pending);

    client.approve_incentive(&approver, &incentive_id);
    let approved = client.get_incentive(&incentive_id).unwrap();
    assert_eq!(approved.status, ApprovalStatus::Approved);
}

#[test]
fn test_one_time_bonus_claim_after_unlock() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &500);

    client.initialize(&owner);
    let incentive_id = client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &300,
        &200,
    );

    client.approve_incentive(&approver, &incentive_id);
    set_time(&env, 220);

    let claimed = client.claim_incentive(&employee, &incentive_id);
    assert_eq!(claimed, 300);
    assert_eq!(token_client.balance(&employee), 300);

    let stored = client.get_incentive(&incentive_id).unwrap();
    assert_eq!(stored.status, ApprovalStatus::Completed);
    assert_eq!(stored.claimed_payouts, 1);
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
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &500);

    client.initialize(&owner);
    let incentive_id = client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &100,
        &50,
    );

    client.approve_incentive(&attacker, &incentive_id);
}

#[test]
#[should_panic(expected = "Incentive is not approved")]
fn test_claim_requires_approval() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &500);

    client.initialize(&owner);
    let incentive_id = client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &100,
        &20,
    );

    set_time(&env, 100);
    client.claim_incentive(&employee, &incentive_id);
}

#[test]
fn test_recurring_incentive_claims_in_batches() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &2_000);

    client.initialize(&owner);
    let incentive_id = client.create_recurring_incentive(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &100,
        &5,
        &1_000,
        &10,
    );

    client.approve_incentive(&approver, &incentive_id);

    set_time(&env, 1_000);
    assert_eq!(client.get_claimable_payouts(&incentive_id), 1);
    assert_eq!(client.claim_incentive(&employee, &incentive_id), 100);

    set_time(&env, 1_029);
    assert_eq!(client.get_claimable_payouts(&incentive_id), 2);
    assert_eq!(client.claim_incentive(&employee, &incentive_id), 200);

    set_time(&env, 2_000);
    assert_eq!(client.get_claimable_payouts(&incentive_id), 2);
    assert_eq!(client.claim_incentive(&employee, &incentive_id), 200);

    assert_eq!(token_client.balance(&employee), 500);
    let stored = client.get_incentive(&incentive_id).unwrap();
    assert_eq!(stored.status, ApprovalStatus::Completed);
    assert_eq!(stored.claimed_payouts, 5);
}

#[test]
fn test_reject_and_cancel_refunds_employer() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &800);

    client.initialize(&owner);
    let incentive_id = client.create_recurring_incentive(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &100,
        &4,
        &500,
        &5,
    );

    client.reject_incentive(&approver, &incentive_id);
    let refunded = client.cancel_incentive(&employer, &incentive_id);

    assert_eq!(refunded, 400);
    assert_eq!(token_client.balance(&employer), 800);

    let stored = client.get_incentive(&incentive_id).unwrap();
    assert_eq!(stored.status, ApprovalStatus::Cancelled);
}

#[test]
#[should_panic(expected = "Incentive cannot be cancelled")]
fn test_cannot_cancel_approved_incentive() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &500);

    client.initialize(&owner);
    let incentive_id = client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &200,
        &10,
    );

    client.approve_incentive(&approver, &incentive_id);
    client.cancel_incentive(&employer, &incentive_id);
}

#[test]
#[should_panic(expected = "No payouts available")]
fn test_recurring_claim_before_start_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &500);

    client.initialize(&owner);
    let incentive_id = client.create_recurring_incentive(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &50,
        &3,
        &1_000,
        &20,
    );

    client.approve_incentive(&approver, &incentive_id);
    set_time(&env, 900);
    client.claim_incentive(&employee, &incentive_id);
}