//! Persistent storage keys for the FeeCollector contract.
//!
//! Every value written to `env.storage().instance()` is indexed by one of
//! these variants. Grouping them here makes it easy to audit all persisted
//! state at a glance and prevents accidental key collisions.

use soroban_sdk::contracttype;

/// Persistent storage keys used by the fee collector contract.
#[contracttype]
#[derive(Clone)]
pub enum StorageKey {
    /// Contract admin address (has privileged access to all config operations).
    Admin,
    /// Treasury / fee recipient address.
    FeeRecipient,
    /// Percentage fee rate in basis points (active when `FeeMode::Percentage`).
    FeeBps,
    /// Flat fee amount in token units (active when `FeeMode::Flat`).
    FlatFee,
    /// Currently active fee mode.
    FeeMode,
    /// Tiered fee schedule (Vec<FeeTier>).
    TieredSchedule,
    /// Cumulative fees collected since initialization (saturates at `i128::MAX`).
    TotalFeesCollected,
    /// Emergency pause flag — when `true`, `collect_fee` panics.
    Paused,
    /// Initialization guard — prevents re-initialization.
    Initialized,
    /// Fee split routing policy (optional).
    FeeSplit,
}
