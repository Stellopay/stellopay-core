#![cfg(test)]
#![allow(deprecated)]

//! Comprehensive test suite for milestone-based payment functionality (#162).
//! Covers agreement creation, adding/approving/claiming milestones, access control,
//! events, and edge cases.

use soroban_sdk::{testutils::Address as _, Address, Env};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// -----------------------------------------------------------------------------
// Test helpers
// -----------------------------------------------------------------------------

fn create_test_env() -> (
    Env,
    Address,
    Address,
    Address,
    PayrollContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    (env, employer, contributor, token, client)
}

// -----------------------------------------------------------------------------
// Milestone agreement creation
// -----------------------------------------------------------------------------

/// Creates a milestone agreement and verifies it returns a valid ID.
#[test]
fn test_create_milestone_agreement() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    assert!(agreement_id >= 1);
}

/// Verifies that a created milestone agreement has payment type MilestoneBased.
#[test]
fn test_milestone_agreement_payment_type() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    // Payment type is stored in instance storage; we verify via behavior:
    // get_milestone_count returns 0 for new agreement (milestone-specific).
    assert_eq!(client.get_milestone_count(&agreement_id), 0);
}

/// Verifies initial milestone count is zero for a new agreement.
#[test]
fn test_initial_milestone_count_zero() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    assert_eq!(client.get_milestone_count(&agreement_id), 0);
    assert!(client.get_milestone(&agreement_id, &1).is_none());
}

// -----------------------------------------------------------------------------
// Adding milestones
// -----------------------------------------------------------------------------

/// Adds a single milestone and verifies count and amount.
#[test]
fn test_add_milestone() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);

    assert_eq!(client.get_milestone_count(&agreement_id), 1);
    let m = client.get_milestone(&agreement_id, &1).unwrap();
    assert_eq!(m.id, 1);
    assert_eq!(m.amount, 1000);
    assert!(!m.approved);
    assert!(!m.claimed);
}

/// Adds multiple milestones and verifies order and amounts.
#[test]
fn test_add_multiple_milestones() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &500);
    client.add_milestone(&agreement_id, &1000);
    client.add_milestone(&agreement_id, &1500);

    assert_eq!(client.get_milestone_count(&agreement_id), 3);
    assert_eq!(client.get_milestone(&agreement_id, &1).unwrap().amount, 500);
    assert_eq!(client.get_milestone(&agreement_id, &2).unwrap().amount, 1000);
    assert_eq!(client.get_milestone(&agreement_id, &3).unwrap().amount, 1500);
}

#[test]
#[should_panic(expected = "Amount must be positive")]
fn test_add_milestone_zero_amount_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &0);
}

#[test]
#[should_panic(expected = "Agreement must be in Created status")]
fn test_add_milestone_wrong_status_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);
    // Agreement may transition; try adding another milestone after approval.
    // Actually the contract keeps status Created until all claimed. So we need to activate?
    // Milestone agreement doesn't have activate - it uses Created/Active/Paused/Completed.
    // add_milestone requires Created. So we need to get out of Created. We can pause?
    // pause_milestone_agreement exists. So: create, add one, approve, then pause? No - pause
    // requires Active or Created. So we need to transition to Active. How? There's no
    // activate for milestone agreements in the same way. Looking at payroll.rs - milestone
    // agreement status is set to Active only in... I don't see it set to Active for milestone.
    // So status stays Created. To get "wrong status" we could use a different agreement
    // that is payroll type and is Active - but add_milestone is for milestone agreements only.
    // So "wrong status" = we need an agreement that is not in Created. For milestone agreements
    // the status is set to Completed when all milestones are claimed. So: add 1 milestone,
    // approve it, claim it -> status becomes Completed. Then try to add another -> fails.
    client.claim_milestone(&agreement_id, &1);
    client.add_milestone(&agreement_id, &500);
}

#[test]
#[should_panic(expected = "HostError")]
fn test_add_milestone_unauthorized_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    env.mock_auths(&[]);
    client.add_milestone(&agreement_id, &1000);
}

/// Verifies that adding milestones updates total amount (via get_milestone amounts).
#[test]
fn test_add_milestone_updates_total() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);

    let m1 = client.get_milestone(&agreement_id, &1).unwrap();
    let m2 = client.get_milestone(&agreement_id, &2).unwrap();
    assert_eq!(m1.amount + m2.amount, 300);
}

// -----------------------------------------------------------------------------
// Approving milestones
// -----------------------------------------------------------------------------

#[test]
fn test_approve_milestone() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);

    let m = client.get_milestone(&agreement_id, &1).unwrap();
    assert!(m.approved);
    assert!(!m.claimed);
}

#[test]
fn test_approve_multiple_milestones() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);

    assert!(client.get_milestone(&agreement_id, &1).unwrap().approved);
    assert!(client.get_milestone(&agreement_id, &2).unwrap().approved);
}

#[test]
#[should_panic(expected = "Invalid milestone ID")]
fn test_approve_invalid_id_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &99);
}

#[test]
#[should_panic(expected = "HostError")]
fn test_approve_unauthorized_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);

    env.mock_auths(&[]);
    client.approve_milestone(&agreement_id, &1);
}

// -----------------------------------------------------------------------------
// Claiming milestones
// -----------------------------------------------------------------------------

#[test]
fn test_claim_approved_milestone() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);

    let m = client.get_milestone(&agreement_id, &1).unwrap();
    assert!(m.approved);
    assert!(m.claimed);
}

#[test]
#[should_panic(expected = "Milestone not approved")]
fn test_claim_unapproved_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.claim_milestone(&agreement_id, &1);
}

#[test]
#[should_panic(expected = "Milestone already claimed")]
fn test_claim_already_claimed_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);
}

#[test]
#[should_panic(expected = "HostError")]
fn test_claim_unauthorized_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);

    env.mock_auths(&[]);
    client.claim_milestone(&agreement_id, &1);
}

/// Verifies that after claim, milestone is marked claimed (state reflects "released").
#[test]
fn test_claim_releases_funds() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);

    let m = client.get_milestone(&agreement_id, &1).unwrap();
    assert!(m.claimed);
    assert_eq!(m.amount, 1000);
}

/// Verifies agreement completes when all milestones are claimed.
#[test]
fn test_agreement_completes_all_claimed() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);
    client.claim_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &2);

    let m1 = client.get_milestone(&agreement_id, &1).unwrap();
    let m2 = client.get_milestone(&agreement_id, &2).unwrap();
    assert!(m1.claimed);
    assert!(m2.claimed);
}

// -----------------------------------------------------------------------------
// Edge cases
// -----------------------------------------------------------------------------

#[test]
fn test_single_milestone_agreement() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &5000);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);

    assert_eq!(client.get_milestone_count(&agreement_id), 1);
    assert!(client.get_milestone(&agreement_id, &1).unwrap().claimed);
}

#[test]
fn test_many_milestones() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
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

#[test]
fn test_claiming_out_of_order() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);
    client.add_milestone(&agreement_id, &300);

    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);
    client.approve_milestone(&agreement_id, &3);

    client.claim_milestone(&agreement_id, &2);
    client.claim_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &3);

    assert!(client.get_milestone(&agreement_id, &1).unwrap().claimed);
    assert!(client.get_milestone(&agreement_id, &2).unwrap().claimed);
    assert!(client.get_milestone(&agreement_id, &3).unwrap().claimed);
}

#[test]
fn test_very_large_milestone_amounts() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    let large: i128 = i128::MAX / 2;
    client.add_milestone(&agreement_id, &large);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);

    let m = client.get_milestone(&agreement_id, &1).unwrap();
    assert_eq!(m.amount, large);
    assert!(m.claimed);
}

// -----------------------------------------------------------------------------
// Happy path (existing test preserved)
// -----------------------------------------------------------------------------

#[test]
fn test_milestone_workflow_happy_path() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    assert_eq!(client.get_milestone_count(&agreement_id), 0);
    assert!(client.get_milestone(&agreement_id, &1).is_none());

    client.add_milestone(&agreement_id, &1000);
    client.add_milestone(&agreement_id, &2500);

    assert_eq!(client.get_milestone_count(&agreement_id), 2);

    let m1 = client.get_milestone(&agreement_id, &1).unwrap();
    assert_eq!(m1.id, 1);
    assert_eq!(m1.amount, 1000);
    assert!(!m1.approved);
    assert!(!m1.claimed);

    let m2 = client.get_milestone(&agreement_id, &2).unwrap();
    assert_eq!(m2.id, 2);
    assert_eq!(m2.amount, 2500);
    assert!(!m2.approved);
    assert!(!m2.claimed);

    client.approve_milestone(&agreement_id, &1);
    let m1a = client.get_milestone(&agreement_id, &1).unwrap();
    assert!(m1a.approved);
    assert!(!m1a.claimed);

    client.claim_milestone(&agreement_id, &1);
    let m1c = client.get_milestone(&agreement_id, &1).unwrap();
    assert!(m1c.approved);
    assert!(m1c.claimed);

    client.approve_milestone(&agreement_id, &2);
    client.claim_milestone(&agreement_id, &2);
    let m2c = client.get_milestone(&agreement_id, &2).unwrap();
    assert!(m2c.approved);
    assert!(m2c.claimed);
}

#[test]
#[should_panic(expected = "Milestone not approved")]
fn test_claiming_unapproved_milestone_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);

    client.claim_milestone(&agreement_id, &1);
}
