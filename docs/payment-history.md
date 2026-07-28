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

Both query functions load the canonical `PaymentRecord` from
`StorageKey::Payment(global_id)`. `get_payment_by_hash` first resolves
`PaymentByHash(hash) → global_id`, then fetches `Payment(global_id)`.
`get_payment_by_id` fetches `Payment(global_id)` directly. The result is
structurally and semantically equal in both cases.

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
fn get_payment_by_id(env: Env, payment_id: u128) -> Option<PaymentRecord>
fn get_payment_by_hash(env: Env, payment_hash: BytesN<32>) -> Option<PaymentRecord>
```

Both return `None` for unknown keys. They never return a wrong record and
never panic on a missing key.

| Scenario | `get_payment_by_id` | `get_payment_by_hash` |
|---|---|---|
| Key recorded | `Some(PaymentRecord)` | `Some(PaymentRecord)` |
| ID 0 / ID never assigned | `None` | — |
| Hash never recorded | — | `None` |

### Paginated index queries

Three indices support paginated browsing. All use 1-based `start_index` and
are silently capped at `MAX_PAGE_SIZE = 100` per page.

```rust
fn get_payments_by_agreement(env, agreement_id, start_index, limit) -> Vec<PaymentRecord>
fn get_payments_by_employer(env, employer, start_index, limit) -> Vec<PaymentRecord>
fn get_payments_by_employee(env, employee, start_index, limit) -> Vec<PaymentRecord>
```

### Date-range filtered index queries

Three `*_in_range` variants add optional Unix timestamp bounds. Filtering is
applied **before** pagination so `start_index` and `limit` operate on the
filtered result set.

```rust
fn get_agreement_payments_in_range(
    env, agreement_id, start_index, limit,
    from_ts: Option<u64>, to_ts: Option<u64>
) -> Vec<PaymentRecord>

fn get_employer_payments_in_range(
    env, employer, start_index, limit,
    from_ts: Option<u64>, to_ts: Option<u64>
) -> Vec<PaymentRecord>

fn get_employee_payments_in_range(
    env, employee, start_index, limit,
    from_ts: Option<u64>, to_ts: Option<u64>
) -> Vec<PaymentRecord>
```

#### Supported combinations

| `from_ts` | `to_ts` | Effect |
|---|---|---|
| `None` | `None` | Identical to the base paginated function |
| `Some(f)` | `None` | Records with `timestamp >= f` |
| `None` | `Some(t)` | Records with `timestamp <= t` |
| `Some(f)` | `Some(t)` | Records with `timestamp` in `[f, t]` (inclusive) |

#### Validation

If both bounds are provided and `from_ts > to_ts`, the function panics with:

```
InvalidRange: from_ts must be <= to_ts
```

Values are never silently swapped. `from_ts == to_ts` is valid (single-timestamp range).

#### Backward compatibility

The original `get_payments_by_agreement`, `get_payments_by_employer`, and
`get_payments_by_employee` functions are **unchanged**. Passing `None, None`
to an `*_in_range` function returns the same records as the corresponding base
function called with identical pagination parameters.

#### Examples

```text
// Range only
get_agreement_payments_in_range(env, id, 1, 100, Some(1_700_000_000), Some(1_700_086_400))

// Pagination only (no date filter — equivalent to base function)
get_agreement_payments_in_range(env, id, 21, 20, None, None)

// Range + pagination (page 2 of a filtered window)
get_agreement_payments_in_range(env, id, 11, 10, Some(1_000), Some(9_999))

// Employer index + range + pagination
get_employer_payments_in_range(env, employer, 1, 50, Some(1_700_000_000), None)
```

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

For `*_in_range` functions, positions refer to the **filtered** set, not the
raw index.

---

## Idempotency

If the same `payment_hash` is submitted more than once, the contract returns
the existing `global_id` without writing any new storage. The global counter,
all index counts, and the stored record are unchanged. `record_payment` is
safe to retry on network failures.

---

## Security Model

| Property | Enforcement |
|---|---|
| Only the registered payroll contract may write | `payroll_contract.require_auth()` |
| Initialization is one-time | Second `initialize` call panics `"Already initialized"` |
| Records are immutable | No update or delete code path exists |
| Index entries are append-only | Counts can only increase; no decrement path |
| Page size is bounded | `limit` silently capped at `MAX_PAGE_SIZE = 100` |
| Duplicate hashes are idempotent | Existing ID returned; no new storage written |
| Invalid date range is rejected | `from_ts > to_ts` panics; no silent swap |

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
payment.

```
topic:  Symbol("payment_recorded")
data:   payment_id, payment_hash, agreement_id, token, amount, from, to, timestamp
```

---

## Reconciliation Patterns

### Hash-first

```rust
let record = client.get_payment_by_hash(tx_hash)?;
```

### ID-first

```rust
let total = client.get_global_payment_count();
for id in 1..=total {
    let record = client.get_payment_by_id(id)?;
}
```

### Date-range

```rust
let day_ago = now_unix - 86_400;
let page = client.get_agreement_payments_in_range(
    &42u128, &1u32, &100u32, &Some(day_ago), &None,
);
```

### Cross-verification

```rust
let by_id   = client.get_payment_by_id(id).unwrap();
let by_hash = client.get_payment_by_hash(by_id.payment_hash.clone()).unwrap();
assert_eq!(by_id, by_hash); // always true
```

---

## Test Coverage

### Dual-index guarantee (issue #912)

| Test | What it proves |
|---|---|
| `test_index_consistency_hash_and_id_return_identical_fields` | Both paths return field-for-field identical records |
| `test_index_consistency_unknown_hash_returns_none_not_wrong_record` | Unknown hash returns `None`; real record is not displaced |
| `test_index_consistency_each_hash_resolves_only_to_its_own_record` | Hash A never resolves to payment B |
| `test_index_consistency_batch_all_pairs_agree` | 8 payments — every (hash, id) pair agrees across both paths |
| `test_index_consistency_duplicate_hash_preserves_original_record` | Replay leaves the original record intact |
| `test_index_consistency_stored_hash_matches_lookup_key` | `payment_hash` field matches the lookup key |
| `test_index_consistency_unknown_hash_returns_none_with_populated_storage` | Unknown hash returns `None` even with other payments present |
| `test_hash_index_written_atomically` | Reverse-lookup written in the same invocation as the primary record |
| `test_same_payment_visible_in_all_five_query_paths` | All five query surfaces return the same record |

### Date-range filtering

| Test | What it proves |
|---|---|
| `test_range_*_no_range_matches_base_query` | `None, None` returns identical results to the base function |
| `test_range_*_from_ts_only` | `from_ts` alone filters `timestamp >= from_ts` |
| `test_range_*_to_ts_only` | `to_ts` alone filters `timestamp <= to_ts` |
| `test_range_*_both_bounds` | Both bounds produce the correct inclusive window |
| `test_range_boundary_inclusive_*` | Exact boundary timestamps are included |
| `test_range_empty_result_*` | Empty set returned when no records fall in range |
| `test_range_*_invalid_range_panics` | `from_ts > to_ts` panics; values are never swapped |
| `test_range_from_ts_equals_to_ts_is_valid` | Equal bounds are valid; single matching record returned |
| `test_range_pagination_page1/2/last` | Pagination operates correctly over the filtered set |
| `test_range_pagination_limit_capped_at_max_page_size` | Page-size cap applies to filtered results |
| `test_range_pagination_start_index_above_filtered_count_returns_empty` | Out-of-range start_index returns empty |
| `test_range_single_record_match` | Narrow range returning exactly one record |
| `test_range_agreement_isolation_*` | One agreement's range is unaffected by other agreements |
| `test_range_entire_history_when_bounds_are_very_wide` | `[0, u64::MAX]` returns all records |
