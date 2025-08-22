use soroban_sdk::{contracttype, Address, Symbol, String};

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

/// Structure for compact history storage
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct CompactPayrollHistoryEntry {
    pub employee: Address,
    pub employer: Address,
    pub token: Address,
    pub amount: i128,
    pub interval: u32,
    pub recurrence_frequency: u32,
    pub timestamp: u64,
    pub last_payment_time: u64,
    pub next_payout_timestamp: u64,
    pub action: Symbol,
    pub id: u64,
}

/// Payroll template structure for reusable payroll configurations
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PayrollTemplate {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub employer: Address,
    pub token: Address,
    pub amount: i128,
    pub interval: u64,
    pub recurrence_frequency: u64,
    pub is_public: bool,
    pub created_at: u64,
    pub updated_at: u64,
    pub usage_count: u32,
}

/// Template preset structure for predefined configurations
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TemplatePreset {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub token: Address,
    pub amount: i128,
    pub interval: u64,
    pub recurrence_frequency: u64,
    pub category: String,
    pub is_active: bool,
    pub created_at: u64,
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

    // PayrollHistory
    PayrollHistoryEntry(Address),        // (employee) -> history_entry
    PayrollHistoryIdCounter(Address),    // (employee) -> history_entry
    AuditTrail(Address),                 // (employee) -> audit_entry
    AuditTrailIdCounter(Address),

  // Compliance-related storage keys
    JurisdictionConfig(Address),         // jurisdiction_hash -> JurisdictionConfig
    ComplianceMetrics(Address),          // jurisdiction_hash -> ComplianceMetrics
    RegulatoryReport(Address),           // report_id_hash -> RegulatoryReport
    AuditEntry(Address),                 // entry_id_hash -> AuditEntry
    AuditIndex(Address),                 // address -> Vec<Address> (audit entry ID hashes)
    ComplianceSettings,                  // Global compliance settings

    // Template and Preset storage keys
    PayrollTemplate(u64),                // template_id -> PayrollTemplate
    NextTemplateId,                      // Next available template ID
    EmployerTemplates(Address),          // employer -> Vec<u64> (template IDs)
    PublicTemplates,                     // Vec<u64> (public template IDs)
    TemplatePreset(u64),                 // preset_id -> TemplatePreset
    NextPresetId,                        // Next available preset ID
    PresetCategory(String),              // category -> Vec<u64> (preset IDs)
    ActivePresets,                       // Vec<u64> (active preset IDs)
}
