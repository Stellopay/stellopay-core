# Payment Scheduler

Recurring and one-time payment scheduling smart contract for StelloPay's payroll ecosystem, built on [Soroban](https://soroban.stellar.org/) (Stellar).

---

## Overview

The `payment_scheduler` contract manages cron-like payment jobs that automatically transfer tokens from a pre-funded escrow (the contract itself) to recipients at configurable intervals. An off-chain keeper — or any Stellar account — invokes `process_due_payments` to execute all due jobs in a single transaction.

### Key Properties

| Property | Description |
|---|---|
| **Deterministic IDs** | Each job has a SHA-256 `schedule_id` fingerprint derived from its immutable parameters |
| **Idempotency** | Duplicate schedule submissions are rejected at the storage level |
| **State-before-interaction** | Job state is written before token transfers to prevent reentrancy issues |
| **Typed errors** | All error paths return `SchedulerError` enum values, not raw panics |
| **Permissionless processing** | Anyone can call `process_due_payments`; the contract trusts only stored state |

---

## Architecture

```
Employer
  │
  ├─ create_job(...)      ──► PaymentJob { id, schedule_id, status: Active, … }
  ├─ fund_job(...)        ──► Token escrow in scheduler contract
  ├─ pause_job / resume_job
  └─ cancel_job(...)      ──► status: Cancelled

Keeper / Anyone
  └─ process_due_payments(max_jobs)
       │
       ├─ For each due Active job:
       │    ├─ balance >= amount?  ──► transfer tokens, advance schedule, emit job_executed
       │    └─ balance < amount?   ──► compute payment_id, call payment_retry::schedule_retry(...)
       │         └─ emit payment_failed
       └─ return count processed
```

---

## Deterministic Schedule IDs

Every `create_job` call derives a `BytesN<32>` SHA-256 fingerprint:

```
schedule_id = SHA-256(
    employer.to_xdr()      |
    recipient.to_xdr()     |
    token.to_xdr()         |
    amount.to_le_bytes()   |   // 16 bytes, little-endian
    start_time.to_le_bytes()   //  8 bytes, little-endian
)
```

This fingerprint is stored under `StorageKey::ScheduleId(schedule_id)`, mapping to the assigned sequential `job_id`. Two calls with identical parameters are `DuplicateSchedule` errors — the second call never creates a new record or charges gas for storage.

**Off-chain pre-check:** systems can compute the schedule fingerprint locally (same algorithm) and call `get_job_id_by_schedule(schedule_id)` to check for an existing registration before submitting a transaction.

---

## Idempotency Semantics

| Scenario | Behaviour |
|---|---|
| Same `(employer, recipient, token, amount, start_time)` submitted twice | Second `create_job` returns `Err(DuplicateSchedule)` |
| `process_due_payments` called twice in same ledger | Jobs already processed have `next_scheduled_time` in the future; second call is a no-op for them |
| `cancel_job` called twice | Second call returns `Err(AlreadyCancelled)` |
| Completed/Failed job cancellation attempt | Returns `Err(JobNotCancellable)` |

---

## Security Model

- **`initialize`** — one-time only. Returns `Err(AlreadyInitialized)` on replay.
- **`create_job`** — requires `employer.require_auth()`. Sets the job owner.
- **`pause_job` / `resume_job` / `cancel_job`** — caller address is checked against `job.employer`; mismatches return `Err(NotEmployer)`.
- **`fund_job`** — any address may fund (useful for treasury intermediaries), but the funder must authenticate.
- **`process_due_payments`** — **permissionless**. The contract reads all state from persistent storage and validates timestamps independently. Caller identity is irrelevant.
- **State-before-interaction** — in `process_due_payments`, job state (status, counters, timestamp) is written to storage *before* the `token::Client::transfer` call to mitigate reentrancy.
- **No overflow** — job IDs use `checked_add`; counters use `saturating_add`.

### Threat Model Notes

| Threat | Mitigation |
|---|---|
| Replay of `create_job` | Deterministic `schedule_id` idempotency key |
| Cross-employer job control | `job.employer == caller` check in pause/resume/cancel |
| Replay of scheduled fire within window | `next_scheduled_time` gate; only eligible when `now >= next_scheduled_time` |
| Infinite retry fund-lock | `max_retries` cap; `Failed` terminal state |
| Mass-drain via `process_due_payments` | Bounded by `max_jobs` parameter |
| Double-processing (reentrancy) | State written before token transfer |

---

## Public API

### `initialize(env, owner, retry_contract) → Result<(), SchedulerError>`

> @notice Initializes the payment scheduler contract.  
> @dev One-time call. Requires `owner` authentication.  
> @param owner Admin/owner address.  
> @param retry_contract Address of the `payment_retry` contract for handling failures.
> @return `Err(AlreadyInitialized)` if called more than once.

---

### `create_job(env, employer, recipient, token, amount, interval_seconds, start_time, max_executions, max_retries) → Result<u128, SchedulerError>`

> @notice Creates a new recurring or one-time payment job.  
> @dev Derives a deterministic `schedule_id` and uses it as the idempotency key.  
> @param employer Must authenticate. Owns the job.  
> @param recipient Token destination per execution.  
> @param token Token contract address.  
> @param amount Positive token amount per execution cycle.  
> @param interval_seconds Seconds between cycles. Must be > 0 except for `max_executions == Some(1)`.  
> @param start_time First eligible execution timestamp.  
> @param max_executions Optional execution cap (`None` = unlimited).  
> @param max_retries Max insufficient-funds retries before `Failed`.  
> @return Sequential job id.  
> @security Emits `job_created` with `job_id` and `schedule_id`.

**Error cases:**
- `AmountNotPositive` — `amount <= 0`
- `IntervalRequired` — `interval_seconds == 0` for a non-one-time job
- `DuplicateSchedule` — identical parameters already registered

---

### `cancel_job(env, employer, job_id) → Result<(), SchedulerError>`

> @notice Permanently cancels a payment job.  
> @dev Only the original employer may cancel. Terminal states (`Completed`, `Failed`) are not cancellable.  
> @param employer Must authenticate and match `job.employer`.  
> @param job_id Sequential identifier.  
> @security Emits `job_cancelled`. Does not return pre-funded tokens.

**Error cases:**
- `NotEmployer` — caller is not the original employer
- `AlreadyCancelled` — job already cancelled (idempotency guard)
- `JobNotCancellable` — job is in `Completed` or `Failed` status

---

### `pause_job(env, employer, job_id) → Result<(), SchedulerError>`

> @notice Suspends an active job. No transfers occur while paused.  
> @param employer Must authenticate and match `job.employer`.

---

### `resume_job(env, employer, job_id) → Result<(), SchedulerError>`

> @notice Restores a paused job to active status.  
> @param employer Must authenticate and match `job.employer`.

---

### `process_due_payments(env, max_jobs) → u32`

> @notice Permissionless. Executes all due active jobs, up to `max_jobs`.  
> @dev Safe to call repeatedly; `next_scheduled_time` acts as gate.  
> @param max_jobs Upper bound (use 10–50 to stay within ledger limits).  
> @return Number of jobs evaluated.

---

### `fund_job(env, from, job_id, amount) → Result<(), SchedulerError>`

> @notice Deposits tokens into the scheduler's escrow for a given job.  
> @param from Must authenticate. Any address may fund.

---

### `get_job(env, job_id) → Option<PaymentJob>`

> @notice View helper. Returns job record or `None`.

---

### `get_owner(env) → Option<Address>`

> @notice Returns the contract owner address.

---

### `get_job_id_by_schedule(env, schedule_id) → Option<u128>`

> @notice Looks up the sequential job id for a deterministic `schedule_id`.  
> @dev Allows off-chain systems to check for existing schedules without a full scan.

---

## Data Model

### `PaymentJob`

| Field | Type | Description |
|---|---|---|
| `id` | `u128` | Sequential identifier |
| `schedule_id` | `BytesN<32>` | Deterministic SHA-256 fingerprint |
| `employer` | `Address` | Job owner |
| `recipient` | `Address` | Payment destination |
| `token` | `Address` | Token contract |
| `amount` | `i128` | Amount per execution (> 0) |
| `interval_seconds` | `u64` | Seconds between cycles |
| `next_scheduled_time` | `u64` | Earliest eligible execution timestamp |
| `max_executions` | `Option<u32>` | Execution cap (`None` = unlimited) |
| `executions` | `u32` | Successful execution count |
| `max_retries` | `u32` | Max retries on insufficient funds |
| `retry_count` | `u32` | Failed-attempt count |
| `status` | `JobStatus` | `Active | Paused | Failed | Completed | Cancelled` |

### `SchedulerError`

| Code | Variant | Meaning |
|---|---|---|
| 1 | `NotInitialized` | `initialize` not called yet |
| 2 | `AlreadyInitialized` | Re-initialization attempt |
| 3 | `JobNotFound` | No job with given id |
| 4 | `NotEmployer` | Caller ≠ job.employer |
| 5 | `JobNotActive` | Expected `Active` status |
| 6 | `JobNotPaused` | Expected `Paused` status |
| 7 | `AmountNotPositive` | `amount <= 0` |
| 8 | `IntervalRequired` | Zero interval on recurring job |
| 9 | `DuplicateSchedule` | Fingerprint already registered |
| 10 | `AlreadyCancelled` | Job already cancelled |
| 11 | `JobNotCancellable` | Terminal state prevents cancellation |

---

## Events

| Topic | Payload | When |
|---|---|---|
| `("job_created", job_id)` | `JobCreatedEvent { job_id, schedule_id, employer, recipient }` | On `create_job` success |
| `("job_executed", job_id)` | `JobExecutedEvent { job_id, execution_index, amount }` | On successful transfer |
| `("job_failed", job_id)` | `JobFailedEvent { job_id, retry_count, max_retries }` | On insufficient-funds attempt |
| `("job_cancelled", job_id)` | `JobCancelledEvent { job_id, employer }` | On `cancel_job` success |
| `("payment_failed", payment_id)` | `BytesN<32>` | On insufficient-funds; offloaded to retry contract |


---

## Usage Example

```rust
// 1. Initialize (once, by admin)
client.initialize(&admin)?;

// 2. Employer creates a bi-weekly payroll job
let job_id = client.create_job(
    &employer,
    &employee,
    &xlm_token,
    &5_000_0000000i128,    // 5000 XLM (7 decimal places)
    &(14 * 24 * 3600u64),  // every 14 days
    &next_payroll_date,
    &None,                 // unlimited recurring
    &3u32,                 // up to 3 retries on insufficient funds
)?;

// 3. Fund the scheduler's escrow
client.fund_job(&employer, &job_id, &60_000_0000000i128)?; // ~3 months

// 4. Any keeper can trigger processing
let executed = client.process_due_payments(&50u32);

// 5. Employer cancels if needed
client.cancel_job(&employer, &job_id)?;
```

---

## Running Tests

```bash
cd onchain
cargo test -p payment_scheduler -- --nocapture
```

Expected output: all 21 tests pass.

### Test Coverage Summary

| Category | Tests |
|---|---|
| Initialization | `test_initialize_and_read_owner`, `test_double_init_rejected` |
| Job creation | `test_create_job_happy_path`, `test_create_job_zero_amount_rejected`, `test_create_job_zero_interval_recurring_rejected`, `test_create_job_one_time_zero_interval_allowed`, `test_create_job_increments_id` |
| Deterministic IDs | `test_deterministic_schedule_id_consistent`, `test_duplicate_schedule_rejected`, `test_same_timestamp_different_employers_allowed`, `test_different_token_produces_different_schedule_id`, `test_conflict_detection_prevents_duplicates` |
| Cancel | `test_cancel_active_job`, `test_cancel_paused_job`, `test_cancel_already_cancelled_rejected`, `test_cancel_wrong_employer_rejected`, `test_cancel_completed_job_rejected` |
| Pause / Resume | `test_pause_and_resume_job`, `test_pause_non_active_job_rejected`, `test_resume_non_paused_job_rejected` |
| Fund | `test_fund_job_increases_scheduler_balance` |
| Processing | `test_process_no_jobs_returns_zero`, `test_process_max_jobs_bound`, `test_basic_recurring_job_execution`, `test_one_time_payment`, `test_cancelled_job_skipped_by_processor`, `test_insufficient_funds_then_retry_success`, `test_retry_exhaustion_marks_failed` |

---

## Contract Location

- **Contract**: `onchain/contracts/payment_scheduler/src/lib.rs`
- **Tests**: `onchain/contracts/payment_scheduler/tests/test_scheduler.rs`
- **This document**: `docs/payment-scheduler.md`
