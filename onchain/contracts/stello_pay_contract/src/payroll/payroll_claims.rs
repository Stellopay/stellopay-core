//! Payroll claim paths: single/employee-indexed claims, in-token settlement
//! and the multisig-approved large-payment path. Batch claims live in
//! [`super::payroll_batch`].

use super::currency::convert_amount;
use super::grace::is_grace_period_active;
use super::{
    acquire_reentrancy_guard, enforce_rate_limit, get_agreement, is_emergency_paused,
    release_reentrancy_guard, MultisigClient, OperationKind, OperationStatus,
    SalaryAdjustmentClient,
};
use crate::events::{
    emit_payroll_claimed, PaymentReceivedEvent, PaymentSentEvent, PayrollClaimedEvent,
};
use crate::storage::{AgreementMode, AgreementStatus, DataKey, PayrollError, StorageKey};
use soroban_sdk::{
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    token, Address, Env, IntoVal, Symbol, Val, Vec,
};

// -----------------------------------------------------------------------------
// Payroll claiming (feature/payroll-claiming)
// -----------------------------------------------------------------------------

/// Claim payroll for an employee in a payroll agreement.
///
/// This function allows an employee to claim their salary based on elapsed time periods
/// since the agreement was activated. Each employee has individual period tracking.
///
/// # Arguments
///
/// * `env` - The Soroban environment
/// * `agreement_id` - The unique identifier for the payroll agreement
/// * `employee_index` - The index of the employee in the agreement (0-based)
///
/// # Returns
///
/// Returns `Ok(())` on success, or a `PayrollError` on failure.
///
/// # Errors
///
/// * `PayrollError::Unauthorized` - If the caller is not the employee at the given index
/// * `PayrollError::InvalidEmployeeIndex` - If the employee index is out of bounds
/// * `PayrollError::AgreementNotFound` - If the agreement doesn't exist or isn't activated
/// * `PayrollError::InsufficientEscrowBalance` - If there aren't enough funds in escrow
/// * `PayrollError::NoPeriodsToClaim` - If there are no periods available to claim
/// * `PayrollError::TransferFailed` - If the token transfer fails
///
/// # Events
///
/// Emits `PayrollClaimed`, `PaymentSent`, and `PaymentReceived` events on success.
pub fn claim_payroll(
    env: &Env,
    caller: &Address,
    agreement_id: u128,
    employee_index: u32,
) -> Result<(), PayrollError> {
    // Guard the entire claim against cross-contract reentrancy (e.g. a hostile
    // token re-entering during `transfer`). The guard is released on every
    // return path; a panic clears it automatically via temporary storage.
    acquire_reentrancy_guard(env)?;
    let result = claim_payroll_inner(env, caller, agreement_id, employee_index);
    release_reentrancy_guard(env);
    result
}

fn claim_payroll_inner(
    env: &Env,
    caller: &Address,
    agreement_id: u128,
    employee_index: u32,
) -> Result<(), PayrollError> {
    enforce_rate_limit(env, caller)?;

    // Check emergency pause
    if is_emergency_paused(env) {
        return Err(PayrollError::EmergencyPaused);
    }

    // Validate employee index
    let employee_count = DataKey::get_employee_count(env, agreement_id);
    if employee_index >= employee_count {
        return Err(PayrollError::InvalidEmployeeIndex);
    }

    // Get agreement and check status
    let agreement = get_agreement(env, agreement_id).ok_or(PayrollError::AgreementNotFound)?;

    // Check if agreement is paused
    if agreement.status == AgreementStatus::Paused {
        return Err(PayrollError::InvalidData);
    }

    // Check agreement mode
    if agreement.mode != AgreementMode::Payroll {
        return Err(PayrollError::InvalidAgreementMode);
    }

    // Allow claims if:
    // 1. Agreement is Active, OR
    // 2. Agreement is Cancelled AND grace period is still active
    let can_claim = match agreement.status {
        AgreementStatus::Active => true,
        AgreementStatus::Cancelled => is_grace_period_active(env, agreement_id),
        _ => false,
    };

    if !can_claim {
        return Err(PayrollError::InvalidData);
    }

    // Get employee address at the given index
    let employee = DataKey::get_employee(env, agreement_id, employee_index)
        .ok_or(PayrollError::AgreementNotFound)?;

    // Validate that caller is the employee
    if *caller != employee {
        return Err(PayrollError::Unauthorized);
    }

    // Get agreement activation time
    let activation_time = DataKey::get_agreement_activation_time(env, agreement_id)
        .ok_or(PayrollError::AgreementNotActivated)?;

    // Get period duration
    let period_duration = DataKey::get_agreement_period_duration(env, agreement_id)
        .ok_or(PayrollError::AgreementNotFound)?;

    // Get token address
    let token =
        DataKey::get_agreement_token(env, agreement_id).ok_or(PayrollError::AgreementNotFound)?;

    // Get current timestamp
    let current_time = env.ledger().timestamp();

    // Calculate elapsed time since activation
    if current_time < activation_time {
        return Err(PayrollError::InvalidData);
    }

    let elapsed_time = current_time - activation_time;

    // Calculate total elapsed periods
    let total_elapsed_periods = (elapsed_time / period_duration) as u32;

    // Get employee's claimed periods
    let claimed_periods = DataKey::get_employee_claimed_periods(env, agreement_id, employee_index);

    // Invariant check: claimed_periods <= num_periods (if defined)
    if let Some(num_periods) = agreement.num_periods {
        assert!(
            claimed_periods <= num_periods,
            "Invariant violation: claimed_periods > num_periods"
        );
    }

    // Calculate periods to pay
    if total_elapsed_periods <= claimed_periods {
        return Err(PayrollError::NoPeriodsToClaim);
    }

    let periods_to_pay = total_elapsed_periods - claimed_periods;

    // Get employee salary per period, checking for dynamic adjustment overrides.
    let mut salary_per_period = DataKey::get_employee_salary(env, agreement_id, employee_index)
        .ok_or(PayrollError::AgreementNotFound)?;

    if let Some(salary_adj_addr) = env
        .storage()
        .persistent()
        .get::<_, Address>(&StorageKey::SalaryAdjustmentContract)
    {
        let client = SalaryAdjustmentClient::new(env, &salary_adj_addr);
        if let Some(adjusted_salary) = client.get_employee_salary(&employee) {
            salary_per_period = adjusted_salary;
        }
    }

    // Calculate total amount to pay
    let amount = salary_per_period
        .checked_mul(periods_to_pay as i128)
        .ok_or(PayrollError::InvalidData)?;

    // If a LargePayment threshold is configured and this claim meets it,
    // reject and require the caller to use claim_payroll_multisig instead.
    if let Some(threshold) = env
        .storage()
        .persistent()
        .get::<_, i128>(&StorageKey::LargePaymentThreshold)
    {
        if threshold > 0 && amount >= threshold {
            return Err(PayrollError::MultisigApprovalRequired);
        }
    }

    // Check escrow balance
    let escrow_balance = DataKey::get_agreement_escrow_balance(env, agreement_id, &token);
    if escrow_balance < amount {
        return Err(PayrollError::InsufficientEscrowBalance);
    }

    // Get contract address (this contract)
    let contract_address = env.current_contract_address();

    // === EFFECTS BEFORE INTERACTION (checks-effects-interactions) ===
    // Persist the new escrow balance, claimed periods, and paid amount BEFORE
    // the external token transfer, so a malicious or hook-enabled token cannot
    // re-enter and observe stale state to double-claim a period. The transient
    // reentrancy guard (acquired by the caller) is the primary defense; this
    // ordering is defense-in-depth.
    let new_escrow_balance = escrow_balance - amount;
    DataKey::set_agreement_escrow_balance(env, agreement_id, &token, new_escrow_balance);

    let new_claimed_periods = claimed_periods + periods_to_pay;
    DataKey::set_employee_claimed_periods(env, agreement_id, employee_index, new_claimed_periods);

    let current_paid = DataKey::get_agreement_paid_amount(env, agreement_id);
    let new_paid = current_paid
        .checked_add(amount)
        .ok_or(PayrollError::InvalidData)?;
    DataKey::set_agreement_paid_amount(env, agreement_id, new_paid);

    // === INTERACTION: transfer tokens from escrow to employee ===
    //
    // IMPORTANT: Token `transfer(from=contract_address, ...)` requires `from.require_auth()`.
    // When the token contract calls `require_auth()` on a contract address, the calling
    // contract must pre-authorize that deeper invocation via `authorize_as_current_contract`.
    let token_client = token::Client::new(env, &token);
    env.authorize_as_current_contract(Vec::from_array(
        env,
        [InvokerContractAuthEntry::Contract(SubContractInvocation {
            context: ContractContext {
                contract: token.clone(),
                fn_name: Symbol::new(env, "transfer"),
                args: Vec::<Val>::from_array(
                    env,
                    [
                        contract_address.clone().into_val(env),
                        employee.clone().into_val(env),
                        amount.into_val(env),
                    ],
                ),
            },
            sub_invocations: Vec::new(env),
        })],
    ));
    token_client.transfer(&contract_address, &employee, &amount);

    // Emit events
    emit_payroll_claimed(
        env,
        PayrollClaimedEvent {
            agreement_id,
            employee: employee.clone(),
            amount,
        },
    );

    #[allow(clippy::needless_borrow)]
    PaymentSentEvent {
        agreement_id,
        from: contract_address,
        to: employee.clone(),
        amount,
        token: token.clone(),
    }
    .publish(&env);

    #[allow(clippy::needless_borrow)]
    PaymentReceivedEvent {
        agreement_id,
        to: employee,
        amount,
        token: token.clone(),
    }
    .publish(&env);

    Ok(())
}

/// Claims payroll for a large payment that has been pre-approved by the multisig contract.
///
/// # Arguments
/// * `caller` - Employee address (must authenticate)
/// * `agreement_id` - Payroll agreement ID
/// * `employee_index` - Employee index within the agreement
/// * `multisig_operation_id` - ID of the Executed LargePayment operation in the multisig
///
/// # Access Control
/// Requires employee authentication and a valid Executed multisig LargePayment operation
/// whose `to` field matches the caller and `amount` matches the computed payout.
pub fn claim_payroll_multisig(
    env: &Env,
    caller: &Address,
    agreement_id: u128,
    employee_index: u32,
    multisig_operation_id: u128,
) -> Result<(), PayrollError> {
    let multisig_addr = env
        .storage()
        .persistent()
        .get::<_, Address>(&StorageKey::MultisigContract)
        .ok_or(PayrollError::MultisigApprovalRequired)?;

    // Temporarily clear the threshold so claim_payroll_core can proceed.
    // We verify the multisig op here before delegating.
    let client = MultisigClient::new(env, &multisig_addr);
    let op = client
        .get_operation(&multisig_operation_id)
        .ok_or(PayrollError::MultisigApprovalRequired)?;
    if op.status != OperationStatus::Executed {
        return Err(PayrollError::MultisigApprovalRequired);
    }
    // Verify the operation targets this caller (employee) with a LargePayment kind.
    match &op.kind {
        OperationKind::LargePayment(_, to, _) if to == caller => {}
        _ => return Err(PayrollError::MultisigApprovalRequired),
    }

    // Bypass the threshold guard by temporarily removing it, run claim, then restore.
    let saved_threshold: Option<i128> = env
        .storage()
        .persistent()
        .get(&StorageKey::LargePaymentThreshold);
    env.storage()
        .persistent()
        .remove(&StorageKey::LargePaymentThreshold);
    let result = claim_payroll(env, caller, agreement_id, employee_index);
    if let Some(t) = saved_threshold {
        env.storage()
            .persistent()
            .set(&StorageKey::LargePaymentThreshold, &t);
    }
    result
}

/// Claims payroll for an employee but settles the payout in a caller-specified
/// currency, using the configured FX rate between the agreement's base token
/// and the requested payout token.
///
/// This preserves all invariants of `claim_payroll` (period counting, grace
/// period semantics, and agreement accounting in base currency), while
/// allowing the actual transfer to occur in any token that has sufficient
/// escrow balance and a configured FX rate.
pub fn claim_payroll_in_token(
    env: &Env,
    caller: &Address,
    agreement_id: u128,
    employee_index: u32,
    payout_token: Address,
) -> Result<(), PayrollError> {
    // Reentrancy guard mirrors `claim_payroll`; released on every return path.
    acquire_reentrancy_guard(env)?;
    let result =
        claim_payroll_in_token_inner(env, caller, agreement_id, employee_index, payout_token);
    release_reentrancy_guard(env);
    result
}

fn claim_payroll_in_token_inner(
    env: &Env,
    caller: &Address,
    agreement_id: u128,
    employee_index: u32,
    payout_token: Address,
) -> Result<(), PayrollError> {
    enforce_rate_limit(env, caller)?;

    // Validate employee index
    let employee_count = DataKey::get_employee_count(env, agreement_id);
    if employee_index >= employee_count {
        return Err(PayrollError::InvalidEmployeeIndex);
    }

    // Get agreement and check status
    let agreement = get_agreement(env, agreement_id).ok_or(PayrollError::AgreementNotFound)?;

    // Check if agreement is paused
    if agreement.status == AgreementStatus::Paused {
        return Err(PayrollError::InvalidData);
    }

    // Check agreement mode
    if agreement.mode != AgreementMode::Payroll {
        return Err(PayrollError::InvalidAgreementMode);
    }

    // Allow claims if:
    // 1. Agreement is Active, OR
    // 2. Agreement is Cancelled AND grace period is still active
    let can_claim = match agreement.status {
        AgreementStatus::Active => true,
        AgreementStatus::Cancelled => is_grace_period_active(env, agreement_id),
        _ => false,
    };

    if !can_claim {
        return Err(PayrollError::InvalidData);
    }

    // Get employee address at the given index
    let employee = DataKey::get_employee(env, agreement_id, employee_index)
        .ok_or(PayrollError::AgreementNotFound)?;

    // Validate that caller is the employee
    if *caller != employee {
        return Err(PayrollError::Unauthorized);
    }

    // Get agreement activation time
    let activation_time = DataKey::get_agreement_activation_time(env, agreement_id)
        .ok_or(PayrollError::AgreementNotActivated)?;

    // Get period duration
    let period_duration = DataKey::get_agreement_period_duration(env, agreement_id)
        .ok_or(PayrollError::AgreementNotFound)?;

    // Get base token address
    let base_token =
        DataKey::get_agreement_token(env, agreement_id).ok_or(PayrollError::AgreementNotFound)?;

    // Shortcut to the single-currency path if payout token == base token.
    // Call the inner (unguarded) variant because the guard is already held by
    // this function's wrapper; re-acquiring would self-trip the guard.
    if payout_token == base_token {
        return claim_payroll_inner(env, caller, agreement_id, employee_index);
    }

    // Get current timestamp
    let current_time = env.ledger().timestamp();

    // Calculate elapsed time since activation
    if current_time < activation_time {
        return Err(PayrollError::InvalidData);
    }

    let elapsed_time = current_time - activation_time;

    // Calculate total elapsed periods
    let total_elapsed_periods = (elapsed_time / period_duration) as u32;

    // Get employee's claimed periods
    let claimed_periods = DataKey::get_employee_claimed_periods(env, agreement_id, employee_index);

    // Calculate periods to pay
    if total_elapsed_periods <= claimed_periods {
        return Err(PayrollError::NoPeriodsToClaim);
    }

    let periods_to_pay = total_elapsed_periods - claimed_periods;

    // Get employee salary per period
    let salary_per_period = DataKey::get_employee_salary(env, agreement_id, employee_index)
        .ok_or(PayrollError::AgreementNotFound)?;

    // Calculate total amount to pay in base currency
    let amount_base = salary_per_period
        .checked_mul(periods_to_pay as i128)
        .ok_or(PayrollError::InvalidData)?;

    // Convert to payout currency using configured FX rate.
    let amount_payout = convert_amount(env, &base_token, &payout_token, amount_base)?;

    // Check escrow balance for payout token
    let escrow_balance_payout =
        DataKey::get_agreement_escrow_balance(env, agreement_id, &payout_token);
    if escrow_balance_payout < amount_payout {
        return Err(PayrollError::InsufficientEscrowBalance);
    }

    // Get contract address (this contract)
    let contract_address = env.current_contract_address();

    // === EFFECTS BEFORE INTERACTION (checks-effects-interactions) ===
    // Persist payout-currency escrow, claimed periods, and base-currency paid
    // amount BEFORE the external transfer (defense-in-depth alongside the guard).
    let new_escrow_payout = escrow_balance_payout - amount_payout;
    DataKey::set_agreement_escrow_balance(env, agreement_id, &payout_token, new_escrow_payout);

    let new_claimed_periods = claimed_periods + periods_to_pay;
    DataKey::set_employee_claimed_periods(env, agreement_id, employee_index, new_claimed_periods);

    let current_paid = DataKey::get_agreement_paid_amount(env, agreement_id);
    let new_paid = current_paid
        .checked_add(amount_base)
        .ok_or(PayrollError::InvalidData)?;
    DataKey::set_agreement_paid_amount(env, agreement_id, new_paid);

    // === INTERACTION: transfer tokens from escrow to employee in payout currency ===
    //
    // Token `transfer(from=contract_address, ...)` requires `from.require_auth()`.
    // We pre-authorize via `authorize_as_current_contract` as in the base path.
    let token_client = token::Client::new(env, &payout_token);
    env.authorize_as_current_contract(Vec::from_array(
        env,
        [InvokerContractAuthEntry::Contract(SubContractInvocation {
            context: ContractContext {
                contract: payout_token.clone(),
                fn_name: Symbol::new(env, "transfer"),
                args: Vec::<Val>::from_array(
                    env,
                    [
                        contract_address.clone().into_val(env),
                        employee.clone().into_val(env),
                        amount_payout.into_val(env),
                    ],
                ),
            },
            sub_invocations: Vec::new(env),
        })],
    ));
    token_client.transfer(&contract_address, &employee, &amount_payout);

    // Emit events: `PayrollClaimed` remains in base currency units, while the
    // payment events reflect the actual payout asset and amount.
    emit_payroll_claimed(
        env,
        PayrollClaimedEvent {
            agreement_id,
            employee: employee.clone(),
            amount: amount_base,
        },
    );

    #[allow(clippy::needless_borrow)]
    PaymentSentEvent {
        agreement_id,
        from: contract_address,
        to: employee.clone(),
        amount: amount_payout,
        token: payout_token.clone(),
    }
    .publish(&env);

    #[allow(clippy::needless_borrow)]
    PaymentReceivedEvent {
        agreement_id,
        to: employee,
        amount: amount_payout,
        token: payout_token,
    }
    .publish(&env);

    Ok(())
}

/// Get the number of periods already claimed by an employee.
///
/// # Arguments
///
/// * `env` - The Soroban environment
/// * `agreement_id` - The unique identifier for the payroll agreement
/// * `employee_index` - The index of the employee in the agreement (0-based)
///
/// # Returns
///
/// Returns the number of claimed periods (0 if none have been claimed).
pub fn get_employee_claimed_periods(env: &Env, agreement_id: u128, employee_index: u32) -> u32 {
    DataKey::get_employee_claimed_periods(env, agreement_id, employee_index)
}
