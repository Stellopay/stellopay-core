//! Dispute domain: arbiter configuration, dispute raising and resolution
//! (including the multisig pre-approval path).

use super::grace::effective_cancelled_grace_duration_seconds;
use super::{
    acquire_reentrancy_guard, get_agreement, release_reentrancy_guard, require_multisig_executed,
    OperationKind,
};
use crate::audit::{record_entry, AuditEvent};
use crate::events::{
    emit_dsipute_raised, emit_dsipute_resolved, emit_set_arbiter, ArbiterSetEvent,
    DisputeRaisedEvent, DisputeResolvedEvent,
};
use crate::storage::{
    AgreementStatus, DataKey, DisputeStatus, EmployeeInfo, PayrollError, StorageKey,
};
use soroban_sdk::{panic_with_error, token::Client as TokenClient, Address, Env, Vec};

/// Set Arbiter
///
/// # Arguments
/// * `env` - Contract environment
/// * `caller` - Address of the caller
/// * `arbiter` - Address of the arbiter to add
///
/// # Access Control
/// Requires caller authentication
pub fn set_arbiter(env: &Env, caller: Address, arbiter: Address) -> bool {
    caller.require_auth();

    // Validation: reject self-appointment (caller == arbiter).
    if arbiter == caller {
        panic_with_error!(env, PayrollError::InvalidArbiter);
    }

    // Validation: reject a no-op duplicate (arbiter already set to this address).
    if let Some(existing) = get_arbiter(env) {
        if existing == arbiter {
            panic_with_error!(env, PayrollError::InvalidArbiter);
        }
    }

    let arbiter_for_log = arbiter.clone();
    env.storage()
        .persistent()
        .set(&StorageKey::Arbiter, &arbiter);
    emit_set_arbiter(env, ArbiterSetEvent { arbiter });

    // Record a lifecycle audit entry so `set_arbiter` is observable in the
    // audit trail (contract-level event: sentinel `agreement_id` 0, subject
    // is the newly-set arbiter).
    record_entry(
        env,
        caller,
        AuditEvent::ArbiterSet,
        0,
        Some(arbiter_for_log),
        None,
    );

    true
}

/// Get Arbiter
///
/// # Arguments
/// * `env` - Contract environment
///
/// # Returns
/// Arbiter address if set, None otherwise
pub fn get_arbiter(env: &Env) -> Option<Address> {
    env.storage().persistent().get(&StorageKey::Arbiter)
}

/// Raise a dispute during the grace period (escrow or payroll agreement).
///
/// # Re-filing guard
/// Rejects the call with `DisputeAlreadyRaised` if the agreement already has an
/// **active** dispute (`dispute_status == Raised`).  Once the active dispute is
/// resolved (`dispute_status` transitions to `Resolved`), a fresh dispute may be
/// raised again within the same grace window.
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - Agreement ID to dispute
///
/// # Access Control
/// Requires caller or employee authentication
pub fn raise_dispute(env: &Env, caller: Address, agreement_id: u128) -> Result<(), PayrollError> {
    caller.require_auth();

    let mut agreement = get_agreement(env, agreement_id).ok_or(PayrollError::AgreementNotFound)?;

    let employees: Vec<EmployeeInfo> = env
        .storage()
        .persistent()
        .get(&StorageKey::AgreementEmployees(agreement_id))
        .unwrap_or(Vec::new(env));

    let is_employee = employees.iter().any(|emp| emp.address == caller);

    if caller != agreement.employer && !is_employee {
        return Err(PayrollError::NotParty);
    }

    if agreement.dispute_status == DisputeStatus::Raised {
        return Err(PayrollError::DisputeAlreadyRaised);
    }

    let now = env.ledger().timestamp();
    // Dispute window semantics:
    // - If the agreement is Cancelled, the dispute window is the *cancellation grace period*.
    // - Otherwise, the dispute window is the *creation grace period* (legacy behavior).
    let window_start = if agreement.status == AgreementStatus::Cancelled {
        agreement
            .cancelled_at
            .ok_or(PayrollError::NotInGracePeriod)?
    } else {
        agreement.created_at
    };

    let grace_window_seconds = if agreement.status == AgreementStatus::Cancelled {
        effective_cancelled_grace_duration_seconds(
            env,
            agreement_id,
            agreement.grace_period_seconds,
        )
    } else {
        agreement.grace_period_seconds
    };

    let grace_end = window_start
        .checked_add(grace_window_seconds)
        .ok_or(PayrollError::GraceExtensionInvalid)?;
    if grace_end <= now {
        return Err(PayrollError::NotInGracePeriod);
    }

    agreement.dispute_status = DisputeStatus::Raised;
    agreement.dispute_raised_at = Some(now);
    agreement.status = AgreementStatus::Disputed;

    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);

    emit_dsipute_raised(env, DisputeRaisedEvent { agreement_id });
    record_entry(
        env,
        caller,
        AuditEvent::DisputeRaised,
        agreement_id,
        Some(agreement.employer),
        Some(agreement.total_amount),
    );

    Ok(())
}

/// Resolve a raised dispute: arbiter splits locked funds between employees and employer.
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - Agreement ID in `DisputeStatus::Raised`
/// * `pay_employee` - Total amount to distribute equally across employees (payroll) or to
///   contributor (escrow)
/// * `refund_employer` - Amount to refund the employer
///
/// # Conservation of funds
/// The payout is deterministic and conserves funds. `pay_employee` is divided
/// equally across employees using integer division; the integer-division
/// **remainder (dust)** is added to the **last** employee's transfer so that the
/// sum of all employee transfers equals `pay_employee` exactly and no tokens are
/// stranded in the contract. Both `pay_employee` and `refund_employer` must be
/// non-negative, and `pay_employee + refund_employer` must not exceed either the
/// agreement's `total_amount` or its **real escrow balance** for the agreement's
/// token; the per-agreement escrow balance is decremented by the distributed
/// total after the transfers succeed.
///
/// # Access Control
/// Requires arbiter authentication
pub fn resolve_dispute(
    env: Env,
    caller: Address,
    agreement_id: u128,
    pay_employee: i128,
    refund_employer: i128,
) -> Result<(), PayrollError> {
    acquire_reentrancy_guard(&env)?;
    let result = resolve_dispute_inner(
        env.clone(),
        caller,
        agreement_id,
        pay_employee,
        refund_employer,
    );
    release_reentrancy_guard(&env);
    result
}

fn resolve_dispute_inner(
    env: Env,
    caller: Address,
    agreement_id: u128,
    pay_employee: i128,
    refund_employer: i128,
) -> Result<(), PayrollError> {
    // If a DisputeResolution threshold is configured and the total payout meets
    // it, reject and require the caller to use resolve_dispute_multisig instead.
    let total_payout = pay_employee + refund_employer;
    if let Some(threshold) = env
        .storage()
        .persistent()
        .get::<_, i128>(&StorageKey::DisputeResolutionThreshold)
    {
        if threshold > 0 && total_payout >= threshold {
            return Err(PayrollError::MultisigApprovalRequired);
        }
    }
    resolve_dispute_core(&env, caller, agreement_id, pay_employee, refund_employer)
}

/// Resolves a dispute that has been pre-approved by the multisig contract.
///
/// # Arguments
/// * `caller` - Arbiter address (must authenticate)
/// * `agreement_id` - Agreement under dispute
/// * `pay_employee` - Amount to distribute to employees
/// * `refund_employer` - Amount to refund the employer
/// * `multisig_operation_id` - ID of the Executed DisputeResolution operation in the multisig
///
/// # Access Control
/// Requires arbiter authentication and a valid Executed multisig operation.
pub fn resolve_dispute_multisig(
    env: Env,
    caller: Address,
    agreement_id: u128,
    pay_employee: i128,
    refund_employer: i128,
    multisig_operation_id: u128,
) -> Result<(), PayrollError> {
    acquire_reentrancy_guard(&env)?;
    let result = resolve_dispute_multisig_inner(
        env.clone(),
        caller,
        agreement_id,
        pay_employee,
        refund_employer,
        multisig_operation_id,
    );
    release_reentrancy_guard(&env);
    result
}

fn resolve_dispute_multisig_inner(
    env: Env,
    caller: Address,
    agreement_id: u128,
    pay_employee: i128,
    refund_employer: i128,
    multisig_operation_id: u128,
) -> Result<(), PayrollError> {
    let multisig_addr = env
        .storage()
        .persistent()
        .get::<_, Address>(&StorageKey::MultisigContract)
        .ok_or(PayrollError::MultisigApprovalRequired)?;

    let payroll_contract = env.current_contract_address();
    require_multisig_executed(&env, &multisig_addr, multisig_operation_id, |kind| {
        matches!(
            kind,
            OperationKind::DisputeResolution(addr, aid, pe, re)
                if *addr == payroll_contract
                    && *aid == agreement_id
                    && *pe == pay_employee
                    && *re == refund_employer
        )
    })?;

    resolve_dispute_core(&env, caller, agreement_id, pay_employee, refund_employer)
}

fn resolve_dispute_core(
    env: &Env,
    caller: Address,
    agreement_id: u128,
    pay_employee: i128,
    refund_employer: i128,
) -> Result<(), PayrollError> {
    caller.require_auth();

    let arbiter = env
        .storage()
        .persistent()
        .get::<_, Address>(&StorageKey::Arbiter)
        .ok_or(PayrollError::NotArbiter)?;
    if caller != arbiter {
        return Err(PayrollError::NotArbiter);
    }

    let mut agreement = get_agreement(env, agreement_id).ok_or(PayrollError::AgreementNotFound)?;

    if agreement.dispute_status != DisputeStatus::Raised {
        return Err(PayrollError::NoDispute);
    }

    // Reject negative payouts (griefing / accounting corruption).
    if pay_employee < 0 || refund_employer < 0 {
        return Err(PayrollError::InvalidPayout);
    }

    let total_payout = pay_employee
        .checked_add(refund_employer)
        .ok_or(PayrollError::InvalidPayout)?;

    // Validate against the agreement's nominal total AND, when a real
    // per-agreement escrow balance is tracked, against that balance too.
    // Validating only against `total_amount` could desync internal accounting
    // from actual token balances and over-distribute across disputes.
    //
    // `escrow_balance == 0` is treated as "untracked" (e.g. legacy agreements
    // whose escrow was never recorded via `set_agreement_escrow_balance`); for
    // those we fall back to the `total_amount` bound and do not decrement, to
    // avoid driving a never-tracked balance negative.
    let total_locked = agreement.total_amount;
    if total_payout > total_locked {
        return Err(PayrollError::InvalidPayout);
    }
    let escrow_balance = DataKey::get_agreement_escrow_balance(env, agreement_id, &agreement.token);
    let escrow_tracked = escrow_balance > 0;
    if escrow_tracked && total_payout > escrow_balance {
        return Err(PayrollError::InvalidPayout);
    }

    let token = TokenClient::new(env, &agreement.token);

    let employees: Vec<EmployeeInfo> = env
        .storage()
        .persistent()
        .get(&StorageKey::AgreementEmployees(agreement_id))
        .unwrap_or(Vec::new(env));

    // Track what is actually transferred out so the escrow balance can be
    // decremented by the exact distributed total (conservation of funds).
    let mut distributed: i128 = 0;

    // Execute transfers. The integer-division remainder (dust) is allocated
    // deterministically to the LAST employee so the employee transfers sum to
    // `pay_employee` exactly and nothing is stranded.
    if pay_employee > 0 {
        let num_employees = employees.len() as i128;
        if num_employees > 0 {
            let amount_per_employee = pay_employee / num_employees;
            let dust = pay_employee - amount_per_employee * num_employees;
            let last_index = employees.len() - 1;
            for (i, employee) in employees.iter().enumerate() {
                let mut amount = amount_per_employee;
                if i as u32 == last_index {
                    amount += dust;
                }
                if amount > 0 {
                    token.transfer(&env.current_contract_address(), &employee.address, &amount);
                    distributed += amount;
                }
            }
        }
        // If there are no employees, `pay_employee` cannot be distributed and is
        // left as part of the (unchanged) escrow rather than silently lost.
    }

    if refund_employer > 0 {
        token.transfer(
            &env.current_contract_address(),
            &agreement.employer,
            &refund_employer,
        );
        distributed += refund_employer;
    }

    // Decrement the per-agreement escrow balance by exactly what was distributed,
    // keeping internal accounting consistent with real token balances. Skipped
    // when escrow was never tracked (balance was 0) so it is not driven negative.
    if escrow_tracked {
        DataKey::set_agreement_escrow_balance(
            env,
            agreement_id,
            &agreement.token,
            escrow_balance - distributed,
        );
    }

    agreement.dispute_status = DisputeStatus::Resolved;
    agreement.status = AgreementStatus::Completed;
    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);

    emit_dsipute_resolved(
        env,
        DisputeResolvedEvent {
            agreement_id,
            pay_contributor: pay_employee,
            refund_employer,
        },
    );
    record_entry(
        env,
        caller,
        AuditEvent::DisputeResolved,
        agreement_id,
        Some(agreement.employer),
        Some(pay_employee + refund_employer),
    );

    Ok(())
}

/// Retrieves current dispute status for an agreement by ID
///
/// # Returns
/// Some(Agreement) if found, None otherwise
pub fn get_dispute_status(env: Env, agreement_id: u128) -> DisputeStatus {
    get_agreement(&env, agreement_id)
        .map(|a| a.dispute_status)
        .unwrap_or(DisputeStatus::None)
}
