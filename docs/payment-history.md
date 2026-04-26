# Payment History Contract

The Payment History Contract provides an immutable, indexed ledger of all completed payments within the StelloPay ecosystem. It exposes a stable query surface — keyed by **hash**, **global ID**, agreement, employer, and employee — so off-chain indexers and UI clients can reconstruct full payment histories without recomputing any payroll math.

## Table of Contents

- [Features](#features)
- [Architecture](#architecture)
- [Security Model](#security-model)
- [Interface](#interface)
  - [Initialization](#initialization)
  - [Recording Payments](#recording-payments)
  - [Point Lookups](#point-lookups)
  - [Paginated Queries](#paginated-queries)
- [Pagination Semantics](#pagination-semantics)
- [Events](#events)
- [Storage Key Reference](#storage-key-reference)
- [Indexer Integration Guide](#indexer-integration-guide)
- [Canonical Reconciliation](#canonical-reconciliation)
- [Edge Cases and Known Boundaries](#edge-cases-and-known-boundaries)
- [Test Output](#test-output)
- [Security Notes](#security-notes)

---

## Features

- **Stable History Model** — Every payment is permanently addressable by three dimensions: its sequential **global ID** (u128), its 32-byte **payment hash** (e.g. the Stellar transaction hash), and its position within the **agreement / employer / employee** indices.
- **Immutable Records** — Once written, a `PaymentRecord` is never modified or deleted. Divergence between an off-chain index and on-chain storage is always an indexer artifact.
- **O(1) Hash Lookup** — A reverse-lookup index (`PaymentByHash`) maps any 32-byte hash directly to the corresponding `PaymentRecord` without scanning sequential data.
- **Three Parallel Sequential Indices** — Every payment is simultaneously indexed by Agreement ID, Employer address, and Employee address, enabling O(n) paginated reads per entity.
- **Bounded Page Reads** — Page size is hard-capped at `MAX_PAGE_SIZE` (100) to prevent resource exhaustion.
- **Idempotent Ingestion** — Replaying the same `payment_hash` returns the existing payment ID and does not mutate indices or emit duplicate history events.
- **Event-Driven Indexing** — A `payment_recorded` event carries both `payment_id` and `payment_hash` so indexers can key their tables by either dimension without polling storage.

---

## Architecture

```
Payroll Contract
      │
      │ record_payment(agreement_id, payment_hash, token, amount, from, to, timestamp)
      ▼
PaymentHistory Contract
      │
      ├─ Payment(global_id)               ← canonical record, written once
      ├─ PaymentByHash(hash)              ← reverse-lookup: hash → global_id
      │
      ├─ AgreementPaymentCount(agr_id)    ┐
      ├─ AgreementPayment(agr_id, pos)    ┤ append-only sequential index by agreement
      │                                   ┘
      ├─ EmployerPaymentCount(employer)   ┐
      ├─ EmployerPayment(employer, pos)   ┤ append-only sequential index by employer
      │                                   ┘
      ├─ EmployeePaymentCount(employee)   ┐
      └─ EmployeePayment(employee, pos)   ┘ append-only sequential index by employee
```

The three sequential index families share the same two-level pattern:

1. A `*Count` key records the total and doubles as the highest valid 1-based position.
2. A `*(entity, position)` key maps each 1-based position to a Global Payment ID.

The `PaymentByHash` reverse index provides direct O(1) access to any record whose hash is known, complementing the sequential indices for transaction-level navigation.

All reads ultimately dereference to the canonical `Payment(global_id)` record, which is stored exactly once regardless of how many indices reference it.

---

## Security Model

| Threat | Mitigation |
|---|---|
| Unauthorized injection | `record_payment` calls `payroll_contract.require_auth()`. Only the address registered at `initialize` can write records. |
| History tampering | No `update` or `delete` entry points exist. Records are written once; the contract has no mechanism to overwrite them. |
| Unauthorized pruning | Index counts are monotonically increasing and can only grow. There is no decrement path, so entries cannot be soft-deleted from the pagination range. |
| Re-initialization | `initialize` checks for an existing `Owner` key and panics with "Already initialized" on any subsequent call. |
| Resource exhaustion via large pages | `limit` is silently capped at `MAX_PAGE_SIZE` (100) before any storage read loop executes. |
| Duplicate source event replay | `record_payment` is idempotent by `payment_hash`; duplicate replay returns existing ID and performs no write. |
| ID aliasing | The global counter is incremented and flushed to storage _before_ any index writes, so a partial failure cannot cause two records to share the same ID. |
| Hash integrity | `payment_hash` is stored verbatim from the payroll contract. Its content is not verified on-chain; integrity depends on the trustworthy payroll caller. Indexers should cross-verify the hash against the Stellar ledger if non-repudiation is required. |

### Security Assumptions

- The **payroll contract** address supplied at initialization is itself secure. If the payroll contract is compromised, it can inject arbitrary records. The history contract trusts its caller unconditionally once auth is satisfied.
- Timestamps are supplied by the payroll contract and are not verified against ledger time. Indexers should use the ledger close time for strict ordering and treat `timestamp` as payroll-domain metadata.

---

## Interface

### Initialization

```rust
fn initialize(env: Env, owner: Address, payroll_contract: Address)
```

Must be called exactly once before any other function.

| Parameter | Type | Description |
|---|---|---|
| `owner` | `Address` | Admin address reserved for future governance. |
| `payroll_contract` | `Address` | The **only** address authorized to call `record_payment`. |

Panics: `"Already initialized"` on a second call.

---

### Recording Payments

```rust
fn record_payment(
    env: Env,
    agreement_id: u128,
    payment_hash: BytesN<32>,
    token: Address,
    amount: i128,
    from: Address,
    to: Address,
    timestamp: u64,
) -> u128
```

Restricted to the payroll contract registered at initialization.

| Parameter | Type | Description |
|---|---|---|
| `agreement_id` | `u128` | The employment agreement this payment belongs to. |
| `payment_hash` | `BytesN<32>` | 32-byte reference hash (e.g. Stellar transaction hash) for transaction-level linkage. Stored verbatim and indexed for O(1) reverse lookup. |
| `token` | `Address` | Stellar asset contract address of the transferred token. |
| `amount` | `i128` | Transfer amount in the token's base unit. |
| `from` | `Address` | Employer (payer). |
| `to` | `Address` | Employee (payee). |
| `timestamp` | `u64` | Unix timestamp (seconds) as provided by the payroll contract. |

**Returns:** The existing Global Payment ID for duplicate `payment_hash` input; otherwise the newly assigned Global Payment ID (starts at 1, increments by 1).

**Side effects:**
- For first-seen `payment_hash`: persists a `PaymentRecord` under `Payment(id)`, writes `PaymentByHash(payment_hash)`, appends all three sequential indices, and emits `payment_recorded`.
- For duplicate `payment_hash`: returns existing ID with no storage mutation and no new event.

**Panics:** `"HostError: Error(Auth, InvalidAction)"` if called by any address other than the registered payroll contract.

---

### Point Lookups

```rust
fn get_payment_by_hash(env: Env, payment_hash: BytesN<32>) -> Option<PaymentRecord>
```

O(1) reverse lookup. Returns `None` if the hash has not been recorded.

```rust
fn get_payment_by_id(env: Env, payment_id: u128) -> Option<PaymentRecord>
```

Fetch by Global Payment ID. Returns `None` for IDs not yet assigned (including 0).

```rust
fn get_global_payment_count(env: Env) -> u128
```

Total recorded payments (= highest assigned Global Payment ID). Returns `0` before any payments.

```rust
fn get_agreement_payment_count(env: Env, agreement_id: u128) -> u32
fn get_employer_payment_count(env: Env, employer: Address) -> u32
fn get_employee_payment_count(env: Env, employee: Address) -> u32
```

Per-entity totals. Return `0` if no payments exist for the entity.

---

### Paginated Queries

```rust
fn get_payments_by_agreement(env: Env, agreement_id: u128, start_index: u32, limit: u32) -> Vec<PaymentRecord>
fn get_payments_by_employer(env: Env, employer: Address, start_index: u32, limit: u32) -> Vec<PaymentRecord>
fn get_payments_by_employee(env: Env, employee: Address, start_index: u32, limit: u32) -> Vec<PaymentRecord>
```

All three functions share the same pagination contract:

| Parameter | Description |
|---|---|
| `start_index` | 1-based, inclusive. `0` or values greater than the total count return an empty vector. |
| `limit` | Maximum records to return. Silently capped at `MAX_PAGE_SIZE` (100). |

Returns records in insertion order (oldest first).

---

## Pagination Semantics

Positions are **1-based** and contiguous. To walk a full history in pages of `P`:

```
page 1:  start_index=1,       limit=P
page 2:  start_index=1+P,     limit=P
page 3:  start_index=1+2*P,   limit=P
...
```

Stop when the returned vector length is less than `P` or when `start_index > total_count`.

**Example** — 7 records, page size 3:

| Call | `start_index` | `limit` | Records returned |
|---|---|---|---|
| Page 1 | 1 | 3 | positions 1–3 |
| Page 2 | 4 | 3 | positions 4–6 |
| Page 3 | 7 | 3 | position 7 (partial) |
| Page 4 | 10 | 3 | empty (out of range) |

---

## Events

### `payment_recorded`

Emitted by `record_payment` on every successful recording.

**Topics:**

| Position | Type | Value |
|---|---|---|
| 0 | `Symbol` | `"payment_recorded"` |

**Data** (fields in declaration order):

| Field | Type | Description |
|---|---|---|
| `payment_id` | `u128` | Global Payment ID. Sequential join key for all index queries. |
| `payment_hash` | `BytesN<32>` | 32-byte reference hash. Key for O(1) reverse lookup and transaction-level linkage. |
| `agreement_id` | `u128` | Agreement the payment belongs to. |
| `token` | `Address` | Stellar asset contract address. |
| `amount` | `i128` | Transfer amount in the token's base unit. |
| `from` | `Address` | Employer (payer). |
| `to` | `Address` | Employee (payee). |
| `timestamp` | `u64` | Unix timestamp in seconds. |

---

## Storage Key Reference

> Internal detail for indexer developers who inspect raw ledger state.

| Key | Value type | Description |
|---|---|---|
| `Owner` | `Address` | Contract owner (set at init). |
| `PayrollContract` | `Address` | Sole address allowed to call `record_payment`. |
| `GlobalPaymentCount` | `u128` | Monotonically increasing total; highest assigned ID. |
| `Payment(id)` | `PaymentRecord` | Canonical record. Written once; never updated. |
| `PaymentByHash(hash)` | `u128` | Reverse-lookup: hash → Global Payment ID. Written once per payment. |
| `AgreementPaymentCount(agr_id)` | `u32` | Total payments for this agreement; also highest valid 1-based position. |
| `AgreementPayment(agr_id, pos)` | `u128` | Global Payment ID at the given 1-based position (agreement index). |
| `EmployerPaymentCount(employer)` | `u32` | Total payments from this employer. |
| `EmployerPayment(employer, pos)` | `u128` | Global Payment ID at the given 1-based position (employer index). |
| `EmployeePaymentCount(employee)` | `u32` | Total payments to this employee. |
| `EmployeePayment(employee, pos)` | `u128` | Global Payment ID at the given 1-based position (employee index). |

All storage uses the `Persistent` tier.

---

## Indexer Integration Guide

### Real-time sync via events

Subscribe to `payment_recorded` events emitted by the deployed contract. Each event carries both `payment_id` and `payment_hash`:

```typescript
// Pseudocode — adapt to your SDK
client.on("payment_recorded", (event) => {
  const { payment_id, payment_hash, agreement_id, token, amount, from, to, timestamp } = event.data;
  db.upsert("payments", { payment_id, payment_hash, agreement_id, token, amount, from, to, timestamp });
  db.index("by_hash",      { payment_hash, payment_id });
  db.index("by_agreement", { agreement_id, payment_id });
  db.index("by_employer",  { employer: from, payment_id });
  db.index("by_employee",  { employee: to,   payment_id });
});
```

### Backfill via paginated reads

```typescript
const PAGE = 50;
let start = 1;
while (true) {
  const page = await contract.get_payments_by_agreement(agreement_id, start, PAGE);
  if (page.length === 0) break;
  for (const rec of page) ingest(rec);
  if (page.length < PAGE) break;
  start += PAGE;
}
```

### Transaction-level navigation

If you have a Stellar transaction hash, look up its payment record in a single call:

```typescript
const record = await contract.get_payment_by_hash(txHash);
if (record) displayPayment(record);
```

### Consistency check

After backfilling, verify `get_agreement_payment_count(agreement_id)` matches the number of records ingested. A mismatch indicates a gap in your event stream; re-run the paginated read.

---

## Canonical Reconciliation

Payment-producing contracts expose different event shapes. The canonical reconciliation flow is:

1. Consume source event.
2. Enrich missing fields from source contract read APIs and deployment metadata.
3. Derive `payment_hash` from the Stellar transaction hash.
4. Submit normalized payload to `record_payment`.

### Required source topics and minimum fields

| Source contract | Required event topic | Required event fields | Required enrichment before `record_payment` |
|---|---|---|---|
| `payment_scheduler` | `job_executed` | `job_id`, `amount` | Read `get_job(job_id)` for `from` (`employer`), `to` (`recipient`), and `token`; derive `agreement_id` from scheduler-to-agreement mapping; set `timestamp` from ledger close time; set `payment_hash` from tx hash. |
| `payroll_escrow` | `released` | `agreement_id`, `to`, `amount` | Read `get_agreement_employer(agreement_id)` for `from`; use escrow token from deployment configuration; set `timestamp` from ledger close time; set `payment_hash` from tx hash. |
| `bonus_system` | `incentive_claimed` | `incentive_id`, `employee`, `amount` | Read `get_incentive(incentive_id)` for `from` (`employer`) and `token`; derive `agreement_id` from bonus-to-agreement mapping; set `timestamp` from ledger close time; set `payment_hash` from tx hash. |
| `expense_reimbursement` | `expense_paid` | `expense_id`, `submitter`, `amount` | Read `get_expense(expense_id)` for `token` and payer context; derive canonical `from` and `agreement_id` from reimbursement policy mapping; set `timestamp` from ledger close time; set `payment_hash` from tx hash. |

### Example indexer normalization logic

```typescript
async function reconcileToPaymentHistory(srcEvent) {
  const txHash = srcEvent.txHash; // 32-byte tx hash
  const ts = srcEvent.ledgerCloseTime;

  const normalized = await normalizeSourceEvent(srcEvent);
  // normalized: { agreement_id, token, amount, from, to }

  // Idempotent by payment_hash: duplicate replay returns existing id.
  const paymentId = await paymentHistory.record_payment(
    normalized.agreement_id,
    txHash,
    normalized.token,
    normalized.amount,
    normalized.from,
    normalized.to,
    ts,
  );

  return paymentId;
}
```

### Failure handling

- Duplicate event replay: Safe. `record_payment` returns the existing ID for the same `payment_hash`.
- Out-of-order ingestion: Safe. Use `payment_id` as canonical ingestion order and `timestamp` for domain time.
- Missing events: Re-scan source ledgers from the last durable checkpoint minus a safety window, then replay through `record_payment`.
- Partial history after crash: Resume from checkpoint and replay unconfirmed ledgers; idempotency prevents double writes.

---

## Edge Cases and Known Boundaries

| Case | Behavior |
|---|---|
| `start_index = 0` | Returns empty vector. |
| `start_index > count` | Returns empty vector. |
| `start_index = count` | Returns exactly 1 record (the last one). |
| `limit = 0` | Returns empty vector. |
| `limit > MAX_PAGE_SIZE` | Silently capped; at most 100 records returned. |
| `get_payment_by_id(0)` | Returns `None`; ID 0 is never assigned. |
| `get_payment_by_id(n > global_count)` | Returns `None`. |
| `get_payment_by_hash(unknown)` | Returns `None`. |
| Duplicate `record_payment` with same `payment_hash` | Returns existing ID; no new record, index entry, or event. |
| Out-of-order source event arrival | Accepted; `payment_id` reflects ingestion order while `timestamp` preserves source time. |
| No payments recorded | All count functions return `0`; all queries return empty. |
| Same address as both employer and employee | Both employer and employee indices are updated; the record appears in both. |

---

## Test Output

All 46 tests pass with zero failures. Run with:

```
cargo test -p payment_history -- --nocapture
```

```
running 46 tests
...
test test_event_based_reconciliation_across_payment_sources ... ok
test test_record_payment_duplicate_hash_is_idempotent ... ok
test test_reconciliation_out_of_order_events_are_stable ... ok
...
test result: ok. 46 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

---

## Security Notes

### 1. Unauthorized record injection
`record_payment` calls `payroll_contract.require_auth()` before touching any storage. Any caller that is not the registered payroll contract receives `Error(Auth, InvalidAction)` and the transaction is rolled back with no state change. Validated by `test_record_payment_unauthorized_no_auth`.

### 2. History tampering
There is no `update_payment` or `delete_payment` function. Once a `PaymentRecord` is written under `Payment(id)`, the only operations that can execute against it are reads. The hash index (`PaymentByHash`) is similarly write-once. Validated by `test_records_are_immutable_after_recording`, which confirms every query path returns the original record unchanged after subsequent payments are added.

### 3. Unauthorized pruning
Sequential index counts (`AgreementPaymentCount`, `EmployerPaymentCount`, `EmployeePaymentCount`) can only increase. There is no decrement path. If counts could be decremented, an attacker could cause entries at the tail of the pagination range to become unreachable without removing the underlying records, effectively hiding payment history without a detectable storage deletion. Validated by `test_index_counts_only_increase`.

### 4. Hash–record atomicity
The `PaymentByHash` reverse index and the primary `Payment(id)` record are written in the same `record_payment` invocation. There is no window where one exists without the other. Validated by `test_hash_index_written_atomically`.

### 5. Double-initialization
`initialize` checks for the `Owner` storage key before writing. A second call panics with `"Already initialized"` before modifying any state, so the registered payroll contract address cannot be overwritten by a re-initialization attack. Validated by `test_initialize_double_init_rejected`.

### 6. Payment hash trust boundary
`payment_hash` is stored verbatim from the payroll contract. The history contract does not compute or verify it. This is an intentional design decision: the payroll contract is the trusted caller, and forcing an on-chain hash computation would increase ledger costs without adding verifiability (the payroll contract could still pass any bytes). Indexers that require non-repudiation should cross-verify `payment_hash` against the Stellar Horizon API independently.

### 7. Replay idempotency
Duplicate event replays keyed by the same `payment_hash` return the existing payment ID and do not mutate counts or sequential indices. This prevents replay amplification from creating duplicate ledger history entries. Validated by `test_record_payment_duplicate_hash_is_idempotent`.
