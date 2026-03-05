#![no_std]
pub mod storage;
pub mod types;

use soroban_sdk::{contract, contractimpl, Address, Env};
use stellar_contract_utils::upgradeable::UpgradeableInternal;
use stellar_macros::Upgradeable;
use types::{DisputeDetails, DisputeError, DisputeStatus, EscalationLevel, StorageKey};

/// Dispute Escalation Contract
///
/// Manages the lifecycle of payment disputes across multiple escalation levels.
/// It enforces strict time limits for appealing to a higher level.
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
    /// Initialize the contract
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `owner` - Contract owner address
    /// * `admin` - Admin address for the dispute system
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn initialize(env: Env, owner: Address, admin: Address) {
        owner.require_auth();
        env.storage().persistent().set(&StorageKey::Owner, &owner);
        env.storage().persistent().set(&StorageKey::Admin, &admin);
    }

    /// Open a new Level 1 dispute.
    ///
    /// # Arguments
    /// * `caller` - The initiator of the dispute (must be authenticated)
    /// * `agreement_id` - ID of the agreement in dispute
    ///
    /// # Returns
    /// Result containing unit on success, or DisputeError
    pub fn file_dispute(env: Env, caller: Address, agreement_id: u128) -> Result<(), DisputeError> {
        caller.require_auth();

        let existing = storage::get_dispute(&env, agreement_id);
        if existing.is_some() {
            // Can't open if already exists; must use escalate or appeal.
            return Err(DisputeError::InvalidTransition);
        }

        let time_limit = storage::get_level_time_limit(&env, EscalationLevel::Level1);
        let now = env.ledger().timestamp();

        let dispute = DisputeDetails {
            agreement_id,
            initiator: caller,
            status: DisputeStatus::Open,
            level: EscalationLevel::Level1,
            phase_started_at: now,
            phase_deadline: now + time_limit,
        };

        storage::set_dispute(&env, agreement_id, &dispute);
        Ok(())
    }

    /// Escalate an existing dispute to the next level.
    ///
    /// # Arguments
    /// * `caller` - The caller performing the escalation (must be authenticated)
    /// * `agreement_id` - ID of the active dispute to escalate
    ///
    /// # Requirements
    /// - Dispute must exist and not be in a Resolved state.
    /// - Must be within the time limit.
    ///
    /// # Returns
    /// Result<(), DisputeError>
    ///
    /// # Errors
    /// Returns an error if validation fails
    pub fn escalate_dispute(
        env: Env,
        caller: Address,
        agreement_id: u128,
    ) -> Result<(), DisputeError> {
        caller.require_auth();

        let mut dispute =
            storage::get_dispute(&env, agreement_id).ok_or(DisputeError::DisputeNotFound)?;

        if dispute.status == DisputeStatus::Resolved {
            return Err(DisputeError::AlreadyResolved);
        }

        let now = env.ledger().timestamp();
        if now > dispute.phase_deadline {
            return Err(DisputeError::TimeLimitExpired);
        }

        let next_level = match dispute.level {
            EscalationLevel::Level1 => EscalationLevel::Level2,
            EscalationLevel::Level2 => EscalationLevel::Level3,
            EscalationLevel::Level3 => return Err(DisputeError::MaxEscalationReached),
        };

        let new_limit = storage::get_level_time_limit(&env, next_level.clone());
        dispute.level = next_level;
        dispute.status = DisputeStatus::Escalated;
        dispute.phase_started_at = now;
        dispute.phase_deadline = now + new_limit;

        storage::set_dispute(&env, agreement_id, &dispute);
        Ok(())
    }

    /// Appeal a resolved ruling to a higher level.
    ///
    /// # Arguments
    /// * `caller` - Initiator of the appeal
    /// * `agreement_id` - ID of the agreement with a resolved dispute
    ///
    /// # Requirements
    /// - Dispute must be resolved.
    /// - Must be within the appeal window of the previous level.
    ///
    /// # Returns
    /// Result<(), DisputeError>
    ///
    /// # Errors
    /// Returns an error if validation fails
    pub fn appeal_ruling(
        env: Env,
        caller: Address,
        agreement_id: u128,
    ) -> Result<(), DisputeError> {
        caller.require_auth();

        let mut dispute =
            storage::get_dispute(&env, agreement_id).ok_or(DisputeError::DisputeNotFound)?;

        if dispute.status != DisputeStatus::Resolved {
            return Err(DisputeError::InvalidTransition);
        }

        let now = env.ledger().timestamp();
        if now > dispute.phase_deadline {
            return Err(DisputeError::TimeLimitExpired);
        }

        let next_level = match dispute.level {
            EscalationLevel::Level1 => EscalationLevel::Level2,
            EscalationLevel::Level2 => EscalationLevel::Level3,
            EscalationLevel::Level3 => return Err(DisputeError::MaxEscalationReached),
        };

        let new_limit = storage::get_level_time_limit(&env, next_level.clone());
        dispute.level = next_level;
        dispute.status = DisputeStatus::Appealed;
        dispute.initiator = caller; // Tracker appellant
        dispute.phase_started_at = now;
        dispute.phase_deadline = now + new_limit;

        storage::set_dispute(&env, agreement_id, &dispute);
        Ok(())
    }

    /// Resolves an active dispute at the current level.
    ///
    /// # Arguments
    /// * `caller` - Authorized admin or arbiter
    /// * `agreement_id` - Dispute agreement ID
    ///
    /// # Requirements
    /// - Caller must be an admin.
    ///
    /// # Returns
    /// Result<(), DisputeError>
    ///
    /// # Errors
    /// Returns an error if validation fails
    pub fn resolve_dispute(
        env: Env,
        caller: Address,
        agreement_id: u128,
    ) -> Result<(), DisputeError> {
        caller.require_auth();

        if !storage::is_admin(&env, &caller) {
            return Err(DisputeError::Unauthorized);
        }

        let mut dispute =
            storage::get_dispute(&env, agreement_id).ok_or(DisputeError::DisputeNotFound)?;

        if dispute.status == DisputeStatus::Resolved {
            return Err(DisputeError::AlreadyResolved);
        }

        // Set status to resolved, and update the appeal deadline for this level
        dispute.status = DisputeStatus::Resolved;

        let now = env.ledger().timestamp();
        // Give 3 days to appeal minimum (or the remaining time limit)
        dispute.phase_started_at = now;
        dispute.phase_deadline = now + 259200; // 3 days

        storage::set_dispute(&env, agreement_id, &dispute);
        Ok(())
    }

    /// Admin configuration to adjust time limits for a given escalation level.
    ///
    /// # Arguments
    /// * `caller` - caller parameter
    /// * `level` - level parameter
    /// * `limit_seconds` - limit_seconds parameter
    ///
    /// # Returns
    /// Result<(), DisputeError>
    ///
    /// # Errors
    /// Returns an error if validation fails
    ///
    /// # Access Control
    /// Requires caller authentication
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

    /// Fetch details for a specific dispute
    ///
    /// # Arguments
    /// * `agreement_id` - agreement_id parameter
    ///
    /// # Returns
    /// `Option<DisputeDetails>`
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_dispute(env: Env, agreement_id: u128) -> Option<DisputeDetails> {
        storage::get_dispute(&env, agreement_id)
    }
}
