//! # Dispute Escalation Contract
//!
//! Manages the full lifecycle of payment disputes across three escalation tiers
//! with configurable per-level deadlines and binding outcome records.
//!
//! ## State Machine
//!
//! ```text
//! file_dispute → Open
//!   Open       + escalate_dispute  (within deadline)  → Escalated
//!   *active*   + expire_dispute    (deadline passed)  → Expired   [terminal]
//!   *active*   + resolve_dispute   (admin, L1/L2)     → Resolved
//!   Resolved   + appeal_ruling     (within window)    → Appealed  (next level)
//!   *active*   + resolve_dispute   (admin, L3)        → Finalised [terminal]
//! ```
//!
//! Terminal states `Finalised` and `Expired` reject all further transitions.
//!
//! ## Security Model
//!
//! * Only the **admin** can resolve disputes.
//! * Only the **admin** can adjust per-level time limits.
//! * Anyone can call `expire_dispute` after the deadline — prevents stuck disputes.
//! * `resolve_dispute` is idempotent-safe: `AlreadyResolved` / `AlreadyFinalised`
//!   guard against double-resolution.
//! * Cannot file a second dispute on the same `agreement_id` while one is active.
//!
//! ## Integration with Payroll State
//!
//! Downstream contracts (payroll escrow, payment splitter) should listen for
//! the `dispute_resolved` and `dispute_finalised` events and act on the
//! `outcome` field to release or redirect funds.

#![no_std]
pub mod storage;
pub mod types;

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};
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
    /// Initializes the contract.
    ///
    /// # Arguments
    /// * `owner` — Contract owner (upgrade authority).
    /// * `admin` — Address authorized to resolve disputes and adjust time limits.
    ///
    /// # Access Control
    /// Owner must authenticate.
    pub fn initialize(env: Env, owner: Address, admin: Address) {
        owner.require_auth();
        env.storage().persistent().set(&StorageKey::Owner, &owner);
        env.storage().persistent().set(&StorageKey::Admin, &admin);
    }

    /// Opens a new Level1 dispute for an agreement.
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

    /// Escalates an open or previously appealed dispute to the next tier.
    ///
    /// # State transition
    /// `Open | Appealed @ LevelN` → `Escalated @ Level(N+1)`
    ///
    /// # Errors
    /// * `DisputeNotFound`       — no dispute for this agreement.
    /// * `AlreadyResolved`       — dispute is already in `Resolved` state.
    /// * `AlreadyFinalised`      — dispute is in terminal `Finalised` state.
    /// * `AlreadyTerminal`       — dispute is in terminal `Expired` state.
    /// * `TimeLimitExpired`      — escalation window has passed.
    /// * `MaxEscalationReached`  — already at Level3.
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

    /// Admin resolves the active dispute and records a binding outcome.
    ///
    /// # State transition (Level1/2)
    /// `Open | Escalated | Appealed @ L1/L2` → `Resolved @ L1/L2`
    /// An appeal window of 3 days opens after this call.
    ///
    /// # State transition (Level3)
    /// `* @ L3` → `Finalised @ L3` (terminal — no further appeal)
    ///
    /// # Security
    /// Cannot double-resolve: `AlreadyResolved` / `AlreadyFinalised` returned
    /// if the dispute is already in a terminal or resolved state.
    ///
    /// # Access Control
    /// Caller must be the admin.
    ///
    /// # Errors
    /// * `Unauthorized`     — caller is not the admin.
    /// * `DisputeNotFound`  — no dispute for this agreement.
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

            env.events().publish(
                ("dispute_finalised",),
                DisputeFinalisedEvent {
                    agreement_id,
                    outcome,
                },
            );
        } else {
            // Level1/2: open a 3-day appeal window.
            let appeal_deadline = now + 259_200; // 3 days
            dispute.status = DisputeStatus::Resolved;
            dispute.phase_deadline = appeal_deadline;

            storage::set_dispute(&env, agreement_id, &dispute);

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

    /// Marks a dispute as `Expired` after its deadline has passed without action.
    ///
    /// Anyone may call this to prevent disputes from being stuck indefinitely.
    /// No funds are moved; downstream contracts use the `dispute_expired` event
    /// to release escrowed funds back to the payer.
    ///
    /// # State transition
    /// `Open | Escalated | Appealed` (deadline passed) → `Expired`
    ///
    /// # Errors
    /// * `DisputeNotFound`    — no dispute for this agreement.
    /// * `AlreadyTerminal`    — already `Expired` or `Finalised`.
    /// * `AlreadyResolved`    — `Resolved` disputes have an appeal window, not expired.
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

        env.events().publish(
            ("dispute_expired",),
            DisputeExpiredEvent { agreement_id },
        );

        Ok(())
    }

    /// Admin configuration: adjust the time limit for a given escalation level.
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

    /// Returns the details of a dispute, or `None` if it does not exist.
    pub fn get_dispute(env: Env, agreement_id: u128) -> Option<DisputeDetails> {
        storage::get_dispute(&env, agreement_id)
    }

    // ─── Private helpers ──────────────────────────────────────────────────────

    /// Returns `Err(AlreadyTerminal)` if the dispute is in a terminal state,
    /// and `Err(AlreadyFinalised)` if finalised.
    fn assert_not_terminal(dispute: &DisputeDetails) -> Result<(), DisputeError> {
        match dispute.status {
            DisputeStatus::Finalised => Err(DisputeError::AlreadyFinalised),
            DisputeStatus::Expired => Err(DisputeError::AlreadyTerminal),
            _ => Ok(()),
        }
    }

    /// Returns the next escalation level, or `Err(MaxEscalationReached)`.
    fn next_level(level: &EscalationLevel) -> Result<EscalationLevel, DisputeError> {
        match level {
            EscalationLevel::Level1 => Ok(EscalationLevel::Level2),
            EscalationLevel::Level2 => Ok(EscalationLevel::Level3),
            EscalationLevel::Level3 => Err(DisputeError::MaxEscalationReached),
        }
    }
}
