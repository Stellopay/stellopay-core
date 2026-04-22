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

// ============================================
// EXISTING TESTS (kept for backward compatibility)
// ============================================

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

    let second = client.try_claim_incentive(&employee, &incentive_id);
    assert!(second.is_err());
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

// ============================================
// BONUS CAP TESTS
// ============================================

#[test]
fn test_set_bonus_cap_by_admin() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);
    client.set_bonus_cap(&owner, &None, &1000);

    assert_eq!(client.get_period_cap(), 1000);
}

#[test]
#[should_panic(expected = "Only owner can set caps")]
fn test_non_admin_cannot_set_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);
    client.set_bonus_cap(&attacker, &None, &1000);
}

#[test]
fn test_bonus_within_cap_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &1000);

    client.initialize(&owner);
    client.set_bonus_cap(&owner, &Some(employee.clone()), &500);

    let incentive_id = client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &300,
        &100,
    );

    let stored = client.get_incentive(&incentive_id).unwrap();
    assert_eq!(stored.amount_per_payout, 300);
}

#[test]
#[should_panic(expected = "Bonus exceeds employee cap")]
fn test_bonus_exceeding_cap_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &1000);

    client.initialize(&owner);
    client.set_bonus_cap(&owner, &Some(employee.clone()), &500);
    client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &300,
        &100,
    );

    // This should fail as it exceeds the cap
    client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &300,
        &100,
    );
}

#[test]
fn test_bonus_at_exact_cap_boundary_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &1000);

    client.initialize(&owner);
    client.set_bonus_cap(&owner, &Some(employee.clone()), &500);

    // Create bonus exactly at cap
    let incentive_id = client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &500,
        &100,
    );

    let stored = client.get_incentive(&incentive_id).unwrap();
    assert_eq!(stored.amount_per_payout, 500);
    assert_eq!(client.get_employee_bonus_total(&employee), 500);
}

#[test]
fn test_multiple_bonuses_in_same_period_respect_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &2000);

    client.initialize(&owner);
    client.set_bonus_cap(&owner, &Some(employee.clone()), &1000);

    // First bonus
    client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &400,
        &100,
    );

    // Second bonus
    client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &400,
        &100,
    );

    assert_eq!(client.get_employee_bonus_total(&employee), 800);

    // Third bonus should fail (would exceed cap)
    let result = client.try_create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &300,
        &100,
    );
    assert!(result.is_err());
}

#[test]
fn test_period_cap_enforcement() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &2000);

    client.initialize(&owner);
    client.set_bonus_cap(&owner, &None, &1000); // Period cap

    // Employee 1 bonus
    client.create_one_time_bonus(
        &employer,
        &employee1,
        &approver,
        &token_client.address,
        &600,
        &100,
    );

    // Employee 2 bonus should fail (would exceed period cap)
    let result = client.try_create_one_time_bonus(
        &employer,
        &employee2,
        &approver,
        &token_client.address,
        &500,
        &100,
    );
    assert!(result.is_err());
}

#[test]
fn test_cap_reset_across_periods() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &2000);

    client.initialize(&owner);
    client.set_bonus_cap(&owner, &Some(employee.clone()), &500);

    // Create bonus in period 0
    set_time(&env, 1000);
    client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &500,
        &100,
    );

    // Move to next period (30 days = 2592000 seconds)
    set_time(&env, 2600000);

    // Should be able to create another bonus in new period
    let incentive_id = client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &500,
        &100,
    );

    let stored = client.get_incentive(&incentive_id).unwrap();
    assert_eq!(stored.amount_per_payout, 500);
}

// ============================================
// CLAWBACK TESTS
// ============================================

#[test]
fn test_admin_clawback_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &1000);

    client.initialize(&owner);
    let incentive_id = client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &500,
        &100,
    );

    client.approve_incentive(&approver, &incentive_id);
    set_time(&env, 200);
    client.claim_incentive(&employee, &incentive_id);

    // Execute clawback (employee approval required, handled by mock_all_auths)
    let reason_hash = 123456789u128;
    let clawed = client.execute_clawback(&owner, &employee, &incentive_id, &500, &reason_hash);

    assert_eq!(clawed, 500);
    assert_eq!(client.get_clawback_total(&incentive_id), 500);
    // Verify funds returned to employer
    assert_eq!(token_client.balance(&employer), 1000);
}

#[test]
#[should_panic(expected = "Clawback exceeds claimed amount")]
fn test_clawback_exceeding_claimed_amount_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &1000);

    client.initialize(&owner);
    let incentive_id = client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &500,
        &100,
    );

    client.approve_incentive(&approver, &incentive_id);
    set_time(&env, 200);
    client.claim_incentive(&employee, &incentive_id);

    // Try to claw back more than claimed
    client.execute_clawback(&owner, &employee, &incentive_id, &600, &123456789u128);
}

#[test]
#[should_panic(expected = "Only owner can execute clawback")]
fn test_non_admin_cannot_clawback() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &1000);

    client.initialize(&owner);
    let incentive_id = client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &500,
        &100,
    );

    client.approve_incentive(&approver, &incentive_id);
    set_time(&env, 200);
    client.claim_incentive(&employee, &incentive_id);

    // Non-owner tries clawback
    client.execute_clawback(&attacker, &employee, &incentive_id, &500, &123456789u128);
}

#[test]
fn test_clawback_returns_to_employer() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &1000);
    let initial_balance = token_client.balance(&employer);

    client.initialize(&owner);
    let incentive_id = client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &500,
        &100,
    );

    client.approve_incentive(&approver, &incentive_id);
    set_time(&env, 200);
    client.claim_incentive(&employee, &incentive_id);

    // Execute clawback
    client.execute_clawback(&owner, &employee, &incentive_id, &500, &123456789u128);

    // Employer should get funds back
    assert_eq!(token_client.balance(&employer), initial_balance);
}

#[test]
fn test_multiple_clawbacks_same_incentive() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &2000);

    client.initialize(&owner);
    let incentive_id = client.create_recurring_incentive(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &100,
        &5,
        &1000,
        &10,
    );

    client.approve_incentive(&approver, &incentive_id);
    set_time(&env, 1025);
    client.claim_incentive(&employee, &incentive_id); // Claim 3 payouts = 300 (time 1000, 1010, 1020)

    // First clawback
    client.execute_clawback(&owner, &employee, &incentive_id, &100, &111u128);
    assert_eq!(client.get_clawback_total(&incentive_id), 100);

    // Second clawback
    client.execute_clawback(&owner, &employee, &incentive_id, &100, &222u128);
    assert_eq!(client.get_clawback_total(&incentive_id), 200);

    // Verify remaining claimable is 100 (300 claimed - 200 clawed)
    let incentive = client.get_incentive(&incentive_id).unwrap();
    let claimed = incentive.claimed_payouts as i128 * incentive.amount_per_payout as i128;
    assert_eq!(claimed, 300);

    // Third clawback should fail (only 100 remaining, trying to claw 150)
    let result = client.try_execute_clawback(&owner, &employee, &incentive_id, &150, &333u128);
    assert!(
        result.is_err(),
        "Should fail: trying to clawback 150 but only 100 remaining"
    );
}

// ============================================
// TERMINATION FLOW TESTS
// ============================================

#[test]
fn test_terminate_employee_by_admin() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employee = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);
    assert!(!client.is_employee_terminated(&employee));

    client.terminate_employee(&owner, &employee);
    assert!(client.is_employee_terminated(&employee));
}

#[test]
fn test_bonus_creation_blocked_after_termination() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &1000);

    client.initialize(&owner);
    client.terminate_employee(&owner, &employee);

    let result = client.try_create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &500,
        &100,
    );

    assert!(result.is_err());
}

#[test]
fn test_existing_bonus_still_claimable_after_termination() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &1000);

    client.initialize(&owner);
    let incentive_id = client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &500,
        &100,
    );

    client.approve_incentive(&approver, &incentive_id);

    // Terminate employee
    client.terminate_employee(&owner, &employee);

    // Employee should still be able to claim
    set_time(&env, 200);
    let claimed = client.claim_incentive(&employee, &incentive_id);
    assert_eq!(claimed, 500);
}

#[test]
fn test_clawback_works_on_terminated_employee() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &1000);

    client.initialize(&owner);
    let incentive_id = client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &500,
        &100,
    );

    client.approve_incentive(&approver, &incentive_id);
    set_time(&env, 200);
    client.claim_incentive(&employee, &incentive_id);

    // Terminate employee
    client.terminate_employee(&owner, &employee);

    // Clawback should still work (with employee approval via mock_all_auths)
    let clawed = client.execute_clawback(&owner, &employee, &incentive_id, &500, &123456789u128);
    assert_eq!(clawed, 500);
}

// ============================================
// PARTIAL WITHDRAWAL + CLAWBACK TESTS
// ============================================

#[test]
fn test_partial_claim_then_clawback() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &2000);

    client.initialize(&owner);
    let incentive_id = client.create_recurring_incentive(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &100,
        &5,
        &1000,
        &10,
    );

    client.approve_incentive(&approver, &incentive_id);
    set_time(&env, 1020);
    client.claim_incentive(&employee, &incentive_id); // Claim 3 payouts = 300

    // Claw back partial amount
    let clawed = client.execute_clawback(&owner, &employee, &incentive_id, &200, &123456789u128);
    assert_eq!(clawed, 200);
    assert_eq!(client.get_clawback_total(&incentive_id), 200);
}

#[test]
fn test_full_claim_then_full_clawback() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &1000);

    client.initialize(&owner);
    let incentive_id = client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &500,
        &100,
    );

    client.approve_incentive(&approver, &incentive_id);
    set_time(&env, 200);
    client.claim_incentive(&employee, &incentive_id);

    // Full clawback
    let clawed = client.execute_clawback(&owner, &employee, &incentive_id, &500, &123456789u128);
    assert_eq!(clawed, 500);

    // Cannot claw back again
    let result = client.try_execute_clawback(&owner, &employee, &incentive_id, &1, &999u128);
    assert!(result.is_err());
}

#[test]
fn test_full_lifecycle_create_approve_claim_clawback() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &2000);

    // Initialize and set cap
    client.initialize(&owner);
    client.set_bonus_cap(&owner, &Some(employee.clone()), &1000);

    // Create bonus
    let incentive_id = client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &500,
        &100,
    );

    // Approve
    client.approve_incentive(&approver, &incentive_id);

    // Claim
    set_time(&env, 200);
    let claimed = client.claim_incentive(&employee, &incentive_id);
    assert_eq!(claimed, 500);

    // Clawback
    let clawed = client.execute_clawback(&owner, &employee, &incentive_id, &500, &123456789u128);
    assert_eq!(clawed, 500);

    // Verify totals
    assert_eq!(client.get_employee_bonus_total(&employee), 500);
    assert_eq!(client.get_clawback_total(&incentive_id), 500);
}

#[test]
fn test_cap_and_termination_interaction() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &2000);

    client.initialize(&owner);
    client.set_bonus_cap(&owner, &Some(employee.clone()), &1000);

    // Create bonus within cap
    let incentive_id = client.create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &500,
        &100,
    );

    // Terminate employee
    client.terminate_employee(&owner, &employee);

    // Cannot create new bonus even within cap
    let result = client.try_create_one_time_bonus(
        &employer,
        &employee,
        &approver,
        &token_client.address,
        &400,
        &100,
    );
    assert!(result.is_err());

    // But existing bonus can still be claimed
    client.approve_incentive(&approver, &incentive_id);
    set_time(&env, 200);
    let claimed = client.claim_incentive(&employee, &incentive_id);
    assert_eq!(claimed, 500);
}

#[test]
fn test_concurrent_bonuses_respect_caps() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&employer, &3000);

    client.initialize(&owner);
    client.set_bonus_cap(&owner, &Some(employee1.clone()), &500);
    client.set_bonus_cap(&owner, &Some(employee2.clone()), &700);

    // Employee 1: Create multiple bonuses
    client.create_one_time_bonus(
        &employer,
        &employee1,
        &approver,
        &token_client.address,
        &200,
        &100,
    );
    client.create_one_time_bonus(
        &employer,
        &employee1,
        &approver,
        &token_client.address,
        &300,
        &100,
    );
    assert_eq!(client.get_employee_bonus_total(&employee1), 500);

    // Employee 2: Create bonuses
    client.create_one_time_bonus(
        &employer,
        &employee2,
        &approver,
        &token_client.address,
        &700,
        &100,
    );
    assert_eq!(client.get_employee_bonus_total(&employee2), 700);

    // Employee 1 cannot create more (at cap)
    let result = client.try_create_one_time_bonus(
        &employer,
        &employee1,
        &approver,
        &token_client.address,
        &1,
        &100,
    );
    assert!(result.is_err());
}
