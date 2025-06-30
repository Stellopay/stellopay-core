use soroban_sdk::{contracttype, Address};

//-----------------------------------------------------------------------------
// Data Structures
//-----------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Payroll {
    pub employer: Address,
    pub token: Address,
    pub amount: i128,
    pub interval: u64,
    pub last_payment_time: u64,
}

//-----------------------------------------------------------------------------
// Storage Keys
//-----------------------------------------------------------------------------

#[contracttype]
pub enum DataKey {
    // Payroll data, keyed by employee address
    PayrollEmployer(Address),
    PayrollToken(Address),
    PayrollAmount(Address),
    PayrollInterval(Address),
    PayrollLastPayment(Address),

    // Employer balance, keyed by (employer, token)
    Balance(Address, Address),

    // Admin
    Owner,
    Paused,
}
