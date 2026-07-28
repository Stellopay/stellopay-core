# Fee Collector

> **Contract path**: `onchain/contracts/fee_collector/src/lib.rs`  
> **Test path**: `onchain/contracts/fee_collector/tests/test_fees.rs`

## Overview

The `FeeCollector` contract is a composable protocol fee layer for StelloPay. It intercepts payment flows, deducts a configurable fee, and routes it to a designated treasury address. All other StelloPay contracts (payroll, escrow, bonus, etc.) can integrate fee collection by calling a single entry-point without changes to their own logic.

```
┌──────────────────────────────────────────────────────────────────────────┐
│  Payer                                                                   │
│   │  approve(fee_collector, gross_amount)                                │
│   │  collect_fee(payer, recipient, token, gross_amount)                  │
│   ▼                                                                      │
│ FeeCollector ──► treasury  (fee_amount)                                  │
│                ──► recipient (net_amount)                                │
└──────────────────────────────────────────────────────────────────────────┘
```

---

## Fee Modes

| Mode          | Formula                                     | Config key          |
|---------------|---------------------------------------------|---------------------|
| `Percentage`  | `floor(gross × fee_bps / 10 000)`           | `fee_bps`           |
| `Flat`        | fixed amount (capped at `gross_amount`)     | `flat_fee`          |
| `Tiered`      | tier-selected bps applied to gross amount   | `tiered_schedule`   |

### Basis Points Reference

| `fee_bps` | Rate  |
|-----------|-------|
| `0`       | 0 %   |
| `10`      | 0.1 % |
| `50`      | 0.5 % |
| `100`     | 1 %   |
| `250`     | 2.5 % |
| `500`     | 5 %   |
| `1 000`   | 10 %  ← maximum (`MAX_FEE_BPS`) |
**Floor rounding** (truncation towards zero) is consistently used for all percentage fees and fee split routing calculations. This slightly favours payers and is the de-facto standard in on-chain fee arithmetic (integer truncation).

All basis point math in this contract is consolidated into a single helper function, `apply_basis_points(amount, bps)`, which ensures:
- Standard floor rounding applies across all calculation endpoints (both during fee calculation/collection and fee split routing).
- Tie-breaking roundings (such as exactly half `0.5` or `1.5` token fractions) are rounded down (e.g., `3` tokens at `50%` yields a `1` token fee).

### Tiered Mode

When `mode = Tiered`, the fee rate is resolved from a stored `tiered_schedule` (a list of
[`FeeTier`](#feecollector-types) values set via
[`update_tiered_schedule`](#update_tiered_schedule)).

**Tier selection algorithm:**

1. Walk the schedule from the first entry to the last.
2. Select the **first** tier whose `limit ≥ gross_amount`.
3. If no tier matches (all limits are below `gross_amount`), use the last tier's `fee_bps` as the
   catch-all.
4. Apply `floor(gross_amount × selected_fee_bps / 10 000)`.

Use `limit: i128::MAX` for the final tier to create an explicit open-ended top tier.

**Example schedule:**

| Tier | Limit       | `fee_bps` | Rate   |
|------|-------------|-----------|--------|
| 1    | `1 000`     | `500`     | 5 %    |
| 2    | `5 000`     | `250`     | 2.5 %  |
| 3    | `i128::MAX` | `100`     | 1 %    |

A gross amount of `800` → Tier 1 → fee = `floor(800 × 500 / 10 000)` = `40`.  
A gross amount of `3 000` → Tier 2 → fee = `floor(3 000 × 250 / 10 000)` = `75`.  
A gross amount of `10 000` → Tier 3 → fee = `floor(10 000 × 100 / 10 000)` = `100`.

**Running-Total Invariant:**

`get_total_fees_collected()` is a **monotonically increasing** counter that is never reset
by `update_tiered_schedule`, `update_fee_config`, or any other admin operation. After any
number of schedule rotations the following holds exactly:

```
get_total_fees_collected() == Σ fee_amount  for every collect_fee call since deployment
```

---

## Constants

| Constant           | Value   | Meaning                            |
|--------------------|---------|------------------------------------|
| `MAX_FEE_BPS`      | `1 000` | Hard cap on `fee_bps` (10 %)       |
| `BPS_DENOMINATOR`  | `10 000`| 100 % in basis points              |

---

## Storage Layout

| Key                   | Type      | Description                                    |
|-----------------------|-----------|------------------------------------------------|
| `Admin`               | `Address` | Has authority over all privileged operations   |
| `PendingAdmin`        | `Address` | Proposed-but-unaccepted admin (two-step handoff); absent when no transfer is in progress |
| `FeeRecipient`        | `Address` | Treasury that receives collected fees          |
| `FeeBps`              | `u32`     | Percentage fee rate in basis points            |
| `FlatFee`             | `i128`    | Flat fee per payment in token units            |
| `FeeMode`             | `FeeMode` | Currently active fee calculation mode          |
| `TotalFeesCollected`  | `i128`    | Cumulative fee income since initialization     |
| `Paused`              | `bool`    | Emergency pause flag                           |
| `Initialized`         | `bool`    | One-time initialization guard                  |

All entries use **persistent** storage.

---

## Contract API

### `initialize`

```rust
pub fn initialize(
    env: Env,
    admin: Address,        // Must authenticate. Becomes the sole privileged operator.
    fee_recipient: Address,// Treasury that receives fees.
    fee_bps: u32,          // Initial percentage rate (0–MAX_FEE_BPS). Used for Percentage mode.
    flat_fee: i128,        // Initial flat fee (>= 0). Used for Flat mode.
    mode: FeeMode,         // Initial fee mode.
)
```

**Panics**:
- `"Contract already initialized"` — duplicate call.
- `"Fee exceeds maximum allowed (1000 bps)"` — `fee_bps > 1 000`.
- `"Flat fee must be non-negative"` — `flat_fee < 0`.

---

### `collect_fee`

```rust
pub fn collect_fee(
    env: Env,
    payer: Address,             // Payment originator. Must have approved this contract for gross_amount.
    payment_recipient: Address, // Receives the net amount.
    token: Address,             // Token contract address.
    gross_amount: i128,         // Total payment before fee. Must be > 0.
) -> (i128, i128)               // (net_amount, fee_amount)
```

**Flow**:
1. Validates state (initialized, not paused) and payer auth.
2. Computes `fee_amount` and `net_amount`.
3. Updates `TotalFeesCollected` **before** any token transfer (state-before-interaction).
4. Transfers `fee_amount` → treasury (skipped if zero).
5. Transfers `net_amount` → recipient (skipped if zero).
6. Emits `FeeCollectedEvent`.

**Panics**:
- `"Contract is paused"` — while paused.
- `"Gross amount must be positive"` — `gross_amount ≤ 0`.

**Emits**: `("fee_collected",)` → `FeeCollectedEvent`

---

### `calculate_fee`

```rust
pub fn calculate_fee(
    env: Env,
    gross_amount: i128,  // Must be >= 0.
) -> (i128, i128)        // (net_amount, fee_amount)
```

Pure read — no token transfers, no state mutation. Use for UI previews and pre-flight checks.

**Panics**: `"Gross amount must be non-negative"` — `gross_amount < 0`.

---

### `update_fee_config`

```rust
pub fn update_fee_config(
    env: Env,
    admin: Address,
    new_fee_bps: u32,
    new_flat_fee: i128,
    new_mode: FeeMode,
)
```

Admin-only. Applies immediately to all subsequent `collect_fee` calls.

**Emits**: `("fee_config_updated",)` → `FeeConfigUpdatedEvent`

---

### `update_tiered_schedule`

```rust
pub fn update_tiered_schedule(
    env: Env,
    admin: Address,
    new_schedule: Vec<FeeTier>,  // Ordered list of (limit, fee_bps) pairs.
)
```

Admin-only. Replaces the entire tier list atomically. Changes take effect immediately on the
next `collect_fee` call. The running total (`TotalFeesCollected`) is **not reset** — fees
collected under the old schedule are preserved and the counter remains additive across all
schedule changes.

**Validation rules:**
- Tier `limit` values must be strictly increasing and all positive (> 0).
- Each tier's `fee_bps` must be ≤ `MAX_FEE_BPS` (1 000).

**Panics:**
- `"Unauthorized: caller is not admin"` — if `admin` is not the stored admin.
- `"Tier limits must be strictly increasing and positive"` — on invalid limit ordering.
- `"Fee in tier exceeds maximum allowed"` — if any `fee_bps > MAX_FEE_BPS`.

**Emits**: `("tiered_schedule_updated",)` → `TieredScheduleUpdatedEvent`

---

### `update_recipient`

```rust
pub fn update_recipient(env: Env, admin: Address, new_recipient: Address)
```

Admin-only. Changes the treasury address. Future fees go to `new_recipient`.

**Emits**: `("recipient_updated",)` → `RecipientUpdatedEvent`

---

### `set_paused`

```rust
pub fn set_paused(env: Env, admin: Address, paused: bool)
```

Admin-only emergency toggle. While `paused = true`, `collect_fee` panics.  
View functions and admin config functions remain available.

**Emits**: `("pause_state_changed",)` → `PauseStateChangedEvent`

---

### `propose_admin`

```rust
pub fn propose_admin(env: Env, admin: Address, proposed_admin: Address)
```

Admin-only. First step of the two-step admin handoff. Stores `proposed_admin` as the pending admin; it has **no privileges** until it calls `accept_admin`. The current admin retains full control until acceptance.

> Replaces the former single-step `transfer_admin` which is no longer available. Propose/accept eliminates the risk of permanently losing admin access due to a mistyped address.

**Panics**: `"Unauthorized: caller is not admin"` — if caller is not the current admin.

**Emits**: `("admin_proposed",)` → `AdminProposedEvent`

---

### `accept_admin`

```rust
pub fn accept_admin(env: Env, caller: Address)
```

Second step of the admin handoff. Only the exact address nominated by `propose_admin` can call this. On success, `caller` becomes the new admin and the pending proposal is cleared.

**Panics**:
- `"No pending admin transfer"` — no outstanding proposal.
- `"Caller is not the proposed admin"` — `caller` is not the proposed address.

**Emits**: `("admin_transferred",)` → `AdminTransferredEvent`

---

### `cancel_admin_transfer`

```rust
pub fn cancel_admin_transfer(env: Env, admin: Address)
```

Admin-only. Cancels an in-progress proposal, clearing the `PendingAdmin` storage key. The current admin retains all privileges unchanged.

**Panics**:
- `"Unauthorized: caller is not admin"` — if caller is not the current admin.
- `"No pending admin transfer"` — if no proposal is outstanding.

**Emits**: `("admin_transfer_cancelled",)` → `AdminTransferCancelledEvent`

---

### View functions

| Function                        | Returns          | Description                                                   |
|---------------------------------|------------------|---------------------------------------------------------------|
| `get_config(env)`               | `FeeConfig`      | Full config snapshot (recipient, bps, flat, mode, paused)     |
| `get_total_fees_collected(env)` | `i128`           | Cumulative fees since initialization                          |
| `get_admin(env)`                | `Address`        | Current admin address                                         |
| `get_pending_admin(env)`        | `Option<Address>`| Pending proposed admin, or `None` when no transfer is in progress |

---

## Events

All events are published under a single-element tuple topic.

### `FeeCollectedEvent`
Topic: `("fee_collected",)`

| Field          | Type      | Description                              |
|----------------|-----------|------------------------------------------|
| `payer`        | `Address` | Payment originator                       |
| `token`        | `Address` | Token contract                           |
| `gross_amount` | `i128`    | Total amount before fee deduction        |
| `fee_amount`   | `i128`    | Fee sent to treasury                     |
| `net_amount`   | `i128`    | Amount forwarded to payment recipient    |
| `fee_recipient`| `Address` | Treasury that received the fee           |

### `FeeConfigUpdatedEvent`
Topic: `("fee_config_updated",)`

| Field         | Type      | Description         |
|---------------|-----------|---------------------|
| `admin`       | `Address` | Admin who updated   |
| `new_fee_bps` | `u32`     | New percentage rate |
| `new_flat_fee`| `i128`    | New flat fee amount |
| `new_mode`    | `FeeMode` | New active mode     |

### `RecipientUpdatedEvent`
Topic: `("recipient_updated",)`

| Field           | Type      | Description          |
|-----------------|-----------|----------------------|
| `admin`         | `Address` | Admin who updated    |
| `old_recipient` | `Address` | Previous treasury    |
| `new_recipient` | `Address` | New treasury         |

### `PauseStateChangedEvent`
Topic: `("pause_state_changed",)`

| Field    | Type      | Description               |
|----------|-----------|---------------------------|
| `admin`  | `Address` | Admin who toggled pause   |
| `paused` | `bool`    | New pause state           |

### `AdminTransferredEvent`
Topic: `("admin_transferred",)`

Emitted when `accept_admin` is called and the handoff completes.

| Field       | Type      | Description         |
|-------------|-----------|---------------------|
| `old_admin` | `Address` | Previous admin      |
| `new_admin` | `Address` | New admin           |

### `AdminProposedEvent`
Topic: `("admin_proposed",)`

Emitted when `propose_admin` is called.

| Field            | Type      | Description                          |
|------------------|-----------|--------------------------------------|
| `current_admin`  | `Address` | Admin who initiated the proposal     |
| `proposed_admin` | `Address` | Address proposed as the new admin    |

### `AdminTransferCancelledEvent`
Topic: `("admin_transfer_cancelled",)`

Emitted when `cancel_admin_transfer` is called.

| Field             | Type      | Description                                  |
|-------------------|-----------|----------------------------------------------|
| `admin`           | `Address` | Admin who cancelled the transfer             |
| `cancelled_admin` | `Address` | The pending address that was cleared         |

---

## Security Analysis

### Access Control

| Operation                | Who can call  |
|--------------------------|---------------|
| `initialize`             | Anyone (once) |
| `collect_fee`            | Any payer (must have token allowance) |
| `calculate_fee`          | Anyone        |
| `update_fee_config`      | Admin only    |
| `update_recipient`       | Admin only    |
| `set_paused`             | Admin only    |
| `propose_admin`          | Admin only    |
| `accept_admin`           | Pending admin only |
| `cancel_admin_transfer`  | Admin only    |
| View functions           | Anyone        |

### Assumptions & Guarantees

1. **Fee cap** — `MAX_FEE_BPS = 1 000` (10 %) is enforced on every write to `fee_bps`. A compromised admin cannot set a fee above 10 %, limiting the worst-case loss per payment.

2. **Non-negative net** — Percentage fees produce `net < gross` because `fee_bps ≤ 10 000`. Flat fees are capped via `.min(gross_amount)`. `net_amount` is always `≥ 0`.

3. **State-before-interaction** — `TotalFeesCollected` is updated before the token `transfer()` calls. This eliminates any re-entrancy surface on the accounting state (Stellar/Soroban does not support re-entrant contract calls, but the pattern is followed defensively).

4. **Overflow safety** — All arithmetic uses Rust's `checked_mul`, `checked_div`, `checked_sub`, `checked_add`. The cumulative counter saturates at `i128::MAX` instead of panicking.

5. **Initialization guard** — The `Initialized` flag prevents duplicate initialization and the associated risk of re-setting `admin` or `fee_recipient`.

6. **Two-step admin handoff** — Admin transfer requires the proposed address to explicitly accept via `accept_admin`. This prevents permanently locking the contract if a new address is mistyped or unreachable. The current admin retains control until acceptance and can cancel at any time via `cancel_admin_transfer`.

7. **Pause does not brick payments** — `set_paused` only blocks `collect_fee`. Protocols that use the fee collector should handle the `"Contract is paused"` panic gracefully (e.g., fall back to fee-free payments) if required by their SLA.

### Threat Model

| Threat                              | Mitigation                                                        |
|-------------------------------------|-------------------------------------------------------------------|
| Admin sets extreme fee              | `MAX_FEE_BPS` hard cap enforced on every write                    |
| Unauthorized config change          | `require_auth` + `require_admin` on every write                   |
| Re-initialization to hijack admin   | `Initialized` guard panics on second call                         |
| Treasury drain via fee manipulation | Fee is always `≤ gross_amount`; overflow-checked                  |
| Pausing disrupts all payments       | Pause only affects `collect_fee`, not other ops                   |
| Admin locked due to mistyped address | Two-step handoff: proposed address must accept before taking effect |
| Third party hijacks admin proposal  | `accept_admin` checks `caller == PendingAdmin` exactly            |

---

## Integration Guide

### 1. Deploy and initialize

```rust
fee_collector_client.initialize(
    &admin,
    &treasury,
    &100u32,          // 1 % percentage fee
    &0i128,           // flat fee unused
    &FeeMode::Percentage,
);
```

### 2. Integrate into a payment flow

```rust
// Before calling collect_fee, the payer must approve the fee_collector contract:
token_client.approve(&payer, &fee_collector_address, &gross_amount, &expiry_ledger);

// Then route the payment through the fee collector:
let (net, fee) = fee_collector_client.collect_fee(
    &payer,
    &payment_recipient,
    &token_address,
    &gross_amount,
);
```

### 3. Preview the fee off-chain (no auth required)

```rust
let (net, fee) = fee_collector_client.calculate_fee(&gross_amount);
```

### 4. Switch to flat fee mode

```rust
fee_collector_client.update_fee_config(
    &admin,
    &0u32,        // fee_bps unused in Flat mode
    &50i128,      // 50 token units per payment
    &FeeMode::Flat,
);
```

### 5. Emergency pause

```rust
fee_collector_client.set_paused(&admin, &true);
// ... investigate ...
fee_collector_client.set_paused(&admin, &false);
```

### 6. Transfer admin (two-step)

```rust
// Step 1: current admin proposes the new admin
fee_collector_client.propose_admin(&admin, &new_admin);

// Step 2: proposed address accepts (a separate transaction, signed by new_admin)
fee_collector_client.accept_admin(&new_admin);

// Optional: cancel before acceptance
fee_collector_client.cancel_admin_transfer(&admin);
```

---

## Building and Testing

```powershell
# From the workspace root (onchain/)
cargo test -p fee_collector

# Build release WASM
cargo build --release --target wasm32-unknown-unknown -p fee_collector
```

### Test Coverage Summary

| Category                                          | Tests |
|---------------------------------------------------|-------|
| Initialization                                    | 7     |
| `collect_fee` (percentage)                        | 7     |
| `collect_fee` (flat)                              | 4     |
| `calculate_fee`                                   | 4     |
| Config update                                     | 4     |
| Recipient update                                  | 3     |
| Pause / unpause                                   | 3     |
| Two-step admin transfer                           | 13    |
| Cumulative totals                                 | 3     |
| Collect fee error cases                           | 2     |
| `calculate_fee` error cases                       | 1     |
| View helpers / edge cases                         | 2     |
| Rounding tie-breaker                              | 1     |
| Tiered: tier selection & validation               | 9     |
| Tiered: running-total reconciliation              | 5     |
| **Total**                                         | **68**|

The five tiered reconciliation tests in `tests/test_tiered_fees.rs` explicitly verify the
running-total invariant (`get_total_fees_collected() == Σ fee_amount`) by independently
summing each `collect_fee` return value and asserting exact equality after every call.
Schedule changes mid-sequence are also covered, confirming the counter is additive and
never reset by `update_tiered_schedule`.

---

## Changelog

| Version | Change                                                                                      |
|---------|---------------------------------------------------------------------------------------------|
| 0.0.0   | Initial implementation — percentage and flat fee modes, pause, admin transfer, cumulative totals |
| 0.1.0   | Replaced single-step `transfer_admin` with two-step `propose_admin` / `accept_admin` / `cancel_admin_transfer` handoff (issue #919). Added `PendingAdmin` storage key, `AdminProposedEvent`, `AdminTransferCancelledEvent`, and `get_pending_admin` view. |
| 0.2.0   | Added `Tiered` fee mode: `FeeTier` type, `tiered_schedule` storage key, `update_tiered_schedule` entry-point, `TieredScheduleUpdatedEvent`. Expanded doc comments in `lib.rs` with Tier Selection Algorithm and Running-Total Continuity guarantee. Added 14 tests in `test_tiered_fees.rs` including 5 reconciliation tests that assert `get_total_fees_collected() == Σ fee_amount` across all tiers and across `update_tiered_schedule` mid-sequence changes. |
