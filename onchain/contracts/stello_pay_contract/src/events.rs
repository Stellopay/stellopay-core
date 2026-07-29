use soroban_sdk::{contractevent, Address, Env};

use crate::storage::AgreementMode;

#[contractevent]
#[derive(Clone, Debug)]
pub struct MilestoneAdded {
    pub agreement_id: u128,
    pub milestone_id: u32,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct MilestoneApproved {
    pub agreement_id: u128,
    pub milestone_id: u32,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct MilestoneClaimed {
    pub agreement_id: u128,
    pub milestone_id: u32,
    pub amount: i128,
    pub to: Address,
}

/// Event: Agreement created
#[contractevent]
#[derive(Clone, Debug)]
pub struct AgreementCreatedEvent {
    pub agreement_id: u128,
    pub employer: Address,
    pub mode: AgreementMode,
}

/// Event: Agreement activated
#[contractevent]
#[derive(Clone, Debug)]
pub struct AgreementActivatedEvent {
    pub agreement_id: u128,
}

/// Event: Employee added to agreement
#[contractevent]
#[derive(Clone, Debug)]
pub struct EmployeeAddedEvent {
    pub agreement_id: u128,
    pub employee: Address,
    pub salary_per_period: i128,
}

/// Event: Payroll claimed by employee
#[contractevent]
#[derive(Clone, Debug)]
pub struct PayrollClaimedEvent {
    pub agreement_id: u128,
    pub employee: Address,
    pub amount: i128,
}

/// Event: Agreement paused
#[contractevent]
#[derive(Clone, Debug)]
pub struct AgreementPausedEvent {
    pub agreement_id: u128,
}

/// Event: Agreement resumed
#[contractevent]
#[derive(Clone, Debug)]
pub struct AgreementResumedEvent {
    pub agreement_id: u128,
}

/// Event: Payment sent
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentSentEvent {
    pub agreement_id: u128,
    pub from: Address,
    pub to: Address,
    pub amount: i128,
    pub token: Address,
}

/// Event: Payment received
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentReceivedEvent {
    pub agreement_id: u128,
    pub to: Address,
    pub amount: i128,
    pub token: Address,
}

/// Event: Contract storage migration applied
#[contractevent]
#[derive(Clone, Debug)]
pub struct ContractMigratedEvent {
    pub from_version: u32,
    pub to_version: u32,
}

pub fn emit_contract_migrated(env: &Env, event: ContractMigratedEvent) {
    event.publish(env);
}

pub fn emit_agreement_created(env: &Env, event: AgreementCreatedEvent) {
    event.publish(env);
}

pub fn emit_agreement_activated(env: &Env, event: AgreementActivatedEvent) {
    event.publish(env);
}

pub fn emit_employee_added(env: &Env, event: EmployeeAddedEvent) {
    event.publish(env);
}

/// Event: ArbiterSet
#[contractevent]
#[derive(Clone, Debug)]
pub struct ArbiterSetEvent {
    pub arbiter: Address,
}

pub fn emit_set_arbiter(env: &Env, event: ArbiterSetEvent) {
    event.publish(env);
}

/// Event: ArbiteDisputeRaisedrSet
#[contractevent]
#[derive(Clone, Debug)]
pub struct DisputeRaisedEvent {
    pub agreement_id: u128,
}

pub fn emit_dsipute_raised(env: &Env, event: DisputeRaisedEvent) {
    event.publish(env);
}

/// Event: ArbiteDisputeRaisedrSet
#[contractevent]
#[derive(Clone, Debug)]
pub struct DisputeResolvedEvent {
    pub agreement_id: u128,
    pub pay_contributor: i128,
    pub refund_employer: i128,
}

pub fn emit_dsipute_resolved(env: &Env, event: DisputeResolvedEvent) {
    event.publish(env);
}
pub fn emit_payroll_claimed(env: &Env, event: PayrollClaimedEvent) {
    event.publish(env);
}

pub fn emit_agreement_paused(env: &Env, event: AgreementPausedEvent) {
    event.publish(env);
}

pub fn emit_agreement_resumed(env: &Env, event: AgreementResumedEvent) {
    event.publish(env);
}

pub fn emit_payment_sent(env: &Env, event: PaymentSentEvent) {
    event.publish(env);
}

pub fn emit_payment_received(env: &Env, event: PaymentReceivedEvent) {
    event.publish(env);
}

/// Event: Agreement cancelled
#[contractevent]
#[derive(Clone, Debug)]
pub struct AgreementCancelledEvent {
    pub agreement_id: u128,
}

pub fn emit_agreement_cancelled(env: &Env, event: AgreementCancelledEvent) {
    event.publish(env);
}

/// Event: Grace period finalized
#[contractevent]
#[derive(Clone, Debug)]
pub struct GracePeriodFinalizedEvent {
    pub agreement_id: u128,
}

pub fn emit_grace_period_finalized(env: &Env, event: GracePeriodFinalizedEvent) {
    event.publish(env);
}

/// Event: Grace period extended (audit trail for employer or owner).
#[contractevent]
#[derive(Clone, Debug)]
pub struct GracePeriodExtendedEvent {
    pub agreement_id: u128,
    /// Seconds added by this call.
    pub additional_seconds: u64,
    /// Total extra seconds stored after this call (excluding base `grace_period_seconds`).
    pub total_extension_seconds: u64,
    /// True if the contract owner authorized the call; false if the employer did.
    pub extended_by_owner: bool,
}

pub fn emit_grace_period_extended(env: &Env, event: GracePeriodExtendedEvent) {
    event.publish(env);
}

/// Event: Batch payroll claimed
#[contractevent]
#[derive(Clone, Debug)]
pub struct BatchPayrollClaimedEvent {
    pub agreement_id: u128,
    pub total_claimed: i128,
    pub successful_claims: u32,
    pub failed_claims: u32,
}

pub fn emit_batch_payroll_claimed(env: &Env, event: BatchPayrollClaimedEvent) {
    event.publish(env);
}

/// Event: Batch milestone claimed
#[contractevent]
#[derive(Clone, Debug)]
pub struct BatchMilestoneClaimedEvent {
    pub agreement_id: u128,
    pub total_claimed: i128,
    pub successful_claims: u32,
    pub failed_claims: u32,
}

pub fn emit_batch_milestone_claimed(env: &Env, event: BatchMilestoneClaimedEvent) {
    event.publish(env);
}

/// Event: Milestone agreement funded by employer.
///
/// Emitted when an employer deposits tokens into the contract for a specific
/// milestone agreement via `fund_milestone_agreement`. The `total_escrow_balance`
/// field reflects the new accounted balance after this deposit.
#[contractevent]
#[derive(Clone, Debug)]
pub struct MilestoneFundedEvent {
    pub agreement_id: u128,
    pub from: Address,
    pub amount: i128,
    pub total_escrow_balance: i128,
}

pub fn emit_milestone_funded(env: &Env, event: MilestoneFundedEvent) {
    event.publish(env);
}

/// Event: Exchange rate set via `set_exchange_rate` or `set_exchange_rate_admin`.
/// Emitted whenever a rate is updated so off-chain indexers can track FX history
/// and monitor who performed the update.
#[contractevent]
#[derive(Clone, Debug)]
pub struct ExchangeRateUpdatedEvent {
    pub base: Address,
    pub quote: Address,
    pub new_rate: i128,
    /// Previous rate, or 0 if this is the first time the pair is set.
    pub prev_rate: i128,
    /// Address that called `set_exchange_rate`.
    pub updater: Address,
    /// Ledger timestamp when this event was emitted.
    pub updated_at: u64,
}

pub fn emit_exchange_rate_updated(env: &Env, event: ExchangeRateUpdatedEvent) {
    event.publish(env);
}

/// Event: multisig approval configuration changed via `set_multisig_config`.
/// Emitted whenever the linked multisig contract or its approval thresholds
/// are updated, so off-chain monitors can track approval-requirement changes
/// mid-lifecycle.
#[contractevent]
#[derive(Clone, Debug)]
pub struct MultisigConfigChangedEvent {
    pub caller: Address,
    pub multisig_contract: Address,
    pub old_large_threshold: i128,
    pub new_large_threshold: i128,
    pub old_dispute_threshold: i128,
    pub new_dispute_threshold: i128,
}

pub fn emit_multisig_config_changed(env: &Env, event: MultisigConfigChangedEvent) {
    event.publish(env);
}

/// Event: A milestone was rejected by the employer.
///
/// Emitted when an employer explicitly rejects a submitted milestone via
/// `reject_milestone`. The `rejected_by` field records the employer address
/// at the time of rejection and `reason` is the mandatory human-readable
/// justification supplied by the caller (must be non-empty). Off-chain
/// indexers can use this event to update milestone status, notify
/// contributors, and track rejection history.
#[contractevent]
#[derive(Clone, Debug)]
pub struct MilestoneRejectedEvent {
    /// The milestone agreement that contains the rejected milestone.
    pub agreement_id: u128,
    /// 1-based identifier of the rejected milestone within the agreement.
    pub milestone_id: u32,
    /// The employer address that performed the rejection.
    pub rejected_by: Address,
    /// Mandatory free-text justification provided by the employer (must be
    /// non-empty and contain at least one non-whitespace character).
    pub reason: soroban_sdk::String,
}

/// Emits a [`MilestoneRejectedEvent`] for the given rejection.
pub fn emit_milestone_rejected(env: &Env, event: MilestoneRejectedEvent) {
    event.publish(env);
}

/// Event: A milestone expired without being claimed or rejected.
///
/// Emitted by `expire_milestone` after the expiry flag is persisted and
/// before the `on_milestone_expired` hook is invoked on the implementing
/// contract (if configured).  Off-chain indexers can use this event to
/// update milestone status, notify contributors, and trigger reconciliation
/// workflows.
#[contractevent]
#[derive(Clone, Debug)]
pub struct MilestoneExpiredEvent {
    /// The milestone agreement that contains the expired milestone.
    pub agreement_id: u128,
    /// 1-based identifier of the expired milestone within the agreement.
    pub milestone_id: u32,
    /// The amount that was locked for this milestone and is now unreleased.
    /// Callers may use this to decide whether to fund a replacement milestone
    /// or cancel the agreement to recover unused escrow.
    pub locked_amount: i128,
    /// The address that triggered expiry (must be the agreement's employer).
    pub expired_by: Address,
}

/// Emits a [`MilestoneExpiredEvent`] for the given expiry.
pub fn emit_milestone_expired(env: &Env, event: MilestoneExpiredEvent) {
    event.publish(env);
}

/// Event: Bulk pause of all agreements for an employer.
#[contractevent]
#[derive(Clone, Debug)]
pub struct BulkAgreementsPausedEvent {
    pub employer: Address,
    pub count: u32,
}

/// Emits a [`BulkAgreementsPausedEvent`] for the given bulk pause.
pub fn emit_bulk_agreements_paused(env: &Env, event: BulkAgreementsPausedEvent) {
    event.publish(env);
}

/// Event: Bulk unpause of all agreements for an employer.
#[contractevent]
#[derive(Clone, Debug)]
pub struct BulkAgreementsUnpausedEvent {
    pub employer: Address,
    pub count: u32,
}

/// Emits a [`BulkAgreementsUnpausedEvent`] for the given bulk unpause.
pub fn emit_bulk_agreements_unpaused(env: &Env, event: BulkAgreementsUnpausedEvent) {
    event.publish(env);
}
