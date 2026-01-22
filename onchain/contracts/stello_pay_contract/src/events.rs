use soroban_sdk::{contracttype, Address};

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
use soroban_sdk::{contracttype, symbol_short, Address, Env,};

use crate::storage::AgreementMode;

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
