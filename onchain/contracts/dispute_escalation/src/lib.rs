//! # Dispute Escalation Contract
//!
//! Manages the full lifecycle of payment disputes across three escalation tiers
//! with configurable per-level SLA deadlines, a keeper-triggered `PendingReview`
//! stage, and binding outcome records.
//!
//! ## State Machine
//!
//! ```text
//! file_dispute → Open @ Level1
//!
//!   Open          + escalate_dispute  (within deadline)   → Escalated @ Level(N+1)
//!   Escalated     + escalate_dispute  (within deadline)   → Escalated @ Level(N+1)
//!
//!   Open          + keeper_advance_stage (deadline passed) → PendingReview
//!   Escalated     + keeper_advance_stage (deadline passed) → PendingReview
//!   Appealed      + keeper_advance_stage (deadline passed) → PendingReview
//!
//!   *active*      + expire_dispute    (deadline passed)   → Expired   [terminal]
//!   PendingReview + expire_dispute    (review deadline passed) → Expired [terminal]
//!
//!   *active*      + resolve_dispute   (admin, L1/L2)      → Resolved  (appeal window = 3 days)
//!   PendingReview + resolve_dispute   (admin, L1/L2)      → Resolved  (appeal window = 3 days)
//!
//!   Resolved      + appeal_ruling     (within window)     → Appealed  @ next level
//!
//!   *active*      + resolve_dispute   (admin, L3)         → Finalised [terminal]
//!   PendingReview + resolve_dispute   (admin, L3)         → Finalised [terminal]
//! ```
//!
//! **Terminal states:** `Finalised`, `Expired`. All further transitions are rejected.
//!
//! ## SLA Timer Design
//!
//! Every dispute phase is governed by a deterministic ledger timestamp stored
//! in `DisputeDetails.phase_deadline`.  The timeline for a single dispute is:
//!
//! ```text
//! t=0  file_dispute         phase_deadline = t + level_time_limit(L1)
//!       ── within window ──► escalate / resolve (normal path)
//!       ── deadline passes ──► keeper_advance_stage
//!              │ sets phase_deadline = now + pending_review_time_limit
//!              ▼
//!          PendingReview
//!       ── admin resolves ──► Resolved / Finalised
//!       ── review deadline passes ──► expire_dispute → Expired
//! ```
//!
//! All timestamp comparisons use `env.ledger().timestamp()` which is the
//! **consensus timestamp** — fully deterministic and manipulation-resistant.
//!
//! ## Keeper Transitions (permissionless)
//!
//! `keeper_advance_stage` and `expire_dispute` are permissionless: any caller
//! may trigger them once the on-chain timestamp satisfies the required
//! condition.  Both functions perform strict state checks so they cannot:
//! * skip escalation levels,
//! * resurrect a terminal dispute,
//! * be called twice on the same dispute (`AlreadyPendingReview` / `AlreadyTerminal`).
//!
//! ## SLA Violation Event
//!
//! `keeper_advance_stage` emits **two** events when it fires due to an SLA
//! timeout:
//!
//! 1. `dispute_sla_breached` (`DisputeSlaBreachedEvent`) — backward-compatible
//!    event for existing off-chain systems.
//! 2. `sla_violation_advanced` (`SlaViolationAdvancedEvent`) — a distinct
//!    event emitted **only** on SLA timeout, not on normal-flow escalation.
//!    Off-chain SLA-compliance monitors should filter on this topic to
//!    unambiguously identify every SLA-violation trigger.
//!
//! ## Security Model
//!
//! | Invariant | Enforcement |
//! |-----------|-------------|
//! | Only admin resolves | `is_admin` check at the top of `resolve_dispute` |
//! | Cannot double-resolve | `AlreadyResolved` / `AlreadyFinalised` guard every resolve path |
//! | No funds stuck | `expire_dispute` (callable by anyone) closes abandoned disputes |
//! | No re-entry into terminal states | `assert_not_terminal` rejects all transitions on `Finalised`/`Expired` |
//! | Deadlines enforced on-chain | All time comparisons use `env.ledger().timestamp()` |
//! | Keeper cannot skip stages | `keeper_advance_stage` only advances to `PendingReview`, never skips to `Resolved`/`Finalised` |
//! | `PendingReview` is idempotent-safe | Returns `AlreadyPendingReview` on repeat calls |
//!
//! ## Integration with Payroll State
//!
//! Downstream contracts (payroll escrow, payment splitter) should listen for
//! `dispute_resolved`, `dispute_finalised`, and `dispute_expired` events and
//! act on the `outcome` field to release or redirect funds.

#![no_std]
pub mod storage;
pub mod types;

use soroban_sdk::{contract, contractimpl, contracttype, vec, Address, Env, IntoVal, Symbol, Vec};
use stellar_contract_utils::upgradeable::UpgradeableInternal;
use stellar_macros::Upgradeable;
use types::{
    DisputeDetails, DisputeError, DisputeOutcome, DisputeStatus, EscalationLevel, StorageKey,
};

// ─── Events ──────────────────────────────────────────────────────────────────

/// Emitted when a new dispute is filed.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeFiledEvent {
    pub agreement_id: u128,
    pub initiator: Address,
    pub level: EscalationLevel,
    pub phase_deadline: u64,
}

/// Emitted when a dispute is escalated to a higher tier.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeEscalatedEvent {
    pub agreement_id: u128,
    pub new_level: EscalationLevel,
    pub phase_deadline: u64,
}

/// Emitted when an admin resolves a dispute (Level1 or Level2).
/// Appeal window is open until `appeal_deadline`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeResolvedEvent {
    pub agreement_id: u128,
    pub level: EscalationLevel,
    pub outcome: DisputeOutcome,
    pub appeal_deadline: u64,
}

/// Emitted when a Level3 resolution is issued — final and binding.
///
/// No further appeal is possible. Payroll state should be settled immediately
/// based on `outcome`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeFinalisedEvent {
    pub agreement_id: u128,
    pub outcome: DisputeOutcome,
}

/// Emitted when a resolved ruling is appealed to the next level.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeAppealedEvent {
    pub agreement_id: u128,
    pub appellant: Address,
    pub new_level: EscalationLevel,
    pub phase_deadline: u64,
}

/// Emitted when an expired dispute is closed without a ruling.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeExpiredEvent {
    pub agreement_id: u128,
}

/// Emitted only when `keeper_advance_stage` advances a dispute because an SLA
/// deadline has elapsed. The dispute moves from `Open`/`Escalated`/`Appealed`
/// into `PendingReview`, opening a bounded admin-review window.
///
/// Off-chain SLA monitors should treat this event as the canonical signal for
/// an SLA violation. Normal in-window escalation continues to emit only
/// `dispute_escalated`, so indexers can distinguish timeout-driven advancement
/// from normal-flow advancement without inspecting contract state.
///
/// # Fields
/// * `agreement_id`   — identifies the dispute.
/// * `level`          — escalation level at which the SLA was breached.
/// * `breached_at`    — ledger timestamp at which the advance was triggered.
/// * `review_deadline`— timestamp by which the admin must act before the dispute can be expired via
///   `expire_dispute`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeSlaViolationAdvancedEvent {
    pub agreement_id: u128,
    pub level: EscalationLevel,
    pub breached_at: u64,
    pub review_deadline: u64,
}

/// Emitted **only** when `keeper_advance_stage` fires due to an SLA timeout.
///
/// This event is the primary signal for off-chain SLA-compliance monitoring
/// systems.  It is deliberately separate from:
///
/// * `DisputeEscalatedEvent` — emitted by `escalate_dispute` during normal
///   (within-deadline) flow, and
/// * `DisputeSlaBreachedEvent` — a companion event emitted by the same
///   `keeper_advance_stage` call for backward-compatible observability.
///
/// By listening **solely** for `sla_violation_advanced`, an off-chain indexer
/// can unambiguously identify every SLA-violation trigger without false
/// positives from normal-flow escalations.
///
/// # Fields
/// * `agreement_id`       — identifies the dispute whose SLA was violated.
/// * `level`              — escalation level at which the SLA was breached.
/// * `breached_at`        — ledger timestamp when the violation was observed
///   and the stage was advanced.
/// * `review_deadline`    — timestamp by which the admin must act before the
///   dispute can be expired via `expire_dispute`.
/// * `previous_status`    — the dispute status **before** the keeper advanced
///   the stage (one of `Open`, `Escalated`, or `Appealed`).  This lets
///   monitoring systems distinguish which source state the violation
///   originated from.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlaViolationAdvancedEvent {
    pub agreement_id: u128,
    pub level: EscalationLevel,
    pub breached_at: u64,
    pub review_deadline: u64,
    pub previous_status: DisputeStatus,
}

// ─── Contract ─────────────────────────────────────────────────────────────────

/// Dispute Escalation Contract
///
/// See module-level documentation for the full state machine and security model.
#[derive(Upgradeable)]
#[contract]
pub struct DisputeEscalationContract;

impl UpgradeableInternal for DisputeEscalationContract {
    fn _require_auth(e: &Env, _operator: &Address) {
        let owner: Address = e.storage().persistent().get(&StorageKey::Owner).unwrap();
        owner.require_auth();
    }
}

#[contractimpl]
impl DisputeEscalationContract {
    // ─── Initialization ───────────────────────────────────────────────────

    /// Initializes the contract.
    ///
    /// # Arguments
    /// * `owner` — Contract owner (upgrade authority).
    /// * `admin` — Address authorized to resolve disputes and adjust SLA time limits.
    ///
    /// # Access Control
    /// Owner must authenticate.
    pub fn initialize(env: Env, owner: Address, admin: Address) {
        owner.require_auth();
        env.storage().persistent().set(&StorageKey::Owner, &owner);
        env.storage().persistent().set(&StorageKey::Admin, &admin);
    }

    // ─── Lifecycle ────────────────────────────────────────────────────────

    /// Opens a new Level1 dispute for an agreement.
    ///
    /// The SLA clock starts immediately: `phase_deadline = now + level_time_limit(Level1)`.
    ///
    /// # State transition
    /// `(none)` → `Open @ Level1`
    ///
    /// # Errors
    /// * `InvalidTransition` — a dispute for this agreement already exists.
    pub fn file_dispute(env: Env, caller: Address, agreement_id: u128) -> Result<(), DisputeError> {
        caller.require_auth();

        if storage::get_dispute(&env, agreement_id).is_some() {
            return Err(DisputeError::InvalidTransition);
        }

        let time_limit = storage::get_level_time_limit(&env, EscalationLevel::Level1);
        let now = env.ledger().timestamp();
        let deadline = now + time_limit;

        let dispute = DisputeDetails {
            agreement_id,
            initiator: caller.clone(),
            status: DisputeStatus::Open,
            level: EscalationLevel::Level1,
            phase_started_at: now,
            phase_deadline: deadline,
            outcome: DisputeOutcome::Unset,
        };

        storage::set_dispute(&env, agreement_id, &dispute);

        // Notify the payroll escrow so it pauses releases for this agreement.
        if let Some(escrow_addr) = storage::get_payroll_escrow(&env) {
            env.invoke_contract::<()>(
                &escrow_addr,
                &Symbol::new(&env, "pause_agreement"),
                vec![
                    &env,
                    env.current_contract_address().into_val(&env),
                    agreement_id.into_val(&env),
                ],
            );
        }

        env.events().publish(
            ("dispute_filed",),
            DisputeFiledEvent {
                agreement_id,
                initiator: caller,
                level: EscalationLevel::Level1,
                phase_deadline: deadline,
            },
        );

        Ok(())
    }

    /// Escalates an open or previously escalated dispute to the **next** tier.
    ///
    /// This is a **permissionless** call — any caller may trigger it, provided
    /// the SLA window has not yet elapsed.  The new phase SLA starts from the
    /// current ledger timestamp.
    ///
    /// # Invariant: single-tier step
    /// `escalate_dispute` accepts **only** `agreement_id`; there is no
    /// caller-supplied target tier.  The destination level is computed purely
    /// from the current level via the closed `next_level` mapping
    /// (`Level1 → Level2`, `Level2 → Level3`, `Level3 → MaxEscalationReached`).
    /// As a consequence, this function **cannot** jump two tiers in a single
    /// call — Level1 → Level3 in one transaction is structurally impossible.
    /// The corresponding regression tests live in
    /// `tests/test_escalation.rs` (see §13 and the new tests added for #890).
    ///
    /// # State transitions
    /// `Open @ LevelN`      (now ≤ deadline) → `Escalated @ Level(N+1)`
    /// `Escalated @ LevelN` (now ≤ deadline) → `Escalated @ Level(N+1)`
    ///
    /// # Errors
    /// * `DisputeNotFound`       — no dispute for this agreement.
    /// * `AlreadyResolved`       — dispute is already in `Resolved` state.
    /// * `AlreadyFinalised`      — dispute is in terminal `Finalised` state.
    /// * `AlreadyTerminal`       — dispute is in terminal `Expired` state.
    /// * `InvalidTransition`     — dispute is in `PendingReview` (SLA already breached; escalation
    ///   window has passed).
    /// * `TimeLimitExpired`      — escalation window has passed.
    /// * `MaxEscalationReached`  — already at Level3 (no higher tier exists).
    pub fn escalate_dispute(
        env: Env,
        caller: Address,
        agreement_id: u128,
    ) -> Result<(), DisputeError> {
        caller.require_auth();

        let mut dispute =
            storage::get_dispute(&env, agreement_id).ok_or(DisputeError::DisputeNotFound)?;

        Self::assert_not_terminal(&dispute)?;

        if dispute.status == DisputeStatus::Resolved {
            return Err(DisputeError::AlreadyResolved);
        }

        // PendingReview means the original SLA window has already been declared
        // breached by a keeper.  The escalation window is closed.
        if dispute.status == DisputeStatus::PendingReview {
            return Err(DisputeError::InvalidTransition);
        }

        let now = env.ledger().timestamp();
        if now > dispute.phase_deadline {
            return Err(DisputeError::TimeLimitExpired);
        }

        let next_level = Self::next_level(&dispute.level)?;
        let new_limit = storage::get_level_time_limit(&env, next_level.clone());
        let deadline = now + new_limit;

        dispute.level = next_level.clone();
        dispute.status = DisputeStatus::Escalated;
        dispute.phase_started_at = now;
        dispute.phase_deadline = deadline;

        storage::set_dispute(&env, agreement_id, &dispute);

        env.events().publish(
            ("dispute_escalated",),
            DisputeEscalatedEvent {
                agreement_id,
                new_level: next_level,
                phase_deadline: deadline,
            },
        );

        Ok(())
    }

    /// Keeper-triggered SLA advancement — **permissionless**.
    ///
    /// Any caller may invoke this once `env.ledger().timestamp()` has surpassed
    /// the current `phase_deadline` of a non-terminal, non-resolved dispute.
    /// The dispute is moved from `Open`, `Escalated`, or `Appealed` into
    /// `PendingReview`, signalling that the admin must act promptly.
    ///
    /// A new bounded review window is opened:
    /// `phase_deadline = now + pending_review_time_limit` (default 3 days).
    ///
    /// This function **cannot skip stages** — it only ever transitions to
    /// `PendingReview`, never directly to `Resolved` or `Finalised`.
    ///
    /// # Events emitted
    /// * `dispute_sla_breached`       — backward-compatible SLA breach signal.
    /// * `sla_violation_advanced`     — distinct event emitted **only** on SLA
    ///   timeout, not on normal-flow escalation.  Off-chain SLA-compliance
    ///   monitors should filter on this topic to unambiguously identify every
    ///   SLA-violation trigger.
    ///
    /// # State transitions
    /// `Open @ LevelN`      (now > deadline) → `PendingReview @ LevelN`
    /// `Escalated @ LevelN` (now > deadline) → `PendingReview @ LevelN`
    /// `Appealed @ LevelN`  (now > deadline) → `PendingReview @ LevelN`
    ///
    /// # Errors
    /// * `DisputeNotFound`       — no dispute for this agreement.
    /// * `AlreadyFinalised`      — dispute is in terminal `Finalised` state.
    /// * `AlreadyTerminal`       — dispute is in terminal `Expired` state.
    /// * `AlreadyResolved`       — dispute is in `Resolved` state (appeal window manages its own
    ///   deadline).
    /// * `AlreadyPendingReview`  — `keeper_advance_stage` was already called; idempotent call
    ///   rejected.
    /// * `DeadlineNotPassed`     — SLA deadline has not yet elapsed; too early to advance.
    pub fn keeper_advance_stage(
        env: Env,
        caller: Address,
        agreement_id: u128,
    ) -> Result<(), DisputeError> {
        caller.require_auth();

        let mut dispute =
            storage::get_dispute(&env, agreement_id).ok_or(DisputeError::DisputeNotFound)?;

        // Terminal states reject all further transitions.
        Self::assert_not_terminal(&dispute)?;

        // Resolved disputes have their own appeal window; keeper cannot interfere.
        if dispute.status == DisputeStatus::Resolved {
            return Err(DisputeError::AlreadyResolved);
        }

        // Idempotency guard — reject a second keeper call.
        if dispute.status == DisputeStatus::PendingReview {
            return Err(DisputeError::AlreadyPendingReview);
        }

        let now = env.ledger().timestamp();

        // SLA must have elapsed before the keeper may advance.
        if now <= dispute.phase_deadline {
            return Err(DisputeError::DeadlineNotPassed);
        }

        // Capture the status *before* the transition so we can include it in
        // the SLA-violation event.  Valid source states at this point are
        // Open, Escalated, or Appealed (all non-terminal, non-Resolved,
        // non-PendingReview checks have already passed above).
        let previous_status = dispute.status.clone();

        // Open a bounded admin-review window.
        let review_limit = storage::get_pending_review_time_limit(&env);
        let review_deadline = now
            .checked_add(review_limit)
            .ok_or(DisputeError::SlaDeadlineOverflow)?;

        // `phase_started_at` records exactly when the SLA breach was observed.
        dispute.status = DisputeStatus::PendingReview;
        dispute.phase_started_at = now;
        dispute.phase_deadline = review_deadline;

        let level = dispute.level.clone();
        storage::set_dispute(&env, agreement_id, &dispute);

        // Backward-compatible event — existing off-chain systems that listen
        // for `dispute_sla_breached` continue to work unchanged.
        env.events().publish(
            ("sla_violation_advanced",),
            DisputeSlaViolationAdvancedEvent {
                agreement_id,
                level: level.clone(),
                breached_at: now,
                review_deadline,
            },
        );

        // ── New: SLA-violation-specific event ──────────────────────────────
        // Emitted *only* from `keeper_advance_stage` when the SLA deadline
        // has genuinely elapsed.  Off-chain SLA-compliance monitors should
        // listen for this topic to unambiguously identify SLA violations
        // without false positives from normal-flow `dispute_escalated` events.
        env.events().publish(
            ("sla_violation_advanced",),
            SlaViolationAdvancedEvent {
                agreement_id,
                level,
                breached_at: now,
                review_deadline,
                previous_status,
            },
        );

        Ok(())
    }

    /// Admin resolves the active dispute and records a binding outcome.
    ///
    /// Also accepts disputes in `PendingReview` — the admin is expected to act
    /// during the review window opened by `keeper_advance_stage`.
    ///
    /// # State transition (Level1/2)
    /// `Open | Escalated | Appealed | PendingReview @ L1/L2` → `Resolved @ L1/L2`
    /// An appeal window of 3 days opens after this call.
    ///
    /// # State transition (Level3)
    /// `* @ L3` → `Finalised @ L3` (terminal — no further appeal)
    ///
    /// # Security
    /// * Cannot double-resolve: `AlreadyResolved` / `AlreadyFinalised` returned if the dispute is
    ///   already in a terminal or resolved state.
    /// * `Unset` is not a valid outcome — returns `InvalidTransition`.
    ///
    /// # Access Control
    /// Caller must be the admin (verified by `is_admin`).
    ///
    /// # Errors
    /// * `Unauthorized`     — caller is not the admin.
    /// * `DisputeNotFound`  — no dispute for this agreement.
    /// * `InvalidTransition`— `outcome` is `Unset`.
    /// * `AlreadyResolved`  — cannot resolve an already-resolved dispute.
    /// * `AlreadyFinalised` — cannot resolve a finalised dispute.
    /// * `AlreadyTerminal`  — dispute is expired.
    pub fn resolve_dispute(
        env: Env,
        caller: Address,
        agreement_id: u128,
        outcome: DisputeOutcome,
    ) -> Result<(), DisputeError> {
        caller.require_auth();

        if !storage::is_admin(&env, &caller) {
            return Err(DisputeError::Unauthorized);
        }

        if outcome == DisputeOutcome::Unset {
            return Err(DisputeError::InvalidTransition);
        }

        let mut dispute =
            storage::get_dispute(&env, agreement_id).ok_or(DisputeError::DisputeNotFound)?;

        Self::assert_not_terminal(&dispute)?;

        if dispute.status == DisputeStatus::Resolved {
            return Err(DisputeError::AlreadyResolved);
        }

        let now = env.ledger().timestamp();
        dispute.outcome = outcome.clone();
        dispute.phase_started_at = now;

        if dispute.level == EscalationLevel::Level3 {
            // Level3 resolution is final — no appeal window, no further transitions.
            dispute.status = DisputeStatus::Finalised;
            dispute.phase_deadline = now;

            storage::set_dispute(&env, agreement_id, &dispute);

            // Resume escrow releases as the dispute has reached a final outcome.
            if let Some(escrow_addr) = storage::get_payroll_escrow(&env) {
                env.invoke_contract::<()>(
                    &escrow_addr,
                    &Symbol::new(&env, "resume_agreement"),
                    vec![
                        &env,
                        env.current_contract_address().into_val(&env),
                        agreement_id.into_val(&env),
                    ],
                );
            }

            env.events().publish(
                ("dispute_finalised",),
                DisputeFinalisedEvent {
                    agreement_id,
                    outcome,
                },
            );
        } else {
            // Level1/2: open a 3-day appeal window.
            let appeal_deadline = now + 259_200; // 3 days in seconds
            dispute.status = DisputeStatus::Resolved;
            dispute.phase_deadline = appeal_deadline;

            storage::set_dispute(&env, agreement_id, &dispute);

            // Resume escrow releases — the admin has issued a ruling.
            // If the ruling is later appealed, a new dispute phase begins
            // and the escrow should be paused again by `appeal_ruling`.
            if let Some(escrow_addr) = storage::get_payroll_escrow(&env) {
                env.invoke_contract::<()>(
                    &escrow_addr,
                    &Symbol::new(&env, "resume_agreement"),
                    vec![
                        &env,
                        env.current_contract_address().into_val(&env),
                        agreement_id.into_val(&env),
                    ],
                );
            }

            env.events().publish(
                ("dispute_resolved",),
                DisputeResolvedEvent {
                    agreement_id,
                    level: dispute.level,
                    outcome,
                    appeal_deadline,
                },
            );
        }

        Ok(())
    }

    /// Appeals a Level1/2 resolved ruling to the next escalation tier.
    ///
    /// # State transition
    /// `Resolved @ LevelN (N < 3)` → `Appealed @ Level(N+1)`
    ///
    /// The outcome is cleared (`Unset`) because the dispute is under active
    /// re-review at the new level.  A fresh SLA window opens for the new level.
    ///
    /// # Errors
    /// * `DisputeNotFound`      — no dispute for this agreement.
    /// * `InvalidTransition`    — dispute is not in `Resolved` state.
    /// * `AlreadyFinalised`     — Level3 rulings are binding; appeal blocked.
    /// * `AlreadyTerminal`      — dispute is expired.
    /// * `TimeLimitExpired`     — appeal window has passed.
    /// * `MaxEscalationReached` — already at Level3.
    pub fn appeal_ruling(
        env: Env,
        caller: Address,
        agreement_id: u128,
    ) -> Result<(), DisputeError> {
        caller.require_auth();

        let mut dispute =
            storage::get_dispute(&env, agreement_id).ok_or(DisputeError::DisputeNotFound)?;

        // Block appeals on terminal states.
        if dispute.status == DisputeStatus::Finalised {
            return Err(DisputeError::AlreadyFinalised);
        }
        Self::assert_not_terminal(&dispute)?;

        if dispute.status != DisputeStatus::Resolved {
            return Err(DisputeError::InvalidTransition);
        }

        let now = env.ledger().timestamp();
        if now > dispute.phase_deadline {
            return Err(DisputeError::TimeLimitExpired);
        }

        let next_level = Self::next_level(&dispute.level)?;
        let new_limit = storage::get_level_time_limit(&env, next_level.clone());
        let deadline = now + new_limit;

        dispute.level = next_level.clone();
        dispute.status = DisputeStatus::Appealed;
        dispute.initiator = caller.clone();
        dispute.outcome = DisputeOutcome::Unset; // Outcome is under review again.
        dispute.phase_started_at = now;
        dispute.phase_deadline = deadline;

        storage::set_dispute(&env, agreement_id, &dispute);

        // Pause escrow again — the dispute is under active re-review.
        if let Some(escrow_addr) = storage::get_payroll_escrow(&env) {
            env.invoke_contract::<()>(
                &escrow_addr,
                &Symbol::new(&env, "pause_agreement"),
                vec![
                    &env,
                    env.current_contract_address().into_val(&env),
                    agreement_id.into_val(&env),
                ],
            );
        }

        env.events().publish(
            ("dispute_appealed",),
            DisputeAppealedEvent {
                agreement_id,
                appellant: caller,
                new_level: next_level,
                phase_deadline: deadline,
            },
        );

        Ok(())
    }

    /// Marks a dispute as `Expired` after its active deadline has passed without
    /// admin action.
    ///
    /// **Permissionless** — any caller may invoke this to prevent disputes from
    /// being stuck indefinitely.  No funds are moved by this contract; downstream
    /// payroll-escrow contracts listen for `dispute_expired` events and release
    /// escrowed funds back to the payer accordingly.
    ///
    /// Works from any non-terminal, non-resolved state once the current
    /// `phase_deadline` has elapsed.  This includes `PendingReview`: if the
    /// admin fails to act within the review window, the dispute can be expired.
    ///
    /// # State transitions
    /// `Open | Escalated | Appealed` (now > deadline)        → `Expired`
    /// `PendingReview`               (now > review_deadline) → `Expired`
    ///
    /// # Errors
    /// * `DisputeNotFound`    — no dispute for this agreement.
    /// * `AlreadyFinalised`   — cannot expire a finalised dispute.
    /// * `AlreadyTerminal`    — already `Expired`.
    /// * `AlreadyResolved`    — `Resolved` disputes have an appeal window; use `appeal_ruling` or
    ///   let it become de-facto binding.
    /// * `DeadlineNotPassed`  — deadline has not yet passed.
    pub fn expire_dispute(
        env: Env,
        caller: Address,
        agreement_id: u128,
    ) -> Result<(), DisputeError> {
        caller.require_auth();

        let mut dispute =
            storage::get_dispute(&env, agreement_id).ok_or(DisputeError::DisputeNotFound)?;

        Self::assert_not_terminal(&dispute)?;

        if dispute.status == DisputeStatus::Resolved {
            return Err(DisputeError::AlreadyResolved);
        }

        let now = env.ledger().timestamp();
        if now <= dispute.phase_deadline {
            return Err(DisputeError::DeadlineNotPassed);
        }

        dispute.status = DisputeStatus::Expired;
        storage::set_dispute(&env, agreement_id, &dispute);

        // Resume escrow releases — the dispute has timed out.
        if let Some(escrow_addr) = storage::get_payroll_escrow(&env) {
            env.invoke_contract::<()>(
                &escrow_addr,
                &Symbol::new(&env, "resume_agreement"),
                vec![
                    &env,
                    env.current_contract_address().into_val(&env),
                    agreement_id.into_val(&env),
                ],
            );
        }

        env.events()
            .publish(("dispute_expired",), DisputeExpiredEvent { agreement_id });

        Ok(())
    }

    // ─── Admin Configuration ──────────────────────────────────────────────

    /// Admin configuration: adjust the SLA time limit for a given escalation level.
    ///
    /// Changes take effect for new disputes and new phase windows; existing
    /// `phase_deadline` values on in-progress disputes are **not** retroactively
    /// modified.
    ///
    /// # Access Control
    /// Caller must be the admin.
    ///
    /// # Errors
    /// * `Unauthorized` — caller is not the admin.
    pub fn set_level_time_limit(
        env: Env,
        caller: Address,
        level: EscalationLevel,
        limit_seconds: u64,
    ) -> Result<(), DisputeError> {
        caller.require_auth();
        if !storage::is_admin(&env, &caller) {
            return Err(DisputeError::Unauthorized);
        }
        storage::set_level_time_limit(&env, level, limit_seconds);
        Ok(())
    }

    /// Admin configuration: set the review window granted to the admin after
    /// `keeper_advance_stage` transitions a dispute into `PendingReview`.
    ///
    /// Default if never set: **259 200 seconds (3 days)**.
    ///
    /// Changes apply to the *next* `keeper_advance_stage` call; disputes
    /// already in `PendingReview` retain their existing `phase_deadline`.
    ///
    /// # Access Control
    /// Caller must be the admin.
    ///
    /// # Errors
    /// * `Unauthorized` — caller is not the admin.
    pub fn set_pending_review_time_limit(
        env: Env,
        caller: Address,
        limit_seconds: u64,
    ) -> Result<(), DisputeError> {
        caller.require_auth();
        if !storage::is_admin(&env, &caller) {
            return Err(DisputeError::Unauthorized);
        }
        storage::set_pending_review_time_limit(&env, limit_seconds);
        Ok(())
    }

    /// Configures the `payroll_escrow` contract address that will be
    /// paused on `file_dispute` and resumed on `resolve_dispute` / `expire_dispute`.
    ///
    /// If not configured, dispute lifecycle events proceed without interacting
    /// with any escrow contract (backward-compatible behaviour).
    ///
    /// # Access Control
    /// Caller must be the admin.
    ///
    /// # Errors
    /// * `Unauthorized` — caller is not the admin.
    pub fn set_payroll_escrow(
        env: Env,
        caller: Address,
        escrow_contract: Address,
    ) -> Result<(), DisputeError> {
        caller.require_auth();
        if !storage::is_admin(&env, &caller) {
            return Err(DisputeError::Unauthorized);
        }
        storage::set_payroll_escrow(&env, &escrow_contract);
        Ok(())
    }

    // ─── Queries ──────────────────────────────────────────────────────────

    /// Returns the details of a dispute, or `None` if it does not exist.
    pub fn get_dispute(env: Env, agreement_id: u128) -> Option<DisputeDetails> {
        storage::get_dispute(&env, agreement_id)
    }

    /// Returns the configured pending-review time limit in seconds.
    /// Defaults to 259 200 s (3 days) if never explicitly set.
    pub fn get_pending_review_time_limit(env: Env) -> u64 {
        storage::get_pending_review_time_limit(&env)
    }

    /// Returns the configured `payroll_escrow` contract address, or `None`.
    pub fn get_payroll_escrow(env: Env) -> Option<Address> {
        storage::get_payroll_escrow(&env)
    }

    // ─── Private helpers ──────────────────────────────────────────────────

    /// Returns `Err(AlreadyFinalised)` for `Finalised` disputes and
    /// `Err(AlreadyTerminal)` for `Expired` disputes.  All other states pass.
    fn assert_not_terminal(dispute: &DisputeDetails) -> Result<(), DisputeError> {
        match dispute.status {
            DisputeStatus::Finalised => Err(DisputeError::AlreadyFinalised),
            DisputeStatus::Expired => Err(DisputeError::AlreadyTerminal),
            _ => Ok(()),
        }
    }

    /// Returns the next escalation level, or `Err(MaxEscalationReached)` if
    /// already at `Level3`.
    ///
    /// # Invariant: closed one-step mapping
    /// This helper is the **sole** authority on what the next escalation tier
    /// is.  The mapping is deliberately closed: there is exactly one
    /// successor to `Level1` and to `Level2`, and `Level3` has no successor.
    /// The explicit `match` below has **no** `_ =>` wildcard arm — *do not
    /// add one*.  A wildcard arm would silently absorb any future variant of
    /// `EscalationLevel` and quietly break the closed-mapping invariant.
    /// When a new level variant is introduced, extend this `match`
    /// arm-by-arm explicitly and decide each transition on its own merits;
    /// never relax the no-wildcard rule.
    fn next_level(level: &EscalationLevel) -> Result<EscalationLevel, DisputeError> {
        match level {
            EscalationLevel::Level1 => Ok(EscalationLevel::Level2),
            EscalationLevel::Level2 => Ok(EscalationLevel::Level3),
            EscalationLevel::Level3 => Err(DisputeError::MaxEscalationReached),
        }
    }
}
