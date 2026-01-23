use crate::storage::AgreementMode;
use soroban_sdk::{contracttype, symbol_short, Address, Env};

#[contracttype]
#[derive(Clone, Debug)]
pub struct MilestoneAdded {
    pub agreement_id: u128,
    pub milestone_id: u32,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct MilestoneApproved {
    pub agreement_id: u128,
    pub milestone_id: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct MilestoneClaimed {
    pub agreement_id: u128,
    pub milestone_id: u32,
    pub amount: i128,
    pub to: Address,
}

/// Event: Agreement created
#[contracttype]
#[derive(Clone, Debug)]
pub struct AgreementCreatedEvent {
    pub agreement_id: u128,
    pub employer: Address,
    pub mode: AgreementMode,
}

/// Event: Agreement activated
#[contracttype]
#[derive(Clone, Debug)]
pub struct AgreementActivatedEvent {
    pub agreement_id: u128,
}

/// Event: Employee added to agreement
#[contracttype]
#[derive(Clone, Debug)]
pub struct EmployeeAddedEvent {
    pub agreement_id: u128,
    pub employee: Address,
    pub salary_per_period: i128,
}

/// Event: Payroll claimed by employee
#[contracttype]
#[derive(Clone, Debug)]
pub struct PayrollClaimedEvent {
    pub agreement_id: u128,
    pub employee: Address,
    pub amount: i128,
}

/// Event: Agreement paused
#[contracttype]
#[derive(Clone, Debug)]
pub struct AgreementPausedEvent {
    pub agreement_id: u128,
}

/// Event: Agreement resumed
#[contracttype]
#[derive(Clone, Debug)]
pub struct AgreementResumedEvent {
    pub agreement_id: u128,
}

/// Event: Payment sent
#[contracttype]
#[derive(Clone, Debug)]
pub struct PaymentSentEvent {
    pub agreement_id: u128,
    pub from: Address,
    pub to: Address,
    pub amount: i128,
    pub token: Address,
}

/// Event: Payment received
#[contracttype]
#[derive(Clone, Debug)]
pub struct PaymentReceivedEvent {
    pub agreement_id: u128,
    pub to: Address,
    pub amount: i128,
    pub token: Address,
}

pub fn emit_agreement_created(env: &Env, event: AgreementCreatedEvent) {
    let topics = (symbol_short!("agr_new"), event.agreement_id);
    env.events().publish(topics, event);
}

pub fn emit_agreement_activated(env: &Env, event: AgreementActivatedEvent) {
    let topics = (symbol_short!("agr_act"), event.agreement_id);
    env.events().publish(topics, event);
}

pub fn emit_employee_added(env: &Env, event: EmployeeAddedEvent) {
    let topics = (symbol_short!("emp_add"), event.agreement_id);
    env.events().publish(topics, event);
}

/// Event: ArbiterSet
#[contracttype]
#[derive(Clone, Debug)]
pub struct ArbiterSetEvent {
    pub arbiter: Address,
}

pub fn emit_set_arbiter(env: &Env, event: ArbiterSetEvent) {
    let topics = (symbol_short!("arb_set"), &event.arbiter);
    env.events().publish(topics, event.clone());
}

/// Event: ArbiteDisputeRaisedrSet
#[contracttype]
#[derive(Clone, Debug)]
pub struct DisputeRaisedEvent {
    pub agreement_id: u128,
}

pub fn emit_dsipute_raised(env: &Env, event: DisputeRaisedEvent) {
    let topics = (symbol_short!("dis_rai"), &event.agreement_id);
    env.events().publish(topics, event.clone());
}

/// Event: ArbiteDisputeRaisedrSet
#[contracttype]
#[derive(Clone, Debug)]
pub struct DisputeResolvedEvent {
    pub agreement_id: u128,
    pub pay_contributor: i128,
    pub refund_employer: i128,
}

pub fn emit_dsipute_resolved(env: &Env, event: DisputeResolvedEvent) {
    let topics = (symbol_short!("dis_res"), &event.agreement_id);
    env.events().publish(topics, event.clone());
}
pub fn emit_payroll_claimed(env: &Env, event: PayrollClaimedEvent) {
    let topics = (symbol_short!("pay_clm"), event.agreement_id);
    env.events().publish(topics, event);
}

pub fn emit_agreement_paused(env: &Env, event: AgreementPausedEvent) {
    let topics = (symbol_short!("agr_pau"), event.agreement_id);
    env.events().publish(topics, event);
}

pub fn emit_agreement_resumed(env: &Env, event: AgreementResumedEvent) {
    let topics = (symbol_short!("agr_res"), event.agreement_id);
    env.events().publish(topics, event);
}

pub fn emit_payment_sent(env: &Env, event: PaymentSentEvent) {
    let topics = (symbol_short!("pay_snt"), event.agreement_id);
    env.events().publish(topics, event);
}

pub fn emit_payment_received(env: &Env, event: PaymentReceivedEvent) {
    let topics = (symbol_short!("pay_rcv"), event.agreement_id);
    env.events().publish(topics, event);
}
