#![cfg(test)]
use crate::{PayrollContract, PayrollContractClient};
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
fn test_milestone_workflow_happy_path() {
    let (env, employer, contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    // Initially no milestones
    assert_eq!(client.get_milestone_count(&agreement_id), 0);
    assert!(client.get_milestone(&agreement_id, &1).is_none());

    // Add milestones
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

    // Approve & claim milestone 1
    client.approve_milestone(&agreement_id, &1);
    let m1a = client.get_milestone(&agreement_id, &1).unwrap();
    assert!(m1a.approved);
    assert!(!m1a.claimed);

    client.claim_milestone(&agreement_id, &1);
    let m1c = client.get_milestone(&agreement_id, &1).unwrap();
    assert!(m1c.approved);
    assert!(m1c.claimed);

    // Approve & claim milestone 2
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

    // Claim without approval should fail
    client.claim_milestone(&agreement_id, &1);
}
