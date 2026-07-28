# Payment History Contract

`payment_history` in `onchain/contracts/payment_history` is the immutable
on-chain payment ledger for the StelloPay ecosystem. Every completed payment
is recorded exactly once, assigned a globally unique sequential **Payment ID**,
and simultaneously indexed by its 32-byte **payment hash** so reconciliation
tooling can look up the same record through either key.

---

## Dual-Index Guarantee

Each `record_payment` call writes two storage entries atomically within a
single contract invocation:

| Storage key | Value | Purpose |
|---|---|---|
| `Payment(global_id)` | `PaymentRecord` | Primary record, keyed by sequential ID |
| `PaymentByHash(payment_hash)` | `global_id` | Reverse-lookup index, keyed by hash |

Because both entries are written in the same invocation there is no window
where one exists without the other. After `record_payment` returns:

- `get_payment_by_id(id)` returns `Some(record)`
- `get_payment_by_hash(hash)` returns `Some(record)`
- The two records are **byte-for-byte identical** — they dereference to the
  same `Payment(global_id)` storage slot.

This property is the **dual-index guarantee**. Reconciliation tooling may use
either key and will always receive the same `PaymentRecord`.

### What "identical" means

Both query functions eventually load the canonical `PaymentRecord` from
`StorageKey::Payment(global_id)`. `get_payment_by_hash` first resolves
`PaymentByHash(hash) → global_id` and then fetches `Payment(global_id)`.
`get_payment_by_id` fetches `Payment(global_id)` directly. The two calls
read the same storage slot; the result is structurally and semantically equal.

---

## PaymentRecord Fields

```rust
pub struct PaymentRecord {
    pub id:           u128,        // 1-based, monotonically increasing global ID
    pub agreement_id: u128,        // employment agreement this payment belongs to
    pub payment_hash: BytesN<32>,  // 32-byte reference hash (e.g. Stellar tx hash)
    pub token:        Address,     // Stellar asset contract address
    pub amount:       i128,        // transfer amount in token's base unit
    pub from:         Address,     // employer (payer)
    pub to:           Address,     // employee (payee)
    pub timestamp:    u64,         // unix timestamp (seconds) from payroll contract
}
```

---

## Query API

### Point lookups

```rust
// Look up by sequential ID (O(1)).
fn get_payment_by_id(env: Env, payment_id: u128) -> Option<PaymentRecord>

// Look up by 32-byte hash (O(1) via reverse-lookup index).
fn get_payment_by_hash(env: Env, payment_hash: BytesN<32>) -> Option<PaymentRecord>
```

Both return `None` when the requested key has never been recorded. They never
return a wrong record and never panic on an unknown key.

### Not-found semantics

| Scenario | `get_payment_by_id` result | `get_payment_by_hash` result |
|---|---|---|
| ID / hash recorded | `Some(PaymentRecord)` | `Some(PaymentRecord)` |
| ID 0 (never assigned) | `None` | — |
| ID never assigned | `None` | — |
| Hash never recorded | — | `None` |
| Hash from a different payment | — | `Some` of that payment, not another |

An unknown hash **always** returns `None`. It never returns a record that
belongs to a different payment. This means reconciliation tooling can safely
use either lookup and treat `None` as an unambiguous "not found".

### Paginated index queries

Three additional indices support paginated browsing. All use 1-based
`start_index` and are silently capped at `MAX_PAGE_SIZE = 100` per page.

```rust
fn get_payments_by_agreement(env, agreement_id, start_index, limit) -> Vec<PaymentRecord>
fn get_payments_by_employer(env,  employer,     start_index, limit) -> Vec<PaymentRecord>
fn get_payments_by_employee(env,  employee,     start_index, limit) -> Vec<PaymentRecord>
```

Records returned from these functions are the same `PaymentRecord` values
stored under `Payment(global_id)`. They are consistent with the point-lookup
results.

---

## Pagination

All paginated functions use 1-based inclusive `start_index`.

```
page 1: start_index=1,   limit=20   → positions [1, 20]
page 2: start_index=21,  limit=20   → positions [21, 40]
page 3: start_index=41,  limit=20   → positions [41, 60]
```

`start_index = 0` or `start_index > count` returns an empty vector without
an error. `limit` values above `MAX_PAGE_SIZE` are silently reduced to 100.

---

## Idempotency

If the same `payment_hash` is submitted to `record_payment` more than once,
the contract returns the existing `global_id` without writing any new storage.
The global counter, all index counts, and the stored record are all unchanged.
This makes `record_payment` safe to retry on network failures.

---

## Security Model

| Property | Enforcement |
|---|---|
| Only the registered payroll contract may write | `payroll_contract.require_auth()` inside `record_payment` |
| Initialization is one-time | Second call to `initialize` panics `"Already initialized"` |
| Records are immutable | No update or delete code path exists |
| Index entries are append-only | Counts can only increase; no decrement path |
| Page size is bounded | `limit` silently capped at `MAX_PAGE_SIZE = 100` |
| Duplicate hashes are idempotent | Existing ID returned; no new storage written |

---

## Storage Key Reference

```
Owner                                → Address
PayrollContract                      → Address
GlobalPaymentCount                   → u128   (highest assigned ID)

Payment(global_id)                   → PaymentRecord
PaymentByHash(hash)                  → u128   (global_id for reverse lookup)

AgreementPaymentCount(agreement_id)  → u32
AgreementPayment(agreement_id, pos)  → u128   (global_id at 1-based pos)

EmployerPaymentCount(employer)       → u32
EmployerPayment(employer, pos)       → u128   (global_id at 1-based pos)

EmployeePaymentCount(employee)       → u32
EmployeePayment(employee, pos)       → u128   (global_id at 1-based pos)
```

---

## Events

`record_payment` emits a `payment_recorded` event on every new (non-duplicate)
payment. The event carries both `payment_id` and `payment_hash` so indexers
can maintain either dimension of the dual-index without polling storage.

```
topic:  Symbol("payment_recorded")
data:   payment_id, payment_hash, agreement_id, token, amount, from, to, timestamp
```

---

## Reconciliation Patterns

### Hash-first reconciliation

An indexer that receives a Stellar transaction hash from the network can
retrieve the full `PaymentRecord` in one call:

```rust
let record = client.get_payment_by_hash(tx_hash)?;
// record.id  — use for sequential ordering
// record.*   — full payroll context without recomputing math
```

### ID-first reconciliation

A reconciliation job iterating the global payment log uses IDs:

```rust
let total = client.get_global_payment_count();
for id in 1..=total {
    let record = client.get_payment_by_id(id)?;
    // process record
}
```

### Cross-verification

Because both indices resolve to the same storage slot, a reconciliation tool
can sanity-check its own state by asserting:

```rust
let by_id   = client.get_payment_by_id(id).unwrap();
let by_hash = client.get_payment_by_hash(by_id.payment_hash.clone()).unwrap();
assert_eq!(by_id, by_hash); // always true while the contract invariant holds
```

This cross-check is exercised in the test suite by
`test_index_consistency_hash_and_id_return_identical_fields` and related tests.

---

## Test Coverage (issue #912)

The following tests in
`onchain/contracts/payment_history/tests/test_history.rs` directly verify the
dual-index guarantee:

| Test | What it proves |
|---|---|
| `test_index_consistency_hash_and_id_return_identical_fields` | Both paths return field-for-field identical records for a single payment |
| `test_index_consistency_unknown_hash_returns_none_not_wrong_record` | Unknown hash returns `None`; the real record is not displaced |
| `test_index_consistency_each_hash_resolves_only_to_its_own_record` | Hash A never resolves to payment B; each hash is isolated |
| `test_index_consistency_batch_all_pairs_agree` | Over 8 payments, every (hash, id) pair agrees across both lookup paths |
| `test_index_consistency_duplicate_hash_preserves_original_record` | Replay of a known hash leaves the original record intact under both keys |
| `test_index_consistency_stored_hash_matches_lookup_key` | The `payment_hash` field inside the record matches the key used to query it |
| `test_index_consistency_unknown_hash_returns_none_with_populated_storage` | Unknown hash returns `None` even when other payments are present |
| `test_hash_index_written_atomically` | Reverse-lookup index is written in the same invocation as the primary record |
| `test_same_payment_visible_in_all_five_query_paths` | All five query surfaces return the same record |
