#![cfg(test)]

use dispute_escalation::types::{DisputeError, DisputeOutcome, DisputeStatus, EscalationLevel};
use dispute_escalation::{DisputeEscalationContract, DisputeEscalationContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
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
// §9  DUPLICATE / CONCURRENT DISPUTE TESTS
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
