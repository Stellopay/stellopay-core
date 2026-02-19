use crate::storage::AgreementMode;
use soroban_sdk::{contractevent, Address, Env};

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
