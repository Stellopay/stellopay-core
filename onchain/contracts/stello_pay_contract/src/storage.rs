use soroban_sdk::{contracttype, Address};

//-----------------------------------------------------------------------------
// Data Structures
//-----------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq,Eq)]
pub struct Payroll {
    pub employer: Address,
    pub token: Address,
    pub amount: i128,
    pub interval: u64,
    pub last_payment_time: u64,
    pub recurrence_frequency: u64, // Frequency in seconds (e.g., 2592000 for 30 days)
    pub next_payout_timestamp: u64,
     // Next scheduled payout timestamp
}
#[contracttype]
#[derive(Clone,PartialEq,Eq,Debug)]
pub struct PayrollModification{
    pub modification_id:u64,
    pub employee: Address,
    pub employer: Address,
    pub new_data: Payroll,
    pub employee_approved: bool,
    pub employer_approved:bool,
    pub requested_at:u64,
}
/// Input structure for batch payroll creation
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PayrollInput {
    pub employee: Address,
    pub token: Address,
    pub amount: i128,
    pub interval: u64,
    pub recurrence_frequency: u64,
}

/// Compact payroll data for storage optimization
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompactPayroll {
    pub employer: Address,
    pub token: Address,
    pub amount: i128,
    pub interval: u32, // Reduced from u64 to u32 for most use cases
    pub last_payment_time: u64,
    pub recurrence_frequency: u32, // Reduced from u64 to u32 for most use cases
    pub next_payout_timestamp: u64,
}

//-----------------------------------------------------------------------------
// Storage Keys
//-----------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataKey {
    // Consolidated payroll storage - single key per employee
    Payroll(Address), // employee -> Payroll struct
    PendingModifications(u64),
    ModificationNonce,
    TimeoutPeriod,
    // Indexing for efficient queries
    EmployerEmployees(Address), // employer -> Vec<Employee>
    TokenEmployees(Address),    // token -> Vec<Employee>

    // Employer balance, keyed by (employer, token)
    Balance(Address, Address),

    // Admin
    Owner,
    Paused,

    SupportedToken(Address),
    TokenMetadata(Address),
}
