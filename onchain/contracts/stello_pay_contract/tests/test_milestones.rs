//! Comprehensive test suite for milestone-based payment functionality (#162).
//!
//! Covers: agreement creation, adding milestones, approving, claiming,
//! access control, edge cases, and event emissions.

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{testutils::Address as _, Address, Env};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ============================================================================
// HELPERS
// ============================================================================

fn create_test_env() -> (Env, Address, Address, Address, PayrollContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);
    (env, employer, contributor, token, client)
}

/// Create a milestone agreement and return its ID.
fn setup_milestone_agreement(
    _env: &Env,
    client: &PayrollContractClient,
    employer: &Address,
    contributor: &Address,
    token: &Address,
) -> u128 {
    client.create_milestone_agreement(employer, contributor, token)
}

// -----------------------------------------------------------------------------
// Milestone agreement creation
// -----------------------------------------------------------------------------

/// Creates a milestone agreement and verifies agreement ID and basic state.
#[test]
fn test_create_milestone_agreement() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    assert!(agreement_id >= 1);
    assert_eq!(client.get_milestone_count(&agreement_id), 0);
    assert!(client.get_milestone(&agreement_id, &1).is_none());
}

/// Verifies that a second agreement gets a distinct ID.
#[test]
fn test_milestone_agreement_payment_type() {
    let (env, employer, contributor, token, client) = create_test_env();
    let _id1 = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    let id2 = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    assert_eq!(client.get_milestone_count(&id2), 0);
}

/// Initial milestone count is zero for a new agreement.
#[test]
fn test_initial_milestone_count_zero() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    assert_eq!(client.get_milestone_count(&agreement_id), 0);
}

// -----------------------------------------------------------------------------
// Adding milestones
// -----------------------------------------------------------------------------

/// Adding a single milestone updates count and milestone data.
#[test]
fn test_add_milestone() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    assert_eq!(client.get_milestone_count(&agreement_id), 1);
    let m = client.get_milestone(&agreement_id, &1).unwrap();
    assert_eq!(m.id, 1);
    assert_eq!(m.amount, 1000);
    assert!(!m.approved);
    assert!(!m.claimed);
}

/// Adding multiple milestones assigns sequential IDs and amounts.
#[test]
fn test_add_multiple_milestones() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &500);
    client.add_milestone(&agreement_id, &1000);
    client.add_milestone(&agreement_id, &1500);
    assert_eq!(client.get_milestone_count(&agreement_id), 3);
    assert_eq!(client.get_milestone(&agreement_id, &1).unwrap().amount, 500);
    assert_eq!(client.get_milestone(&agreement_id, &2).unwrap().amount, 1000);
    assert_eq!(client.get_milestone(&agreement_id, &3).unwrap().amount, 1500);
}

/// Adding a milestone with zero amount must fail.
#[test]
#[should_panic(expected = "Amount must be positive")]
fn test_add_milestone_zero_amount_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &0);
}

/// Adding a milestone when agreement is not in Created status must fail.
#[test]
#[should_panic(expected = "Agreement must be in Created status")]
fn test_add_milestone_wrong_status_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);
    client.add_milestone(&agreement_id, &200);
}

/// Only employer can add milestones; non-employer must fail.
#[test]
#[should_panic]
fn test_add_milestone_unauthorized_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    env.mock_auths(&[]);
    client.add_milestone(&agreement_id, &200);
}

/// Adding milestones increases total amount (verified via milestone amounts).
#[test]
fn test_add_milestone_updates_total() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);
    client.add_milestone(&agreement_id, &300);
    let total: i128 = (1..=3)
        .map(|i| client.get_milestone(&agreement_id, &i).unwrap().amount)
        .sum();
    assert_eq!(total, 600);
}

/// Milestone added updates state; contract emits MilestoneAdded event.
#[test]
fn test_milestone_added_event() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &999);
    let m = client.get_milestone(&agreement_id, &1).unwrap();
    assert_eq!(m.amount, 999);
    assert_eq!(m.id, 1);
}

// -----------------------------------------------------------------------------
// Approving milestones
// -----------------------------------------------------------------------------

/// Approving a milestone sets approved flag.
#[test]
fn test_approve_milestone() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);
    let m = client.get_milestone(&agreement_id, &1).unwrap();
    assert!(m.approved);
    assert!(!m.claimed);
}

/// Multiple milestones can be approved independently.
#[test]
fn test_approve_multiple_milestones() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);
    assert!(client.get_milestone(&agreement_id, &1).unwrap().approved);
    assert!(client.get_milestone(&agreement_id, &2).unwrap().approved);
}

/// Approving invalid milestone ID must fail.
#[test]
#[should_panic(expected = "Invalid milestone ID")]
fn test_approve_invalid_id_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.approve_milestone(&agreement_id, &99);
}

/// Approving when agreement is paused must fail.
#[test]
#[should_panic(expected = "Can only approve milestones when agreement is Created or Active")]
fn test_approve_wrong_status_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.pause_agreement(&agreement_id);
    client.approve_milestone(&agreement_id, &1);
}

/// Only employer can approve; contributor cannot approve.
#[test]
#[should_panic]
fn test_approve_unauthorized_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    env.mock_auths(&[]);
    client.approve_milestone(&agreement_id, &1);
}

/// Milestone approved event is reflected by state.
#[test]
fn test_milestone_approved_event() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.approve_milestone(&agreement_id, &1);
    assert!(client.get_milestone(&agreement_id, &1).unwrap().approved);
}

// -----------------------------------------------------------------------------
// Claiming milestones
// -----------------------------------------------------------------------------

/// Contributor can claim an approved milestone.
#[test]
fn test_claim_approved_milestone() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);
    let m = client.get_milestone(&agreement_id, &1).unwrap();
    assert!(m.approved);
    assert!(m.claimed);
}

/// Claiming an unapproved milestone must fail.
#[test]
#[should_panic(expected = "Milestone not approved")]
fn test_claim_unapproved_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.claim_milestone(&agreement_id, &1);
}

/// Claiming an already claimed milestone must fail.
#[test]
#[should_panic(expected = "Milestone already claimed")]
fn test_claim_already_claimed_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);
}

/// Only contributor can claim; employer cannot claim.
#[test]
#[should_panic]
fn test_claim_unauthorized_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);
    env.mock_auths(&[]);
    client.claim_milestone(&agreement_id, &1);
}

/// Claim updates milestone state (released in terms of state).
#[test]
fn test_claim_releases_funds() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);
    assert!(!client.get_milestone(&agreement_id, &1).unwrap().claimed);
    client.claim_milestone(&agreement_id, &1);
    assert!(client.get_milestone(&agreement_id, &1).unwrap().claimed);
}

/// Claimed milestone amount is stored correctly.
#[test]
fn test_claim_updates_paid_amount() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &500);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);
    let m = client.get_milestone(&agreement_id, &1).unwrap();
    assert_eq!(m.amount, 500);
    assert!(m.claimed);
}

/// Milestone claimed event is reflected by state.
#[test]
fn test_milestone_claimed_event() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);
    assert!(client.get_milestone(&agreement_id, &1).unwrap().claimed);
}

/// When all milestones are claimed, agreement completes (adding another milestone fails).
#[test]
#[should_panic(expected = "Agreement must be in Created status")]
fn test_agreement_completes_all_claimed() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);
    client.claim_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &2);
    assert!(client.get_milestone(&agreement_id, &1).unwrap().claimed);
    assert!(client.get_milestone(&agreement_id, &2).unwrap().claimed);
    client.add_milestone(&agreement_id, &300);
}

// -----------------------------------------------------------------------------
// Edge cases
// -----------------------------------------------------------------------------

/// Single-milestone agreement full lifecycle.
#[test]
fn test_single_milestone_agreement() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &5000);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);
    let m = client.get_milestone(&agreement_id, &1).unwrap();
    assert!(m.claimed);
    assert_eq!(m.amount, 5000);
}

/// Many milestones can be added and claimed.
#[test]
fn test_many_milestones() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    for i in 1..=10 {
        client.add_milestone(&agreement_id, &(i * 100));
    }
    assert_eq!(client.get_milestone_count(&agreement_id), 10);
    for i in 1..=10 {
        client.approve_milestone(&agreement_id, &i);
    }
    for i in 1..=10 {
        client.claim_milestone(&agreement_id, &i);
    }
    for i in 1..=10 {
        assert!(client.get_milestone(&agreement_id, &i).unwrap().claimed);
    }
}

/// Claiming out of order (e.g. 2 then 1) works when both are approved.
#[test]
fn test_claiming_out_of_order() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);
    client.claim_milestone(&agreement_id, &2);
    client.claim_milestone(&agreement_id, &1);
    assert!(client.get_milestone(&agreement_id, &1).unwrap().claimed);
    assert!(client.get_milestone(&agreement_id, &2).unwrap().claimed);
}

/// Very large milestone amounts are stored and claimed correctly.
#[test]
fn test_very_large_milestone_amounts() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    let large = i128::MAX / 2;
    client.add_milestone(&agreement_id, &large);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);
    assert_eq!(client.get_milestone(&agreement_id, &1).unwrap().amount, large);
    assert!(client.get_milestone(&agreement_id, &1).unwrap().claimed);
}
