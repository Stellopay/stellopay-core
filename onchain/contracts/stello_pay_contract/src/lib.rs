#![no_std]

mod events;
mod payroll;
pub mod storage;

use soroban_sdk::{contract, contractimpl, Address, Env, Vec};
use storage::{Agreement, DisputeStatus, Milestone, PayrollError, StorageKey};

/// Payroll Contract for managing payroll agreements with employee claiming functionality.
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
    pub fn create_payroll_agreement(
        env: Env,
        employer: Address,
        token: Address,
        grace_period_seconds: u64,
    ) -> u128 {
        payroll::create_payroll_agreement(&env, employer, token, grace_period_seconds)
    }

    /// Creates an escrow agreement for a single contributor.
    pub fn create_escrow_agreement(
        env: Env,
        employer: Address,
        contributor: Address,
        token: Address,
        amount_per_period: i128,
        period_seconds: u64,
        num_periods: u32,
    ) -> Result<u128, storage::PayrollError> {
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
    pub fn create_milestone_agreement(
        env: Env,
        employer: Address,
        contributor: Address,
        token: Address,
    ) -> u128 {
        payroll::create_milestone_agreement(env, employer, contributor, token)
    }

    /// Adds a milestone to a milestone-based agreement.
    pub fn add_milestone(env: Env, agreement_id: u128, amount: i128) {
        payroll::add_milestone(env, agreement_id, amount);
    }

    /// Approves a milestone for payment.
    pub fn approve_milestone(env: Env, agreement_id: u128, milestone_id: u32) {
        payroll::approve_milestone(env, agreement_id, milestone_id);
    }

    /// Claims payment for an approved milestone.
    pub fn claim_milestone(env: Env, agreement_id: u128, milestone_id: u32) {
        payroll::claim_milestone(env, agreement_id, milestone_id);
    }

    /// Gets the total number of milestones for an agreement.
    pub fn get_milestone_count(env: Env, agreement_id: u128) -> u32 {
        payroll::get_milestone_count(env, agreement_id)
    }

    /// Gets details of a specific milestone.
    pub fn get_milestone(env: Env, agreement_id: u128, milestone_id: u32) -> Option<Milestone> {
        payroll::get_milestone(env, agreement_id, milestone_id)
    }

    /// Adds an employee to a payroll agreement.
    pub fn add_employee_to_agreement(
        env: Env,
        agreement_id: u128,
        employee: Address,
        salary_per_period: i128,
    ) {
        payroll::add_employee_to_agreement(&env, agreement_id, employee, salary_per_period);
    }

    /// Activates an agreement.
    pub fn activate_agreement(env: Env, agreement_id: u128) {
        payroll::activate_agreement(&env, agreement_id);
    }

    /// Retrieves an agreement by ID.
    pub fn get_agreement(env: Env, agreement_id: u128) -> Option<Agreement> {
        payroll::get_agreement(&env, agreement_id)
    }

    /// Retrieves all employee addresses for an agreement.
    pub fn get_agreement_employees(env: Env, agreement_id: u128) -> Vec<Address> {
        payroll::get_agreement_employees(&env, agreement_id)
    }

    /// Set Arbiter.
    pub fn set_arbiter(env: Env, caller: Address, arbiter: Address) -> bool {
        payroll::set_arbiter(&env, caller, arbiter)
    }

    /// Get Arbiter.
    pub fn get_arbiter(env: Env) -> Option<Address> {
        payroll::get_arbiter(&env)
    }

    /// Raise Dispute.
    pub fn raise_dispute(
        env: Env,
        caller: Address,
        agreement_id: u128,
    ) -> Result<(), PayrollError> {
        payroll::raise_dispute(&env, caller, agreement_id)
    }

    /// Resolve Dispute.
    pub fn resolve_dispute(
        env: Env,
        caller: Address,
        agreement_id: u128,
        pay_employee: i128,
        refund_employer: i128,
    ) -> Result<(), PayrollError> {
        payroll::resolve_dispute(env, caller, agreement_id, pay_employee, refund_employer)
    }

    /// Retrieves current dispute status for an agreement by ID.
    pub fn get_dispute_status(env: Env, agreement_id: u128) -> DisputeStatus {
        payroll::get_dispute_status(env, agreement_id)
    }

    /// Claims payroll for the calling employee.
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
    pub fn get_employee_claimed_periods(env: Env, agreement_id: u128, employee_index: u32) -> u32 {
        payroll::get_employee_claimed_periods(&env, agreement_id, employee_index)
    }

    /// Pauses an active agreement.
    pub fn pause_agreement(env: Env, agreement_id: u128) {
        if payroll::get_agreement(&env, agreement_id).is_some() {
            payroll::pause_agreement(&env, agreement_id);
            return;
        }
        payroll::pause_milestone_agreement(env, agreement_id);
    }

    /// Resumes a paused agreement.
    pub fn resume_agreement(env: Env, agreement_id: u128) {
        if payroll::get_agreement(&env, agreement_id).is_some() {
            payroll::resume_agreement(&env, agreement_id);
            return;
        }
        payroll::resume_milestone_agreement(env, agreement_id);
    }

    /// Claims time-based payments for an escrow agreement.
    pub fn claim_time_based(env: Env, agreement_id: u128) -> Result<(), soroban_sdk::Error> {
        payroll::claim_time_based(&env, agreement_id).map_err(Into::into)
    }

    /// Gets the number of claimed periods for a time-based escrow agreement.
    pub fn get_claimed_periods(env: Env, agreement_id: u128) -> u32 {
        payroll::get_claimed_periods(&env, agreement_id)
    }

    /// Cancels an agreement, initiating the grace period.
    pub fn cancel_agreement(env: Env, agreement_id: u128) {
        payroll::cancel_agreement(&env, agreement_id);
    }

    /// Finalizes the grace period and allows refund of remaining balance.
    pub fn finalize_grace_period(env: Env, agreement_id: u128) {
        payroll::finalize_grace_period(&env, agreement_id);
    }

    /// Checks if the grace period is currently active for a cancelled agreement.
    pub fn is_grace_period_active(env: Env, agreement_id: u128) -> bool {
        payroll::is_grace_period_active(&env, agreement_id)
    }

    /// Gets the grace period end timestamp for a cancelled agreement.
    pub fn get_grace_period_end(env: Env, agreement_id: u128) -> Option<u64> {
        payroll::get_grace_period_end(&env, agreement_id)
    }
}
