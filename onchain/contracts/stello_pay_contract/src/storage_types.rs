use soroban_sdk::{contracttype, Address};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Payroll(Address), // Keyed by Employee Address
    Owner,
    Paused, // Global pause state (found in one snapshot)
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Payroll {
    pub amount: i128,
    pub employer: Address,
    pub interval: u32,
    pub is_paused: bool,
    pub last_payment_time: u64,
    pub next_payout_timestamp: u64,
    pub recurrence_frequency: u32,
    pub token: Address,
}
