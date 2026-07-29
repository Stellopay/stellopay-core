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
    [payment_scheduler failure]
         │
         ▼
    schedule_retry()
         │
         ▼
    [Scheduled] ─── fund_payment() ──► escrow balance increases
         │
         │  process_retry() called periodically
         │
         ├─ escrow balance ≥ amount ──► [Success] + payment_success event
         │
         └─ escrow balance < amount
                │
                ├─ retry_count ≤ max_retries
                │      └──► [Retrying], update next_retry_at, emit payment_retry_failed
                │
                └─ retry_count > max_retries ──► [Failed] + payment_retry_failed event
```

Payers can also cancel a non-terminal request at any time — **including mid-backoff, while a `next_retry_at` is already armed** — via `cancel_payment()`, which moves it to the terminal `Cancelled` state and atomically refunds the payer's escrow deposit. See [`cancel_payment`](#cancel_paymentpayer-payment_id) and [Cancellation permanence](#cancellation-permanence-cancel-beats-an-armed-backoff) for the full contract.

### Retry states

| State | Terminal? | Meaning |
|---|---|---|
| `Pending` | no | Record exists but is not yet scheduled (reserved). |
| `Scheduled` | no | Created; awaiting its first attempt. |
| `Retrying` | no | At least one attempt failed; a backoff window is armed. |
| `Success` | **yes** | Transfer settled. |
| `Failed` | **yes** | `retry_count` exceeded `max_retry_attempts` — the protocol gave up. |
| `Cancelled` | **yes** | The payer explicitly revoked the request via `cancel_payment`. |

`Cancelled` is distinct from `Failed` so indexers can tell "the protocol gave up" apart from "the payer opted out". Both equally and permanently block further processing. `RetryState::is_terminal()` is the single source of truth used by every guard in the contract.

---

## API Reference

### `initialize(owner: Address)`

Initialises the contract. Can only be called once. The `owner` must authenticate.

---

### `schedule_retry(payment_id, payer, recipient, token, amount, config)`

Schedules a retry for a failed payment. Deterministic `payment_id` prevents duplicates.

| Parameter    | Type           | Description |
|--------------|----------------|-------------|
| `payment_id` | `BytesN<32>`   | Deterministic ID: hash(employer + employee + amount + timestamp) |
| `payer`      | `Address`      | Employer address |
| `recipient`  | `Address`      | Destination address |
| `token`      | `Address`      | Token address |
| `amount`     | `i128`         | Payment amount |
| `config`     | `RetryConfig`  | Retry parameters (max_retries, intervals) |

---

### `process_retry(payment_id: BytesN<32>)`

Attempts to execute a scheduled retry.

- **Idempotency**: checks `processed_payments` map; no-op if already successful.
- **Success**: transfers tokens, marks `Success`, records in `processed_payments`.
- **Failure**: increments `retry_count`; marks `Failed` if limits reached, otherwise updates `next_retry_at`.

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

Cancels a non-terminal request. Only the original payer may cancel (and must authenticate); cancelling a request that has already reached `Success`, `Failed` or `Cancelled` is rejected with `"Payment is already terminal"` (double-cancel is impossible).

Cancellation performs two effects atomically in one call:

1. **Stops processing, permanently.** The request transitions to the terminal `Cancelled` state and is removed from the pending index, so `process_due_payments` and `process_retry` skip it thereafter and the recipient is never paid.
2. **Refunds escrow.** The exact amount the payer deposited for this request via `fund_payment` (tracked per request and accumulated across multiple deposits) is transferred back to the payer. Cancelling an unfunded request refunds nothing.

Because a request can only be cancelled from a non-terminal state, a deposit already paid out to the recipient on success can never also be refunded here, so the same funds are never double-spent, and the refund touches only this request's escrow ledger — deposits belonging to other requests are untouched. A `payment_cancelled` event is emitted carrying the `refunded_amount`.

#### Cancellation permanence: cancel beats an armed backoff

> **Guarantee.** Once a payment's retry is cancelled it can **never** fire again — not even after the ledger timestamp advances past the `next_retry_at` that was already computed and stored by the backoff policy before the cancellation.

**The race being closed.** A record fails one attempt, so the contract arms a backoff by writing `next_retry_at = now + retry_intervals[retry_count - 1]`. The payer then cancels *inside* that window. A keeper transaction that was constructed earlier, or that simply processes late, finally lands after `now >= next_retry_at`. An implementation that gated only on the due timestamp — or that checked the due time before checking terminality — would treat the record as eligible and transfer escrowed funds to the recipient, defeating the cancellation and stealing back a refund the payer had already received.

**How the contract prevents it.** `process_payment_if_due` evaluates guards in a fixed, security-critical order:

```text
1. already_processed(id)?          → untrack, return false      ← cancellation writes this flag
2. state.is_terminal()?            → untrack, return false      ← authoritative check
3. retry_count > max_attempts?     → mark Failed, return
4. now < next_retry_at?            → return false               ← due-time check happens LAST
5. …token interaction…
```

Terminality is resolved at step 2, **before** the due-time comparison at step 4 and before any token client is touched. A stale `next_retry_at` therefore carries no authority once the record is terminal.

Three independent barriers back the guarantee, so no single mistake re-opens the race:

| # | Barrier | Written by `cancel_payment` | Effect |
|---|---|---|---|
| 1 | `PendingPayments` index | id removed via `untrack_pending_payment` | The batch keeper never even iterates the id. |
| 2 | `Processed(id)` flag | set via `mark_processed` | Step 1 short-circuits before the record is read. |
| 3 | `state = Cancelled` | persisted via `write_payment` | Step 2 rejects direct `process_retry(id)` calls. |

Additionally, escrow is zeroed and refunded during cancellation, so even a hypothetical bypass of all three barriers would find **no balance** to pay out — settlement is backed strictly by the request's own `Escrow(id)` ledger entry, never the pooled contract balance.

`next_retry_at` is deliberately **not** cleared on cancellation. It is retained as a historical record of the backoff that had been armed, which is useful for audit and dashboards. Off-chain consumers must therefore gate on `state`, never on `next_retry_at`.

**Related guard:** `fund_payment` also rejects terminal records, so a cancelled request cannot be re-funded back into a payable condition.

---

### `get_payment(payment_id) -> Option<PaymentRequest>`

Returns the full payment record, or `None` if it does not exist.

**Retry field semantics** (relevant for off-chain monitoring dashboards):

| Field | Initial value | On failed attempt | On successful attempt | After terminal state |
|---|---|---|---|---|
| `retry_count` | `0` | Incremented by `1` | **Unchanged** (never incremented) | Retains last value |
| `next_retry_at` | `created_at` | Set to `now + interval_for_retry(...)` | Not modified by success path | Retains last computed value |
| `state` | `Scheduled` | → `Retrying` (if retries remain) / `Failed` (if exhausted) | → `Success` | Terminal (`Success`, `Failed` or `Cancelled`) |

> **Security note**: `retry_count` is only incremented *after* a failed escrow-balance check (state-before-interaction pattern). A successful transfer never bumps the counter, preventing callers from inflating retries to prematurely exhaust the policy.

> **Security note**: a terminal `state` is authoritative and overrides `next_retry_at`. `get_payment` on a cancelled record returns `state = Cancelled` together with the stale armed `next_retry_at`; that timestamp is inert and must never be used by a caller to infer eligibility. Any subsequent `process_due_payments` / `process_retry` call is a guaranteed no-op that moves no funds.

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

## Maximum-Attempt Ceiling

Each payment request carries a `max_retry_attempts` field that defines the **maximum number of failed attempts** before the retry policy is exhausted and the request transitions to the terminal `Failed` state. This ceiling prevents indefinite retry loops that could lock escrow funds or cause unbounded gas consumption.

### How the ceiling works

```
Retry lifecycle (example: max_retry_attempts = 2)

  Attempt 1: retry_count 0 → 1   (1 ≤ 2)  → Retrying, next_retry_at scheduled
  Attempt 2: retry_count 1 → 2   (2 ≤ 2)  → Retrying, next_retry_at scheduled
  Attempt 3: retry_count 2 → 3   (3 > 2)  → Failed (terminal), payment_failed event emitted
  Attempt 4+:                               → No-op (ceiling already enforced)
```

The contract enforces the ceiling at **two checkpoints** inside `process_payment_if_due`:

1. **Pre-attempt guard** — If `retry_count > max_retry_attempts` at the start of processing (e.g. the record was stored before the ceiling was reached), the payment is immediately marked `Failed` without attempting a transfer.
2. **Post-failure guard** — After a failed transfer, `retry_count` is incremented. If the new value exceeds `max_retry_attempts`, the payment transitions to `Failed` and emits `payment_failed` **instead** of scheduling another retry.

### Behaviour after the ceiling

Once the ceiling is reached:

- **No further retries.** Calling `process_retry` or `process_due_payments` for this record is a no-op.
- **`retry_count` is frozen.** It retains the value from the terminal attempt.
- **`payment_failed` event emitted.** The event carries `retry_count`, `max_retry_attempts`, and the `failure_notifier` address for off-chain alert routing.
- **No `retry_scheduled` event.** The terminal attempt does not emit a retry-scheduled event, giving indexers a clear signal that retries have stopped.

### Relation to retry interval sequence

The ceiling interacts with the retry interval sequence (see [Retry Interval Semantics](#retry-interval-semantics)) as follows:

| `max_retry_attempts` | `retry_intervals` | Behaviour |
|---|---|---|
| `0` | `[30]` | First failure → `Failed` immediately (no retry) |
| `1` | `[30]` | First failure → retry after 30s; second failure → `Failed` |
| `3` | `[10, 30, 60]` | Failures 1–3 scheduled at t+10, t+40, t+100; 4th failure → `Failed` |
| `5` | `[300]` | Failures 1–5 each wait 300s; 6th failure → `Failed` |

The interval for attempt *N* is `retry_intervals[min(N-1, len-1)]`. The ceiling caps *N* such that the last interval in the list is reused for all attempts beyond the list length, but never beyond `max_retry_attempts`.

### Protocol cap

`max_retry_attempts` is hard-capped at [`MAX_RETRY_ATTEMPTS`](#constants) (100) at the contract level. Requests specifying a higher value are rejected at creation time. This prevents infinite-retry scenarios that could lock escrow funds indefinitely or facilitate draining via repeated small transfers.

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
- Terminal records (`Success`, `Failed`, `Cancelled`) are never re-processed by `process_due_payments` or `process_retry`, as enforced centrally by `RetryState::is_terminal()`.

### Cancelled-retry execution prevention

- A cancelled record cannot fire after the ledger crosses its previously-armed `next_retry_at`; terminality is checked before the due-time comparison and before any token interaction. See [Cancellation permanence](#cancellation-permanence-cancel-beats-an-armed-backoff).
- Cancellation is single-shot: `cancel_payment` panics on an already-terminal record, so the refund path cannot be replayed to drain escrow.
- `fund_payment` rejects terminal records, so a cancelled request cannot be resurrected into a payable state by depositing new funds.
- Cancelling one record never affects a sibling record's escrow or schedule — refunds are drawn only from the cancelled request's own `Escrow(id)` entry.

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
| `payment_cancelled` | `payment_id`, `payer`, `refunded_amount` | Mark the request as revoked by the payer and reconcile the escrow refund; stop expecting any further retry for this `payment_id` |

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
- **Cancellation permanence** (`test_cancel_mid_backoff_blocks_retry_past_scheduled_time` and friends):
  - cancel mid-backoff, then advance the ledger to exactly `next_retry_at` and far beyond — no transfer, record stays `Cancelled`, `retry_count` frozen
  - cancel before the first attempt, then advance far past the initial due time
  - repeated late keeper wake-ups (12 successive ledger advances) never resurrect the record
  - a cancelled record does not block or starve a live sibling in the same batch
  - double-cancel rejected; funding a cancelled request rejected; non-payer cancel rejected
  - `get_payment` reports the terminal `Cancelled` state, distinct from `Failed`
- `get_payment` — `retry_count` increments per failed attempt, `next_retry_at` updates each retry, successful retry leaves `retry_count` unchanged
- **Maximum-attempt ceiling:**
  - Exhaustion via `process_retry` — verifies `payment_failed` event, no `retry_scheduled` on terminal attempt, no-op after ceiling
  - Exhaustion via `process_due_payments` — verifies batch path honours the ceiling identically
  - State distinguishability — asserts `Failed` is not equal to any other `RetryState` variant
  - Zero max-retries — first failure transitions directly to `Failed` without scheduling
  - Success before exhaustion — ensures ceiling logic does not interfere with successful payments
- Security: infinite-retry drain prevention, max_retry_attempts cap enforcement
- View helpers (`get_payment`, `get_owner`)
