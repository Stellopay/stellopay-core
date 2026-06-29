//! Internal guard functions and pure fee-arithmetic helpers.
//!
//! None of these functions are part of the public contract interface — they are
//! called exclusively from [`crate::FeeCollectorContract`]'s `#[contractimpl]`
//! block. Grouping them here keeps `lib.rs` focused on the public API and makes
//! the guards easy to unit-test in isolation.

use soroban_sdk::{Address, Env};

use crate::storage::StorageKey;
use crate::types::{FeeMode, FeeTier};
use crate::{BPS_DENOMINATOR, TTL_MAX_LEDGERS, TTL_MIN_LEDGERS};

// ---------------------------------------------------------------------------
// TTL
// ---------------------------------------------------------------------------

/// Extends the contract instance TTL so all instance-storage entries remain
/// alive for at least [`TTL_MIN_LEDGERS`] more ledgers, and at most
/// [`TTL_MAX_LEDGERS`] ledgers from now.
///
/// Called at the start of every public entry-point so the contract never
/// becomes permanently inaccessible due to ledger-entry expiry.
pub(crate) fn bump_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(TTL_MIN_LEDGERS, TTL_MAX_LEDGERS);
}

// ---------------------------------------------------------------------------
// State guards
// ---------------------------------------------------------------------------

/// Panics with `"Contract not initialized"` if the contract has not been initialized.
pub(crate) fn require_initialized(env: &Env) {
    let initialized = env
        .storage()
        .instance()
        .get::<_, bool>(&StorageKey::Initialized)
        .unwrap_or(false);
    assert!(initialized, "Contract not initialized");
}

/// Panics with `"Unauthorized: caller is not admin"` if `caller` is not the stored admin.
pub(crate) fn require_admin(env: &Env, caller: &Address) {
    let admin: Address = env
        .storage()
        .instance()
        .get(&StorageKey::Admin)
        .expect("Admin not set");
    assert!(*caller == admin, "Unauthorized: caller is not admin");
}

/// Panics with `"Contract is paused"` if fee collection is currently paused.
pub(crate) fn require_not_paused(env: &Env) {
    let paused: bool = env
        .storage()
        .instance()
        .get(&StorageKey::Paused)
        .unwrap_or(false);
    assert!(!paused, "Contract is paused");
}

// ---------------------------------------------------------------------------
// Fee arithmetic
// ---------------------------------------------------------------------------

/// Computes a percentage fee using integer floor division.
///
/// `fee = floor(gross_amount × fee_bps / 10 000)`
///
/// # Guarantees
///
/// * Returns `0` when `fee_bps == 0` or `gross_amount == 0`.
/// * Since `fee_bps ≤ MAX_FEE_BPS (1 000) < BPS_DENOMINATOR (10 000)`, the
///   result is always strictly less than `gross_amount` for positive inputs.
/// * Panics on overflow (unreachable with `i128` and fees ≤ 10 %).
pub(crate) fn compute_percentage_fee(gross_amount: i128, fee_bps: u32) -> i128 {
    if fee_bps == 0 || gross_amount == 0 {
        return 0;
    }
    let bps = fee_bps as i128;
    let denom = BPS_DENOMINATOR as i128;
    gross_amount
        .checked_mul(bps)
        .expect("Fee computation: multiplication overflow")
        .checked_div(denom)
        .expect("Fee computation: division by zero")
}

/// Computes a tiered fee based on the gross amount and a schedule of tiers.
pub(crate) fn compute_tiered_fee(
    gross_amount: i128,
    schedule: soroban_sdk::Vec<FeeTier>,
) -> i128 {
    if gross_amount == 0 || schedule.is_empty() {
        return 0;
    }

    let mut selected_bps = 0;
    for tier in schedule.iter() {
        if gross_amount <= tier.limit {
            selected_bps = tier.fee_bps;
            break;
        }
        // Fallback to the last tier if no limit is matched yet
        selected_bps = tier.fee_bps;
    }

    compute_percentage_fee(gross_amount, selected_bps)
}

/// Dispatches fee computation to the active [`FeeMode`].
///
/// * `Percentage` — delegates to [`compute_percentage_fee`].
/// * `Flat`       — returns `min(flat_fee, gross_amount)` so net is never negative.
pub(crate) fn compute_fee_internal(env: &Env, gross_amount: i128) -> i128 {
    let mode: FeeMode = env
        .storage()
        .instance()
        .get(&StorageKey::FeeMode)
        .expect("FeeMode not set");
    match mode {
        FeeMode::Percentage => {
            let fee_bps: u32 = env
                .storage()
                .instance()
                .get(&StorageKey::FeeBps)
                .unwrap_or(0);
            compute_percentage_fee(gross_amount, fee_bps)
        }
        FeeMode::Flat => {
            let flat_fee: i128 = env
                .storage()
                .instance()
                .get(&StorageKey::FlatFee)
                .unwrap_or(0);
            // Cap at gross_amount to guarantee net >= 0.
            flat_fee.min(gross_amount)
        }
        FeeMode::Tiered => {
            let schedule: soroban_sdk::Vec<FeeTier> = env
                .storage()
                .instance()
                .get(&StorageKey::TieredSchedule)
                .unwrap_or_else(|| soroban_sdk::Vec::new(env));
            compute_tiered_fee(gross_amount, schedule)
        }
    }
}
