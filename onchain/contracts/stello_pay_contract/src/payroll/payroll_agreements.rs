//! Payroll agreement lifecycle: creation, batch creation, employee
//! management and activation. Claim paths live in [`super::payroll_claims`]
//! and [`super::payroll_batch`].

use super::{add_to_employer_agreements, get_agreement, get_next_agreement_id};
use crate::audit::{record_entry, AuditEvent};
use crate::events::{
    emit_agreement_activated, emit_agreement_created, emit_employee_added, AgreementActivatedEvent,
    AgreementCreatedEvent, EmployeeAddedEvent,
};
use crate::storage::{
    Agreement, AgreementMode, AgreementStatus, BatchPayrollCreateResult, DisputeStatus,
    EmployeeInfo, PayrollCreateParams, PayrollCreateResult, PayrollError, StorageKey,
    MAX_BATCH_SIZE,
};
use soroban_sdk::{panic_with_error, Address, Env, Vec};

// -----------------------------------------------------------------------------
// Agreement lifecycle (main)
// -----------------------------------------------------------------------------

/// Creates a payroll agreement for multiple employees
///
/// # Arguments
/// * `env` - Contract environment
/// * `employer` - Address of the employer creating the agreement
/// * `token` - Token address for payments
/// * `grace_period_seconds` - Grace period before agreement can be cancelled
///
/// # Returns
/// Agreement ID
///
/// # Access Control
/// Requires employer authentication
pub fn create_payroll_agreement(
    env: &Env,
    employer: Address,
    token: Address,
    grace_period_seconds: u64,
) -> u128 {
    employer.require_auth();
    create_payroll_agreement_internal(env, employer, token, grace_period_seconds)
}

fn create_payroll_agreement_internal(
    env: &Env,
    employer: Address,
    token: Address,
    grace_period_seconds: u64,
) -> u128 {
    let agreement_id = get_next_agreement_id(env);

    let agreement = Agreement {
        id: agreement_id,
        employer: employer.clone(),
        token,
        mode: AgreementMode::Payroll,
        status: AgreementStatus::Created,
        total_amount: 0,
        paid_amount: 0,
        created_at: env.ledger().timestamp(),
        activated_at: None,
        cancelled_at: None,
        grace_period_seconds,
        dispute_status: DisputeStatus::None,
        dispute_raised_at: None,
        amount_per_period: None,
        period_seconds: None,
        num_periods: None,
        claimed_periods: None,
    };

    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);

    let employees: Vec<EmployeeInfo> = Vec::new(env);
    env.storage()
        .persistent()
        .set(&StorageKey::AgreementEmployees(agreement_id), &employees);

    add_to_employer_agreements(env, &employer, agreement_id);

    emit_agreement_created(
        env,
        AgreementCreatedEvent {
            agreement_id,
            employer: employer.clone(),
            mode: AgreementMode::Payroll,
        },
    );
    record_entry(
        env,
        employer,
        AuditEvent::AgreementCreated,
        agreement_id,
        None,
        Some(0),
    );

    agreement_id
}

/// Creates multiple payroll agreements in a single transaction.
///
/// # Atomicity
///
/// This function is **all-or-nothing**: it validates every item in `items`
/// before writing any state.  If any entry fails validation the function
/// returns `Err(PayrollError::InvalidData)` and **zero** agreements are
/// created.  This prevents partial batch application from leaving the system
/// in an inconsistent state.
///
/// # Validation rules (per item)
/// * `grace_period_seconds` must be > 0 — a zero grace period is ambiguous and prevents any dispute
///   or claim window from opening after cancellation.
///
/// # Arguments
/// * `env` - Contract environment
/// * `employer` - Address of the employer creating the agreements
/// * `items` - Vector of payroll creation parameters. At most `MAX_BATCH_SIZE` items are accepted.
///
/// # Returns
/// `Ok(BatchPayrollCreateResult)` — every item was valid and all agreements
/// were created successfully.
///
/// # Errors
/// * `PayrollError::InvalidData` — `items` is empty **or** any item fails per-item validation.  No
///   agreements are created in either case.
/// * `PayrollError::BatchTooLarge` — more than `MAX_BATCH_SIZE` items.
///
/// # Gas rationale
/// `MAX_BATCH_SIZE` is 20, matching the largest batch size measured in
/// `tests/gas_benchmarks.rs`; the cap avoids late Soroban resource exhaustion
/// after partially creating agreements.
pub fn batch_create_payroll_agreements(
    env: &Env,
    employer: Address,
    items: Vec<PayrollCreateParams>,
) -> Result<BatchPayrollCreateResult, PayrollError> {
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
        if params.grace_period_seconds == 0 {
            return Err(PayrollError::InvalidData);
        }
    }

    // ── Creation pass ────────────────────────────────────────────────────────
    // All items passed validation; now commit every agreement.
    let mut agreement_ids: Vec<u128> = Vec::new(env);
    let mut results: Vec<PayrollCreateResult> = Vec::new(env);
    let mut total_created: u32 = 0;

    for params in items.iter() {
        let id = create_payroll_agreement_internal(
            env,
            employer.clone(),
            params.token.clone(),
            params.grace_period_seconds,
        );
        agreement_ids.push_back(id);
        results.push_back(PayrollCreateResult {
            agreement_id: Some(id),
            success: true,
            error_code: 0,
        });
        total_created += 1;
    }

    Ok(BatchPayrollCreateResult {
        total_created,
        total_failed: 0,
        agreement_ids,
        results,
    })
}

/// Adds an employee to a payroll agreement
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the agreement
/// * `employee` - Address of the employee to add
/// * `salary_per_period` - Employee's salary per period
///
/// # Access Control
/// Requires employer authentication
/// Agreement must be in Created status
pub fn add_employee_to_agreement(
    env: &Env,
    agreement_id: u128,
    employee: Address,
    salary_per_period: i128,
) {
    let mut agreement = get_agreement(env, agreement_id)
        .unwrap_or_else(|| panic_with_error!(env, PayrollError::AgreementNotFound));

    agreement.employer.require_auth();

    assert!(
        agreement.status == AgreementStatus::Created,
        "Can only add employees to Created agreements"
    );

    assert!(
        agreement.mode == AgreementMode::Payroll,
        "Can only add employees to Payroll agreements"
    );

    assert!(salary_per_period > 0, "Salary must be positive");

    let mut employees: Vec<EmployeeInfo> = env
        .storage()
        .persistent()
        .get(&StorageKey::AgreementEmployees(agreement_id))
        .unwrap_or(Vec::new(env));

    // Reject duplicate employee addresses. Each address must map to exactly one
    // salary entry within an agreement; adding the same address twice would
    // create two salary streams and corrupt per-employee claim accounting.
    // A removed employee can be re-added because they are no longer present.
    for existing in employees.iter() {
        if existing.address == employee {
            panic_with_error!(env, PayrollError::EmployeeAlreadyExists);
        }
    }

    employees.push_back(EmployeeInfo {
        address: employee.clone(),
        salary_per_period,
        added_at: env.ledger().timestamp(),
    });

    agreement.total_amount += salary_per_period;

    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);
    env.storage()
        .persistent()
        .set(&StorageKey::AgreementEmployees(agreement_id), &employees);

    emit_employee_added(
        env,
        EmployeeAddedEvent {
            agreement_id,
            employee,
            salary_per_period,
        },
    );
}

/// Activates a payroll or escrow agreement, transitioning it from `Created` to `Active`.
///
/// # Arguments
///
/// * `env` - Contract environment used to authenticate the employer and update the stored agreement.
/// * `agreement_id` - ID of the agreement to activate; must resolve to a valid agreement in
///   `Created` status.
///
/// # Preconditions
///
/// * The agreement must exist and its status must be `AgreementStatus::Created`.
/// * For `Payroll`-mode agreements, at least one employee must have been added via
///   `add_employee_to_agreement` before activation.
/// * The caller must be the agreement's employer (`employer.require_auth()`).
///
/// # State Transition
///
/// `Created` → `Active`
///
/// # Access Control
///
/// Requires employer authentication via `Address::require_auth`.
///
/// # Postconditions
///
/// * `agreement.status` is set to `AgreementStatus::Active`.
/// * `agreement.activated_at` is set to the current ledger timestamp.
/// * The updated agreement is persisted to durable storage.
///
/// # Panics
///
/// * If the agreement is not in `Created` status (includes already-Active, Paused, Cancelled,
///   Completed, or Disputed agreements).
/// * If the agreement is in `Payroll` mode and has no employees.
///
/// # Emits
///
/// * [`AgreementActivatedEvent`] with the `agreement_id`.
pub fn activate_agreement(env: &Env, agreement_id: u128) {
    let mut agreement = get_agreement(env, agreement_id)
        .unwrap_or_else(|| panic_with_error!(env, PayrollError::AgreementNotFound));

    agreement.employer.require_auth();

    assert!(
        agreement.status == AgreementStatus::Created,
        "Agreement must be in Created status"
    );

    if agreement.mode == AgreementMode::Payroll {
        let employees: Vec<EmployeeInfo> = env
            .storage()
            .persistent()
            .get(&StorageKey::AgreementEmployees(agreement_id))
            .unwrap_or(Vec::new(env));
        assert!(
            !employees.is_empty(),
            "Payroll agreement must have at least one employee to activate"
        );
    }

    agreement.status = AgreementStatus::Active;
    agreement.activated_at = Some(env.ledger().timestamp());

    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);

    emit_agreement_activated(env, AgreementActivatedEvent { agreement_id });
    record_entry(
        env,
        agreement.employer,
        AuditEvent::AgreementActivated,
        agreement_id,
        None,
        Some(agreement.total_amount),
    );
}
