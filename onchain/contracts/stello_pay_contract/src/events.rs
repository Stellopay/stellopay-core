//-----------------------------------------------------------------------------
// Events
//-----------------------------------------------------------------------------

use soroban_sdk::{contracttype, symbol_short, Address, Env, Symbol};

/// Event emitted when contract is paused
pub const PAUSED_EVENT: Symbol = symbol_short!("paused");

/// Event emitted when contract is unpaused
pub const UNPAUSED_EVENT: Symbol = symbol_short!("unpaused");

pub const DEPOSIT_EVENT: Symbol = symbol_short!("deposit");

/// Event emitted when an individual employee's payroll is paused
pub const EMPLOYEE_PAUSED_EVENT: Symbol = symbol_short!("emppaused");

/// Event emitted when an individual employee's payroll is resumed
pub const EMPLOYEE_RESUMED_EVENT: Symbol = symbol_short!("empresume");

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
