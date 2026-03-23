//! # PaymentRetry — Payment Retry Policy Contract
//!
//! This contract provides a retry policy for failed token transfers within
//! StelloPay's payroll and escrow system. When a payment transfer cannot be
//! completed (e.g. insufficient escrow balance, token freeze, or account
//! restrictions), the contract records the failure, enforces configurable
//! backoff delays between retry attempts, and exposes a unified retry entry
//! point (`process_due_payments`) that serves as both an automated keeper hook
//! and a manual on-demand trigger.
//!
//! ## Design Overview
//!
//! Payers deposit funds into this contract's escrow. An off-chain keeper (or
//! any caller) invokes `process_due_payments` periodically; each call checks
//! all `Pending` records whose `next_retry_at` has elapsed and attempts the
//! transfer. If the escrow balance is still insufficient the record is
//! rescheduled according to its `retry_intervals` list. Once `retry_count`
//! exceeds `max_retry_attempts` the record transitions to `Failed` and a
//! terminal event is emitted so off-chain systems (payroll completion logic,
//! alerting) can react.
//!
//! ## Retry Interval Semantics
//!
//! Each `PaymentRequest` carries a `retry_intervals: Vec<u64>` list of delays
//! (in seconds). The interval for attempt *N* is `retry_intervals[N-1]` or, if
//! *N* exceeds the list length, the last element is reused. This allows simple
//! fixed-delay policies (`[30]`), stepped policies (`[30, 60, 120]`), and
//! anything in between without requiring on-chain arithmetic.
//!
//! ## Security Model
//!
//! * Only the original **payer** can fund or cancel their own payment request.
//! * `process_due_payments` is permissionless but bounded by `max_payments`
//!   to prevent runaway gas consumption.
//! * `retry_count` is only incremented *after* a failed escrow-balance check,
//!   never on a successful transfer. This prevents a caller from inflating the
//!   counter to prematurely exhaust retries (state-before-interaction pattern).
//! * `max_retry_attempts` is hard-capped at [`MAX_RETRY_ATTEMPTS`] (100) at the
//!   contract level, preventing infinite-retry scenarios that could lock escrow
//!   funds indefinitely or facilitate draining via repeated small transfers.
//! * An optional `alternate_payout` address can be specified at creation time.
//!   When set, successful transfers are routed there instead of `recipient`,
//!   providing a fallback destination (e.g. a cold wallet) without requiring
//!   cancellation and re-creation.
//!
//! ## Idempotency
//!
//! `process_due_payments` is safe to call multiple times within the same ledger
//! period. Each `PaymentRequest` carries its own `next_retry_at` timestamp;
//! calls before that time are no-ops for that record. Because state is written
//! (retry_count incremented, next_retry_at updated) only inside the
//! `escrow_balance < amount` branch—and the completed/failed terminal states
//! are written before the function returns—repeated calls never double-process
//! a record. Callers may therefore invoke this function from cron jobs or
//! keepers without coordination.
//!
//! ## Integration with Payroll Completion State
//!
//! Off-chain payroll systems should subscribe to the events emitted by this
//! contract:
//! * `payment_succeeded` — mark the corresponding payroll period as paid.
//! * `payment_failed`    — flag the agreement for manual review; the funds
//!   remain in escrow until a human operator cancels or re-funds.
//!
//! The `failure_notifier` address stored in each record is included in the
//! `PaymentFailedEvent` so indexers can route the alert to the correct employer.

#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Vec};

#[contract]
pub struct PaymentRetryContract;

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// Lifecycle state of a payment request.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaymentStatus {
    /// Awaiting a successful transfer attempt; eligible for retry.
    Pending,
    /// Transfer completed successfully; record is terminal.
    Completed,
    /// `retry_count` exceeded `max_retry_attempts`; record is terminal.
    Failed,
    /// Cancelled by the payer before completion; record is terminal.
    Cancelled,
}

/// A payment request with embedded retry policy and optional alternate payout.
///
/// # Idempotency note
///
/// All fields that change during retries (`retry_count`, `next_retry_at`,
/// `status`) are updated atomically in a single `write_payment` call.
/// `process_due_payments` skips records that are not `Pending` or whose
/// `next_retry_at` has not yet elapsed, making repeated calls safe.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentRequest {
    /// Unique sequential identifier assigned at creation.
    pub id: u128,
    /// Address that funded escrow and owns this request.
    pub payer: Address,
    /// Primary destination for the transfer.
    pub recipient: Address,
    /// Token contract used for the transfer.
    pub token: Address,
    /// Token amount to transfer on success.
    pub amount: i128,
    /// Ledger timestamp when this request was created.
    pub created_at: u64,
    /// Earliest ledger timestamp at which the next attempt is eligible.
    /// Zero means the request is immediately eligible.
    pub next_retry_at: u64,
    /// Number of failed attempts so far (incremented on each failed transfer).
    pub retry_count: u32,
    /// Maximum number of failed attempts before the request is marked `Failed`.
    pub max_retry_attempts: u32,
    /// Per-attempt delay list (seconds). Attempt N uses index N-1 or the last
    /// element when N exceeds the list length.
    pub retry_intervals: Vec<u64>,
    /// Address included in `PaymentFailedEvent` for off-chain alert routing.
    pub failure_notifier: Address,
    /// Lifecycle state.
    pub status: PaymentStatus,
    /// Optional fallback destination. When `Some`, successful transfers are
    /// sent here instead of `recipient`. Useful for routing to a cold wallet
    /// or alternative account without cancelling the original request.
    pub alternate_payout: Option<Address>,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
enum StorageKey {
    Initialized,
    Owner,
    NextPaymentId,
    Payment(u128),
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Emitted when a new payment request is created.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentCreatedEvent {
    pub payment_id: u128,
    pub payer: Address,
    pub recipient: Address,
    pub amount: i128,
}

/// Emitted when a retry is scheduled after a failed transfer attempt.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetryScheduledEvent {
    pub payment_id: u128,
    pub retry_count: u32,
    pub next_retry_at: u64,
}

/// Emitted when a transfer succeeds (first attempt or a retry).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentSucceededEvent {
    pub payment_id: u128,
    /// Actual destination address (may be `alternate_payout` if set).
    pub recipient: Address,
    pub amount: i128,
}

/// Emitted when `retry_count` exceeds `max_retry_attempts` and the request
/// transitions to the terminal `Failed` state.
///
/// Off-chain payroll systems should treat this as a signal to mark the
/// corresponding payroll period as unpaid and trigger human review.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentFailedEvent {
    pub payment_id: u128,
    pub retry_count: u32,
    pub max_retry_attempts: u32,
    /// Copied from the request so indexers can route the alert without an
    /// additional contract read.
    pub notifier: Address,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Hard cap on `max_retry_attempts` per payment request.
///
/// Prevents indefinite fund lock-up and ensures escrow accounts can always be
/// drained (either via success or cancellation) within a bounded timeframe.
pub const MAX_RETRY_ATTEMPTS: u32 = 100;

/// Hard cap on the number of entries in `retry_intervals`.
pub const MAX_RETRY_INTERVALS: u32 = 100;

/// Upper bound on a single retry interval (1 year in seconds).
///
/// Prevents a misconfigured request from locking escrow funds for an
/// impractical duration.
pub const MAX_SINGLE_RETRY_INTERVAL_SECONDS: u64 = 31_536_000;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn require_initialized(env: &Env) {
    let initialized = env
        .storage()
        .persistent()
        .get::<_, bool>(&StorageKey::Initialized)
        .unwrap_or(false);
    assert!(initialized, "Contract not initialized");
}

fn read_payment(env: &Env, payment_id: u128) -> PaymentRequest {
    env.storage()
        .persistent()
        .get::<_, PaymentRequest>(&StorageKey::Payment(payment_id))
        .expect("Payment not found")
}

fn write_payment(env: &Env, payment: &PaymentRequest) {
    env.storage()
        .persistent()
        .set(&StorageKey::Payment(payment.id), payment);
}

/// Atomically increments and returns the next payment ID.
fn next_payment_id(env: &Env) -> u128 {
    let current = env
        .storage()
        .persistent()
        .get::<_, u128>(&StorageKey::NextPaymentId)
        .unwrap_or(0);
    let next = current.checked_add(1).expect("Payment id overflow");
    env.storage()
        .persistent()
        .set(&StorageKey::NextPaymentId, &next);
    next
}

/// Validates that `max_retry_attempts` and `retry_intervals` satisfy protocol
/// constraints. Called at creation time; never called during retry processing.
fn validate_retry_configuration(max_retry_attempts: u32, retry_intervals: &Vec<u64>) {
    assert!(
        max_retry_attempts <= MAX_RETRY_ATTEMPTS,
        "Too many retry attempts"
    );
    assert!(
        retry_intervals.len() <= MAX_RETRY_INTERVALS,
        "Too many retry intervals"
    );

    if max_retry_attempts > 0 {
        assert!(
            retry_intervals.len() > 0,
            "Retry intervals required when retries are enabled"
        );
    }

    let mut i: u32 = 0;
    while i < retry_intervals.len() {
        let interval = retry_intervals.get(i).expect("Retry interval missing");
        assert!(interval > 0, "Retry interval must be positive");
        assert!(
            interval <= MAX_SINGLE_RETRY_INTERVAL_SECONDS,
            "Retry interval too large"
        );
        i = i.saturating_add(1);
    }
}

/// Returns the delay (seconds) to apply before retry attempt number
/// `retry_count`. Uses index `retry_count - 1`, clamped to the last element.
///
/// Returns `0` for an empty interval list (immediate retry / no-retry policy).
fn interval_for_retry(retry_intervals: &Vec<u64>, retry_count: u32) -> u64 {
    if retry_intervals.is_empty() {
        return 0;
    }

    let mut index = retry_count.saturating_sub(1);
    let max_index = retry_intervals.len().saturating_sub(1);
    if index > max_index {
        index = max_index;
    }

    retry_intervals.get(index).expect("Retry interval missing")
}

// ---------------------------------------------------------------------------
// Contract implementation
// ---------------------------------------------------------------------------

#[contractimpl]
impl PaymentRetryContract {
    /// Initializes the payment retry contract.
    ///
    /// # Arguments
    ///
    /// * `env`   — Soroban environment.
    /// * `owner` — Administrative owner address (must authenticate). The owner
    ///             address is stored for informational purposes; no privileged
    ///             operations are currently gated on it beyond initialization.
    ///
    /// # Panics
    ///
    /// * `"Contract already initialized"` — if called a second time.
    ///
    /// # Access Control
    ///
    /// Requires `owner` authentication.
    pub fn initialize(env: Env, owner: Address) {
        owner.require_auth();

        let initialized = env
            .storage()
            .persistent()
            .get::<_, bool>(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(!initialized, "Contract already initialized");

        env.storage().persistent().set(&StorageKey::Owner, &owner);
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);
    }

    /// Creates a payment request with a custom retry policy and optional
    /// alternate payout address.
    ///
    /// The payer must subsequently call `fund_payment` to deposit tokens into
    /// escrow before the first `process_due_payments` call, otherwise the first
    /// attempt will fail and consume one retry slot.
    ///
    /// # Arguments
    ///
    /// * `env`                — Soroban environment.
    /// * `payer`              — Address that funds escrow and owns this request
    ///                          (must authenticate).
    /// * `recipient`          — Primary destination address.
    /// * `token`              — Token contract for the transfer.
    /// * `amount`             — Positive token amount to transfer.
    /// * `max_retry_attempts` — Max failed attempts before terminal `Failed`
    ///                          state. Capped at [`MAX_RETRY_ATTEMPTS`].
    /// * `retry_intervals`    — Ordered list of per-attempt delays (seconds).
    ///                          Required when `max_retry_attempts > 0`.
    /// * `failure_notifier`   — Address included in `PaymentFailedEvent` for
    ///                          off-chain alert routing.
    /// * `alternate_payout`   — Optional fallback destination. When `Some`,
    ///                          successful transfers go here instead of
    ///                          `recipient`.
    ///
    /// # Returns
    ///
    /// The newly assigned `payment_id`.
    ///
    /// # Panics
    ///
    /// * `"Amount must be positive"` — if `amount ≤ 0`.
    /// * `"Too many retry attempts"` — if `max_retry_attempts > MAX_RETRY_ATTEMPTS`.
    /// * `"Retry intervals required when retries are enabled"` — if
    ///   `max_retry_attempts > 0` and `retry_intervals` is empty.
    /// * `"Retry interval must be positive"` / `"Retry interval too large"`.
    ///
    /// # Events
    ///
    /// Emits `("payment_created", payment_id)` carrying a [`PaymentCreatedEvent`].
    pub fn create_payment_request(
        env: Env,
        payer: Address,
        recipient: Address,
        token: Address,
        amount: i128,
        max_retry_attempts: u32,
        retry_intervals: Vec<u64>,
        failure_notifier: Address,
        alternate_payout: Option<Address>,
    ) -> u128 {
        require_initialized(&env);
        payer.require_auth();
        assert!(amount > 0, "Amount must be positive");
        validate_retry_configuration(max_retry_attempts, &retry_intervals);

        let payment_id = next_payment_id(&env);
        let now = env.ledger().timestamp();

        let payment = PaymentRequest {
            id: payment_id,
            payer: payer.clone(),
            recipient: recipient.clone(),
            token,
            amount,
            created_at: now,
            next_retry_at: now,
            retry_count: 0,
            max_retry_attempts,
            retry_intervals,
            failure_notifier,
            status: PaymentStatus::Pending,
            alternate_payout,
        };

        write_payment(&env, &payment);

        env.events().publish(
            ("payment_created", payment_id),
            PaymentCreatedEvent {
                payment_id,
                payer,
                recipient,
                amount,
            },
        );

        payment_id
    }

    /// Deposits tokens from the payer into this contract's escrow.
    ///
    /// The payer must have approved this contract to spend at least `amount`
    /// of the payment's token before calling. Multiple calls are allowed; the
    /// escrow balance accumulates.
    ///
    /// # Arguments
    ///
    /// * `env`        — Soroban environment.
    /// * `payer`      — Must match the request's `payer` (must authenticate).
    /// * `payment_id` — Target payment request.
    /// * `amount`     — Positive token amount to deposit.
    ///
    /// # Panics
    ///
    /// * `"Only payer can fund payment"` — if `payer` does not match the record.
    /// * `"Payment is not pending"` — if the request is in a terminal state.
    ///
    /// # Access Control
    ///
    /// Requires `payer` authentication.
    pub fn fund_payment(env: Env, payer: Address, payment_id: u128, amount: i128) {
        require_initialized(&env);
        payer.require_auth();
        assert!(amount > 0, "Amount must be positive");

        let payment = read_payment(&env, payment_id);
        assert!(payment.payer == payer, "Only payer can fund payment");
        assert!(
            payment.status == PaymentStatus::Pending,
            "Payment is not pending"
        );

        let token_client = token::Client::new(&env, &payment.token);
        token_client.transfer(&payer, &env.current_contract_address(), &amount);
    }

    /// Processes up to `max_payments` due payment requests in a single call.
    ///
    /// For each `Pending` record whose `next_retry_at ≤ now`:
    /// * If the escrow balance covers `amount`: transfer succeeds →
    ///   `status = Completed`, emit `payment_succeeded`.
    /// * If the escrow balance is insufficient:
    ///   - Increment `retry_count`.
    ///   - If `retry_count > max_retry_attempts`: `status = Failed`,
    ///     emit `payment_failed`.
    ///   - Otherwise: compute `next_retry_at` and emit `retry_scheduled`.
    ///
    /// Transfers route to `alternate_payout` when set, otherwise `recipient`.
    ///
    /// # Idempotency
    ///
    /// Safe to call multiple times per ledger. Each record's `next_retry_at`
    /// acts as a gate; records already processed in the same time window are
    /// skipped. Terminal records (`Completed`, `Failed`, `Cancelled`) are never
    /// re-processed.
    ///
    /// # Arguments
    ///
    /// * `env`          — Soroban environment.
    /// * `max_payments` — Upper bound on records processed. Pass a small value
    ///                    (e.g. 20–50) to stay within ledger resource limits.
    ///
    /// # Returns
    ///
    /// Number of payment records actually evaluated (not necessarily
    /// transferred) in this call.
    pub fn process_due_payments(env: Env, max_payments: u32) -> u32 {
        require_initialized(&env);

        if max_payments == 0 {
            return 0;
        }

        let now = env.ledger().timestamp();
        let mut processed = 0u32;

        let highest_id = env
            .storage()
            .persistent()
            .get::<_, u128>(&StorageKey::NextPaymentId)
            .unwrap_or(0);

        if highest_id == 0 {
            return 0;
        }

        let mut payment_id = 1u128;
        while payment_id <= highest_id && processed < max_payments {
            if let Some(mut payment) = env
                .storage()
                .persistent()
                .get::<_, PaymentRequest>(&StorageKey::Payment(payment_id))
            {
                if payment.status == PaymentStatus::Pending && now >= payment.next_retry_at {
                    let token_client = token::Client::new(&env, &payment.token);
                    let escrow_balance = token_client.balance(&env.current_contract_address());

                    if escrow_balance >= payment.amount {
                        // Determine the effective destination: alternate if provided.
                        let destination = payment
                            .alternate_payout
                            .clone()
                            .unwrap_or(payment.recipient.clone());

                        // State-before-interaction: mark Completed before transferring.
                        payment.status = PaymentStatus::Completed;
                        write_payment(&env, &payment);

                        token_client.transfer(
                            &env.current_contract_address(),
                            &destination,
                            &payment.amount,
                        );

                        env.events().publish(
                            ("payment_succeeded", payment.id),
                            PaymentSucceededEvent {
                                payment_id: payment.id,
                                recipient: destination,
                                amount: payment.amount,
                            },
                        );
                    } else {
                        payment.retry_count = payment.retry_count.saturating_add(1);

                        if payment.retry_count > payment.max_retry_attempts {
                            payment.status = PaymentStatus::Failed;
                            write_payment(&env, &payment);

                            env.events().publish(
                                ("payment_failed", payment.id),
                                PaymentFailedEvent {
                                    payment_id: payment.id,
                                    retry_count: payment.retry_count,
                                    max_retry_attempts: payment.max_retry_attempts,
                                    notifier: payment.failure_notifier,
                                },
                            );
                        } else {
                            let retry_interval =
                                interval_for_retry(&payment.retry_intervals, payment.retry_count);
                            payment.next_retry_at = now.saturating_add(retry_interval);
                            write_payment(&env, &payment);

                            env.events().publish(
                                ("retry_scheduled", payment.id),
                                RetryScheduledEvent {
                                    payment_id: payment.id,
                                    retry_count: payment.retry_count,
                                    next_retry_at: payment.next_retry_at,
                                },
                            );
                        }
                    }

                    processed = processed.saturating_add(1);
                }
            }

            payment_id = payment_id.saturating_add(1);
        }

        processed
    }

    /// Cancels a `Pending` payment request, preventing any future processing.
    ///
    /// The payer should separately reclaim escrow funds by withdrawing the
    /// deposited tokens. (Fund withdrawal is out of scope for this contract;
    /// the payer should not deposit more than one request's worth of tokens
    /// per escrow account, or use a dedicated escrow contract.)
    ///
    /// # Arguments
    ///
    /// * `env`        — Soroban environment.
    /// * `payer`      — Must match the request's `payer` (must authenticate).
    /// * `payment_id` — Request to cancel.
    ///
    /// # Panics
    ///
    /// * `"Only payer can cancel payment"` — if `payer` does not match.
    /// * `"Payment is not pending"` — if the request is already terminal.
    ///
    /// # Access Control
    ///
    /// Requires `payer` authentication.
    pub fn cancel_payment(env: Env, payer: Address, payment_id: u128) {
        require_initialized(&env);
        payer.require_auth();

        let mut payment = read_payment(&env, payment_id);
        assert!(payment.payer == payer, "Only payer can cancel payment");
        assert!(
            payment.status == PaymentStatus::Pending,
            "Payment is not pending"
        );

        payment.status = PaymentStatus::Cancelled;
        write_payment(&env, &payment);
    }

    /// Returns a payment request by ID, or `None` if it does not exist.
    ///
    /// # Arguments
    ///
    /// * `env`        — Soroban environment.
    /// * `payment_id` — Request identifier.
    pub fn get_payment(env: Env, payment_id: u128) -> Option<PaymentRequest> {
        env.storage()
            .persistent()
            .get::<_, PaymentRequest>(&StorageKey::Payment(payment_id))
    }

    /// Returns the contract owner address, or `None` before initialization.
    pub fn get_owner(env: Env) -> Option<Address> {
        env.storage()
            .persistent()
            .get::<_, Address>(&StorageKey::Owner)
    }
}
