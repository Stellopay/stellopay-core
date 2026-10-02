//! Batch payroll claiming: all-or-nothing batch validation with
//! per-employee partial-success results.

use super::grace::is_grace_period_active;
use super::{
    acquire_reentrancy_guard, enforce_rate_limit, get_agreement, release_reentrancy_guard,
    SalaryAdjustmentClient,
};
use crate::events::{
    emit_payroll_claimed, BatchPayrollClaimedEvent, PaymentReceivedEvent, PaymentSentEvent,
    PayrollClaimedEvent,
};
use crate::storage::{
    AgreementMode, AgreementStatus, BatchPayrollResult, DataKey, PayrollClaimResult, PayrollError,
    StorageKey, MAX_BATCH_SIZE,
};
use soroban_sdk::{
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    token, Address, Env, IntoVal, Symbol, Val, Vec,
};

/// Attempts `claim_payroll` semantics for each entry in `employee_indices`.
/// A failure for one employee does **not** abort the remaining claims —
/// partial success is intentional so that one unclaimable index cannot
/// block all others.
///
/// # Arguments
/// * `env` - Contract environment
/// * `caller` - Must equal the employee address at each supplied index (each claim still enforces
///   `caller == employee`)
/// * `agreement_id` - ID of the payroll agreement
/// * `employee_indices` - 0-based employee indices to claim for. Duplicates are detected in-memory
///   and skipped. At most `MAX_BATCH_SIZE` indices are accepted.
///
/// # Returns
/// `Ok(BatchPayrollResult)` — always succeeds at the batch level; inspect
/// `.failed_claims` / `.results` to detect per-employee failures.
///
/// # Batch-level errors (returned before any processing)
/// * `PayrollError::InvalidData` — empty index list, or agreement Paused
/// * `PayrollError::AgreementNotFound` — agreement does not exist
/// * `PayrollError::InvalidAgreementMode` — agreement is not Payroll mode
/// * `PayrollError::AgreementNotActivated` — activation timestamp missing
/// * `PayrollError::BatchTooLarge` — more than `MAX_BATCH_SIZE` indices
///
/// # Gas rationale
/// `MAX_BATCH_SIZE` is 20 because `tests/gas_benchmarks.rs` records the batch
/// ceiling and enforces the committed gas threshold. The bound is checked
/// before payroll state updates or token transfers, preserving partial-success
/// semantics for only bounded batches.
///
/// # Gas optimisations
/// * Agreement metadata (token, activation time, period duration) read once.
/// * Escrow balance tracked in-memory; written back with a single `set()`.
/// * Token client constructed once and reused.
/// * Completion / status updates are batched where possible.
pub fn batch_claim_payroll(
    env: &Env,
    caller: &Address,
    agreement_id: u128,
    employee_indices: Vec<u32>,
) -> Result<BatchPayrollResult, PayrollError> {
    // Reentrancy guard covers the whole batch (each per-employee transfer is a
    // potential reentry point); released on every return path.
    acquire_reentrancy_guard(env)?;
    let result = batch_claim_payroll_inner(env, caller, agreement_id, employee_indices);
    release_reentrancy_guard(env);
    result
}

fn batch_claim_payroll_inner(
    env: &Env,
    caller: &Address,
    agreement_id: u128,
    employee_indices: Vec<u32>,
) -> Result<BatchPayrollResult, PayrollError> {
    caller.require_auth();

    enforce_rate_limit(env, caller)?;

    if employee_indices.is_empty() {
        return Err(PayrollError::InvalidData);
    }
    if employee_indices.len() > MAX_BATCH_SIZE {
        return Err(PayrollError::BatchTooLarge);
    }

    let agreement = get_agreement(env, agreement_id).ok_or(PayrollError::AgreementNotFound)?;

    if agreement.mode != AgreementMode::Payroll {
        return Err(PayrollError::InvalidAgreementMode);
    }
    if agreement.status == AgreementStatus::Paused {
        return Err(PayrollError::InvalidData);
    }

    let can_claim = match agreement.status {
        AgreementStatus::Active => true,
        AgreementStatus::Cancelled => is_grace_period_active(env, agreement_id),
        _ => false,
    };
    if !can_claim {
        return Err(PayrollError::InvalidData);
    }

    // Read shared metadata once
    let activation_time = DataKey::get_agreement_activation_time(env, agreement_id)
        .ok_or(PayrollError::AgreementNotActivated)?;
    let period_duration = DataKey::get_agreement_period_duration(env, agreement_id)
        .ok_or(PayrollError::AgreementNotFound)?;
    let token =
        DataKey::get_agreement_token(env, agreement_id).ok_or(PayrollError::AgreementNotFound)?;
    let employee_count = DataKey::get_employee_count(env, agreement_id);

    let current_time = env.ledger().timestamp();
    if current_time < activation_time {
        return Err(PayrollError::InvalidData);
    }

    let total_elapsed_periods = ((current_time - activation_time) / period_duration) as u32;

    // Load escrow balance once; update in-memory, write back once at the end
    let mut escrow_balance = DataKey::get_agreement_escrow_balance(env, agreement_id, &token);

    let token_client = token::Client::new(env, &token);
    let contract_address = env.current_contract_address();

    let mut results: Vec<PayrollClaimResult> = Vec::new(env);
    let mut total_claimed: i128 = 0;
    let mut successful_claims: u32 = 0;
    let mut failed_claims: u32 = 0;
    let mut processed: Vec<u32> = Vec::new(env);

    for employee_index in employee_indices.iter() {
        // Duplicate guard
        if processed.iter().any(|p| p == employee_index) {
            failed_claims += 1;
            results.push_back(PayrollClaimResult {
                employee_index,
                success: false,
                amount_claimed: 0,
                error_code: PayrollError::InvalidData as u32,
            });
            continue;
        }
        processed.push_back(employee_index);

        // Bounds check
        if employee_index >= employee_count {
            failed_claims += 1;
            results.push_back(PayrollClaimResult {
                employee_index,
                success: false,
                amount_claimed: 0,
                error_code: PayrollError::InvalidEmployeeIndex as u32,
            });
            continue;
        }

        // Employee must exist
        let employee = match DataKey::get_employee(env, agreement_id, employee_index) {
            Some(addr) => addr,
            None => {
                failed_claims += 1;
                results.push_back(PayrollClaimResult {
                    employee_index,
                    success: false,
                    amount_claimed: 0,
                    error_code: PayrollError::AgreementNotFound as u32,
                });
                continue;
            }
        };

        // Caller must be this specific employee (same check as claim_payroll)
        if *caller != employee {
            failed_claims += 1;
            results.push_back(PayrollClaimResult {
                employee_index,
                success: false,
                amount_claimed: 0,
                error_code: PayrollError::Unauthorized as u32,
            });
            continue;
        }

        // Must have unclaimed periods
        let claimed_periods =
            DataKey::get_employee_claimed_periods(env, agreement_id, employee_index);

        // Invariant check: claimed_periods <= num_periods (if defined)
        if let Some(num_periods) = agreement.num_periods {
            assert!(
                claimed_periods <= num_periods,
                "Invariant violation: claimed_periods > num_periods"
            );
        }

        if total_elapsed_periods <= claimed_periods {
            failed_claims += 1;
            results.push_back(PayrollClaimResult {
                employee_index,
                success: false,
                amount_claimed: 0,
                error_code: PayrollError::NoPeriodsToClaim as u32,
            });
            continue;
        }

        let periods_to_pay = total_elapsed_periods - claimed_periods;

        // Salary must be configured, checking for dynamic adjustment overrides.
        let mut salary_per_period =
            match DataKey::get_employee_salary(env, agreement_id, employee_index) {
                Some(s) => s,
                None => {
                    failed_claims += 1;
                    results.push_back(PayrollClaimResult {
                        employee_index,
                        success: false,
                        amount_claimed: 0,
                        error_code: PayrollError::AgreementNotFound as u32,
                    });
                    continue;
                }
            };

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

        // Overflow-safe amount
        let amount = match salary_per_period.checked_mul(periods_to_pay as i128) {
            Some(a) => a,
            None => {
                failed_claims += 1;
                results.push_back(PayrollClaimResult {
                    employee_index,
                    success: false,
                    amount_claimed: 0,
                    error_code: PayrollError::InvalidData as u32,
                });
                continue;
            }
        };

        // Check in-memory escrow
        if escrow_balance < amount {
            failed_claims += 1;
            results.push_back(PayrollClaimResult {
                employee_index,
                success: false,
                amount_claimed: 0,
                error_code: PayrollError::InsufficientEscrowBalance as u32,
            });
            continue;
        }

        // === EFFECTS BEFORE INTERACTION (checks-effects-interactions) ===
        // Decrement the in-memory escrow and persist per-employee claimed
        // periods and the agreement paid amount BEFORE the external transfer,
        // so a hostile token cannot re-enter and observe stale per-employee
        // state. The transaction-level reentrancy guard is the primary defense.
        escrow_balance -= amount;
        total_claimed += amount;
        successful_claims += 1;

        DataKey::set_employee_claimed_periods(
            env,
            agreement_id,
            employee_index,
            claimed_periods + periods_to_pay,
        );

        let new_paid = DataKey::get_agreement_paid_amount(env, agreement_id)
            .checked_add(amount)
            .unwrap_or(DataKey::get_agreement_paid_amount(env, agreement_id));
        DataKey::set_agreement_paid_amount(env, agreement_id, new_paid);

        // === INTERACTION: transfer tokens from escrow to employee ===
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

        // Events — identical to claim_payroll
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
            from: contract_address.clone(),
            to: employee.clone(),
            amount,
            token: token.clone(),
        }
        .publish(&env);
        #[allow(clippy::needless_borrow)]
        PaymentReceivedEvent {
            agreement_id,
            to: employee.clone(),
            amount,
            token: token.clone(),
        }
        .publish(&env);

        results.push_back(PayrollClaimResult {
            employee_index,
            success: true,
            amount_claimed: amount,
            error_code: 0,
        });
    }

    DataKey::set_agreement_escrow_balance(env, agreement_id, &token, escrow_balance);

    #[allow(clippy::needless_borrow)]
    BatchPayrollClaimedEvent {
        agreement_id,
        total_claimed,
        successful_claims,
        failed_claims,
    }
    .publish(&env);

    Ok(BatchPayrollResult {
        agreement_id,
        total_claimed,
        successful_claims,
        failed_claims,
        results,
    })
}
