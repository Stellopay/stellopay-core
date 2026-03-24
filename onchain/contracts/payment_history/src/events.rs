use soroban_sdk::{contractevent, Address, BytesN, Env};

/// Event emitted every time a payment is successfully recorded.
///
/// @notice Off-chain indexers should subscribe to this event to maintain a
/// real-time mirror of the payment history without querying storage on every
/// block. Three fields serve as stable lookup keys for different query paths:
/// `payment_id` (global sequential ID), `payment_hash` (32-byte reference
/// hash for transaction-level linkage), and `agreement_id` (logical grouping).
///
/// @dev Topics: `Symbol("payment_recorded")`
/// Data: all fields below, in declaration order.
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentRecorded {
    /// Globally unique, monotonically increasing payment identifier.
    ///
    /// @dev Starts at 1, increments by 1 per recorded payment. Use this as
    /// the canonical sequential join key when rebuilding paginated indices
    /// off-chain. All three index families (agreement, employer, employee)
    /// dereference to this ID.
    pub payment_id: u128,

    /// 32-byte reference hash for this payment.
    ///
    /// @dev Supplied verbatim by the payroll contract — typically the Stellar
    /// transaction hash of the underlying token transfer. Indexers can use
    /// this to deep-link to the on-chain transaction via Horizon or RPC
    /// without recomputing any payroll math. Also serves as the key for the
    /// `PaymentByHash` reverse-lookup index.
    pub payment_hash: BytesN<32>,

    /// The employment agreement this payment belongs to.
    /// Matches the `agreement_id` stored in the `PaymentRecord`.
    pub agreement_id: u128,

    /// Stellar asset contract address of the token transferred.
    pub token: Address,

    /// Transfer amount in the token's smallest base unit (always positive).
    pub amount: i128,

    /// Employer address (payer / `from` side of the transfer).
    pub from: Address,

    /// Employee address (payee / `to` side of the transfer).
    pub to: Address,

    /// Unix timestamp in seconds, as supplied by the payroll contract.
    /// Used for time-range queries and correlation with ledger close time.
    pub timestamp: u64,
}

/// Publish a `payment_recorded` event to the current ledger's event log.
pub fn emit_payment_recorded(e: &Env, event: PaymentRecorded) {
    event.publish(e);
}
