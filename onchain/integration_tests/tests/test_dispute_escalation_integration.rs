//! Integration tests for dispute_escalation contract: file -> escalate -> resolve -> appeal -> resolve.
#![cfg(test)]

use dispute_escalation::types::{DisputeError, DisputeStatus, EscalationLevel};
use dispute_escalation::{DisputeEscalationContract, DisputeEscalationContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (
    Env,
    DisputeEscalationContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(DisputeEscalationContract, ());
    let client = DisputeEscalationContractClient::new(&env, &id);
    let owner = Address::generate(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&owner, &admin);
    (env, client, owner, admin, user)
}

/// Full escalation flow: open -> escalate -> admin resolve -> appeal -> admin resolve.
#[test]
fn test_escalation_appeal_full_flow() {
    let (_env, client, _owner, admin, user) = setup();
    let agreement_id = 201u128;

    client.file_dispute(&user, &agreement_id);
    assert_eq!(
        client.get_dispute(&agreement_id).unwrap().status,
        DisputeStatus::Open
    );

    client.escalate_dispute(&user, &agreement_id);
    let d = client.get_dispute(&agreement_id).unwrap();
    assert_eq!(d.status, DisputeStatus::Escalated);
    assert_eq!(d.level, EscalationLevel::Level2);

    client.resolve_dispute(&admin, &agreement_id);
    assert_eq!(
        client.get_dispute(&agreement_id).unwrap().status,
        DisputeStatus::Resolved
    );

    client.appeal_ruling(&user, &agreement_id);
    let d = client.get_dispute(&agreement_id).unwrap();
    assert_eq!(d.status, DisputeStatus::Appealed);
    assert_eq!(d.level, EscalationLevel::Level3);

    client.resolve_dispute(&admin, &agreement_id);
    assert_eq!(
        client.get_dispute(&agreement_id).unwrap().status,
        DisputeStatus::Resolved
    );
}

/// Wrong caller cannot resolve (mirrors contract unit test at integration boundary).
#[test]
fn test_escalation_resolve_unauthorized_integration() {
    let (_env, client, _owner, _admin, user) = setup();
    let agreement_id = 202u128;
    client.file_dispute(&user, &agreement_id);

    let res = client.try_resolve_dispute(&user, &agreement_id);
    assert!(res.is_err());
    match res {
        Err(Ok(e)) => assert_eq!(e, DisputeError::Unauthorized.into()),
        _ => panic!("expected Unauthorized"),
    }
}
