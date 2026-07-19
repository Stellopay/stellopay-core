#![no_std]
pub mod audit;
pub mod backup;
pub mod events;
mod payroll;
pub mod storage;

use events::{emit_contract_migrated, ContractMigratedEvent};
use rbac_interface::{RbacContractClient, Role};
use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, Vec};
use storage::{
    Agreement, BatchEscrowCreateResult, BatchMilestoneResult, BatchPayrollCreateResult,
    BatchPayrollResult, DisputeStatus, EscrowCreateParams, GracePeriodExtensionPolicy, Milestone,
    PayrollCreateParams, PayrollError, StorageKey,
};

use crate::audit::LifecycleAuditEntry;

/// Payroll Contract for managing payroll agreements with employee claiming functionality.
///
/// # Security Assumptions
/// - The contract owner is trusted and responsible for upgrades and emergency pauses.
/// - Token contracts are assumed to follow the Soroban token interface correctly.
/// - Exchange rate admins are trusted to provide accurate price data.
/// - Employers are responsible for the accuracy of agreement parameters.
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
    fn require_upgrade_admin(env: &Env, operator: &Address) {
        if let Some(rbac_addr) = env
            .storage()
            .persistent()
            .get::<_, Address>(&StorageKey::RbacContract)
        {
            operator.require_auth();
            let rbac = RbacContractClient::new(env, &rbac_addr);
            assert!(
                rbac.has_role(operator, &Role::Admin),
                "Missing required role"
            );
            return;
        }

        let owner: Address = env.storage().persistent().get(&StorageKey::Owner).unwrap();
        operator.require_auth();
        assert!(*operator == owner, "Unauthorized");
    }

    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment
    /// * `owner` - The contract owner address
    ///
    /// # Access Control
    /// Requires caller authentication. Only callable once (implicitly via storage check if needed,
    /// though usually handled by deployment scripts).
    ///
    /// # Security
    /// Sets the initial administrative authority for the contract.
    pub fn initialize(env: Env, owner: Address) {
        owner.require_auth();
        if env.storage().persistent().has(&StorageKey::Owner) {
            panic!("Already initialized");
        }
        env.storage().persistent().set(&StorageKey::Owner, &owner);
    }

    /// Sets the linked RBAC contract address used for admin-gated operations (e.g. upgrades).
    ///
    /// # Arguments
    /// * `owner` - owner parameter
    /// * `rbac_contract` - rbac_contract parameter
    ///
    /// # Access Control
    /// Requires owner authentication
    pub fn set_rbac_contract(env: Env, owner: Address, rbac_contract: Address) {
        let stored_owner: Address = env.storage().persistent().get(&StorageKey::Owner).unwrap();
        owner.require_auth();
        assert!(owner == stored_owner, "Unauthorized");
        env.storage()
            .persistent()
            .set(&StorageKey::RbacContract, &rbac_contract);
    }

    /// Sets the linked Rate Limiter contract address used to throttle claims.
    ///
    /// # Arguments
    /// * `owner` - Owner of the contract
    /// * `rate_limiter` - Rate limiter contract address
    ///
    /// # Access Control
    /// Requires owner authentication
    pub fn set_rate_limiter_contract(env: Env, owner: Address, rate_limiter: Address) {
        let stored_owner: Address = env.storage().persistent().get(&StorageKey::Owner).unwrap();
        owner.require_auth();
        assert!(owner == stored_owner, "Unauthorized");
        env.storage()
            .persistent()
            .set(&StorageKey::RateLimiterContract, &rate_limiter);
    }

    /// Gets the linked Rate Limiter contract address, if any.
    pub fn get_rate_limiter_contract(env: Env) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&StorageKey::RateLimiterContract)
    }

    /// Sets the linked Salary Adjustment contract address used for dynamic salary overrides.
    ///
    /// # Arguments
    /// * `owner` - Owner of the contract
    /// * `salary_adjustment` - Salary adjustment contract address
    ///
    /// # Access Control
    /// Requires owner authentication
    pub fn set_salary_adjustment_contract(env: Env, owner: Address, salary_adjustment: Address) {
        let stored_owner: Address = env.storage().persistent().get(&StorageKey::Owner).unwrap();
        owner.require_auth();
        assert!(owner == stored_owner, "Unauthorized");
        env.storage()
            .persistent()
            .set(&StorageKey::SalaryAdjustmentContract, &salary_adjustment);
    }

    /// Gets the linked Salary Adjustment contract address, if any.
    pub fn get_salary_adjustment_contract(env: Env) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&StorageKey::SalaryAdjustmentContract)
    }

    /// @notice Upgrades the contract's WASM code to a new version.
    /// @dev Highly critical administrative function to alter contract bytecode.
    /// Gated strictly by require_upgrade_admin logic.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment.
    /// * `new_wasm_hash` - The 32-byte SHA-256 hash of the uploaded new WASM code.
    /// * `operator` - The address initiating the upgrade, which must possess administrative authority.
    ///
    /// # Access Control
    /// - If an RBAC contract is configured, the `operator` must possess the `Admin` role.
    /// - Otherwise, the `operator` must be the stored contract owner.
    /// - `operator.require_auth()` is called to verify authorization signature.
    ///
    /// # Security Assumptions
    /// - The `new_wasm_hash` must represent a valid, pre-uploaded WASM blob.
    /// - The new bytecode must correctly preserve existing storage keys/layouts to prevent state corruption.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>, operator: Address) {
        Self::require_upgrade_admin(&env, &operator);
        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }

    /// Migrates persistent storage state from a previous schema version.
    ///
    /// # Arguments
    /// * `operator` - operator parameter
    /// * `from_version` - from_version parameter
    ///
    /// # Access Control
    /// Requires admin authorization via RBAC when configured (or owner auth when RBAC is unset).
    pub fn migrate_state(env: Env, operator: Address, from_version: u32) {
        Self::require_upgrade_admin(&env, &operator);

        let current: u32 = env
            .storage()
            .persistent()
            .get(&StorageKey::ContractVersion)
            .unwrap_or(0u32);
        assert!(from_version == current, "Invalid migration version");

        // v0 -> v1: first explicit version marker. No schema changes yet.
        if from_version == 0 {
            let next_id: u128 = env
                .storage()
                .persistent()
                .get(&StorageKey::NextAgreementId)
                .unwrap_or(0u128);
            let cap: u128 = if next_id > 10 { 10 } else { next_id };
            let mut i: u128 = 0;
            while i < cap {
                let _maybe: Option<Agreement> =
                    env.storage().persistent().get(&StorageKey::Agreement(i));
                i += 1;
            }

            env.storage()
                .persistent()
                .set(&StorageKey::ContractVersion, &1u32);
            emit_contract_migrated(
                &env,
                ContractMigratedEvent {
                    from_version,
                    to_version: 1,
                },
            );
            return;
        }

        panic!("Unsupported migration version");
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
    /// # Security
    /// - Requires `employer` to authenticate.
    /// - `grace_period_seconds` should be set to a reasonable value to protect employees.
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn create_payroll_agreement(
        env: Env,
        employer: Address,
        token: Address,
        grace_period_seconds: u64,
    ) -> u128 {
        payroll::create_payroll_agreement(&env, employer, token, grace_period_seconds)
    }

    /// Creates multiple payroll agreements in a single transaction.
    ///
    /// # Arguments
    /// * `employer` - Address of the employer creating the agreements
    /// * `items` - Vector of payroll creation parameters
    ///
    /// # Returns
    /// `Ok(BatchPayrollCreateResult)` on success, or `Err(PayrollError)` if inputs invalid
    ///
    /// # Events
    /// Emits `agreement_created_event` per created agreement
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn batch_create_payroll_agreements(
        env: Env,
        employer: Address,
        items: Vec<PayrollCreateParams>,
    ) -> Result<BatchPayrollCreateResult, PayrollError> {
        payroll::batch_create_payroll_agreements(&env, employer, items)
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
    ///
    /// # Access Control
    /// Requires caller authentication
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

    /// Creates multiple escrow agreements in a single transaction.
    ///
    /// # Arguments
    /// * `employer` - Address of the employer
    /// * `items` - Vector of escrow creation parameters
    ///
    /// # Returns
    /// `Ok(BatchEscrowCreateResult)` on success, or `Err(PayrollError)` if inputs invalid
    ///
    /// # Events
    /// Emits `agreement_created_event` and `employee_added_event` per created agreement
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn batch_create_escrow_agreements(
        env: Env,
        employer: Address,
        items: Vec<EscrowCreateParams>,
    ) -> Result<BatchEscrowCreateResult, PayrollError> {
        payroll::batch_create_escrow_agreements(&env, employer, items)
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

    /// Deposits tokens from `from` into the contract to fund a milestone agreement.
    ///
    /// This is the only supported way to bring tokens into scope for a milestone
    /// agreement. The function records an **accounted escrow balance** separate
    /// from the raw on-chain `token.balance()` of the contract address, so that
    /// `approve_milestone` and `claim_milestone` invariant checks cannot be
    /// satisfied by unrelated deposits.
    ///
    /// # Arguments
    /// * `agreement_id` - ID of the milestone agreement to fund.
    /// * `from`         - Address transferring the tokens; must be the stored employer.
    /// * `amount`       - Strictly-positive token amount to deposit.
    ///
    /// # State Transition
    /// No status change. `MilestoneKey::MilestoneEscrowBalance` is incremented by `amount`.
    ///
    /// # Access Control
    /// - `from` must equal the employer stored for `agreement_id`.
    /// - `from.require_auth()` is enforced.
    ///
    /// # Errors
    /// Panics with descriptive messages for: unknown agreement, wrong caller,
    /// non-positive amount, `Cancelled` or `Completed` status, arithmetic overflow.
    pub fn fund_milestone_agreement(env: Env, agreement_id: u128, from: Address, amount: i128) {
        payroll::fund_milestone_agreement(&env, agreement_id, from, amount);
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
    pub fn add_milestone(env: Env, agreement_id: u128, amount: i128) -> Result<(), PayrollError> {
        payroll::add_milestone(env, agreement_id, amount)
    }

    /// Approves a milestone for payment.
    ///
    /// # Invariants
    /// - `escrow balance >= sum of all unclaimed milestone amounts`
    ///
    /// # Arguments
    /// * `agreement_id` - ID of the agreement
    /// * `milestone_id` - ID of the milestone to approve
    ///
    /// # Requirements
    /// - Milestone must exist
    /// - Milestone must not be already approved
    /// - Caller must be the employer
    pub fn approve_milestone(
        env: Env,
        agreement_id: u128,
        milestone_id: u32,
    ) -> Result<(), PayrollError> {
        payroll::approve_milestone(env, agreement_id, milestone_id)
    }

    /// Rejects a milestone, preventing it from being approved or claimed.
    ///
    /// Only the employer may reject a milestone. The milestone cannot be
    /// re-rejected, approved, or claimed after this call. The stored escrow
    /// balance is not adjusted — the employer should fund a replacement
    /// milestone or cancel the agreement to recover unused escrow.
    ///
    /// # Arguments
    /// * `agreement_id` - ID of the milestone agreement.
    /// * `milestone_id` - 1-based ID of the milestone to reject.
    /// * `reason`       - Optional human-readable reason (pass empty string if none).
    ///
    /// # Requirements
    /// - Caller must be the employer.
    /// - Agreement must be in `Created` or `Active` status.
    /// - Milestone must not already be rejected, approved, or claimed.
    pub fn reject_milestone(
        env: Env,
        agreement_id: u128,
        milestone_id: u32,
        reason: soroban_sdk::String,
    ) -> Result<(), PayrollError> {
        payroll::reject_milestone(env, agreement_id, milestone_id, reason)
    }

    /// Claims payment for an approved milestone.
    ///
    /// # Invariants
    /// - `escrow balance >= sum of all unclaimed milestone amounts`
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
    pub fn claim_milestone(
        env: Env,
        agreement_id: u128,
        milestone_id: u32,
    ) -> Result<(), PayrollError> {
        payroll::claim_milestone(env, agreement_id, milestone_id)
    }

    /// Claims payment for multiple approved milestones in a single transaction.
    ///
    /// This is a high-frequency disbursement path. Instruction cost scales
    /// linearly with the number of milestones in `milestone_ids` (one token
    /// transfer and storage write per successful claim). Baselines are tracked
    /// in `benchmarks/stello_pay_contract_gas.json` and enforced in CI via
    /// `tests/gas_benchmarks.rs`.
    ///
    /// # Arguments
    /// * `agreement_id` - ID of the milestone agreement
    /// * `milestone_ids` - 1-based milestone IDs to claim (must be non-empty)
    ///   and no longer than `storage::MAX_BATCH_SIZE`.
    ///
    /// # Returns
    /// `BatchMilestoneResult` with per-milestone success/failure details.
    /// Partial success is supported: invalid or duplicate IDs are recorded as
    /// failures without aborting the batch.
    ///
    /// # Security
    /// - Requires the milestone **contributor** to authenticate (`require_auth`).
    /// - Marks each milestone claimed **before** the token transfer (CEI pattern).
    /// - Rejects claims when the agreement is paused.
    /// - Empty `milestone_ids` panics at the contract boundary.
    /// - Oversized batches fail up front with `PayrollError::BatchTooLarge`.
    ///
    /// # Gas
    /// Benchmarked at N = 1, 5, 20 milestones in `tests/gas_benchmarks.rs`.
    /// N = 20 is the documented ceiling for all batch entrypoints.
    pub fn batch_claim_milestones(
        env: Env,
        agreement_id: u128,
        milestone_ids: Vec<u32>,
    ) -> Result<BatchMilestoneResult, PayrollError> {
        payroll::batch_claim_milestones(&env, agreement_id, milestone_ids)
    }

    /// Gets the total number of milestones for an agreement.
    ///
    /// # Arguments
    /// * `agreement_id` - ID of the agreement
    ///
    /// # Returns
    /// Number of milestones
    ///
    /// # Access Control
    /// Requires caller authentication
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
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_milestone(env: Env, agreement_id: u128, milestone_id: u32) -> Option<Milestone> {
        payroll::get_milestone(env, agreement_id, milestone_id)
    }

    /// Adds an employee to a payroll agreement.
    ///
    /// # Argumentls
    /// * `agreement_id` - ID of the agreement
    /// * `employee` - Address of the employee to add
    /// * `salary_per_period` - Employee's salary per period
    ///
    /// # Requirements
    /// - Agreement must be in Created status
    /// - Agreement must be Payroll mode
    /// - Caller must be the employer
    /// - The employee address must not already be present in the agreement;
    ///   a duplicate add panics with `PayrollError::EmployeeAlreadyExists` to
    ///   preserve the 1:1 employee-to-salary mapping. A previously removed
    ///   employee may be re-added.
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
    ///
    /// # Arguments
    /// * `agreement_id` - agreement_id parameter
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_agreement(env: Env, agreement_id: u128) -> Option<Agreement> {
        payroll::get_agreement(&env, agreement_id)
    }

    /// Retrieves all employee addresses for an agreement.
    ///
    /// # Returns
    /// Vector of employee addresses
    ///
    /// # Arguments
    /// * `agreement_id` - agreement_id parameter
    ///
    /// # Access Control
    /// Requires caller authentication
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
    ///
    /// # Returns
    /// bool
    pub fn set_arbiter(env: Env, caller: Address, arbiter: Address) -> bool {
        payroll::set_arbiter(&env, caller, arbiter)
    }

    /// Gets the current arbiter address
    ///
    /// # Returns
    /// Arbiter address if set, None otherwise
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_arbiter(env: Env) -> Option<Address> {
        payroll::get_arbiter(&env)
    }

    /// @notice Configures the shared audit logger used for lifecycle audit entries.
    /// @dev Only the initialized contract owner can set this address. Once configured,
    /// successful lifecycle mutations append to the local audit stream and call the
    /// external audit logger's append-only entrypoint.
    pub fn set_audit_logger(env: Env, owner: Address, audit_logger: Address) {
        audit::set_audit_logger(&env, owner, audit_logger);
    }

    /// @notice Returns the configured shared audit logger address, if one is set.
    pub fn get_audit_logger(env: Env) -> Option<Address> {
        audit::get_audit_logger(&env)
    }

    /// @notice Returns the number of lifecycle audit entries appended locally.
    pub fn get_audit_entry_count(env: Env) -> u64 {
        audit::get_audit_entry_count(&env)
    }

    /// @notice Returns a lifecycle audit entry by append-only id.
    pub fn get_audit_entry(env: Env, audit_id: u64) -> Option<LifecycleAuditEntry> {
        audit::get_audit_entry(&env, audit_id)
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
    ///
    /// # Returns
    /// Result<(), PayrollError>
    ///
    /// # Errors
    /// Returns an error if validation fails
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
    ///
    /// # Returns
    /// Result<(), PayrollError>
    ///
    /// # Errors
    /// Returns an error if validation fails
    pub fn resolve_dispute(
        env: Env,
        caller: Address,
        agreement_id: u128,
        pay_employee: i128,
        refund_employer: i128,
    ) -> Result<(), PayrollError> {
        payroll::resolve_dispute(env, caller, agreement_id, pay_employee, refund_employer)
    }

    /// Resolves a dispute that has been pre-approved by the multisig contract.
    ///
    /// # Arguments
    /// * `caller` - Arbiter address (must authenticate)
    /// * `agreement_id` - Agreement under dispute
    /// * `pay_employee` - Amount to distribute to employees
    /// * `refund_employer` - Amount to refund the employer
    /// * `multisig_operation_id` - ID of the Executed DisputeResolution operation in the multisig
    pub fn resolve_dispute_multisig(
        env: Env,
        caller: Address,
        agreement_id: u128,
        pay_employee: i128,
        refund_employer: i128,
        multisig_operation_id: u128,
    ) -> Result<(), PayrollError> {
        payroll::resolve_dispute_multisig(
            env,
            caller,
            agreement_id,
            pay_employee,
            refund_employer,
            multisig_operation_id,
        )
    }

    /// Configures the multisig integration thresholds.
    ///
    /// # Arguments
    /// * `owner` - Contract owner (must authenticate)
    /// * `multisig_contract` - Address of the deployed multisig contract
    /// * `large_payment_threshold` - Min amount requiring multisig for LargePayment (0 = disabled)
    /// * `dispute_resolution_threshold` - Min total payout requiring multisig for DisputeResolution (0 = disabled)
    pub fn set_multisig_config(
        env: Env,
        owner: Address,
        multisig_contract: Address,
        large_payment_threshold: i128,
        dispute_resolution_threshold: i128,
    ) -> Result<(), PayrollError> {
        payroll::set_multisig_config(
            &env,
            owner,
            multisig_contract,
            large_payment_threshold,
            dispute_resolution_threshold,
        )
    }

    /// Returns the configured multisig contract address, if any.
    pub fn get_multisig_contract(env: Env) -> Option<Address> {
        payroll::get_multisig_contract(&env)
    }

    /// Retrieves current dispute status for an agreement by ID
    ///
    /// # Returns
    /// DisputeStatus enum
    ///
    /// # Arguments
    /// * `agreement_id` - agreement_id parameter
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_dispute_status(env: Env, agreement_id: u128) -> DisputeStatus {
        payroll::get_dispute_status(env, agreement_id)
    }

    /// Sets the global FX rate admin address that is allowed to update
    /// exchange rates in addition to the contract owner (e.g. an oracle
    /// contract responsible for pushing prices on-chain).
    ///
    /// # Arguments
    /// * `caller` - caller parameter
    /// * `admin` - admin parameter
    ///
    /// # Returns
    /// Result<(), PayrollError>
    ///
    /// # Errors
    /// Returns an error if validation fails
    ///
    /// # Access Control
    /// Requires caller authentication
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
    ///
    /// # Arguments
    /// * `caller` - caller parameter
    /// * `base` - base parameter
    /// * `quote` - quote parameter
    /// * `rate` - rate parameter
    ///
    /// # Returns
    /// Result<(), PayrollError>
    ///
    /// # Errors
    /// Returns an error if validation fails
    pub fn set_exchange_rate(
        env: Env,
        caller: Address,
        base: Address,
        quote: Address,
        rate: i128,
    ) -> Result<(), PayrollError> {
        payroll::set_exchange_rate(&env, caller, base, quote, rate)
    }

    /// Sets an absolute upper-bound sanity limit for exchange rates.
    /// Any `set_exchange_rate` call with a rate above this value will be rejected.
    /// Caller must be the contract owner.
    pub fn set_fx_rate_sanity_bound(
        env: Env,
        caller: Address,
        max_rate: i128,
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
        if max_rate <= 0 {
            return Err(PayrollError::ExchangeRateInvalid);
        }
        storage::DataKey::set_exchange_rate_max_rate_sanity_bound(&env, max_rate);
        Ok(())
    }

    /// Converts an `amount` from one token into another using the configured
    /// FX rate, without performing any on-chain transfer. This is useful for
    /// off-chain estimation and validation of multi-currency payouts.
    ///
    /// # Arguments
    /// * `from_token` - from_token parameter
    /// * `to_token` - to_token parameter
    /// * `amount` - amount parameter
    ///
    /// # Returns
    /// Result<i128, PayrollError>
    ///
    /// # Errors
    /// Returns an error if validation fails
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn convert_currency(
        env: Env,
        from_token: Address,
        to_token: Address,
        amount: i128,
    ) -> Result<i128, PayrollError> {
        payroll::convert_currency(&env, from_token, to_token, amount)
    }

    /// Claims accrued payroll for a single employee in a payroll agreement.
    ///
    /// This is the highest-frequency on-chain operation. The employee receives
    /// salary for all unclaimed elapsed periods in one transfer. Instruction
    /// cost is **O(1)** in the number of backlog periods because period
    /// arithmetic is constant-time and only one token transfer is executed.
    /// Baselines are tracked in `benchmarks/stello_pay_contract_gas.json`
    /// and enforced in CI via `tests/gas_benchmarks.rs`.
    ///
    /// # Invariants
    /// - `claimed_periods <= num_periods` when `num_periods` is set on the agreement.
    /// - Escrow balance must cover `salary_per_period * periods_to_pay`.
    ///
    /// # Arguments
    /// * `caller` - Employee address (must match `employee_index`)
    /// * `agreement_id` - Payroll agreement ID
    /// * `employee_index` - 0-based index of the employee within the agreement
    ///
    /// # Security
    /// - Requires `caller` to be the employee at `employee_index` (`Unauthorized` otherwise).
    /// - Rejects claims when the contract is emergency-paused or the agreement is paused.
    /// - Large payments above `LargePaymentThreshold` require `claim_payroll_multisig`.
    /// - Enforces checks-effects-interactions: escrow balance, `claimed_periods`,
    ///   and `paid_amount` are persisted BEFORE the external token transfer.
    /// - Protected by a transient reentrancy guard (temporary storage, cleared per
    ///   transaction). A reentrant call during transfer fails with
    ///   `PayrollError::ReentrancyDetected`, preventing double-payment of a period
    ///   via a hostile or hook-enabled token.
    ///
    /// # Gas
    /// Benchmarked at 1, 10, and 50 elapsed payroll periods. See `docs/gas-benchmarks.md`.
    ///
    /// # Returns
    /// `Ok(())` on success, or `PayrollError` on validation or transfer failure.
    pub fn claim_payroll(
        env: Env,
        caller: Address,
        agreement_id: u128,
        employee_index: u32,
    ) -> Result<(), PayrollError> {
        payroll::claim_payroll(&env, &caller, agreement_id, employee_index)
    }

    /// Claims payroll for a large payment pre-approved by the multisig contract.
    ///
    /// # Arguments
    /// * `caller` - Employee address (must authenticate)
    /// * `agreement_id` - Payroll agreement ID
    /// * `employee_index` - Employee index within the agreement
    /// * `multisig_operation_id` - ID of the Executed LargePayment operation in the multisig
    pub fn claim_payroll_multisig(
        env: Env,
        caller: Address,
        agreement_id: u128,
        employee_index: u32,
        multisig_operation_id: u128,
    ) -> Result<(), PayrollError> {
        payroll::claim_payroll_multisig(
            &env,
            &caller,
            agreement_id,
            employee_index,
            multisig_operation_id,
        )
    }

    /// Claims payroll for an employee, but settles the transfer in a
    /// caller-specified payout token. The agreement continues to track its
    /// accounting in the base token while the actual transfer is executed
    /// in the requested payout currency using the configured FX rate.
    ///
    /// # Arguments
    /// * `caller` - caller parameter
    /// * `agreement_id` - agreement_id parameter
    /// * `employee_index` - employee_index parameter
    /// * `payout_token` - payout_token parameter
    ///
    /// # Returns
    /// Result<(), PayrollError>
    ///
    /// # Errors
    /// Returns an error if validation fails
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn claim_payroll_in_token(
        env: Env,
        caller: Address,
        agreement_id: u128,
        employee_index: u32,
        payout_token: Address,
    ) -> Result<(), PayrollError> {
        payroll::claim_payroll_in_token(&env, &caller, agreement_id, employee_index, payout_token)
    }

    /// Batch Claim Payroll
    ///
    /// # Arguments
    /// * `caller` - caller parameter
    /// * `agreement_id` - agreement_id parameter
    /// * `employee_indices` - employee_indices parameter
    ///
    /// # Returns
    /// Result<BatchPayrollResult, PayrollError>
    ///
    /// # Errors
    /// Returns an error if validation fails
    ///
    /// # Access Control
    /// Requires caller authentication
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
    ///
    /// # Access Control
    /// Requires caller authentication
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
    pub fn pause_agreement(env: Env, agreement_id: u128) -> Result<(), PayrollError> {
        // Try new-style agreement first (payroll/escrow)
        if payroll::get_agreement(&env, agreement_id).is_some() {
            payroll::pause_agreement(&env, agreement_id);
            return Ok(());
        }

        // Fall back to milestone-based agreement
        payroll::pause_milestone_agreement(env, agreement_id)
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
    pub fn resume_agreement(env: Env, agreement_id: u128) -> Result<(), PayrollError> {
        // Try new-style agreement first (payroll/escrow)
        if payroll::get_agreement(&env, agreement_id).is_some() {
            payroll::resume_agreement(&env, agreement_id);
            return Ok(());
        }

        // Fall back to milestone-based agreement
        payroll::resume_milestone_agreement(env, agreement_id)
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
    ///
    /// # Access Control
    /// Requires caller authentication
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
    ///
    /// # Access Control
    /// Requires caller authentication
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
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_grace_period_end(env: Env, agreement_id: u128) -> Option<u64> {
        payroll::get_grace_period_end(&env, agreement_id)
    }

    /// Extends the effective grace / dispute window for a **cancelled** agreement.
    ///
    /// # Authorization
    /// Contract owner or the agreement employer (both must pass `require_auth`).
    ///
    /// # Limits
    /// Subject to `GracePeriodExtensionPolicy` (owner-configurable within hard sanity bounds).
    ///
    /// # Events
    /// Emits `grace_period_extended_event` for auditing.
    pub fn extend_grace_period(
        env: Env,
        caller: Address,
        agreement_id: u128,
        additional_seconds: u64,
    ) -> Result<(), PayrollError> {
        payroll::extend_grace_period(&env, caller, agreement_id, additional_seconds)
    }

    /// Owner-only: updates caps for grace extensions (basis points of base grace, per-call max).
    pub fn set_grace_extension_policy(
        env: Env,
        caller: Address,
        policy: GracePeriodExtensionPolicy,
    ) -> Result<(), PayrollError> {
        payroll::set_grace_extension_policy(&env, caller, policy)
    }

    /// Current grace extension policy (defaults until explicitly set).
    pub fn get_grace_extension_policy(env: Env) -> GracePeriodExtensionPolicy {
        payroll::get_grace_extension_policy(&env)
    }

    /// Cumulative extra seconds applied on top of `Agreement.grace_period_seconds`.
    pub fn get_grace_extension_seconds(env: Env, agreement_id: u128) -> u64 {
        payroll::get_grace_extension_seconds(&env, agreement_id)
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
    ///
    /// # Access Control
    /// Requires caller authentication
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
    ///
    /// # Returns
    /// Result<(), storage::PayrollError>
    ///
    /// # Errors
    /// Returns an error if validation fails
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
    ///
    /// # Returns
    /// Result<(), storage::PayrollError>
    ///
    /// # Errors
    /// Returns an error if validation fails
    pub fn approve_emergency_pause(env: Env, caller: Address) -> Result<(), storage::PayrollError> {
        payroll::approve_emergency_pause(&env, caller)
    }

    /// Immediately activates emergency pause (owner only)
    ///
    /// # Security
    /// - Requires contract owner authentication.
    /// - Provides an immediate "kill switch" to stop all claims in case of a discovered vulnerability.
    /// - Should be used with caution as it stops all legitimate operations.
    ///
    /// # Access Control
    /// Requires owner authentication
    ///
    /// # Returns
    /// Result<(), storage::PayrollError>
    pub fn emergency_pause(env: Env) -> Result<(), storage::PayrollError> {
        payroll::emergency_pause(&env)
    }

    /// Unpauses contract after emergency resolved
    ///
    /// # Access Control
    /// Requires owner authentication
    ///
    /// # Returns
    /// Result<(), storage::PayrollError>
    ///
    /// # Errors
    /// Returns an error if validation fails
    pub fn emergency_unpause(env: Env) -> Result<(), storage::PayrollError> {
        payroll::emergency_unpause(&env)
    }

    /// Checks if contract is in emergency pause state
    ///
    /// # Returns
    /// true if paused, false otherwise
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn is_emergency_paused(env: Env) -> bool {
        payroll::is_emergency_paused(&env)
    }

    /// Gets emergency pause state details
    ///
    /// # Returns
    /// EmergencyPause state if set
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_emergency_pause_state(env: Env) -> Option<storage::EmergencyPause> {
        payroll::get_emergency_pause_state(&env)
    }

    // ============================================================================
    // Encrypted Backup & Recovery
    // ============================================================================

    /// Admin-only: restore an `Agreement` from a pre-decrypted struct.
    ///
    /// Use this when the operator has already decrypted and verified the backup
    /// off-chain and simply needs to re-write the state into persistent storage.
    ///
    /// # Arguments
    /// * `caller`    – must be the contract owner.
    /// * `agreement` – the `Agreement` to write back.
    ///
    /// # Access Control
    /// Requires owner authentication.
    pub fn admin_restore_agreement(
        env: Env,
        caller: Address,
        agreement: storage::Agreement,
    ) -> Result<(), storage::PayrollError> {
        caller.require_auth();
        let owner: Address = env
            .storage()
            .persistent()
            .get(&storage::StorageKey::Owner)
            .ok_or(storage::PayrollError::Unauthorized)?;
        if caller != owner {
            return Err(storage::PayrollError::Unauthorized);
        }
        backup::admin_restore_agreement(&env, agreement);
        Ok(())
    }

    /// Admin-only: decrypt an encrypted backup envelope and restore the
    /// contained `Agreement` into persistent storage in a single call.
    ///
    /// # Arguments
    /// * `caller`     – must be the contract owner.
    /// * `envelope`   – encrypted backup bytes (version | salt | nonce | ciphertext).
    /// * `passphrase` – decryption passphrase; never stored on-chain.
    ///
    /// # Returns
    /// The restored `agreement_id` on success.
    ///
    /// # Errors
    /// Returns `PayrollError::InvalidData` if decryption or deserialisation fails.
    /// Returns `PayrollError::Unauthorized` if caller is not the owner.
    ///
    /// # Access Control
    /// Requires owner authentication.
    pub fn admin_restore_from_encrypted(
        env: Env,
        caller: Address,
        envelope: soroban_sdk::Bytes,
        passphrase: soroban_sdk::Bytes,
    ) -> Result<u128, storage::PayrollError> {
        caller.require_auth();
        let owner: Address = env
            .storage()
            .persistent()
            .get(&storage::StorageKey::Owner)
            .ok_or(storage::PayrollError::Unauthorized)?;
        if caller != owner {
            return Err(storage::PayrollError::Unauthorized);
        }
        backup::admin_restore_from_encrypted(&env, envelope, passphrase)
    }
}
