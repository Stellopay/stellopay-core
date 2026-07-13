use soroban_sdk::token::TokenClient;
use soroban_sdk::{Address, Env, Vec};

use crate::audit::{record_entry, AuditEvent};
use crate::events::{
    emit_agreement_activated, emit_agreement_cancelled, emit_agreement_created,
    emit_agreement_paused, emit_agreement_resumed, emit_dsipute_raised, emit_dsipute_resolved,
    emit_employee_added, emit_exchange_rate_changed, emit_grace_period_extended,
    emit_grace_period_finalized, emit_milestone_funded, emit_multisig_config_changed,
    emit_payment_received, emit_payment_sent, emit_payroll_claimed, emit_set_arbiter,
    AgreementActivatedEvent, AgreementCancelledEvent, AgreementCreatedEvent, AgreementPausedEvent,
    AgreementResumedEvent, ArbiterSetEvent, BatchMilestoneClaimedEvent, BatchPayrollClaimedEvent,
    DisputeRaisedEvent, DisputeResolvedEvent, EmployeeAddedEvent, ExchangeRateChangedEvent,
    GracePeriodExtendedEvent, GracePeriodFinalizedEvent, MilestoneAdded, MilestoneApproved,
    MilestoneClaimed, MilestoneFundedEvent, MultisigConfigChangedEvent, PaymentReceivedEvent,
    PaymentSentEvent, PayrollClaimedEvent,
};
use crate::storage::{
    Agreement, AgreementMode, AgreementStatus, BatchEscrowCreateResult, BatchMilestoneResult,
    BatchPayrollCreateResult, BatchPayrollResult, DataKey, DisputeStatus, EmployeeInfo,
    EscrowCreateParams, EscrowCreateResult, GracePeriodExtensionPolicy, Milestone,
    MilestoneClaimResult, MilestoneKey, PaymentType, PayrollClaimResult, PayrollCreateParams,
    PayrollCreateResult, PayrollError, StorageKey, MAX_BATCH_SIZE,
};
use soroban_sdk::{
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    contractclient, contracttype, panic_with_error, token, IntoVal, Symbol, Val,
};

/// Minimal interface for cross-contract calls into the deployed multisig contract.
#[contractclient(name = "MultisigClient")]
trait MultisigInterface {
    fn get_operation(env: Env, operation_id: u128) -> Option<Operation>;
}

#[contractclient(name = "RateLimiterClient")]
trait RateLimiterInterface {
    fn check_and_consume(env: Env, subject: Address) -> u32;
}

#[contractclient(name = "SalaryAdjustmentClient")]
trait SalaryAdjustmentInterface {
    fn get_employee_salary(env: Env, employee: Address) -> Option<i128>;
}

/// Mirror of multisig::OperationStatus — names must match for XDR decoding.
#[contracttype]
#[derive(Clone, PartialEq)]
enum OperationStatus {
    Pending,
    Executed,
    Cancelled,
}

/// Minimal mirror of multisig::Operation for cross-contract reads.
#[contracttype]
#[derive(Clone)]
struct Operation {
    pub id: u128,
    pub kind: OperationKind,
    pub creator: Address,
    pub status: OperationStatus,
    pub created_at: u64,
    pub executed_at: Option<u64>,
}

/// Mirror of multisig::OperationKind — names must match for XDR decoding.
#[contracttype]
#[derive(Clone)]
enum OperationKind {
    ContractUpgrade(Address, soroban_sdk::BytesN<32>),
    LargePayment(Address, Address, i128),
    DisputeResolution(Address, u128, i128, i128),
}

/// Configures the multisig integration for this payroll contract.
///
/// # Arguments
/// * `owner` - Contract owner (must authenticate)
/// * `multisig_contract` - Address of the deployed multisig contract
/// * `large_payment_threshold` - Minimum amount requiring multisig for LargePayment (0 = disabled)
/// * `dispute_resolution_threshold` - Minimum total payout requiring multisig for DisputeResolution (0 = disabled)
///
/// # Access Control
/// Only the contract owner can call this.
pub fn set_multisig_config(
    env: &Env,
    owner: Address,
    multisig_contract: Address,
    large_payment_threshold: i128,
    dispute_resolution_threshold: i128,
) -> Result<(), PayrollError> {
    let stored_owner: Address = env
        .storage()
        .persistent()
        .get(&StorageKey::Owner)
        .ok_or(PayrollError::Unauthorized)?;
    owner.require_auth();
    if owner != stored_owner {
        return Err(PayrollError::Unauthorized);
    }

    // Capture the previous thresholds before overwriting so the emitted event
    // and audit entry can report old-vs-new values (0 = previously unset).
    let old_large_payment_threshold: i128 = env
        .storage()
        .persistent()
        .get(&StorageKey::LargePaymentThreshold)
        .unwrap_or(0);
    let old_dispute_resolution_threshold: i128 = env
        .storage()
        .persistent()
        .get(&StorageKey::DisputeResolutionThreshold)
        .unwrap_or(0);

    env.storage()
        .persistent()
        .set(&StorageKey::MultisigContract, &multisig_contract);
    env.storage()
        .persistent()
        .set(&StorageKey::LargePaymentThreshold, &large_payment_threshold);
    env.storage().persistent().set(
        &StorageKey::DisputeResolutionThreshold,
        &dispute_resolution_threshold,
    );

    // Emit a structured event so off-chain monitors observe approval-requirement
    // changes mid-lifecycle. Only public configuration is exposed.
    emit_multisig_config_changed(
        env,
        MultisigConfigChangedEvent {
            caller: owner.clone(),
            multisig_contract: multisig_contract.clone(),
            old_large_threshold: old_large_payment_threshold,
            new_large_threshold: large_payment_threshold,
            old_dispute_threshold: old_dispute_resolution_threshold,
            new_dispute_threshold: dispute_resolution_threshold,
        },
    );

    // Record a tamper-evident audit entry via the existing audit path. This is a
    // contract-level change, so it uses the sentinel `agreement_id = 0` and
    // reports the new large-payment threshold as the entry's `amount`.
    record_entry(
        env,
        owner,
        AuditEvent::MultisigConfigChanged,
        0,
        None,
        Some(large_payment_threshold),
    );

    Ok(())
}

/// Returns the configured multisig contract address, if any.
pub fn get_multisig_contract(env: &Env) -> Option<Address> {
    env.storage()
        .persistent()
        .get(&StorageKey::MultisigContract)
}

/// Checks that a multisig operation with the given id exists, is Executed,
/// and matches the expected kind discriminant. Returns `MultisigApprovalRequired`
/// if the check fails.
fn require_multisig_executed(
    env: &Env,
    multisig_addr: &Address,
    operation_id: u128,
    check: impl Fn(&OperationKind) -> bool,
) -> Result<(), PayrollError> {
    let client = MultisigClient::new(env, multisig_addr);
    let op = client
        .get_operation(&operation_id)
        .ok_or(PayrollError::MultisigApprovalRequired)?;
    if op.status != OperationStatus::Executed {
        return Err(PayrollError::MultisigApprovalRequired);
    }
    if !check(&op.kind) {
        return Err(PayrollError::MultisigApprovalRequired);
    }
    Ok(())
}

fn enforce_rate_limit(env: &Env, caller: &Address) -> Result<(), PayrollError> {
    if let Some(rate_limiter_addr) = env
        .storage()
        .persistent()
        .get::<_, Address>(&StorageKey::RateLimiterContract)
    {
        let client = RateLimiterClient::new(env, &rate_limiter_addr);
        if client.try_check_and_consume(caller).is_err() {
            return Err(PayrollError::RateLimited);
        }
    }
    Ok(())
}

/// Acquire the transient reentrancy guard for a claim path.
///
/// Returns [`PayrollError::ReentrancyDetected`] if the guard is already held,
/// i.e. this call path was re-entered (e.g. via a hostile or hook-enabled
/// token during `transfer`). The guard is kept in *temporary* storage (see
/// [`StorageKey::ReentrancyGuard`]) so it is automatically cleared at the end
/// of the transaction even if a panic strands it mid-call.
fn acquire_reentrancy_guard(env: &Env) -> Result<(), PayrollError> {
    if env.storage().temporary().has(&StorageKey::ReentrancyGuard) {
        return Err(PayrollError::ReentrancyDetected);
    }
    env.storage()
        .temporary()
        .set(&StorageKey::ReentrancyGuard, &true);
    Ok(())
}

/// Release the transient reentrancy guard acquired by [`acquire_reentrancy_guard`].
///
/// Must be called on every return path of the guarded function so the guard
/// never outlives a single top-level call.
fn release_reentrancy_guard(env: &Env) {
    env.storage()
        .temporary()
        .remove(&StorageKey::ReentrancyGuard);
}

/// Fixed-point scaling factor for FX rates: 1e6 precision.
const FX_SCALE: i128 = 1_000_000;

/// Minimum converted amount (in quote-token base units) below which the
/// conversion is treated as pure dust and rejected.
///
/// Rounding policy: `convert_amount` uses **floor division** (truncation toward
/// zero). Any remainder is discarded. If truncation reduces the converted amount
/// to zero the call returns `ExchangeRateInvalid` so callers are not silently
/// credited nothing. Callers that need to claim very small amounts should
/// accumulate multiple periods before claiming.
const DUST_THRESHOLD: i128 = 1;

pub fn create_milestone_agreement(
    env: Env,
    employer: Address,
    contributor: Address,
    token: Address,
) -> u128 {
    employer.require_auth();

    let mut counter: u128 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::AgreementCounter)
        .unwrap_or(0);
    counter += 1;

    let agreement_id = counter;

    env.storage()
        .persistent()
        .set(&MilestoneKey::AgreementCounter, &counter);
    env.storage()
        .persistent()
        .set(&MilestoneKey::Employer(agreement_id), &employer);
    env.storage()
        .persistent()
        .set(&MilestoneKey::Contributor(agreement_id), &contributor);
    env.storage()
        .persistent()
        .set(&MilestoneKey::Token(agreement_id), &token);
    env.storage().persistent().set(
        &MilestoneKey::PaymentType(agreement_id),
        &PaymentType::MilestoneBased,
    );
    env.storage().persistent().set(
        &MilestoneKey::Status(agreement_id),
        &AgreementStatus::Created,
    );
    env.storage()
        .persistent()
        .set(&MilestoneKey::TotalAmount(agreement_id), &0i128);
    env.storage()
        .persistent()
        .set(&MilestoneKey::MilestoneCount(agreement_id), &0u32);

    agreement_id
}

/// Deposits tokens from `from` into the contract for the specified milestone
/// agreement, crediting the accounted escrow balance.
///
/// # Why this exists
/// `approve_milestone` and `claim_milestone` assert that the contract holds
/// enough tokens to cover unclaimed milestones. Without a dedicated funding
/// entrypoint the only way to satisfy that invariant is to send tokens
/// out-of-band, which is undiscoverable and unauditable. This function
/// provides a first-class, authenticated, on-chain funding path.
///
/// # Arguments
/// * `env`          - Contract environment.
/// * `agreement_id` - ID of the milestone agreement to fund.
/// * `from`         - Address to pull tokens from; must be the agreement's
///                    employer and must pass `require_auth`.
/// * `amount`       - Number of tokens to deposit; must be strictly positive.
///
/// # Access Control
/// `from` must equal the employer stored for the agreement, and
/// `from.require_auth()` is called before any state mutation or transfer.
///
/// # State changes — O(1)
/// - Reads + writes `MilestoneKey::MilestoneEscrowBalance(agreement_id)` once.
/// - Executes exactly one `token.transfer(from, contract_address, amount)`.
///
/// # Errors / panics
/// - "Agreement not found" — `agreement_id` does not correspond to a known milestone agreement.
/// - "Unauthorized: only the employer can fund a milestone agreement" — `from` ≠ stored employer.
/// - "Amount must be positive" — `amount` is zero or negative.
/// - "Cannot fund a Cancelled agreement" — agreement status is `Cancelled`.
/// - "Cannot fund a Completed agreement" — agreement status is `Completed`.
/// - "Escrow balance overflow" — cumulative funded amount would overflow `i128`.
/// - Token-transfer panics propagated from the Soroban token host.
///
/// # Security
/// The accounted balance (`MilestoneEscrowBalance`) is the sole source of
/// truth used by `approve_milestone` and `claim_milestone` invariant checks.
/// Raw `token.balance()` of the contract is intentionally **not** consulted
/// so that third-party deposits cannot inflate claimable funds.
pub fn fund_milestone_agreement(env: &Env, agreement_id: u128, from: Address, amount: i128) {
    let employer: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Employer(agreement_id))
        .expect("Agreement not found");

    // Only the agreement's employer may fund it.
    assert!(
        from == employer,
        "Unauthorized: only the employer can fund a milestone agreement"
    );
    from.require_auth();

    assert!(amount > 0, "Amount must be positive");

    let status: AgreementStatus = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Status(agreement_id))
        .expect("Agreement not found");
    assert!(
        status != AgreementStatus::Cancelled,
        "Cannot fund a Cancelled agreement"
    );
    assert!(
        status != AgreementStatus::Completed,
        "Cannot fund a Completed agreement"
    );

    let current_balance: i128 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneEscrowBalance(agreement_id))
        .unwrap_or(0i128);
    let new_balance = current_balance
        .checked_add(amount)
        .expect("Escrow balance overflow");
    env.storage().persistent().set(
        &MilestoneKey::MilestoneEscrowBalance(agreement_id),
        &new_balance,
    );

    let token_address: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Token(agreement_id))
        .expect("Token not found");
    TokenClient::new(env, &token_address).transfer(&from, &env.current_contract_address(), &amount);

    emit_milestone_funded(
        env,
        MilestoneFundedEvent {
            agreement_id,
            from,
            amount,
            total_escrow_balance: new_balance,
        },
    );
}

/// Adds a milestone to an agreement
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the agreement
/// * `amount` - Payment amount for this milestone
///
/// # Errors
/// * `PayrollError::AgreementNotFound` — the milestone agreement does not exist.
/// * `PayrollError::MilestoneAgreementInvalidStatus` — the agreement is not in `Created` status.
/// * `PayrollError::MilestoneAmountInvalid` — `amount` is not strictly positive.
pub fn add_milestone(env: Env, agreement_id: u128, amount: i128) -> Result<(), PayrollError> {
    let status: AgreementStatus = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Status(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;

    if status != AgreementStatus::Created {
        return Err(PayrollError::MilestoneAgreementInvalidStatus);
    }
    if amount <= 0 {
        return Err(PayrollError::MilestoneAmountInvalid);
    }

    let employer: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Employer(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    employer.require_auth();

    let count: u32 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .unwrap_or(0);

    let milestone_id = count + 1;

    env.storage().persistent().set(
        &MilestoneKey::MilestoneAmount(agreement_id, milestone_id),
        &amount,
    );
    env.storage().persistent().set(
        &MilestoneKey::MilestoneApproved(agreement_id, milestone_id),
        &false,
    );
    env.storage().persistent().set(
        &MilestoneKey::MilestoneClaimed(agreement_id, milestone_id),
        &false,
    );
    env.storage()
        .persistent()
        .set(&MilestoneKey::MilestoneCount(agreement_id), &milestone_id);

    let total: i128 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::TotalAmount(agreement_id))
        .unwrap_or(0);
    env.storage()
        .persistent()
        .set(&MilestoneKey::TotalAmount(agreement_id), &(total + amount));

    // Post-invariant: total amount should equal sum of milestones
    #[cfg(debug_assertions)]
    {
        let total_sum = sum_all_milestones(&env, agreement_id);
        assert!(
            total_sum == total + amount,
            "Total amount mismatch after adding milestone"
        );
    }

    MilestoneAdded {
        agreement_id,
        milestone_id,
        amount,
    }
    .publish(&env);

    Ok(())
}

/// Returns the total configured amount across all milestones for an agreement.
///
/// # Arguments
/// * `env` - Contract environment used to read milestone count and amounts from instance storage.
/// * `agreement_id` - Milestone agreement identifier whose milestone amounts should be summed.
///
/// # Returns
/// Sum of every stored milestone amount for the agreement, treating missing amount entries as zero.
///
/// # Cost
/// O(n) in the stored milestone count for `agreement_id`, where `n` is bounded by the
/// milestones created for that agreement.
fn sum_all_milestones(env: &Env, agreement_id: u128) -> i128 {
    let count: u32 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .unwrap_or(0);
    let mut sum = 0i128;
    for i in 1..=count {
        sum += env
            .storage()
            .persistent()
            .get::<_, i128>(&MilestoneKey::MilestoneAmount(agreement_id, i))
            .unwrap_or(0);
    }
    sum
}

/// Returns the total amount still locked for unclaimed milestones.
///
/// # Arguments
/// * `env` - Contract environment used to read approval, claim, count, and amount entries.
/// * `agreement_id` - Milestone agreement identifier whose unclaimed milestones are inspected.
///
/// # Returns
/// Sum of milestone amounts that have not been claimed, treating missing boolean or amount entries as false/zero.
///
/// # Cost
/// O(n) in the stored milestone count for `agreement_id`, with one approval lookup,
/// one claimed lookup, and at most one amount lookup per milestone.
fn sum_unclaimed_milestones(env: &Env, agreement_id: u128) -> i128 {
    let count: u32 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .unwrap_or(0);
    let mut sum = 0i128;
    for i in 1..=count {
        let approved: bool = env
            .storage()
            .persistent()
            .get(&MilestoneKey::MilestoneApproved(agreement_id, i))
            .unwrap_or(false);
        let claimed: bool = env
            .storage()
            .persistent()
            .get(&MilestoneKey::MilestoneClaimed(agreement_id, i))
            .unwrap_or(false);
        if approved && !claimed {
            sum += env
                .storage()
                .persistent()
                .get::<_, i128>(&MilestoneKey::MilestoneAmount(agreement_id, i))
                .unwrap_or(0);
        }
    }
    sum
}

/// Approves a milestone for payment
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the agreement
/// * `milestone_id` - ID of the milestone to approve
///
/// # Errors
/// * `PayrollError::AgreementNotFound` — the milestone agreement does not exist.
/// * `PayrollError::MilestoneAgreementInvalidStatus` — the agreement is not in `Created` or `Active` status.
/// * `PayrollError::MilestoneNotFound` — `milestone_id` is out of range for the agreement.
/// * `PayrollError::MilestoneAlreadyApproved` — the milestone was already approved.
/// * `PayrollError::InsufficientEscrowBalance` — funded escrow cannot cover all unclaimed milestones.
pub fn approve_milestone(
    env: Env,
    agreement_id: u128,
    milestone_id: u32,
) -> Result<(), PayrollError> {
    let employer: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Employer(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    employer.require_auth();

    let status: AgreementStatus = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Status(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    if status != AgreementStatus::Created && status != AgreementStatus::Active {
        return Err(PayrollError::MilestoneAgreementInvalidStatus);
    }

    let count: u32 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .ok_or(PayrollError::MilestoneNotFound)?;
    if milestone_id == 0 || milestone_id > count {
        return Err(PayrollError::MilestoneNotFound);
    }

    let already_approved: bool = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneApproved(agreement_id, milestone_id))
        .unwrap_or(false);
    if already_approved {
        return Err(PayrollError::MilestoneAlreadyApproved);
    }

    env.storage().persistent().set(
        &MilestoneKey::MilestoneApproved(agreement_id, milestone_id),
        &true,
    );

    // Invariant: accounted escrow balance must cover all unclaimed milestones
    // (including the one just approved). Uses the accounted balance rather than
    // raw token.balance() so that unrelated deposits cannot satisfy this check.
    let unclaimed_sum = sum_unclaimed_milestones(&env, agreement_id);
    let escrow_balance: i128 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneEscrowBalance(agreement_id))
        .unwrap_or(0i128);
    if escrow_balance < unclaimed_sum {
        return Err(PayrollError::InsufficientEscrowBalance);
    }

    MilestoneApproved {
        agreement_id,
        milestone_id,
    }
    .publish(&env);

    Ok(())
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
///
/// # Errors
/// * `PayrollError::EmergencyPaused` — the contract is under an emergency pause.
/// * `PayrollError::AgreementNotFound` — the milestone agreement (or its token) does not exist.
/// * `PayrollError::AgreementPaused` — the agreement is currently paused.
/// * `PayrollError::MilestoneNotFound` — `milestone_id` is out of range or its amount is missing.
/// * `PayrollError::MilestoneNotApproved` — the milestone has not been approved.
/// * `PayrollError::MilestoneAlreadyClaimed` — the milestone was already claimed.
/// * `PayrollError::InsufficientEscrowBalance` — funded escrow cannot cover all unclaimed milestones.
pub fn claim_milestone(
    env: Env,
    agreement_id: u128,
    milestone_id: u32,
) -> Result<(), PayrollError> {
    // Check emergency pause
    if is_emergency_paused(&env) {
        return Err(PayrollError::EmergencyPaused);
    }

    let contributor: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Contributor(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    contributor.require_auth();

    // Check if agreement is paused
    let status: AgreementStatus = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Status(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    if status == AgreementStatus::Paused {
        return Err(PayrollError::AgreementPaused);
    }

    let count: u32 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .ok_or(PayrollError::MilestoneNotFound)?;
    if milestone_id == 0 || milestone_id > count {
        return Err(PayrollError::MilestoneNotFound);
    }

    let approved: bool = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneApproved(agreement_id, milestone_id))
        .unwrap_or(false);
    if !approved {
        return Err(PayrollError::MilestoneNotApproved);
    }

    let already_claimed: bool = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneClaimed(agreement_id, milestone_id))
        .unwrap_or(false);
    if already_claimed {
        return Err(PayrollError::MilestoneAlreadyClaimed);
    }

    // Invariant check: accounted escrow balance must cover all unclaimed
    // milestones before we allow the transfer. Using the accounted balance
    // prevents third-party token transfers from inflating claimable funds.
    let unclaimed_sum = sum_unclaimed_milestones(&env, agreement_id);
    let escrow_balance: i128 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneEscrowBalance(agreement_id))
        .unwrap_or(0i128);
    if escrow_balance < unclaimed_sum {
        return Err(PayrollError::InsufficientEscrowBalance);
    }

    let amount: i128 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneAmount(agreement_id, milestone_id))
        .ok_or(PayrollError::MilestoneNotFound)?;

    let token_address: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Token(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;

    // Checks-Effects-Interactions: update all state before the external transfer.
    env.storage().persistent().set(
        &MilestoneKey::MilestoneClaimed(agreement_id, milestone_id),
        &true,
    );

    // Decrement the accounted escrow balance so subsequent invariant checks
    // reflect the reduced available balance.
    let escrow_balance: i128 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneEscrowBalance(agreement_id))
        .unwrap_or(0i128);
    env.storage().persistent().set(
        &MilestoneKey::MilestoneEscrowBalance(agreement_id),
        &escrow_balance.saturating_sub(amount),
    );

    TokenClient::new(&env, &token_address).transfer(
        &env.current_contract_address(),
        &contributor,
        &amount,
    );

    MilestoneClaimed {
        agreement_id,
        milestone_id,
        amount,
        to: contributor.clone(),
    }
    .publish(&env);

    let all_claimed = all_milestones_claimed(&env, agreement_id, count);
    if all_claimed {
        env.storage().persistent().set(
            &MilestoneKey::Status(agreement_id),
            &AgreementStatus::Completed,
        );
    }

    Ok(())
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
///   At most `MAX_BATCH_SIZE` IDs are accepted.
///
/// # Returns
/// `Ok(BatchMilestoneResult)` with per-milestone results.
///
/// # Batch-level errors
/// These stop the whole batch before any state mutation or transfer:
/// * `PayrollError::AgreementNotFound` — no such agreement (contributor,
///   status, or token record missing).
/// * `PayrollError::InvalidData` — the milestone ID list is empty.
/// * `PayrollError::BatchTooLarge` — more than `MAX_BATCH_SIZE` IDs.
/// * `PayrollError::AgreementPaused` — the agreement is paused.
/// * `PayrollError::MilestoneNotFound` — the agreement has no milestones.
///
/// # Per-milestone `error_code` (in each `MilestoneClaimResult`)
/// `0` = success | `1` = duplicate in this batch | `2` = invalid/unknown
/// milestone ID | `3` = not approved | `4` = already claimed.
///
/// # Gas rationale
/// `MAX_BATCH_SIZE` is 20 because `tests/gas_benchmarks.rs` measures the
/// milestone batch path at that size and enforces the committed gas ceiling.
/// The bound is checked before milestone state updates or token transfers.
pub fn batch_claim_milestones(
    env: &Env,
    agreement_id: u128,
    milestone_ids: Vec<u32>,
) -> Result<BatchMilestoneResult, PayrollError> {
    let contributor: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Contributor(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    contributor.require_auth();

    if milestone_ids.is_empty() {
        return Err(PayrollError::InvalidData);
    }
    if milestone_ids.len() > MAX_BATCH_SIZE {
        return Err(PayrollError::BatchTooLarge);
    }

    // Shared pre-flight
    let status: AgreementStatus = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Status(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    if status == AgreementStatus::Paused {
        return Err(PayrollError::AgreementPaused);
    }

    let count: u32 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .ok_or(PayrollError::MilestoneNotFound)?;

    // Token client created once and reused
    let token: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Token(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
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
        processed.push_back(milestone_id);

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
            .persistent()
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
            .persistent()
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

        let amount: i128 = match env
            .storage()
            .persistent()
            .get(&MilestoneKey::MilestoneAmount(agreement_id, milestone_id))
        {
            Some(amount) => amount,
            None => {
                // Record a per-item failure and continue: an early return here
                // would abort the batch after earlier milestones in this loop
                // had already transferred funds.
                failed_claims += 1;
                results.push_back(MilestoneClaimResult {
                    milestone_id,
                    success: false,
                    amount_claimed: 0,
                    error_code: 2, // amount/milestone not found
                });
                continue;
            }
        };

        // Checks-Effects-Interactions: update all state before the external transfer.
        env.storage().persistent().set(
            &MilestoneKey::MilestoneClaimed(agreement_id, milestone_id),
            &true,
        );

        // Decrement the accounted escrow balance to keep invariants consistent
        // across subsequent iterations of this batch.
        let escrow_balance: i128 = env
            .storage()
            .persistent()
            .get(&MilestoneKey::MilestoneEscrowBalance(agreement_id))
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &MilestoneKey::MilestoneEscrowBalance(agreement_id),
            &escrow_balance.saturating_sub(amount),
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
    }

    if all_milestones_claimed(env, agreement_id, count) {
        env.storage().persistent().set(
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

    Ok(BatchMilestoneResult {
        agreement_id,
        total_claimed,
        successful_claims,
        failed_claims,
        results,
    })
}

pub fn get_milestone_count(env: Env, agreement_id: u128) -> u32 {
    env.storage()
        .persistent()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .unwrap_or(0)
}

pub fn get_milestone(env: Env, agreement_id: u128, milestone_id: u32) -> Option<Milestone> {
    let count: u32 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .unwrap_or(0);

    if milestone_id == 0 || milestone_id > count {
        return None;
    }

    let amount: i128 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneAmount(agreement_id, milestone_id))?;
    let approved: bool = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneApproved(agreement_id, milestone_id))
        .unwrap_or(false);
    let claimed: bool = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneClaimed(agreement_id, milestone_id))
        .unwrap_or(false);

    Some(Milestone {
        id: milestone_id,
        amount,
        approved,
        claimed,
    })
}

/// Reports whether every milestone up to `count` has been claimed.
///
/// # Arguments
/// * `env` - Contract environment used to read claimed flags from instance storage.
/// * `agreement_id` - Milestone agreement identifier whose claim flags should be checked.
/// * `count` - Number of milestones to scan, usually the stored `MilestoneCount` for the agreement.
///
/// # Returns
/// `true` when all milestone IDs from `1..=count` are marked claimed; otherwise `false`.
///
/// # Cost
/// O(n) in `count`. The scan short-circuits on the first unclaimed milestone and is
/// bounded by the caller-supplied milestone count.
fn all_milestones_claimed(env: &Env, agreement_id: u128, count: u32) -> bool {
    for i in 1..=count {
        let claimed: bool = env
            .storage()
            .persistent()
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
/// # Arguments
/// * `env` - Contract environment
/// * `employer` - Address of the employer creating the agreements
/// * `items` - Vector of payroll creation parameters
///   At most `MAX_BATCH_SIZE` items are accepted.
///
/// # Returns
/// `Ok(BatchPayrollCreateResult)` — always succeeds at the batch level
/// unless `items` is empty; inspect per-item results for failures.
///
/// # Batch-level errors
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
///   At most `MAX_BATCH_SIZE` items are accepted.
///
/// # Returns
/// `Ok(BatchEscrowCreateResult)` — always succeeds at the batch level
/// unless `items` is empty; inspect per-item `results` for failures.
///
/// # Batch-level errors
/// * `PayrollError::BatchTooLarge` — more than `MAX_BATCH_SIZE` items.
///
/// # Gas rationale
/// `MAX_BATCH_SIZE` is 20, matching the largest batch size measured in
/// `tests/gas_benchmarks.rs`; the cap avoids late Soroban resource exhaustion
/// after partially creating agreements.
pub fn batch_create_escrow_agreements(
    env: &Env,
    employer: Address,
    items: Vec<EscrowCreateParams>,
) -> Result<BatchEscrowCreateResult, PayrollError> {
    employer.require_auth();

    if items.is_empty() {
        return Err(PayrollError::InvalidData);
    }
    if items.len() > MAX_BATCH_SIZE {
        return Err(PayrollError::BatchTooLarge);
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
    record_entry(
        env,
        agreement.employer,
        AuditEvent::AgreementActivated,
        agreement_id,
        None,
        Some(agreement.total_amount),
    );
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

    let arbiter_for_log = arbiter.clone();
    env.storage()
        .persistent()
        .set(&StorageKey::Arbiter, &arbiter);
    emit_set_arbiter(env, ArbiterSetEvent { arbiter });

    // Record a lifecycle audit entry so `set_arbiter` is observable in the
    // audit trail (contract-level event: sentinel `agreement_id` 0, subject
    // is the newly-set arbiter).
    record_entry(
        env,
        caller,
        AuditEvent::ArbiterSet,
        0,
        Some(arbiter_for_log),
        None,
    );

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
///
/// # Policy semantics
/// Both policy fields must be strictly positive. A zero value is treated as an
/// invalid configuration (not "disabled"), because a zero cap silently disables
/// all grace extensions and removes a safety mechanism with no error:
/// - `max_cumulative_extension_bps == 0` would make every extension exceed the
///   (zero) cumulative cap, so no extension could ever be applied.
/// - `max_extension_per_call_seconds == 0` would reject every single-call
///   extension.
///
/// To intentionally stop allowing extensions, set the caps to a small, explicit
/// non-zero value rather than zero, so the configuration choice is auditable and
/// cannot happen by accident. Both fields are also bounded above so a compromised
/// owner key cannot configure absurd values in one transaction.
///
/// Returns [`PayrollError::GraceExtensionInvalid`] when either field is zero or
/// exceeds its upper bound.
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
    // Reject zero on both fields: a zero cap silently disables grace extensions,
    // which must be an explicit, non-zero configuration rather than an accident.
    if policy.max_cumulative_extension_bps == 0 || policy.max_cumulative_extension_bps > MAX_BPS {
        return Err(PayrollError::GraceExtensionInvalid);
    }
    if policy.max_extension_per_call_seconds == 0
        || policy.max_extension_per_call_seconds > MAX_PER_CALL
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
    let max_extra = (base.saturating_mul(policy.max_cumulative_extension_bps as u128)) / 10000;

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
        effective_cancelled_grace_duration_seconds(
            env,
            agreement_id,
            agreement.grace_period_seconds,
        )
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
    record_entry(
        env,
        caller,
        AuditEvent::DisputeRaised,
        agreement_id,
        Some(agreement.employer),
        Some(agreement.total_amount),
    );

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
/// # Conservation of funds
/// The payout is deterministic and conserves funds. `pay_employee` is divided
/// equally across employees using integer division; the integer-division
/// **remainder (dust)** is added to the **last** employee's transfer so that the
/// sum of all employee transfers equals `pay_employee` exactly and no tokens are
/// stranded in the contract. Both `pay_employee` and `refund_employer` must be
/// non-negative, and `pay_employee + refund_employer` must not exceed either the
/// agreement's `total_amount` or its **real escrow balance** for the agreement's
/// token; the per-agreement escrow balance is decremented by the distributed
/// total after the transfers succeed.
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
    // If a DisputeResolution threshold is configured and the total payout meets
    // it, reject and require the caller to use resolve_dispute_multisig instead.
    let total_payout = pay_employee + refund_employer;
    if let Some(threshold) = env
        .storage()
        .persistent()
        .get::<_, i128>(&StorageKey::DisputeResolutionThreshold)
    {
        if threshold > 0 && total_payout >= threshold {
            return Err(PayrollError::MultisigApprovalRequired);
        }
    }
    resolve_dispute_core(&env, caller, agreement_id, pay_employee, refund_employer)
}

/// Resolves a dispute that has been pre-approved by the multisig contract.
///
/// # Arguments
/// * `caller` - Arbiter address (must authenticate)
/// * `agreement_id` - Agreement under dispute
/// * `pay_employee` - Amount to distribute to employees
/// * `refund_employer` - Amount to refund the employer
/// * `multisig_operation_id` - ID of the Executed DisputeResolution operation in the multisig
///
/// # Access Control
/// Requires arbiter authentication and a valid Executed multisig operation.
pub fn resolve_dispute_multisig(
    env: Env,
    caller: Address,
    agreement_id: u128,
    pay_employee: i128,
    refund_employer: i128,
    multisig_operation_id: u128,
) -> Result<(), PayrollError> {
    let multisig_addr = env
        .storage()
        .persistent()
        .get::<_, Address>(&StorageKey::MultisigContract)
        .ok_or(PayrollError::MultisigApprovalRequired)?;

    let payroll_contract = env.current_contract_address();
    require_multisig_executed(&env, &multisig_addr, multisig_operation_id, |kind| {
        matches!(
            kind,
            OperationKind::DisputeResolution(addr, aid, pe, re)
                if *addr == payroll_contract
                    && *aid == agreement_id
                    && *pe == pay_employee
                    && *re == refund_employer
        )
    })?;

    resolve_dispute_core(&env, caller, agreement_id, pay_employee, refund_employer)
}

fn resolve_dispute_core(
    env: &Env,
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

    let mut agreement = get_agreement(env, agreement_id).ok_or(PayrollError::AgreementNotFound)?;

    if agreement.dispute_status != DisputeStatus::Raised {
        return Err(PayrollError::NoDispute);
    }

    // Reject negative payouts (griefing / accounting corruption).
    if pay_employee < 0 || refund_employer < 0 {
        return Err(PayrollError::InvalidPayout);
    }

    let total_payout = pay_employee
        .checked_add(refund_employer)
        .ok_or(PayrollError::InvalidPayout)?;

    // Validate against the agreement's nominal total AND, when a real
    // per-agreement escrow balance is tracked, against that balance too.
    // Validating only against `total_amount` could desync internal accounting
    // from actual token balances and over-distribute across disputes.
    //
    // `escrow_balance == 0` is treated as "untracked" (e.g. legacy agreements
    // whose escrow was never recorded via `set_agreement_escrow_balance`); for
    // those we fall back to the `total_amount` bound and do not decrement, to
    // avoid driving a never-tracked balance negative.
    let total_locked = agreement.total_amount;
    if total_payout > total_locked {
        return Err(PayrollError::InvalidPayout);
    }
    let escrow_balance = DataKey::get_agreement_escrow_balance(env, agreement_id, &agreement.token);
    let escrow_tracked = escrow_balance > 0;
    if escrow_tracked && total_payout > escrow_balance {
        return Err(PayrollError::InvalidPayout);
    }

    let token = TokenClient::new(env, &agreement.token);

    let employees: Vec<EmployeeInfo> = env
        .storage()
        .persistent()
        .get(&StorageKey::AgreementEmployees(agreement_id))
        .unwrap_or(Vec::new(env));

    // Track what is actually transferred out so the escrow balance can be
    // decremented by the exact distributed total (conservation of funds).
    let mut distributed: i128 = 0;

    // Execute transfers. The integer-division remainder (dust) is allocated
    // deterministically to the LAST employee so the employee transfers sum to
    // `pay_employee` exactly and nothing is stranded.
    if pay_employee > 0 {
        let num_employees = employees.len() as i128;
        if num_employees > 0 {
            let amount_per_employee = pay_employee / num_employees;
            let dust = pay_employee - amount_per_employee * num_employees;
            let last_index = employees.len() - 1;
            for (i, employee) in employees.iter().enumerate() {
                let mut amount = amount_per_employee;
                if i as u32 == last_index {
                    amount += dust;
                }
                if amount > 0 {
                    token.transfer(&env.current_contract_address(), &employee.address, &amount);
                    distributed += amount;
                }
            }
        }
        // If there are no employees, `pay_employee` cannot be distributed and is
        // left as part of the (unchanged) escrow rather than silently lost.
    }

    if refund_employer > 0 {
        token.transfer(
            &env.current_contract_address(),
            &agreement.employer,
            &refund_employer,
        );
        distributed += refund_employer;
    }

    // Decrement the per-agreement escrow balance by exactly what was distributed,
    // keeping internal accounting consistent with real token balances. Skipped
    // when escrow was never tracked (balance was 0) so it is not driven negative.
    if escrow_tracked {
        DataKey::set_agreement_escrow_balance(
            env,
            agreement_id,
            &agreement.token,
            escrow_balance - distributed,
        );
    }

    agreement.dispute_status = DisputeStatus::Resolved;
    agreement.status = AgreementStatus::Completed;
    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement_id), &agreement);

    emit_dsipute_resolved(
        env,
        DisputeResolvedEvent {
            agreement_id,
            pay_contributor: pay_employee,
            refund_employer,
        },
    );
    record_entry(
        env,
        caller,
        AuditEvent::DisputeResolved,
        agreement_id,
        Some(agreement.employer),
        Some(pay_employee + refund_employer),
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

    // Enforce absolute sanity bound if configured.
    if let Some(max_rate) = DataKey::get_exchange_rate_max_rate_sanity_bound(env) {
        if rate > max_rate {
            return Err(PayrollError::ExchangeRateInvalid);
        }
    }

    // Enforce max-deviation if configured: compare with previous rate.
    if let Some(max_dev_bps) = DataKey::get_exchange_rate_max_deviation_bps(env) {
        if let Some(prev) = DataKey::get_exchange_rate(env, &base, &quote) {
            // compute allowed delta = prev.rate * max_dev_bps / 10000
            let prev_rate = prev.rate;
            // Avoid negative or zero prev_rate (shouldn't happen)
            if prev_rate > 0 {
                let allowed_delta = (prev_rate
                    .checked_mul(max_dev_bps as i128)
                    .unwrap_or(i128::MAX))
                .checked_div(10_000i128)
                .unwrap_or(i128::MAX);
                let diff = if rate > prev_rate {
                    rate - prev_rate
                } else {
                    prev_rate - rate
                };
                if diff > allowed_delta {
                    return Err(PayrollError::ExchangeRateInvalid);
                }
            }
        }
    }

    let prev_rate = DataKey::get_exchange_rate(env, &base, &quote)
        .map(|r| r.rate)
        .unwrap_or(0);

    DataKey::set_exchange_rate(env, &base, &quote, rate);

    emit_exchange_rate_changed(
        env,
        ExchangeRateChangedEvent {
            base,
            quote,
            new_rate: rate,
            prev_rate,
            updated_at: env.ledger().timestamp(),
        },
    );

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
/// Bumps the agreement entry's TTL on read (see [`crate::storage::extend_persistent_ttl`])
/// so an active agreement that is accessed but not rewritten for a long time is
/// not archived under Soroban's state-archival model.
///
/// # Returns
/// Some(Agreement) if found, None otherwise
pub fn get_agreement(env: &Env, agreement_id: u128) -> Option<Agreement> {
    let key = StorageKey::Agreement(agreement_id);
    let agreement = env.storage().persistent().get(&key);
    if agreement.is_some() {
        crate::storage::extend_persistent_ttl(env, &key);
    }
    agreement
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
    // Guard the entire claim against cross-contract reentrancy (e.g. a hostile
    // token re-entering during `transfer`). The guard is released on every
    // return path; a panic clears it automatically via temporary storage.
    acquire_reentrancy_guard(env)?;
    let result = claim_payroll_inner(env, caller, agreement_id, employee_index);
    release_reentrancy_guard(env);
    result
}

fn claim_payroll_inner(
    env: &Env,
    caller: &Address,
    agreement_id: u128,
    employee_index: u32,
) -> Result<(), PayrollError> {
    enforce_rate_limit(env, caller)?;

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

    // Invariant check: claimed_periods <= num_periods (if defined)
    if let Some(num_periods) = agreement.num_periods {
        assert!(
            claimed_periods <= num_periods,
            "Invariant violation: claimed_periods > num_periods"
        );
    }

    // Calculate periods to pay
    if total_elapsed_periods <= claimed_periods {
        return Err(PayrollError::NoPeriodsToClaim);
    }

    let periods_to_pay = total_elapsed_periods - claimed_periods;

    // Get employee salary per period, checking for dynamic adjustment overrides.
    let mut salary_per_period = DataKey::get_employee_salary(env, agreement_id, employee_index)
        .ok_or(PayrollError::AgreementNotFound)?;

    if let Some(salary_adj_addr) = env
        .storage()
        .persistent()
        .get::<_, Address>(&StorageKey::SalaryAdjustmentContract)
    {
        let client = SalaryAdjustmentClient::new(env, &salary_adj_addr);
        if let Some(adjusted_salary) = client.get_employee_salary(&employee) {
            salary_per_period = adjusted_salary;
        }
    }

    // Calculate total amount to pay
    let amount = salary_per_period
        .checked_mul(periods_to_pay as i128)
        .ok_or(PayrollError::InvalidData)?;

    // If a LargePayment threshold is configured and this claim meets it,
    // reject and require the caller to use claim_payroll_multisig instead.
    if let Some(threshold) = env
        .storage()
        .persistent()
        .get::<_, i128>(&StorageKey::LargePaymentThreshold)
    {
        if threshold > 0 && amount >= threshold {
            return Err(PayrollError::MultisigApprovalRequired);
        }
    }

    // Check escrow balance
    let escrow_balance = DataKey::get_agreement_escrow_balance(env, agreement_id, &token);
    if escrow_balance < amount {
        return Err(PayrollError::InsufficientEscrowBalance);
    }

    // Get contract address (this contract)
    let contract_address = env.current_contract_address();

    // === EFFECTS BEFORE INTERACTION (checks-effects-interactions) ===
    // Persist the new escrow balance, claimed periods, and paid amount BEFORE
    // the external token transfer, so a malicious or hook-enabled token cannot
    // re-enter and observe stale state to double-claim a period. The transient
    // reentrancy guard (acquired by the caller) is the primary defense; this
    // ordering is defense-in-depth.
    let new_escrow_balance = escrow_balance - amount;
    DataKey::set_agreement_escrow_balance(env, agreement_id, &token, new_escrow_balance);

    let new_claimed_periods = claimed_periods + periods_to_pay;
    DataKey::set_employee_claimed_periods(env, agreement_id, employee_index, new_claimed_periods);

    let current_paid = DataKey::get_agreement_paid_amount(env, agreement_id);
    let new_paid = current_paid
        .checked_add(amount)
        .ok_or(PayrollError::InvalidData)?;
    DataKey::set_agreement_paid_amount(env, agreement_id, new_paid);

    // === INTERACTION: transfer tokens from escrow to employee ===
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

/// Claims payroll for a large payment that has been pre-approved by the multisig contract.
///
/// # Arguments
/// * `caller` - Employee address (must authenticate)
/// * `agreement_id` - Payroll agreement ID
/// * `employee_index` - Employee index within the agreement
/// * `multisig_operation_id` - ID of the Executed LargePayment operation in the multisig
///
/// # Access Control
/// Requires employee authentication and a valid Executed multisig LargePayment operation
/// whose `to` field matches the caller and `amount` matches the computed payout.
pub fn claim_payroll_multisig(
    env: &Env,
    caller: &Address,
    agreement_id: u128,
    employee_index: u32,
    multisig_operation_id: u128,
) -> Result<(), PayrollError> {
    let multisig_addr = env
        .storage()
        .persistent()
        .get::<_, Address>(&StorageKey::MultisigContract)
        .ok_or(PayrollError::MultisigApprovalRequired)?;

    // Temporarily clear the threshold so claim_payroll_core can proceed.
    // We verify the multisig op here before delegating.
    let client = MultisigClient::new(env, &multisig_addr);
    let op = client
        .get_operation(&multisig_operation_id)
        .ok_or(PayrollError::MultisigApprovalRequired)?;
    if op.status != OperationStatus::Executed {
        return Err(PayrollError::MultisigApprovalRequired);
    }
    // Verify the operation targets this caller (employee) with a LargePayment kind.
    match &op.kind {
        OperationKind::LargePayment(_, to, _) if to == caller => {}
        _ => return Err(PayrollError::MultisigApprovalRequired),
    }

    // Bypass the threshold guard by temporarily removing it, run claim, then restore.
    let saved_threshold: Option<i128> = env
        .storage()
        .persistent()
        .get(&StorageKey::LargePaymentThreshold);
    env.storage()
        .persistent()
        .remove(&StorageKey::LargePaymentThreshold);
    let result = claim_payroll(env, caller, agreement_id, employee_index);
    if let Some(t) = saved_threshold {
        env.storage()
            .persistent()
            .set(&StorageKey::LargePaymentThreshold, &t);
    }
    result
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
    // Reentrancy guard mirrors `claim_payroll`; released on every return path.
    acquire_reentrancy_guard(env)?;
    let result =
        claim_payroll_in_token_inner(env, caller, agreement_id, employee_index, payout_token);
    release_reentrancy_guard(env);
    result
}

fn claim_payroll_in_token_inner(
    env: &Env,
    caller: &Address,
    agreement_id: u128,
    employee_index: u32,
    payout_token: Address,
) -> Result<(), PayrollError> {
    enforce_rate_limit(env, caller)?;

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
    // Call the inner (unguarded) variant because the guard is already held by
    // this function's wrapper; re-acquiring would self-trip the guard.
    if payout_token == base_token {
        return claim_payroll_inner(env, caller, agreement_id, employee_index);
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

    // === EFFECTS BEFORE INTERACTION (checks-effects-interactions) ===
    // Persist payout-currency escrow, claimed periods, and base-currency paid
    // amount BEFORE the external transfer (defense-in-depth alongside the guard).
    let new_escrow_payout = escrow_balance_payout - amount_payout;
    DataKey::set_agreement_escrow_balance(env, agreement_id, &payout_token, new_escrow_payout);

    let new_claimed_periods = claimed_periods + periods_to_pay;
    DataKey::set_employee_claimed_periods(env, agreement_id, employee_index, new_claimed_periods);

    let current_paid = DataKey::get_agreement_paid_amount(env, agreement_id);
    let new_paid = current_paid
        .checked_add(amount_base)
        .ok_or(PayrollError::InvalidData)?;
    DataKey::set_agreement_paid_amount(env, agreement_id, new_paid);

    // === INTERACTION: transfer tokens from escrow to employee in payout currency ===
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
///   At most `MAX_BATCH_SIZE` indices are accepted.
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
/// * `PayrollError::BatchTooLarge` — more than `MAX_BATCH_SIZE` indices
///
/// # Gas rationale
/// `MAX_BATCH_SIZE` is 20 because `tests/gas_benchmarks.rs` records the batch
/// ceiling and enforces the committed gas threshold. The bound is checked
/// before payroll state updates or token transfers, preserving partial-success
/// semantics for only bounded batches.
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
    // Reentrancy guard covers the whole batch (each per-employee transfer is a
    // potential reentry point); released on every return path.
    acquire_reentrancy_guard(env)?;
    let result = batch_claim_payroll_inner(env, caller, agreement_id, employee_indices);
    release_reentrancy_guard(env);
    result
}

fn batch_claim_payroll_inner(
    env: &Env,
    caller: &Address,
    agreement_id: u128,
    employee_indices: Vec<u32>,
) -> Result<BatchPayrollResult, PayrollError> {
    caller.require_auth();

    if let Err(e) = enforce_rate_limit(env, caller) {
        return Err(e);
    }

    if employee_indices.is_empty() {
        return Err(PayrollError::InvalidData);
    }
    if employee_indices.len() > MAX_BATCH_SIZE {
        return Err(PayrollError::BatchTooLarge);
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
        processed.push_back(employee_index);

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

        // Invariant check: claimed_periods <= num_periods (if defined)
        if let Some(num_periods) = agreement.num_periods {
            assert!(
                claimed_periods <= num_periods,
                "Invariant violation: claimed_periods > num_periods"
            );
        }

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

        // Salary must be configured, checking for dynamic adjustment overrides.
        let mut salary_per_period =
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

        if let Some(salary_adj_addr) = env
            .storage()
            .persistent()
            .get::<_, Address>(&StorageKey::SalaryAdjustmentContract)
        {
            let client = SalaryAdjustmentClient::new(env, &salary_adj_addr);
            if let Some(adjusted_salary) = client.get_employee_salary(&employee) {
                salary_per_period = adjusted_salary;
            }
        }

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

        // === EFFECTS BEFORE INTERACTION (checks-effects-interactions) ===
        // Decrement the in-memory escrow and persist per-employee claimed
        // periods and the agreement paid amount BEFORE the external transfer,
        // so a hostile token cannot re-enter and observe stale per-employee
        // state. The transaction-level reentrancy guard is the primary defense.
        escrow_balance -= amount;
        total_claimed += amount;
        successful_claims += 1;

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

        // === INTERACTION: transfer tokens from escrow to employee ===
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

    // Invariant check
    assert!(
        claimed_periods <= num_periods,
        "Invariant violation: claimed_periods > num_periods"
    );

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
    // Keep the standalone paid-amount key in sync, like claim_payroll,
    // batch_claim_payroll and resolve_dispute do, so the escrow-conservation
    // invariant (remaining + paid == total) holds for time-based claims too.
    DataKey::set_agreement_paid_amount(env, agreement_id, agreement.paid_amount);

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

    let info = DataKey::get_exchange_rate(env, from_token, to_token)
        .ok_or(PayrollError::ExchangeRateNotFound)?;

    let rate = info.rate;
    if rate <= 0 {
        return Err(PayrollError::ExchangeRateInvalid);
    }

    // Enforce staleness (max-age) if configured
    if let Some(max_age) = DataKey::get_exchange_rate_max_age_seconds(env) {
        let now = env.ledger().timestamp();
        // Protect against underflow
        if now < info.updated_at {
            return Err(PayrollError::ExchangeRateInvalid);
        }
        if now - info.updated_at > max_age {
            return Err(PayrollError::ExchangeRateNotFound);
        }
    }

    let scaled = amount
        .checked_mul(rate)
        .ok_or(PayrollError::ExchangeRateOverflow)?;

    let converted = scaled
        .checked_div(FX_SCALE)
        .ok_or(PayrollError::ExchangeRateInvalid)?;

    // Dust guard: reject conversions that floor-round to zero to prevent
    // callers from being silently credited nothing for a non-zero input.
    if converted < DUST_THRESHOLD {
        return Err(PayrollError::ExchangeRateInvalid);
    }

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
/// * `env` - Contract environment used to authenticate the employer and update the stored agreement status.
/// * `agreement_id` - ID of the milestone agreement to pause; must resolve to existing employer and status records.
///
/// # Returns
/// No value. Emits `AgreementPausedEvent` after writing the paused status.
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
///
/// # Errors
/// * `PayrollError::AgreementNotFound` — the milestone agreement does not exist.
/// * `PayrollError::MilestoneAgreementInvalidStatus` — the agreement is not in `Active` or `Created` status.
pub fn pause_milestone_agreement(env: Env, agreement_id: u128) -> Result<(), PayrollError> {
    let employer: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Employer(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    employer.require_auth();

    let status: AgreementStatus = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Status(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;

    // Allow pausing Active agreements, or Created agreements (which can have claimable milestones)
    if status != AgreementStatus::Active && status != AgreementStatus::Created {
        return Err(PayrollError::MilestoneAgreementInvalidStatus);
    }

    env.storage().persistent().set(
        &MilestoneKey::Status(agreement_id),
        &AgreementStatus::Paused,
    );

    AgreementPausedEvent { agreement_id }.publish(&env);

    Ok(())
}

/// Resumes a paused milestone-based agreement, allowing claims again
///
/// # Arguments
/// * `env` - Contract environment used to authenticate the employer and update the stored agreement status.
/// * `agreement_id` - ID of the paused milestone agreement to resume; must resolve to existing employer and status records.
///
/// # Returns
/// No value. Emits `AgreementResumedEvent` after writing the active status.
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
///
/// # Errors
/// * `PayrollError::AgreementNotFound` — the milestone agreement does not exist.
/// * `PayrollError::MilestoneAgreementInvalidStatus` — the agreement is not in `Paused` status.
pub fn resume_milestone_agreement(env: Env, agreement_id: u128) -> Result<(), PayrollError> {
    let employer: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Employer(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    employer.require_auth();

    let status: AgreementStatus = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Status(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;

    if status != AgreementStatus::Paused {
        return Err(PayrollError::MilestoneAgreementInvalidStatus);
    }

    // Resume to Active status (milestone agreements can have claimable milestones in Active state)
    env.storage().persistent().set(
        &MilestoneKey::Status(agreement_id),
        &AgreementStatus::Active,
    );

    AgreementResumedEvent { agreement_id }.publish(&env);

    Ok(())
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
    record_entry(
        env,
        agreement.employer,
        AuditEvent::AgreementCancelled,
        agreement_id,
        None,
        Some(agreement.total_amount),
    );
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
