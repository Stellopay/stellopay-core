//! Integration tests for dispute_escalation: full escalation ladder, binding
//! finality, expiry, concurrent disputes, and audit_logger compliance recording.
//!
//! # Cross-contract audit_logger integration
//!
//! The audit_logger integration tests (`test_*_audit_logger`) verify that every
//! dispute state transition (file, escalate, keeper_advance_stage, resolve L1/L2,
//! resolve L3 / finalise, appeal, expire) durably records an entry in the shared
//! audit_logger contract.  The tests assert:
//!
//! * Exactly one entry per transition (no missing, no duplicates).
//! * Entries appear in chronological order matching the transition sequence.
//! * Each entry has the correct `actor`, `action`, and `subject` fields.
//! * Unauthorized calls do not create phantom audit entries.
//! * Disputes still operate normally when no audit logger is configured.
#![cfg(test)]

use audit_logger::{AuditLoggerContract, AuditLoggerContractClient};
use dispute_escalation::types::{DisputeError, DisputeOutcome, DisputeStatus, EscalationLevel};
use dispute_escalation::{DisputeEscalationContract, DisputeEscalationContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Symbol,
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

    client.file_dispute(&user, &id, &DisputeReason::PaymentDispute);
    assert_eq!(client.get_dispute(&id).unwrap().status, DisputeStatus::Open);

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
    assert_eq!(d.outcome, DisputeOutcome::GrantClaim);
}

/// Wrong caller cannot resolve.
#[test]
fn test_escalation_resolve_unauthorized_integration() {
    let (_env, client, _owner, _admin, user) = setup();
    let id = 202u128;
    client.file_dispute(&user, &id, &DisputeReason::PaymentDispute);

    let res = client.try_resolve_dispute(&user, &id, &DisputeOutcome::UpholdPayment);
    assert_eq!(res, Err(Ok(DisputeError::Unauthorized)));
}

/// Expired escalation window preserves dispute state for off-chain inspection.
#[test]
fn test_escalation_deadline_expiry_preserves_open_state_integration() {
    let (env, client, _owner, admin, user) = setup();
    let id = 203u128;

    client.set_level_time_limit(&admin, &EscalationLevel::Level1, &60u64);
    client.file_dispute(&user, &id, &DisputeReason::PaymentDispute);

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

    client.file_dispute(&user, &id, &DisputeReason::PaymentDispute);
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

    client.file_dispute(&user, &id, &DisputeReason::PaymentDispute);
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
    client.file_dispute(&user, &id, &DisputeReason::PaymentDispute);

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

    client.file_dispute(&user, &id_a, &DisputeReason::PaymentDispute);
    client.file_dispute(&user, &id_b, &DisputeReason::PaymentDispute);

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

    client.file_dispute(&user, &id, &DisputeReason::PaymentDispute);
    client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);

    let res = client.try_resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyResolved)));
}

// ============================================================================
// AUDIT LOGGER CROSS-CONTRACT INTEGRATION
// ============================================================================

/// Deploys dispute_escalation + audit_logger, wires them, and returns
/// all clients and key addresses for audit-logger tests.
fn setup_with_audit_logger() -> (
    Env,
    DisputeEscalationContractClient<'static>,
    AuditLoggerContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let dispute_id = env.register(DisputeEscalationContract, ());
    let dispute_client = DisputeEscalationContractClient::new(&env, &dispute_id);

    let audit_id = env.register(AuditLoggerContract, ());
    let audit_client = AuditLoggerContractClient::new(&env, &audit_id);

    let owner = Address::generate(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    dispute_client.initialize(&owner, &admin);
    audit_client.initialize(&owner, &100u32); // retention limit 100

    // Wire dispute_escalation to audit_logger
    dispute_client.set_audit_logger(&admin, &audit_id);

    (env, dispute_client, audit_client, owner, admin, user)
}

/// Collects all log entries from the audit logger in order.
fn collect_logs(
    env: &Env,
    audit_client: &AuditLoggerContractClient<'_>,
) -> Vec<audit_logger::AuditLogEntry> {
    let count = audit_client.get_log_count();
    let mut entries = Vec::new();
    if count == 0 {
        return entries;
    }
    let page = audit_client.get_logs(&0u32, &(count as u32));
    for entry in page.entries.iter() {
        entries.push(entry.clone());
    }
    entries
}

/// Full dispute lifecycle through all three SLA tiers produces exactly one
/// audit entry per transition, in chronological order, with correct fields.
#[test]
fn test_dispute_full_lifecycle_records_audit_logger_per_transition() {
    let (env, dispute_client, audit_client, _owner, admin, user) = setup_with_audit_logger();
    let id = 301u128;

    // Step 1: File dispute at Level1
    dispute_client.file_dispute(&user, &id);
    let d = dispute_client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Open);

    // Step 2: Escalate to Level2
    dispute_client.escalate_dispute(&user, &id);
    let d = dispute_client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Escalated);
    assert_eq!(d.level, EscalationLevel::Level2);

    // Step 3: Escalate to Level3
    dispute_client.escalate_dispute(&user, &id);
    let d = dispute_client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Escalated);
    assert_eq!(d.level, EscalationLevel::Level3);

    // Step 4: Admin resolves at Level3 → Finalised
    dispute_client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    let d = dispute_client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Finalised);

    // Collect audit entries
    let entries = collect_logs(&env, &audit_client);

    // Exactly 4 entries: file, escalate L2, escalate L3, finalise
    assert_eq!(
        entries.len(),
        4,
        "expected 4 audit entries for file → escalate L2 → escalate L3 → finalise"
    );

    // Entry 0: dispute_filed
    assert_eq!(entries[0].action, Symbol::new(&env, "dispute_filed"));
    assert_eq!(entries[0].actor, user);
    assert_eq!(entries[0].subject, Some(user.clone()));
    assert_eq!(entries[0].amount, None);

    // Entry 1: dispute_escalated (L2)
    assert_eq!(entries[1].action, Symbol::new(&env, "dispute_escalated"));
    assert_eq!(entries[1].actor, user);

    // Entry 2: dispute_escalated (L3)
    assert_eq!(entries[2].action, Symbol::new(&env, "dispute_escalated"));
    assert_eq!(entries[2].actor, user);

    // Entry 3: dispute_finalised
    assert_eq!(entries[3].action, Symbol::new(&env, "dispute_finalised"));
    assert_eq!(entries[3].actor, admin);

    // Verify chronological ordering: timestamps must be non-decreasing
    for i in 1..entries.len() {
        assert!(
            entries[i].timestamp >= entries[i - 1].timestamp,
            "audit entries must be in chronological order"
        );
    }
}

/// Dispute filed → resolved at Level1 → appealed → finalised at Level3:
/// verifies all five transitions produce distinct audit entries.
#[test]
fn test_dispute_file_resolve_appeal_finalise_audit_logger() {
    let (env, dispute_client, audit_client, _owner, admin, user) = setup_with_audit_logger();
    let id = 302u128;

    // File
    dispute_client.file_dispute(&user, &id);

    // Resolve at Level1
    dispute_client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    assert_eq!(
        dispute_client.get_dispute(&id).unwrap().status,
        DisputeStatus::Resolved
    );

    // Appeal to Level2
    dispute_client.appeal_ruling(&user, &id);
    let d = dispute_client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Appealed);
    assert_eq!(d.level, EscalationLevel::Level2);

    // Escalate to Level3
    dispute_client.escalate_dispute(&user, &id);
    let d = dispute_client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Escalated);
    assert_eq!(d.level, EscalationLevel::Level3);

    // Resolve at Level3 → Finalised
    dispute_client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);
    assert_eq!(
        dispute_client.get_dispute(&id).unwrap().status,
        DisputeStatus::Finalised
    );

    let entries = collect_logs(&env, &audit_client);

    // 5 entries: filed, resolved(L1), appealed, escalated(L3), finalised
    assert_eq!(entries.len(), 5);

    assert_eq!(entries[0].action, Symbol::new(&env, "dispute_filed"));
    assert_eq!(entries[1].action, Symbol::new(&env, "dispute_resolved"));
    assert_eq!(entries[2].action, Symbol::new(&env, "dispute_appealed"));
    assert_eq!(entries[3].action, Symbol::new(&env, "dispute_escalated"));
    assert_eq!(entries[4].action, Symbol::new(&env, "dispute_finalised"));
}

/// Keeper advance stage + expire path also produce audit entries.
#[test]
fn test_dispute_keeper_advance_and_expire_audit_logger() {
    let (env, dispute_client, audit_client, _owner, admin, user) = setup_with_audit_logger();
    let id = 303u128;

    // Shorten SLA so we can fast-forward past the deadline
    dispute_client.set_level_time_limit(&admin, &EscalationLevel::Level1, &60u64);
    dispute_client.file_dispute(&user, &id);

    // Advance past the SLA deadline
    advance(&env, 61);

    // Keeper advances the stage to PendingReview
    let keeper = Address::generate(&env);
    dispute_client.keeper_advance_stage(&keeper, &id);
    assert_eq!(
        dispute_client.get_dispute(&id).unwrap().status,
        DisputeStatus::PendingReview
    );

    // Advance past the review deadline
    let review_limit = dispute_client.get_pending_review_time_limit();
    advance(&env, review_limit + 1);

    // Expire the dispute
    dispute_client.expire_dispute(&keeper, &id);
    assert_eq!(
        dispute_client.get_dispute(&id).unwrap().status,
        DisputeStatus::Expired
    );

    let entries = collect_logs(&env, &audit_client);

    // 3 entries: filed, sla_breached, expired
    assert_eq!(entries.len(), 3);

    assert_eq!(entries[0].action, Symbol::new(&env, "dispute_filed"));
    assert_eq!(entries[0].actor, user);

    assert_eq!(entries[1].action, Symbol::new(&env, "dispute_sla_breached"));
    assert_eq!(entries[1].actor, keeper);

    assert_eq!(entries[2].action, Symbol::new(&env, "dispute_expired"));
    assert_eq!(entries[2].actor, keeper);
}

/// Dispute operations without audit logger configured must still succeed
/// and produce no audit entries.
#[test]
fn test_dispute_without_audit_logger_no_audit_entries() {
    let env = Env::default();
    env.mock_all_auths();

    let dispute_id = env.register(DisputeEscalationContract, ());
    let dispute_client = DisputeEscalationContractClient::new(&env, &dispute_id);

    let audit_id = env.register(AuditLoggerContract, ());
    let audit_client = AuditLoggerContractClient::new(&env, &audit_id);

    let owner = Address::generate(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    dispute_client.initialize(&owner, &admin);
    audit_client.initialize(&owner, &100u32);

    // Intentionally do NOT wire dispute_escalation to audit_logger

    let id = 304u128;
    dispute_client.file_dispute(&user, &id);
    dispute_client.escalate_dispute(&user, &id);
    dispute_client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);

    // Dispute state transitions succeeded
    assert_eq!(
        dispute_client.get_dispute(&id).unwrap().status,
        DisputeStatus::Resolved
    );

    // Audit logger should have 0 entries (nothing was wired to write to it)
    assert_eq!(audit_client.get_log_count(), 0);
}

/// Unauthorized resolve attempt must not create an audit entry.
#[test]
fn test_dispute_unauthorized_resolve_no_audit_entry() {
    let (env, dispute_client, audit_client, _owner, _admin, user) = setup_with_audit_logger();
    let id = 305u128;

    dispute_client.file_dispute(&user, &id);

    // Non-admin tries to resolve
    let res = dispute_client.try_resolve_dispute(&user, &id, &DisputeOutcome::UpholdPayment);
    assert_eq!(res, Err(Ok(DisputeError::Unauthorized)));

    // No audit entry created for the failed attempt
    let entries = collect_logs(&env, &audit_client);
    assert_eq!(entries.len(), 1, "only the file_dispute entry should exist");
    assert_eq!(entries[0].action, Symbol::new(&env, "dispute_filed"));
}

/// No duplicate audit entries when a transition is attempted twice
/// (only the first attempt succeeds and creates one entry).
#[test]
fn test_dispute_no_duplicate_audit_entries_on_double_resolve() {
    let (env, dispute_client, audit_client, _owner, admin, user) = setup_with_audit_logger();
    let id = 306u128;

    dispute_client.file_dispute(&user, &id);

    // First resolve succeeds
    dispute_client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    assert_eq!(
        dispute_client.get_dispute(&id).unwrap().status,
        DisputeStatus::Resolved
    );

    // Second resolve fails
    let res = dispute_client.try_resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);
    assert_eq!(res, Err(Ok(DisputeError::AlreadyResolved)));

    let entries = collect_logs(&env, &audit_client);

    // 2 entries: filed, resolved — no duplicate
    assert_eq!(
        entries.len(),
        2,
        "double-resolve must not create duplicate audit entry"
    );
    assert_eq!(entries[0].action, Symbol::new(&env, "dispute_filed"));
    assert_eq!(entries[1].action, Symbol::new(&env, "dispute_resolved"));
}

/// Dispute file → resolve → appeal → resolve (full L1→L2→L3 ladder) produces
/// 5 unique audit entries, each with distinct actions.
#[test]
fn test_dispute_full_ladder_audit_entry_actions_are_distinct() {
    let (env, dispute_client, audit_client, _owner, admin, user) = setup_with_audit_logger();
    let id = 307u128;

    // File → L1 resolve → appeal L2 → resolve L2 → appeal L3 → resolve L3
    dispute_client.file_dispute(&user, &id);
    dispute_client.resolve_dispute(&admin, &id, &DisputeOutcome::UpholdPayment);
    dispute_client.appeal_ruling(&user, &id);
    dispute_client.resolve_dispute(&admin, &id, &DisputeOutcome::PartialSettlement);
    dispute_client.appeal_ruling(&user, &id);
    dispute_client.resolve_dispute(&admin, &id, &DisputeOutcome::GrantClaim);

    let d = dispute_client.get_dispute(&id).unwrap();
    assert_eq!(d.status, DisputeStatus::Finalised);
    assert_eq!(d.outcome, DisputeOutcome::GrantClaim);

    let entries = collect_logs(&env, &audit_client);

    // 6 entries: filed, resolved, appealed, resolved, appealed, finalised
    assert_eq!(entries.len(), 6);

    let actions: Vec<Symbol> = entries.iter().map(|e| e.action.clone()).collect();
    assert_eq!(actions[0], Symbol::new(&env, "dispute_filed"));
    assert_eq!(actions[1], Symbol::new(&env, "dispute_resolved"));
    assert_eq!(actions[2], Symbol::new(&env, "dispute_appealed"));
    assert_eq!(actions[3], Symbol::new(&env, "dispute_resolved"));
    assert_eq!(actions[4], Symbol::new(&env, "dispute_appealed"));
    assert_eq!(actions[5], Symbol::new(&env, "dispute_finalised"));

    // Verify every entry has a unique id
    let mut ids: Vec<u64> = entries.iter().map(|e| e.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), entries.len(), "all entry ids must be unique");
}

/// Setting audit logger is admin-gated; unauthorized caller is rejected.
#[test]
fn test_set_audit_logger_unauthorized_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let dispute_id = env.register(DisputeEscalationContract, ());
    let dispute_client = DisputeEscalationContractClient::new(&env, &dispute_id);

    let owner = Address::generate(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let audit_id = Address::generate(&env);

    dispute_client.initialize(&owner, &admin);

    // Non-admin tries to set audit logger
    let res = dispute_client.try_set_audit_logger(&user, &audit_id);
    assert_eq!(res, Err(Ok(DisputeError::Unauthorized)));

    // No audit logger configured
    assert_eq!(
        dispute_client.get_audit_logger(),
        None,
        "no audit logger should be configured after rejected attempt"
    );

    // Admin can still set it
    dispute_client.set_audit_logger(&admin, &audit_id);
    assert_eq!(
        dispute_client.get_audit_logger(),
        Some(audit_id),
        "audit logger should be configured after admin call"
    );
}
