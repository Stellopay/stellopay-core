use soroban_sdk::{contracttype, Address, Env, Map, String, Symbol, Vec};

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

// -----------------------------------------------------------------------------
// Multi-currency hedging & taxation data structures (top-level)
// -----------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct HedgeSettings {
    pub enabled: bool,
    pub target_coverage_bps: u32, // e.g., 5000 = 50%
    pub max_tenor_days: u32,
    pub rebalance_threshold_bps: u32,
    pub last_updated: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum HedgeSide {
    Buy,
    Sell,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct HedgingPosition {
    pub id: u64,
    pub employer: Address,
    pub base_token: Address,
    pub quote_token: Address,
    pub side: HedgeSide,
    pub notional: i128,
    pub opened_at: u64,
    pub expires_at: Option<u64>,
    pub is_open: bool,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TaxBracket {
    pub up_to: i128,
    pub rate_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TaxConfig {
    pub jurisdiction: crate::compliance::Jurisdiction,
    pub brackets: Vec<TaxBracket>,
    pub flat_withholding_bps: u32,
    pub allowance: i128,
    pub last_updated: u64,
}

#[contracttype]
pub enum FxExtendedKey {
    EmployerHedgeSettings(Address),
    HedgePosition(u64),
    NextHedgeId,
    EmployerHedges(Address),
    TaxConfig(crate::compliance::Jurisdiction),
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

/// Structure to store performance metrics
#[derive(Clone)]
#[contracttype]
pub struct PerformanceMetrics {
    pub total_disbursements: u64,
    pub total_amount: i128,
    pub operation_count: u64,
    pub timestamp: u64,
    pub employee_count: u32,
    pub operation_type_counts: Map<Symbol, u64>,
    pub late_disbursements: u64,
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
    Full,       // Complete system backup
    Employer,   // Employer-specific backup
    Employee,   // Employee-specific backup
    Template,   // Template backup
    Insurance,  // Insurance data backup
    Compliance, // Compliance data backup
}

/// Backup status enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum BackupStatus {
    Creating,  // Backup is being created
    Completed, // Backup completed successfully
    Failed,    // Backup failed
    Verifying, // Backup is being verified
    Verified,  // Backup verified successfully
    Restoring, // Backup is being restored
    Restored,  // Backup restored successfully
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
    Full,      // Complete system recovery
    Partial,   // Partial system recovery
    Emergency, // Emergency recovery
    Test,      // Test recovery
}

/// Recovery status enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum RecoveryStatus {
    Pending,    // Recovery pending
    InProgress, // Recovery in progress
    Completed,  // Recovery completed
    Failed,     // Recovery failed
    RolledBack, // Recovery rolled back
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
    Recurring,   // Regular recurring payroll
    OneTime,     // One-time scheduled payroll
    Conditional, // Conditional payroll based on triggers
    Batch,       // Batch payroll processing
    Emergency,   // Emergency payroll processing
}

/// Schedule frequency enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ScheduleFrequency {
    Daily,       // Daily execution
    Weekly,      // Weekly execution
    BiWeekly,    // Bi-weekly execution
    Monthly,     // Monthly execution
    Quarterly,   // Quarterly execution
    Yearly,      // Yearly execution
    Custom(u64), // Custom frequency in seconds
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
    Balance,    // Balance-based triggers
    Time,       // Time-based triggers
    Employee,   // Employee-based triggers
    Compliance, // Compliance-based triggers
    Custom,     // Custom triggers
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

// Role delegation record: from -> to for a role, optional expiry
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct RoleDelegation {
    pub id: u64,
    pub role_id: String,
    pub from: Address,
    pub to: Address,
    pub delegated_at: u64,
    pub expires_at: Option<u64>,
    pub accepted: bool,
}

// Temporary role assignment applied by admin/employer
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TempRoleAssignment {
    pub id: u64,
    pub role_id: String,
    pub user: Address,
    pub assigned_by: Address,
    pub assigned_at: u64,
    pub expires_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PermissionAuditEntry {
    pub id: u64,
    pub actor: Address,     // who triggered the action/check
    pub subject: Address,   // user whose permissions were checked/changed
    pub permission: String, // permission name
    pub action: String,     // "check", "assign", "revoke", "delegate", ...
    pub result: String,     // "allowed" / "denied" / "granted" / "revoked"
    pub timestamp: u64,
    pub details: String, // optional JSON or text
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct UserRolesResponse {
    pub direct_roles: Vec<String>,
    pub temp_roles: Vec<TempRoleAssignment>,
    pub delegated_roles: Vec<RoleDelegation>,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct RoleDetails {
    pub role: Role,
    pub parent_role: Option<String>,
    pub members: Vec<Address>,
    pub all_permissions: Vec<Permission>,
}

//-----------------------------------------------------------------------------
// Storage Keys
//-----------------------------------------------------------------------------

// Core DataKey enum - essential functionality only
#[contracttype]
pub enum DataKey {
    // Consolidated payroll storage - single key per employee
    Payroll(Address), // employee -> Payroll struct

    // Indexing for efficient queries
    EmployerEmployees(Address), // employer -> Vec<Employee>
    TokenEmployees(Address),    // token -> Vec<Employee>

    // Employer balance, keyed by (employer, token)
    Balance(Address, Address),

    // Metrics storage - daily aggregated metrics
    Metrics(u64), // timestamp -> PerformanceMetrics

    // Unique employee tracking
    Employee(Address), // employee -> bool

    // Admin
    Owner,
    Paused,

    SupportedToken(Address),
    TokenMetadata(Address),

    // Insurance-related storage keys
    InsurancePolicy(Address),    // employee -> InsurancePolicy
    InsuranceClaim(u64),         // claim_id -> InsuranceClaim
    NextClaimId,                 // Next available claim ID
    InsurancePool(Address),      // token -> InsurancePool
    GuaranteeFund(Address),      // token -> GuaranteeFund
    Guarantee(u64),              // guarantee_id -> Guarantee
    NextGuaranteeId,             // Next available guarantee ID
    EmployerGuarantees(Address), // employer -> Vec<u64> (guarantee IDs)
    RiskAssessment(Address),     // employee -> u32 (risk score)
    InsuranceSettings,           // Global insurance settings

    // PayrollHistory
    PayrollHistoryEntry(Address),   // (employee) -> history_entry
    PayrollHistoryCounter(Address), // (employee) -> history_entry
    AuditTrail(Address),            // (employee) -> audit_entry

    // Webhook system keys - CORE FUNCTIONALITY
    Webhook(u64),           // webhook_id -> Webhook
    NextWebhookId,          // counter for webhook IDs
    NextWebhookAttemptId,   // counter for webhook attempt IDs
    OwnerWebhooks(Address), // owner -> Vec<u64> (webhook IDs)
    WebhookRateLimit(u64),  // webhook_id -> last_request_timestamp

    // Audit and History - ESSENTIAL
    AuditIdCounter(Address),

    // Security - MINIMAL SET
    SecuritySettings, // Global security settings

    // -----------------------------
    // Multi-currency & FX (used by token_swap.rs)
    // -----------------------------
    /// Token pair configuration key. Uses a deterministic Address key derived by TokenSwapSystem
    TokenPair(Address),
    /// Conversion rate for a token pair. Key must match TokenPair(Address)
    ConversionRate(Address),
    /// Global swap fee configuration
    SwapFee,
    /// Swap request/result/history
    SwapRequest(String),
    SwapResult(String),
    SwapHistoryEntry(String),
    SwapHistoryIndex(Address),
    /// Global token swap settings
    TokenSwapSettings,

    // -----------------------------
    // International Compliance & Reporting (used by compliance.rs)
    // -----------------------------
    /// Global compliance settings
    ComplianceSettings,
    /// Metrics per jurisdiction
    ComplianceMetrics(crate::compliance::Jurisdiction),
    /// Jurisdiction configuration
    JurisdictionConfig(crate::compliance::Jurisdiction),
    /// Audit trail storage
    AuditEntry(String),
    AuditIndex(Address),
    /// Regulatory report storage
    RegulatoryReport(String),

    // Employee region/jurisdiction mapping
    EmployeeJurisdiction(Address),
}

// Extended functionality keys - separate enum to avoid size limits
#[contracttype]
pub enum ExtendedDataKey {
    // Templates - MINIMAL SET
    NextTmplId,            // Next available template ID
    Template(u64),         // template_id -> PayrollTemplate
    EmpTemplates(Address), // employer -> Vec<u64> (template IDs)
    PubTemplates,          // Vec<u64> (public template IDs)
    Preset(u64),           // preset_id -> TemplatePreset
    NextPresetId,          // Next available preset ID
    PresetCat(String),     // category -> Vec<u64> (preset IDs)
    ActivePresets,         // Vec<u64> (active preset IDs)

    // Backup - MINIMAL SET
    Backup(u64),         // backup_id -> PayrollBackup
    NextBackupId,        // Next available backup ID
    EmpBackups(Address), // employer -> Vec<u64> (backup IDs)
    BackupData(u64),     // backup_id -> BackupData
    BackupIndex,         // Vec<u64> (all backup IDs)
    Recovery(u64),       // recovery_point_id -> RecoveryPoint
    NextRecoveryId,      // Next available recovery point ID

    // Scheduling - MINIMAL SET
    Schedule(u64),         // schedule_id -> PayrollSchedule
    NextSchedId,           // Next available schedule ID
    EmpSchedules(Address), // employer -> Vec<u64> (schedule IDs)
    Rule(u64),             // rule_id -> AutomationRule
    NextRuleId,            // Next available rule ID
    EmpRules(Address),     // employer -> Vec<u64> (rule IDs)
}

#[contracttype]
pub enum RoleDataKey {
    // --- RBAC core ---
    Role(String),        // role_id -> Role
    RoleMembers(String), // role_id -> Vec<Address>
    RoleParent(String),  // role_id -> Option<String>
    UserRole(Address),   // user -> Vec<String> (assigned role ids)

    // --- Delegation ---
    Delegation(u64),  // delegation_id -> RoleDelegation
    NextDelegationId, // counter for delegation ids

    // --- Temporary role assignments ---
    TempRole(u64),  // temp_role_id -> TempRoleAssignment
    NextTempRoleId, // counter for temp assignments

    // --- Auditing ---
    Audit(u64),  // audit_id -> PermissionAuditEntry
    NextAuditId, // counter for audits
}

//-----------------------------------------------------------------------------
// Employee Lifecycle Management Data Structures
//-----------------------------------------------------------------------------

/// Employee status enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum EmployeeStatus {
    Pending,    // Onboarding in progress
    Active,     // Active employee
    Inactive,   // Temporarily inactive
    Terminated, // Terminated employee
    OnLeave,    // On leave
    Suspended,  // Suspended
}

/// Employee profile structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct EmployeeProfile {
    pub employee: Address,
    pub employer: Address,
    pub department_id: Option<u64>,
    pub status: EmployeeStatus,
    pub hire_date: u64,
    pub termination_date: Option<u64>,
    pub job_title: String,
    pub employee_id: String,
    pub manager: Option<Address>,
    pub created_at: u64,
    pub updated_at: u64,
    pub metadata: Map<String, String>,
}

/// Onboarding workflow structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct OnboardingWorkflow {
    pub id: u64,
    pub employee: Address,
    pub employer: Address,
    pub status: WorkflowStatus,
    pub checklist: Vec<OnboardingTask>,
    pub approvals: Vec<WorkflowApproval>,
    pub created_at: u64,
    pub completed_at: Option<u64>,
    pub expires_at: u64,
}

/// Offboarding workflow structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct OffboardingWorkflow {
    pub id: u64,
    pub employee: Address,
    pub employer: Address,
    pub status: WorkflowStatus,
    pub checklist: Vec<OffboardingTask>,
    pub has_final_payment: bool,
    pub approvals: Vec<WorkflowApproval>,
    pub created_at: u64,
    pub completed_at: Option<u64>,
    pub termination_reason: String,
}

/// Workflow status enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum WorkflowStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
    Expired,
}

/// Onboarding task structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct OnboardingTask {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub required: bool,
    pub completed: bool,
    pub completed_at: Option<u64>,
    pub completed_by: Option<Address>,
    pub due_date: Option<u64>,
}

/// Offboarding task structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct OffboardingTask {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub required: bool,
    pub completed: bool,
    pub completed_at: Option<u64>,
    pub completed_by: Option<Address>,
    pub due_date: Option<u64>,
}

/// Workflow approval structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct WorkflowApproval {
    pub approver: Address,
    pub approved: bool,
    pub comment: String,
    pub timestamp: u64,
    pub required: bool,
}

/// Final payment structure for offboarding
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct FinalPayment {
    pub amount: i128,
    pub token: Address,
    pub includes_severance: bool,
    pub includes_unused_leave: bool,
    pub processed: bool,
    pub processed_at: Option<u64>,
}

/// Employee transfer structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct EmployeeTransfer {
    pub id: u64,
    pub employee: Address,
    pub from_department: u64,
    pub to_department: u64,
    pub from_manager: Address,
    pub to_manager: Address,
    pub transfer_date: u64,
    pub reason: String,
    pub approved: bool,
    pub approved_by: Option<Address>,
    pub approved_at: Option<u64>,
    pub created_at: u64,
}

/// Compliance tracking structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ComplianceRecord {
    pub employee: Address,
    pub compliance_type: String,
    pub status: ComplianceStatus,
    pub due_date: u64,
    pub completed_date: Option<u64>,
    pub notes: String,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Compliance status enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ComplianceStatus {
    Pending,
    Completed,
    Overdue,
    NotRequired,
}

//-----------------------------------------------------------------------------
// Lifecycle Storage Helper - Clever workaround for DataKey size limit
//-----------------------------------------------------------------------------

/// Lifecycle storage helper that uses existing DataKey variants with prefixed strings
/// This avoids adding new variants to the DataKey enum which is already at size limit
pub struct LifecycleStorage;

impl LifecycleStorage {
    // Storage prefixes for different lifecycle data types
    const PROFILE_PREFIX: &'static str = "lc_profile_";
    const ONBOARDING_PREFIX: &'static str = "lc_onboard_";
    const OFFBOARDING_PREFIX: &'static str = "lc_offboard_";
    const TRANSFER_PREFIX: &'static str = "lc_transfer_";
    const COMPLIANCE_PREFIX: &'static str = "lc_comply_";
    const COUNTER_PREFIX: &'static str = "lc_counter_";

    // methods continue below

    /// Store compliance record using AuditTrail key
    pub fn store_compliance(
        env: &Env,
        employee: &Address,
        _compliance_type: &String,
        record: &ComplianceRecord,
    ) {
        // Use employee address for compliance records
        env.storage()
            .persistent()
            .set(&DataKey::AuditTrail(employee.clone()), record);
    }

    /// Get compliance record
    pub fn get_compliance(
        env: &Env,
        employee: &Address,
        _compliance_type: &String,
    ) -> Option<ComplianceRecord> {
        env.storage()
            .persistent()
            .get(&DataKey::AuditTrail(employee.clone()))
    }

    /// Store/get counters using simple numeric keys
    pub fn get_next_onboarding_id(env: &Env) -> u64 {
        let current_id: u64 = env
            .storage()
            .persistent()
            .get(&ExtendedDataKey::NextTmplId)
            .unwrap_or(1);
        env.storage()
            .persistent()
            .set(&ExtendedDataKey::NextTmplId, &(current_id + 1));
        current_id
    }

    pub fn get_next_offboarding_id(env: &Env) -> u64 {
        let current_id: u64 = env
            .storage()
            .persistent()
            .get(&ExtendedDataKey::NextPresetId)
            .unwrap_or(1);
        env.storage()
            .persistent()
            .set(&ExtendedDataKey::NextPresetId, &(current_id + 1));
        current_id
    }

    pub fn get_next_transfer_id(env: &Env) -> u64 {
        let current_id: u64 = env
            .storage()
            .persistent()
            .get(&ExtendedDataKey::NextBackupId)
            .unwrap_or(1);
        env.storage()
            .persistent()
            .set(&ExtendedDataKey::NextBackupId, &(current_id + 1));
        current_id
    }

    /// Link employee to workflow ID using existing keys
    pub fn link_employee_onboarding(env: &Env, employee: &Address, workflow_id: u64) {
        // Store workflow ID in the employee's balance slot with a special token
        let link_token = Address::from_string(&String::from_str(env, "ONBOARD"));
        env.storage().persistent().set(
            &DataKey::Balance(employee.clone(), link_token),
            &(workflow_id as i128),
        );
    }

    pub fn get_employee_onboarding_id(env: &Env, employee: &Address) -> Option<u64> {
        let link_token = Address::from_string(&String::from_str(env, "ONBOARD"));
        env.storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::Balance(employee.clone(), link_token))
            .map(|id| id as u64)
    }

    pub fn link_employee_offboarding(env: &Env, employee: &Address, workflow_id: u64) {
        let link_token = Address::from_string(&String::from_str(env, "OFFBOARD"));
        env.storage().persistent().set(
            &DataKey::Balance(employee.clone(), link_token),
            &(workflow_id as i128),
        );
    }

    pub fn get_employee_offboarding_id(env: &Env, employee: &Address) -> Option<u64> {
        let link_token = Address::from_string(&String::from_str(env, "OFFBOARD"));
        env.storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::Balance(employee.clone(), link_token))
            .map(|id| id as u64)
    }

    /// Store final payment separately to avoid serialization issues
    pub fn store_final_payment(env: &Env, employee: &Address, final_payment: &FinalPayment) {
        let payment_key = Address::from_string(&String::from_str(env, "FINALPAY"));
        env.storage().persistent().set(
            &DataKey::Balance(employee.clone(), payment_key),
            final_payment,
        );
    }

    /// Get final payment
    pub fn get_final_payment(env: &Env, employee: &Address) -> Option<FinalPayment> {
        let payment_key = Address::from_string(&String::from_str(env, "FINALPAY"));
        env.storage()
            .persistent()
            .get(&DataKey::Balance(employee.clone(), payment_key))
    }
}

impl LifecycleStorage {
    /// Store and get employee profile via special balance slot key
    pub fn store_profile(env: &Env, employee: &Address, profile: &EmployeeProfile) {
        let k = Address::from_string(&String::from_str(env, "PROFILE"));
        env.storage().persistent().set(&DataKey::Balance(employee.clone(), k), profile);
    }

    pub fn get_profile(env: &Env, employee: &Address) -> Option<EmployeeProfile> {
        let k = Address::from_string(&String::from_str(env, "PROFILE"));
        env.storage().persistent().get(&DataKey::Balance(employee.clone(), k))
    }

    /// Onboarding workflows stored under ExtendedDataKey::Rule(workflow_id)
    pub fn store_onboarding(env: &Env, workflow_id: u64, wf: &OnboardingWorkflow) {
        env.storage().persistent().set(&ExtendedDataKey::Rule(workflow_id), wf);
    }

    pub fn get_onboarding(env: &Env, workflow_id: u64) -> Option<OnboardingWorkflow> {
        env.storage().persistent().get(&ExtendedDataKey::Rule(workflow_id))
    }

    /// Offboarding workflows stored under ExtendedDataKey::Recovery(workflow_id)
    pub fn store_offboarding(env: &Env, workflow_id: u64, wf: &OffboardingWorkflow) {
        env.storage().persistent().set(&ExtendedDataKey::Recovery(workflow_id), wf);
    }

    pub fn get_offboarding(env: &Env, workflow_id: u64) -> Option<OffboardingWorkflow> {
        env.storage().persistent().get(&ExtendedDataKey::Recovery(workflow_id))
    }

    /// Employee transfers stored under ExtendedDataKey::Preset(transfer_id)
    pub fn store_transfer(env: &Env, transfer_id: u64, tr: &EmployeeTransfer) {
        env.storage().persistent().set(&ExtendedDataKey::Preset(transfer_id), tr);
    }
}
