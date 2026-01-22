use crate::events::{
    emit_agreement_activated, emit_agreement_created, emit_agreement_paused,
    emit_agreement_resumed, emit_employee_added, emit_payroll_claimed, emit_payment_received,
    emit_payment_sent,
    AgreementActivatedEvent, AgreementCreatedEvent, AgreementPausedEvent,
    AgreementResumedEvent, EmployeeAddedEvent, MilestoneAdded, MilestoneApproved,
    MilestoneClaimed, PayrollClaimedEvent, PaymentReceivedEvent, PaymentSentEvent,
};
use crate::storage::{
    Agreement, AgreementMode, AgreementStatus, DataKey, EmployeeInfo, Milestone, PaymentType,
    StorageKey,
};
use soroban_sdk::{Address, Env, Vec};

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
        .get(&DataKey::AgreementCounter)
        .unwrap_or(0);
    counter += 1;

    let agreement_id = counter;

    env.storage()
        .instance()
        .set(&DataKey::AgreementCounter, &counter);
    env.storage()
        .instance()
        .set(&DataKey::Employer(agreement_id), &employer);
    env.storage()
        .instance()
        .set(&DataKey::Contributor(agreement_id), &contributor);
    env.storage()
        .instance()
        .set(&DataKey::Token(agreement_id), &token);
    env.storage().instance().set(
        &DataKey::PaymentType(agreement_id),
        &PaymentType::MilestoneBased,
    );
    env.storage()
        .instance()
        .set(&DataKey::Status(agreement_id), &AgreementStatus::Created);
    env.storage()
        .instance()
        .set(&DataKey::TotalAmount(agreement_id), &0i128);
    env.storage()
        .instance()
        .set(&DataKey::MilestoneCount(agreement_id), &0u32);

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
        .get(&DataKey::Status(agreement_id))
        .expect("Agreement not found");

    assert!(
        status == AgreementStatus::Created,
        "Agreement must be in Created status"
    );
    assert!(amount > 0, "Amount must be positive");

    let employer: Address = env
        .storage()
        .instance()
        .get(&DataKey::Employer(agreement_id))
        .expect("Employer not found");
    employer.require_auth();

    let count: u32 = env
        .storage()
        .instance()
        .get(&DataKey::MilestoneCount(agreement_id))
        .unwrap_or(0);

    let milestone_id = count + 1;

    env.storage().instance().set(
        &DataKey::MilestoneAmount(agreement_id, milestone_id),
        &amount,
    );
    env.storage().instance().set(
        &DataKey::MilestoneApproved(agreement_id, milestone_id),
        &false,
    );
    env.storage().instance().set(
        &DataKey::MilestoneClaimed(agreement_id, milestone_id),
        &false,
    );
    env.storage()
        .instance()
        .set(&DataKey::MilestoneCount(agreement_id), &milestone_id);

    let total: i128 = env
        .storage()
        .instance()
        .get(&DataKey::TotalAmount(agreement_id))
        .unwrap_or(0);
    env.storage()
        .instance()
        .set(&DataKey::TotalAmount(agreement_id), &(total + amount));

    env.events().publish(
        ("milestone_added", agreement_id),
        MilestoneAdded {
            agreement_id,
            milestone_id,
            amount,
        },
    );
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
        .get(&DataKey::Employer(agreement_id))
        .expect("Employer not found");
    employer.require_auth();

    let count: u32 = env
        .storage()
        .instance()
        .get(&DataKey::MilestoneCount(agreement_id))
        .expect("No milestones found");
    assert!(
        milestone_id > 0 && milestone_id <= count,
        "Invalid milestone ID"
    );

    let already_approved: bool = env
        .storage()
        .instance()
        .get(&DataKey::MilestoneApproved(agreement_id, milestone_id))
        .unwrap_or(false);
    assert!(!already_approved, "Milestone already approved");

    env.storage().instance().set(
        &DataKey::MilestoneApproved(agreement_id, milestone_id),
        &true,
    );

    env.events().publish(
        ("milestone_approved", agreement_id),
        MilestoneApproved {
            agreement_id,
            milestone_id,
        },
    );
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
    let contributor: Address = env
        .storage()
        .instance()
        .get(&DataKey::Contributor(agreement_id))
        .expect("Contributor not found");
    contributor.require_auth();

    // Check if agreement is paused
    let status: AgreementStatus = env
        .storage()
        .instance()
        .get(&DataKey::Status(agreement_id))
        .expect("Agreement not found");
    assert!(
        status != AgreementStatus::Paused,
        "Cannot claim when agreement is paused"
    );

    let count: u32 = env
        .storage()
        .instance()
        .get(&DataKey::MilestoneCount(agreement_id))
        .expect("No milestones found");
    assert!(
        milestone_id > 0 && milestone_id <= count,
        "Invalid milestone ID"
    );

    let approved: bool = env
        .storage()
        .instance()
        .get(&DataKey::MilestoneApproved(agreement_id, milestone_id))
        .unwrap_or(false);
    assert!(approved, "Milestone not approved");

    let already_claimed: bool = env
        .storage()
        .instance()
        .get(&DataKey::MilestoneClaimed(agreement_id, milestone_id))
        .unwrap_or(false);
    assert!(!already_claimed, "Milestone already claimed");

    let amount: i128 = env
        .storage()
        .instance()
        .get(&DataKey::MilestoneAmount(agreement_id, milestone_id))
        .expect("Milestone amount not found");

    env.storage().instance().set(
        &DataKey::MilestoneClaimed(agreement_id, milestone_id),
        &true,
    );

    let _token: Address = env
        .storage()
        .instance()
        .get(&DataKey::Token(agreement_id))
        .expect("Token not found");

    env.events().publish(
        ("milestone_claimed", agreement_id),
        MilestoneClaimed {
            agreement_id,
            milestone_id,
            amount,
            to: contributor.clone(),
        },
    );

    let all_claimed = all_milestones_claimed(&env, agreement_id, count);
    if all_claimed {
        env.storage()
            .instance()
            .set(&DataKey::Status(agreement_id), &AgreementStatus::Completed);
    }
}

pub fn get_milestone_count(env: Env, agreement_id: u128) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::MilestoneCount(agreement_id))
        .unwrap_or(0)
}

pub fn get_milestone(env: Env, agreement_id: u128, milestone_id: u32) -> Option<Milestone> {
    let count: u32 = env
        .storage()
        .instance()
        .get(&DataKey::MilestoneCount(agreement_id))
        .unwrap_or(0);

    if milestone_id == 0 || milestone_id > count {
        return None;
    }

    let amount: i128 = env
        .storage()
        .instance()
        .get(&DataKey::MilestoneAmount(agreement_id, milestone_id))?;
    let approved: bool = env
        .storage()
        .instance()
        .get(&DataKey::MilestoneApproved(agreement_id, milestone_id))
        .unwrap_or(false);
    let claimed: bool = env
        .storage()
        .instance()
        .get(&DataKey::MilestoneClaimed(agreement_id, milestone_id))
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
            .get(&DataKey::MilestoneClaimed(agreement_id, i))
            .unwrap_or(false);
        if !claimed {
            return false;
        }
    }
    true
}

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
        amount_per_period: None,
        period_seconds: None,
        num_periods: None,
        claimed_periods: None,
    };

    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);

    // Initialize empty employee list
    let employees: Vec<EmployeeInfo> = Vec::new(env);
    env.storage()
        .persistent()
        .set(&StorageKey::AgreementEmployees(agreement_id), &employees);

    // Track employer's agreements
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
) -> u128 {
    employer.require_auth();

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

    agreement_id
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

    agreement.status = AgreementStatus::Active;
    agreement.activated_at = Some(env.ledger().timestamp());

    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);

    emit_agreement_activated(env, AgreementActivatedEvent { agreement_id });
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

/// Claims payroll for the calling employee
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the agreement
/// * `employee` - Address of the employee claiming
///
/// # Requirements
/// - Agreement must be Active (not Paused, Cancelled, etc.)
/// - Caller must be an employee of the agreement
/// - If agreement is Cancelled, must be past grace period
pub fn claim_payroll(env: &Env, agreement_id: u128, employee: Address) {
    employee.require_auth();

    let agreement: Agreement = env
        .storage()
        .persistent()
        .get(&StorageKey::Agreement(agreement_id))
        .expect("Agreement not found");

    // Check if agreement is paused
    assert!(
        agreement.status != AgreementStatus::Paused,
        "Cannot claim when agreement is paused"
    );

    // Check if agreement is active
    assert!(
        agreement.status == AgreementStatus::Active,
        "Agreement must be active"
    );

    // Note: The cancelled check is redundant here since we already check for Active,
    // but keeping it for clarity and future-proofing
    if agreement.status == AgreementStatus::Cancelled {
        let current_time = env.ledger().timestamp();
        let cancelled_at = agreement
            .cancelled_at
            .expect("Cancelled agreement must have cancelled_at");
        let grace_end = cancelled_at + agreement.grace_period_seconds;
        assert!(current_time > grace_end, "Cannot claim during grace period");
    }

    let employees: Vec<EmployeeInfo> = env
        .storage()
        .persistent()
        .get(&StorageKey::AgreementEmployees(agreement_id))
        .unwrap_or(Vec::new(env));

    let mut employee_info: Option<EmployeeInfo> = None;
    for emp in employees.iter() {
        if emp.address == employee {
            employee_info = Some(emp.clone());
            break;
        }
    }
    let employee_info = employee_info.expect("Caller is not an employee");

    let mut agreement = agreement;
    agreement.paid_amount += employee_info.salary_per_period;
    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);

    emit_payroll_claimed(
        env,
        PayrollClaimedEvent {
            agreement_id,
            employee,
            amount: employee_info.salary_per_period,
        },
    );
}

/// Claims time-based payments for an escrow agreement based on elapsed periods
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the escrow agreement
///
/// # Requirements
/// - Agreement must be Active (not Paused, Cancelled, etc.)
/// - Agreement must be activated
/// - Caller must be the contributor
/// - Cannot claim more than total periods
/// - Works during grace period
pub fn claim_time_based(env: &Env, agreement_id: u128) {
    let mut agreement = get_agreement(env, agreement_id).expect("Agreement not found");

    assert!(
        agreement.mode == AgreementMode::Escrow,
        "Can only claim time-based payments for escrow agreements"
    );

    assert!(
        agreement.status != AgreementStatus::Paused,
        "Cannot claim when agreement is paused"
    );

    assert!(
        agreement.status == AgreementStatus::Active,
        "Agreement must be active"
    );

    let employees: Vec<EmployeeInfo> = env
        .storage()
        .persistent()
        .get(&StorageKey::AgreementEmployees(agreement_id))
        .unwrap_or(Vec::new(env));

    let contributor = employees
        .get(0)
        .expect("Escrow agreement must have a contributor")
        .address
        .clone();

    contributor.require_auth();

    let activated_at = agreement
        .activated_at
        .expect("Agreement must be activated before claiming");

    let amount_per_period = agreement
        .amount_per_period
        .expect("Agreement must have amount_per_period");
    let period_seconds = agreement
        .period_seconds
        .expect("Agreement must have period_seconds");
    let num_periods = agreement
        .num_periods
        .expect("Agreement must have num_periods");
    let mut claimed_periods = agreement
        .claimed_periods
        .unwrap_or(0);

    assert!(
        claimed_periods < num_periods,
        "All periods have been claimed"
    );

    let current_time = env.ledger().timestamp();
    let elapsed_seconds = current_time - activated_at;
    let periods_elapsed = (elapsed_seconds / period_seconds) as u32;

    let periods_to_pay = if periods_elapsed > num_periods {
        num_periods - claimed_periods
    } else {
        periods_elapsed - claimed_periods
    };

    assert!(periods_to_pay > 0, "No periods available to claim");

    let amount = amount_per_period * (periods_to_pay as i128);

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

// Helper functions

fn get_next_agreement_id(env: &Env) -> u128 {
    let key = StorageKey::NextAgreementId;
    let id: u128 = env.storage().persistent().get(&key).unwrap_or(1);
    env.storage().persistent().set(&key, &(id + 1));
    id
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
        .get(&DataKey::Employer(agreement_id))
        .expect("Agreement not found");
    employer.require_auth();

    let status: AgreementStatus = env
        .storage()
        .instance()
        .get(&DataKey::Status(agreement_id))
        .expect("Agreement not found");

    // Allow pausing Active agreements, or Created agreements (which can have claimable milestones)
    assert!(
        status == AgreementStatus::Active || status == AgreementStatus::Created,
        "Can only pause Active or Created agreements"
    );

    env.storage()
        .instance()
        .set(&DataKey::Status(agreement_id), &AgreementStatus::Paused);

    env.events().publish(
        ("agreement_paused", agreement_id),
        AgreementPausedEvent { agreement_id },
    );
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
        .get(&DataKey::Employer(agreement_id))
        .expect("Agreement not found");
    employer.require_auth();

    let status: AgreementStatus = env
        .storage()
        .instance()
        .get(&DataKey::Status(agreement_id))
        .expect("Agreement not found");

    assert!(
        status == AgreementStatus::Paused,
        "Can only resume Paused agreements"
    );

    // Resume to Active status (milestone agreements can have claimable milestones in Active state)
    env.storage()
        .instance()
        .set(&DataKey::Status(agreement_id), &AgreementStatus::Active);

    env.events().publish(
        ("agreement_resumed", agreement_id),
        AgreementResumedEvent { agreement_id },
    );
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
