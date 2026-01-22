use soroban_sdk::{contracttype, Address};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Milestone {
    pub id: u32,
    pub amount: i128,
    pub approved: bool,
    pub claimed: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaymentType {
    LumpSum,
    MilestoneBased,
}

/// Core milestone agreement structure
#[contracttype]
#[derive(Clone, Debug)]
pub struct MilestoneAgreement {
    pub id: u128,
    pub employer: Address,
    pub contributor: Address,
    pub token: Address,
    pub payment_type: PaymentType,
    pub status: AgreementStatus,
    pub total_amount: i128,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Counter for agreement IDs
    AgreementCounter,
    /// Agreement data: agreement_id -> MilestoneAgreement
    Agreement(u128),
    /// Employer address: agreement_id -> Address
    Employer(u128),
    /// Contributor address: agreement_id -> Address
    Contributor(u128),
    /// Token address: agreement_id -> Address
    Token(u128),
    /// Payment type: agreement_id -> PaymentType
    PaymentType(u128),
    /// Agreement status: agreement_id -> AgreementStatus
    Status(u128),
    /// Total amount: agreement_id -> i128
    TotalAmount(u128),

    // Milestone-specific keys
    /// Number of milestones: agreement_id -> u32
    MilestoneCount(u128),
    /// Milestone amount: (agreement_id, milestone_id) -> i128
    MilestoneAmount(u128, u32),
    /// Milestone approval status: (agreement_id, milestone_id) -> bool
    MilestoneApproved(u128, u32),
    /// Milestone claim status: (agreement_id, milestone_id) -> bool
    MilestoneClaimed(u128, u32),
}

impl Milestone {
    pub fn new(id: u32, amount: i128) -> Self {
        Self {
            id,
            amount,
            approved: false,
            claimed: false,
        }
    }

    pub fn can_claim(&self) -> bool {
        self.approved && !self.claimed
    }
}

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
    pub dispute_status: DisputeStatus,
    pub dispute_raised_at: Option<u64>,
    // Time-based payment fields (for escrow mode)
    pub amount_per_period: Option<i128>,
    pub period_seconds: Option<u64>,
    pub num_periods: Option<u32>,
    pub claimed_periods: Option<u32>,
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
    /// Dispute Status
    DisputeStatus(u128),
    DisputeRaisedAt(u128),
    Arbiter,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, PartialOrd)]
pub enum DisputeStatus {
    None,
    Raised,
    Resolved,
}

use soroban_sdk::{contracterror, contractimpl, events};

#[contracterror]
#[derive(Copy, Clone)]
pub enum PayrollError {
    DisputeAlreadyRaised = 1,
    NotInGracePeriod = 2,
    NotParty = 3,
    NotArbiter = 4,
    InvalidPayout = 5,
    ActiveDispute = 6,
    AgreementNotFound = 7,
    NoDispute = 8,
    NoEmployee = 9,
    NotActivated = 10,
}
