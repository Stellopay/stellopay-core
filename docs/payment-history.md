# Payment History

This document describes the storage key layout, pagination strategy, ordering guarantees, and integration patterns for the `payment_history` contract.

Contract source: [`onchain/contracts/payment_history/src/lib.rs`](../onchain/contracts/payment_history/src/lib.rs)  
Storage types: [`onchain/contracts/payment_history/src/storage.rs`](../onchain/contracts/payment_history/src/storage.rs)

---

## Storage Key Reference

All state is stored in **persistent** ledger storage. Keys are encoded as `StorageKey` enum variants via Soroban's `contracttype` derive.

```
Owner                                → Address
PayrollContract                      → Address
GlobalPaymentCount                   → u128   (highest assigned global ID)

Payment(global_id: u128)             → PaymentRecord
PaymentByHash(hash: BytesN<32>)      → u128   (reverse lookup: hash → global_id)

AgreementPaymentCount(agreement_id)  → u32    (# payments for agreement)
AgreementPayment(agreement_id, pos)  → u128   (global_id at 1-based position)

EmployerPaymentCount(employer)       → u32    (# payments by employer)
EmployerPayment(employer, pos)       → u128   (global_id at 1-based position)

EmployeePaymentCount(employee)       → u32    (# payments to employee)
EmployeePayment(employee, pos)       → u128   (global_id at 1-based position)
```

Every key is **written once and never mutated**. There is no update or delete path.

---

## Index Layout

The contract maintains four indices over the same set of `PaymentRecord` values.

### Primary record store

`Payment(global_id)` holds the canonical `PaymentRecord`. All other indices are pointers back to this key.

### Reverse-lookup (hash index)

`PaymentByHash(hash)` maps a 32-byte payment hash directly to a `global_id`. This enables O(1) point reads by transaction hash without scanning any sequential index.

### Sequential indices (Agreement, Employer, Employee)

Each of the three sequential indices follows the same pattern:

| Key | Type | Purpose |
|-----|------|---------|
| `*Count(entity)` | `u32` | Total entries; also the highest valid 1-based position |
| `*(entity, position)` | `u128` | Pointer to `global_id` at this position |

Positions are assigned in **insertion order**: the first payment recorded for an entity gets position 1, the second gets position 2, and so on. Positions are never reused or reassigned.

To read a record at position `p`:
1. Read `*(entity, p)` → `global_id`
2. Read `Payment(global_id)` → `PaymentRecord`

---

## Pagination Strategy

All three sequential indices share the same pagination interface:

```
get_payments_by_agreement(agreement_id, start_index, limit)
get_payments_by_employer(employer, start_index, limit)
get_payments_by_employee(employee, start_index, limit)
```

**Parameters:**

- `start_index` — 1-based, inclusive. A value of `0` or greater than the total count returns an empty vector immediately with no ledger reads.
- `limit` — maximum records to return. Silently capped at `MAX_PAGE_SIZE` (100). Requesting more than 100 returns at most 100 records; no error is raised.

**Walking all records in batches of 20:**

```
page 1: start_index=1,  limit=20  → positions 1–20
page 2: start_index=21, limit=20  → positions 21–40
page 3: start_index=41, limit=20  → positions 41–60
...
```

Stop when the returned slice is shorter than `limit`, or when `start_index` exceeds the count returned by the corresponding `get_*_payment_count` function.

**Ledger reads per page:** `2 × min(limit, MAX_PAGE_SIZE)` — one read for the position pointer and one for the `PaymentRecord`.

---

## Ordering Guarantees

| Index | Order |
|-------|-------|
| Agreement | Insertion order (call order of `record_payment`) |
| Employer | Insertion order (call order of `record_payment`) |
| Employee | Insertion order (call order of `record_payment`) |

**Ordering is by insertion order, not by `timestamp`.** The `timestamp` field is supplied by the calling payroll contract and is not verified. A payment recorded later may carry an earlier timestamp (for example, when an indexer ingests events out of chronological order). Callers that need chronological ordering must sort by `timestamp` after fetching a page.

Position indices never skip or reorder entries. Once position `p` is assigned to a `global_id`, that mapping is permanent.

---

## Idempotency

`record_payment` is idempotent on `payment_hash`. If the same 32-byte hash is submitted a second time, the existing `global_id` is returned and no storage is written. Index counts do not increase. This allows the payroll contract to retry safely without creating duplicate records.

---

## Security Properties

- **Authorization** — only the address registered at `initialize` as `payroll_contract` may call `record_payment`. Any other caller receives `Auth(InvalidAction)`.
- **Immutability** — there is no update or delete path. Once written, a `PaymentRecord` cannot be changed.
- **No pruning** — index counts can only increase. There is no decrement path, so no entry can be silently removed from the pagination range.
- **Atomicity** — the hash reverse-lookup index is written in the same `record_payment` invocation as the primary record. Both are always in sync.
- **Page size cap** — `limit` is hard-capped at 100 to bound ledger reads per invocation and prevent resource exhaustion.

---

## Integration with Indexers

Subscribe to the `payment_recorded` contract event to maintain an off-chain index in real time. Each event carries:

- `payment_id` — the assigned global ID (sequential position key)
- `payment_hash` — the 32-byte transaction-level reference key
- `agreement_id`, `token`, `amount`, `from`, `to`, `timestamp`

Because records are immutable, indexers never need to handle update or delete messages. A reconciliation pass only needs to forward-scan from the last known `global_id` to `get_global_payment_count()`.

**Reconciliation pattern:**

```
last_known_id = <stored by indexer>
total = get_global_payment_count()

for id in (last_known_id + 1)..=total:
    record = get_payment_by_id(id)
    index(record)

last_known_id = total
```

**Hash-based lookup** (when you have a Stellar transaction hash):

```
record = get_payment_by_hash(tx_hash)
if record is Some → use it directly
if record is None → payment not yet recorded
```

---

## Related Documentation

- [Architecture](architecture.md)
- [API documentation](api/README.md)
- [Integration guide](integration/README.md)
- [Audit logger contract](../onchain/contracts/audit_logger/) — companion audit trail
