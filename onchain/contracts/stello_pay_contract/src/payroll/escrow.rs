//! Escrow agreement domain: creation, batch creation and time-based
//! contributor claims.

use super::{
    add_to_employer_agreements, get_agreement, get_next_agreement_id, is_emergency_paused,
    is_grace_period_active,
};
use crate::events::{
    emit_agreement_created, emit_employee_added, emit_payment_received, emit_payment_sent,
    AgreementCreatedEvent, EmployeeAddedEvent, PaymentReceivedEvent, PaymentSentEvent,
};
use crate::storage::{
    Agreement, AgreementMode, AgreementStatus, BatchEscrowCreateResult, DataKey, DisputeStatus,
    EmployeeInfo, EscrowCreateParams, EscrowCreateResult, PayrollError, StorageKey, MAX_BATCH_SIZE,
};
use soroban_sdk::{
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    panic_with_error, token, Address, Env, IntoVal, Symbol, Val, Vec,
};

/// Creates an escrow agreement for a single contributor
///
/// # Arguments
/// * `env` - Contract environment
/// * `employer` - Address of the employer
/// * `contributor` - Address of the contributor
/// * `token` - Token address for payments
/// * `amount_per_period` - Payment amount per period
/// * `period_seconds` - Duration of each period; must be strictly positive or the call returns `ZeroPeriodDuration`
/// * `num_periods` - Number of periods
///
/// # Returns
/// Agreement ID
///
/// # Access Control
/// Requires employer authentication
pub fn create_escrow_agreement(
    env: &Env,
    employer: Address,
    contributor: Address,
    token: Address,
    amount_per_period: i128,
    period_seconds: u64,
    num_periods: u32,
) -> Result<u128, PayrollError> {
    employer.require_auth();
    create_escrow_agreement_internal(
        env,
        employer,
        contributor,
        token,
        amount_per_period,
        period_seconds,
        num_periods,
    )
}

fn create_escrow_agreement_internal(
    env: &Env,
    employer: Address,
    contributor: Address,
    token: Address,
    amount_per_period: i128,
    period_seconds: u64,
    num_periods: u32,
) -> Result<u128, PayrollError> {
    if amount_per_period <= 0 {
        return Err(PayrollError::ZeroAmountPerPeriod);
    }
    if period_seconds == 0 {
        return Err(PayrollError::ZeroPeriodDuration);
    }
    if num_periods == 0 {
        return Err(PayrollError::ZeroNumPeriods);
    }

    let agreement_id = get_next_agreement_id(env);
    let total_amount = amount_per_period * (num_periods as i128);

    let agreement = Agreement {
        id: agreement_id,
        employer: employer.clone(),
        token,
        mode: AgreementMode::Escrow,
        status: AgreementStatus::Created,
        total_amount,
        paid_amount: 0,
        created_at: env.ledger().timestamp(),
        activated_at: None,
        cancelled_at: None,
        grace_period_seconds: period_seconds * (num_periods as u64),
        dispute_status: DisputeStatus::None,
        dispute_raised_at: None,
        amount_per_period: Some(amount_per_period),
        period_seconds: Some(period_seconds),
        num_periods: Some(num_periods),
        claimed_periods: Some(0),
    };

    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);

    // Add the contributor as the sole employee
    let mut employees: Vec<EmployeeInfo> = Vec::new(env);
    employees.push_back(EmployeeInfo {
        address: contributor.clone(),
        salary_per_period: amount_per_period,
        added_at: env.ledger().timestamp(),
    });
    env.storage()
        .persistent()
        .set(&StorageKey::AgreementEmployees(agreement_id), &employees);

    add_to_employer_agreements(env, &employer, agreement_id);

    emit_agreement_created(
        env,
        AgreementCreatedEvent {
            agreement_id,
            employer,
            mode: AgreementMode::Escrow,
        },
    );

    emit_employee_added(
        env,
        EmployeeAddedEvent {
            agreement_id,
            employee: contributor,
            salary_per_period: amount_per_period,
        },
    );

    Ok(agreement_id)
}

/// Creates multiple escrow agreements in a single transaction.
///
/// # Atomicity
///
/// This function is **all-or-nothing**: it validates every item in `items`
/// before writing any state.  If any entry fails validation the function
/// returns `Err` with the first validation error encountered and **zero**
/// agreements are created.  This prevents partial batch application from
/// leaving the system in an inconsistent state.
///
/// # Validation rules (per item, mirrors `create_escrow_agreement_internal`)
/// * `amount_per_period` must be > 0  → `ZeroAmountPerPeriod`
/// * `period_seconds` must be > 0     → `ZeroPeriodDuration`
/// * `num_periods` must be > 0        → `ZeroNumPeriods`
///
/// # Arguments
/// * `env` - Contract environment
/// * `employer` - Address of the employer
/// * `items` - Vector of escrow creation parameters. At most `MAX_BATCH_SIZE` items are accepted.
///
/// # Returns
/// `Ok(BatchEscrowCreateResult)` — every item was valid and all agreements
/// were created successfully.
///
/// # Errors
/// * First per-item `PayrollError` — `items` is empty **or** any item fails per-item validation. No
///   agreements are created in either case.
/// * `PayrollError::BatchTooLarge` — more than `MAX_BATCH_SIZE` items.
///
/// # Gas rationale
/// `MAX_BATCH_SIZE` is 20, matching the largest batch size measured in
/// `tests/gas_benchmarks.rs`; the cap avoids late Soroban resource exhaustion
/// after partially creating agreements.
pub fn batch_create_escrow_agreements(
    env: &Env,
    employer: Address,
    items: Vec<EscrowCreateParams>,
) -> Result<BatchEscrowCreateResult, PayrollError> {
    employer.require_auth();

    if items.is_empty() {
        return Err(PayrollError::InvalidData);
    }
    if items.len() > MAX_BATCH_SIZE {
        return Err(PayrollError::BatchTooLarge);
    }

    // ── Validation pass ──────────────────────────────────────────────────────
    // Inspect every item BEFORE writing any state.  A single invalid entry
    // causes the whole batch to be rejected so no partial set of agreements
    // is ever persisted.
    for params in items.iter() {
        if params.amount_per_period <= 0 {
            return Err(PayrollError::ZeroAmountPerPeriod);
        }
        if params.period_seconds == 0 {
            return Err(PayrollError::ZeroPeriodDuration);
        }
        if params.num_periods == 0 {
            return Err(PayrollError::ZeroNumPeriods);
        }
    }

    // ── Creation pass ────────────────────────────────────────────────────────
    // All items passed validation; now commit every agreement.
    let mut agreement_ids: Vec<u128> = Vec::new(env);
    let mut results: Vec<EscrowCreateResult> = Vec::new(env);
    let mut total_created: u32 = 0;

    for params in items.iter() {
        // create_escrow_agreement_internal performs the same checks — this
        // call can only fail if there is a bug in our validation pass above,
        // so we propagate any unexpected error rather than silently swallowing it.
        let id = create_escrow_agreement_internal(
            env,
            employer.clone(),
            params.contributor.clone(),
            params.token.clone(),
            params.amount_per_period,
            params.period_seconds,
            params.num_periods,
        )?;
        agreement_ids.push_back(id);
        results.push_back(EscrowCreateResult {
            agreement_id: Some(id),
            success: true,
            error_code: 0,
        });
        total_created += 1;
    }

    Ok(BatchEscrowCreateResult {
        total_created,
        total_failed: 0,
        agreement_ids,
        results,
    })
}

/// Claims time-based payments for an escrow agreement based on elapsed periods
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the escrow agreement
///
/// # Returns
/// * `Ok(())` on success
/// * `Err(PayrollError)` on failure
///
/// # Requirements
/// - Agreement must be Active (not Paused, Cancelled, etc.)
/// - Agreement must be activated
/// - Caller must be the contributor
/// - Cannot claim more than total periods
/// - Works during grace period
pub fn claim_time_based(env: &Env, agreement_id: u128) -> Result<(), PayrollError> {
    // Check emergency pause
    if is_emergency_paused(env) {
        return Err(PayrollError::EmergencyPaused);
    }

    let mut agreement = get_agreement(env, agreement_id).ok_or(PayrollError::AgreementNotFound)?;

    // Check agreement mode
    if agreement.mode != AgreementMode::Escrow {
        return Err(PayrollError::InvalidAgreementMode);
    }

    // Check if agreement is paused
    if agreement.status == AgreementStatus::Paused {
        return Err(PayrollError::AgreementPaused);
    }

    // Check if agreement is activated (must check before status for better error messages)
    let activated_at = agreement
        .activated_at
        .ok_or(PayrollError::AgreementNotActivated)?;

    // Get period info early for completion check
    let amount_per_period = agreement
        .amount_per_period
        .ok_or(PayrollError::InvalidData)?;

    let period_seconds = agreement.period_seconds.ok_or(PayrollError::InvalidData)?;

    let num_periods = agreement.num_periods.ok_or(PayrollError::InvalidData)?;

    let mut claimed_periods = agreement.claimed_periods.unwrap_or(0);

    // Check if all periods have been claimed (before general status check for better error)
    if claimed_periods >= num_periods {
        return Err(PayrollError::AllPeriodsClaimed);
    }

    // Invariant check
    assert!(
        claimed_periods <= num_periods,
        "Invariant violation: claimed_periods > num_periods"
    );

    // Allow claims if:
    // 1. Agreement is Active, OR
    // 2. Agreement is Cancelled AND grace period is still active
    let can_claim = match agreement.status {
        AgreementStatus::Active => true,
        AgreementStatus::Cancelled => is_grace_period_active(env, agreement_id),
        _ => false,
    };

    if !can_claim {
        return Err(PayrollError::NotInGracePeriod);
    }

    let employees: Vec<EmployeeInfo> = env
        .storage()
        .persistent()
        .get(&StorageKey::AgreementEmployees(agreement_id))
        .unwrap_or(Vec::new(env));

    let contributor = employees
        .get(0)
        .ok_or(PayrollError::NoEmployee)?
        .address
        .clone();

    contributor.require_auth();

    let current_time = env.ledger().timestamp();
    let elapsed_seconds = current_time - activated_at;
    let periods_elapsed = (elapsed_seconds / period_seconds) as u32;

    let periods_to_pay = if periods_elapsed > num_periods {
        num_periods - claimed_periods
    } else {
        periods_elapsed - claimed_periods
    };

    if periods_to_pay == 0 {
        return Err(PayrollError::NoPeriodsToClaim);
    }

    let amount = amount_per_period
        .checked_mul(periods_to_pay as i128)
        .ok_or(PayrollError::InvalidData)?;

    // Check escrow balance
    let escrow_balance = DataKey::get_agreement_escrow_balance(env, agreement_id, &agreement.token);
    if escrow_balance < amount {
        return Err(PayrollError::InsufficientEscrowBalance);
    }

    // Get contract address
    let contract_address = env.current_contract_address();

    // Transfer tokens from escrow to contributor
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
                        contributor.clone().into_val(env),
                        amount.into_val(env),
                    ],
                ),
            },
            sub_invocations: Vec::new(env),
        })],
    ));
    token_client.transfer(&contract_address, &contributor, &amount);

    // Update escrow balance
    let new_escrow_balance = escrow_balance - amount;
    DataKey::set_agreement_escrow_balance(env, agreement_id, &agreement.token, new_escrow_balance);

    // Update claimed periods and paid amount
    claimed_periods += periods_to_pay;
    agreement.claimed_periods = Some(claimed_periods);
    agreement.paid_amount += amount;
    // Keep the standalone paid-amount key in sync, like claim_payroll,
    // batch_claim_payroll and resolve_dispute do, so the escrow-conservation
    // invariant (remaining + paid == total) holds for time-based claims too.
    DataKey::set_agreement_paid_amount(env, agreement_id, agreement.paid_amount);

    if claimed_periods >= num_periods {
        agreement.status = AgreementStatus::Completed;
    }

    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);

    emit_payment_sent(
        env,
        PaymentSentEvent {
            agreement_id,
            from: agreement.employer.clone(),
            to: contributor.clone(),
            amount,
            token: agreement.token.clone(),
        },
    );

    emit_payment_received(
        env,
        PaymentReceivedEvent {
            agreement_id,
            to: contributor,
            amount,
            token: agreement.token,
        },
    );

    Ok(())
}

/// Gets the number of claimed periods for a time-based escrow agreement
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the agreement
///
/// # Returns
/// Number of claimed periods, or 0 if not a time-based agreement
pub fn get_claimed_periods(env: &Env, agreement_id: u128) -> u32 {
    let agreement = get_agreement(env, agreement_id)
        .unwrap_or_else(|| panic_with_error!(env, PayrollError::AgreementNotFound));

    agreement.claimed_periods.unwrap_or(0)
}
