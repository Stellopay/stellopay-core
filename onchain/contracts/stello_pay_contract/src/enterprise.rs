use soroban_sdk::{contracttype, Address, Symbol, String, Vec, Map};

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
} 