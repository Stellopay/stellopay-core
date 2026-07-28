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
//!
//! # Conformance testing
//!
//! Contracts that implement this trait should include a conformance test that
//! exercises the trait surface via `MilestoneContractClient` and compares
//! results against direct contract-client calls.  See
//! `test_milestone_interface_conformance` in
//! `stello_pay_contract/tests/test_milestones.rs` for a reference
//! implementation.

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

/// Thin cross-contract client interface for milestone agreement read operations
/// and lifecycle extension hooks.
///
/// Only query / view methods and optional hooks are exposed here.  Mutating calls
/// (fund, add, approve, claim) are performed directly on `stello_pay_contract`
/// which owns the state.
///
/// # Extension hooks
///
/// Hooks are methods with a provided no-op default body.  Implementors can
/// override them to react to lifecycle events without breaking the interface
/// contract for callers that do not need the behaviour.
///
/// ## `on_milestone_expired`
///
/// **Convention:** `stello_pay_contract` calls this hook from its
/// `expire_milestone` entry-point immediately after persisting the
/// `MilestoneKey::MilestoneExpired` flag and emitting the
/// `MilestoneExpiredEvent`.  Because Soroban traits cannot enforce call-site
/// ordering at the type level, the contract that implements this trait is
/// responsible for ensuring the hook is only invoked once per milestone and
/// only after expiry has been durably recorded.
///
/// Implementors should treat the hook as best-effort: if it panics the whole
/// `expire_milestone` transaction is rolled back, so implementations should
/// be defensive and avoid panicking on unexpected state.
#[contractclient(name = "MilestoneContractClient")]
pub trait MilestoneContractInterface {
    /// Returns a specific milestone, or `None` if the milestone does not exist.
    ///
    /// # Arguments
    /// * `agreement_id` - The agreement to query.
    /// * `milestone_id` - The 1-based milestone identifier within the agreement.
    ///
    /// # Returns
    /// `Some(MilestoneView)` if the milestone exists; `None` if the
    /// `agreement_id` is unrecognized or the `milestone_id` is out of range.
    ///
    /// # Errors / panics
    /// Implementors **must not panic** on invalid input.  Return `None` for any
    /// caller error (unknown agreement, out-of-range id, etc.) and let the
    /// caller decide how to handle the missing value.
    fn get_milestone(env: Env, agreement_id: u128, milestone_id: u32) -> Option<MilestoneView>;

    /// Returns the number of milestones in an agreement.
    ///
    /// # Arguments
    /// * `agreement_id` - The agreement to query.
    ///
    /// # Returns
    /// The milestone count for the agreement, or `0` if the `agreement_id` is
    /// unrecognized (unknown agreement has zero milestones).
    ///
    /// # Errors / panics
    /// Implementors **must not panic** on any input.  Return `0` for an unknown
    /// `agreement_id` and let callers distinguish "no agreement" from "an
    /// agreement with zero milestones" via a separate existence check.
    fn get_milestone_count(env: Env, agreement_id: u128) -> u32;

    /// Hook called when a milestone expires without being claimed or rejected.
    ///
    /// # Semantics
    ///
    /// This method is invoked by the payroll contract's `expire_milestone`
    /// entry-point after it has:
    /// 1. Verified that the milestone is in a state eligible for expiry
    ///    (not already approved, claimed, rejected, or previously expired).
    /// 2. Persisted the expiry flag (`MilestoneKey::MilestoneExpired`) to
    ///    durable storage.
    /// 3. Emitted the `MilestoneExpiredEvent` for off-chain indexers.
    ///
    /// Implementors may use this hook to trigger additional on-chain reactions
    /// such as releasing escrowed funds back to the employer, notifying a
    /// governance contract, or recording an audit entry.
    ///
    /// # Default implementation
    ///
    /// The default body is a no-op, so existing implementors that do not
    /// override this method will continue to compile and run without change.
    ///
    /// # Arguments
    ///
    /// * `env`          – Contract environment provided by the Soroban host.
    /// * `agreement_id` – The milestone agreement that contains the expired milestone.
    /// * `milestone_id` – The 1-based identifier of the expired milestone within
    ///                    `agreement_id`.
    ///
    /// # Panics
    ///
    /// The default implementation never panics.  Custom implementations should
    /// avoid panicking, as a panic here rolls back the entire `expire_milestone`
    /// transaction in the calling contract.
    fn on_milestone_expired(_env: Env, _agreement_id: u128, _milestone_id: u32) {
        // no-op default — existing implementors are unaffected
    }
}
