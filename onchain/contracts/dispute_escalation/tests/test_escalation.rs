#![cfg(test)]

use dispute_escalation::types::{DisputeError, DisputeOutcome, DisputeStatus, EscalationLevel};
use dispute_escalation::{DisputeEscalationContract, DisputeEscalationContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

// ─── Fixtures ────────────────────────────────────────────────────────────────

fn setup() -> (
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

fn advance(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| li.timestamp += seconds);
}

// ─── Lifecycle tests ─────────────────────────────────────────────────────────

#[test]
fn test_full_dispute_lifecycle_to_level3_finalised() {
    let (_env, client, _owner, admin, user) = setup();
    let id = 100u128;

    // 1. File
    client.file_dispute(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Open);
    assert_eq!(d.level, EscalationLevel::Level1);
    assert!(d.outcome.is_none());

    // 2. Escalate → Level2
    client.escalate_dispute(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Escalated);
    assert_eq!(d.level, EscalationLevel::Level2);

    // 3. Admin resolves Level2 → Resolved (appeal window opens)
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Resolved);
    assert_eq!(d.outcome, Some(DisputeOutcome::UpholdPayment));

    // 4. Appeal → Level3
    client.appeal_ruling(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Appealed);
    assert_eq!(d.level, EscalationLevel::Level3);
    assert!(d.outcome.is_none()); // outcome cleared for re-review

    // 5. Admin resolves Level3 → Finalised (terminal)
    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Finalised);
    assert_eq!(d.outcome, Some(DisputeOutcome::GrantClaim));
    assert_eq!(d.level, EscalationLevel::Level3);
}

#[test]
fn test_resolve_level1_directly() {
    let (_env, client, _owner, admin, user) = setup();
    let id = 101u128;

    client.file_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::PartialSettlement);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Resolved);
    assert_eq!(d.level, EscalationLevel::Level1);
    assert_eq!(d.outcome, Some(DisputeOutcome::PartialSettlement));
}

// ─── Time limit tests ─────────────────────────────────────────────────────────

#[test]
fn test_escalate_fails_after_deadline() {
    let (env, client, _owner, _admin, user) = setup();
    let id = 102u128;

    client.file_dispute(&user, &id);
    advance(&env, 604_801); // past default 7-day limit

    let res = client.try_escalate_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::TimeLimitExpired)));
}

#[test]
fn test_appeal_fails_after_appeal_window() {
    let (env, client, _owner, admin, user) = setup();
    let id = 103u128;

    client.file_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);

    // Appeal window is 3 days (259_200 s)
    advance(&env, 259_201);

    let res = client.try_appeal_ruling(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::TimeLimitExpired)));
}

#[test]
fn test_custom_time_limit_applied() {
    let (env, client, _owner, admin, user) = setup();
    let id = 104u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &60u64);
    client.file_dispute(&user, &id);

    let opened = client.get_dispute(&id).unwrap();
    assert_eq!(opened.phase_deadline, opened.phase_started_at + 60);

    advance(&env, 30);
    // Still within window — escalate should succeed
    client.escalate_dispute(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.level, EscalationLevel::Level2);
}

// ─── Access control tests ─────────────────────────────────────────────────────

#[test]
fn test_non_admin_cannot_resolve() {
    let (_env, client, _owner, _admin, user) = setup();
    let id = 105u128;

    client.file_dispute(&user, &id);
    let res = client.try_resolve_dispute(&user, &id, &DisputeOutcome::UpholdPayment);
    assert_eq!(res, Err(Ok(DisputeError::Unauthorized)));
}

#[test]
fn test_non_admin_cannot_set_time_limit() {
    let (_env, client, _owner, _admin, user) = setup();
    let res = client.try_set_level_time_limit(&user, &EscalationLevel::Level1, &120u64);
    assert_eq!(res, Err(Ok(DisputeError::Unauthorized)));
}

// ─── Double-resolve / finality tests ─────────────────────────────────────────

#[test]
fn test_cannot_double_resolve() {
    let (_env, client, _owner, admin, user) = setup();
    let id = 106u128;

    client.file_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);

    // Second resolve must fail
    let res = client.try_resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyResolved)));
}

#[test]
fn test_cannot_resolve_finalised_dispute() {
    let (_env, client, _owner, admin, user) = setup();
    let id = 107u128;

    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    client.appeal_ruling(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim); // → Finalised

    let res = client.try_resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyFinalised)));
}

#[test]
fn test_cannot_appeal_finalised_dispute() {
    let (_env, client, _owner, admin, user) = setup();
    let id = 108u128;

    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    client.appeal_ruling(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim); // → Finalised

    let res = client.try_appeal_ruling(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyFinalised)));
}

#[test]
fn test_cannot_escalate_beyond_level3() {
    let (_env, client, _owner, admin, user) = setup();
    let id = 109u128;

    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id); // → Level2
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    client.appeal_ruling(&user, &id); // → Level3

    // Escalate at Level3 must fail
    let res = client.try_escalate_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::MaxEscalationReached)));
}

#[test]
fn test_cannot_appeal_beyond_level3() {
    let (_env, client, _owner, admin, user) = setup();
    let id = 110u128;

    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    client.appeal_ruling(&user, &id); // → Level3

    // Resolve Level3 to Finalised, then try to appeal
    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);
    let res = client.try_appeal_ruling(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyFinalised)));
}

// ─── Expire dispute tests ─────────────────────────────────────────────────────

#[test]
fn test_expire_dispute_after_deadline() {
    let (env, client, _owner, _admin, user) = setup();
    let id = 111u128;

    client.file_dispute(&user, &id);
    advance(&env, 604_801);

    client.expire_dispute(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Expired);
}

#[test]
fn test_cannot_expire_before_deadline() {
    let (_env, client, _owner, _admin, user) = setup();
    let id = 112u128;

    client.file_dispute(&user, &id);

    let res = client.try_expire_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::DeadlineNotPassed)));
}

#[test]
fn test_cannot_expire_already_terminal_dispute() {
    let (env, client, _owner, admin, user) = setup();
    let id = 113u128;

    // Expire once
    client.file_dispute(&user, &id);
    advance(&env, 604_801);
    client.expire_dispute(&user, &id);

    // Second expire must fail
    let res = client.try_expire_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyTerminal)));
}

#[test]
fn test_cannot_resolve_expired_dispute() {
    let (env, client, _owner, admin, user) = setup();
    let id = 114u128;

    client.file_dispute(&user, &id);
    advance(&env, 604_801);
    client.expire_dispute(&user, &id);

    let res = client.try_resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyTerminal)));
}

#[test]
fn test_cannot_escalate_expired_dispute() {
    let (env, client, _owner, _admin, user) = setup();
    let id = 115u128;

    client.file_dispute(&user, &id);
    advance(&env, 604_801);
    client.expire_dispute(&user, &id);

    let res = client.try_escalate_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyTerminal)));
}

// ─── Duplicate dispute tests ──────────────────────────────────────────────────

#[test]
fn test_cannot_file_duplicate_dispute() {
    let (_env, client, _owner, _admin, user) = setup();
    let id = 116u128;

    client.file_dispute(&user, &id);

    let res = client.try_file_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::InvalidTransition)));
}

// ─── Appeal on non-resolved state ────────────────────────────────────────────

#[test]
fn test_cannot_appeal_open_dispute() {
    let (_env, client, _owner, _admin, user) = setup();
    let id = 117u128;

    client.file_dispute(&user, &id);

    let res = client.try_appeal_ruling(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::InvalidTransition)));
}

// ─── Concurrent disputes ──────────────────────────────────────────────────────

#[test]
fn test_concurrent_disputes_are_independent() {
    let (_env, client, _owner, admin, user) = setup();
    let id1 = 118u128;
    let id2 = 119u128;

    client.file_dispute(&user, &id1);
    client.file_dispute(&user, &id2);

    client.resolve_dispute(&admin, &id1, &DisputeOutcome::UpholdPayment);

    // id2 should still be Open
    assert_eq!(
        client.get_dispute(&id2).unwrap().status,
        DisputeStatus::Open
    );
    // id1 should be Resolved
    assert_eq!(
        client.get_dispute(&id1).unwrap().status,
        DisputeStatus::Resolved
    );
}

// ─── Nonexistent dispute ──────────────────────────────────────────────────────

#[test]
fn test_get_nonexistent_dispute_returns_none() {
    let (_env, client, _owner, _admin, _user) = setup();
    assert!(client.get_dispute(&999u128).is_none());
}

#[test]
fn test_escalate_nonexistent_dispute() {
    let (_env, client, _owner, _admin, user) = setup();
    let res = client.try_escalate_dispute(&user, &999u128);
    assert_eq!(res, Err(Ok(DisputeError::DisputeNotFound)));
}
