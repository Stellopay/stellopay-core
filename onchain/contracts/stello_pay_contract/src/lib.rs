#![no_std]
pub mod events;
mod payroll;
pub mod storage;

use soroban_sdk::{contract, contractimpl, Address, Env, Vec};
use stellar_contract_utils::upgradeable::UpgradeableInternal;
use stellar_macros::Upgradeable;
use storage::{
    Agreement, BatchMilestoneResult, BatchPayrollResult, DisputeStatus, Milestone, PayrollError,
    StorageKey,
};

/// Payroll Contract for managing payroll agreements with employee claiming functionality.
///
///
/// This contract supports:
/// - Multiple employees per agreement with individual salary tracking
/// - Per-employee period tracking for claimed salaries
/// - Employee-initiated payroll claiming based on elapsed time periods
/// - Secure escrow fund release
/// - Grace period support for claims
#[derive(Upgradeable)]
#[contract]
pub struct PayrollContract;

/// UpgradeableInternal implementation for PayrollContract
///
///
impl UpgradeableInternal for PayrollContract {
    fn _require_auth(e: &Env, _operator: &Address) {
        let owner: Address = e.storage().persistent().get(&StorageKey::Owner).unwrap();
        owner.require_auth();
    }
}

#[contractimpl]
impl PayrollContract {
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
    /// * `Ok(agreement_id)` - New agreement ID on success
    /// * `Err(PayrollError)` - Error on validation failure
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

    pub fn batch_claim_milestones(
        env: Env,
        agreement_id: u128,
        milestone_ids: Vec<u32>,
    ) -> BatchMilestoneResult {
        payroll::batch_claim_milestones(&env, agreement_id, milestone_ids)
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

    /// Set Arbiter
    ///
    /// # Arguments
    /// * `env` - Contract environment
    /// * `caller` - Address of the caller
    /// * `arbiter` - Address of the arbiter to add
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn set_arbiter(env: Env, caller: Address, arbiter: Address) -> bool {
        payroll::set_arbiter(&env, caller, arbiter)
    }

    /// Gets the current arbiter address
    ///
    /// # Returns
    /// Arbiter address if set, None otherwise
    pub fn get_arbiter(env: Env) -> Option<Address> {
        payroll::get_arbiter(&env)
    }

    /// Raise Dispute
    ///
    /// # Arguments
    /// * `env` - Contract environment
    /// * `caller` - Address of the caller
    /// * `agreement_id` - ID of the agreement to raise dispute for
    ///
    /// # Access Control
    /// Requires caller or employee authentication
    pub fn raise_dispute(
        env: Env,
        caller: Address,
        agreement_id: u128,
    ) -> Result<(), PayrollError> {
        payroll::raise_dispute(&env, caller, agreement_id)
    }

    /// Resolve Dispute
    ///
    /// # Arguments
    /// * `env` - Contract environment
    /// * `caller` - Address of the caller
    /// * `agreement_id` - ID of the agreement to raise dispute for
    /// * `pay_employee` - Amount to pay the employee
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
        payroll::resolve_dispute(env, caller, agreement_id, pay_employee, refund_employer)
    }

    /// Retrieves current dispute status for an agreement by ID
    ///
    /// # Returns
    /// DisputeStatus enum
    pub fn get_dispute_status(env: Env, agreement_id: u128) -> DisputeStatus {
        payroll::get_dispute_status(env, agreement_id)
    }

    /// Sets the global FX rate admin address that is allowed to update
    /// exchange rates in addition to the contract owner (e.g. an oracle
    /// contract responsible for pushing prices on-chain).
    pub fn set_exchange_rate_admin(
        env: Env,
        caller: Address,
        admin: Address,
    ) -> Result<(), PayrollError> {
        payroll::set_exchange_rate_admin(&env, caller, admin)
    }

    /// Configures the FX rate for a `(base, quote)` token pair.
    ///
    /// Access control:
    /// - Contract owner OR
    /// - FX admin set via `set_exchange_rate_admin`
    pub fn set_exchange_rate(
        env: Env,
        caller: Address,
        base: Address,
        quote: Address,
        rate: i128,
    ) -> Result<(), PayrollError> {
        payroll::set_exchange_rate(&env, caller, base, quote, rate)
    }

    /// Converts an `amount` from one token into another using the configured
    /// FX rate, without performing any on-chain transfer. This is useful for
    /// off-chain estimation and validation of multi-currency payouts.
    pub fn convert_currency(
        env: Env,
        from_token: Address,
        to_token: Address,
        amount: i128,
    ) -> Result<i128, PayrollError> {
        payroll::convert_currency(&env, from_token, to_token, amount)
    }

    /// Claim payroll for an employee
    ///
    /// # Arguments
    /// * `env` - Contract environment
    /// * `caller` - Address of the caller
    /// * `agreement_id` - ID of the agreement
    /// * `employee_index` - Index of the employee in the agreement
    ///
    /// # Access Control
    /// Requires caller to be the employee
    pub fn claim_payroll(
        env: Env,
        caller: Address,
        agreement_id: u128,
        employee_index: u32,
    ) -> Result<(), PayrollError> {
        payroll::claim_payroll(&env, &caller, agreement_id, employee_index)
    }

    /// Claims payroll for an employee, but settles the transfer in a
    /// caller-specified payout token. The agreement continues to track its
    /// accounting in the base token while the actual transfer is executed
    /// in the requested payout currency using the configured FX rate.
    pub fn claim_payroll_in_token(
        env: Env,
        caller: Address,
        agreement_id: u128,
        employee_index: u32,
        payout_token: Address,
    ) -> Result<(), PayrollError> {
        payroll::claim_payroll_in_token(&env, &caller, agreement_id, employee_index, payout_token)
    }

    pub fn batch_claim_payroll(
        env: Env,
        caller: Address,
        agreement_id: u128,
        employee_indices: Vec<u32>,
    ) -> Result<BatchPayrollResult, PayrollError> {
        payroll::batch_claim_payroll(&env, &caller, agreement_id, employee_indices)
    }

    /// Get claimed periods for an employee
    ///
    /// # Arguments
    /// * `env` - Contract environment
    /// * `agreement_id` - ID of the agreement
    /// * `employee_index` - Index of the employee in the agreement
    ///
    /// # Returns
    /// Number of periods claimed
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
    /// # Returns
    /// * `Ok(())` on success
    /// * `Err(PayrollError)` on failure
    ///
    /// # Requirements
    /// - Agreement must be Active and activated
    /// - Agreement must be Escrow mode
    /// - Caller must be the contributor
    /// - Cannot claim more than total periods
    /// - Works during grace period
    pub fn claim_time_based(env: Env, agreement_id: u128) -> Result<(), storage::PayrollError> {
        payroll::claim_time_based(&env, agreement_id)
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

    /// Cancels an agreement, initiating the grace period.
    ///
    /// # Arguments
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
    pub fn cancel_agreement(env: Env, agreement_id: u128) {
        payroll::cancel_agreement(&env, agreement_id);
    }

    /// Finalizes the grace period and allows refund of remaining balance.
    ///
    /// # Arguments
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
    pub fn finalize_grace_period(env: Env, agreement_id: u128) {
        payroll::finalize_grace_period(&env, agreement_id);
    }

    /// Checks if the grace period is currently active for a cancelled agreement.
    ///
    /// # Arguments
    /// * `agreement_id` - ID of the agreement
    ///
    /// # Returns
    /// true if grace period is active, false otherwise
    pub fn is_grace_period_active(env: Env, agreement_id: u128) -> bool {
        payroll::is_grace_period_active(&env, agreement_id)
    }

    /// Gets the grace period end timestamp for a cancelled agreement.
    ///
    /// # Arguments
    /// * `agreement_id` - ID of the agreement
    ///
    /// # Returns
    /// Some(timestamp) if agreement is cancelled, None otherwise
    pub fn get_grace_period_end(env: Env, agreement_id: u128) -> Option<u64> {
        payroll::get_grace_period_end(&env, agreement_id)
    }

    // ============================================================================
    // Emergency Pause Functions
    // ============================================================================

    /// Sets emergency guardians for multi-sig pause activation
    ///
    /// # Arguments
    /// * `guardians` - Vector of guardian addresses
    ///
    /// # Access Control
    /// Requires owner authentication
    pub fn set_emergency_guardians(env: Env, guardians: Vec<Address>) {
        payroll::set_emergency_guardians(&env, guardians);
    }

    /// Gets current emergency guardians
    ///
    /// # Returns
    /// Vector of guardian addresses if set
    pub fn get_emergency_guardians(env: Env) -> Option<Vec<Address>> {
        payroll::get_emergency_guardians(&env)
    }

    /// Proposes emergency pause with optional timelock
    ///
    /// # Arguments
    /// * `caller` - Guardian proposing the pause
    /// * `timelock_seconds` - Delay before pause activates (0 for immediate)
    ///
    /// # Access Control
    /// Requires guardian authentication
    pub fn propose_emergency_pause(
        env: Env,
        caller: Address,
        timelock_seconds: u64,
    ) -> Result<(), storage::PayrollError> {
        payroll::propose_emergency_pause(&env, caller, timelock_seconds)
    }

    /// Approves pending emergency pause proposal
    ///
    /// # Arguments
    /// * `caller` - Guardian approving the pause
    ///
    /// # Access Control
    /// Requires guardian authentication
    pub fn approve_emergency_pause(env: Env, caller: Address) -> Result<(), storage::PayrollError> {
        payroll::approve_emergency_pause(&env, caller)
    }

    /// Immediately activates emergency pause (owner only)
    ///
    /// # Access Control
    /// Requires owner authentication
    pub fn emergency_pause(env: Env) -> Result<(), storage::PayrollError> {
        payroll::emergency_pause(&env)
    }

    /// Unpauses contract after emergency resolved
    ///
    /// # Access Control
    /// Requires owner authentication
    pub fn emergency_unpause(env: Env) -> Result<(), storage::PayrollError> {
        payroll::emergency_unpause(&env)
    }

    /// Checks if contract is in emergency pause state
    ///
    /// # Returns
    /// true if paused, false otherwise
    pub fn is_emergency_paused(env: Env) -> bool {
        payroll::is_emergency_paused(&env)
    }

    /// Gets emergency pause state details
    ///
    /// # Returns
    /// EmergencyPause state if set
    pub fn get_emergency_pause_state(env: Env) -> Option<storage::EmergencyPause> {
        payroll::get_emergency_pause_state(&env)
    }
}
