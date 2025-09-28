//-----------------------------------------------------------------------------
// Events
//-----------------------------------------------------------------------------

use soroban_sdk::{contracttype, symbol_short, Address, Env, String, Symbol};

/// Event emitted when contract is paused
pub const PAUSED_EVENT: Symbol = symbol_short!("paused");

/// Event emitted when contract is unpaused
pub const UNPAUSED_EVENT: Symbol = symbol_short!("unpaused");

pub const DEPOSIT_EVENT: Symbol = symbol_short!("deposit");

/// Event emitted when an individual employee's payroll is paused
pub const EMPLOYEE_PAUSED_EVENT: Symbol = symbol_short!("emppaused");

/// Event emitted when an individual employee's payroll is resumed
pub const EMPLOYEE_RESUMED_EVENT: Symbol = symbol_short!("empresume");

/// Event emitted when performance metrics are updated
pub const METRICS_UPDATED_EVENT: Symbol = symbol_short!("metricupd");

// Insurance-related events
pub const INS_POLICY_CREATED: Symbol = symbol_short!("ins_pol_c");
pub const INS_POLICY_UPDATED: Symbol = symbol_short!("ins_pol_u");
pub const INS_CLAIM_FILED: Symbol = symbol_short!("ins_clm_f");
pub const INS_CLAIM_APPROVED: Symbol = symbol_short!("ins_clm_a");
pub const INS_CLAIM_PAID: Symbol = symbol_short!("ins_clm_p");
pub const PREMIUM_PAID: Symbol = symbol_short!("prem_pai");
pub const GUAR_ISSUED: Symbol = symbol_short!("guar_iss");
pub const GUAR_REPAID: Symbol = symbol_short!("guar_rep");
pub const POOL_FUNDED: Symbol = symbol_short!("pool_fun");

// Template and Preset Events
pub const TEMPLATE_CREATED_EVENT: Symbol = symbol_short!("tmpl_crt");
pub const TEMPLATE_UPDATED_EVENT: Symbol = symbol_short!("tmpl_upd");
pub const TEMPLATE_APPLIED_EVENT: Symbol = symbol_short!("tmpl_app");
pub const TEMPLATE_SHARED_EVENT: Symbol = symbol_short!("tmpl_shr");
pub const PRESET_CREATED_EVENT: Symbol = symbol_short!("prst_crt");

// Backup and Recovery Events
pub const BACKUP_CREATED_EVENT: Symbol = symbol_short!("backup_c");
pub const BACKUP_VERIFIED_EVENT: Symbol = symbol_short!("backup_v");
pub const RECOVERY_STARTED_EVENT: Symbol = symbol_short!("rcvry_st");
pub const RECOVERY_COMPLETED_EVENT: Symbol = symbol_short!("rcvry_cp");

// Scheduling and Automation Events
pub const SCHEDULE_CREATED_EVENT: Symbol = symbol_short!("sched_c");
pub const SCHEDULE_UPDATED_EVENT: Symbol = symbol_short!("sched_u");
pub const SCHEDULE_EXECUTED_EVENT: Symbol = symbol_short!("sched_e");
pub const RULE_CREATED_EVENT: Symbol = symbol_short!("rule_c");
pub const RULE_EXECUTED_EVENT: Symbol = symbol_short!("rule_e");

// Security Events
pub const ROLE_ASSIGNED_EVENT: Symbol = symbol_short!("role_a");
pub const ROLE_REVOKED_EVENT: Symbol = symbol_short!("role_r");
pub const SECURITY_AUDIT_EVENT: Symbol = symbol_short!("sec_aud");
pub const SECURITY_POLICY_VIOLATION_EVENT: Symbol = symbol_short!("sec_viol");

// Employee Lifecycle Events
pub const EMPLOYEE_ONBOARDED: Symbol = symbol_short!("emp_onb");
pub const EMPLOYEE_OFFBOARDED: Symbol = symbol_short!("emp_off");
pub const EMPLOYEE_TRANSFERRED: Symbol = symbol_short!("emp_trf");
pub const EMPLOYEE_STATUS_CHANGED: Symbol = symbol_short!("emp_sts");
pub const ONBOARDING_STARTED: Symbol = symbol_short!("onb_str");
pub const ONBOARDING_COMPLETED: Symbol = symbol_short!("onb_cmp");
pub const OFFBOARDING_STARTED: Symbol = symbol_short!("off_str");
pub const OFFBOARDING_COMPLETED: Symbol = symbol_short!("off_cmp");
pub const FINAL_PAYMENT_PROCESSED: Symbol = symbol_short!("fin_pay");
pub const COMPLIANCE_UPDATED: Symbol = symbol_short!("cmp_upd");
pub const WORKFLOW_APPROVED: Symbol = symbol_short!("wf_app");
pub const TASK_COMPLETED: Symbol = symbol_short!("tsk_cmp");

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SalaryDisbursed {
    pub employer: Address,
    pub employee: Address,
    pub token: Address,
    pub amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmployerWithdrawn {
    pub employer: Address,
    pub token: Address,
    pub amount: i128,
    pub timestamp: u64,
}

// Insurance event structures
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsurancePolicyCreated {
    pub employer: Address,
    pub employee: Address,
    pub coverage_amount: i128,
    pub premium_amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsuranceClaimFiled {
    pub employee: Address,
    pub claim_id: u64,
    pub claim_amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsuranceClaimPaid {
    pub claim_id: u64,
    pub employee: Address,
    pub amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuaranteeIssued {
    pub employer: Address,
    pub guarantee_id: u64,
    pub guarantee_amount: i128,
    pub timestamp: u64,
}

// Employee Lifecycle Event Structures
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmployeeOnboarded {
    pub employee: Address,
    pub employer: Address,
    pub department_id: Option<u64>,
    pub job_title: String,
    pub hire_date: u64,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmployeeOffboarded {
    pub employee: Address,
    pub employer: Address,
    pub termination_date: u64,
    pub reason: String,
    pub final_payment_amount: Option<i128>,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmployeeTransferred {
    pub employee: Address,
    pub from_department: u64,
    pub to_department: u64,
    pub from_manager: Address,
    pub to_manager: Address,
    pub transfer_date: u64,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmployeeStatusChanged {
    pub employee: Address,
    pub employer: Address,
    pub old_status: String,
    pub new_status: String,
    pub changed_by: Address,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OnboardingWorkflowEvent {
    pub workflow_id: u64,
    pub employee: Address,
    pub employer: Address,
    pub status: String,
    pub completed_tasks: u32,
    pub total_tasks: u32,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OffboardingWorkflowEvent {
    pub workflow_id: u64,
    pub employee: Address,
    pub employer: Address,
    pub status: String,
    pub completed_tasks: u32,
    pub total_tasks: u32,
    pub has_final_payment: bool,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalPaymentProcessed {
    pub employee: Address,
    pub employer: Address,
    pub amount: i128,
    pub token: Address,
    pub includes_severance: bool,
    pub includes_unused_leave: bool,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComplianceUpdated {
    pub employee: Address,
    pub compliance_type: String,
    pub status: String,
    pub due_date: u64,
    pub completed_date: Option<u64>,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowApproved {
    pub workflow_id: u64,
    pub workflow_type: String,
    pub employee: Address,
    pub approver: Address,
    pub approved: bool,
    pub comment: String,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskCompleted {
    pub workflow_id: u64,
    pub task_id: u32,
    pub task_name: String,
    pub completed_by: Address,
    pub timestamp: u64,
}

pub fn emit_disburse(
    e: Env,
    employer: Address,
    employee: Address,
    token: Address,
    amount: i128,
    timestamp: u64,
) {
    let topics = (Symbol::new(&e, "SalaryDisbursed"),);
    let event_data = SalaryDisbursed {
        employer,
        employee,
        token,
        amount,
        timestamp,
    };
    e.events().publish(topics, event_data.clone());
}

pub fn emit_employer_withdrawn(
    e: Env,
    employer: Address,
    token: Address,
    amount: i128,
    timestamp: u64,
) {
    let topics = (Symbol::new(&e, "EmployerWithdrawn"),);
    let event_data = EmployerWithdrawn {
        employer,
        token,
        amount,
        timestamp,
    };
    e.events().publish(topics, event_data.clone());
}

// Insurance event emission functions
pub fn emit_insurance_policy_created(
    e: Env,
    employer: Address,
    employee: Address,
    coverage_amount: i128,
    premium_amount: i128,
    timestamp: u64,
) {
    let topics = (INS_POLICY_CREATED,);
    let event_data = InsurancePolicyCreated {
        employer,
        employee,
        coverage_amount,
        premium_amount,
        timestamp,
    };
    e.events().publish(topics, event_data.clone());
}

pub fn emit_insurance_claim_filed(
    e: Env,
    employee: Address,
    claim_id: u64,
    claim_amount: i128,
    timestamp: u64,
) {
    let topics = (INS_CLAIM_FILED,);
    let event_data = InsuranceClaimFiled {
        employee,
        claim_id,
        claim_amount,
        timestamp,
    };
    e.events().publish(topics, event_data.clone());
}

pub fn emit_insurance_claim_paid(
    e: Env,
    claim_id: u64,
    employee: Address,
    amount: i128,
    timestamp: u64,
) {
    let topics = (INS_CLAIM_PAID,);
    let event_data = InsuranceClaimPaid {
        claim_id,
        employee,
        amount,
        timestamp,
    };
    e.events().publish(topics, event_data.clone());
}

pub fn emit_guarantee_issued(
    e: Env,
    employer: Address,
    guarantee_id: u64,
    guarantee_amount: i128,
    timestamp: u64,
) {
    let topics = (GUAR_ISSUED,);
    let event_data = GuaranteeIssued {
        employer,
        guarantee_id,
        guarantee_amount,
        timestamp,
    };
    e.events().publish(topics, event_data.clone());
}

// Employee Lifecycle Event Emission Functions
pub fn emit_employee_onboarded(
    e: Env,
    employee: Address,
    employer: Address,
    department_id: Option<u64>,
    job_title: String,
    hire_date: u64,
    timestamp: u64,
) {
    let topics = (EMPLOYEE_ONBOARDED,);
    let event_data = EmployeeOnboarded {
        employee,
        employer,
        department_id,
        job_title,
        hire_date,
        timestamp,
    };
    e.events().publish(topics, event_data);
}

pub fn emit_employee_offboarded(
    e: Env,
    employee: Address,
    employer: Address,
    termination_date: u64,
    reason: String,
    final_payment_amount: Option<i128>,
    timestamp: u64,
) {
    let topics = (EMPLOYEE_OFFBOARDED,);
    let event_data = EmployeeOffboarded {
        employee,
        employer,
        termination_date,
        reason,
        final_payment_amount,
        timestamp,
    };
    e.events().publish(topics, event_data);
}

pub fn emit_employee_transferred(
    e: Env,
    employee: Address,
    from_department: u64,
    to_department: u64,
    from_manager: Address,
    to_manager: Address,
    transfer_date: u64,
    timestamp: u64,
) {
    let topics = (EMPLOYEE_TRANSFERRED,);
    let event_data = EmployeeTransferred {
        employee,
        from_department,
        to_department,
        from_manager,
        to_manager,
        transfer_date,
        timestamp,
    };
    e.events().publish(topics, event_data);
}

pub fn emit_employee_status_changed(
    e: Env,
    employee: Address,
    employer: Address,
    old_status: String,
    new_status: String,
    changed_by: Address,
    timestamp: u64,
) {
    let topics = (EMPLOYEE_STATUS_CHANGED,);
    let event_data = EmployeeStatusChanged {
        employee,
        employer,
        old_status,
        new_status,
        changed_by,
        timestamp,
    };
    e.events().publish(topics, event_data);
}

pub fn emit_onboarding_workflow_event(
    e: Env,
    workflow_id: u64,
    employee: Address,
    employer: Address,
    status: String,
    completed_tasks: u32,
    total_tasks: u32,
    timestamp: u64,
) {
    let topics = (ONBOARDING_STARTED,);
    let event_data = OnboardingWorkflowEvent {
        workflow_id,
        employee,
        employer,
        status,
        completed_tasks,
        total_tasks,
        timestamp,
    };
    e.events().publish(topics, event_data);
}

pub fn emit_offboarding_workflow_event(
    e: Env,
    workflow_id: u64,
    employee: Address,
    employer: Address,
    status: String,
    completed_tasks: u32,
    total_tasks: u32,
    has_final_payment: bool,
    timestamp: u64,
) {
    let topics = (OFFBOARDING_STARTED,);
    let event_data = OffboardingWorkflowEvent {
        workflow_id,
        employee,
        employer,
        status,
        completed_tasks,
        total_tasks,
        has_final_payment,
        timestamp,
    };
    e.events().publish(topics, event_data);
}

pub fn emit_final_payment_processed(
    e: Env,
    employee: Address,
    employer: Address,
    amount: i128,
    token: Address,
    includes_severance: bool,
    includes_unused_leave: bool,
    timestamp: u64,
) {
    let topics = (FINAL_PAYMENT_PROCESSED,);
    let event_data = FinalPaymentProcessed {
        employee,
        employer,
        amount,
        token,
        includes_severance,
        includes_unused_leave,
        timestamp,
    };
    e.events().publish(topics, event_data);
}

pub fn emit_compliance_updated(
    e: Env,
    employee: Address,
    compliance_type: String,
    status: String,
    due_date: u64,
    completed_date: Option<u64>,
    timestamp: u64,
) {
    let topics = (COMPLIANCE_UPDATED,);
    let event_data = ComplianceUpdated {
        employee,
        compliance_type,
        status,
        due_date,
        completed_date,
        timestamp,
    };
    e.events().publish(topics, event_data);
}

pub fn emit_workflow_approved(
    e: Env,
    workflow_id: u64,
    workflow_type: String,
    employee: Address,
    approver: Address,
    approved: bool,
    comment: String,
    timestamp: u64,
) {
    let topics = (WORKFLOW_APPROVED,);
    let event_data = WorkflowApproved {
        workflow_id,
        workflow_type,
        employee,
        approver,
        approved,
        comment,
        timestamp,
    };
    e.events().publish(topics, event_data);
}

pub fn emit_task_completed(
    e: Env,
    workflow_id: u64,
    task_id: u32,
    task_name: String,
    completed_by: Address,
    timestamp: u64,
) {
    let topics = (TASK_COMPLETED,);
    let event_data = TaskCompleted {
        workflow_id,
        task_id,
        task_name,
        completed_by,
        timestamp,
    };
    e.events().publish(topics, event_data);
}
