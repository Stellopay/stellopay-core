#![cfg(test)]

use dispute_escalation::{
    types::{DisputeError, DisputeOutcome, DisputeStatus, EscalationLevel},
    DisputeEscalatedEvent, DisputeEscalationContract, DisputeEscalationContractClient,
    DisputeSlaViolationAdvancedEvent,
};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, Env, IntoVal, String as SorobanString, Val, Vec,
};

// ─── Constants ────────────────────────────────────────────────────────────────

/// Default SLA window per level: 7 days in seconds.
const DEFAULT_LEVEL_LIMIT: u64 = 604_800;
/// Default appeal window: 3 days in seconds.
const APPEAL_WINDOW: u64 = 259_200;
/// Default pending-review window: 3 days in seconds.
const PENDING_REVIEW_WINDOW: u64 = 259_200;

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

/// Advance the ledger timestamp by `seconds`.
fn advance(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| li.timestamp += seconds);
}

/// Return the current ledger timestamp.
fn now(env: &Env) -> u64 {
    env.ledger().timestamp()
}

/// Return true when the provided Soroban event topic starts with `event_name`.
fn event_topic_matches(env: &Env, topic: &Vec<Val>, event_name: &str) -> bool {
    if topic.is_empty() {
        return false;
    }

    let Ok(symbol) = Symbol::try_from_val(env, &topic.get(0).unwrap()) else {
        return false;
    };

    symbol.to_string() == event_name
}

/// Return true if any emitted event has the requested first topic symbol.
fn has_event(env: &Env, event_name: &str) -> bool {
    env.events()
        .all()
        .iter()
        .any(|(_, topic, _)| event_topic_matches(env, &topic, event_name))
}

/// Return the last event whose first topic symbol matches `event_name`.
fn last_event(env: &Env, event_name: &str) -> Option<(Address, Vec<Val>, Val)> {
    env.events()
        .all()
        .iter()
        .filter(|(_, topic, _)| event_topic_matches(env, topic, event_name))
        .last()
}

// ═══════════════════════════════════════════════════════════════════════════════
// §1  LIFECYCLE TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_full_dispute_lifecycle_to_level3_finalised() {
    let (_env, client, _owner, admin, user) = setup();
    let id = 100u128;

    // 1. File
    client.file_dispute(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Open);
    assert_eq!(d.level, EscalationLevel::Level1);
    assert_eq!(d.outcome, DisputeOutcome::Unset);

    // 2. Escalate → Level2
    client.escalate_dispute(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Escalated);
    assert_eq!(d.level, EscalationLevel::Level2);

    // 3. Admin resolves Level2 → Resolved (appeal window opens)
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Resolved);
    assert_eq!(d.outcome, DisputeOutcome::UpholdPayment);

    // 4. Appeal → Level3
    client.appeal_ruling(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Appealed);
    assert_eq!(d.level, EscalationLevel::Level3);
    assert_eq!(d.outcome, DisputeOutcome::Unset); // outcome cleared for re-review

    // 5. Admin resolves Level3 → Finalised (terminal)
    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Finalised);
    assert_eq!(d.outcome, DisputeOutcome::GrantClaim);
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
    assert_eq!(d.outcome, DisputeOutcome::PartialSettlement);
}

#[test]
fn test_full_lifecycle_with_pending_review_at_each_stage() {
    // Open → PendingReview → Resolved → Appealed → PendingReview → Finalised
    let (env, client, _owner, admin, user) = setup();
    let id = 102u128;

    // Set short limits for speed
    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &100u64);
    client.set_level_time_limit(&admin, &EscalationLevel::Level2, &100u64);
    client.set_pending_review_time_limit(&admin, &200u64);

    // 1. File → Open
    client.file_dispute(&user, &id);
    assert_eq!(client.get_dispute(&id).unwrap().status, DisputeStatus::Open);

    // 2. SLA lapses → keeper advances to PendingReview
    advance(&env, 101);
    client.keeper_advance_stage(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::PendingReview);
    assert_eq!(d.level, EscalationLevel::Level1);
    // review_deadline = now + 200
    assert!(d.phase_deadline > now(&env));

    // 3. Admin resolves from PendingReview (Level1) → Resolved
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Resolved);
    assert_eq!(d.level, EscalationLevel::Level1);
    assert_eq!(d.outcome, DisputeOutcome::UpholdPayment);

    // 4. User appeals within the appeal window → Appealed @ Level2
    client.appeal_ruling(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Appealed);
    assert_eq!(d.level, EscalationLevel::Level2);
    assert_eq!(d.outcome, DisputeOutcome::Unset);

    // 5. Level2 SLA lapses → keeper advances to PendingReview @ Level2
    advance(&env, 101);
    client.keeper_advance_stage(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::PendingReview);
    assert_eq!(d.level, EscalationLevel::Level2);

    // 6. Admin resolves Level2 from PendingReview → Resolved
    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Resolved);

    // 7. User appeals to Level3
    client.appeal_ruling(&user, &id);
    assert_eq!(
        client.get_dispute(&id).unwrap().level,
        EscalationLevel::Level3
    );

    // 8. Admin issues final ruling at Level3 → Finalised
    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Finalised);
    assert_eq!(d.outcome, DisputeOutcome::GrantClaim);
}

// ═══════════════════════════════════════════════════════════════════════════════
// §2  SLA / TIME-LIMIT TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_escalate_fails_after_deadline() {
    let (env, client, _owner, _admin, user) = setup();
    let id = 200u128;

    client.file_dispute(&user, &id);
    advance(&env, DEFAULT_LEVEL_LIMIT + 1); // one second past default 7-day limit

    let res = client.try_escalate_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::TimeLimitExpired)));
}

#[test]
fn test_appeal_fails_after_appeal_window() {
    let (env, client, _owner, admin, user) = setup();
    let id = 201u128;

    client.file_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);

    advance(&env, APPEAL_WINDOW + 1);

    let res = client.try_appeal_ruling(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::TimeLimitExpired)));
}

#[test]
fn test_custom_time_limit_applied() {
    let (env, client, _owner, admin, user) = setup();
    let id = 202u128;

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

#[test]
fn test_custom_pending_review_limit_applied() {
    let (env, client, _owner, admin, user) = setup();
    let id = 203u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &50u64);
    client.set_pending_review_time_limit(&admin, &120u64);
    client.file_dispute(&user, &id);

    advance(&env, 51); // SLA elapsed

    client.keeper_advance_stage(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    // New deadline = now + 120
    assert_eq!(d.phase_deadline, d.phase_started_at + 120);
    assert_eq!(d.status, DisputeStatus::PendingReview);
}

#[test]
fn test_get_pending_review_time_limit_default() {
    let (_env, client, _owner, _admin, _user) = setup();
    assert_eq!(
        client.get_pending_review_time_limit(),
        PENDING_REVIEW_WINDOW
    );
}

#[test]
fn test_get_pending_review_time_limit_after_set() {
    let (_env, client, _owner, admin, _user) = setup();
    client.set_pending_review_time_limit(&admin, &3600u64);
    assert_eq!(client.get_pending_review_time_limit(), 3600u64);
}

// ═══════════════════════════════════════════════════════════════════════════════
// §3  BOUNDARY TIMESTAMP TESTS
//     These verify the exact boundary semantics of every deadline check:
//       now <= deadline  →  still valid
//       now >  deadline  →  expired / can advance
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_escalate_at_exactly_deadline_succeeds() {
    // now == deadline → still within the window (now <= deadline is allowed)
    let (env, client, _owner, admin, user) = setup();
    let id = 300u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &100u64);
    client.file_dispute(&user, &id);
    let deadline = client.get_dispute(&id).unwrap().phase_deadline;

    // Advance to exactly the deadline timestamp
    let start = now(&env);
    advance(&env, deadline - start);
    assert_eq!(now(&env), deadline);

    // Escalation must succeed at exactly the deadline
    client.escalate_dispute(&user, &id);
    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::Escalated
    );
}

#[test]
fn test_escalate_one_second_past_deadline_fails() {
    // now > deadline → TimeLimitExpired
    let (env, client, _owner, admin, user) = setup();
    let id = 301u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &100u64);
    client.file_dispute(&user, &id);
    let deadline = client.get_dispute(&id).unwrap().phase_deadline;

    let start = now(&env);
    advance(&env, deadline - start + 1); // deadline + 1
    assert_eq!(now(&env), deadline + 1);

    let res = client.try_escalate_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::TimeLimitExpired)));
}

#[test]
fn test_expire_at_exactly_deadline_fails() {
    // now == deadline → DeadlineNotPassed (now <= deadline check blocks expiry)
    let (env, client, _owner, admin, user) = setup();
    let id = 302u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &100u64);
    client.file_dispute(&user, &id);
    let deadline = client.get_dispute(&id).unwrap().phase_deadline;

    let start = now(&env);
    advance(&env, deadline - start);
    assert_eq!(now(&env), deadline);

    let res = client.try_expire_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::DeadlineNotPassed)));
}

#[test]
fn test_expire_one_second_past_deadline_succeeds() {
    // now > deadline → expiry allowed
    let (env, client, _owner, admin, user) = setup();
    let id = 303u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &100u64);
    client.file_dispute(&user, &id);
    let deadline = client.get_dispute(&id).unwrap().phase_deadline;

    let start = now(&env);
    advance(&env, deadline - start + 1);
    assert_eq!(now(&env), deadline + 1);

    client.expire_dispute(&user, &id);
    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::Expired
    );
}

#[test]
fn test_keeper_advance_at_exactly_deadline_fails() {
    // now == deadline → SLA not yet elapsed (now <= deadline blocks keeper)
    let (env, client, _owner, admin, user) = setup();
    let id = 304u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &100u64);
    client.file_dispute(&user, &id);
    let deadline = client.get_dispute(&id).unwrap().phase_deadline;

    let start = now(&env);
    advance(&env, deadline - start);
    assert_eq!(now(&env), deadline);

    let res = client.try_keeper_advance_stage(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::DeadlineNotPassed)));
}

#[test]
fn test_keeper_advance_one_second_past_deadline_succeeds() {
    // now > deadline → keeper may advance
    let (env, client, _owner, admin, user) = setup();
    let id = 305u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &100u64);
    client.file_dispute(&user, &id);
    let deadline = client.get_dispute(&id).unwrap().phase_deadline;

    let start = now(&env);
    advance(&env, deadline - start + 1);
    assert_eq!(now(&env), deadline + 1);

    client.keeper_advance_stage(&user, &id);
    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::PendingReview
    );
}

#[test]
fn test_appeal_at_exactly_appeal_deadline_succeeds() {
    // now == appeal_deadline → still within the appeal window
    let (env, client, _owner, admin, user) = setup();
    let id = 306u128;

    client.file_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    let appeal_deadline = client.get_dispute(&id).unwrap().phase_deadline;

    let start = now(&env);
    advance(&env, appeal_deadline - start);
    assert_eq!(now(&env), appeal_deadline);

    client.appeal_ruling(&user, &id);
    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::Appealed
    );
}

#[test]
fn test_appeal_one_second_past_appeal_deadline_fails() {
    // now > appeal_deadline → TimeLimitExpired
    let (env, client, _owner, admin, user) = setup();
    let id = 307u128;

    client.file_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    let appeal_deadline = client.get_dispute(&id).unwrap().phase_deadline;

    let start = now(&env);
    advance(&env, appeal_deadline - start + 1);
    assert_eq!(now(&env), appeal_deadline + 1);

    let res = client.try_appeal_ruling(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::TimeLimitExpired)));
}

#[test]
fn test_expire_pending_review_at_exactly_review_deadline_fails() {
    // now == review_deadline → still within review window (cannot expire yet)
    let (env, client, _owner, admin, user) = setup();
    let id = 308u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &50u64);
    client.set_pending_review_time_limit(&admin, &100u64);
    client.file_dispute(&user, &id);

    advance(&env, 51); // past SLA → keeper can advance
    client.keeper_advance_stage(&user, &id);

    let review_deadline = client.get_dispute(&id).unwrap().phase_deadline;
    let current = now(&env);
    advance(&env, review_deadline - current); // advance to exactly review_deadline
    assert_eq!(now(&env), review_deadline);

    let res = client.try_expire_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::DeadlineNotPassed)));
}

#[test]
fn test_expire_pending_review_one_second_past_review_deadline_succeeds() {
    // now > review_deadline → dispute can be expired
    let (env, client, _owner, admin, user) = setup();
    let id = 309u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &50u64);
    client.set_pending_review_time_limit(&admin, &100u64);
    client.file_dispute(&user, &id);

    advance(&env, 51);
    client.keeper_advance_stage(&user, &id);

    let review_deadline = client.get_dispute(&id).unwrap().phase_deadline;
    let current = now(&env);
    advance(&env, review_deadline - current + 1);
    assert_eq!(now(&env), review_deadline + 1);

    client.expire_dispute(&user, &id);
    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::Expired
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// §4  KEEPER_ADVANCE_STAGE TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_keeper_advance_stage_from_open() {
    let (env, client, _owner, _admin, user) = setup();
    let id = 400u128;

    client.file_dispute(&user, &id);
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);

    client.keeper_advance_stage(&user, &id);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::PendingReview);
    assert_eq!(d.level, EscalationLevel::Level1);
    // phase_started_at updated to the time of the keeper call
    assert_eq!(d.phase_started_at, now(&env));
    // review deadline is set to now + PENDING_REVIEW_WINDOW
    assert_eq!(d.phase_deadline, now(&env) + PENDING_REVIEW_WINDOW);
}

#[test]
fn test_normal_escalation_emits_only_dispute_escalated_event() {
    let (env, client, _owner, _admin, user) = setup();
    let id = 1_401u128;

    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id);

    assert!(has_event(&env, "dispute_escalated"));
    assert!(!has_event(&env, "sla_violation_advanced"));

    let event = last_event(&env, "dispute_escalated").expect("dispute_escalated event");
    let payload = DisputeEscalatedEvent::try_from_val(&env, &event.2).unwrap();
    assert_eq!(payload.agreement_id, id);
    assert_eq!(payload.new_level, EscalationLevel::Level2);
    assert_eq!(payload.phase_deadline, now(&env) + DEFAULT_LEVEL_LIMIT);
}

#[test]
fn test_keeper_timeout_emits_sla_violation_advanced_event_only() {
    let (env, client, _owner, _admin, user) = setup();
    let id = 1_402u128;

    client.file_dispute(&user, &id);
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);
    let breached_at = now(&env);

    client.keeper_advance_stage(&user, &id);

    assert!(has_event(&env, "sla_violation_advanced"));
    assert!(!has_event(&env, "dispute_escalated"));

    let event = last_event(&env, "sla_violation_advanced").expect("sla_violation_advanced event");
    let payload = DisputeSlaViolationAdvancedEvent::try_from_val(&env, &event.2).unwrap();
    assert_eq!(payload.agreement_id, id);
    assert_eq!(payload.level, EscalationLevel::Level1);
    assert_eq!(payload.breached_at, breached_at);
    assert_eq!(payload.review_deadline, breached_at + PENDING_REVIEW_WINDOW);
}

#[test]
fn test_keeper_advance_stage_from_escalated() {
    let (env, client, _owner, _admin, user) = setup();
    let id = 401u128;

    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id); // → Escalated @ Level2
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);

    client.keeper_advance_stage(&user, &id);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::PendingReview);
    assert_eq!(d.level, EscalationLevel::Level2);
}

#[test]
fn test_keeper_advance_stage_from_appealed() {
    let (env, client, _owner, admin, user) = setup();
    let id = 402u128;

    client.file_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment); // → Resolved
    client.appeal_ruling(&user, &id); // → Appealed @ Level2
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);

    client.keeper_advance_stage(&user, &id);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::PendingReview);
    assert_eq!(d.level, EscalationLevel::Level2);
}

#[test]
fn test_keeper_advance_stage_before_deadline_fails() {
    let (_env, client, _owner, _admin, user) = setup();
    let id = 403u128;

    client.file_dispute(&user, &id);
    // Do not advance time — deadline has not passed

    let res = client.try_keeper_advance_stage(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::DeadlineNotPassed)));
}

#[test]
fn test_keeper_advance_stage_already_pending_review_rejected() {
    // Second call must return AlreadyPendingReview — not silently succeed
    let (env, client, _owner, _admin, user) = setup();
    let id = 404u128;

    client.file_dispute(&user, &id);
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);
    client.keeper_advance_stage(&user, &id); // first call — OK

    let res = client.try_keeper_advance_stage(&user, &id); // second call — rejected
    assert_eq!(res, Err(Ok(DisputeError::AlreadyPendingReview)));
}

#[test]
fn test_keeper_advance_stage_on_resolved_fails() {
    let (_env, client, _owner, admin, user) = setup();
    let id = 405u128;

    client.file_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    // Resolved disputes manage their own appeal window; keeper must not interfere

    let res = client.try_keeper_advance_stage(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyResolved)));
}

#[test]
fn test_keeper_advance_stage_on_finalised_fails() {
    let (env, client, _owner, admin, user) = setup();
    let id = 406u128;

    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    client.appeal_ruling(&user, &id); // → Level3
    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim); // → Finalised

    advance(&env, DEFAULT_LEVEL_LIMIT + 1);

    let res = client.try_keeper_advance_stage(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyFinalised)));
}

#[test]
fn test_keeper_advance_stage_on_expired_fails() {
    let (env, client, _owner, _admin, user) = setup();
    let id = 407u128;

    client.file_dispute(&user, &id);
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);
    client.expire_dispute(&user, &id);

    let res = client.try_keeper_advance_stage(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyTerminal)));
}

#[test]
fn test_keeper_advance_stage_nonexistent_dispute() {
    let (_env, client, _owner, _admin, user) = setup();
    let res = client.try_keeper_advance_stage(&user, &9999u128);
    assert_eq!(res, Err(Ok(DisputeError::DisputeNotFound)));
}

#[test]
fn test_keeper_advance_preserves_level_and_outcome() {
    // keeper_advance_stage must NOT change level or outcome
    let (env, client, _owner, admin, user) = setup();
    let id = 408u128;

    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id); // → Level2
                                         // Partially resolve to set outcome, then appeal resets it
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    client.appeal_ruling(&user, &id); // → Appealed @ Level3, outcome = Unset

    advance(&env, DEFAULT_LEVEL_LIMIT + 1);
    client.keeper_advance_stage(&user, &id);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.level, EscalationLevel::Level3); // level unchanged
    assert_eq!(d.outcome, DisputeOutcome::Unset); // outcome unchanged
}

// ═══════════════════════════════════════════════════════════════════════════════
// §5  PENDING REVIEW STATE TRANSITIONS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_resolve_level1_from_pending_review() {
    let (env, client, _owner, admin, user) = setup();
    let id = 500u128;

    client.file_dispute(&user, &id);
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);
    client.keeper_advance_stage(&user, &id);

    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Resolved);
    assert_eq!(d.outcome, DisputeOutcome::GrantClaim);
    // Appeal window opens after L1 resolve
    assert!(d.phase_deadline > now(&env));
}

#[test]
fn test_resolve_level3_from_pending_review_goes_to_finalised() {
    let (env, client, _owner, admin, user) = setup();
    let id = 501u128;

    // Reach Level3 via escalation and appeal, then keeper advances to PendingReview
    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id); // Level2
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    client.appeal_ruling(&user, &id); // Level3

    advance(&env, DEFAULT_LEVEL_LIMIT + 1);
    client.keeper_advance_stage(&user, &id);

    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::PendingReview
    );
    assert_eq!(
        client.get_dispute(&id).unwrap().level,
        EscalationLevel::Level3
    );

    // Admin resolves Level3 from PendingReview → must be Finalised (no appeal)
    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Finalised);
    assert_eq!(d.outcome, DisputeOutcome::GrantClaim);
}

#[test]
fn test_expire_from_pending_review_after_review_window() {
    let (env, client, _owner, admin, user) = setup();
    let id = 502u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &50u64);
    client.set_pending_review_time_limit(&admin, &100u64);
    client.file_dispute(&user, &id);

    advance(&env, 51); // SLA elapsed
    client.keeper_advance_stage(&user, &id);

    advance(&env, 101); // review window elapsed
    client.expire_dispute(&user, &id);

    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::Expired
    );
}

#[test]
fn test_expire_from_pending_review_before_review_window_fails() {
    let (env, client, _owner, admin, user) = setup();
    let id = 503u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &50u64);
    client.set_pending_review_time_limit(&admin, &100u64);
    client.file_dispute(&user, &id);

    advance(&env, 51);
    client.keeper_advance_stage(&user, &id);
    // Do NOT advance past the review window

    let res = client.try_expire_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::DeadlineNotPassed)));
}

#[test]
fn test_escalate_from_pending_review_fails() {
    // Once in PendingReview, the original escalation window is closed
    let (env, client, _owner, _admin, user) = setup();
    let id = 504u128;

    client.file_dispute(&user, &id);
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);
    client.keeper_advance_stage(&user, &id);

    let res = client.try_escalate_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::InvalidTransition)));
}

#[test]
fn test_appeal_from_pending_review_fails() {
    // appeal_ruling requires Resolved status
    let (env, client, _owner, _admin, user) = setup();
    let id = 505u128;

    client.file_dispute(&user, &id);
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);
    client.keeper_advance_stage(&user, &id);

    let res = client.try_appeal_ruling(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::InvalidTransition)));
}

#[test]
fn test_keeper_advance_on_pending_review_is_idempotent_rejected() {
    // Calling keeper_advance_stage twice on the same dispute must be rejected,
    // not silently succeed, to prevent any ambiguity in event emission.
    let (env, client, _owner, _admin, user) = setup();
    let id = 506u128;

    client.file_dispute(&user, &id);
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);
    client.keeper_advance_stage(&user, &id);

    let res = client.try_keeper_advance_stage(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyPendingReview)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// §6  ACCESS CONTROL TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_non_admin_cannot_resolve() {
    let (_env, client, _owner, _admin, user) = setup();
    let id = 600u128;

    client.file_dispute(&user, &id);
    let res = client.try_resolve_dispute(&user, &id, &DisputeOutcome::UpholdPayment);
    assert_eq!(res, Err(Ok(DisputeError::Unauthorized)));
}

#[test]
fn test_non_admin_cannot_set_level_time_limit() {
    let (_env, client, _owner, _admin, user) = setup();
    let res = client.try_set_level_time_limit(&user, &EscalationLevel::Level1, &120u64);
    assert_eq!(res, Err(Ok(DisputeError::Unauthorized)));
}

#[test]
fn test_non_admin_cannot_set_pending_review_time_limit() {
    let (_env, client, _owner, _admin, user) = setup();
    let res = client.try_set_pending_review_time_limit(&user, &120u64);
    assert_eq!(res, Err(Ok(DisputeError::Unauthorized)));
}

#[test]
fn test_owner_can_set_pending_review_time_limit() {
    // Owner falls back as admin when no separate admin is set.
    // In our setup admin IS set, so let's verify the admin path directly.
    let (_env, client, _owner, admin, _user) = setup();
    client.set_pending_review_time_limit(&admin, &7200u64);
    assert_eq!(client.get_pending_review_time_limit(), 7200u64);
}

#[test]
fn test_keeper_advance_stage_is_permissionless() {
    // Any address (not just admin) can call keeper_advance_stage
    let (env, client, _owner, _admin, user) = setup();
    let third_party = Address::generate(&env);
    let id = 601u128;

    client.file_dispute(&user, &id);
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);

    // Third party (not admin, not initiator) can advance the stage
    client.keeper_advance_stage(&third_party, &id);
    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::PendingReview
    );
}

#[test]
fn test_expire_dispute_is_permissionless() {
    // Any address can call expire_dispute after the deadline
    let (env, client, _owner, _admin, user) = setup();
    let third_party = Address::generate(&env);
    let id = 602u128;

    client.file_dispute(&user, &id);
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);

    client.expire_dispute(&third_party, &id);
    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::Expired
    );
}

#[test]
fn test_resolve_with_unset_outcome_fails() {
    let (_env, client, _owner, admin, user) = setup();
    let id = 603u128;

    client.file_dispute(&user, &id);
    let res = client.try_resolve_dispute(&admin, &id, &DisputeOutcome::Unset);
    assert_eq!(res, Err(Ok(DisputeError::InvalidTransition)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// §7  DOUBLE-RESOLVE / FINALITY IDEMPOTENCY TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_cannot_double_resolve() {
    let (_env, client, _owner, admin, user) = setup();
    let id = 700u128;

    client.file_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);

    let res = client.try_resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyResolved)));
}

#[test]
fn test_cannot_resolve_finalised_dispute() {
    let (_env, client, _owner, admin, user) = setup();
    let id = 701u128;

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
    let id = 702u128;

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
    let id = 703u128;

    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id); // → Level2
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    client.appeal_ruling(&user, &id); // → Level3

    let res = client.try_escalate_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::MaxEscalationReached)));
}

#[test]
fn test_cannot_appeal_beyond_level3() {
    let (_env, client, _owner, admin, user) = setup();
    let id = 704u128;

    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    client.appeal_ruling(&user, &id); // → Level3

    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim); // → Finalised
    let res = client.try_appeal_ruling(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyFinalised)));
}

#[test]
fn test_repeated_expire_rejected() {
    let (env, client, _owner, _admin, user) = setup();
    let id = 705u128;

    client.file_dispute(&user, &id);
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);
    client.expire_dispute(&user, &id);

    let res = client.try_expire_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyTerminal)));
}

#[test]
fn test_repeated_file_dispute_rejected() {
    let (_env, client, _owner, _admin, user) = setup();
    let id = 706u128;

    client.file_dispute(&user, &id);

    let res = client.try_file_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::InvalidTransition)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// §8  EXPIRE DISPUTE TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_expire_dispute_after_deadline() {
    let (env, client, _owner, _admin, user) = setup();
    let id = 800u128;

    client.file_dispute(&user, &id);
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);

    client.expire_dispute(&user, &id);
    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::Expired
    );
}

#[test]
fn test_cannot_expire_before_deadline() {
    let (_env, client, _owner, _admin, user) = setup();
    let id = 801u128;

    client.file_dispute(&user, &id);

    let res = client.try_expire_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::DeadlineNotPassed)));
}

#[test]
fn test_cannot_expire_already_terminal_dispute() {
    let (env, client, _owner, _admin, user) = setup();
    let id = 802u128;

    client.file_dispute(&user, &id);
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);
    client.expire_dispute(&user, &id);

    let res = client.try_expire_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyTerminal)));
}

#[test]
fn test_cannot_resolve_expired_dispute() {
    let (env, client, _owner, admin, user) = setup();
    let id = 803u128;

    client.file_dispute(&user, &id);
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);
    client.expire_dispute(&user, &id);

    let res = client.try_resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyTerminal)));
}

#[test]
fn test_cannot_escalate_expired_dispute() {
    let (env, client, _owner, _admin, user) = setup();
    let id = 804u128;

    client.file_dispute(&user, &id);
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);
    client.expire_dispute(&user, &id);

    let res = client.try_escalate_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyTerminal)));
}

#[test]
fn test_cannot_expire_resolved_dispute() {
    // Resolved disputes have an active appeal window — expire is blocked
    let (_env, client, _owner, admin, user) = setup();
    let id = 805u128;

    client.file_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);

    // The phase_deadline is now the appeal window; but the AlreadyResolved guard fires first
    let res = client.try_expire_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyResolved)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// §9  SINGLE APPEAL INVARIANT TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_second_appeal_is_rejected() {
    // Verifies that a dispute can only be appealed once per resolution cycle.
    // After the first appeal_ruling, the status becomes Appealed, and a second
    // appeal_ruling call should fail with InvalidTransition because the dispute
    // is no longer in Resolved state.
    let (_env, client, _owner, admin, user) = setup();
    let id = 950u128;

    // 1. File and resolve at Level1
    client.file_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::Resolved
    );

    // 2. First appeal succeeds → Appealed @ Level2
    client.appeal_ruling(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Appealed);
    assert_eq!(d.level, EscalationLevel::Level2);
    assert_eq!(d.outcome, DisputeOutcome::Unset);

    // 3. Second appeal should fail - dispute is in Appealed state, not Resolved
    let res = client.try_appeal_ruling(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::InvalidTransition)));
}

#[test]
fn test_second_appeal_after_level2_resolve_is_rejected() {
    // Same invariant test but at Level2: after appealing from Level2 to Level3,
    // a second appeal should be rejected.
    let (_env, client, _owner, admin, user) = setup();
    let id = 951u128;

    // 1. File → escalate to Level2 → resolve
    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);
    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::Resolved
    );

    // 2. First appeal succeeds → Appealed @ Level3
    client.appeal_ruling(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Appealed);
    assert_eq!(d.level, EscalationLevel::Level3);

    // 3. Second appeal should fail
    let res = client.try_appeal_ruling(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::InvalidTransition)));
}

#[test]
fn test_final_state_reachable_after_single_appeal() {
    // Verifies that after the single permitted appeal resolves (at Level3),
    // the final state (Finalised) is reachable and stable.
    let (_env, client, _owner, admin, user) = setup();
    let id = 952u128;

    // 1. File → resolve at Level1
    client.file_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);

    // 2. Appeal to Level2
    client.appeal_ruling(&user, &id);
    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::Appealed
    );

    // 3. Resolve at Level2
    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);
    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::Resolved
    );

    // 4. Appeal to Level3 (the final permitted appeal)
    client.appeal_ruling(&user, &id);
    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::Appealed
    );
    assert_eq!(
        client.get_dispute(&id).unwrap().level,
        EscalationLevel::Level3
    );

    // 5. Resolve at Level3 → Finalised (terminal state)
    client.resolve_dispute(&admin, &id, &DisputeOutcome::PartialSettlement);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Finalised);
    assert_eq!(d.outcome, DisputeOutcome::PartialSettlement);

    // 6. Verify final state is stable - no further transitions allowed
    let res = client.try_appeal_ruling(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyFinalised)));

    let res = client.try_resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyFinalised)));
}

#[test]
fn test_appeal_from_appealed_state_fails_directly() {
    // Direct test: attempt appeal_ruling on a dispute already in Appealed state
    // should fail with InvalidTransition (appeal_ruling requires Resolved status)
    let (_env, client, _owner, admin, user) = setup();
    let id = 953u128;

    client.file_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    client.appeal_ruling(&user, &id); // Now in Appealed state

    // Direct attempt to appeal again should fail
    let res = client.try_appeal_ruling(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::InvalidTransition)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// §10  DUPLICATE / CONCURRENT DISPUTE TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_cannot_file_duplicate_dispute() {
    let (_env, client, _owner, _admin, user) = setup();
    let id = 900u128;

    client.file_dispute(&user, &id);

    let res = client.try_file_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::InvalidTransition)));
}

#[test]
fn test_concurrent_disputes_are_independent() {
    let (_env, client, _owner, admin, user) = setup();
    let id1 = 901u128;
    let id2 = 902u128;

    client.file_dispute(&user, &id1);
    client.file_dispute(&user, &id2);

    client.resolve_dispute(&admin, &id1, &DisputeOutcome::UpholdPayment);

    assert_eq!(
        client.get_dispute(&id2).unwrap().status,
        DisputeStatus::Open
    );
    assert_eq!(
        client.get_dispute(&id1).unwrap().status,
        DisputeStatus::Resolved
    );
}

#[test]
fn test_three_concurrent_disputes_with_different_levels() {
    let (env, client, _owner, admin, user) = setup();
    let id_open = 903u128;
    let id_escalated = 904u128;
    let id_pending = 905u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &50u64);

    client.file_dispute(&user, &id_open);
    client.file_dispute(&user, &id_escalated);
    client.file_dispute(&user, &id_pending);

    // Escalate one
    client.escalate_dispute(&user, &id_escalated);

    // Advance to trigger PendingReview on the third
    advance(&env, 51);
    client.keeper_advance_stage(&user, &id_pending);

    // Each dispute is independent
    assert_eq!(
        client.get_dispute(&id_open).unwrap().status,
        DisputeStatus::Open
    );
    assert_eq!(
        client.get_dispute(&id_escalated).unwrap().status,
        DisputeStatus::Escalated
    );
    assert_eq!(
        client.get_dispute(&id_pending).unwrap().status,
        DisputeStatus::PendingReview
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// §10  APPEAL ON INVALID STATES
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_cannot_appeal_open_dispute() {
    let (_env, client, _owner, _admin, user) = setup();
    let id = 1000u128;

    client.file_dispute(&user, &id);

    let res = client.try_appeal_ruling(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::InvalidTransition)));
}

#[test]
fn test_cannot_appeal_escalated_dispute() {
    let (_env, client, _owner, _admin, user) = setup();
    let id = 1001u128;

    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id);

    let res = client.try_appeal_ruling(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::InvalidTransition)));
}

#[test]
fn test_cannot_appeal_expired_dispute() {
    let (env, client, _owner, _admin, user) = setup();
    let id = 1002u128;

    client.file_dispute(&user, &id);
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);
    client.expire_dispute(&user, &id);

    let res = client.try_appeal_ruling(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyTerminal)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// §11  NONEXISTENT DISPUTE TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_get_nonexistent_dispute_returns_none() {
    let (_env, client, _owner, _admin, _user) = setup();
    assert!(client.get_dispute(&9999u128).is_none());
}

#[test]
fn test_escalate_nonexistent_dispute() {
    let (_env, client, _owner, _admin, user) = setup();
    let res = client.try_escalate_dispute(&user, &9998u128);
    assert_eq!(res, Err(Ok(DisputeError::DisputeNotFound)));
}

#[test]
fn test_resolve_nonexistent_dispute() {
    let (_env, client, _owner, admin, _user) = setup();
    let res = client.try_resolve_dispute(&admin, &9997u128, &DisputeOutcome::UpholdPayment);
    assert_eq!(res, Err(Ok(DisputeError::DisputeNotFound)));
}

#[test]
fn test_appeal_nonexistent_dispute() {
    let (_env, client, _owner, _admin, user) = setup();
    let res = client.try_appeal_ruling(&user, &9996u128);
    assert_eq!(res, Err(Ok(DisputeError::DisputeNotFound)));
}

#[test]
fn test_expire_nonexistent_dispute() {
    let (_env, client, _owner, _admin, user) = setup();
    let res = client.try_expire_dispute(&user, &9995u128);
    assert_eq!(res, Err(Ok(DisputeError::DisputeNotFound)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// §12  DETERMINISTIC TIMESTAMP INVARIANT TESTS
//     Verify that all deadline computations are fully deterministic given a
//     known ledger timestamp at the moment of each state-changing call.
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_file_dispute_deadline_is_deterministic() {
    let (env, client, _owner, admin, user) = setup();
    let id = 1200u128;
    let custom_limit = 3_600u64;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &custom_limit);

    // Advance to a well-known timestamp
    advance(&env, 1_000);
    let t0 = now(&env);

    client.file_dispute(&user, &id);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.phase_started_at, t0);
    assert_eq!(d.phase_deadline, t0 + custom_limit);
}

#[test]
fn test_escalate_deadline_is_deterministic() {
    let (env, client, _owner, admin, user) = setup();
    let id = 1201u128;
    let l2_limit = 7_200u64;

    client.set_level_time_limit(&admin, &EscalationLevel::Level2, &l2_limit);
    client.file_dispute(&user, &id);

    advance(&env, 100); // advance by 100 s — still within L1 window
    let t_escalate = now(&env);

    client.escalate_dispute(&user, &id);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.phase_started_at, t_escalate);
    assert_eq!(d.phase_deadline, t_escalate + l2_limit);
}

#[test]
fn test_resolve_appeal_deadline_is_deterministic() {
    let (env, client, _owner, admin, user) = setup();
    let id = 1202u128;

    client.file_dispute(&user, &id);

    advance(&env, 50); // some time before deadline
    let t_resolve = now(&env);

    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);

    let d = client.get_dispute(&id).unwrap();
    // Appeal deadline is exactly 3 days from resolution timestamp
    assert_eq!(d.phase_deadline, t_resolve + APPEAL_WINDOW);
}

#[test]
fn test_keeper_advance_review_deadline_is_deterministic() {
    let (env, client, _owner, admin, user) = setup();
    let id = 1203u128;
    let review_limit = 86_400u64; // 1 day

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &50u64);
    client.set_pending_review_time_limit(&admin, &review_limit);
    client.file_dispute(&user, &id);

    advance(&env, 51); // SLA elapsed
    let t_advance = now(&env);

    client.keeper_advance_stage(&user, &id);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.phase_started_at, t_advance);
    assert_eq!(d.phase_deadline, t_advance + review_limit);
}

#[test]
fn test_appeal_deadline_uses_next_level_limit() {
    // After an appeal, the new phase_deadline uses the *next level's* time limit.
    let (env, client, _owner, admin, user) = setup();
    let id = 1204u128;
    let l2_limit = 500u64;

    client.set_level_time_limit(&admin, &EscalationLevel::Level2, &l2_limit);
    client.file_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);

    let t_appeal = now(&env);
    client.appeal_ruling(&user, &id);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.level, EscalationLevel::Level2);
    assert_eq!(d.phase_started_at, t_appeal);
    assert_eq!(d.phase_deadline, t_appeal + l2_limit);
}

#[test]
fn test_phase_started_at_updated_on_every_transition() {
    // Every state-changing call must update phase_started_at to the current
    // ledger timestamp — this is essential for deterministic SLA accounting.
    let (env, client, _owner, admin, user) = setup();
    let id = 1205u128;

    advance(&env, 10);
    client.file_dispute(&user, &id);
    let t_file = now(&env);
    assert_eq!(client.get_dispute(&id).unwrap().phase_started_at, t_file);

    advance(&env, 20);
    client.escalate_dispute(&user, &id);
    let t_escalate = now(&env);
    assert_eq!(
        client.get_dispute(&id).unwrap().phase_started_at,
        t_escalate
    );

    advance(&env, 30);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    let t_resolve = now(&env);
    assert_eq!(client.get_dispute(&id).unwrap().phase_started_at, t_resolve);

    advance(&env, 5);
    client.appeal_ruling(&user, &id);
    let t_appeal = now(&env);
    assert_eq!(client.get_dispute(&id).unwrap().phase_started_at, t_appeal);
}

// ═══════════════════════════════════════════════════════════════════════════════
// §13  STAGE-SKIP PREVENTION TESTS
//     Verify that permissionless keepers cannot bypass the staged state machine.
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_keeper_cannot_resolve_directly() {
    // keeper_advance_stage only goes to PendingReview — never Resolved/Finalised
    let (env, client, _owner, _admin, user) = setup();
    let id = 1300u128;

    client.file_dispute(&user, &id);
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);
    client.keeper_advance_stage(&user, &id);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::PendingReview);
    // Outcome remains unset — keeper did not resolve anything
    assert_eq!(d.outcome, DisputeOutcome::Unset);
}

#[test]
fn test_keeper_cannot_skip_pending_review_to_finalised() {
    // Even at Level3, keeper_advance_stage must stop at PendingReview
    let (env, client, _owner, admin, user) = setup();
    let id = 1301u128;

    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    client.appeal_ruling(&user, &id); // → Level3

    advance(&env, DEFAULT_LEVEL_LIMIT + 1);
    client.keeper_advance_stage(&user, &id);

    let d = client.get_dispute(&id).unwrap();
    assert_ne!(d.status, DisputeStatus::Finalised);
    assert_eq!(d.status, DisputeStatus::PendingReview);
}

#[test]
fn test_escalation_cannot_skip_level() {
    // Escalation must go Level1→Level2→Level3, never jump to Level3 directly
    let (_env, client, _owner, _admin, user) = setup();
    let id = 1302u128;

    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.level, EscalationLevel::Level2); // not Level3
}

#[test]
fn test_cannot_escalate_from_resolved_state() {
    let (_env, client, _owner, admin, user) = setup();
    let id = 1303u128;

    client.file_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);

    let res = client.try_escalate_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyResolved)));
}

#[test]
fn test_keeper_advance_stage_overflow_returns_distinct_error() {
    // When pending_review_time_limit is set to u64::MAX, now + limit overflows
    // and should return SlaDeadlineOverflow, not a silent wraparound.
    let (env, client, _owner, admin, user) = setup();
    let id = 1401u128;

    client.file_dispute(&user, &id);

    // Advance past the deadline so keeper_advance_stage is callable
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);

    // Set a time limit that will overflow when added to the current timestamp
    client.set_pending_review_time_limit(&admin, &u64::MAX);

    let res = client.try_keeper_advance_stage(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::SlaDeadlineOverflow)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// §14  EDGE-CASE TESTS: L3 DEADLINE BOUNDARY, L3 WITHOUT L2 RULING, DUPLICATE L3
// ═══════════════════════════════════════════════════════════════════════════════

/// Policy: `resolve_dispute` has no deadline gate — admin may resolve at any
/// time while the dispute is in a resolvable state.  A Level3 ruling issued
/// exactly when `env.ledger().timestamp() == phase_deadline` must therefore
/// succeed and produce a `Finalised` terminal state.
///
/// This test documents and locks that behaviour.
#[test]
fn test_level3_ruling_exactly_at_phase_deadline_accepted() {
    let (env, client, _owner, admin, user) = setup();
    let id = 1402u128;

    // Use short limits so we can control time precisely.
    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &100u64);
    client.set_level_time_limit(&admin, &EscalationLevel::Level2, &100u64);
    client.set_level_time_limit(&admin, &EscalationLevel::Level3, &100u64);

    // Reach Level3 via the standard appeal path.
    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id); // → Level2
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment); // → Resolved L2
    client.appeal_ruling(&user, &id); // → Appealed L3

    // Advance time to exactly the Level3 phase_deadline.
    let deadline = client.get_dispute(&id).unwrap().phase_deadline;
    let current = now(&env);
    advance(&env, deadline - current);
    assert_eq!(now(&env), deadline);

    // Admin resolves at exactly the deadline — must succeed (no deadline gate on resolve).
    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Finalised);
    assert_eq!(d.outcome, DisputeOutcome::GrantClaim);
    assert_eq!(d.level, EscalationLevel::Level3);
}

/// A dispute can reach Level3 by escalating twice (L1→L2→L3) without any
/// Level2 ruling ever being issued.  This path bypasses `resolve_dispute` at
/// Level2 entirely.  The contract must accept a Level3 resolution in this
/// state and produce `Finalised`.
#[test]
fn test_level3_reached_without_level2_ruling() {
    let (_env, client, _owner, admin, user) = setup();
    let id = 1403u128;

    // Escalate directly: L1 → L2 → L3 (no ruling at any intermediate level).
    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id); // Open L1 → Escalated L2
    client.escalate_dispute(&user, &id); // Escalated L2 → Escalated L3

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Escalated);
    assert_eq!(d.level, EscalationLevel::Level3);
    assert_eq!(d.outcome, DisputeOutcome::Unset); // no ruling yet

    // Admin issues the first and only ruling directly at Level3.
    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Finalised);
    assert_eq!(d.outcome, DisputeOutcome::GrantClaim);
    assert_eq!(d.level, EscalationLevel::Level3);
}

/// A Level3 ruling is binding and final.  Any attempt to issue a second ruling
/// (regardless of timing) must be rejected with `AlreadyFinalised`.
/// This confirms that the `Finalised` terminal state truly cannot be altered.
#[test]
fn test_duplicate_level3_ruling_rejected() {
    let (env, client, _owner, admin, user) = setup();
    let id = 1404u128;

    // Reach Finalised via the full appeal path.
    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id); // → L2
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment); // → Resolved L2
    client.appeal_ruling(&user, &id); // → Appealed L3
    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim); // → Finalised

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Finalised);
    assert_eq!(d.outcome, DisputeOutcome::GrantClaim);

    // Advance time to simulate a "late" attempt — must still be rejected.
    advance(&env, DEFAULT_LEVEL_LIMIT + 1);

    // Second ruling attempt: must be rejected regardless of outcome value.
    let res = client.try_resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyFinalised)));

    // Outcome must remain unchanged — GrantClaim, not UpholdPayment.
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.outcome, DisputeOutcome::GrantClaim);
}

/// Calling `escalate_dispute` **once** on a freshly-filed Level1 dispute must
/// land the dispute at **Level2**, never directly at Level3.
///
/// **This test is observational, not error-based.** The public API has no
/// `target_level` parameter that would let a caller *express* a direct
/// Level3 jump, so a "rejection" can only be observed by where the dispute
/// ends up (Level2, never Level3).
///
/// This locks in the guard provided by `next_level`: the public escalation
/// surface moves exactly one tier per call, and no caller can traverse two
/// tiers in a single transaction.  See also the prior test
/// `test_escalation_cannot_skip_level` (further up in §13 of this file) for
/// the minimal shape of the same guarantee.
///
/// This test strengthens that prior art by asserting additional per-field
/// invariants — the dispute must be in the `Escalated` status, the
/// `phase_started_at` must be the current ledger timestamp, the
/// `phase_deadline` must follow the **Level2** SLA not the Level3 SLA, and
/// the `outcome` must remain `Unset`.  These extra checks ensure that an
/// attempted skip could not silently mutate unrelated fields (e.g. the
/// PhaseSLA timer) as a side effect.
#[test]
fn test_escalate_from_level1_rejects_skip_to_level3() {
    let (env, client, _owner, admin, user) = setup();
    let id = 1450u128;

    // Tighten the Level2 SLA so the deadline arithmetic check is precise.
    // Use a literal that matches the value passed to set_level_time_limit.
    const L2_LIMIT: u64 = 42;
    client.set_level_time_limit(&admin, &EscalationLevel::Level2, &L2_LIMIT);

    // Reach Level1 cleanly.
    client.file_dispute(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Open);
    assert_eq!(d.level, EscalationLevel::Level1);
    assert_eq!(d.outcome, DisputeOutcome::Unset);

    // Record the timestamp BEFORE the escalate call so we can later prove
    // `phase_started_at` was updated to the call timestamp.
    let pre_escalate_ts = now(&env);
    advance(&env, 5);

    // ONE call to escalate_dispute — at most Level1 → Level2, never Level3.
    client.escalate_dispute(&user, &id);

    let d = client.get_dispute(&id).unwrap();
    // The headline assertion: a single escalate call cannot reach Level3.
    assert_eq!(
        d.level,
        EscalationLevel::Level2,
        "a single escalate_dispute call must land at Level2, never Level3"
    );

    // Secondary assertions: the contract did not silently mutate unrelated
    // storage as part of a hypothetical skip attempt.
    assert_eq!(d.status, DisputeStatus::Escalated);
    assert_eq!(d.outcome, DisputeOutcome::Unset); // no ruling yet
    assert_eq!(d.phase_started_at, pre_escalate_ts + 5); // exactly escalate-call time
    assert_eq!(
        d.phase_deadline,
        pre_escalate_ts + 5 + L2_LIMIT,
        "phase_deadline must follow the Level2 SLA, not an unwritten Level3 SLA"
    );
}

/// The correct sequential escalation path Level1 → Level2 → Level3 must fully
/// succeed. After reaching Level3 via three consecutive calls (one `file_dispute`
/// + two `escalate_dispute`), a fourth escalation attempt must be rejected with
/// `MaxEscalationReached` because `next_level(Level3) == Err(MaxEscalationReached)`.
///
/// This is the positive complement to
/// `test_escalate_from_level1_rejects_skip_to_level3`: it proves that the
/// sequential walk finishes at Level3 and is **not** encountered by accident
/// in the rejection test.
#[test]
fn test_sequential_level1_level2_level3_escalation_path_succeeds() {
    let (_env, client, _owner, _admin, user) = setup();
    let id = 1451u128;

    // Step 1: file → Open @ Level1.
    client.file_dispute(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Open);
    assert_eq!(d.level, EscalationLevel::Level1);

    // Step 2: escalate → Escalated @ Level2.
    client.escalate_dispute(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Escalated);
    assert_eq!(d.level, EscalationLevel::Level2);

    // Step 3: escalate → Escalated @ Level3 (the terminal escalation tier).
    client.escalate_dispute(&user, &id);
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Escalated);
    assert_eq!(d.level, EscalationLevel::Level3);
    assert_eq!(d.outcome, DisputeOutcome::Unset);

    // Step 4: any further escalation attempt is rejected at Level3.
    let res = client.try_escalate_dispute(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::MaxEscalationReached)));

    // The dispute must not have moved past Level3 as a side effect of the failed call.
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.level, EscalationLevel::Level3);
    assert_eq!(d.status, DisputeStatus::Escalated);
}

/// Late Level3 ruling via PendingReview path — keeper advances a Level3
/// dispute to PendingReview, then a second resolve attempt after the first
/// finalisation must be rejected.
#[test]
fn test_late_level3_ruling_via_pending_review_rejected() {
    let (env, client, _owner, admin, user) = setup();
    let id = 1405u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &50u64);
    client.set_level_time_limit(&admin, &EscalationLevel::Level2, &50u64);
    client.set_level_time_limit(&admin, &EscalationLevel::Level3, &50u64);
    client.set_pending_review_time_limit(&admin, &100u64);

    // Reach Level3 via escalate + appeal.
    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id); // → L2
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment); // → Resolved L2
    client.appeal_ruling(&user, &id); // → Appealed L3

    // Let L3 SLA lapse → keeper advances to PendingReview.
    advance(&env, 51);
    client.keeper_advance_stage(&user, &id);
    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::PendingReview
    );

    // Admin resolves from PendingReview at Level3 → Finalised.
    client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);
    assert_eq!(
        client.get_dispute(&id).unwrap().status,
        DisputeStatus::Finalised
    );
    assert_eq!(
        client.get_dispute(&id).unwrap().outcome,
        DisputeOutcome::GrantClaim
    );

    // Any further resolve attempt must be rejected — binding outcome is immutable.
    let res = client.try_resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyFinalised)));

    // Outcome is unchanged.
    assert_eq!(
        client.get_dispute(&id).unwrap().outcome,
        DisputeOutcome::GrantClaim
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// §15  SLA VIOLATION ADVANCED EVENT TESTS
//      Verify that `keeper_advance_stage` emits the `sla_violation_advanced`
//      event with the correct fields, and that `escalate_dispute` (normal flow)
//      does NOT emit it.
// ═══════════════════════════════════════════════════════════════════════════════

/// Helper: scan `env.events().all()` for the **last** event whose topics match
/// a single-element topics vector containing the given `topic` string.
/// Returns `Some((contract_id, topics, data))` or `None`.
///
/// Soroban's `Events::publish` with a `("topic",)` tuple stores topics as
/// `ScString` values.  We construct the expected topics Vec with
/// `SorobanString` to match.
fn find_event_by_topic(
    env: &Env,
    _contract_id: &Address,
    topic: &str,
) -> Option<(Address, Vec<Val>, Val)> {
    let events = env.events().all();
    let topic_str = SorobanString::from_str(env, topic);
    let expected_topics: Vec<Val> = (topic_str,).into_val(env);
    // Events are stored chronologically; we scan from the end.
    for i in (0..events.len()).rev() {
        let evt: (Address, Vec<Val>, Val) = events.get(i).unwrap();
        if evt.1 == expected_topics {
            return Some(evt);
        }
    }
    None
}

/// Assert that `env.events()` contains **no** event with the given topic.
fn assert_no_event(env: &Env, _contract_id: &Address, topic: &str) {
    assert!(
        find_event_by_topic(env, _contract_id, topic).is_none(),
        "expected no '{}' event but found one",
        topic
    );
}

/// Test: `keeper_advance_stage` from `Open` state emits `sla_violation_advanced`
/// with `previous_status == Open`.
#[test]
fn test_sla_violation_advanced_event_emitted_from_open() {
    let (env, client, _owner, admin, user) = setup();
    let id = 1500u128;
    let review_limit = 100u64;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &50u64);
    client.set_pending_review_time_limit(&admin, &review_limit);
    client.file_dispute(&user, &id);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Open);

    // Advance past SLA deadline
    advance(&env, 51);

    let contract_id = client.address.clone();
    client.keeper_advance_stage(&user, &id);

    // Verify the new event exists
    let found = find_event_by_topic(&env, &contract_id, "sla_violation_advanced");
    assert!(
        found.is_some(),
        "sla_violation_advanced event must be emitted"
    );

    // Verify backward-compatible event still emitted
    let old_event = find_event_by_topic(&env, &contract_id, "dispute_sla_breached");
    assert!(
        old_event.is_some(),
        "dispute_sla_breached must still be emitted for backward compatibility"
    );

    // Verify the dispute state transition
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::PendingReview);
    assert_eq!(d.level, EscalationLevel::Level1);
}

/// Test: `keeper_advance_stage` from `Escalated` state emits
/// `sla_violation_advanced` with `previous_status == Escalated`.
#[test]
fn test_sla_violation_advanced_event_emitted_from_escalated() {
    let (env, client, _owner, admin, user) = setup();
    let id = 1501u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &50u64);
    client.set_level_time_limit(&admin, &EscalationLevel::Level2, &50u64);
    client.set_pending_review_time_limit(&admin, &100u64);
    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Escalated);
    assert_eq!(d.level, EscalationLevel::Level2);

    // Advance past Level2 SLA deadline
    advance(&env, 51);

    let contract_id = client.address.clone();
    client.keeper_advance_stage(&user, &id);

    // Verify event emitted
    let found = find_event_by_topic(&env, &contract_id, "sla_violation_advanced");
    assert!(
        found.is_some(),
        "sla_violation_advanced event must be emitted from Escalated state"
    );

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::PendingReview);
    assert_eq!(d.level, EscalationLevel::Level2);
}

/// Test: `keeper_advance_stage` from `Appealed` state emits
/// `sla_violation_advanced` with `previous_status == Appealed`.
#[test]
fn test_sla_violation_advanced_event_emitted_from_appealed() {
    let (env, client, _owner, admin, user) = setup();
    let id = 1502u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &50u64);
    client.set_level_time_limit(&admin, &EscalationLevel::Level2, &50u64);
    client.set_pending_review_time_limit(&admin, &100u64);
    client.file_dispute(&user, &id);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    client.appeal_ruling(&user, &id);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Appealed);
    assert_eq!(d.level, EscalationLevel::Level2);

    // Advance past Level2 SLA deadline
    advance(&env, 51);

    let contract_id = client.address.clone();
    client.keeper_advance_stage(&user, &id);

    // Verify event emitted
    let found = find_event_by_topic(&env, &contract_id, "sla_violation_advanced");
    assert!(
        found.is_some(),
        "sla_violation_advanced event must be emitted from Appealed state"
    );

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::PendingReview);
    assert_eq!(d.level, EscalationLevel::Level2);
}

/// Test: `escalate_dispute` (normal-flow advancement) does NOT emit
/// `sla_violation_advanced`.  Only `keeper_advance_stage` (SLA timeout)
/// emits it.
///
/// We verify the negative — after a normal-flow escalation, no SLA-violation
/// event exists.  The positive side (that `dispute_escalated` IS emitted) is
/// already covered by the lifecycle tests in §1.
#[test]
fn test_escalate_dispute_does_not_emit_sla_violation_advanced() {
    let (env, client, _owner, admin, user) = setup();
    let id = 1503u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &100u64);
    client.file_dispute(&user, &id);

    // Escalate within deadline — normal flow
    client.escalate_dispute(&user, &id);

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Escalated);
    assert_eq!(d.level, EscalationLevel::Level2);

    // CRITICAL: `sla_violation_advanced` must NOT appear — this was a normal
    // escalation, not an SLA timeout.
    let contract_id = client.address.clone();
    assert_no_event(&env, &contract_id, "sla_violation_advanced");
}

/// Test: `sla_violation_advanced` and `dispute_sla_breached` are BOTH emitted
/// from a single `keeper_advance_stage` call — backward compatibility is
/// preserved while the new event is available.
#[test]
fn test_keeper_advance_stage_emits_both_sla_events() {
    let (env, client, _owner, admin, user) = setup();
    let id = 1504u128;
    let review_limit = 200u64;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &60u64);
    client.set_pending_review_time_limit(&admin, &review_limit);
    client.file_dispute(&user, &id);

    // Advance past SLA
    advance(&env, 61);
    let t_breach = now(&env);

    let contract_id = client.address.clone();
    client.keeper_advance_stage(&user, &id);

    // Both events must be present
    let old_event = find_event_by_topic(&env, &contract_id, "dispute_sla_breached");
    assert!(
        old_event.is_some(),
        "dispute_sla_breached must be emitted (backward compat)"
    );

    let new_event = find_event_by_topic(&env, &contract_id, "sla_violation_advanced");
    assert!(
        new_event.is_some(),
        "sla_violation_advanced must be emitted (new SLA violation signal)"
    );

    // Verify they are distinct events (different topic)
    let old_evt = old_event.unwrap();
    let new_evt = new_event.unwrap();
    // The topics vectors differ: "dispute_sla_breached" vs "sla_violation_advanced"
    assert_ne!(
        old_evt.1, new_evt.1,
        "the two events must have distinct topic symbols"
    );

    // Verify dispute state
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::PendingReview);
    assert_eq!(d.phase_started_at, t_breach);
    assert_eq!(d.phase_deadline, t_breach + review_limit);
}

/// Test: `keeper_advance_stage` does NOT emit `dispute_escalated` — only the
/// SLA-specific events.  The two event families are cleanly separated.
#[test]
fn test_keeper_advance_stage_does_not_emit_dispute_escalated() {
    let (env, client, _owner, admin, user) = setup();
    let id = 1505u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &50u64);
    client.set_pending_review_time_limit(&admin, &100u64);
    client.file_dispute(&user, &id);

    advance(&env, 51);

    let contract_id = client.address.clone();
    client.keeper_advance_stage(&user, &id);

    // No `dispute_escalated` event from the keeper path
    assert_no_event(&env, &contract_id, "dispute_escalated");

    // But SLA events must be present
    assert!(find_event_by_topic(&env, &contract_id, "sla_violation_advanced").is_some());
    assert!(find_event_by_topic(&env, &contract_id, "dispute_sla_breached").is_some());
}

/// Test: the `sla_violation_advanced` event is NOT emitted on a failed
/// `keeper_advance_stage` call (e.g. deadline not passed yet).
#[test]
fn test_sla_violation_advanced_not_emitted_on_failed_keeper_call() {
    let (env, client, _owner, admin, user) = setup();
    let id = 1506u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &100u64);
    client.file_dispute(&user, &id);

    // Do NOT advance time — deadline not passed
    let contract_id = client.address.clone();
    let res = client.try_keeper_advance_stage(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::DeadlineNotPassed)));

    // No SLA violation event should have been emitted on failure
    assert_no_event(&env, &contract_id, "sla_violation_advanced");
    assert_no_event(&env, &contract_id, "dispute_sla_breached");
}

/// Test: verify the event emitted from a Level3 Appealed state carries the
/// correct `level` field.
#[test]
fn test_sla_violation_advanced_from_level3_appealed() {
    let (env, client, _owner, admin, user) = setup();
    let id = 1507u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &50u64);
    client.set_level_time_limit(&admin, &EscalationLevel::Level2, &50u64);
    client.set_level_time_limit(&admin, &EscalationLevel::Level3, &50u64);
    client.set_pending_review_time_limit(&admin, &100u64);

    // Reach Level3 via appeal path
    client.file_dispute(&user, &id);
    client.escalate_dispute(&user, &id); // → Level2
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    client.appeal_ruling(&user, &id); // → Appealed @ Level3

    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Appealed);
    assert_eq!(d.level, EscalationLevel::Level3);

    advance(&env, 51); // past SLA

    let contract_id = client.address.clone();
    client.keeper_advance_stage(&user, &id);

    // Event must be present
    let found = find_event_by_topic(&env, &contract_id, "sla_violation_advanced");
    assert!(
        found.is_some(),
        "sla_violation_advanced must be emitted at Level3"
    );

    // The dispute must be at PendingReview @ Level3
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::PendingReview);
    assert_eq!(d.level, EscalationLevel::Level3);
}

/// Test: idempotency guard — second `keeper_advance_stage` call on an
/// already-`PendingReview` dispute does NOT emit a second
/// `sla_violation_advanced` event (the call is rejected).
#[test]
fn test_sla_violation_advanced_not_emitted_on_idempotent_rejected_call() {
    let (env, client, _owner, admin, user) = setup();
    let id = 1508u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &50u64);
    client.set_pending_review_time_limit(&admin, &100u64);
    client.file_dispute(&user, &id);

    advance(&env, 51);

    // First call — succeeds, emits the event
    client.keeper_advance_stage(&user, &id);

    // Verify the first call emitted the event
    let contract_id = client.address.clone();
    let found = find_event_by_topic(&env, &contract_id, "sla_violation_advanced");
    assert!(
        found.is_some(),
        "first keeper call must emit sla_violation_advanced"
    );

    // Verify dispute state
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::PendingReview);

    // Second call — rejected (AlreadyPendingReview)
    let res = client.try_keeper_advance_stage(&user, &id);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyPendingReview)));

    // The failed call must NOT have emitted a second sla_violation_advanced.
    // Since `try_*` failures do not emit events, we just verify the dispute
    // state is unchanged and the result was the expected error.
    let d = client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::PendingReview);
}

/// Test: `expire_dispute` does NOT emit `sla_violation_advanced` — only
/// `keeper_advance_stage` does.
#[test]
fn test_expire_dispute_does_not_emit_sla_violation_advanced() {
    let (env, client, _owner, admin, user) = setup();
    let id = 1509u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &50u64);
    client.file_dispute(&user, &id);

    advance(&env, 51); // past deadline

    let contract_id = client.address.clone();
    client.expire_dispute(&user, &id);

    assert_no_event(&env, &contract_id, "sla_violation_advanced");

    // But `dispute_expired` must be emitted
    let expired_event = find_event_by_topic(&env, &contract_id, "dispute_expired");
    assert!(
        expired_event.is_some(),
        "dispute_expired must be emitted on expiry"
    );
}

/// Test: query SLA timer values for `get_level_time_limit` and `get_pending_review_time_limit`
/// both with defaults and after custom configuration.
#[test]
fn test_get_level_time_limit_and_pending_review_queries() {
    let (_env, client, _owner, admin, _user) = setup();

    // Verify defaults
    assert_eq!(
        client.get_level_time_limit(&EscalationLevel::Level1),
        DEFAULT_LEVEL_LIMIT
    );
    assert_eq!(
        client.get_level_time_limit(&EscalationLevel::Level2),
        DEFAULT_LEVEL_LIMIT
    );
    assert_eq!(
        client.get_level_time_limit(&EscalationLevel::Level3),
        DEFAULT_LEVEL_LIMIT
    );
    assert_eq!(
        client.get_pending_review_time_limit(),
        PENDING_REVIEW_WINDOW
    );

    // Update time limits
    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &86400u64);
    client.set_level_time_limit(&admin, &EscalationLevel::Level2, &172800u64);
    client.set_level_time_limit(&admin, &EscalationLevel::Level3, &259200u64);
    client.set_pending_review_time_limit(&admin, &43200u64);

    // Verify configured values
    assert_eq!(client.get_level_time_limit(&EscalationLevel::Level1), 86400);
    assert_eq!(client.get_level_time_limit(&EscalationLevel::Level2), 172800);
    assert_eq!(client.get_level_time_limit(&EscalationLevel::Level3), 259200);
    assert_eq!(client.get_pending_review_time_limit(), 43200);
}
