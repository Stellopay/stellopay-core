//! # FeeCollector — Protocol Fee Collection Contract
//!
//! This contract collects a configurable protocol fee on each payment and routes
//! it to a designated treasury address (fee recipient). It is designed to be called
//! by other StelloPay contracts (payroll, escrow, bonus) as a composable fee layer.
//!
//! ## Fee Modes
//!
//! | Mode          | Calculation                                       | Config key         |
//! |---------------|---------------------------------------------------|--------------------|
//! | `Percentage`  | `floor(gross × fee_bps / 10 000)`                 | `fee_bps`          |
//! | `Flat`        | fixed amount per payment (capped at gross)        | `flat_fee`         |
//! | `Tiered`      | tier-selected bps applied to gross amount         | `tiered_schedule`  |
//!
//! ### Tiered Mode
//!
//! A [`FeeTier`] list is stored as the `tiered_schedule`. On each `collect_fee`
//! call the contract walks the list in order and selects the first tier whose
//! `limit ≥ gross_amount`; if no tier matches the last tier's `fee_bps` is used
//! as a catch-all. Set the last tier's `limit` to `i128::MAX` to create an
//! explicit open-ended top tier.
//!
//! ## Running-Total Invariant
//!
//! `get_total_fees_collected()` is a monotonically increasing counter:
//!
//! ```text
//! get_total_fees_collected() == Σ fee_amount  for every collect_fee call since deployment
//! ```
//!
//! The counter is **never** reset by [`FeeCollectorContract::update_tiered_schedule`],
//! [`FeeCollectorContract::update_fee_config`], or any other admin operation. This
//! makes it safe to use as an audit trail across schedule changes.
//!
//! ## Security Model
//!
//! * Only the **admin** can change fee config, fee recipient, pause state, or transfer admin
//!   rights.
//! * The admin must call `require_auth()` on every privileged operation.
//! * The percentage fee is hard-capped at [`MAX_FEE_BPS`] (1 000 bps = 10 %).
//! * State counters are updated **before** token transfers (state-before-interaction).
//! * All arithmetic uses `checked_*` to panic on overflow rather than wrap.
//! * The contract can be paused for emergencies; `collect_fee` panics while paused.
//!
//! ## Integration
//!
//! ```ignore
//! // Payer approves this contract for gross_amount before calling.
//! let (net, fee) = fee_collector_client.collect_fee(
//!     &payer,
//!     &payment_recipient,
//!     &token_address,
//!     &gross_amount,
//! );
//! ```

#![no_std]

mod events;
mod helpers;
mod storage;
mod types;

pub use events::{
    AdminProposedEvent, AdminTransferCancelledEvent, AdminTransferredEvent, FeeCollectedEvent,
    FeeConfigUpdatedEvent, PauseStateChangedEvent, RecipientUpdatedEvent,
    TieredScheduleUpdatedEvent,
};
pub use storage::StorageKey;
pub use types::{FeeConfig, FeeMode, FeeQuote, FeeSplit, FeeTier};

use helpers::{
    apply_basis_points, bump_ttl, compute_fee_internal, require_admin, require_initialized,
    require_not_paused,
};
use soroban_sdk::{contract, contractimpl, token, Address, Env};
pub use storage::StorageKey;
pub use types::{FeeConfig, FeeMode, FeeSplit, FeeTier};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum configurable fee rate: 1 000 bps = 10 %.
///
/// This hard cap is a protocol-level safety parameter. It prevents a malicious
/// or compromised admin from draining payer funds by setting an excessive rate.
pub const MAX_FEE_BPS: u32 = 1_000;

/// Basis point denominator: 10 000 bps = 100 %.
pub const BPS_DENOMINATOR: u32 = 10_000;

/// Minimum ledgers remaining before the instance TTL is extended
/// (≈ 30 days at ~5 s/ledger on Stellar mainnet).
///
/// If the remaining TTL is already above this threshold the `extend_ttl` call
/// is a no-op, so bumping on every function call is cheap.
pub const TTL_MIN_LEDGERS: u32 = 518_400;

/// Target ledgers after a TTL extension
/// (≈ 1 year at ~5 s/ledger, safely below the current protocol maximum).
pub const TTL_MAX_LEDGERS: u32 = 6_307_200;

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

/// FeeCollector — collects protocol fees on payments and routes them to a treasury.
///
/// # Lifecycle
///
/// 1. Deploy and call `initialize` once to configure admin, fee recipient, and rate.
/// 2. External contracts call `collect_fee` per payment — the fee is transferred to the treasury
///    and the net amount to the intended recipient.
/// 3. Admin may update config or recipient at any time via privileged methods.
/// 4. Admin may pause `collect_fee` in emergencies via `set_paused`.
#[contract]
pub struct FeeCollectorContract;

// Contract implementation

#[contractimpl]
impl FeeCollectorContract {
    // Lifecycle

    /// Initializes the fee collector contract.
    ///
    /// Must be called exactly once after deployment. The caller (`admin`) becomes
    /// the sole privileged address able to update config, recipient, or pause state.
    ///
    /// # Arguments
    ///
    /// * `env`           — Soroban environment.
    /// * `admin`         — Admin address; must authenticate. Receives full control over fee
    ///   configuration and emergency pause.
    /// * `fee_recipient` — Treasury address that will receive all collected fees.
    /// * `fee_bps`       — Initial percentage fee in basis points (0 – [`MAX_FEE_BPS`]). Pass `0`
    ///   for fee-free operation. Only used when `mode = Percentage`.
    /// * `flat_fee`      — Initial flat fee amount in token units (≥ 0). Only used when `mode =
    ///   Flat`.
    /// * `mode`          — Initial [`FeeMode`].
    ///
    /// # Panics
    ///
    /// * `"Contract already initialized"` — if called a second time.
    /// * `"Fee exceeds maximum allowed (1000 bps)"` — if `fee_bps > MAX_FEE_BPS`.
    /// * `"Flat fee must be non-negative"` — if `flat_fee < 0`.
    pub fn initialize(
        env: Env,
        admin: Address,
        fee_recipient: Address,
        fee_bps: u32,
        flat_fee: i128,
        mode: FeeMode,
    ) {
        admin.require_auth();

        let already_init = env
            .storage()
            .instance()
            .get::<_, bool>(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(!already_init, "Contract already initialized");

        assert!(
            fee_bps <= MAX_FEE_BPS,
            "Fee exceeds maximum allowed (1000 bps)"
        );
        assert!(flat_fee >= 0, "Flat fee must be non-negative");

        env.storage().instance().set(&StorageKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&StorageKey::FeeRecipient, &fee_recipient);
        env.storage().instance().set(&StorageKey::FeeBps, &fee_bps);
        env.storage()
            .instance()
            .set(&StorageKey::FlatFee, &flat_fee);
        env.storage().instance().set(&StorageKey::FeeMode, &mode);
        env.storage()
            .instance()
            .set(&StorageKey::TotalFeesCollected, &0i128);
        env.storage().instance().set(&StorageKey::Paused, &false);
        env.storage()
            .instance()
            .set(&StorageKey::Initialized, &true);
        env.storage().instance().set(
            &StorageKey::TieredSchedule,
            &soroban_sdk::Vec::<FeeTier>::new(&env),
        );

        // Establish initial TTL for the contract instance.
        bump_ttl(&env);
    }

    // Core fee collection

    /// Collects a protocol fee on a gross payment and routes funds to both the treasury
    /// and the intended payment recipient in a single atomic transaction.
    ///
    /// # Flow
    ///
    /// 1. Validates contract state and payer authentication.
    /// 2. Reuses the most recently cached quote for `gross_amount` when present;
    ///    otherwise it computes `fee_amount` and `net_amount` from the live config.
    /// 3. Updates the cumulative `TotalFeesCollected` counter **before** any transfer
    ///    (state-before-interaction pattern).
    /// 4. Transfers `fee_amount` from `payer` to the treasury (if `> 0`).
    /// 5. Transfers `net_amount` from `payer` to `payment_recipient` (if `> 0`).
    /// 6. Emits a [`FeeCollectedEvent`].
    ///
    /// The payer must have pre-approved this contract to spend at least `gross_amount`
    /// of `token` via `token::approve`.
    ///
    /// # Arguments
    ///
    /// * `env`               — Soroban environment.
    /// * `payer`             — Payment originator (must authenticate). Their allowance is spent for
    ///   both the fee and the net payment.
    /// * `payment_recipient` — Address that receives the net payment after fee deduction.
    /// * `token`             — Token contract address for the payment.
    /// * `gross_amount`      — Total payment amount before fee deduction. Must be `> 0`.
    ///
    /// # Returns
    ///
    /// `(net_amount, fee_amount)` — token amounts transferred to recipient and treasury.
    ///
    /// # Panics
    ///
    /// * `"Contract is paused"` — while paused.
    /// * `"Gross amount must be positive"` — if `gross_amount ≤ 0`.
    ///
    /// # Events
    ///
    /// Emits `("fee_collected",)` carrying a [`FeeCollectedEvent`] payload.
    pub fn collect_fee(
        env: Env,
        payer: Address,
        payment_recipient: Address,
        token: Address,
        gross_amount: i128,
    ) -> (i128, i128) {
        require_initialized(&env);
        bump_ttl(&env);
        require_not_paused(&env);
        payer.require_auth();
        assert!(gross_amount > 0, "Gross amount must be positive");

        let fee_recipient: Address = env
            .storage()
            .instance()
            .get(&StorageKey::FeeRecipient)
            .expect("Fee recipient not set");

        let quote: Option<FeeQuote> = env
            .storage()
            .instance()
            .get(&StorageKey::LatestFeeQuote);
        let (fee_amount, net_amount) = match quote {
            Some(stored_quote) if stored_quote.gross_amount == gross_amount => {
                (stored_quote.fee_amount, stored_quote.net_amount)
            }
            _ => {
                let fee_amount = compute_fee_internal(&env, gross_amount);
                let net_amount = gross_amount
                    .checked_sub(fee_amount)
                    .expect("Net amount underflow");
                (fee_amount, net_amount)
            }
        };

        // Update cumulative counter BEFORE any external calls (state-before-interaction).
        if fee_amount > 0 {
            let prev: i128 = env
                .storage()
                .instance()
                .get(&StorageKey::TotalFeesCollected)
                .unwrap_or(0);
            // Saturate rather than panic on overflow — fees cannot reverse.
            let new_total = prev.checked_add(fee_amount).unwrap_or(i128::MAX);
            env.storage()
                .instance()
                .set(&StorageKey::TotalFeesCollected, &new_total);
        }

        let token_client = token::Client::new(&env, &token);

        if fee_amount > 0 {
            // Check for fee split routing
            let split: FeeSplit = env
                .storage()
                .instance()
                .get(&StorageKey::FeeSplit)
                .unwrap_or(FeeSplit::None);
            match split {
                FeeSplit::Treasury(addr) => {
                    token_client.transfer(&payer, &addr, &fee_amount);
                }
                FeeSplit::Burn(addr) => {
                    token_client.transfer(&payer, &addr, &fee_amount);
                }
                FeeSplit::Split(treasury, burn, treasury_bps, _burn_bps) => {
                    let treasury_share = apply_basis_points(fee_amount, treasury_bps);
                    let burn_share = fee_amount - treasury_share;
                    if treasury_share > 0 {
                        token_client.transfer(&payer, &treasury, &treasury_share);
                    }
                    if burn_share > 0 {
                        token_client.transfer(&payer, &burn, &burn_share);
                    }
                }
                FeeSplit::None => {
                    // Default: all fees to single recipient
                    token_client.transfer(&payer, &fee_recipient, &fee_amount);
                }
            }
        }
        if net_amount > 0 {
            token_client.transfer(&payer, &payment_recipient, &net_amount);
        }

        env.events().publish(
            ("fee_collected",),
            FeeCollectedEvent {
                payer,
                token,
                gross_amount,
                fee_amount,
                net_amount,
                fee_recipient,
            },
        );

        (net_amount, fee_amount)
    }

    /// Computes the fee and net amounts for a given gross amount **without** executing
    /// any token transfer or modifying contract state.
    ///
    /// The computed quote is cached for the provided gross amount so a later
    /// `collect_fee` call can settle against the same quote even if the active
    /// config changes in between. A fresh `calculate_fee` call overwrites the
    /// cached quote for that gross amount.
    ///
    /// Use this for UI previews, pre-flight checks, or unit-testing fee arithmetic.
    ///
    /// # Arguments
    ///
    /// * `env`          — Soroban environment.
    /// * `gross_amount` — Hypothetical payment amount. Must be `≥ 0`.
    ///
    /// # Returns
    ///
    /// `(net_amount, fee_amount)` — amounts that *would* be transferred.
    ///
    /// # Panics
    ///
    /// * `"Gross amount must be non-negative"` — if `gross_amount < 0`.
    ///
    /// # Access Control
    /// Read-only — no authentication required.
    pub fn calculate_fee(env: Env, gross_amount: i128) -> (i128, i128) {
        require_initialized(&env);
        bump_ttl(&env);
        assert!(gross_amount >= 0, "Gross amount must be non-negative");
        if gross_amount == 0 {
            let quote = FeeQuote {
                gross_amount: 0,
                fee_amount: 0,
                net_amount: 0,
            };
            env.storage().instance().set(&StorageKey::LatestFeeQuote, &quote);
            return (0, 0);
        }
        let fee_amount = compute_fee_internal(&env, gross_amount);
        let net_amount = gross_amount
            .checked_sub(fee_amount)
            .expect("Net amount underflow");
        let quote = FeeQuote {
            gross_amount,
            fee_amount,
            net_amount,
        };
        env.storage().instance().set(&StorageKey::LatestFeeQuote, &quote);
        (net_amount, fee_amount)
    }

    // Admin configuration

    /// Updates the fee rate, flat fee, and/or fee mode.
    ///
    /// Both `fee_bps` (used when mode = `Percentage`) and `flat_fee` (used when
    /// mode = `Flat`) are stored at all times; only the active mode's value is used
    /// during fee calculation.
    ///
    /// # Arguments
    ///
    /// * `env`          — Soroban environment.
    /// * `admin`        — Current admin (must authenticate).
    /// * `new_fee_bps`  — New percentage fee in basis points (0 – [`MAX_FEE_BPS`]).
    /// * `new_flat_fee` — New flat fee amount (≥ 0).
    /// * `new_mode`     — New active [`FeeMode`].
    ///
    /// # Panics
    ///
    /// * `"Unauthorized: caller is not admin"`.
    /// * `"Fee exceeds maximum allowed (1000 bps)"`.
    /// * `"Flat fee must be non-negative"`.
    ///
    /// # Events
    ///
    /// Emits `("fee_config_updated",)` carrying a [`FeeConfigUpdatedEvent`].
    pub fn update_fee_config(
        env: Env,
        admin: Address,
        new_fee_bps: u32,
        new_flat_fee: i128,
        new_mode: FeeMode,
    ) {
        require_initialized(&env);
        bump_ttl(&env);
        admin.require_auth();
        require_admin(&env, &admin);

        assert!(
            new_fee_bps <= MAX_FEE_BPS,
            "Fee exceeds maximum allowed (1000 bps)"
        );
        assert!(new_flat_fee >= 0, "Flat fee must be non-negative");

        env.storage()
            .instance()
            .set(&StorageKey::FeeBps, &new_fee_bps);
        env.storage()
            .instance()
            .set(&StorageKey::FlatFee, &new_flat_fee);
        env.storage()
            .instance()
            .set(&StorageKey::FeeMode, &new_mode);

        env.events().publish(
            ("fee_config_updated",),
            FeeConfigUpdatedEvent {
                admin,
                new_fee_bps,
                new_flat_fee,
                new_mode,
            },
        );
    }

    /// Updates the tiered fee schedule.
    ///
    /// Replaces the entire tier list atomically. The active fee rate applied to
    /// any subsequent `collect_fee` call is determined by the **new** schedule
    /// immediately after this call returns.
    ///
    /// # Tier Selection Algorithm
    ///
    /// For a given `gross_amount` the contract walks the schedule in order and
    /// selects the **first** tier whose `limit ≥ gross_amount`. If no tier
    /// matches (i.e., `gross_amount` exceeds every listed limit), the last
    /// tier's `fee_bps` is used as a catch-all. Use `limit: i128::MAX` as the
    /// final tier to make the catch-all explicit.
    ///
    /// # Running Total Continuity
    ///
    /// `get_total_fees_collected` is a **monotonically increasing** counter that
    /// is never reset by a schedule change. Fees collected under the old schedule
    /// are preserved; fees collected after the change are additive. The invariant
    ///
    /// ```text
    /// get_total_fees_collected() == Σ fee_amount for every collect_fee call
    /// ```
    ///
    /// holds across any number of `update_tiered_schedule` calls.
    ///
    /// # Arguments
    ///
    /// * `env`          — Soroban environment.
    /// * `admin`        — Current admin (must authenticate).
    /// * `new_schedule` — Ordered list of [`FeeTier`] values. Must be strictly
    ///   increasing by `limit` and each `fee_bps` must be ≤ [`MAX_FEE_BPS`].
    ///
    /// # Panics
    ///
    /// * `"Unauthorized: caller is not admin"` — if `admin` is not the stored admin.
    /// * `"Tier limits must be strictly increasing and positive"` — if any `limit`
    ///   is ≤ the previous tier's limit (or ≤ 0 for the first tier).
    /// * `"Fee in tier exceeds maximum allowed"` — if any `fee_bps > MAX_FEE_BPS`.
    ///
    /// # Events
    ///
    /// Emits `("tiered_schedule_updated",)` carrying a [`TieredScheduleUpdatedEvent`].
    pub fn update_tiered_schedule(
        env: Env,
        admin: Address,
        new_schedule: soroban_sdk::Vec<FeeTier>,
    ) {
        require_initialized(&env);
        bump_ttl(&env);
        admin.require_auth();
        require_admin(&env, &admin);

        let mut prev_limit: i128 = 0;
        for tier in new_schedule.iter() {
            assert!(
                tier.limit > prev_limit,
                "Tier limits must be strictly increasing and positive"
            );
            assert!(
                tier.fee_bps <= MAX_FEE_BPS,
                "Fee in tier exceeds maximum allowed"
            );
            prev_limit = tier.limit;
        }

        env.storage()
            .instance()
            .set(&StorageKey::TieredSchedule, &new_schedule);

        env.events().publish(
            ("tiered_schedule_updated",),
            TieredScheduleUpdatedEvent {
                admin,
                new_schedule,
            },
        );
    }

    /// Updates the fee recipient (treasury) address.
    ///
    /// All future fee collections will be routed to `new_recipient`. Fees already
    /// collected are not affected.
    ///
    /// # Arguments
    ///
    /// * `env`           — Soroban environment.
    /// * `admin`         — Current admin (must authenticate).
    /// * `new_recipient` — New treasury address.
    ///
    /// # Panics
    ///
    /// * `"Unauthorized: caller is not admin"`.
    ///
    /// # Events
    ///
    /// Emits `("recipient_updated",)` carrying a [`RecipientUpdatedEvent`].
    pub fn update_recipient(env: Env, admin: Address, new_recipient: Address) {
        require_initialized(&env);
        bump_ttl(&env);
        admin.require_auth();
        require_admin(&env, &admin);

        let old_recipient: Address = env
            .storage()
            .instance()
            .get(&StorageKey::FeeRecipient)
            .expect("Fee recipient not set");

        env.storage()
            .instance()
            .set(&StorageKey::FeeRecipient, &new_recipient);

        env.events().publish(
            ("recipient_updated",),
            RecipientUpdatedEvent {
                admin,
                old_recipient,
                new_recipient,
            },
        );
    }

    /// Pauses or unpauses protocol fee collection.
    ///
    /// While paused, every call to `collect_fee` panics with `"Contract is paused"`.
    /// All other functions (view, admin config, `calculate_fee`) remain available.
    ///
    /// This is an **emergency mechanism** only — it is the admin's responsibility to
    /// communicate the pause reason to protocol participants.
    ///
    /// # Arguments
    ///
    /// * `env`    — Soroban environment.
    /// * `admin`  — Current admin (must authenticate).
    /// * `paused` — `true` to pause, `false` to resume.
    ///
    /// # Events
    ///
    /// Emits `("pause_state_changed",)` carrying a [`PauseStateChangedEvent`].
    pub fn set_paused(env: Env, admin: Address, paused: bool) {
        require_initialized(&env);
        bump_ttl(&env);
        admin.require_auth();
        require_admin(&env, &admin);

        env.storage().instance().set(&StorageKey::Paused, &paused);

        env.events().publish(
            ("pause_state_changed",),
            PauseStateChangedEvent { admin, paused },
        );
    }

    /// Updates the fee routing split policy.
    ///
    /// When set, collected fees are routed according to the split (e.g., part to treasury,
    /// part to a burn address). When set to `FeeSplit::None`, all fees go to the single
    /// `FeeRecipient`.
    ///
    /// # Arguments
    ///
    /// * `env`   — Soroban environment.
    /// * `admin` — Current admin (must authenticate).
    /// * `split` — New fee split policy, or `FeeSplit::None` to use single-recipient mode.
    ///
    /// # Panics
    ///
    /// * `"Unauthorized: caller is not admin"`.
    /// * `"Split BPS must equal 10000"` — if `treasury_bps + burn_bps != BPS_DENOMINATOR`.
    ///
    /// # Events
    ///
    /// Emits `("fee_split_updated",)`.
    pub fn update_fee_split(env: Env, admin: Address, split: FeeSplit) {
        require_initialized(&env);
        bump_ttl(&env);
        admin.require_auth();
        require_admin(&env, &admin);

        if let FeeSplit::Split(_, _, treasury_bps, burn_bps) = &split {
            assert!(
                treasury_bps + burn_bps == BPS_DENOMINATOR,
                "Split BPS must equal 10000"
            );
        }

        env.storage().instance().set(&StorageKey::FeeSplit, &split);
    }

    /// Proposes a new admin address as the first step of a two-step admin handoff.
    ///
    /// The proposed address is stored as pending. It has **no privileges** until it
    /// explicitly calls [`FeeCollectorContract::accept_admin`]. The current admin
    /// retains full control until acceptance.
    ///
    /// Call [`FeeCollectorContract::cancel_admin_transfer`] at any time to revoke
    /// the pending proposal before it is accepted.
    ///
    /// # Arguments
    ///
    /// * `env`           — Soroban environment.
    /// * `admin`         — Current admin (must authenticate).
    /// * `proposed_admin`— Address being nominated as the new admin.
    ///
    /// # Panics
    ///
    /// * `"Unauthorized: caller is not admin"` — if `admin` is not the current admin.
    ///
    /// # Events
    ///
    /// Emits `("admin_proposed",)` carrying an [`AdminProposedEvent`].
    pub fn propose_admin(env: Env, admin: Address, proposed_admin: Address) {
        require_initialized(&env);
        bump_ttl(&env);
        admin.require_auth();
        require_admin(&env, &admin);

        env.storage()
            .instance()
            .set(&StorageKey::PendingAdmin, &proposed_admin);

        env.events().publish(
            ("admin_proposed",),
            AdminProposedEvent {
                current_admin: admin,
                proposed_admin,
            },
        );
    }

    /// Completes a pending two-step admin handoff.
    ///
    /// Only the address that was proposed via [`FeeCollectorContract::propose_admin`]
    /// can call this. Once accepted, the caller becomes the new admin and the pending
    /// proposal is cleared.
    ///
    /// # Arguments
    ///
    /// * `env`    — Soroban environment.
    /// * `caller` — Must be the pending admin (must authenticate).
    ///
    /// # Panics
    ///
    /// * `"No pending admin transfer"` — if no proposal is outstanding.
    /// * `"Caller is not the proposed admin"` — if `caller` is not the proposed address.
    ///
    /// # Events
    ///
    /// Emits `("admin_transferred",)` carrying an [`AdminTransferredEvent`].
    pub fn accept_admin(env: Env, caller: Address) {
        require_initialized(&env);
        bump_ttl(&env);
        caller.require_auth();

        let pending: Address = env
            .storage()
            .instance()
            .get(&StorageKey::PendingAdmin)
            .expect("No pending admin transfer");

        assert!(caller == pending, "Caller is not the proposed admin");

        let old_admin: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Admin)
            .expect("Admin not set");

        env.storage().instance().set(&StorageKey::Admin, &caller);
        env.storage().instance().remove(&StorageKey::PendingAdmin);

        env.events().publish(
            ("admin_transferred",),
            AdminTransferredEvent {
                old_admin,
                new_admin: caller,
            },
        );
    }

    /// Cancels a pending admin transfer, clearing the proposed address.
    ///
    /// Only the **current admin** can cancel. Has no effect if there is no
    /// outstanding proposal, other than bumping the TTL.
    ///
    /// # Arguments
    ///
    /// * `env`   — Soroban environment.
    /// * `admin` — Current admin (must authenticate).
    ///
    /// # Panics
    ///
    /// * `"Unauthorized: caller is not admin"` — if `admin` is not the current admin.
    /// * `"No pending admin transfer"` — if no proposal is outstanding.
    ///
    /// # Events
    ///
    /// Emits `("admin_transfer_cancelled",)` carrying an [`AdminTransferCancelledEvent`].
    pub fn cancel_admin_transfer(env: Env, admin: Address) {
        require_initialized(&env);
        bump_ttl(&env);
        admin.require_auth();
        require_admin(&env, &admin);

        let pending: Address = env
            .storage()
            .instance()
            .get(&StorageKey::PendingAdmin)
            .expect("No pending admin transfer");

        env.storage().instance().remove(&StorageKey::PendingAdmin);

        env.events().publish(
            ("admin_transfer_cancelled",),
            AdminTransferCancelledEvent {
                admin,
                cancelled_admin: pending,
            },
        );
    }

    // View functions

    /// Returns a snapshot of the current fee configuration.
    ///
    /// Includes recipient, both fee parameters, active mode, and pause state.
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_config(env: Env) -> FeeConfig {
        require_initialized(&env);
        bump_ttl(&env);
        FeeConfig {
            recipient: env
                .storage()
                .instance()
                .get(&StorageKey::FeeRecipient)
                .expect("FeeRecipient not set"),
            fee_bps: env
                .storage()
                .instance()
                .get(&StorageKey::FeeBps)
                .unwrap_or(0),
            flat_fee: env
                .storage()
                .instance()
                .get(&StorageKey::FlatFee)
                .unwrap_or(0),
            mode: env
                .storage()
                .instance()
                .get(&StorageKey::FeeMode)
                .expect("FeeMode not set"),
            tiered_schedule: env
                .storage()
                .instance()
                .get(&StorageKey::TieredSchedule)
                .unwrap_or_else(|| soroban_sdk::Vec::new(&env)),
            paused: env
                .storage()
                .instance()
                .get(&StorageKey::Paused)
                .unwrap_or(false),
            split: env
                .storage()
                .instance()
                .get(&StorageKey::FeeSplit)
                .unwrap_or(FeeSplit::None),
        }
    }

    /// Returns the cumulative total fees collected since initialization.
    ///
    /// This counter saturates at [`i128::MAX`] rather than wrapping on overflow
    /// (an unreachable condition in practice).
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_total_fees_collected(env: Env) -> i128 {
        require_initialized(&env);
        bump_ttl(&env);
        env.storage()
            .instance()
            .get(&StorageKey::TotalFeesCollected)
            .unwrap_or(0)
    }

    /// Returns the current admin address.
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_admin(env: Env) -> Address {
        require_initialized(&env);
        bump_ttl(&env);
        env.storage()
            .instance()
            .get(&StorageKey::Admin)
            .expect("Admin not set")
    }

    /// Returns the pending admin address, if a two-step transfer is in progress.
    ///
    /// Returns `None` when no transfer has been proposed or after it has been
    /// accepted or cancelled.
    ///
    /// # Access Control
    /// Read-only — no authentication required.
    pub fn get_pending_admin(env: Env) -> Option<Address> {
        require_initialized(&env);
        bump_ttl(&env);
        env.storage()
            .instance()
            .get(&StorageKey::PendingAdmin)
    }
}
