//! Grace period domain: extension policy, cancellation, grace-window checks
//! and finalization/refund.

use super::{get_agreement, is_emergency_paused};
use crate::audit::{record_entry, AuditEvent};
use crate::events::{
    emit_agreement_cancelled, emit_grace_period_extended, emit_grace_period_finalized,
    AgreementCancelledEvent, GracePeriodExtendedEvent, GracePeriodFinalizedEvent,
};
use crate::storage::{
    AgreementStatus, DataKey, GracePeriodExtensionPolicy, PayrollError, StorageKey,
};
use soroban_sdk::{
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    panic_with_error, token, Address, Env, IntoVal, Symbol, Val, Vec,
};

fn grace_period_extension_seconds(env: &Env, agreement_id: u128) -> u64 {
    env.storage()
        .persistent()
        .get(&StorageKey::GracePeriodExtensionSeconds(agreement_id))
        .unwrap_or(0u64)
}

pub(super) fn effective_cancelled_grace_duration_seconds(
    env: &Env,
    agreement_id: u128,
    base_grace_seconds: u64,
) -> u64 {
    base_grace_seconds.saturating_add(grace_period_extension_seconds(env, agreement_id))
}

/// Returns the owner-configured extension caps (defaults apply until `set_grace_extension_policy`
/// runs).
pub fn get_grace_extension_policy(env: &Env) -> GracePeriodExtensionPolicy {
    env.storage()
        .persistent()
        .get(&StorageKey::GracePeriodExtensionPolicy)
        .unwrap_or(GracePeriodExtensionPolicy {
            max_cumulative_extension_bps: 10_000,
            max_extension_per_call_seconds: 90 * 24 * 3600,
        })
}

/// Sets caps for per-agreement grace extensions. Callable only by the contract owner.
///
/// # Policy semantics
/// Both policy fields must be strictly positive. A zero value is treated as an
/// invalid configuration (not "disabled"), because a zero cap silently disables
/// all grace extensions and removes a safety mechanism with no error:
/// - `max_cumulative_extension_bps == 0` would make every extension exceed the (zero) cumulative
///   cap, so no extension could ever be applied.
/// - `max_extension_per_call_seconds == 0` would reject every single-call extension.
///
/// To intentionally stop allowing extensions, set the caps to a small, explicit
/// non-zero value rather than zero, so the configuration choice is auditable and
/// cannot happen by accident. Both fields are also bounded above so a compromised
/// owner key cannot configure absurd values in one transaction.
///
/// Returns [`PayrollError::GraceExtensionInvalid`] when either field is zero or
/// exceeds its upper bound.
pub fn set_grace_extension_policy(
    env: &Env,
    caller: Address,
    policy: GracePeriodExtensionPolicy,
) -> Result<(), PayrollError> {
    caller.require_auth();
    let owner = crate::contract_owner(env)?;
    if caller != owner {
        return Err(PayrollError::Unauthorized);
    }
    // Sanity bounds so a compromised owner key cannot configure absurd values in one tx.
    const MAX_BPS: u32 = 500_000;
    const MAX_PER_CALL: u64 = 730 * 24 * 3600;
    // Reject zero on both fields: a zero cap silently disables grace extensions,
    // which must be an explicit, non-zero configuration rather than an accident.
    if policy.max_cumulative_extension_bps == 0 || policy.max_cumulative_extension_bps > MAX_BPS {
        return Err(PayrollError::GraceExtensionInvalid);
    }
    if policy.max_extension_per_call_seconds == 0
        || policy.max_extension_per_call_seconds > MAX_PER_CALL
    {
        return Err(PayrollError::GraceExtensionInvalid);
    }
    env.storage()
        .persistent()
        .set(&StorageKey::GracePeriodExtensionPolicy, &policy);
    Ok(())
}

/// Extra seconds applied on top of the agreement's base grace (cancellation / dispute window).
pub fn get_grace_extension_seconds(env: &Env, agreement_id: u128) -> u64 {
    grace_period_extension_seconds(env, agreement_id)
}

/// Extends the effective cancellation grace (claims, dispute window while cancelled) by
/// `additional_seconds`.
///
/// Authorization: contract owner or agreement employer. Emits [`GracePeriodExtendedEvent`].
pub fn extend_grace_period(
    env: &Env,
    caller: Address,
    agreement_id: u128,
    additional_seconds: u64,
) -> Result<(), PayrollError> {
    caller.require_auth();
    if is_emergency_paused(env) {
        return Err(PayrollError::EmergencyPaused);
    }
    if additional_seconds == 0 {
        return Err(PayrollError::GraceExtensionInvalid);
    }

    let agreement = get_agreement(env, agreement_id).ok_or(PayrollError::AgreementNotFound)?;
    if agreement.status != AgreementStatus::Cancelled {
        return Err(PayrollError::GraceExtensionInvalid);
    }

    let owner = crate::contract_owner(env)?;
    let extended_by_owner = caller == owner;
    if !extended_by_owner && caller != agreement.employer {
        return Err(PayrollError::Unauthorized);
    }

    let policy = get_grace_extension_policy(env);
    if additional_seconds > policy.max_extension_per_call_seconds {
        return Err(PayrollError::GraceExtensionInvalid);
    }

    let current = grace_period_extension_seconds(env, agreement_id);
    let new_total = current
        .checked_add(additional_seconds)
        .ok_or(PayrollError::GraceExtensionInvalid)?;

    let base = agreement.grace_period_seconds as u128;
    let max_extra = (base.saturating_mul(policy.max_cumulative_extension_bps as u128)) / 10000;

    if (new_total as u128) > max_extra {
        return Err(PayrollError::GraceExtensionCapExceeded);
    }

    env.storage().persistent().set(
        &StorageKey::GracePeriodExtensionSeconds(agreement_id),
        &new_total,
    );

    emit_grace_period_extended(
        env,
        GracePeriodExtendedEvent {
            agreement_id,
            additional_seconds,
            total_extension_seconds: new_total,
            extended_by_owner,
        },
    );

    Ok(())
}

// -----------------------------------------------------------------------------
// Grace Period and Cancellation
// -----------------------------------------------------------------------------

/// Cancels an agreement, initiating the grace period and blocking new claims after expiry.
///
/// # Arguments
///
/// * `env` - Contract environment used to authenticate the employer and update the stored agreement.
/// * `agreement_id` - ID of the agreement to cancel.
///
/// # Preconditions
///
/// * The agreement must be in `AgreementStatus::Active` or `AgreementStatus::Created`.
/// * The caller must be the agreement's employer (`employer.require_auth()`).
///
/// # State Transition
///
/// `Active` or `Created` → `Cancelled`
///
/// # Access Control
///
/// Requires employer authentication via `Address::require_auth`.
///
/// # Postconditions
///
/// * `agreement.status` is set to `AgreementStatus::Cancelled`.
/// * `agreement.cancelled_at` is set to the current ledger timestamp.
/// * The updated agreement is persisted to durable storage.
/// * Claims are still allowed during the grace period but blocked after expiry.
/// * Refunds are prevented until the grace period expires and `finalize_grace_period` is called.
///
/// # Panics
///
/// * If the agreement is not in `Active` or `Created` status (includes already-Cancelled, Paused,
///   Completed, or Disputed agreements).
///
/// # Emits
///
/// * [`AgreementCancelledEvent`] with the `agreement_id`.
/// * An audit entry of type `AuditEvent::AgreementCancelled` for the employer.
pub fn cancel_agreement(env: &Env, agreement_id: u128) {
    let mut agreement = get_agreement(env, agreement_id)
        .unwrap_or_else(|| panic_with_error!(env, PayrollError::AgreementNotFound));

    agreement.employer.require_auth();

    assert!(
        agreement.status == AgreementStatus::Active || agreement.status == AgreementStatus::Created,
        "Can only cancel Active or Created agreements"
    );

    agreement.status = AgreementStatus::Cancelled;
    agreement.cancelled_at = Some(env.ledger().timestamp());

    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);

    emit_agreement_cancelled(env, AgreementCancelledEvent { agreement_id });
    record_entry(
        env,
        agreement.employer,
        AuditEvent::AgreementCancelled,
        agreement_id,
        None,
        Some(agreement.total_amount),
    );
}

/// Finalizes the grace period and allows refund of remaining balance.
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the agreement
///
/// # Requirements
/// - Agreement must be in Cancelled status
/// - Grace period must have expired
/// - Caller must be the employer
///
/// # Behavior
/// - Refunds remaining escrow balance to employer
/// - Marks agreement as ready for finalization
pub fn finalize_grace_period(env: &Env, agreement_id: u128) {
    let agreement = get_agreement(env, agreement_id)
        .unwrap_or_else(|| panic_with_error!(env, PayrollError::AgreementNotFound));

    agreement.employer.require_auth();

    assert!(
        agreement.status == AgreementStatus::Cancelled,
        "Agreement must be cancelled"
    );

    let cancelled_at = agreement
        .cancelled_at
        .unwrap_or_else(|| panic_with_error!(env, PayrollError::InvalidData));

    let current_time = env.ledger().timestamp();
    let effective_grace = effective_cancelled_grace_duration_seconds(
        env,
        agreement_id,
        agreement.grace_period_seconds,
    );
    let grace_end = cancelled_at
        .checked_add(effective_grace)
        .unwrap_or_else(|| panic_with_error!(env, PayrollError::InvalidData));

    assert!(
        current_time >= grace_end,
        "Grace period has not expired yet"
    );

    // Idempotency guard: if already finalized, return early as a no-op
    // rather than re-executing the refund or re-emitting the event.
    if DataKey::is_agreement_grace_period_finalized(env, agreement_id) {
        return;
    }

    // Refund remaining balance using escrow contract if available
    // For now, we'll use the existing escrow balance tracking
    let escrow_balance = DataKey::get_agreement_escrow_balance(env, agreement_id, &agreement.token);

    if escrow_balance > 0 {
        // Token `transfer(from=contract_address, ...)` requires contract auth.
        // Mirror the same `authorize_as_current_contract` pattern used in claim paths.
        let contract_address = env.current_contract_address();
        let token_client = token::Client::new(env, &agreement.token);
        env.authorize_as_current_contract(Vec::from_array(
            env,
            [InvokerContractAuthEntry::Contract(SubContractInvocation {
                context: ContractContext {
                    contract: agreement.token.clone(),
                    fn_name: Symbol::new(env, "transfer"),
                    args: Vec::<Val>::from_array(
                        env,
                        [
                            contract_address.clone().into_val(env),
                            agreement.employer.clone().into_val(env),
                            escrow_balance.into_val(env),
                        ],
                    ),
                },
                sub_invocations: Vec::new(env),
            })],
        ));
        token_client.transfer(&contract_address, &agreement.employer, &escrow_balance);

        // Clear escrow balance
        DataKey::set_agreement_escrow_balance(env, agreement_id, &agreement.token, 0);
    }

    emit_grace_period_finalized(env, GracePeriodFinalizedEvent { agreement_id });
    DataKey::set_agreement_grace_period_finalized(env, agreement_id);
}

/// Checks if the grace period is currently active for a cancelled agreement.
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the agreement
///
/// # Returns
/// true if grace period is active, false otherwise
pub fn is_grace_period_active(env: &Env, agreement_id: u128) -> bool {
    let agreement = match get_agreement(env, agreement_id) {
        Some(agreement) => agreement,
        None => return false,
    };

    if agreement.status != AgreementStatus::Cancelled {
        return false;
    }

    let cancelled_at = match agreement.cancelled_at {
        Some(timestamp) => timestamp,
        None => return false,
    };

    let current_time = env.ledger().timestamp();
    let effective_grace = effective_cancelled_grace_duration_seconds(
        env,
        agreement_id,
        agreement.grace_period_seconds,
    );
    let grace_end = match cancelled_at.checked_add(effective_grace) {
        Some(t) => t,
        None => return false,
    };

    current_time < grace_end
}

/// Gets the grace period end timestamp for a cancelled agreement.
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the agreement
///
/// # Returns
/// Some(timestamp) if agreement is cancelled, None otherwise
pub fn get_grace_period_end(env: &Env, agreement_id: u128) -> Option<u64> {
    let agreement = get_agreement(env, agreement_id)?;

    if agreement.status != AgreementStatus::Cancelled {
        return None;
    }

    let cancelled_at = agreement.cancelled_at?;
    let effective_grace = effective_cancelled_grace_duration_seconds(
        env,
        agreement_id,
        agreement.grace_period_seconds,
    );
    cancelled_at.checked_add(effective_grace)
}
