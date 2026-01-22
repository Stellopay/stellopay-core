// test_milestones.rs - Comprehensive test suite for milestone-based payments

#![cfg(test)]
use crate::payroll::{PayrollContract, PayrollContractClient};
use crate::storage::Milestone;
use soroban_sdk::{testutils::Address as _, Address, Env};

fn create_test_env() -> (
    Env,
    Address,
    Address,
    Address,
    PayrollContractClient<'static>,
) {
    let env = Env::default();
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    (env, employer, contributor, token, client)
}

#[test]
fn test_create_milestone_agreement() {
    let (env, employer, contributor, token, client) = create_test_env();

    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    assert_eq!(agreement_id, 1);
    assert_eq!(client.get_milestone_count(&agreement_id), 0);
}

#[test]
fn test_add_milestone_success() {
    let (env, employer, contributor, token, client) = create_test_env();

    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    client.add_milestone(&agreement_id, &1000);

    assert_eq!(client.get_milestone_count(&agreement_id), 1);

    let milestone = client.get_milestone(&agreement_id, &1).unwrap();
    assert_eq!(milestone.id, 1);
    assert_eq!(milestone.amount, 1000);
    assert_eq!(milestone.approved, false);
    assert_eq!(milestone.claimed, false);
}

#[test]
fn test_add_multiple_milestones() {
    let (env, employer, contributor, token, client) = create_test_env();

    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    client.add_milestone(&agreement_id, &1000);
    client.add_milestone(&agreement_id, &2000);
    client.add_milestone(&agreement_id, &1500);

    assert_eq!(client.get_milestone_count(&agreement_id), 3);

    let m1 = client.get_milestone(&agreement_id, &1).unwrap();
    assert_eq!(m1.amount, 1000);

    let m2 = client.get_milestone(&agreement_id, &2).unwrap();
    assert_eq!(m2.amount, 2000);

    let m3 = client.get_milestone(&agreement_id, &3).unwrap();
    assert_eq!(m3.amount, 1500);
}

#[test]
#[should_panic(expected = "Amount must be positive")]
fn test_add_milestone_zero_amount() {
    let (env, employer, contributor, token, client) = create_test_env();

    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    client.add_milestone(&agreement_id, &0);
}

#[test]
#[should_panic(expected = "Amount must be positive")]
fn test_add_milestone_negative_amount() {
    let (env, employer, contributor, token, client) = create_test_env();

    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    client.add_milestone(&agreement_id, &-100);
}

#[test]
fn test_approve_milestone_success() {
    let (env, employer, contributor, token, client) = create_test_env();

    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);

    client.approve_milestone(&agreement_id, &1);

    let milestone = client.get_milestone(&agreement_id, &1).unwrap();
    assert_eq!(milestone.approved, true);
    assert_eq!(milestone.claimed, false);
}

#[test]
#[should_panic(expected = "Invalid milestone ID")]
fn test_approve_invalid_milestone_id() {
    let (env, employer, contributor, token, client) = create_test_env();

    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);

    client.approve_milestone(&agreement_id, &5);
}

#[test]
#[should_panic(expected = "Invalid milestone ID")]
fn test_approve_milestone_id_zero() {
    let (env, employer, contributor, token, client) = create_test_env();

    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);

    client.approve_milestone(&agreement_id, &0);
}

#[test]
#[should_panic(expected = "Milestone already approved")]
fn test_approve_already_approved_milestone() {
    let (env, employer, contributor, token, client) = create_test_env();

    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);

    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &1); // Should panic
}

#[test]
fn test_claim_milestone_success() {
    let (env, employer, contributor, token, client) = create_test_env();

    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);

    client.claim_milestone(&agreement_id, &1);

    let milestone = client.get_milestone(&agreement_id, &1).unwrap();
    assert_eq!(milestone.approved, true);
    assert_eq!(milestone.claimed, true);
}

#[test]
#[should_panic(expected = "Milestone not approved")]
fn test_claim_unapproved_milestone() {
    let (env, employer, contributor, token, client) = create_test_env();

    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);

    client.claim_milestone(&agreement_id, &1); // Should panic
}

#[test]
#[should_panic(expected = "Milestone already claimed")]
fn test_claim_already_claimed_milestone() {
    let (env, employer, contributor, token, client) = create_test_env();

    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);

    client.claim_milestone(&agreement_id, &1); // Should panic
}

#[test]
#[should_panic(expected = "Invalid milestone ID")]
fn test_claim_invalid_milestone_id() {
    let (env, employer, contributor, token, client) = create_test_env();

    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);

    client.claim_milestone(&agreement_id, &10); // Should panic
}

#[test]
fn test_get_milestone_nonexistent() {
    let (env, employer, contributor, token, client) = create_test_env();

    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    let result = client.get_milestone(&agreement_id, &1);
    assert!(result.is_none());
}

#[test]
fn test_milestone_workflow_complete() {
    let (env, employer, contributor, token, client) = create_test_env();

    env.mock_all_auths();

    // Create agreement
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    // Add 3 milestones
    client.add_milestone(&agreement_id, &1000);
    client.add_milestone(&agreement_id, &2000);
    client.add_milestone(&agreement_id, &1500);

    assert_eq!(client.get_milestone_count(&agreement_id), 3);

    // Approve and claim first milestone
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);

    let m1 = client.get_milestone(&agreement_id, &1).unwrap();
    assert!(m1.approved && m1.claimed);

    // Approve and claim third milestone (out of order)
    client.approve_milestone(&agreement_id, &3);
    client.claim_milestone(&agreement_id, &3);

    let m3 = client.get_milestone(&agreement_id, &3).unwrap();
    assert!(m3.approved && m3.claimed);

    // Second milestone still unclaimed
    let m2 = client.get_milestone(&agreement_id, &2).unwrap();
    assert!(!m2.approved && !m2.claimed);

    // Complete second milestone
    client.approve_milestone(&agreement_id, &2);
    client.claim_milestone(&agreement_id, &2);

    // All milestones should be claimed now
    for i in 1..=3 {
        let m = client.get_milestone(&agreement_id, &i).unwrap();
        assert!(m.approved && m.claimed);
    }
}

#[test]
fn test_milestone_can_claim_helper() {
    let milestone_unclaimed = Milestone {
        id: 1,
        amount: 1000,
        approved: true,
        claimed: false,
    };
    assert!(milestone_unclaimed.can_claim());

    let milestone_claimed = Milestone {
        id: 1,
        amount: 1000,
        approved: true,
        claimed: true,
    };
    assert!(!milestone_claimed.can_claim());

    let milestone_unapproved = Milestone {
        id: 1,
        amount: 1000,
        approved: false,
        claimed: false,
    };
    assert!(!milestone_unapproved.can_claim());
}

#[test]
fn test_multiple_agreements_isolation() {
    let (env, employer, contributor, token, client) = create_test_env();
    let employer2 = Address::generate(&env);
    let contributor2 = Address::generate(&env);

    env.mock_all_auths();

    // Create two separate agreements
    let agreement1 = client.create_milestone_agreement(&employer, &contributor, &token);
    let agreement2 = client.create_milestone_agreement(&employer2, &contributor2, &token);

    // Add milestones to first agreement
    client.add_milestone(&agreement1, &1000);
    client.add_milestone(&agreement1, &2000);

    // Add milestone to second agreement
    client.add_milestone(&agreement2, &3000);

    // Verify isolation
    assert_eq!(client.get_milestone_count(&agreement1), 2);
    assert_eq!(client.get_milestone_count(&agreement2), 1);

    let m1_a1 = client.get_milestone(&agreement1, &1).unwrap();
    assert_eq!(m1_a1.amount, 1000);

    let m1_a2 = client.get_milestone(&agreement2, &1).unwrap();
    assert_eq!(m1_a2.amount, 3000);
}

#[test]
fn test_edge_case_many_milestones() {
    let (env, employer, contributor, token, client) = create_test_env();

    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    // Add 50 milestones
    for i in 1..=50 {
        client.add_milestone(&agreement_id, &(i * 100));
    }

    assert_eq!(client.get_milestone_count(&agreement_id), 50);

    // Verify random milestones
    let m10 = client.get_milestone(&agreement_id, &10).unwrap();
    assert_eq!(m10.amount, 1000);

    let m50 = client.get_milestone(&agreement_id, &50).unwrap();
    assert_eq!(m50.amount, 5000);
}
