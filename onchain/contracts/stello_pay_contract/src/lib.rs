#![no_std]
mod events;
mod payroll;
pub mod storage;

use soroban_sdk::{contract, contractimpl, Address, Env, Vec};
#[cfg(test)]
mod test_milestones;

use storage::{Agreement, Milestone, StorageKey};

/// Payroll Contract for managing payroll agreements with employee claiming functionality.
///
/// This contract supports:
/// - Multiple employees per agreement with individual salary tracking
/// - Per-employee period tracking for claimed salaries
/// - Employee-initiated payroll claiming based on elapsed time periods
/// - Secure escrow fund release
/// - Grace period support for claims
#[contract]
pub struct PayrollContract;

#[contractimpl]
impl PayrollContract {
    /// One-time initialization hook.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment
    /// * `owner` - The contract owner address
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

    /// Creates a milestone-based payment agreement.
    ///
    /// # Arguments
    /// * `employer` - Address of the employer who will approve milestones
    /// * `contributor` - Address of the contributor who will complete work
    /// * `token` - Token address for payments
    ///
    /// # Returns
    /// New agreement ID
    ///
    /// # State Transition
    /// None -> Created
    ///
    /// # Access Control
    /// Requires employer authentication
    pub fn create_milestone_agreement(
        env: Env,
        employer: Address,
        contributor: Address,
        token: Address,
    ) -> u128 {
        payroll::create_milestone_agreement(env, employer, contributor, token)
    }

    /// Adds a milestone to a milestone-based agreement.
    ///
    /// # Arguments
    /// * `agreement_id` - ID of the agreement
    /// * `amount` - Payment amount for this milestone
    ///
    /// # Requirements
    /// - Agreement must be in Created status
    /// - Amount must be positive
    /// - Caller must be the employer
    pub fn add_milestone(env: Env, agreement_id: u128, amount: i128) {
        payroll::add_milestone(env, agreement_id, amount);
    }

    /// Approves a milestone for payment.
    ///
    /// # Arguments
    /// * `agreement_id` - ID of the agreement
    /// * `milestone_id` - ID of the milestone to approve
    ///
    /// # Requirements
    /// - Milestone must exist
    /// - Milestone must not be already approved
    /// - Caller must be the employer
    pub fn approve_milestone(env: Env, agreement_id: u128, milestone_id: u32) {
        payroll::approve_milestone(env, agreement_id, milestone_id);
    }

    /// Claims payment for an approved milestone.
    ///
    /// # Arguments
    /// * `agreement_id` - ID of the agreement
    /// * `milestone_id` - ID of the milestone to claim
    ///
    /// # Requirements
    /// - Milestone must be approved
    /// - Milestone must not be already claimed
    /// - Caller must be the contributor
    /// - Agreement auto-completes when all milestones are claimed
    pub fn claim_milestone(env: Env, agreement_id: u128, milestone_id: u32) {
        payroll::claim_milestone(env, agreement_id, milestone_id);
    }

    /// Gets the total number of milestones for an agreement.
    ///
    /// # Arguments
    /// * `agreement_id` - ID of the agreement
    ///
    /// # Returns
    /// Number of milestones
    pub fn get_milestone_count(env: Env, agreement_id: u128) -> u32 {
        payroll::get_milestone_count(env, agreement_id)
    }

    /// Gets details of a specific milestone.
    ///
    /// # Arguments
    /// * `agreement_id` - ID of the agreement
    /// * `milestone_id` - ID of the milestone
    ///
    /// # Returns
    /// Milestone details if it exists, None otherwise
    pub fn get_milestone(env: Env, agreement_id: u128, milestone_id: u32) -> Option<Milestone> {
        payroll::get_milestone(env, agreement_id, milestone_id)
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

    /// Claim payroll for an employee in a payroll agreement.
    ///
    /// Allows an employee to claim their salary based on elapsed time periods since
    /// the agreement was activated. Each employee has individual period tracking,
    /// allowing independent claiming.
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
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Employee at index 0 claims their payroll
    /// let employee = Address::from_str("...");
    /// employee.require_auth();
    /// contract.claim_payroll(&env, employee, 1u128, 0u32)?;
    /// ```
    pub fn claim_payroll(
        env: Env,
        caller: Address,
        agreement_id: u128,
        employee_index: u32,
    ) -> Result<(), soroban_sdk::Error> {
        caller.require_auth();
        payroll::claim_payroll(&env, &caller, agreement_id, employee_index).map_err(Into::into)
    }

    /// Get the number of periods already claimed by an employee.
    ///
    /// This function returns the individual claimed period count for a specific employee
    /// within an agreement, enabling independent tracking per employee.
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
    ///
    /// # Example
    ///
    /// ```ignore
    /// let claimed = contract.get_employee_claimed_periods(&env, 1u128, 0u32);
    /// println!("Employee has claimed {} periods", claimed);
    /// ```
    pub fn get_employee_claimed_periods(env: Env, agreement_id: u128, employee_index: u32) -> u32 {
        payroll::get_employee_claimed_periods(&env, agreement_id, employee_index)
    }

    /// Pauses an active agreement, preventing claims.
    ///
    /// # Arguments
    /// * `agreement_id` - ID of the agreement to pause
    ///
    /// # State Transition
    /// Active -> Paused
    ///
    /// # Requirements
    /// - Agreement must be in Active status
    /// - Caller must be the employer
    ///
    /// # Behavior
    /// - Paused agreements cannot have claims processed
    /// - Agreement state is preserved
    /// - Can be resumed later or cancelled
    pub fn pause_agreement(env: Env, agreement_id: u128) {
        // Try new-style agreement first (payroll/escrow)
        if payroll::get_agreement(&env, agreement_id).is_some() {
            payroll::pause_agreement(&env, agreement_id);
            return;
        }

        // Fall back to milestone-based agreement
        payroll::pause_milestone_agreement(env, agreement_id);
    }

    /// Resumes a paused agreement, allowing claims again.
    ///
    /// # Arguments
    /// * `agreement_id` - ID of the agreement to resume
    ///
    /// # State Transition
    /// Paused -> Active
    ///
    /// # Requirements
    /// - Agreement must be in Paused status
    /// - Caller must be the employer
    ///
    /// # Behavior
    /// - Agreement returns to Active status
    /// - Claims can be processed again
    /// - All agreement data is preserved
    pub fn resume_agreement(env: Env, agreement_id: u128) {
        // Try new-style agreement first (payroll/escrow)
        if payroll::get_agreement(&env, agreement_id).is_some() {
            payroll::resume_agreement(&env, agreement_id);
            return;
        }

        // Fall back to milestone-based agreement
        payroll::resume_milestone_agreement(env, agreement_id);
    }

    /// Claims time-based payments for an escrow agreement based on elapsed periods.
    ///
    /// # Arguments
    /// * `agreement_id` - ID of the escrow agreement
    ///
    /// # Requirements
    /// - Agreement must be Active and activated
    /// - Agreement must be Escrow mode
    /// - Caller must be the contributor
    /// - Cannot claim more than total periods
    /// - Works during grace period
    pub fn claim_time_based(env: Env, agreement_id: u128) {
        payroll::claim_time_based(&env, agreement_id);
    }

    /// Gets the number of claimed periods for a time-based escrow agreement.
    ///
    /// # Arguments
    /// * `agreement_id` - ID of the agreement
    ///
    /// # Returns
    /// Number of claimed periods, or 0 if not a time-based agreement
    pub fn get_claimed_periods(env: Env, agreement_id: u128) -> u32 {
        payroll::get_claimed_periods(&env, agreement_id)
    }
}
