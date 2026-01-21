use soroban_sdk::{contracttype, Address};

/// Operating mode for agreements
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgreementMode {
    /// Escrow mode for freelance/contract work
    Escrow,
    /// Payroll mode for traditional employee payroll
    Payroll,
}

/// Lifecycle states for agreements
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgreementStatus {
    /// Agreement created but not yet funded/activated
    Created,
    /// Agreement is active and payments can be processed
    Active,
    /// Agreement temporarily paused
    Paused,
    /// Agreement cancelled by employer
    Cancelled,
    /// Agreement completed successfully
    Completed,
    /// Agreement in dispute
    Disputed,
}

/// Core agreement structure
#[contracttype]
#[derive(Clone, Debug)]
pub struct Agreement {
    pub id: u128,
    pub employer: Address,
    pub token: Address,
    pub mode: AgreementMode,
    pub status: AgreementStatus,
    pub total_amount: i128,
    pub paid_amount: i128,
    pub created_at: u64,
    pub activated_at: Option<u64>,
    pub cancelled_at: Option<u64>,
    pub grace_period_seconds: u64,
}

/// Employee info within an agreement
#[contracttype]
#[derive(Clone, Debug)]
pub struct EmployeeInfo {
    pub address: Address,
    pub salary_per_period: i128,
    pub added_at: u64,
}

/// Storage keys
#[contracttype]
#[derive(Clone)]
pub enum StorageKey {
    /// Contract owner
    Owner,
    /// Agreement by ID
    Agreement(u128),
    /// List of employees for an agreement
    AgreementEmployees(u128),
    /// Next agreement ID counter
    NextAgreementId,
    /// List of agreement IDs for an employer
    EmployerAgreements(Address),
}