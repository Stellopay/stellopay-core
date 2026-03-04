//! Event types emitted by the FeeCollector contract.
//!
//! Off-chain indexers (horizon, custom processors) can subscribe to these
//! events to reconstruct fee history, detect config changes, and monitor
//! emergency pause toggles without polling contract state.

use soroban_sdk::{contracttype, Address};

use crate::types::FeeMode;

// ---------------------------------------------------------------------------
// Fee collection
// ---------------------------------------------------------------------------

/// Emitted on every successful call to `collect_fee`.
///
/// Off-chain indexers can use this event stream to reconstruct the full history
/// of protocol fee income and verify treasury accounting.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeCollectedEvent {
    /// Address that originated the payment (and whose allowance was spent).
    pub payer: Address,
    /// Token contract address used for the payment.
    pub token: Address,
    /// Total amount supplied by the payer before fee deduction.
    pub gross_amount: i128,
    /// Protocol fee transferred to the treasury. Zero when fee rate is zero.
    pub fee_amount: i128,
    /// Net amount forwarded to the intended payment recipient.
    pub net_amount: i128,
    /// Treasury address that received `fee_amount`.
    pub fee_recipient: Address,
}

// ---------------------------------------------------------------------------
// Admin config events
// ---------------------------------------------------------------------------

/// Emitted when the admin updates the fee configuration.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeConfigUpdatedEvent {
    /// Admin who performed the update.
    pub admin: Address,
    /// New percentage fee in basis points.
    pub new_fee_bps: u32,
    /// New flat fee amount.
    pub new_flat_fee: i128,
    /// New active fee mode.
    pub new_mode: FeeMode,
}

/// Emitted when the fee recipient (treasury) address is changed.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipientUpdatedEvent {
    /// Admin who performed the update.
    pub admin: Address,
    /// Previous treasury address.
    pub old_recipient: Address,
    /// New treasury address.
    pub new_recipient: Address,
}

/// Emitted when the contract pause state is toggled.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PauseStateChangedEvent {
    /// Admin who toggled the pause state.
    pub admin: Address,
    /// New pause state (`true` = paused, `false` = active).
    pub paused: bool,
}

/// Emitted when admin rights are transferred to a new address.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminTransferredEvent {
    /// Previous admin address.
    pub old_admin: Address,
    /// New admin address.
    pub new_admin: Address,
}
