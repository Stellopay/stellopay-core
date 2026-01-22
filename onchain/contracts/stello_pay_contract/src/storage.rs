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

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgreementStatus {
    Created,
    Active,
    Completed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Agreement {
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
    /// Agreement data: agreement_id -> Agreement
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