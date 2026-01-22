use soroban_sdk::{contract, contractimpl, Address, Env};
use crate::storage::{ AgreementStatus, DataKey, Milestone, PaymentType};
use crate::events::{MilestoneAdded, MilestoneApproved, MilestoneClaimed};

#[contract]
pub struct PayrollContract;

#[contractimpl]
impl PayrollContract {

    pub fn create_milestone_agreement(
        env: Env,
        employer: Address,
        contributor: Address,
        token: Address,
    ) -> u128 {
        employer.require_auth();

        let mut counter: u128 = env.storage().instance()
            .get(&DataKey::AgreementCounter)
            .unwrap_or(0);
        counter += 1;
        
        let agreement_id = counter;

        env.storage().instance().set(&DataKey::AgreementCounter, &counter);
        env.storage().instance().set(&DataKey::Employer(agreement_id), &employer);
        env.storage().instance().set(&DataKey::Contributor(agreement_id), &contributor);
        env.storage().instance().set(&DataKey::Token(agreement_id), &token);
        env.storage().instance().set(&DataKey::PaymentType(agreement_id), &PaymentType::MilestoneBased);
        env.storage().instance().set(&DataKey::Status(agreement_id), &AgreementStatus::Created);
        env.storage().instance().set(&DataKey::TotalAmount(agreement_id), &0i128);
        env.storage().instance().set(&DataKey::MilestoneCount(agreement_id), &0u32);
        
        agreement_id
    }
    
    /// Adds a milestone to an agreement
    /// 
    /// # Arguments
    /// * `env` - Contract environment
    /// * `agreement_id` - ID of the agreement
    /// * `amount` - Payment amount for this milestone
    pub fn add_milestone(env: Env, agreement_id: u128, amount: i128) {
   
        let status: AgreementStatus = env.storage().instance()
            .get(&DataKey::Status(agreement_id))
            .expect("Agreement not found");
        
        assert!(status == AgreementStatus::Created, "Agreement must be in Created status");
        assert!(amount > 0, "Amount must be positive");
  
        let employer: Address = env.storage().instance()
            .get(&DataKey::Employer(agreement_id))
            .expect("Employer not found");
        employer.require_auth();
  
        let count: u32 = env.storage().instance()
            .get(&DataKey::MilestoneCount(agreement_id))
            .unwrap_or(0);
        
        let milestone_id = count + 1;
 
        env.storage().instance().set(&DataKey::MilestoneAmount(agreement_id, milestone_id), &amount);
        env.storage().instance().set(&DataKey::MilestoneApproved(agreement_id, milestone_id), &false);
        env.storage().instance().set(&DataKey::MilestoneClaimed(agreement_id, milestone_id), &false);
        env.storage().instance().set(&DataKey::MilestoneCount(agreement_id), &milestone_id);

        let total: i128 = env.storage().instance()
            .get(&DataKey::TotalAmount(agreement_id))
            .unwrap_or(0);
        env.storage().instance().set(&DataKey::TotalAmount(agreement_id), &(total + amount));
        

        env.events().publish(
            ("milestone_added", agreement_id),
            MilestoneAdded {
                agreement_id,
                milestone_id,
                amount,
            }
        );
    }
    
    /// Approves a milestone for payment
    /// 
    /// # Arguments
    /// * `env` - Contract environment
    /// * `agreement_id` - ID of the agreement
    /// * `milestone_id` - ID of the milestone to approve

    pub fn approve_milestone(env: Env, agreement_id: u128, milestone_id: u32) {

        let employer: Address = env.storage().instance()
            .get(&DataKey::Employer(agreement_id))
            .expect("Employer not found");
        employer.require_auth();

        let count: u32 = env.storage().instance()
            .get(&DataKey::MilestoneCount(agreement_id))
            .expect("No milestones found");
        assert!(milestone_id > 0 && milestone_id <= count, "Invalid milestone ID");

        let already_approved: bool = env.storage().instance()
            .get(&DataKey::MilestoneApproved(agreement_id, milestone_id))
            .unwrap_or(false);
        assert!(!already_approved, "Milestone already approved");

        env.storage().instance().set(&DataKey::MilestoneApproved(agreement_id, milestone_id), &true);

        env.events().publish(
            ("milestone_approved", agreement_id),
            MilestoneApproved {
                agreement_id,
                milestone_id,
            }
        );
    }
    
    /// Claims payment for an approved milestone
    /// 
    /// # Arguments
    /// * `env` - Contract environment
    /// * `agreement_id` - ID of the agreement
    /// * `milestone_id` - ID of the milestone to claim
      pub fn claim_milestone(env: Env, agreement_id: u128, milestone_id: u32) {

        let contributor: Address = env.storage().instance()
            .get(&DataKey::Contributor(agreement_id))
            .expect("Contributor not found");
        contributor.require_auth();

        let count: u32 = env.storage().instance()
            .get(&DataKey::MilestoneCount(agreement_id))
            .expect("No milestones found");
        assert!(milestone_id > 0 && milestone_id <= count, "Invalid milestone ID");

        let approved: bool = env.storage().instance()
            .get(&DataKey::MilestoneApproved(agreement_id, milestone_id))
            .unwrap_or(false);
        assert!(approved, "Milestone not approved");

        let already_claimed: bool = env.storage().instance()
            .get(&DataKey::MilestoneClaimed(agreement_id, milestone_id))
            .unwrap_or(false);
        assert!(!already_claimed, "Milestone already claimed");

        let amount: i128 = env.storage().instance()
            .get(&DataKey::MilestoneAmount(agreement_id, milestone_id))
            .expect("Milestone amount not found");

        env.storage().instance().set(&DataKey::MilestoneClaimed(agreement_id, milestone_id), &true);

        let _token: Address = env.storage().instance()
            .get(&DataKey::Token(agreement_id))
            .expect("Token not found");
        

        env.events().publish(
            ("milestone_claimed", agreement_id),
            MilestoneClaimed {
                agreement_id,
                milestone_id,
                amount,
                to: contributor.clone(),
            }
        );

        let all_claimed = Self::all_milestones_claimed(&env, agreement_id, count);
        if all_claimed {
            env.storage().instance().set(&DataKey::Status(agreement_id), &AgreementStatus::Completed);
        }
    }

    pub fn get_milestone_count(env: Env, agreement_id: u128) -> u32 {
        env.storage().instance()
            .get(&DataKey::MilestoneCount(agreement_id))
            .unwrap_or(0)
    }
    

    pub fn get_milestone(env: Env, agreement_id: u128, milestone_id: u32) -> Option<Milestone> {
        let count: u32 = env.storage().instance()
            .get(&DataKey::MilestoneCount(agreement_id))
            .unwrap_or(0);
        
        if milestone_id == 0 || milestone_id > count {
            return None;
        }
        
        let amount: i128 = env.storage().instance()
            .get(&DataKey::MilestoneAmount(agreement_id, milestone_id))?;
        let approved: bool = env.storage().instance()
            .get(&DataKey::MilestoneApproved(agreement_id, milestone_id))
            .unwrap_or(false);
        let claimed: bool = env.storage().instance()
            .get(&DataKey::MilestoneClaimed(agreement_id, milestone_id))
            .unwrap_or(false);
        
        Some(Milestone {
            id: milestone_id,
            amount,
            approved,
            claimed,
        })
    }

    fn all_milestones_claimed(env: &Env, agreement_id: u128, count: u32) -> bool {
        for i in 1..=count {
            let claimed: bool = env.storage().instance()
                .get(&DataKey::MilestoneClaimed(agreement_id, i))
                .unwrap_or(false);
            if !claimed {
                return false;
            }
        }
        true
    }
}