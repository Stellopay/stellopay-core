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
    emit_agreement_activated, emit_agreement_created, emit_employee_added,
    AgreementActivatedEvent, AgreementCreatedEvent, EmployeeAddedEvent,
};
use crate::storage::{
    Agreement, AgreementMode, AgreementStatus, DataKey, EmployeeInfo, StorageKey,
};

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

/// Event data for payroll claiming
#[derive(Clone)]
#[contracttype]
pub struct PayrollClaimedEvent {
    pub agreement_id: u128,
    pub employee: Address,
    pub amount: i128,
    pub periods: u32,
}

/// Event data for payment sent
#[derive(Clone)]
#[contracttype]
pub struct PaymentSentEvent {
    pub agreement_id: u128,
    pub from: Address,
    pub to: Address,
    pub amount: i128,
    pub token: Address,
}

/// Event data for payment received
#[derive(Clone)]
#[contracttype]
pub struct PaymentReceivedEvent {
    pub agreement_id: u128,
    pub to: Address,
    pub amount: i128,
    pub token: Address,
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
    };

    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);

    // Initialize empty employee list
    let employees: Vec<EmployeeInfo> = Vec::new(env);
    env.storage().persistent().set(
        &StorageKey::AgreementEmployees(agreement_id),
        &employees,
    );

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
    env.storage().persistent().set(
        &StorageKey::AgreementEmployees(agreement_id),
        &employees,
    );

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
    env.storage().persistent().set(
        &StorageKey::AgreementEmployees(agreement_id),
        &employees,
    );

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
    env.events().publish(
        (Symbol::new(env, "PayrollClaimed"),),
        PayrollClaimedEvent {
            agreement_id,
            employee: employee.clone(),
            amount,
            periods: periods_to_pay,
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

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn get_next_agreement_id(env: &Env) -> u128 {
    let key = StorageKey::NextAgreementId;
    let id: u128 = env.storage().persistent().get(&key).unwrap_or(1);
    env.storage().persistent().set(&key, &(id + 1));
    id
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
