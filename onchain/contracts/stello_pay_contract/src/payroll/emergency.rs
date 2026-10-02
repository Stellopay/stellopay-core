//! Emergency domain: per-agreement pause/resume, employer-wide bulk
//! pause/unpause and the guardian-driven emergency pause.

use super::get_agreement;
use crate::events::{
    emit_agreement_paused, emit_agreement_resumed, emit_bulk_agreements_paused,
    emit_bulk_agreements_unpaused, AgreementPausedEvent, AgreementResumedEvent,
    BulkAgreementsPausedEvent, BulkAgreementsUnpausedEvent,
};
use crate::storage::{AgreementStatus, MilestoneKey, PayrollError, StorageKey};
use soroban_sdk::{panic_with_error, Address, Env, Vec};

/// Pauses an active agreement, preventing further claims until resumed.
///
/// # Arguments
///
/// * `env` - Contract environment used to authenticate the employer and update the stored agreement.
/// * `agreement_id` - ID of the agreement to pause.
///
/// # Preconditions
///
/// * The agreement must be in `AgreementStatus::Active`.
/// * The caller must be the agreement's employer (`employer.require_auth()`).
///
/// # State Transition
///
/// `Active` → `Paused`
///
/// # Access Control
///
/// Requires employer authentication via `Address::require_auth`.
///
/// # Postconditions
///
/// * `agreement.status` is set to `AgreementStatus::Paused`.
/// * The updated agreement is persisted to durable storage.
/// * Claims (both payroll and time-based) are blocked until the agreement is resumed.
///
/// # Panics
///
/// * If the agreement is not in `Active` status (includes already-Paused, Created, Cancelled,
///   Completed, or Disputed agreements).
///
/// # Emits
///
/// * [`AgreementPausedEvent`] with the `agreement_id`.
pub fn pause_agreement(env: &Env, agreement_id: u128) -> Result<(), PayrollError> {
    let mut agreement = get_agreement(env, agreement_id).ok_or(PayrollError::AgreementNotFound)?;

    agreement.employer.require_auth();

    if agreement.status != AgreementStatus::Active {
        return Err(PayrollError::AgreementPaused);
    }

    agreement.status = AgreementStatus::Paused;

    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);

    emit_agreement_paused(env, AgreementPausedEvent { agreement_id });

    Ok(())
}

/// Resumes a paused agreement, allowing claims to be processed again.
///
/// # Arguments
///
/// * `env` - Contract environment used to authenticate the employer and update the stored agreement.
/// * `agreement_id` - ID of the agreement to resume.
///
/// # Preconditions
///
/// * The agreement must be in `AgreementStatus::Paused`.
/// * The caller must be the agreement's employer (`employer.require_auth()`).
///
/// # State Transition
///
/// `Paused` → `Active`
///
/// # Access Control
///
/// Requires employer authentication via `Address::require_auth`.
///
/// # Postconditions
///
/// * `agreement.status` is set to `AgreementStatus::Active`.
/// * The updated agreement is persisted to durable storage.
/// * Claims can be processed again.
/// * All agreement data (employees, amounts, timestamps) is preserved.
///
/// # Panics
///
/// * If the agreement is not in `Paused` status (includes Active, Created, Cancelled, Completed,
///   or Disputed agreements).
///
/// # Emits
///
/// * [`AgreementResumedEvent`] with the `agreement_id`.
pub fn resume_agreement(env: &Env, agreement_id: u128) {
    let mut agreement = get_agreement(env, agreement_id)
        .unwrap_or_else(|| panic_with_error!(env, PayrollError::AgreementNotFound));

    agreement.employer.require_auth();

    assert!(
        agreement.status == AgreementStatus::Paused,
        "Can only resume Paused agreements"
    );

    agreement.status = AgreementStatus::Active;

    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);

    emit_agreement_resumed(env, AgreementResumedEvent { agreement_id });
}

/// Pauses a milestone-based agreement, preventing claims
///
/// # Arguments
/// * `env` - Contract environment used to authenticate the employer and update the stored agreement
///   status.
/// * `agreement_id` - ID of the milestone agreement to pause; must resolve to existing employer and
///   status records.
///
/// # Returns
/// No value. Emits `AgreementPausedEvent` after writing the paused status.
///
/// # State Transition
/// Active -> Paused, or Created -> Paused (if has approved milestones)
///
/// # Access Control
/// Requires employer authentication
///
/// # Requirements
/// - Agreement must be in Active status, or Created status with approved milestones
/// - Only the employer can pause the agreement
///
/// # Note
/// Milestone agreements can be paused in Created status if they have approved milestones
/// that could be claimed, effectively making them "active" for claiming purposes.
///
/// # Errors
/// * `PayrollError::AgreementNotFound` — the milestone agreement does not exist.
/// * `PayrollError::MilestoneAgreementInvalidStatus` — the agreement is not in `Active` or
///   `Created` status.
pub fn pause_milestone_agreement(env: Env, agreement_id: u128) -> Result<(), PayrollError> {
    let employer: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Employer(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    employer.require_auth();

    let status: AgreementStatus = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Status(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;

    // Allow pausing Active agreements, or Created agreements (which can have claimable milestones)
    if status != AgreementStatus::Active && status != AgreementStatus::Created {
        return Err(PayrollError::MilestoneAgreementInvalidStatus);
    }

    env.storage().persistent().set(
        &MilestoneKey::Status(agreement_id),
        &AgreementStatus::Paused,
    );

    AgreementPausedEvent { agreement_id }.publish(&env);

    Ok(())
}

/// Resumes a paused milestone-based agreement, allowing claims again
///
/// # Arguments
/// * `env` - Contract environment used to authenticate the employer and update the stored agreement
///   status.
/// * `agreement_id` - ID of the paused milestone agreement to resume; must resolve to existing
///   employer and status records.
///
/// # Returns
/// No value. Emits `AgreementResumedEvent` after writing the active status.
///
/// # State Transition
/// Paused -> Active (or Paused -> Created if it was Created before)
///
/// # Access Control
/// Requires employer authentication
///
/// # Requirements
/// - Agreement must be in Paused status
/// - Only the employer can resume the agreement
///
/// # Note
/// Resumed milestone agreements return to Active status. If they were Created before
/// pausing, they will be Active after resuming (allowing milestone claims).
///
/// # Errors
/// * `PayrollError::AgreementNotFound` — the milestone agreement does not exist.
/// * `PayrollError::MilestoneAgreementInvalidStatus` — the agreement is not in `Paused` status.
pub fn resume_milestone_agreement(env: Env, agreement_id: u128) -> Result<(), PayrollError> {
    let employer: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Employer(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    employer.require_auth();

    let status: AgreementStatus = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Status(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;

    if status != AgreementStatus::Paused {
        return Err(PayrollError::MilestoneAgreementInvalidStatus);
    }

    // Resume to Active status (milestone agreements can have claimable milestones in Active state)
    env.storage().persistent().set(
        &MilestoneKey::Status(agreement_id),
        &AgreementStatus::Active,
    );

    AgreementResumedEvent { agreement_id }.publish(&env);

    Ok(())
}

// -----------------------------------------------------------------------------
// Bulk pause / unpause (employer-scoped)
// -----------------------------------------------------------------------------

/// Pauses all active agreements belonging to the caller.
///
/// Iterates over every agreement ID stored under `EmployerAgreements(employer)`,
/// skipping any that are not currently in a pausable state.  Emits individual
/// [`AgreementPausedEvent`] for each affected agreement followed by a single
/// [`EmployerAgreementsPausedEvent`] summarising the total count.
///
/// # Arguments
/// * `env`      - Contract environment.
/// * `employer` - Address whose agreements should be paused.  Must authenticate.
///
/// # Returns
/// `Ok(u32)` — the number of agreements that were actually paused.
///
/// # Access Control
/// `employer.require_auth()` is enforced before any state is read or written.
pub fn pause_employer_agreements(env: &Env, employer: Address) -> Result<u32, PayrollError> {
    employer.require_auth();

    let key = StorageKey::EmployerAgreements(employer.clone());
    let agreements: Vec<u128> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env));

    let mut paused_count: u32 = 0;

    for agreement_id in agreements.iter() {
        // Try as payroll / escrow agreement
        if let Some(mut agreement) = get_agreement(env, agreement_id) {
            if agreement.status == AgreementStatus::Active {
                agreement.status = AgreementStatus::Paused;
                env.storage()
                    .persistent()
                    .set(&StorageKey::Agreement(agreement_id), &agreement);
                emit_agreement_paused(env, AgreementPausedEvent { agreement_id });
                paused_count += 1;
            }
        } else {
            // Try as milestone agreement
            let stored_employer: Option<Address> = env
                .storage()
                .persistent()
                .get(&MilestoneKey::Employer(agreement_id));
            if stored_employer.is_some_and(|e| e == employer) {
                let status: Option<AgreementStatus> = env
                    .storage()
                    .persistent()
                    .get(&MilestoneKey::Status(agreement_id));
                if let Some(s) = status {
                    if s == AgreementStatus::Active || s == AgreementStatus::Created {
                        env.storage().persistent().set(
                            &MilestoneKey::Status(agreement_id),
                            &AgreementStatus::Paused,
                        );
                        AgreementPausedEvent { agreement_id }.publish(env);
                        paused_count += 1;
                    }
                }
            }
        }
    }

    if paused_count > 0 {
        emit_bulk_agreements_paused(
            env,
            BulkAgreementsPausedEvent {
                employer,
                count: paused_count,
            },
        );
    }

    Ok(paused_count)
}

/// Unpauses all paused agreements belonging to the caller.
///
/// Iterates over every agreement ID stored under `EmployerAgreements(employer)`,
/// skipping any that are not currently paused.  Emits individual
/// [`AgreementResumedEvent`] for each affected agreement followed by a single
/// [`EmployerAgreementsResumedEvent`] summarising the total count.
///
/// # Arguments
/// * `env`      - Contract environment.
/// * `employer` - Address whose agreements should be unpaused.  Must authenticate.
///
/// # Returns
/// `Ok(u32)` — the number of agreements that were actually unpaused.
///
/// # Access Control
/// `employer.require_auth()` is enforced before any state is read or written.
pub fn unpause_employer_agreements(env: &Env, employer: Address) -> Result<u32, PayrollError> {
    employer.require_auth();

    let key = StorageKey::EmployerAgreements(employer.clone());
    let agreements: Vec<u128> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env));

    let mut unpaused_count: u32 = 0;

    for agreement_id in agreements.iter() {
        // Try as payroll / escrow agreement
        if let Some(mut agreement) = get_agreement(env, agreement_id) {
            if agreement.status == AgreementStatus::Paused {
                agreement.status = AgreementStatus::Active;
                env.storage()
                    .persistent()
                    .set(&StorageKey::Agreement(agreement_id), &agreement);
                emit_agreement_resumed(env, AgreementResumedEvent { agreement_id });
                unpaused_count += 1;
            }
        } else {
            // Try as milestone agreement
            let stored_employer: Option<Address> = env
                .storage()
                .persistent()
                .get(&MilestoneKey::Employer(agreement_id));
            if stored_employer.is_some_and(|e| e == employer) {
                let status: Option<AgreementStatus> = env
                    .storage()
                    .persistent()
                    .get(&MilestoneKey::Status(agreement_id));
                if status == Some(AgreementStatus::Paused) {
                    env.storage().persistent().set(
                        &MilestoneKey::Status(agreement_id),
                        &AgreementStatus::Active,
                    );
                    AgreementResumedEvent { agreement_id }.publish(env);
                    unpaused_count += 1;
                }
            }
        }
    }

    if unpaused_count > 0 {
        emit_bulk_agreements_unpaused(
            env,
            BulkAgreementsUnpausedEvent {
                employer,
                count: unpaused_count,
            },
        );
    }

    Ok(unpaused_count)
}

// ============================================================================
// Emergency Pause Functions
// ============================================================================

/// Checks if contract is in emergency pause state
pub fn is_emergency_paused(env: &Env) -> bool {
    env.storage()
        .persistent()
        .get::<StorageKey, crate::storage::EmergencyPause>(&StorageKey::EmergencyPause)
        .map(|p| p.is_paused)
        .unwrap_or(false)
}

/// Adds emergency guardians (multi-sig addresses)
///
/// # Arguments
/// * `env` - Contract environment
/// * `guardians` - Vector of guardian addresses
///
/// # Access Control
/// Requires owner authentication
pub fn set_emergency_guardians(env: &Env, guardians: Vec<Address>) {
    let owner = match crate::contract_owner(env) {
        Ok(owner) => owner,
        Err(error) => panic_with_error!(env, error),
    };
    owner.require_auth();
    env.storage()
        .persistent()
        .set(&StorageKey::EmergencyGuardians, &guardians);
}

/// Gets emergency guardians
pub fn get_emergency_guardians(env: &Env) -> Option<Vec<Address>> {
    env.storage()
        .persistent()
        .get(&StorageKey::EmergencyGuardians)
}

/// Proposes emergency pause with timelock
///
/// # Arguments
/// * `env` - Contract environment
/// * `caller` - Guardian proposing the pause
/// * `timelock_seconds` - Delay before pause activates (0 for immediate)
///
/// # Access Control
/// Requires guardian authentication
pub fn propose_emergency_pause(
    env: &Env,
    caller: Address,
    timelock_seconds: u64,
) -> Result<(), PayrollError> {
    caller.require_auth();

    let guardians: Vec<Address> = env
        .storage()
        .persistent()
        .get(&StorageKey::EmergencyGuardians)
        .ok_or(PayrollError::NotGuardian)?;

    if !guardians.iter().any(|g| g == caller) {
        return Err(PayrollError::NotGuardian);
    }

    let timelock_end = if timelock_seconds > 0 {
        Some(env.ledger().timestamp() + timelock_seconds)
    } else {
        None
    };

    let pause_state = crate::storage::EmergencyPause {
        is_paused: false,
        paused_at: None,
        paused_by: Some(caller.clone()),
        timelock_end,
    };

    env.storage()
        .persistent()
        .set(&StorageKey::PendingPause, &pause_state);

    let mut approvals: Vec<Address> = Vec::new(env);
    approvals.push_back(caller);
    env.storage()
        .persistent()
        .set(&StorageKey::PauseApprovals, &approvals);

    Ok(())
}

/// Approves pending emergency pause
///
/// # Arguments
/// * `env` - Contract environment
/// * `caller` - Guardian approving the pause
///
/// # Access Control
/// Requires guardian authentication
pub fn approve_emergency_pause(env: &Env, caller: Address) -> Result<(), PayrollError> {
    caller.require_auth();

    let guardians: Vec<Address> = env
        .storage()
        .persistent()
        .get(&StorageKey::EmergencyGuardians)
        .ok_or(PayrollError::NotGuardian)?;

    if !guardians.iter().any(|g| g == caller) {
        return Err(PayrollError::NotGuardian);
    }

    let mut approvals: Vec<Address> = env
        .storage()
        .persistent()
        .get(&StorageKey::PauseApprovals)
        .unwrap_or(Vec::new(env));

    if approvals.iter().any(|a| a == caller) {
        return Ok(());
    }

    approvals.push_back(caller);
    env.storage()
        .persistent()
        .set(&StorageKey::PauseApprovals, &approvals);

    let threshold = (guardians.len() / 2) + 1;
    if approvals.len() >= threshold {
        execute_emergency_pause(env)?;
    }

    Ok(())
}

/// Executes emergency pause after approval threshold met
fn execute_emergency_pause(env: &Env) -> Result<(), PayrollError> {
    let mut pending: crate::storage::EmergencyPause = env
        .storage()
        .persistent()
        .get(&StorageKey::PendingPause)
        .ok_or(PayrollError::Unauthorized)?;

    if let Some(timelock_end) = pending.timelock_end {
        if env.ledger().timestamp() < timelock_end {
            return Err(PayrollError::TimelockActive);
        }
    }

    pending.is_paused = true;
    pending.paused_at = Some(env.ledger().timestamp());

    env.storage()
        .persistent()
        .set(&StorageKey::EmergencyPause, &pending);
    env.storage().persistent().remove(&StorageKey::PendingPause);
    env.storage()
        .persistent()
        .remove(&StorageKey::PauseApprovals);

    Ok(())
}

/// Immediately activates emergency pause (owner only)
///
/// # Arguments
/// * `env` - Contract environment
///
/// # Access Control
/// Requires owner authentication
pub fn emergency_pause(env: &Env) -> Result<(), PayrollError> {
    let owner = crate::contract_owner(env)?;
    owner.require_auth();

    let pause_state = crate::storage::EmergencyPause {
        is_paused: true,
        paused_at: Some(env.ledger().timestamp()),
        paused_by: Some(owner),
        timelock_end: None,
    };

    env.storage()
        .persistent()
        .set(&StorageKey::EmergencyPause, &pause_state);

    Ok(())
}

/// Unpauses contract after emergency resolved
///
/// # Arguments
/// * `env` - Contract environment
///
/// # Access Control
/// Requires owner authentication
pub fn emergency_unpause(env: &Env) -> Result<(), PayrollError> {
    let owner = crate::contract_owner(env)?;
    owner.require_auth();

    let pause_state = crate::storage::EmergencyPause {
        is_paused: false,
        paused_at: None,
        paused_by: None,
        timelock_end: None,
    };

    env.storage()
        .persistent()
        .set(&StorageKey::EmergencyPause, &pause_state);

    Ok(())
}

/// Gets emergency pause state
pub fn get_emergency_pause_state(env: &Env) -> Option<crate::storage::EmergencyPause> {
    env.storage().persistent().get(&StorageKey::EmergencyPause)
}
