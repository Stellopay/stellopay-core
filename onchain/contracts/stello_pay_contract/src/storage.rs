use soroban_sdk::{contracttype, Address, Symbol, String, Vec, Map};

// Import insurance types for backup functionality
use crate::insurance::InsurancePolicy;

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

/// Payroll backup structure for data recovery
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PayrollBackup {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub employer: Address,
    pub created_at: u64,
    pub backup_type: BackupType,
    pub status: BackupStatus,
    pub checksum: String,
    pub data_hash: String,
    pub size_bytes: u64,
    pub version: u32,
}

/// Backup type enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum BackupType {
    Full,           // Complete system backup
    Employer,       // Employer-specific backup
    Employee,       // Employee-specific backup
    Template,       // Template backup
    Insurance,      // Insurance data backup
    Compliance,     // Compliance data backup
}

/// Backup status enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum BackupStatus {
    Creating,       // Backup is being created
    Completed,      // Backup completed successfully
    Failed,         // Backup failed
    Verifying,      // Backup is being verified
    Verified,       // Backup verified successfully
    Restoring,      // Backup is being restored
    Restored,       // Backup restored successfully
}

/// Backup data structure for storing actual backup content
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct BackupData {
    pub backup_id: u64,
    pub payroll_data: Vec<Payroll>,
    pub template_data: Vec<PayrollTemplate>,
    pub preset_data: Vec<TemplatePreset>,
    pub insurance_data: Vec<InsurancePolicy>,
    pub compliance_data: String, // Serialized compliance data as string
    pub metadata: BackupMetadata,
}

/// Backup metadata for additional information
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct BackupMetadata {
    pub total_employees: u32,
    pub total_templates: u32,
    pub total_presets: u32,
    pub total_insurance_policies: u32,
    pub backup_timestamp: u64,
    pub contract_version: String,
    pub data_integrity_hash: String,
}

/// Recovery point structure for disaster recovery
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct RecoveryPoint {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub created_at: u64,
    pub backup_id: u64,
    pub recovery_type: RecoveryType,
    pub status: RecoveryStatus,
    pub checksum: String,
    pub metadata: RecoveryMetadata,
}

/// Recovery type enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum RecoveryType {
    Full,           // Complete system recovery
    Partial,        // Partial system recovery
    Emergency,      // Emergency recovery
    Test,           // Test recovery
}

/// Recovery status enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum RecoveryStatus {
    Pending,        // Recovery pending
    InProgress,     // Recovery in progress
    Completed,      // Recovery completed
    Failed,         // Recovery failed
    RolledBack,     // Recovery rolled back
}

/// Recovery metadata for additional information
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct RecoveryMetadata {
    pub total_operations: u32,
    pub success_count: u32,
    pub failure_count: u32,
    pub recovery_timestamp: u64,
    pub duration_seconds: u64,
    pub data_verification_status: String,
}

/// Payroll schedule structure for automated disbursements
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PayrollSchedule {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub employer: Address,
    pub schedule_type: ScheduleType,
    pub frequency: ScheduleFrequency,
    pub start_date: u64,
    pub end_date: Option<u64>,
    pub next_execution: u64,
    pub is_active: bool,
    pub created_at: u64,
    pub updated_at: u64,
    pub execution_count: u32,
    pub last_execution: Option<u64>,
    pub metadata: ScheduleMetadata,
}

/// Schedule type enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ScheduleType {
    Recurring,      // Regular recurring payroll
    OneTime,        // One-time scheduled payroll
    Conditional,    // Conditional payroll based on triggers
    Batch,          // Batch payroll processing
    Emergency,      // Emergency payroll processing
}

/// Schedule frequency enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ScheduleFrequency {
    Daily,          // Daily execution
    Weekly,         // Weekly execution
    BiWeekly,       // Bi-weekly execution
    Monthly,        // Monthly execution
    Quarterly,      // Quarterly execution
    Yearly,         // Yearly execution
    Custom(u64),    // Custom frequency in seconds
}

/// Schedule metadata for additional information
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduleMetadata {
    pub total_employees: u32,
    pub total_amount: i128,
    pub token_address: Address,
    pub priority: u32,
    pub retry_count: u32,
    pub max_retries: u32,
    pub success_rate: u32, // Success rate as percentage (0-100)
    pub average_execution_time: u64,
}

/// Automation rule structure for conditional triggers
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AutomationRule {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub employer: Address,
    pub rule_type: RuleType,
    pub conditions: Vec<RuleCondition>,
    pub actions: Vec<RuleAction>,
    pub is_active: bool,
    pub created_at: u64,
    pub updated_at: u64,
    pub execution_count: u32,
    pub last_execution: Option<u64>,
    pub priority: u32,
}

/// Rule type enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum RuleType {
    Balance,        // Balance-based triggers
    Time,           // Time-based triggers
    Employee,       // Employee-based triggers
    Compliance,     // Compliance-based triggers
    Custom,         // Custom triggers
}

/// Rule condition structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct RuleCondition {
    pub field: String,
    pub operator: ConditionOperator,
    pub value: String,
    pub logical_operator: LogicalOperator,
}

/// Condition operator enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ConditionOperator {
    Equals,
    NotEquals,
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
    Contains,
    NotContains,
    IsEmpty,
    IsNotEmpty,
}

/// Logical operator enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum LogicalOperator {
    And,
    Or,
    Not,
}

/// Rule action structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct RuleAction {
    pub action_type: ActionType,
    pub parameters: Vec<String>,
    pub delay_seconds: u64,
    pub retry_count: u32,
}

/// Action type enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ActionType {
    DisburseSalary,
    PausePayroll,
    ResumePayroll,
    CreateBackup,
    SendNotification,
    UpdateSchedule,
    ExecuteRecovery,
    Custom,
}

/// Schedule execution record
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduleExecution {
    pub id: u64,
    pub schedule_id: u64,
    pub execution_time: u64,
    pub status: ExecutionStatus,
    pub result: ExecutionResult,
    pub duration: u64,
    pub error_message: Option<String>,
    pub metadata: ExecutionMetadata,
}

/// Rule execution record
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct RuleExecution {
    pub id: u64,
    pub rule_id: u64,
    pub execution_time: u64,
    pub status: ExecutionStatus,
    pub result: ExecutionResult,
    pub duration: u64,
    pub error_message: Option<String>,
    pub triggered_conditions: Vec<RuleCondition>,
    pub executed_actions: Vec<RuleAction>,
}

/// Execution status enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ExecutionStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Cancelled,
    Retrying,
}

/// Execution result enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ExecutionResult {
    Success,
    PartialSuccess,
    Failure,
    Skipped,
    Timeout,
}

/// Execution metadata
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ExecutionMetadata {
    pub total_operations: u32,
    pub success_count: u32,
    pub failure_count: u32,
    pub gas_used: u64,
    pub memory_used: u64,
}

//-----------------------------------------------------------------------------
// Security & Access Control Data Structures
//-----------------------------------------------------------------------------

/// User role enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum UserRole {
    Owner,
    Admin,
    Manager,
    Employee,
    Auditor,
    ComplianceOfficer,
    SecurityOfficer,
    Custom(String),
}

/// Permission enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum Permission {
    CreatePayroll,
    UpdatePayroll,
    DeletePayroll,
    DisburseSalary,
    PausePayroll,
    ResumePayroll,
    CreateTemplate,
    UpdateTemplate,
    ShareTemplate,
    CreateBackup,
    RestoreBackup,
    CreateSchedule,
    UpdateSchedule,
    ExecuteSchedule,
    CreateRule,
    UpdateRule,
    ExecuteRule,
    ViewAuditTrail,
    ManageRoles,
    ManageSecurity,
    EmergencyOperations,
    ComplianceReporting,
    InsuranceManagement,
    TokenManagement,
    BatchOperations,
    Custom(String),
}

/// Role-based access control structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Role {
    pub id: String,
    pub name: String,
    pub description: String,
    pub permissions: Vec<Permission>,
    pub is_active: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

/// User role assignment
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct UserRoleAssignment {
    pub user: Address,
    pub role: String,
    pub assigned_by: Address,
    pub assigned_at: u64,
    pub expires_at: Option<u64>,
    pub is_active: bool,
}

/// Security policy structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SecurityPolicy {
    pub id: String,
    pub name: String,
    pub description: String,
    pub policy_type: SecurityPolicyType,
    pub rules: Vec<SecurityRule>,
    pub is_active: bool,
    pub priority: u32,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Security policy type enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum SecurityPolicyType {
    AccessControl,
    RateLimiting,
    AuditLogging,
    DataProtection,
    Compliance,
    Emergency,
    Custom(String),
}

/// Security rule structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SecurityRule {
    pub field: String,
    pub operator: SecurityRuleOperator,
    pub value: String,
    pub action: SecurityRuleAction,
    pub priority: u32,
}

/// Security rule operator enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum SecurityRuleOperator {
    Equals,
    NotEquals,
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
    Contains,
    NotContains,
    In,
    NotIn,
    Regex,
    Custom(String),
}

/// Security rule action enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum SecurityRuleAction {
    Allow,
    Deny,
    RequireMFA,
    Log,
    Alert,
    Block,
    RateLimit,
    Custom(String),
}

/// Security audit log entry
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SecurityAuditEntry {
    pub entry_id: String,
    pub user: Address,
    pub action: String,
    pub resource: String,
    pub result: SecurityAuditResult,
    pub details: Map<String, String>,
    pub timestamp: u64,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub session_id: Option<String>,
}

/// Security audit result enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum SecurityAuditResult {
    Success,
    Failure,
    Denied,
    Suspicious,
    Blocked,
    RateLimited,
}

/// Rate limiting configuration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct RateLimitConfig {
    pub user: Address,
    pub operation: String,
    pub max_requests: u32,
    pub time_window: u64, // in seconds
    pub current_count: u32,
    pub reset_time: u64,
    pub is_blocked: bool,
    pub block_until: Option<u64>,
}

/// Security settings structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SecuritySettings {
    pub mfa_required: bool,
    pub session_timeout: u64, // in seconds
    pub max_login_attempts: u32,
    pub lockout_duration: u64, // in seconds
    pub ip_whitelist: Vec<String>,
    pub ip_blacklist: Vec<String>,
    pub audit_logging_enabled: bool,
    pub rate_limiting_enabled: bool,
    pub security_policies_enabled: bool,
    pub emergency_mode: bool,
    pub last_updated: u64,
}

/// Suspicious activity detection
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SuspiciousActivity {
    pub id: String,
    pub user: Address,
    pub activity_type: SuspiciousActivityType,
    pub severity: SuspiciousActivitySeverity,
    pub details: Map<String, String>,
    pub detected_at: u64,
    pub is_resolved: bool,
    pub resolved_at: Option<u64>,
    pub resolved_by: Option<Address>,
}

/// Suspicious activity type enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum SuspiciousActivityType {
    UnusualAccess,
    MultipleFailedLogins,
    UnauthorizedAccess,
    DataExfiltration,
    PrivilegeEscalation,
    RateLimitViolation,
    PolicyViolation,
    Custom(String),
}

/// Suspicious activity severity enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum SuspiciousActivitySeverity {
    Low,
    Medium,
    High,
    Critical,
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
    PayrollHistoryCounter(Address),      // (employee) -> history_entry
    AuditTrail(Address),                 // (employee) -> audit_entry

    // Webhook system keys - CORE FUNCTIONALITY
    Webhook(u64),                        // webhook_id -> Webhook
    NextWebhookId,                       // counter for webhook IDs
    
    // Audit and History - ESSENTIAL
    AuditIdCounter(Address),
    
    // Templates - MINIMAL SET
    NextTmplId,                          // Next available template ID
    Template(u64),                       // template_id -> PayrollTemplate
    EmpTemplates(Address),               // employer -> Vec<u64> (template IDs)  
    PubTemplates,                        // Vec<u64> (public template IDs)
    Preset(u64),                         // preset_id -> TemplatePreset
    NextPresetId,                        // Next available preset ID
    PresetCat(String),                   // category -> Vec<u64> (preset IDs)
    ActivePresets,                       // Vec<u64> (active preset IDs)

    // Backup - MINIMAL SET  
    Backup(u64),                         // backup_id -> PayrollBackup
    NextBackupId,                        // Next available backup ID
    EmpBackups(Address),                 // employer -> Vec<u64> (backup IDs)
    BackupData(u64),                     // backup_id -> BackupData
    BackupIndex,                         // Vec<u64> (all backup IDs)
    Recovery(u64),                       // recovery_point_id -> RecoveryPoint
    NextRecoveryId,                      // Next available recovery point ID

    // Scheduling - MINIMAL SET
    Schedule(u64),                       // schedule_id -> PayrollSchedule
    NextSchedId,                         // Next available schedule ID
    EmpSchedules(Address),               // employer -> Vec<u64> (schedule IDs)
    Rule(u64),                           // rule_id -> AutomationRule
    NextRuleId,                          // Next available rule ID
    EmpRules(Address),                   // employer -> Vec<u64> (rule IDs)

    // Security - MINIMAL SET 
    Role(String),                        // role_id -> Role
    UserRole(Address),                   // user -> UserRoleAssignment
    SecuritySettings                     // Global security settings
}
