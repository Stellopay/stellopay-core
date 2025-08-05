//-----------------------------------------------------------------------------
// Events
//-----------------------------------------------------------------------------

use soroban_sdk::{contracttype, symbol_short, Address, Env, Symbol};

/// Event emitted when contract is paused
pub const PAUSED_EVENT: Symbol = symbol_short!("paused");

/// Event emitted when contract is unpaused
pub const UNPAUSED_EVENT: Symbol = symbol_short!("unpaused");

pub const DEPOSIT_EVENT: Symbol = symbol_short!("deposit");

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SalaryDisbursed {
    pub employer: Address,
    pub employee: Address,
    pub token: Address,
    pub amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModificationEvent {
    pub id: u64,
    pub employee: Address,
    pub employer: Address,
    pub action:Symbol, //"request","approve","reject"
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmployerWithdrawn {
    pub employer: Address,
    pub token: Address,
    pub amount: i128,
    pub timestamp: u64,
}

pub fn modification_event(env:Env,symbol:&str,ev:ModificationEvent){
    let topic=(Symbol::new(&env,symbol),);
    env.events().publish(topic,ev);
}

pub fn emit_modification_event(env:&Env,event:ModificationEvent){
    env.events().publish(
        (Symbol::short("modification"),),
        event,
    );
}
pub fn emit_disburse(
    e: Env,
    employer: Address,
    employee: Address,
    token: Address,
    amount: i128,
    timestamp: u64,
) {
    let topics = (Symbol::new(&e, "SalaryDisbursed"),);
    let event_data = SalaryDisbursed {
        employer,
        employee,
        token,
        amount,
        timestamp,
    };
    e.events().publish(topics, event_data.clone());
}

pub fn emit_employer_withdrawn(
    e: Env,
    employer: Address,
    token: Address,
    amount: i128,
    timestamp: u64,
) {
    let topics = (Symbol::new(&e, "EmployerWithdrawn"),);
    let event_data = EmployerWithdrawn {
        employer,
        token,
        amount,
        timestamp,
    };
    e.events().publish(topics, event_data.clone());
}
