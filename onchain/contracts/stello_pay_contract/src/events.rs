//-----------------------------------------------------------------------------
// Events
//-----------------------------------------------------------------------------

use soroban_sdk::{symbol_short, Address, Env, Symbol};

/// Event emitted when contract is paused
pub const PAUSED_EVENT: Symbol = symbol_short!("paused");

/// Event emitted when contract is unpaused
pub const UNPAUSED_EVENT: Symbol = symbol_short!("unpaused");

pub const DEPOSIT_EVENT: Symbol = symbol_short!("deposit");


pub fn emit_disburse(
        e: Env,
        employer: Address,
        employee: Address,
        token: Address,
        amount: i128,
        timestamp: u64,
    ) {
        let topics = (Symbol::new(&e, "SalaryDisbursed"),);
        e.events().publish(topics, 
            (
                employer,
                employee,
                token,
                amount,
                timestamp,
            )
        );
    }

