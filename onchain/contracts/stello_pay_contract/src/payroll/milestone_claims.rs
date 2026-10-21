//! Milestone claim and settlement paths, milestone read helpers, and the
//! shared milestone scan helper (`sum_unclaimed_milestones`).
//!
//! `sum_unclaimed_milestones` is `pub(super)` so [`super::milestones`] can
//! reuse it instead of duplicating the scan logic.

use super::is_emergency_paused;
use crate::events::{BatchMilestoneClaimedEvent, MilestoneClaimed};
use crate::storage::{
    AgreementStatus, BatchMilestoneResult, Milestone, MilestoneClaimResult, MilestoneKey,
    PayrollError, MAX_BATCH_SIZE,
};
use soroban_sdk::{token::Client as TokenClient, Address, Env, Vec};

/// Returns the total amount still locked for unclaimed milestones.
///
/// # Arguments
/// * `env` - Contract environment used to read approval, claim, count, and amount entries.
/// * `agreement_id` - Milestone agreement identifier whose unclaimed milestones are inspected.
///
/// # Returns
/// Sum of milestone amounts that have not been claimed, treating missing boolean or amount entries
/// as false/zero.
///
/// # Cost
/// O(n) in the stored milestone count for `agreement_id`, with one approval lookup,
/// one claimed lookup, and at most one amount lookup per milestone.
pub(super) fn sum_unclaimed_milestones(env: &Env, agreement_id: u128) -> i128 {
    let count: u32 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .unwrap_or(0);
    let mut sum = 0i128;
    for i in 1..=count {
        let approved: bool = env
            .storage()
            .persistent()
            .get(&MilestoneKey::MilestoneApproved(agreement_id, i))
            .unwrap_or(false);
        let claimed: bool = env
            .storage()
            .persistent()
            .get(&MilestoneKey::MilestoneClaimed(agreement_id, i))
            .unwrap_or(false);
        if approved && !claimed {
            sum += env
                .storage()
                .persistent()
                .get::<_, i128>(&MilestoneKey::MilestoneAmount(agreement_id, i))
                .unwrap_or(0);
        }
    }
    sum
}

///
/// # Requirements
/// - Agreement must not be Paused
/// - Milestone must be approved
/// - Milestone must not be already claimed
///
/// # Errors
/// * `PayrollError::EmergencyPaused` — the contract is under an emergency pause.
/// * `PayrollError::AgreementNotFound` — the milestone agreement (or its token) does not exist.
/// * `PayrollError::AgreementPaused` — the agreement is currently paused.
/// * `PayrollError::MilestoneNotFound` — `milestone_id` is out of range or its amount is missing.
/// * `PayrollError::MilestoneNotApproved` — the milestone has not been approved.
/// * `PayrollError::MilestoneAlreadyClaimed` — the milestone was already claimed.
/// * `PayrollError::InsufficientEscrowBalance` — funded escrow cannot cover all unclaimed
///   milestones.
pub fn claim_milestone(
    env: Env,
    agreement_id: u128,
    milestone_id: u32,
) -> Result<(), PayrollError> {
    // Check emergency pause
    if is_emergency_paused(&env) {
        return Err(PayrollError::EmergencyPaused);
    }

    let contributor: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Contributor(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    contributor.require_auth();

    // Check if agreement is paused
    let status: AgreementStatus = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Status(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    if status == AgreementStatus::Paused {
        return Err(PayrollError::AgreementPaused);
    }

    let count: u32 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .ok_or(PayrollError::MilestoneNotFound)?;
    if milestone_id == 0 || milestone_id > count {
        return Err(PayrollError::MilestoneNotFound);
    }

    let approved: bool = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneApproved(agreement_id, milestone_id))
        .unwrap_or(false);
    if !approved {
        return Err(PayrollError::MilestoneNotApproved);
    }

    let already_claimed: bool = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneClaimed(agreement_id, milestone_id))
        .unwrap_or(false);
    if already_claimed {
        return Err(PayrollError::MilestoneAlreadyClaimed);
    }

    // Invariant check: accounted escrow balance must cover all unclaimed
    // milestones before we allow the transfer. Using the accounted balance
    // prevents third-party token transfers from inflating claimable funds.
    let unclaimed_sum = sum_unclaimed_milestones(&env, agreement_id);
    let escrow_balance: i128 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneEscrowBalance(agreement_id))
        .unwrap_or(0i128);
    if escrow_balance < unclaimed_sum {
        return Err(PayrollError::InsufficientEscrowBalance);
    }

    let amount: i128 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneAmount(agreement_id, milestone_id))
        .ok_or(PayrollError::MilestoneNotFound)?;

    let token_address: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Token(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;

    // Checks-Effects-Interactions: update all state before the external transfer.
    env.storage().persistent().set(
        &MilestoneKey::MilestoneClaimed(agreement_id, milestone_id),
        &true,
    );

    // Decrement the accounted escrow balance so subsequent invariant checks
    // reflect the reduced available balance.
    let escrow_balance: i128 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneEscrowBalance(agreement_id))
        .unwrap_or(0i128);
    env.storage().persistent().set(
        &MilestoneKey::MilestoneEscrowBalance(agreement_id),
        &escrow_balance.saturating_sub(amount),
    );

    TokenClient::new(&env, &token_address).transfer(
        &env.current_contract_address(),
        &contributor,
        &amount,
    );

    MilestoneClaimed {
        agreement_id,
        milestone_id,
        amount,
        to: contributor.clone(),
    }
    .publish(&env);

    let all_claimed = all_milestones_claimed(&env, agreement_id, count);
    if all_claimed {
        env.storage().persistent().set(
            &MilestoneKey::Status(agreement_id),
            &AgreementStatus::Completed,
        );
    }

    Ok(())
}

/// Iterates over `milestone_ids` and claims each approved, unclaimed milestone
/// for the authenticated contributor. Failures are non-fatal; processing
/// continues to the next ID on error.
///
/// # Arguments
/// * `env`           - Contract environment
/// * `agreement_id`  - ID of the milestone agreement
/// * `milestone_ids` - 1-based milestone IDs to claim. Duplicates are detected in-memory and
///   skipped. At most `MAX_BATCH_SIZE` IDs are accepted.
///
/// # Returns
/// `Ok(BatchMilestoneResult)` with per-milestone results.
///
/// # Batch-level errors
/// These stop the whole batch before any state mutation or transfer:
/// * `PayrollError::AgreementNotFound` — no such agreement (contributor, status, or token record
///   missing).
/// * `PayrollError::InvalidData` — the milestone ID list is empty.
/// * `PayrollError::BatchTooLarge` — more than `MAX_BATCH_SIZE` IDs.
/// * `PayrollError::AgreementPaused` — the agreement is paused.
/// * `PayrollError::MilestoneNotFound` — the agreement has no milestones.
///
/// # Per-milestone `error_code` (in each `MilestoneClaimResult`)
/// `0` = success | `1` = duplicate in this batch | `2` = invalid/unknown
/// milestone ID | `3` = not approved | `4` = already claimed.
///
/// # Gas rationale
/// `MAX_BATCH_SIZE` is 20 because `tests/gas_benchmarks.rs` measures the
/// milestone batch path at that size and enforces the committed gas ceiling.
/// The bound is checked before milestone state updates or token transfers.
pub fn batch_claim_milestones(
    env: &Env,
    agreement_id: u128,
    milestone_ids: Vec<u32>,
) -> Result<BatchMilestoneResult, PayrollError> {
    let contributor: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Contributor(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    contributor.require_auth();

    if milestone_ids.is_empty() {
        return Err(PayrollError::InvalidData);
    }
    if milestone_ids.len() > MAX_BATCH_SIZE {
        return Err(PayrollError::BatchTooLarge);
    }

    // Shared pre-flight
    let status: AgreementStatus = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Status(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    if status == AgreementStatus::Paused {
        return Err(PayrollError::AgreementPaused);
    }

    let count: u32 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .ok_or(PayrollError::MilestoneNotFound)?;

    // Token client created once and reused
    let token: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Token(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    let token_client = TokenClient::new(env, &token);
    let contract_address = env.current_contract_address();

    let mut results: Vec<MilestoneClaimResult> = Vec::new(env);
    let mut total_claimed: i128 = 0;
    let mut successful_claims: u32 = 0;
    let mut failed_claims: u32 = 0;
    let mut processed: Vec<u32> = Vec::new(env);

    for milestone_id in milestone_ids.iter() {
        // Duplicate guard
        if processed.iter().any(|p| p == milestone_id) {
            failed_claims += 1;
            results.push_back(MilestoneClaimResult {
                milestone_id,
                success: false,
                amount_claimed: 0,
                error_code: 1, // duplicate
            });
            continue;
        }
        processed.push_back(milestone_id);

        // Bounds check (1-based, mirrors claim_milestone)
        if milestone_id == 0 || milestone_id > count {
            failed_claims += 1;
            results.push_back(MilestoneClaimResult {
                milestone_id,
                success: false,
                amount_claimed: 0,
                error_code: 2, // invalid ID
            });
            continue;
        }

        // Approved check
        let approved: bool = env
            .storage()
            .persistent()
            .get(&MilestoneKey::MilestoneApproved(agreement_id, milestone_id))
            .unwrap_or(false);
        if !approved {
            failed_claims += 1;
            results.push_back(MilestoneClaimResult {
                milestone_id,
                success: false,
                amount_claimed: 0,
                error_code: 3, // not approved
            });
            continue;
        }

        // Already-claimed check
        let already_claimed: bool = env
            .storage()
            .persistent()
            .get(&MilestoneKey::MilestoneClaimed(agreement_id, milestone_id))
            .unwrap_or(false);
        if already_claimed {
            failed_claims += 1;
            results.push_back(MilestoneClaimResult {
                milestone_id,
                success: false,
                amount_claimed: 0,
                error_code: 4, // already claimed
            });
            continue;
        }

        let amount: i128 = match env
            .storage()
            .persistent()
            .get(&MilestoneKey::MilestoneAmount(agreement_id, milestone_id))
        {
            Some(amount) => amount,
            None => {
                // Record a per-item failure and continue: an early return here
                // would abort the batch after earlier milestones in this loop
                // had already transferred funds.
                failed_claims += 1;
                results.push_back(MilestoneClaimResult {
                    milestone_id,
                    success: false,
                    amount_claimed: 0,
                    error_code: 2, // amount/milestone not found
                });
                continue;
            }
        };

        // Checks-Effects-Interactions: update all state before the external transfer.
        env.storage().persistent().set(
            &MilestoneKey::MilestoneClaimed(agreement_id, milestone_id),
            &true,
        );

        // Decrement the accounted escrow balance to keep invariants consistent
        // across subsequent iterations of this batch.
        let escrow_balance: i128 = env
            .storage()
            .persistent()
            .get(&MilestoneKey::MilestoneEscrowBalance(agreement_id))
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &MilestoneKey::MilestoneEscrowBalance(agreement_id),
            &escrow_balance.saturating_sub(amount),
        );

        token_client.transfer(&contract_address, &contributor, &amount);

        total_claimed += amount;
        successful_claims += 1;

        // Event — identical to claim_milestone
        #[allow(clippy::needless_borrow)]
        MilestoneClaimed {
            agreement_id,
            milestone_id,
            amount,
            to: contributor.clone(),
        }
        .publish(&env);

        results.push_back(MilestoneClaimResult {
            milestone_id,
            success: true,
            amount_claimed: amount,
            error_code: 0,
        });
    }

    if all_milestones_claimed(env, agreement_id, count) {
        env.storage().persistent().set(
            &MilestoneKey::Status(agreement_id),
            &AgreementStatus::Completed,
        );
    }

    #[allow(clippy::needless_borrow)]
    BatchMilestoneClaimedEvent {
        agreement_id,
        total_claimed,
        successful_claims,
        failed_claims,
    }
    .publish(&env);

    Ok(BatchMilestoneResult {
        agreement_id,
        total_claimed,
        successful_claims,
        failed_claims,
        results,
    })
}

pub fn get_milestone_count(env: Env, agreement_id: u128) -> u32 {
    env.storage()
        .persistent()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .unwrap_or(0)
}

pub fn get_milestone(env: Env, agreement_id: u128, milestone_id: u32) -> Option<Milestone> {
    let count: u32 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .unwrap_or(0);

    if milestone_id == 0 || milestone_id > count {
        return None;
    }

    let amount: i128 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneAmount(agreement_id, milestone_id))?;
    let approved: bool = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneApproved(agreement_id, milestone_id))
        .unwrap_or(false);
    let claimed: bool = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneClaimed(agreement_id, milestone_id))
        .unwrap_or(false);

    Some(Milestone {
        id: milestone_id,
        amount,
        approved,
        claimed,
    })
}

/// Reports whether every milestone up to `count` has been claimed.
///
/// # Arguments
/// * `env` - Contract environment used to read claimed flags from instance storage.
/// * `agreement_id` - Milestone agreement identifier whose claim flags should be checked.
/// * `count` - Number of milestones to scan, usually the stored `MilestoneCount` for the agreement.
///
/// # Returns
/// `true` when all milestone IDs from `1..=count` are marked claimed; otherwise `false`.
///
/// # Cost
/// O(n) in `count`. The scan short-circuits on the first unclaimed milestone and is
/// bounded by the caller-supplied milestone count.
fn all_milestones_claimed(env: &Env, agreement_id: u128, count: u32) -> bool {
    for i in 1..=count {
        let claimed: bool = env
            .storage()
            .persistent()
            .get(&MilestoneKey::MilestoneClaimed(agreement_id, i))
            .unwrap_or(false);
        if !claimed {
            return false;
        }
    }
    true
}
