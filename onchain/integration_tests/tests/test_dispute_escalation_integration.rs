//! Integration tests for dispute_escalation: full escalation ladder, binding
//! finality, expiry, and concurrent disputes.
#![cfg(test)]

use dispute_escalation::types::{DisputeError, DisputeOutcome, DisputeStatus, EscalationLevel};
use dispute_escalation::{DisputeEscalationContract, DisputeEscalationContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

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

fn advance(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| li.timestamp += seconds);
}

/// Full escalation flow: open → escalate → resolve → appeal → finalise.
#[test]
fn test_escalation_appeal_full_flow() {
    let (_env, client, _owner, admin, user) = setup();
    let id = 201u128;

    client.file_dispute(&user, &id);
    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::Open
    );

    client.escalate_dispute(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Escalated);
    assert_eq!(d.level, EscalationLevel::Level2);

    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::Resolved
    );

    client.appeal_ruling(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Appealed);
    assert_eq!(d.level, EscalationLevel::Level3);

    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Finalised);
    assert_eq!(d.outcome, Some(DisputeOutcome::GrantClaim));
}

/// Wrong caller cannot resolve.
#[test]
fn test_escalation_resolve_unauthorized_integration() {
    let (_env, client, _owner, _admin, user) = setup();
    let id = 202u128;
    client.file_dispute(&user, &id);

    let res = client.try_resolve_dispute(&user, &id, &DisputeOutcome::UpholdPayment);
    assert_eq!(res, Err(Ok(DisputeError::Unauthorized)));
}

/// Expired escalation window preserves dispute state for off-chain inspection.
#[test]
fn test_escalation_deadline_expiry_preserves_open_state_integration() {
    let (env, client, _owner, admin, user) = setup();
    let id = 203u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &60u64);
    client.file_dispute(&user, &id);

    advance(&env, 61);
    let res = client.try_escalate_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::TimeLimitExpired)));

    let dispute = client.get_dispute(&id).unwrap();
    assert_eq!(dispute.status, DisputeStatus::Open);
    assert_eq!(dispute.level, EscalationLevel::Level1);
}

/// Admin-configured windows apply correctly through the full sequence.
#[test]
fn test_escalation_custom_deadlines_apply_to_appeals_integration() {
    let (env, client, _owner, admin, user) = setup();
    let id = 204u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &120u64);
    client.set_level_time_limit(&admin, &EscalationLevel::Level2, &240u64);

    client.file_dispute(&user, &id);
    let opened = client.get_dispute(&id).unwrap();
    assert_eq!(opened.phase_deadline, opened.phase_started_at + 120);

    advance(&env, 30);
    client.escalate_dispute(&user, &id);
    let escalated = client.get_dispute(&id).unwrap();
    assert_eq!(escalated.status, DisputeStatus::Escalated);
    assert_eq!(escalated.level, EscalationLevel::Level2);
    assert_eq!(escalated.phase_deadline, escalated.phase_started_at + 240);

    client.resolve_dispute(&admin, &id, &DisputeOutcome::PartialSettlement);
    let resolved = client.get_dispute(&id).unwrap();
    assert_eq!(resolved.status, DisputeStatus::Resolved);

    advance(&env, 100);
    client.appeal_ruling(&user, &id);
    let appealed = client.get_dispute(&id).unwrap();
    assert_eq!(appealed.status, DisputeStatus::Appealed);
    assert_eq!(appealed.level, EscalationLevel::Level3);
}

/// Level3 resolution is binding — further appeal or resolution is blocked.
#[test]
fn test_binding_finality_at_level3_integration() {
    let (_env, client, _owner, admin, user) = setup();
    let id = 205u128;

    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    client.appeal_ruling(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Finalised);

    // Both further appeal and further resolve must be blocked
    assert_eq!(
        client.try_appeal_ruling(&user, &id),
        Err(Ok(DisputeError::AlreadyFinalised))
    );
    assert_eq!(
        client.try_resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment),
        Err(Ok(DisputeError::AlreadyFinalised))
    );
}

/// Missed escalation window: anyone can expire to unblock payroll.
#[test]
fn test_missed_escalation_window_expires_integration() {
    let (env, client, _owner, admin, user) = setup();
    let id = 206u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &30u64);
    client.file_dispute(&user, &id);

    advance(&env, 31);

    // Escalate blocked by deadline
    assert!(client.try_escalate_dispute(&user, &id).is_err());

    // Expire unblocks the dispute
    client.expire_dispute(&user, &id);
    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::Expired
    );
}

/// Concurrent disputes on different agreement_ids are fully independent.
#[test]
fn test_concurrent_disputes_independent_integration() {
    let (_env, client, _owner, admin, user) = setup();
    let id_a = 207u128;
    let id_b = 208u128;

    client.file_dispute(&user, &id_a);
    client.file_dispute(&user, &id_b);

    // Resolve A → Finalised via Level3
    client.escalate_dispute(&user, &id_a);
    client.resolve_dispute(&admin, &id_a, &DisputeOutcome::UpholdPayment);
    client.appeal_ruling(&user, &id_a);
    client.resolve_dispute(&admin, &id_a, &DisputeOutcome::UpholdPayment);

    // B is still Open and unaffected
    let b = client.get_dispute(&id_b).unwrap();
    assert_eq!(b.status, DisputeStatus::Open);
    assert_eq!(b.level, EscalationLevel::Level1);

    let a = client.get_dispute(&id_a).unwrap();
    assert_eq!(a.status, DisputeStatus::Finalised);
}

/// Cannot double-resolve at integration boundary.
#[test]
fn test_no_double_resolve_integration() {
    let (_env, client, _owner, admin, user) = setup();
    let id = 209u128;

    client.file_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);

    let res = client.try_resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyResolved)));
}
