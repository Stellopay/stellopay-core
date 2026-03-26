use soroban_sdk::token::TokenClient;
use soroban_sdk::{Address, Env, Vec};

use crate::events::{
    emit_agreement_activated, emit_agreement_cancelled, emit_agreement_created,
    emit_agreement_paused, emit_agreement_resumed, emit_dsipute_raised, emit_dsipute_resolved,
    emit_employee_added, emit_grace_period_extended, emit_grace_period_finalized,
    emit_payment_received, emit_payment_sent, emit_payroll_claimed, emit_set_arbiter,
    AgreementActivatedEvent, AgreementCancelledEvent, GracePeriodExtendedEvent,
    AgreementCreatedEvent, AgreementPausedEvent, AgreementResumedEvent, ArbiterSetEvent,
    BatchMilestoneClaimedEvent, BatchPayrollClaimedEvent, DisputeRaisedEvent, DisputeResolvedEvent,
    EmployeeAddedEvent, GracePeriodFinalizedEvent, MilestoneAdded, MilestoneApproved,
    MilestoneClaimed, PaymentReceivedEvent, PaymentSentEvent, PayrollClaimedEvent,
};
use crate::storage::{
    Agreement, AgreementMode, AgreementStatus, BatchEscrowCreateResult, BatchMilestoneResult,
    BatchPayrollCreateResult, BatchPayrollResult, DataKey, DisputeStatus, EmployeeInfo,
    EscrowCreateParams, EscrowCreateResult, GracePeriodExtensionPolicy, Milestone,
    MilestoneClaimResult, MilestoneKey, PaymentType, PayrollClaimResult, PayrollCreateParams,
    PayrollCreateResult, PayrollError, StorageKey,
};
use soroban_sdk::{
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    token, IntoVal, Symbol, Val,
};

/// Fixed-point scaling factor for FX rates: 1e6 precision.
const FX_SCALE: i128 = 1_000_000;

pub fn create_milestone_agreement(
    env: Env,
    employer: Address,
    contributor: Address,
    token: Address,
) -> u128 {
    employer.require_auth();

    let mut counter: u128 = env
        .storage()
        .instance()
        .get(&MilestoneKey::AgreementCounter)
        .unwrap_or(0);
    counter += 1;

    let agreement_id = counter;

    env.storage()
        .instance()
        .set(&MilestoneKey::AgreementCounter, &counter);
    env.storage()
        .instance()
        .set(&MilestoneKey::Employer(agreement_id), &employer);
    env.storage()
        .instance()
        .set(&MilestoneKey::Contributor(agreement_id), &contributor);
    env.storage()
        .instance()
        .set(&MilestoneKey::Token(agreement_id), &token);
    env.storage().instance().set(
        &MilestoneKey::PaymentType(agreement_id),
        &PaymentType::MilestoneBased,
    );
    env.storage().instance().set(
        &MilestoneKey::Status(agreement_id),
        &AgreementStatus::Created,
    );
    env.storage()
        .instance()
        .set(&MilestoneKey::TotalAmount(agreement_id), &0i128);
    env.storage()
        .instance()
        .set(&MilestoneKey::MilestoneCount(agreement_id), &0u32);

    agreement_id
}

/// Adds a milestone to an agreement
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the agreement
/// * `amount` - Payment amount for this milestone
pub fn add_milestone(env: Env, agreement_id: u128, amount: i128) {
    let status: AgreementStatus = env
        .storage()
        .instance()
        .get(&MilestoneKey::Status(agreement_id))
        .expect("Agreement not found");

    assert!(
        status == AgreementStatus::Created,
        "Agreement must be in Created status"
    );
    assert!(amount > 0, "Amount must be positive");

    let employer: Address = env
        .storage()
        .instance()
        .get(&MilestoneKey::Employer(agreement_id))
        .expect("Employer not found");
    employer.require_auth();

    let count: u32 = env
        .storage()
        .instance()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .unwrap_or(0);

    let milestone_id = count + 1;

    env.storage().instance().set(
        &MilestoneKey::MilestoneAmount(agreement_id, milestone_id),
        &amount,
    );
    env.storage().instance().set(
        &MilestoneKey::MilestoneApproved(agreement_id, milestone_id),
        &false,
    );
    env.storage().instance().set(
        &MilestoneKey::MilestoneClaimed(agreement_id, milestone_id),
        &false,
    );
    env.storage()
        .instance()
        .set(&MilestoneKey::MilestoneCount(agreement_id), &milestone_id);

    let total: i128 = env
        .storage()
        .instance()
        .get(&MilestoneKey::TotalAmount(agreement_id))
        .unwrap_or(0);
    env.storage()
        .instance()
        .set(&MilestoneKey::TotalAmount(agreement_id), &(total + amount));

    MilestoneAdded {
        agreement_id,
        milestone_id,
        amount,
    }
    .publish(&env);
}

/// Approves a milestone for payment
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the agreement
/// * `milestone_id` - ID of the milestone to approve
pub fn approve_milestone(env: Env, agreement_id: u128, milestone_id: u32) {
    let employer: Address = env
        .storage()
        .instance()
        .get(&MilestoneKey::Employer(agreement_id))
        .expect("Employer not found");
    employer.require_auth();

    let status: AgreementStatus = env
        .storage()
        .instance()
        .get(&MilestoneKey::Status(agreement_id))
        .expect("Agreement not found");
    assert!(
        status == AgreementStatus::Created || status == AgreementStatus::Active,
        "Can only approve milestones when agreement is Created or Active"
    );

    let count: u32 = env
        .storage()
        .instance()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .expect("No milestones found");
    assert!(
        milestone_id > 0 && milestone_id <= count,
        "Invalid milestone ID"
    );

    let already_approved: bool = env
        .storage()
        .instance()
        .get(&MilestoneKey::MilestoneApproved(agreement_id, milestone_id))
        .unwrap_or(false);
    assert!(!already_approved, "Milestone already approved");

    env.storage().instance().set(
        &MilestoneKey::MilestoneApproved(agreement_id, milestone_id),
        &true,
    );

    MilestoneApproved {
        agreement_id,
        milestone_id,
    }
    .publish(&env);
}

/// Claims payment for an approved milestone
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the agreement
/// * `milestone_id` - ID of the milestone to claim
///
/// # Requirements
/// - Agreement must not be Paused
/// - Milestone must be approved
/// - Milestone must not be already claimed
pub fn claim_milestone(env: Env, agreement_id: u128, milestone_id: u32) {
    // Check emergency pause
    assert!(!is_emergency_paused(&env), "Contract is emergency paused");

    let contributor: Address = env
        .storage()
        .instance()
        .get(&MilestoneKey::Contributor(agreement_id))
        .expect("Contributor not found");
    contributor.require_auth();

    // Check if agreement is paused
    let status: AgreementStatus = env
        .storage()
        .instance()
        .get(&MilestoneKey::Status(agreement_id))
        .expect("Agreement not found");
    assert!(
        status != AgreementStatus::Paused,
        "Cannot claim when agreement is paused"
    );

    let count: u32 = env
        .storage()
        .instance()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .expect("No milestones found");
    assert!(
        milestone_id > 0 && milestone_id <= count,
        "Invalid milestone ID"
    );

    let approved: bool = env
        .storage()
        .instance()
        .get(&MilestoneKey::MilestoneApproved(agreement_id, milestone_id))
        .unwrap_or(false);
    assert!(approved, "Milestone not approved");

    let already_claimed: bool = env
        .storage()
        .instance()
        .get(&MilestoneKey::MilestoneClaimed(agreement_id, milestone_id))
        .unwrap_or(false);
    assert!(!already_claimed, "Milestone already claimed");

    let amount: i128 = env
        .storage()
        .instance()
        .get(&MilestoneKey::MilestoneAmount(agreement_id, milestone_id))
        .expect("Milestone amount not found");

    env.storage().instance().set(
        &MilestoneKey::MilestoneClaimed(agreement_id, milestone_id),
        &true,
    );

    let _token: Address = env
        .storage()
        .instance()
        .get(&MilestoneKey::Token(agreement_id))
        .expect("Token not found");

    MilestoneClaimed {
        agreement_id,
        milestone_id,
        amount,
        to: contributor.clone(),
    }
    .publish(&env);

    let all_claimed = all_milestones_claimed(&env, agreement_id, count);
    if all_claimed {
        env.storage().instance().set(
            &MilestoneKey::Status(agreement_id),
            &AgreementStatus::Completed,
        );
    }
}

/// Iterates over `milestone_ids` and claims each approved, unclaimed milestone
/// for the authenticated contributor. Failures are non-fatal; processing
/// continues to the next ID on error.
///
/// # Arguments
/// * `env`           - Contract environment
/// * `agreement_id`  - ID of the milestone agreement
/// * `milestone_ids` - 1-based milestone IDs to claim.
///   Duplicates are detected in-memory and skipped.
///
/// # Returns
/// `BatchMilestoneResult` — always returns (never panics at batch level).
pub fn batch_claim_milestones(
    env: &Env,
    agreement_id: u128,
    milestone_ids: Vec<u32>,
) -> BatchMilestoneResult {
    let contributor: Address = env
        .storage()
        .instance()
        .get(&MilestoneKey::Contributor(agreement_id))
        .expect("Contributor not found");
    contributor.require_auth();

    assert!(!milestone_ids.is_empty(), "No milestone IDs provided");

    // Shared pre-flight
    let status: AgreementStatus = env
        .storage()
        .instance()
        .get(&MilestoneKey::Status(agreement_id))
        .expect("Agreement not found");
    assert!(
        status != AgreementStatus::Paused,
        "Cannot claim when agreement is paused"
    );

    let count: u32 = env
        .storage()
        .instance()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .expect("No milestones found");

    // Token client created once and reused
    let token: Address = env
        .storage()
        .instance()
        .get(&MilestoneKey::Token(agreement_id))
        .expect("Token not found");
    let token_client = TokenClient::new(env, &token);
    let contract_address = env.current_contract_address();

    let mut results: Vec<MilestoneClaimResult> = Vec::new(env);
    let mut total_claimed: i128 = 0;
    let mut successful_claims: u32 = 0;
    let mut failed_claims: u32 = 0;
    let mut processed: Vec<u32> = Vec::new(env);

    for milestone_id in milestone_ids.iter() {
        // Duplicate guard
        if processed.iter().any(|p| p == milestone_id) {
            failed_claims += 1;
            results.push_back(MilestoneClaimResult {
                milestone_id,
                success: false,
                amount_claimed: 0,
                error_code: 1, // duplicate
            });
            continue;
        }

        // Bounds check (1-based, mirrors claim_milestone)
        if milestone_id == 0 || milestone_id > count {
            failed_claims += 1;
            results.push_back(MilestoneClaimResult {
                milestone_id,
                success: false,
                amount_claimed: 0,
                error_code: 2, // invalid ID
            });
            continue;
        }

        // Approved check
        let approved: bool = env
            .storage()
            .instance()
            .get(&MilestoneKey::MilestoneApproved(agreement_id, milestone_id))
            .unwrap_or(false);
        if !approved {
            failed_claims += 1;
            results.push_back(MilestoneClaimResult {
                milestone_id,
                success: false,
                amount_claimed: 0,
                error_code: 3, // not approved
            });
            continue;
        }

        // Already-claimed check
        let already_claimed: bool = env
            .storage()
            .instance()
            .get(&MilestoneKey::MilestoneClaimed(agreement_id, milestone_id))
            .unwrap_or(false);
        if already_claimed {
            failed_claims += 1;
            results.push_back(MilestoneClaimResult {
                milestone_id,
                success: false,
                amount_claimed: 0,
                error_code: 4, // already claimed
            });
            continue;
        }

        let amount: i128 = env
            .storage()
            .instance()
            .get(&MilestoneKey::MilestoneAmount(agreement_id, milestone_id))
            .expect("Milestone amount not found");

        // Checks-Effects-Interactions: mark claimed BEFORE transfer
        env.storage().instance().set(
            &MilestoneKey::MilestoneClaimed(agreement_id, milestone_id),
            &true,
        );

        token_client.transfer(&contract_address, &contributor, &amount);

        total_claimed += amount;
        successful_claims += 1;

        // Event — identical to claim_milestone
        #[allow(clippy::needless_borrow)]
        MilestoneClaimed {
            agreement_id,
            milestone_id,
            amount,
            to: contributor.clone(),
        }
        .publish(&env);

        results.push_back(MilestoneClaimResult {
            milestone_id,
            success: true,
            amount_claimed: amount,
            error_code: 0,
        });

        processed.push_back(milestone_id);
    }

    if all_milestones_claimed(env, agreement_id, count) {
        env.storage().instance().set(
            &MilestoneKey::Status(agreement_id),
            &AgreementStatus::Completed,
        );
    }

    #[allow(clippy::needless_borrow)]
    BatchMilestoneClaimedEvent {
        agreement_id,
        total_claimed,
        successful_claims,
        failed_claims,
    }
    .publish(&env);

    BatchMilestoneResult {
        agreement_id,
        total_claimed,
        successful_claims,
        failed_claims,
        results,
    }
}

pub fn get_milestone_count(env: Env, agreement_id: u128) -> u32 {
    env.storage()
        .instance()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .unwrap_or(0)
}

pub fn get_milestone(env: Env, agreement_id: u128, milestone_id: u32) -> Option<Milestone> {
    let count: u32 = env
        .storage()
        .instance()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .unwrap_or(0);

    if milestone_id == 0 || milestone_id > count {
        return None;
    }

    let amount: i128 = env
        .storage()
        .instance()
        .get(&MilestoneKey::MilestoneAmount(agreement_id, milestone_id))?;
    let approved: bool = env
        .storage()
        .instance()
        .get(&MilestoneKey::MilestoneApproved(agreement_id, milestone_id))
        .unwrap_or(false);
    let claimed: bool = env
        .storage()
        .instance()
        .get(&MilestoneKey::MilestoneClaimed(agreement_id, milestone_id))
        .unwrap_or(false);

    Some(Milestone {
        id: milestone_id,
        amount,
        approved,
        claimed,
    })
}

fn all_milestones_claimed(env: &Env, agreement_id: u128, count: u32) -> bool {
    for i in 1..=count {
        let claimed: bool = env
            .storage()
            .instance()
            .get(&MilestoneKey::MilestoneClaimed(agreement_id, i))
            .unwrap_or(false);
        if !claimed {
            return false;
        }
    }
    true
}

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
            employer,
            mode: AgreementMode::Payroll,
        },
    );

    agreement_id
}

/// Creates multiple payroll agreements in a single transaction.
///
/// # Arguments
/// * `env` - Contract environment
/// * `employer` - Address of the employer creating the agreements
/// * `items` - Vector of payroll creation parameters
///
/// # Returns
/// `Ok(BatchPayrollCreateResult)` — always succeeds at the batch level
/// unless `items` is empty; inspect per-item results for failures.
pub fn batch_create_payroll_agreements(
    env: &Env,
    employer: Address,
    items: Vec<PayrollCreateParams>,
) -> Result<BatchPayrollCreateResult, PayrollError> {
    employer.require_auth();

    if items.is_empty() {
        return Err(PayrollError::InvalidData);
    }

    let mut agreement_ids: Vec<u128> = Vec::new(env);
    let mut results: Vec<PayrollCreateResult> = Vec::new(env);
    let mut total_created: u32 = 0;
    let total_failed: u32 = 0;

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
        total_failed,
        agreement_ids,
        results,
    })
}

/// Creates an escrow agreement for a single contributor
///
/// # Arguments
/// * `env` - Contract environment
/// * `employer` - Address of the employer
/// * `contributor` - Address of the contributor
/// * `token` - Token address for payments
/// * `amount_per_period` - Payment amount per period
/// * `period_seconds` - Duration of each period
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
/// # Arguments
/// * `env` - Contract environment
/// * `employer` - Address of the employer
/// * `items` - Vector of escrow creation parameters
///
/// # Returns
/// `Ok(BatchEscrowCreateResult)` — always succeeds at the batch level
/// unless `items` is empty; inspect per-item `results` for failures.
pub fn batch_create_escrow_agreements(
    env: &Env,
    employer: Address,
    items: Vec<EscrowCreateParams>,
) -> Result<BatchEscrowCreateResult, PayrollError> {
    employer.require_auth();

    if items.is_empty() {
        return Err(PayrollError::InvalidData);
    }

    let mut agreement_ids: Vec<u128> = Vec::new(env);
    let mut results: Vec<EscrowCreateResult> = Vec::new(env);
    let mut total_created: u32 = 0;
    let mut total_failed: u32 = 0;

    for params in items.iter() {
        match create_escrow_agreement_internal(
            env,
            employer.clone(),
            params.contributor.clone(),
            params.token.clone(),
            params.amount_per_period,
            params.period_seconds,
            params.num_periods,
        ) {
            Ok(id) => {
                agreement_ids.push_back(id);
                results.push_back(EscrowCreateResult {
                    agreement_id: Some(id),
                    success: true,
                    error_code: 0,
                });
                total_created += 1;
            }
            Err(err) => {
                results.push_back(EscrowCreateResult {
                    agreement_id: None,
                    success: false,
                    error_code: err as u32,
                });
                total_failed += 1;
            }
        }
    }

    Ok(BatchEscrowCreateResult {
        total_created,
        total_failed,
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
    let mut agreement = get_agreement(env, agreement_id).expect("Agreement not found");

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

/// Activates an agreement
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the agreement to activate
///
/// # State Transition
/// Created -> Active
///
/// # Access Control
/// Requires employer authentication
pub fn activate_agreement(env: &Env, agreement_id: u128) {
    let mut agreement = get_agreement(env, agreement_id).expect("Agreement not found");

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
}

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

    env.storage()
        .persistent()
        .set(&StorageKey::Arbiter, &arbiter);
    emit_set_arbiter(env, ArbiterSetEvent { arbiter });

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

fn grace_period_extension_seconds(env: &Env, agreement_id: u128) -> u64 {
    env.storage()
        .persistent()
        .get(&StorageKey::GracePeriodExtensionSeconds(agreement_id))
        .unwrap_or(0u64)
}

fn effective_cancelled_grace_duration_seconds(
    env: &Env,
    agreement_id: u128,
    base_grace_seconds: u64,
) -> u64 {
    base_grace_seconds.saturating_add(grace_period_extension_seconds(env, agreement_id))
}

/// Returns the owner-configured extension caps (defaults apply until `set_grace_extension_policy` runs).
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
pub fn set_grace_extension_policy(
    env: &Env,
    caller: Address,
    policy: GracePeriodExtensionPolicy,
) -> Result<(), PayrollError> {
    caller.require_auth();
    let owner: Address = env.storage().persistent().get(&StorageKey::Owner).unwrap();
    if caller != owner {
        return Err(PayrollError::Unauthorized);
    }
    // Sanity bounds so a compromised owner key cannot configure absurd values in one tx.
    const MAX_BPS: u32 = 500_000;
    const MAX_PER_CALL: u64 = 730 * 24 * 3600;
    if policy.max_cumulative_extension_bps > MAX_BPS {
        return Err(PayrollError::GraceExtensionInvalid);
    }
    if policy.max_extension_per_call_seconds == 0 || policy.max_extension_per_call_seconds > MAX_PER_CALL
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

/// Extends the effective cancellation grace (claims, dispute window while cancelled) by `additional_seconds`.
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

    let owner: Address = env.storage().persistent().get(&StorageKey::Owner).unwrap();
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
    let max_extra = (base
        .saturating_mul(policy.max_cumulative_extension_bps as u128))
        / 10000;

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

/// Raise a dispute during the grace period (escrow or payroll agreement).
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

    if agreement.dispute_status != DisputeStatus::None {
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
        effective_cancelled_grace_duration_seconds(env, agreement_id, agreement.grace_period_seconds)
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

    Ok(())
}

/// Resolve a raised dispute: arbiter splits locked funds between employees and employer.
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - Agreement ID in `DisputeStatus::Raised`
/// * `pay_employee` - Total amount to distribute equally across employees (payroll) or to contributor (escrow)
/// * `refund_employer` - Amount to refund the employer
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
    caller.require_auth();

    let arbiter = env
        .storage()
        .persistent()
        .get::<_, Address>(&StorageKey::Arbiter)
        .expect("No Arbiter");
    if caller != arbiter {
        return Err(PayrollError::NotArbiter);
    }

    let mut agreement = get_agreement(&env, agreement_id).ok_or(PayrollError::AgreementNotFound)?;

    if agreement.dispute_status != DisputeStatus::Raised {
        return Err(PayrollError::NoDispute);
    }

    let total_locked = agreement.total_amount;
    if pay_employee + refund_employer > total_locked {
        return Err(PayrollError::InvalidPayout);
    }

    let token = TokenClient::new(&env, &agreement.token);

    let employees: Vec<EmployeeInfo> = env
        .storage()
        .persistent()
        .get(&StorageKey::AgreementEmployees(agreement_id))
        .unwrap_or(Vec::new(&env));

    // Execute transfers
    if pay_employee > 0 {
        let num_employees = employees.len() as i128;
        if num_employees > 0 {
            let amount_per_employee = pay_employee / num_employees;
            for employee in employees.iter() {
                token.transfer(
                    &env.current_contract_address(),
                    &employee.address,
                    &amount_per_employee,
                );
            }
        }
    }

    if refund_employer > 0 {
        token.transfer(
            &env.current_contract_address(),
            &agreement.employer,
            &refund_employer,
        );
    }

    agreement.dispute_status = DisputeStatus::Resolved;
    agreement.status = AgreementStatus::Completed;
    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);

    emit_dsipute_resolved(
        &env,
        DisputeResolvedEvent {
            agreement_id,
            pay_contributor: pay_employee,
            refund_employer,
        },
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

/// Sets the global FX rate admin address that is allowed to update exchange
/// rates in addition to the contract owner (e.g. an oracle contract).
pub fn set_exchange_rate_admin(
    env: &Env,
    caller: Address,
    admin: Address,
) -> Result<(), PayrollError> {
    let owner: Address = env
        .storage()
        .persistent()
        .get(&StorageKey::Owner)
        .ok_or(PayrollError::Unauthorized)?;

    caller.require_auth();

    if caller != owner {
        return Err(PayrollError::Unauthorized);
    }

    env.storage()
        .persistent()
        .set(&StorageKey::ExchangeRateAdmin, &admin);

    Ok(())
}

/// Configures the FX rate for a `(base, quote)` token pair.
///
/// Access control:
/// - Contract owner OR
/// - FX admin set via `set_exchange_rate_admin`
pub fn set_exchange_rate(
    env: &Env,
    caller: Address,
    base: Address,
    quote: Address,
    rate: i128,
) -> Result<(), PayrollError> {
    if rate <= 0 || base == quote {
        return Err(PayrollError::ExchangeRateInvalid);
    }

    caller.require_auth();

    let owner: Option<Address> = env.storage().persistent().get(&StorageKey::Owner);
    let fx_admin: Option<Address> = env
        .storage()
        .persistent()
        .get(&StorageKey::ExchangeRateAdmin);

    let is_authorized = match (owner, fx_admin) {
        (Some(o), _) if caller == o => true,
        (_, Some(a)) if caller == a => true,
        _ => false,
    };

    if !is_authorized {
        return Err(PayrollError::Unauthorized);
    }

    DataKey::set_exchange_rate(env, &base, &quote, rate);

    Ok(())
}

/// Pure conversion helper exposed as a contract entry point so off-chain
/// clients can query expected converted amounts without performing a transfer.
pub fn convert_currency(
    env: &Env,
    from_token: Address,
    to_token: Address,
    amount: i128,
) -> Result<i128, PayrollError> {
    convert_amount(env, &from_token, &to_token, amount)
}

/// Retrieves an agreement by ID
///
/// # Returns
/// Some(Agreement) if found, None otherwise
pub fn get_agreement(env: &Env, agreement_id: u128) -> Option<Agreement> {
    env.storage()
        .persistent()
        .get(&StorageKey::Agreement(agreement_id))
}

/// Retrieves all employees for an agreement
///
/// # Returns
/// Vector of employee addresses
pub fn get_agreement_employees(env: &Env, agreement_id: u128) -> Vec<Address> {
    let employees: Vec<EmployeeInfo> = env
        .storage()
        .persistent()
        .get(&StorageKey::AgreementEmployees(agreement_id))
        .unwrap_or(Vec::new(env));

    let mut addresses = Vec::new(env);
    for emp in employees.iter() {
        addresses.push_back(emp.address);
    }
    addresses
}

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

    // Calculate periods to pay
    if total_elapsed_periods <= claimed_periods {
        return Err(PayrollError::NoPeriodsToClaim);
    }

    let periods_to_pay = total_elapsed_periods - claimed_periods;

    // Get employee salary per period
    let salary_per_period = DataKey::get_employee_salary(env, agreement_id, employee_index)
        .ok_or(PayrollError::AgreementNotFound)?;

    // Calculate total amount to pay
    let amount = salary_per_period
        .checked_mul(periods_to_pay as i128)
        .ok_or(PayrollError::InvalidData)?;

    // Check escrow balance
    let escrow_balance = DataKey::get_agreement_escrow_balance(env, agreement_id, &token);
    if escrow_balance < amount {
        return Err(PayrollError::InsufficientEscrowBalance);
    }

    // Get contract address (this contract)
    let contract_address = env.current_contract_address();

    // Transfer tokens from escrow to employee.
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

    // Update escrow balance
    let new_escrow_balance = escrow_balance - amount;
    DataKey::set_agreement_escrow_balance(env, agreement_id, &token, new_escrow_balance);

    // Update employee's claimed periods
    let new_claimed_periods = claimed_periods + periods_to_pay;
    DataKey::set_employee_claimed_periods(env, agreement_id, employee_index, new_claimed_periods);

    // Update agreement total paid amount
    let current_paid = DataKey::get_agreement_paid_amount(env, agreement_id);
    let new_paid = current_paid
        .checked_add(amount)
        .ok_or(PayrollError::InvalidData)?;
    DataKey::set_agreement_paid_amount(env, agreement_id, new_paid);

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
    if payout_token == base_token {
        return claim_payroll(env, caller, agreement_id, employee_index);
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

    // Transfer tokens from escrow to employee in payout currency.
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

    // Update escrow balance for payout currency
    let new_escrow_payout = escrow_balance_payout - amount_payout;
    DataKey::set_agreement_escrow_balance(env, agreement_id, &payout_token, new_escrow_payout);

    // Update employee's claimed periods
    let new_claimed_periods = claimed_periods + periods_to_pay;
    DataKey::set_employee_claimed_periods(env, agreement_id, employee_index, new_claimed_periods);

    // Update agreement total paid amount in base currency
    let current_paid = DataKey::get_agreement_paid_amount(env, agreement_id);
    let new_paid = current_paid
        .checked_add(amount_base)
        .ok_or(PayrollError::InvalidData)?;
    DataKey::set_agreement_paid_amount(env, agreement_id, new_paid);

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

/// Attempts `claim_payroll` semantics for each entry in `employee_indices`.
/// A failure for one employee does **not** abort the remaining claims —
/// partial success is intentional so that one unclaimable index cannot
/// block all others.
///
/// # Arguments
/// * `env` - Contract environment
/// * `caller` - Must equal the employee address at each supplied
///   index (each claim still enforces `caller == employee`)
/// * `agreement_id` - ID of the payroll agreement
/// * `employee_indices` - 0-based employee indices to claim for.
///   Duplicates are detected in-memory and skipped.
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
    caller.require_auth();

    if employee_indices.is_empty() {
        return Err(PayrollError::InvalidData);
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

        // Salary must be configured
        let salary_per_period =
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

        // Update in-memory balance
        escrow_balance -= amount;
        total_claimed += amount;
        successful_claims += 1;

        // Persist per-employee state
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

        processed.push_back(employee_index);
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
    let agreement = get_agreement(env, agreement_id).unwrap_or_else(|| {
        panic!("Agreement not found");
    });

    agreement.claimed_periods.unwrap_or(0)
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn get_next_agreement_id(env: &Env) -> u128 {
    let key = StorageKey::NextAgreementId;
    let id: u128 = env.storage().persistent().get(&key).unwrap_or(1);
    env.storage().persistent().set(&key, &(id + 1));
    id
}

/// Internal helper: convert `amount` from `from_token` into `to_token` using
/// the configured FX rate stored in `DataKey::ExchangeRate`.
///
/// The rate is interpreted as `quote_per_base * FX_SCALE`, where `from_token`
/// is the base and `to_token` is the quote.
fn convert_amount(
    env: &Env,
    from_token: &Address,
    to_token: &Address,
    amount: i128,
) -> Result<i128, PayrollError> {
    if amount == 0 || from_token == to_token {
        return Ok(amount);
    }

    let rate = DataKey::get_exchange_rate(env, from_token, to_token)
        .ok_or(PayrollError::ExchangeRateNotFound)?;

    if rate <= 0 {
        return Err(PayrollError::ExchangeRateInvalid);
    }

    let scaled = amount
        .checked_mul(rate)
        .ok_or(PayrollError::ExchangeRateOverflow)?;

    let converted = scaled
        .checked_div(FX_SCALE)
        .ok_or(PayrollError::ExchangeRateInvalid)?;

    Ok(converted)
}

/// Pauses an active agreement, preventing claims
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the agreement to pause
///
/// # State Transition
/// Active -> Paused
///
/// # Access Control
/// Requires employer authentication
///
/// # Requirements
/// - Agreement must be in Active status
/// - Only the employer can pause the agreement
///
/// # Behavior
/// - Paused agreements cannot have claims processed
/// - Agreement state is preserved
/// - Can be resumed later or cancelled
pub fn pause_agreement(env: &Env, agreement_id: u128) {
    let mut agreement = get_agreement(env, agreement_id).expect("Agreement not found");

    agreement.employer.require_auth();

    assert!(
        agreement.status == AgreementStatus::Active,
        "Can only pause Active agreements"
    );

    agreement.status = AgreementStatus::Paused;

    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);

    emit_agreement_paused(env, AgreementPausedEvent { agreement_id });
}

/// Resumes a paused agreement, allowing claims again
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the agreement to resume
///
/// # State Transition
/// Paused -> Active
///
/// # Access Control
/// Requires employer authentication
///
/// # Requirements
/// - Agreement must be in Paused status
/// - Only the employer can resume the agreement
///
/// # Behavior
/// - Agreement returns to Active status
/// - Claims can be processed again
/// - All agreement data is preserved
pub fn resume_agreement(env: &Env, agreement_id: u128) {
    let mut agreement = get_agreement(env, agreement_id).expect("Agreement not found");

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
/// * `env` - Contract environment
/// * `agreement_id` - ID of the milestone agreement to pause
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
pub fn pause_milestone_agreement(env: Env, agreement_id: u128) {
    let employer: Address = env
        .storage()
        .instance()
        .get(&MilestoneKey::Employer(agreement_id))
        .expect("Agreement not found");
    employer.require_auth();

    let status: AgreementStatus = env
        .storage()
        .instance()
        .get(&MilestoneKey::Status(agreement_id))
        .expect("Agreement not found");

    // Allow pausing Active agreements, or Created agreements (which can have claimable milestones)
    assert!(
        status == AgreementStatus::Active || status == AgreementStatus::Created,
        "Can only pause Active or Created agreements"
    );

    env.storage().instance().set(
        &MilestoneKey::Status(agreement_id),
        &AgreementStatus::Paused,
    );

    AgreementPausedEvent { agreement_id }.publish(&env);
}

/// Resumes a paused milestone-based agreement, allowing claims again
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the milestone agreement to resume
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
pub fn resume_milestone_agreement(env: Env, agreement_id: u128) {
    let employer: Address = env
        .storage()
        .instance()
        .get(&MilestoneKey::Employer(agreement_id))
        .expect("Agreement not found");
    employer.require_auth();

    let status: AgreementStatus = env
        .storage()
        .instance()
        .get(&MilestoneKey::Status(agreement_id))
        .expect("Agreement not found");

    assert!(
        status == AgreementStatus::Paused,
        "Can only resume Paused agreements"
    );

    // Resume to Active status (milestone agreements can have claimable milestones in Active state)
    env.storage().instance().set(
        &MilestoneKey::Status(agreement_id),
        &AgreementStatus::Active,
    );

    AgreementResumedEvent { agreement_id }.publish(&env);
}

fn add_to_employer_agreements(env: &Env, employer: &Address, agreement_id: u128) {
    let key = StorageKey::EmployerAgreements(employer.clone());
    let mut agreements: Vec<u128> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env));
    agreements.push_back(agreement_id);
    env.storage().persistent().set(&key, &agreements);
}

// -----------------------------------------------------------------------------
// Grace Period and Cancellation
// -----------------------------------------------------------------------------

/// Cancels an agreement, initiating the grace period.
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the agreement to cancel
///
/// # Requirements
/// - Agreement must be in Active or Created status
/// - Caller must be the employer
///
/// # State Transition
/// Active/Created -> Cancelled
///
/// # Behavior
/// - Sets cancelled_at timestamp
/// - Claims are allowed during grace period
/// - Refunds are prevented until grace period expires
pub fn cancel_agreement(env: &Env, agreement_id: u128) {
    let mut agreement = get_agreement(env, agreement_id).expect("Agreement not found");

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
    let agreement = get_agreement(env, agreement_id).expect("Agreement not found");

    agreement.employer.require_auth();

    assert!(
        agreement.status == AgreementStatus::Cancelled,
        "Agreement must be cancelled"
    );

    let cancelled_at = agreement
        .cancelled_at
        .expect("Cancelled agreement must have cancelled_at timestamp");

    let current_time = env.ledger().timestamp();
    let effective_grace = effective_cancelled_grace_duration_seconds(
        env,
        agreement_id,
        agreement.grace_period_seconds,
    );
    let grace_end = cancelled_at
        .checked_add(effective_grace)
        .expect("grace end timestamp overflow");

    assert!(
        current_time >= grace_end,
        "Grace period has not expired yet"
    );

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
    let owner: Address = env.storage().persistent().get(&StorageKey::Owner).unwrap();
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
    let owner: Address = env.storage().persistent().get(&StorageKey::Owner).unwrap();
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
    let owner: Address = env.storage().persistent().get(&StorageKey::Owner).unwrap();
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
