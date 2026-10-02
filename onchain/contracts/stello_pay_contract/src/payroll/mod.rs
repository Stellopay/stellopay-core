//! Payroll module: shared plumbing plus one submodule per domain.
//!
//! This was split out of a single ~4.7k-line file along its existing domain
//! boundaries. Each submodule owns one domain:
//!
//! - [`milestones`] / [`milestone_claims`]: milestone agreements
//! - [`payroll_agreements`] / [`payroll_claims`] / [`payroll_batch`]: payroll agreements
//! - [`escrow`]: escrow agreements
//! - [`disputes`]: arbiter configuration and dispute resolution
//! - [`grace`]: grace periods and cancellation
//! - [`currency`]: FX rate administration and conversion
//! - [`emergency`]: agreement pause/resume and contract emergency pause
//!
//! Shared helpers (multisig wiring, rate limiting, the reentrancy guard,
//! agreement accessors, id allocation and the employer -> agreements index)
//! live here so they are not duplicated across submodules.
//!
//! Crate-facing entry points are re-exported with `pub use`, so
//! `crate::payroll::<fn>` resolves exactly as it did before the split.

mod currency;
mod disputes;
mod emergency;
mod escrow;
mod grace;
mod milestone_claims;
mod milestones;
mod payroll_agreements;
mod payroll_batch;
mod payroll_claims;

pub use currency::*;
pub use disputes::*;
pub use emergency::*;
pub use escrow::*;
pub use grace::*;
pub use milestone_claims::*;
pub use milestones::*;
pub use payroll_agreements::*;
pub use payroll_batch::*;
pub use payroll_claims::*;

use crate::audit::{record_entry, AuditEvent};
use crate::events::{emit_multisig_config_changed, MultisigConfigChangedEvent};
use crate::storage::{Agreement, EmployeeInfo, PayrollError, StorageKey};
use soroban_sdk::{contractclient, contracttype, Address, Env, Vec};

/// Minimal interface for cross-contract calls into the deployed multisig contract.
// The macro consumes this trait to generate the client used by integrations;
// the trait itself has no local Rust call sites.
#[allow(dead_code)]
#[contractclient(name = "MultisigClient")]
trait MultisigInterface {
    fn get_operation(env: Env, operation_id: u128) -> Option<Operation>;
}

// The macro consumes this trait to generate the client used by integrations;
// the trait itself has no local Rust call sites.
#[allow(dead_code)]
#[contractclient(name = "RateLimiterClient")]
trait RateLimiterInterface {
    fn check_and_consume(env: Env, subject: Address) -> u32;
}

// The macro consumes this trait to generate the client used by integrations;
// the trait itself has no local Rust call sites.
#[allow(dead_code)]
#[contractclient(name = "SalaryAdjustmentClient")]
trait SalaryAdjustmentInterface {
    fn get_employee_salary(env: Env, employee: Address) -> Option<i128>;
}

/// Mirror of multisig::OperationStatus — names must match for XDR decoding.
#[contracttype]
#[derive(Clone, PartialEq)]
enum OperationStatus {
    Pending,
    Executed,
    Cancelled,
}

/// Minimal mirror of multisig::Operation for cross-contract reads.
#[contracttype]
#[derive(Clone)]
struct Operation {
    pub id: u128,
    pub kind: OperationKind,
    pub creator: Address,
    pub status: OperationStatus,
    pub created_at: u64,
    pub executed_at: Option<u64>,
}

/// Mirror of multisig::OperationKind — names must match for XDR decoding.
#[contracttype]
#[derive(Clone)]
enum OperationKind {
    ContractUpgrade(Address, soroban_sdk::BytesN<32>),
    LargePayment(Address, Address, i128),
    DisputeResolution(Address, u128, i128, i128),
}

/// Configures the multisig integration for this payroll contract.
///
/// # Arguments
/// * `owner` - Contract owner (must authenticate)
/// * `multisig_contract` - Address of the deployed multisig contract
/// * `large_payment_threshold` - Minimum amount requiring multisig for LargePayment (0 = disabled)
/// * `dispute_resolution_threshold` - Minimum total payout requiring multisig for DisputeResolution
///   (0 = disabled)
///
/// # Access Control
/// Only the contract owner can call this.
pub fn set_multisig_config(
    env: &Env,
    owner: Address,
    multisig_contract: Address,
    large_payment_threshold: i128,
    dispute_resolution_threshold: i128,
) -> Result<(), PayrollError> {
    let stored_owner: Address = env
        .storage()
        .persistent()
        .get(&StorageKey::Owner)
        .ok_or(PayrollError::Unauthorized)?;
    owner.require_auth();
    if owner != stored_owner {
        return Err(PayrollError::Unauthorized);
    }

    // Capture the previous thresholds before overwriting so the emitted event
    // and audit entry can report old-vs-new values (0 = previously unset).
    let old_large_payment_threshold: i128 = env
        .storage()
        .persistent()
        .get(&StorageKey::LargePaymentThreshold)
        .unwrap_or(0);
    let old_dispute_resolution_threshold: i128 = env
        .storage()
        .persistent()
        .get(&StorageKey::DisputeResolutionThreshold)
        .unwrap_or(0);

    env.storage()
        .persistent()
        .set(&StorageKey::MultisigContract, &multisig_contract);
    env.storage()
        .persistent()
        .set(&StorageKey::LargePaymentThreshold, &large_payment_threshold);
    env.storage().persistent().set(
        &StorageKey::DisputeResolutionThreshold,
        &dispute_resolution_threshold,
    );

    // Emit a structured event so off-chain monitors observe approval-requirement
    // changes mid-lifecycle. Only public configuration is exposed.
    emit_multisig_config_changed(
        env,
        MultisigConfigChangedEvent {
            caller: owner.clone(),
            multisig_contract: multisig_contract.clone(),
            old_large_threshold: old_large_payment_threshold,
            new_large_threshold: large_payment_threshold,
            old_dispute_threshold: old_dispute_resolution_threshold,
            new_dispute_threshold: dispute_resolution_threshold,
        },
    );

    // Record a tamper-evident audit entry via the existing audit path. This is a
    // contract-level change, so it uses the sentinel `agreement_id = 0` and
    // reports the new large-payment threshold as the entry's `amount`.
    record_entry(
        env,
        owner,
        AuditEvent::MultisigConfigChanged,
        0,
        None,
        Some(large_payment_threshold),
    );

    Ok(())
}

/// Returns the configured multisig contract address, if any.
pub fn get_multisig_contract(env: &Env) -> Option<Address> {
    env.storage()
        .persistent()
        .get(&StorageKey::MultisigContract)
}

/// Checks that a multisig operation with the given id exists, is Executed,
/// and matches the expected kind discriminant. Returns `MultisigApprovalRequired`
/// if the check fails.
fn require_multisig_executed(
    env: &Env,
    multisig_addr: &Address,
    operation_id: u128,
    check: impl Fn(&OperationKind) -> bool,
) -> Result<(), PayrollError> {
    let client = MultisigClient::new(env, multisig_addr);
    let op = client
        .get_operation(&operation_id)
        .ok_or(PayrollError::MultisigApprovalRequired)?;
    if op.status != OperationStatus::Executed {
        return Err(PayrollError::MultisigApprovalRequired);
    }
    if !check(&op.kind) {
        return Err(PayrollError::MultisigApprovalRequired);
    }
    Ok(())
}

fn enforce_rate_limit(env: &Env, caller: &Address) -> Result<(), PayrollError> {
    if let Some(rate_limiter_addr) = env
        .storage()
        .persistent()
        .get::<_, Address>(&StorageKey::RateLimiterContract)
    {
        let client = RateLimiterClient::new(env, &rate_limiter_addr);
        if client.try_check_and_consume(caller).is_err() {
            return Err(PayrollError::RateLimited);
        }
    }
    Ok(())
}

/// Acquire the transient reentrancy guard for a claim path.
///
/// Returns [`PayrollError::ReentrancyDetected`] if the guard is already held,
/// i.e. this call path was re-entered (e.g. via a hostile or hook-enabled
/// token during `transfer`). The guard is kept in *temporary* storage (see
/// [`StorageKey::ReentrancyGuard`]) so it is automatically cleared at the end
/// of the transaction even if a panic strands it mid-call.
fn acquire_reentrancy_guard(env: &Env) -> Result<(), PayrollError> {
    if env.storage().temporary().has(&StorageKey::ReentrancyGuard) {
        return Err(PayrollError::ReentrancyDetected);
    }
    env.storage()
        .temporary()
        .set(&StorageKey::ReentrancyGuard, &true);
    Ok(())
}

/// Release the transient reentrancy guard acquired by [`acquire_reentrancy_guard`].
///
/// Must be called on every return path of the guarded function so the guard
/// never outlives a single top-level call.
fn release_reentrancy_guard(env: &Env) {
    env.storage()
        .temporary()
        .remove(&StorageKey::ReentrancyGuard);
}

/// Retrieves an agreement by ID
///
/// Bumps the agreement entry's TTL on read (see [`crate::storage::extend_persistent_ttl`])
/// so an active agreement that is accessed but not rewritten for a long time is
/// not archived under Soroban's state-archival model.
///
/// # Returns
/// Some(Agreement) if found, None otherwise
pub fn get_agreement(env: &Env, agreement_id: u128) -> Option<Agreement> {
    let key = StorageKey::Agreement(agreement_id);
    let agreement = env.storage().persistent().get(&key);
    if agreement.is_some() {
        crate::storage::extend_persistent_ttl(env, &key);
    }
    agreement
}

/// Retrieves all employees for an agreement
///
/// # Returns
/// Vector of employee addresses
pub fn get_agreement_employees(env: &Env, agreement_id: u128) -> Vec<Address> {
    let employees: Vec<EmployeeInfo> = env
        .storage()
        .persistent()
        .get(&StorageKey::AgreementEmployees(agreement_id))
        .unwrap_or(Vec::new(env));

    let mut addresses = Vec::new(env);
    for emp in employees.iter() {
        addresses.push_back(emp.address);
    }
    addresses
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn get_next_agreement_id(env: &Env) -> u128 {
    let key = StorageKey::NextAgreementId;
    let id: u128 = env.storage().persistent().get(&key).unwrap_or(1);
    env.storage().persistent().set(&key, &(id + 1));
    id
}

fn add_to_employer_agreements(env: &Env, employer: &Address, agreement_id: u128) {
    let key = StorageKey::EmployerAgreements(employer.clone());
    let mut agreements: Vec<u128> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env));
    agreements.push_back(agreement_id);
    env.storage().persistent().set(&key, &agreements);
}
