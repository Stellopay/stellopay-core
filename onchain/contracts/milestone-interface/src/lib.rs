//! Typed cross-contract interface for milestone agreement queries.
//!
//! Depend on this crate (rlib only) from contracts that need to inspect
//! milestone state without linking the full `stello_pay_contract` cdylib.
//! Deploy the `stello_pay_contract` separately.
//!
//! # Usage
//!
//! ```ignore
//! use milestone_interface::{MilestoneContractClient, MilestoneKey};
//!
//! let client = MilestoneContractClient::new(&env, &milestone_contract_address);
//! let milestone = client.get_milestone(&agreement_id, &milestone_id);
//! ```

#![no_std]

use soroban_sdk::{contractclient, contracttype, Address, Env};

/// Lifecycle states for milestone agreements.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MilestoneAgreementStatus {
    Created,
    Active,
    Paused,
    Cancelled,
    Completed,
    Disputed,
}

/// A single milestone within an agreement.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneView {
    /// 1-based milestone identifier within the agreement.
    pub id: u32,
    /// Token amount claimable for this milestone.
    pub amount: i128,
    /// True once the employer has approved this milestone.
    pub approved: bool,
    /// True once the contributor has claimed this milestone's payment.
    pub claimed: bool,
}

/// Summary view of a milestone agreement.
#[contracttype]
#[derive(Clone, Debug)]
pub struct MilestoneAgreementView {
    pub id: u128,
    pub employer: Address,
    pub contributor: Address,
    pub token: Address,
    pub status: MilestoneAgreementStatus,
    pub total_amount: i128,
    /// Accounted escrow balance (tokens deposited via fund_milestone_agreement).
    pub escrow_balance: i128,
    /// Number of milestones added to this agreement.
    pub milestone_count: u32,
}

/// Thin cross-contract client interface for milestone agreement read operations.
///
/// Only query / view methods are exposed here. Mutating calls (fund, add, approve,
/// claim) are performed directly on `stello_pay_contract` which owns the state.
#[contractclient(name = "MilestoneContractClient")]
pub trait MilestoneContractInterface {
    /// Returns a specific milestone, or None if the id is out of range.
    fn get_milestone(env: Env, agreement_id: u128, milestone_id: u32) -> Option<MilestoneView>;

    /// Returns the number of milestones in an agreement.
    fn get_milestone_count(env: Env, agreement_id: u128) -> u32;
}
