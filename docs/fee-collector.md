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

| Mode          | Formula                                     | Config key   |
|---------------|---------------------------------------------|--------------|
| `Percentage`  | `floor(gross × fee_bps / 10 000)`           | `fee_bps`    |
| `Flat`        | fixed amount (capped at `gross_amount`)     | `flat_fee`   |

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

**Floor rounding** is used for percentage fees. This slightly favours payers and is the de-facto standard in on-chain fee arithmetic (integer truncation).

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

### `transfer_admin`

```rust
pub fn transfer_admin(env: Env, admin: Address, new_admin: Address)
```

Admin-only. Immediately transfers admin rights. **One-way, no confirmation step.**  
Use a multi-sig contract as `admin` in production.

**Emits**: `("admin_transferred",)` → `AdminTransferredEvent`

---

### View functions

| Function                    | Returns      | Description                                        |
|-----------------------------|--------------|----------------------------------------------------|
| `get_config(env)`           | `FeeConfig`  | Full config snapshot (recipient, bps, flat, mode, paused) |
| `get_total_fees_collected(env)` | `i128`   | Cumulative fees since initialization               |
| `get_admin(env)`            | `Address`    | Current admin address                              |

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

| Field       | Type      | Description         |
|-------------|-----------|---------------------|
| `old_admin` | `Address` | Previous admin      |
| `new_admin` | `Address` | New admin           |

---

## Security Analysis

### Access Control

| Operation              | Who can call  |
|------------------------|---------------|
| `initialize`           | Anyone (once) |
| `collect_fee`          | Any payer (must have token allowance) |
| `calculate_fee`        | Anyone        |
| `update_fee_config`    | Admin only    |
| `update_recipient`     | Admin only    |
| `set_paused`           | Admin only    |
| `transfer_admin`       | Admin only    |
| View functions         | Anyone        |

### Assumptions & Guarantees

1. **Fee cap** — `MAX_FEE_BPS = 1 000` (10 %) is enforced on every write to `fee_bps`. A compromised admin cannot set a fee above 10 %, limiting the worst-case loss per payment.

2. **Non-negative net** — Percentage fees produce `net < gross` because `fee_bps ≤ 10 000`. Flat fees are capped via `.min(gross_amount)`. `net_amount` is always `≥ 0`.

3. **State-before-interaction** — `TotalFeesCollected` is updated before the token `transfer()` calls. This eliminates any re-entrancy surface on the accounting state (Stellar/Soroban does not support re-entrant contract calls, but the pattern is followed defensively).

4. **Overflow safety** — All arithmetic uses Rust's `checked_mul`, `checked_div`, `checked_sub`, `checked_add`. The cumulative counter saturates at `i128::MAX` instead of panicking.

5. **Initialization guard** — The `Initialized` flag prevents duplicate initialization and the associated risk of re-setting `admin` or `fee_recipient`.

6. **Admin transfer is immediate** — There is no two-step confirmation. Operators should use a multi-sig contract as `admin`. The `AdminTransferredEvent` provides an audit trail.

7. **Pause does not brick payments** — `set_paused` only blocks `collect_fee`. Protocols that use the fee collector should handle the `"Contract is paused"` panic gracefully (e.g., fall back to fee-free payments) if required by their SLA.

### Threat Model

| Threat                              | Mitigation                                      |
|-------------------------------------|-------------------------------------------------|
| Admin sets extreme fee              | `MAX_FEE_BPS` hard cap enforced on every write  |
| Unauthorized config change          | `require_auth` + `require_admin` on every write |
| Re-initialization to hijack admin   | `Initialized` guard panics on second call       |
| Treasury drain via fee manipulation | Fee is always `≤ gross_amount`; overflow-checked|
| Pausing disrupts all payments       | Pause only affects `collect_fee`, not other ops |

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

---

## Building and Testing

```powershell
# From the workspace root (onchain/)
cargo test -p fee_collector

# Build release WASM
cargo build --release --target wasm32-unknown-unknown -p fee_collector
```

### Test Coverage Summary

| Category                       | Tests |
|--------------------------------|-------|
| Initialization                 | 7     |
| `collect_fee` (percentage)     | 7     |
| `collect_fee` (flat)           | 4     |
| `calculate_fee`                | 4     |
| Config update                  | 4     |
| Recipient update               | 3     |
| Pause / unpause                | 3     |
| Admin transfer                 | 3     |
| Cumulative totals              | 3     |
| Collect fee error cases        | 2     |
| `calculate_fee` error cases    | 1     |
| View helpers / edge cases      | 2     |
| **Total**                      | **47**|

---

## Changelog

| Version | Change                                   |
|---------|------------------------------------------|
| 0.0.0   | Initial implementation — percentage and flat fee modes, pause, admin transfer, cumulative totals |
