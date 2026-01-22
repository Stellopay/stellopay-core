use soroban_sdk::{contracttype, Address};

#[contracttype]
#[derive(Clone, Debug)]
pub struct MilestoneAdded {
    pub agreement_id: u128,
    pub milestone_id: u32,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct MilestoneApproved {
    pub agreement_id: u128,
    pub milestone_id: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct MilestoneClaimed {
    pub agreement_id: u128,
    pub milestone_id: u32,
    pub amount: i128,
    pub to: Address,
}
