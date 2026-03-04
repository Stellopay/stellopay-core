//! Core data types for the FeeCollector contract.
//!
//! Defines the fee calculation mode and the read-only config snapshot returned
//! by [`crate::FeeCollectorContract::get_config`].

use soroban_sdk::{contracttype, Address};

// ---------------------------------------------------------------------------
// Fee mode
// ---------------------------------------------------------------------------

/// Determines how the protocol fee is calculated on each payment.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FeeMode {
    /// Percentage fee expressed in basis points.
    ///
    /// `fee = floor(gross_amount × fee_bps / 10 000)`
    ///
    /// Floor (truncation) is used because it slightly favours the payer and is
    /// the de-facto standard for on-chain fee arithmetic.
    Percentage,

    /// Fixed flat fee in the token's smallest denomination.
    ///
    /// The fee is automatically capped at `gross_amount` so `net` never goes below 0.
    Flat,
}

// ---------------------------------------------------------------------------
// Config snapshot
// ---------------------------------------------------------------------------

/// Read-only snapshot of the current fee configuration.
///
/// Returned by [`crate::FeeCollectorContract::get_config`].
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeConfig {
    /// Treasury address that receives collected fees.
    pub recipient: Address,
    /// Percentage fee in basis points. Only active when `mode = Percentage`.
    pub fee_bps: u32,
    /// Flat fee amount in token units. Only active when `mode = Flat`.
    pub flat_fee: i128,
    /// Currently active fee mode.
    pub mode: FeeMode,
    /// Whether fee collection is currently paused.
    pub paused: bool,
}
