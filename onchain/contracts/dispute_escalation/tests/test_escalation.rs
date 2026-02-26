#![cfg(test)]

use dispute_escalation::types::{DisputeError, DisputeStatus, EscalationLevel};
use dispute_escalation::{DisputeEscalationContract, DisputeEscalationContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

fn setup_test() -> (
    Env,
    DisputeEscalationContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(DisputeEscalationContract, ());
    let client = DisputeEscalationContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&owner, &admin);

    (env, client, owner, admin, user)
}

#[test]
fn test_dispute_lifecycle() {
    let (_env, client, _owner, admin, user) = setup_test();
    let agreement_id = 100u128;

    // 1. File a dispute
    client.file_dispute(&user, &agreement_id);
    let dispute = client.get_dispute(&agreement_id).unwrap();
    assert_eq!(dispute.status, DisputeStatus::Open);
    assert_eq!(dispute.level, EscalationLevel::Level1);

    // 2. Escalate to Level 2
    client.escalate_dispute(&user, &agreement_id);
    let dispute = client.get_dispute(&agreement_id).unwrap();
    assert_eq!(dispute.status, DisputeStatus::Escalated);
    assert_eq!(dispute.level, EscalationLevel::Level2);

    // 3. Admin resolves Level 2
    client.resolve_dispute(&admin, &agreement_id);
    let dispute = client.get_dispute(&agreement_id).unwrap();
    assert_eq!(dispute.status, DisputeStatus::Resolved);
    assert_eq!(dispute.level, EscalationLevel::Level2);

    // 4. User appeals ruling to Level 3
    client.appeal_ruling(&user, &agreement_id);
    let dispute = client.get_dispute(&agreement_id).unwrap();
    assert_eq!(dispute.status, DisputeStatus::Appealed);
    assert_eq!(dispute.level, EscalationLevel::Level3);

    // 5. Admin resolves Level 3
    client.resolve_dispute(&admin, &agreement_id);
    let dispute = client.get_dispute(&agreement_id).unwrap();
    assert_eq!(dispute.status, DisputeStatus::Resolved);
    assert_eq!(dispute.level, EscalationLevel::Level3);
}

#[test]
fn test_time_limit_enforcement() {
    let (env, client, _owner, _admin, user) = setup_test();
    let agreement_id = 101u128;

    // File Dispute
    client.file_dispute(&user, &agreement_id);

    // Simulate time passing beyond the default 7 days limit (604800 seconds)
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 800000);

    // Escalate should fail due to time limit
    let res = client.try_escalate_dispute(&user, &agreement_id);
    assert!(res.is_err());

    match res {
        Err(Ok(e)) => assert_eq!(e, DisputeError::TimeLimitExpired.into()),
        _ => panic!("Expected TimeLimitExpired error"),
    }
}

#[test]
fn test_unauthorized_resolution() {
    let (_env, client, _owner, _admin, user) = setup_test();
    let agreement_id = 102u128;

    client.file_dispute(&user, &agreement_id);

    // User tries to resolve their own dispute, should fail
    let res = client.try_resolve_dispute(&user, &agreement_id);
    assert!(res.is_err());
    match res {
        Err(Ok(e)) => assert_eq!(e, DisputeError::Unauthorized.into()),
        _ => panic!("Expected Unauthorized error"),
    }
}
