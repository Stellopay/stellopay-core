# Payment Retry Policy

## Overview

The `payment_retry` contract provides a configurable retry policy for failed token transfers in the StelloPay payroll and escrow system. When a transfer cannot complete (e.g. insufficient escrow balance, frozen token account, or transient ledger errors), the contract records the failure, enforces per-request backoff delays between attempts, and exposes both automated and manual retry entry points.

## Contract Location

- Implementation: `onchain/contracts/payment_retry/src/lib.rs`
- Tests: `onchain/contracts/payment_retry/tests/test_retry.rs`

---

## How It Works

### Payment Request Lifecycle

```
create_payment_request()
        │
        ▼
   [Pending] ─── fund_payment() ──► escrow balance increases
        │
        │  process_due_payments() called when next_retry_at ≤ now
        │
        ├─ escrow balance ≥ amount ──► [Completed] + payment_succeeded event
        │
        └─ escrow balance < amount
               │
               ├─ retry_count ≤ max_retry_attempts
               │      └──► update next_retry_at, emit retry_scheduled
               │
               └─ retry_count > max_retry_attempts ──► [Failed] + payment_failed event
```

Payers can also cancel a `Pending` request at any time via `cancel_payment()`, transitioning it to the terminal `Cancelled` state.

---

## API Reference

### `initialize(owner: Address)`

Initialises the contract. Can only be called once. The `owner` must authenticate.

---

### `create_payment_request(...) -> u128`

Creates a new payment request and returns its unique `payment_id`.

| Parameter             | Type              | Description |
|-----------------------|-------------------|-------------|
| `payer`               | `Address`         | Funds escrow; owns the request (must authenticate) |
| `recipient`           | `Address`         | Primary transfer destination |
| `token`               | `Address`         | Token contract for the payment |
| `amount`              | `i128`            | Positive token amount |
| `max_retry_attempts`  | `u32`             | Max failed attempts before `Failed` state (≤ 100) |
| `retry_intervals`     | `Vec<u64>`        | Per-attempt delays in seconds (required if `max_retry_attempts > 0`) |
| `failure_notifier`    | `Address`         | Included in `PaymentFailedEvent` for alert routing |
| `alternate_payout`    | `Option<Address>` | Optional fallback destination; overrides `recipient` on success |

**Panics**: `"Amount must be positive"`, `"Too many retry attempts"`, `"Retry intervals required when retries are enabled"`, `"Retry interval must be positive"`, `"Retry interval too large"`.

---

### `fund_payment(payer, payment_id, amount)`

Deposits tokens from the payer into this contract's escrow. The payer must pre-approve the contract to spend `amount` of the payment token. Multiple calls are additive.

**Panics**: if `payer` does not match the record owner, or if the request is not `Pending`.

---

### `process_due_payments(max_payments: u32) -> u32`

Processes up to `max_payments` due records in a single call. Designed to be invoked by a permissionless keeper or cron job.

For each eligible `Pending` record (where `now >= next_retry_at`):

- **Escrow balance sufficient**: marks `Completed`, transfers to `alternate_payout` (if set) or `recipient`, emits `payment_succeeded`.
- **Insufficient balance**: increments `retry_count`; if exhausted emits `payment_failed` and marks `Failed`; otherwise schedules the next retry and emits `retry_scheduled`.

Returns the number of records evaluated.

---

### `cancel_payment(payer, payment_id)`

Cancels a `Pending` request. Only the original payer may cancel. Escrowed funds are not automatically returned — the payer should reclaim them externally.

---

### `get_payment(payment_id) -> Option<PaymentRequest>`

Returns the full payment record, or `None` if it does not exist.

---

### `get_owner() -> Option<Address>`

Returns the contract owner address.

---

## Retry Interval Semantics

Each request carries a `retry_intervals: Vec<u64>` list of delays (seconds). The delay before attempt *N* is:

```
delay = retry_intervals[min(N-1, len-1)]
next_retry_at = now + delay
```

This means:

- **Fixed delay** (`[30]`): every retry waits 30 seconds.
- **Stepped backoff** (`[30, 60, 120]`): first retry waits 30 s, second 60 s, third and beyond 120 s.
- **Immediate first retry** is not directly supported — set a small interval (e.g. `[1]`) if desired.

### Examples

| `retry_intervals`    | `max_retry_attempts` | Retry schedule (from t=0) |
|----------------------|----------------------|---------------------------|
| `[30]`               | `3`                  | t=30, t=60, t=90          |
| `[10, 30, 60]`       | `3`                  | t=10, t=40, t=100         |
| `[5]`                | `0`                  | Terminal on first failure  |

---

## Alternate Payout Address

An optional `alternate_payout: Option<Address>` may be specified at creation time. When set, successful transfers are routed to that address instead of `recipient`. This is useful for:

- Routing payroll to a cold wallet if the primary hot wallet is unavailable.
- Redirecting to a treasury address without cancelling and re-creating the request.
- Compliance use cases requiring a different settlement account.

The `alternate_payout` field does not affect failure handling or retry scheduling.

---

## Security Assumptions

### Infinite-retry drain prevention

- `max_retry_attempts` is hard-capped at **100** at the protocol level via `MAX_RETRY_ATTEMPTS`. Requests specifying a higher value are rejected at creation time.
- `retry_count` is incremented only on failed transfer attempts, never on successful ones.
- Each individual retry interval is bounded at **1 year** (`MAX_SINGLE_RETRY_INTERVAL_SECONDS = 31_536_000`), preventing indefinite fund lock-up.
- Terminal records (`Completed`, `Failed`, `Cancelled`) are never re-processed by `process_due_payments`.

### Access control

- Only the original **payer** can fund or cancel their payment request.
- `process_due_payments` is permissionless but bounded by `max_payments`. It cannot create, modify, or cancel requests — it only advances eligible `Pending` records.
- The `failure_notifier` field is for off-chain routing only; it carries no on-chain privileges.

### Idempotency

`process_due_payments` is safe to call multiple times per ledger:

- The `next_retry_at` timestamp gates each record; calls before that time skip the record.
- The `status` field is updated to `Completed` **before** the token transfer is executed (state-before-interaction pattern), preventing double-processing.
- All state mutations are atomic — if the contract panics, the entire transaction reverts and no state is persisted.

---

## Integration with Payroll Completion State

Subscribe to the following events from an off-chain indexer:

| Event topic         | Payload fields | Recommended action |
|---------------------|----------------|--------------------|
| `payment_succeeded` | `payment_id`, `recipient`, `amount` | Mark the corresponding payroll period or escrow milestone as paid |
| `payment_failed`    | `payment_id`, `retry_count`, `max_retry_attempts`, `notifier` | Flag the agreement for manual review; alert the `notifier` address |
| `retry_scheduled`   | `payment_id`, `retry_count`, `next_retry_at` | Update UI with the next expected retry time |
| `payment_created`   | `payment_id`, `payer`, `recipient`, `amount` | Register the request in the payroll ledger |

---

## Usage Example

```rust
// 1. Deploy and initialise
payment_retry_client.initialize(&admin);

// 2. Create a payment request with 3 retries and stepped backoff
let payment_id = payment_retry_client.create_payment_request(
    &employer,
    &employee,
    &xlm_token,
    &salary_amount,
    &3u32,
    &vec![&env, 300u64, 600u64, 1800u64], // 5 min, 10 min, 30 min
    &hr_system_address,
    &None, // no alternate payout
);

// 3. Fund escrow (employer approves token spend first)
payment_retry_client.fund_payment(&employer, &payment_id, &salary_amount);

// 4. Keeper calls this on every cron tick
payment_retry_client.process_due_payments(&50u32);

// 5. On PaymentFailedEvent: human reviews and may cancel
payment_retry_client.cancel_payment(&employer, &payment_id);
```

---

## Constants

| Constant | Value | Description |
|---|---|---|
| `MAX_RETRY_ATTEMPTS` | `100` | Protocol ceiling on `max_retry_attempts` |
| `MAX_RETRY_INTERVALS` | `100` | Maximum entries in `retry_intervals` |
| `MAX_SINGLE_RETRY_INTERVAL_SECONDS` | `31_536_000` | Maximum single retry delay (1 year) |

---

## Testing

Run the full test suite:

```bash
cd onchain
cargo test -p payment_retry
```

Test coverage includes:

- Initialization (happy path, double-init guard)
- `create_payment_request` — happy path, zero amount, missing intervals, cap enforcement, alternate payout
- `fund_payment` — happy path, wrong payer, terminal state guard
- `process_due_payments` — immediate success, retry on insufficient balance, backoff timing, last-interval reuse, terminal failure, alternate payout routing, `max_payments` bound, idempotency
- `cancel_payment` — cancels pending, prevents processing, wrong payer, completed guard
- Security: infinite-retry drain prevention, max_retry_attempts cap enforcement
- View helpers (`get_payment`, `get_owner`)
