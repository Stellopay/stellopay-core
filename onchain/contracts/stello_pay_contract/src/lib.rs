#![no_std]

mod events;
mod payroll;
mod storage;

use soroban_sdk::{contract, contractimpl, Address, Env, Vec};
use storage::{Agreement, StorageKey, PayrollError};

/// Minimal baseline Soroban contract.
///
/// Contributors will implement all business features from scratch on top of this.
#[contract]
pub struct PayrollContract;

#[contractimpl]
impl PayrollContract {
    /// One-time initialization hook.
    pub fn initialize(env: Env, owner: Address) {
        owner.require_auth();
        env.storage().persistent().set(&StorageKey::Owner, &owner);
    }

    /// Creates a payroll agreement for multiple employees.
    ///
    /// # Arguments
    /// * `employer` - Address of the employer creating the agreement
    /// * `token` - Token address for payments
    /// * `grace_period_seconds` - Grace period before agreement can be cancelled
    ///
    /// # Returns
    /// New agreement ID
    ///
    /// # State Transition
    /// None -> Created
    pub fn create_payroll_agreement(
        env: Env,
        employer: Address,
        token: Address,
        grace_period_seconds: u64,
    ) -> u128 {
        payroll::create_payroll_agreement(&env, employer, token, grace_period_seconds)
    }

    /// Creates an escrow agreement for a single contributor.
    ///
    /// # Arguments
    /// * `employer` - Address of the employer
    /// * `contributor` - Address of the contributor
    /// * `token` - Token address for payments
    /// * `amount_per_period` - Payment amount per period
    /// * `period_seconds` - Duration of each period
    /// * `num_periods` - Number of periods
    ///
    /// # Returns
    /// New agreement ID
    ///
    /// # State Transition
    /// None -> Created
    pub fn create_escrow_agreement(
        env: Env,
        employer: Address,
        contributor: Address,
        token: Address,
        amount_per_period: i128,
        period_seconds: u64,
        num_periods: u32,
    ) -> u128 {
        payroll::create_escrow_agreement(
            &env,
            employer,
            contributor,
            token,
            amount_per_period,
            period_seconds,
            num_periods,
        )
    }

    /// Adds an employee to a payroll agreement.
    ///
    /// # Arguments
    /// * `agreement_id` - ID of the agreement
    /// * `employee` - Address of the employee to add
    /// * `salary_per_period` - Employee's salary per period
    ///
    /// # Requirements
    /// - Agreement must be in Created status
    /// - Agreement must be Payroll mode
    /// - Caller must be the employer
    pub fn add_employee_to_agreement(
        env: Env,
        agreement_id: u128,
        employee: Address,
        salary_per_period: i128,
    ) {
        payroll::add_employee_to_agreement(&env, agreement_id, employee, salary_per_period);
    }

    /// Activates an agreement, making it ready for payments.
    ///
    /// # Arguments
    /// * `agreement_id` - ID of the agreement to activate
    ///
    /// # State Transition
    /// Created -> Active
    ///
    /// # Requirements
    /// - Agreement must be in Created status
    /// - Caller must be the employer
    pub fn activate_agreement(env: Env, agreement_id: u128) {
        payroll::activate_agreement(&env, agreement_id);
    }

    /// Retrieves an agreement by ID.
    ///
    /// # Returns
    /// Agreement details if found, None otherwise
    pub fn get_agreement(env: Env, agreement_id: u128) -> Option<Agreement> {
        payroll::get_agreement(&env, agreement_id)
    }

    /// Retrieves all employee addresses for an agreement.
    ///
    /// # Returns
    /// Vector of employee addresses
    pub fn get_agreement_employees(env: Env, agreement_id: u128) -> Vec<Address> {
        payroll::get_agreement_employees(&env, agreement_id)
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
        payroll::set_arbiter(env, caller, arbiter)
    }

    /// Raise Disoute
    ///
    /// # Arguments
    /// * `env` - Contract environment
    /// * agreement_id` - ID of the agreement to raise dispute for
    ///
    /// # Access Control
    /// Requires employer or employee authentication
    pub fn raise_dispute(env: &Env, caller: Address, agreement_id: u128) -> Result<(), PayrollError> {
        payroll::raise_dispute(env, caller, agreement_id)
    }
}