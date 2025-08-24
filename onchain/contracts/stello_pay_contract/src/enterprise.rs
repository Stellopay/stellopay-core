use soroban_sdk::{contracttype, Address, Symbol, String, Vec, Map, Env};

//-----------------------------------------------------------------------------
// Enterprise Features Data Structures
//-----------------------------------------------------------------------------

/// Department structure for organizational hierarchy
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Department {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub employer: Address,
    pub manager: Address,
    pub parent_department: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
    pub is_active: bool,
}

/// Approval workflow structure for multi-step approvals
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ApprovalWorkflow {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub employer: Address,
    pub steps: Vec<ApprovalStep>,
    pub created_at: u64,
    pub updated_at: u64,
    pub is_active: bool,
}

/// Individual approval step in a workflow
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ApprovalStep {
    pub step_number: u32,
    pub approver_role: String,
    pub timeout_hours: u32,
    pub is_required: bool,
}

/// Pending approval structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PendingApproval {
    pub id: u64,
    pub workflow_id: u64,
    pub request_type: String,
    pub request_data: String,
    pub requester: Address,
    pub current_step: u32,
    pub approvals: Vec<Approval>,
    pub created_at: u64,
    pub expires_at: u64,
    pub status: ApprovalStatus,
}

/// Individual approval record
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Approval {
    pub approver: Address,
    pub step_number: u32,
    pub approved: bool,
    pub comment: String,
    pub timestamp: u64,
}

impl Approval {
    pub fn default(env: &Env) -> Self {
        Self {
            approver: Address::from_str(env, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
            step_number: 0,
            approved: false,
            comment: String::from_str(env, ""),
            timestamp: 0,
        }
    }
}
/// Approval status enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
}

/// Webhook endpoint for external integrations
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct WebhookEndpoint {
    pub id: String,
    pub name: String,
    pub url: String,
    pub employer: Address,
    pub events: Vec<String>,
    pub headers: Map<String, String>,
    pub is_active: bool,
    pub created_at: u64,
    pub last_triggered: Option<u64>,
}

/// Report template for analytics
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ReportTemplate {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub employer: Address,
    pub query_parameters: Map<String, String>,
    pub schedule: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub is_active: bool,
}

/// Backup schedule for automated backups
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct BackupSchedule {
    pub id: u64,
    pub name: String,
    pub employer: Address,
    pub frequency: String, // "daily", "weekly", "monthly"
    pub retention_days: u32,
    pub is_active: bool,
    pub created_at: u64,
    pub last_backup: Option<u64>,
}

/// Payroll modification request structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PayrollModificationRequest {
    pub id: u64,
    pub employee: Address,
    pub employer: Address,
    pub request_type: PayrollModificationType,
    pub current_value: String,
    pub proposed_value: String,
    pub reason: String,
    pub requester: Address,
    pub employer_approval: Approval,
    pub employee_approval: Approval,
    pub created_at: u64,
    pub expires_at: u64,
    pub status: PayrollModificationStatus,
}

/// Payroll modification type enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum PayrollModificationType {
    Salary,
    Interval,
    RecurrenceFrequency,
    Token,
    Custom(String),
}

/// Payroll modification status enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum PayrollModificationStatus {
    Pending,
    EmployerApproved,
    EmployeeApproved,
    BothApproved,
    Rejected,
    Expired,
    Cancelled,
}

//-----------------------------------------------------------------------------
// Dispute Resolution Data Structures
//-----------------------------------------------------------------------------

/// Dispute type enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum DisputeType {
    SalaryDiscrepancy,
    PaymentDelay,
    IncorrectAmount,
    MissingPayment,
    ContractViolation,
    UnauthorizedDeduction,
    WrongToken,
    RecurrenceIssue,
    PauseDispute,
    TerminationDispute,
    Custom(String),
}

/// Dispute status enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum DisputeStatus {
    Open,
    UnderReview,
    Escalated,
    Mediation,
    Resolved,
    Closed,
    Expired,
}

/// Dispute priority levels
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum DisputePriority {
    Low,
    Medium,
    High,
    Critical,
}

/// Dispute structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Dispute {
    pub id: u64,
    pub employee: Address,
    pub employer: Address,
    pub dispute_type: DisputeType,
    pub description: String,
    pub evidence: Vec<String>, // Evidence documents/descriptions
    pub amount_involved: Option<i128>,
    pub token_involved: Option<Address>,
    pub priority: DisputePriority,
    pub status: DisputeStatus,
    pub created_at: u64,
    pub updated_at: u64,
    pub expires_at: u64,
    pub resolved_at: Option<u64>,
    pub resolution: Option<String>,
    pub resolution_by: Option<Address>,
}

/// Escalation level enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum EscalationLevel {
    Level1, // Direct resolution attempt
    Level2, // Supervisor/Manager review
    Level3, // HR/Compliance review
    Level4, // Legal/External mediation
    Level5, // Arbitration/Court
}

/// Escalation structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Escalation {
    pub id: u64,
    pub dispute_id: u64,
    pub level: EscalationLevel,
    pub reason: String,
    pub escalated_by: Address,
    pub escalated_at: u64,
    pub mediator: Option<Address>,
    pub mediator_assigned_at: Option<u64>,
    pub resolution: Option<String>,
    pub resolved_at: Option<u64>,
    pub resolved_by: Option<Address>,
    pub timeout_at: u64,
}

/// Mediator structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Mediator {
    pub address: Address,
    pub name: String,
    pub specialization: Vec<String>,
    pub success_rate: u32, // Percentage
    pub total_cases: u32,
    pub resolved_cases: u32,
    pub is_active: bool,
    pub created_at: u64,
    pub last_active: u64,
}

/// Dispute settings structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct DisputeSettings {
    pub auto_escalation_days: u32,
    pub mediation_timeout: u32,
    pub arbitration_timeout: u32,
    pub max_escalation_levels: u32,
    pub evidence_required: bool,
    pub min_evidence_count: u32,
    pub dispute_timeout: u32,
    pub escalation_cooldown: u32,
}

//-----------------------------------------------------------------------------
// Enterprise Storage Keys
//-----------------------------------------------------------------------------

#[contracttype]
pub enum EnterpriseDataKey {
    // Department management
    Department(u64),                     // department_id -> Department
    NextDepartmentId,                    // Next available department ID
    EmployerDepartments(Address),        // employer -> Vec<u64> (department IDs)
    EmployeeDepartment(Address),         // employee -> department_id
    
    // Approval workflows
    ApprovalWorkflow(u64),               // workflow_id -> ApprovalWorkflow
    NextWorkflowId,                      // Next available workflow ID
    PendingApproval(u64),                // approval_id -> PendingApproval
    NextApprovalId,                      // Next available approval ID
    
    // Integration capabilities
    WebhookEndpoint(String),             // endpoint_id -> WebhookEndpoint
    NextWebhookId,                       // Next available webhook ID
    EmployerWebhooks(Address),           // employer -> Vec<String> (webhook IDs)
    
    // Reporting
    ReportTemplate(u64),                 // report_id -> ReportTemplate
    NextReportId,                        // Next available report ID
    EmployerReports(Address),            // employer -> Vec<u64> (report IDs)
    
    // Backup & Recovery
    BackupSchedule(u64),                 // schedule_id -> BackupSchedule
    NextBackupScheduleId,                // Next available backup schedule ID
    EmployerBackupSchedules(Address),    // employer -> Vec<u64> (backup schedule IDs)
    
    // Payroll Modification Approval System
    PayrollModificationRequest(u64),     // request_id -> PayrollModificationRequest
    NextModificationRequestId,           // Next available modification request ID
    EmployeeModificationRequests(Address), // employee -> Vec<u64> (request IDs)
    EmployerModificationRequests(Address), // employer -> Vec<u64> (request IDs)
    PendingModificationRequests,         // Vec<u64> (pending request IDs)
    
    // Dispute Resolution System
    Dispute(u64),                        // dispute_id -> Dispute
    NextDisputeId,                       // Next available dispute ID
    EmployeeDisputes(Address),           // employee -> Vec<u64> (dispute IDs)
    EmployerDisputes(Address),           // employer -> Vec<u64> (dispute IDs)
    OpenDisputes,                        // Vec<u64> (open dispute IDs)
    EscalatedDisputes,                   // Vec<u64> (escalated dispute IDs)
    Escalation(u64),                     // escalation_id -> Escalation
    NextEscalationId,                    // Next available escalation ID
    DisputeEscalations(u64),             // dispute_id -> Vec<u64> (escalation IDs)
    MediatorEscalations(Address),        // mediator -> Vec<u64> (escalation IDs)
    Mediator(Address),                   // mediator_address -> Mediator
    ActiveMediators,                     // Vec<Address> (active mediator addresses)
    MediatorBySpecialization(String),   // specialization -> Vec<Address> (mediator addresses)
    DisputeSettings,                     // Global dispute settings
}

//-----------------------------------------------------------------------------
// Enterprise Errors
//-----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum EnterpriseError {
    DepartmentNotFound,
    WorkflowNotFound,
    ApprovalNotFound,
    WebhookNotFound,
    ReportNotFound,
    BackupScheduleNotFound,
    InvalidDepartmentHierarchy,
    WorkflowStepNotFound,
    ApprovalExpired,
    InsufficientPermissions,
    InvalidWebhookUrl,
    ReportGenerationFailed,
    BackupScheduleConflict,
    ModificationRequestNotFound,
    ModificationRequestExpired,
    ModificationRequestAlreadyApproved,
    ModificationRequestAlreadyRejected,
    InvalidModificationType,
    InvalidModificationValues,
    ModificationTimeoutInvalid,
    DisputeNotFound,
    DisputeAlreadyResolved,
    DisputeExpired,
    EscalationNotFound,
    EscalationExpired,
    MediatorNotFound,
    MediatorNotActive,
    InvalidDisputeType,
    InvalidDisputePriority,
    InsufficientEvidence,
    DisputeTimeoutInvalid,
    EscalationLevelInvalid,
} 