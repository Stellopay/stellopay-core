# ⚡ Batch Payments

Process multiple payroll or milestone claims in **one transaction** — lower fees, less code.

---

## Why Use It?

| Single Claims | Batch Claims |
|---|---|
| N transactions | 1 transaction |
| N gas costs | ~1 gas cost |
| N signatures | 1 signature |

---

## Failure model

**A batch is not all-or-nothing, with the exception of the batch-level errors listed below.**

* Every element is attempted in the order it appears in the input.
* An element that cannot settle is recorded in `results` with its own
  `error_code`, and processing continues with the next element. A failure never
  rolls back elements that already settled.
* Two groups of failures are therefore distinct: **batch-level errors**, which
  are returned as `Err(PayrollError)` with no state change and no transfer at
  all, and **per-element failures**, which are reported inside the returned
  result.

### Batch-level errors

These reject the whole call before any element is processed, so nothing is
settled and nothing is transferred:

| Error | Cause |
|---|---|
| `PayrollError::InvalidData` | empty input list; for `batch_claim_payroll` also a paused agreement or a claim window that has not opened |
| `PayrollError::BatchTooLarge` | more elements than `MAX_BATCH_SIZE` |
| `PayrollError::AgreementNotFound` | agreement does not exist |
| `PayrollError::InvalidAgreementMode` | `batch_claim_payroll` against a non-Payroll agreement |
| `PayrollError::AgreementNotActivated` | activation timestamp missing |
| `PayrollError::AgreementPaused` | `batch_claim_milestones` against a paused agreement |
| `PayrollError::MilestoneNotFound` | `batch_claim_milestones` against an agreement with no milestones |
| `PayrollError::RateLimited` | `batch_claim_payroll` while a rate limiter is configured and the caller is over its budget |

### Batch size limit

A batch accepts **at most `MAX_BATCH_SIZE` elements, which is 20**. The limit is
checked before any element is processed, so an over-sized batch is rejected with
`PayrollError::BatchTooLarge` and settles nothing. The bound exists because
`tests/gas_benchmarks.rs` measures the batch path at that size and enforces the
committed gas ceiling.

---

## Functions

### `batch_claim_payroll`

Claim payroll for multiple employees at once.

```rust
batch_claim_payroll(env, caller, agreement_id, employee_indices)
```

| Param | Type | Description |
|---|---|---|
| `caller` | `Address` | Must match each employee address |
| `agreement_id` | `u128` | Payroll agreement ID |
| `employee_indices` | `Vec<u32>` | 0-based employee indices, at most `MAX_BATCH_SIZE` |

**Returns:** `BatchPayrollResult`

Because `caller` is a single address and each element still enforces
`caller == employee`, only the caller's own index can settle. Any other index is
reported as `PayrollError::Unauthorized`, so a multi-element payroll batch is a
convenient way to claim your own index while explicitly declaring the ones you
expect to fail — it is not a way to claim on another employee's behalf.

---

### `batch_claim_milestones`

Claim multiple approved milestones at once.

```rust
batch_claim_milestones(env, agreement_id, milestone_ids)
```

| Param | Type | Description |
|---|---|---|
| `agreement_id` | `u128` | Milestone agreement ID |
| `milestone_ids` | `Vec<u32>` | 1-based milestone IDs, at most `MAX_BATCH_SIZE` |

**Returns:** `BatchMilestoneResult`

---

## Result Shape

Both functions return a summary plus a per-element breakdown:

```rust
{
  total_claimed: i128,       // total tokens transferred
  successful_claims: u32,    // how many succeeded
  failed_claims: u32,        // how many failed
  results: Vec<...>,         // per-item breakdown, one entry per element
}
```

`results` has one entry per element of the input, in the same order, so a caller
can map an outcome back to the request without re-reading per-element state:

* `batch_claim_payroll` → `PayrollClaimResult { employee_index, success, amount_claimed, error_code }`
* `batch_claim_milestones` → `MilestoneClaimResult { milestone_id, success, amount_claimed, error_code }`

> ✅ **Partial success is valid** — one failure never blocks the rest.

---

## Error Codes

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | Duplicate ID in batch |
| `2` | Invalid ID / out of bounds |
| `3` | Not approved *(milestones only)* |
| `4` | Already claimed *(milestones only)* |
| `PayrollError::*` | Standard payroll errors *(payroll only)* |

For payroll the `error_code` is the numeric value of the `PayrollError` variant,
for example `PayrollError::NoPeriodsToClaim`, `PayrollError::Unauthorized` or
`PayrollError::InsufficientEscrowBalance`.

---

## Retrying a partial batch

A batch that partially settled can be retried safely: **an element that already
settled is never paid twice.** On a retry it is reported as a per-element
failure — code `4` (already claimed) for milestones, `NoPeriodsToClaim` for
payroll — with `amount_claimed` of `0`, and no tokens move.

The intended retry loop:

1. Submit the batch.
2. Read `results` and keep the elements where `success` is `false` **and** the
   `error_code` describes a cause you can fix (for example milestone `3`, "not
   approved").
3. Fix the cause, then resubmit just those elements.
4. Re-submitting the whole original batch is also safe, but the elements that
   already settled come back as failures with `amount_claimed: 0`, so use
   `total_claimed` from the retry to know what actually moved.

For payroll, "the remainder" is time-based rather than an approval: a retry pays
exactly the periods that have elapsed since the last successful claim, and a
retry with no new period elapsed settles nothing.

---

## Quick Example

```rust
// Claim payroll for employees 0, 1, and 2
let result = client.batch_claim_payroll(
    &employee,
    &agreement_id,
    &vec![&env, 0u32, 1u32, 2u32],
);

assert!(result.successful_claims > 0);

// Claim milestones 1, 2, and 3
let result = client.batch_claim_milestones(
    &agreement_id,
    &vec![&env, 1u32, 2u32, 3u32],
);

assert_eq!(result.failed_claims, 0);
```

---

## Rules

- Agreement must be **Active** (or **Cancelled** within grace period)
- Agreement must **not be Paused**
- Caller must be the registered employee/contributor
- Empty input → immediate error
- At most `MAX_BATCH_SIZE` (20) elements per batch

---

## Tests

`tests/test_batch_claim_semantics.rs` covers the failure model end to end: a
fully successful batch, a failing element followed by a settling one, a retry of
the remainder, and a retry of an already settled batch that must move nothing.
`tests/test_payment_failures.rs` covers the over-limit rejection and the
per-element error codes.
