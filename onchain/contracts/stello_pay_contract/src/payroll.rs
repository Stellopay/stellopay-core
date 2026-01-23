use core::ops::Add;

use soroban_sdk::token::TokenClient;
use soroban_sdk::{Address, Env, Vec};

use soroban_sdk::{
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    contracttype,
    token,
    Address,
    Env,
    Error,
    IntoVal,
    Symbol,
    Val,
    Vec,
};
use crate::events::{
    emit_agreement_activated, emit_agreement_created, emit_agreement_paused,
    emit_agreement_resumed, emit_dsipute_raised, emit_dsipute_resolved, emit_employee_added,
    emit_payment_received, emit_payment_sent, emit_payroll_claimed, emit_set_arbiter,
    AgreementActivatedEvent, AgreementCreatedEvent, AgreementPausedEvent, AgreementResumedEvent,
    ArbiterSetEvent, DisputeRaisedEvent, DisputeResolvedEvent, EmployeeAddedEvent, MilestoneAdded,
    MilestoneApproved, MilestoneClaimed, PaymentReceivedEvent, PaymentSentEvent,
    PayrollClaimedEvent,
};
use crate::storage::{
    Agreement, AgreementMode, AgreementStatus, DataKey, DisputeStatus, EmployeeInfo, Milestone, MilestoneKey,
    PaymentType, PayrollError, StorageKey,
};

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
    env.storage()
        .instance()
        .set(&MilestoneKey::Status(agreement_id), &AgreementStatus::Created);
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
        .get(&MilestoneKey::Employer(agreement_id))
        .expect("Employer not found");
    employer.require_auth();

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
            .set(&MilestoneKey::Status(agreement_id), &AgreementStatus::Completed);
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

/// Error types for payroll operations
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[contracttype]
#[repr(u32)]
pub enum PayrollError {
    Unauthorized = 1,
    InvalidEmployeeIndex = 2,
    InvalidData = 3,
    AgreementNotFound = 4,
    TransferFailed = 5,
    InsufficientEscrowBalance = 6,
    NoPeriodsToClaim = 7,
    AgreementNotActivated = 8,
}

impl From<PayrollError> for Error {
    fn from(err: PayrollError) -> Self {
        Error::from_contract_error(err as u32)
    }
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

/// Raise Dispute
///
/// # Arguments
/// * `env` - Contract environment
/// * agreement_id` - ID of the agreement to raise dispute for
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
    let created_time = agreement.created_at;
    if created_time + agreement.grace_period_seconds <= now {
        return Err(PayrollError::NotInGracePeriod);
    }

    agreement.dispute_status = DisputeStatus::Raised;
    agreement.dispute_raised_at = Some(now);

    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);

    emit_dsipute_raised(env, DisputeRaisedEvent { agreement_id });

    Ok(())
}

/// Resove Dispute
///
/// # Arguments
/// * `env` - Contract environment
/// * agreement_id` - ID of the agreement to raise dispute for
/// * pay_employee` - ID of the agreement to raise dispute for
/// * refund_employer` - ID of the agreement to raise dispute for
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
    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);

    emit_dsipute_resolved(
        &env,
        DisputeResolvedEvent {
            agreement_id,
            pay_contributor: pay_employee,
            refund_employer: refund_employer,
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
    // Validate employee index
    let employee_count = DataKey::get_employee_count(env, agreement_id);
    if employee_index >= employee_count {
        return Err(PayrollError::InvalidEmployeeIndex);
    }

    // Get agreement and check status
    let agreement = get_agreement(env, agreement_id)
        .ok_or(PayrollError::AgreementNotFound)?;

    // Check if agreement is paused
    if agreement.status == AgreementStatus::Paused {
        return Err(PayrollError::InvalidData);
    }

    // Check if agreement is active
    if agreement.status != AgreementStatus::Active {
        return Err(PayrollError::InvalidData);
    }

    // Handle cancelled agreement grace period check
    if agreement.status == AgreementStatus::Cancelled {
        let current_time = env.ledger().timestamp();
        if let Some(cancelled_at) = agreement.cancelled_at {
            let grace_end = cancelled_at + agreement.grace_period_seconds;
            if current_time <= grace_end {
                return Err(PayrollError::InvalidData);
            }
        }
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
    let token = DataKey::get_agreement_token(env, agreement_id)
        .ok_or(PayrollError::AgreementNotFound)?;

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
    let claimed_periods =
        DataKey::get_employee_claimed_periods(env, agreement_id, employee_index);

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

    env.events().publish(
        (Symbol::new(env, "PaymentSent"),),
        PaymentSentEvent {
            agreement_id,
            from: contract_address,
            to: employee.clone(),
            amount,
            token: token.clone(),
        },
    );

    env.events().publish(
        (Symbol::new(env, "PaymentReceived"),),
        PaymentReceivedEvent {
            agreement_id,
            to: employee,
            amount,
            token,
        },
    );

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
    let mut claimed_periods = agreement.claimed_periods.unwrap_or(0);

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

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

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

    env.storage()
        .instance()
        .set(&MilestoneKey::Status(agreement_id), &AgreementStatus::Paused);

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
    env.storage()
        .instance()
        .set(&MilestoneKey::Status(agreement_id), &AgreementStatus::Active);

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
