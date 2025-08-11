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
    pub recurrence_frequency: u64, // Frequency in seconds (e.g., 2592000 for 30 days)
    pub next_payout_timestamp: u64, // Next scheduled payout timestamp
    pub is_paused: bool,
}

/// Input structure for batch payroll creation
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PayrollInput {
    pub employee: Address,
    pub token: Address,
    pub amount: i128,
    pub interval: u64,
    pub recurrence_frequency: u64,
}

/// Compact payroll data for storage optimization
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct CompactPayroll {
    pub employer: Address,
    pub token: Address,
    pub amount: i128,
    pub interval: u32, // Reduced from u64 to u32 for most use cases
    pub last_payment_time: u64,
    pub recurrence_frequency: u32, // Reduced from u64 to u32 for most use cases
    pub next_payout_timestamp: u64,
    pub is_paused: bool,
}

//-----------------------------------------------------------------------------
// Storage Keys
//-----------------------------------------------------------------------------

#[contracttype]
pub enum DataKey {
    // Consolidated payroll storage - single key per employee
    Payroll(Address), // employee -> Payroll struct

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

    // Insurance-related storage keys
    InsurancePolicy(Address),            // employee -> InsurancePolicy
    InsuranceClaim(u64),                 // claim_id -> InsuranceClaim
    NextClaimId,                         // Next available claim ID
    InsurancePool(Address),              // token -> InsurancePool
    GuaranteeFund(Address),              // token -> GuaranteeFund
    Guarantee(u64),                      // guarantee_id -> Guarantee
    NextGuaranteeId,                     // Next available guarantee ID
    EmployerGuarantees(Address),         // employer -> Vec<u64> (guarantee IDs)
    RiskAssessment(Address),             // employee -> u32 (risk score)
    InsuranceSettings,                   // Global insurance settings

    // Compliance-related storage keys
    JurisdictionConfig(Address),         // jurisdiction_hash -> JurisdictionConfig
    ComplianceMetrics(Address),          // jurisdiction_hash -> ComplianceMetrics
    RegulatoryReport(Address),           // report_id_hash -> RegulatoryReport
    AuditEntry(Address),                 // entry_id_hash -> AuditEntry
    AuditIndex(Address),                 // address -> Vec<Address> (audit entry ID hashes)
    ComplianceSettings,                  // Global compliance settings
}
