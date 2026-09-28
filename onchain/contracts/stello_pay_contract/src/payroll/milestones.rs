//! Milestone agreement lifecycle: creation, funding and milestone state
//! transitions (add / approve / reject / expire). Claim and settlement paths
//! live in [`super::milestone_claims`].

use super::{add_to_employer_agreements, sum_unclaimed_milestones};
use crate::events::{
    emit_milestone_expired, emit_milestone_funded, emit_milestone_rejected, MilestoneAdded,
    MilestoneApproved, MilestoneExpiredEvent, MilestoneFundedEvent, MilestoneRejectedEvent,
};
use crate::storage::{AgreementStatus, MilestoneKey, PaymentType, PayrollError, StorageKey};
use soroban_sdk::{panic_with_error, token::Client as TokenClient, Address, Env, String, Vec};

pub fn create_milestone_agreement(
    env: Env,
    employer: Address,
    contributor: Address,
    token: Address,
    milestones: Vec<i128>,
) -> u128 {
    employer.require_auth();

    if milestones.is_empty() {
        panic_with_error!(&env, PayrollError::EmptyMilestoneList);
    }

    let mut counter: u128 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::AgreementCounter)
        .unwrap_or(0);
    counter += 1;

    let agreement_id = counter;

    env.storage()
        .persistent()
        .set(&MilestoneKey::AgreementCounter, &counter);
    env.storage()
        .persistent()
        .set(&MilestoneKey::Employer(agreement_id), &employer);
    env.storage()
        .persistent()
        .set(&MilestoneKey::Contributor(agreement_id), &contributor);
    env.storage()
        .persistent()
        .set(&MilestoneKey::Token(agreement_id), &token);
    env.storage().persistent().set(
        &MilestoneKey::PaymentType(agreement_id),
        &PaymentType::MilestoneBased,
    );
    env.storage().persistent().set(
        &MilestoneKey::Status(agreement_id),
        &AgreementStatus::Created,
    );

    let milestone_count: u32 = milestones.len();
    let mut total: i128 = 0;
    for (i, amount) in milestones.iter().enumerate() {
        if amount <= 0 {
            panic_with_error!(&env, PayrollError::MilestoneAmountInvalid);
        }
        let milestone_id: u32 = (i as u32) + 1;
        env.storage().persistent().set(
            &MilestoneKey::MilestoneAmount(agreement_id, milestone_id),
            &amount,
        );
        env.storage().persistent().set(
            &MilestoneKey::MilestoneApproved(agreement_id, milestone_id),
            &false,
        );
        env.storage().persistent().set(
            &MilestoneKey::MilestoneClaimed(agreement_id, milestone_id),
            &false,
        );
        total += amount;

        MilestoneAdded {
            agreement_id,
            milestone_id,
            amount,
        }
        .publish(&env);
    }

    env.storage().persistent().set(
        &MilestoneKey::MilestoneCount(agreement_id),
        &milestone_count,
    );
    env.storage()
        .persistent()
        .set(&MilestoneKey::TotalAmount(agreement_id), &total);

    add_to_employer_agreements(&env, &employer, agreement_id);

    agreement_id
}

/// Deposits tokens from `from` into the contract for the specified milestone
/// agreement, crediting the accounted escrow balance.
///
/// # Why this exists
/// `approve_milestone` and `claim_milestone` assert that the contract holds
/// enough tokens to cover unclaimed milestones. Without a dedicated funding
/// entrypoint the only way to satisfy that invariant is to send tokens
/// out-of-band, which is undiscoverable and unauditable. This function
/// provides a first-class, authenticated, on-chain funding path.
///
/// # Arguments
/// * `env`          - Contract environment.
/// * `agreement_id` - ID of the milestone agreement to fund.
/// * `from`         - Address to pull tokens from; must be the agreement's employer and must pass
///   `require_auth`.
/// * `amount`       - Number of tokens to deposit; must be strictly positive.
///
/// # Access Control
/// `from` must equal the employer stored for the agreement, and
/// `from.require_auth()` is called before any state mutation or transfer.
///
/// # State changes — O(1)
/// - Reads + writes `MilestoneKey::MilestoneEscrowBalance(agreement_id)` once.
/// - Executes exactly one `token.transfer(from, contract_address, amount)`.
///
/// # Errors / panics
/// - "Agreement not found" — `agreement_id` does not correspond to a known milestone agreement.
/// - "Unauthorized: only the employer can fund a milestone agreement" — `from` ≠ stored employer.
/// - "Amount must be positive" — `amount` is zero or negative.
/// - "Cannot fund a Cancelled agreement" — agreement status is `Cancelled`.
/// - "Cannot fund a Completed agreement" — agreement status is `Completed`.
/// - "Escrow balance overflow" — cumulative funded amount would overflow `i128`.
/// - Token-transfer panics propagated from the Soroban token host.
///
/// # Security
/// The accounted balance (`MilestoneEscrowBalance`) is the sole source of
/// truth used by `approve_milestone` and `claim_milestone` invariant checks.
/// Raw `token.balance()` of the contract is intentionally **not** consulted
/// so that third-party deposits cannot inflate claimable funds.
pub fn fund_milestone_agreement(env: &Env, agreement_id: u128, from: Address, amount: i128) {
    let employer: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Employer(agreement_id))
        .unwrap_or_else(|| panic_with_error!(env, PayrollError::AgreementNotFound));

    // Only the agreement's employer may fund it.
    assert!(
        from == employer,
        "Unauthorized: only the employer can fund a milestone agreement"
    );
    from.require_auth();

    assert!(amount > 0, "Amount must be positive");

    let status: AgreementStatus = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Status(agreement_id))
        .unwrap_or_else(|| panic_with_error!(env, PayrollError::AgreementNotFound));
    assert!(
        status != AgreementStatus::Cancelled,
        "Cannot fund a Cancelled agreement"
    );
    assert!(
        status != AgreementStatus::Completed,
        "Cannot fund a Completed agreement"
    );

    let current_balance: i128 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneEscrowBalance(agreement_id))
        .unwrap_or(0i128);
    let new_balance = current_balance
        .checked_add(amount)
        .unwrap_or_else(|| panic_with_error!(env, PayrollError::InvalidData));
    env.storage().persistent().set(
        &MilestoneKey::MilestoneEscrowBalance(agreement_id),
        &new_balance,
    );

    let token_address: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Token(agreement_id))
        .unwrap_or_else(|| panic_with_error!(env, PayrollError::AgreementNotFound));
    TokenClient::new(env, &token_address).transfer(&from, env.current_contract_address(), &amount);

    emit_milestone_funded(
        env,
        MilestoneFundedEvent {
            agreement_id,
            from,
            amount,
            total_escrow_balance: new_balance,
        },
    );
}

/// Adds a milestone to an agreement
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the agreement
/// * `amount` - Payment amount for this milestone
///
/// # Errors
/// * `PayrollError::AgreementNotFound` — the milestone agreement does not exist.
/// * `PayrollError::MilestoneAgreementInvalidStatus` — the agreement is not in `Created` status.
/// * `PayrollError::MilestoneAmountInvalid` — `amount` is not strictly positive.
pub fn add_milestone(env: Env, agreement_id: u128, amount: i128) -> Result<(), PayrollError> {
    let status: AgreementStatus = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Status(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;

    if status != AgreementStatus::Created {
        return Err(PayrollError::MilestoneAgreementInvalidStatus);
    }
    if amount <= 0 {
        return Err(PayrollError::MilestoneAmountInvalid);
    }

    let employer: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Employer(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    employer.require_auth();

    let count: u32 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .unwrap_or(0);

    let milestone_id = count + 1;

    env.storage().persistent().set(
        &MilestoneKey::MilestoneAmount(agreement_id, milestone_id),
        &amount,
    );
    env.storage().persistent().set(
        &MilestoneKey::MilestoneApproved(agreement_id, milestone_id),
        &false,
    );
    env.storage().persistent().set(
        &MilestoneKey::MilestoneClaimed(agreement_id, milestone_id),
        &false,
    );
    env.storage()
        .persistent()
        .set(&MilestoneKey::MilestoneCount(agreement_id), &milestone_id);

    let total: i128 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::TotalAmount(agreement_id))
        .unwrap_or(0);
    let new_total = total.checked_add(amount).ok_or(PayrollError::InvalidData)?;
    env.storage()
        .persistent()
        .set(&MilestoneKey::TotalAmount(agreement_id), &new_total);

    // Post-invariant: total amount should equal sum of milestones
    #[cfg(debug_assertions)]
    {
        let total_sum = sum_all_milestones(&env, agreement_id);
        assert!(
            total_sum == total + amount,
            "Total amount mismatch after adding milestone"
        );
    }

    MilestoneAdded {
        agreement_id,
        milestone_id,
        amount,
    }
    .publish(&env);

    Ok(())
}

/// Returns the total configured amount across all milestones for an agreement.
///
/// # Arguments
/// * `env` - Contract environment used to read milestone count and amounts from instance storage.
/// * `agreement_id` - Milestone agreement identifier whose milestone amounts should be summed.
///
/// # Returns
/// Sum of every stored milestone amount for the agreement, treating missing amount entries as zero.
///
/// # Cost
/// O(n) in the stored milestone count for `agreement_id`, where `n` is bounded by the
/// milestones created for that agreement.
fn sum_all_milestones(env: &Env, agreement_id: u128) -> i128 {
    let count: u32 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .unwrap_or(0);
    let mut sum = 0i128;
    for i in 1..=count {
        sum = sum
            .checked_add(
                env.storage()
                    .persistent()
                    .get::<_, i128>(&MilestoneKey::MilestoneAmount(agreement_id, i))
                    .unwrap_or(0),
            )
            .unwrap_or_else(|| panic_with_error!(env, PayrollError::InvalidData));
    }
    sum
}

/// Approves a milestone for payment
///
/// # Arguments
/// * `env` - Contract environment
/// * `agreement_id` - ID of the agreement
/// * `milestone_id` - ID of the milestone to approve
///
/// # Errors
/// * `PayrollError::AgreementNotFound` — the milestone agreement does not exist.
/// * `PayrollError::MilestoneAgreementInvalidStatus` — the agreement is not in `Created` or
///   `Active` status.
/// * `PayrollError::MilestoneNotFound` — `milestone_id` is out of range for the agreement.
/// * `PayrollError::MilestoneAlreadyApproved` — the milestone was already approved.
/// * `PayrollError::InsufficientEscrowBalance` — funded escrow cannot cover all unclaimed
///   milestones.
pub fn approve_milestone(
    env: Env,
    agreement_id: u128,
    milestone_id: u32,
) -> Result<(), PayrollError> {
    let employer: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Employer(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    employer.require_auth();

    let status: AgreementStatus = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Status(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    if status != AgreementStatus::Created && status != AgreementStatus::Active {
        return Err(PayrollError::MilestoneAgreementInvalidStatus);
    }

    let count: u32 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .ok_or(PayrollError::MilestoneNotFound)?;
    if milestone_id == 0 || milestone_id > count {
        return Err(PayrollError::MilestoneNotFound);
    }

    let already_approved: bool = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneApproved(agreement_id, milestone_id))
        .unwrap_or(false);
    if already_approved {
        return Err(PayrollError::MilestoneAlreadyApproved);
    }

    // Guard: cannot approve a milestone that has been rejected.
    let already_rejected: bool = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneRejected(agreement_id, milestone_id))
        .unwrap_or(false);
    if already_rejected {
        return Err(PayrollError::MilestoneAlreadyRejected);
    }

    env.storage().persistent().set(
        &MilestoneKey::MilestoneApproved(agreement_id, milestone_id),
        &true,
    );

    // Invariant: accounted escrow balance must cover all unclaimed milestones
    // (including the one just approved). Uses the accounted balance rather than
    // raw token.balance() so that unrelated deposits cannot satisfy this check.
    let unclaimed_sum = sum_unclaimed_milestones(&env, agreement_id);
    let escrow_balance: i128 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneEscrowBalance(agreement_id))
        .unwrap_or(0i128);
    if escrow_balance < unclaimed_sum {
        return Err(PayrollError::InsufficientEscrowBalance);
    }

    MilestoneApproved {
        agreement_id,
        milestone_id,
    }
    .publish(&env);

    Ok(())
}

/// Rejects a milestone, preventing it from being approved or claimed.
///
/// Only the employer may reject a milestone. A rejected milestone is
/// permanently excluded from the approval-claim lifecycle; it can be neither
/// approved nor claimed after this call. The stored escrow balance is **not**
/// adjusted — the employer should fund a replacement milestone or cancel the
/// agreement to recover unused escrow.
///
/// # Arguments
/// * `env`          - Contract environment.
/// * `agreement_id` - ID of the milestone agreement.
/// * `milestone_id` - 1-based ID of the milestone to reject.
/// * `reason`       - Optional human-readable rationale. Pass an empty string when no reason is
///   required.
///
/// # Errors
/// * `PayrollError::AgreementNotFound`                — agreement or employer record missing.
/// * `PayrollError::MilestoneAgreementInvalidStatus`  — agreement is not `Created` or `Active`.
/// * `PayrollError::MilestoneRejectionReasonEmpty`    — `reason` is empty or whitespace-only.
/// * `PayrollError::MilestoneNotFound`                — `milestone_id` is out of range.
/// * `PayrollError::MilestoneAlreadyRejected`         — milestone was already rejected.
/// * `PayrollError::MilestoneApprovedCannotReject` — milestone is already approved.
/// * `PayrollError::MilestoneClaimedCannotReject`  — milestone is already claimed.
///
/// # Events
/// Emits [`MilestoneRejectedEvent`] on success.
pub fn reject_milestone(
    env: Env,
    agreement_id: u128,
    milestone_id: u32,
    reason: String,
) -> Result<(), PayrollError> {
    // Auth: only the employer may reject a milestone.
    let employer: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Employer(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    employer.require_auth();

    // Agreement must exist and be in a mutable state.
    let status: AgreementStatus = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Status(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    if status != AgreementStatus::Created && status != AgreementStatus::Active {
        return Err(PayrollError::MilestoneAgreementInvalidStatus);
    }

    // Reason must be non-empty and contain at least one non-whitespace character
    // so that off-chain indexers and dispute reviewers can reconstruct the audit trail.
    if reason.is_empty() {
        return Err(PayrollError::MilestoneRejectionReasonEmpty);
    }
    {
        let bytes = reason.to_bytes();
        let mut all_whitespace = true;
        for i in 0..bytes.len() {
            let Some(b) = bytes.get(i) else {
                continue;
            };
            if !(b == b' ' || b == b'\t' || b == b'\n' || b == b'\r') {
                all_whitespace = false;
                break;
            }
        }
        if all_whitespace {
            return Err(PayrollError::MilestoneRejectionReasonEmpty);
        }
    }

    // Milestone must exist.
    let count: u32 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .ok_or(PayrollError::MilestoneNotFound)?;
    if milestone_id == 0 || milestone_id > count {
        return Err(PayrollError::MilestoneNotFound);
    }

    // Guard: cannot re-reject.
    let already_rejected: bool = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneRejected(agreement_id, milestone_id))
        .unwrap_or(false);
    if already_rejected {
        return Err(PayrollError::MilestoneAlreadyRejected);
    }

    // Guard: cannot reject a milestone that has already been claimed.
    // Checked before the approved guard because a claimed milestone is also
    // approved; the more specific error takes priority.
    let already_claimed: bool = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneClaimed(agreement_id, milestone_id))
        .unwrap_or(false);
    if already_claimed {
        return Err(PayrollError::MilestoneClaimedCannotReject);
    }

    // Guard: cannot reject a milestone that has already been approved.
    let already_approved: bool = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneApproved(agreement_id, milestone_id))
        .unwrap_or(false);
    if already_approved {
        return Err(PayrollError::MilestoneApprovedCannotReject);
    }

    // Mark the milestone as rejected.
    env.storage().persistent().set(
        &MilestoneKey::MilestoneRejected(agreement_id, milestone_id),
        &true,
    );

    // Emit the structured rejection event so off-chain indexers can track it.
    emit_milestone_rejected(
        &env,
        MilestoneRejectedEvent {
            agreement_id,
            milestone_id,
            rejected_by: employer,
            reason,
        },
    );

    Ok(())
}

/// Marks a milestone as expired, recording that it was never approved, claimed,
/// or rejected before the employer declared it ineligible.
///
/// This is an employer-initiated operation — only the agreement's employer may
/// expire a milestone. The function does **not** release escrowed funds; the
/// employer should fund a replacement milestone or cancel the agreement to
/// recover unused escrow.
///
/// # When to use
///
/// Call `expire_milestone` when a milestone deadline has passed and neither
/// party has moved it forward (no approval, claim, or rejection).  This cleanly
/// closes the milestone's lifecycle slot so off-chain systems and auditors have
/// a definitive terminal state rather than an ambiguous "never touched" record.
///
/// # Hook convention (`on_milestone_expired`)
///
/// After persisting the expiry flag and emitting `MilestoneExpiredEvent`, this
/// function invokes `MilestoneContractInterface::on_milestone_expired` on any
/// contract address stored under `StorageKey::MilestoneHookContract`.  The
/// call is a best-effort fire-and-forget from the implementing contract's
/// perspective; if no hook address is configured the call is skipped entirely.
/// If the hook panics the whole transaction is rolled back by the Soroban host.
///
/// # Arguments
/// * `env`          - Contract environment.
/// * `agreement_id` - ID of the milestone agreement.
/// * `milestone_id` - 1-based ID of the milestone to expire.
///
/// # Errors
/// * `PayrollError::AgreementNotFound`                  — agreement or employer record missing.
/// * `PayrollError::MilestoneAgreementInvalidStatus`    — agreement is not `Created` or `Active`.
/// * `PayrollError::MilestoneNotFound`                  — `milestone_id` is out of range.
/// * `PayrollError::MilestoneAlreadyExpired`            — milestone was already expired.
/// * `PayrollError::MilestoneAlreadyApproved`           — milestone is already approved
///   (contributor still has the right to claim it).
/// * `PayrollError::MilestoneAlreadyClaimed`            — milestone is already claimed.
/// * `PayrollError::MilestoneAlreadyRejected`           — milestone is already rejected.
///
/// # Events
/// Emits [`MilestoneExpiredEvent`] on success.
pub fn expire_milestone(
    env: Env,
    agreement_id: u128,
    milestone_id: u32,
) -> Result<(), PayrollError> {
    // Auth: only the employer may expire a milestone.
    let employer: Address = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Employer(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    employer.require_auth();

    // Agreement must exist and be in a mutable state.
    let status: AgreementStatus = env
        .storage()
        .persistent()
        .get(&MilestoneKey::Status(agreement_id))
        .ok_or(PayrollError::AgreementNotFound)?;
    if status != AgreementStatus::Created && status != AgreementStatus::Active {
        return Err(PayrollError::MilestoneAgreementInvalidStatus);
    }

    // Milestone must exist.
    let count: u32 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneCount(agreement_id))
        .ok_or(PayrollError::MilestoneNotFound)?;
    if milestone_id == 0 || milestone_id > count {
        return Err(PayrollError::MilestoneNotFound);
    }

    // Guard: cannot re-expire.
    let already_expired: bool = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneExpired(agreement_id, milestone_id))
        .unwrap_or(false);
    if already_expired {
        return Err(PayrollError::MilestoneAlreadyExpired);
    }

    // Guard: cannot expire a milestone that has already been claimed.
    let already_claimed: bool = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneClaimed(agreement_id, milestone_id))
        .unwrap_or(false);
    if already_claimed {
        // Reuses MilestoneAlreadyClaimed (existing error) — callers can
        // distinguish "cannot expire because claimed" from the claimed state.
        return Err(PayrollError::MilestoneAlreadyClaimed);
    }

    // Guard: cannot expire a milestone that has already been approved
    // (the contributor still has the right to claim it).
    let already_approved: bool = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneApproved(agreement_id, milestone_id))
        .unwrap_or(false);
    if already_approved {
        // Reuses MilestoneAlreadyApproved (existing error).
        return Err(PayrollError::MilestoneAlreadyApproved);
    }

    // Guard: cannot expire a milestone that has already been rejected.
    let already_rejected: bool = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneRejected(agreement_id, milestone_id))
        .unwrap_or(false);
    if already_rejected {
        // Reuses MilestoneAlreadyRejected (existing error).
        return Err(PayrollError::MilestoneAlreadyRejected);
    }

    // Retrieve the locked amount for the event (best-effort; 0 if not set).
    let locked_amount: i128 = env
        .storage()
        .persistent()
        .get(&MilestoneKey::MilestoneAmount(agreement_id, milestone_id))
        .unwrap_or(0i128);

    // Mark the milestone as expired.
    env.storage().persistent().set(
        &MilestoneKey::MilestoneExpired(agreement_id, milestone_id),
        &true,
    );

    // Emit the structured expiry event so off-chain indexers can track it.
    emit_milestone_expired(
        &env,
        MilestoneExpiredEvent {
            agreement_id,
            milestone_id,
            locked_amount,
            expired_by: employer.clone(),
        },
    );

    // Fire the on_milestone_expired hook if a hook contract address is
    // configured.  This is a convention-based call — the hook contract must
    // implement MilestoneContractInterface and its `on_milestone_expired`
    // override.  The default no-op means contracts without an override are
    // unaffected.  No hook address configured → silently skip.
    if let Some(hook_addr) = env
        .storage()
        .persistent()
        .get::<_, Address>(&StorageKey::MilestoneHookContract)
    {
        let hook_client = milestone_interface::MilestoneContractClient::new(&env, &hook_addr);
        hook_client.on_milestone_expired(&agreement_id, &milestone_id);
    }

    Ok(())
}
