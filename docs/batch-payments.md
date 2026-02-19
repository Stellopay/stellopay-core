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
| `employee_indices` | `Vec<u32>` | 0-based employee indices |

**Returns:** `BatchPayrollResult`

---

### `batch_claim_milestones`
Claim multiple approved milestones at once.

```rust
batch_claim_milestones(env, agreement_id, milestone_ids)
```

| Param | Type | Description |
|---|---|---|
| `agreement_id` | `u128` | Milestone agreement ID |
| `milestone_ids` | `Vec<u32>` | 1-based milestone IDs |

**Returns:** `BatchMilestoneResult`

---

## Result Shape

Both functions return the same structure:

```rust
{
  total_claimed: i128,       // total tokens transferred
  successful_claims: u32,    // how many succeeded
  failed_claims: u32,        // how many failed
  results: Vec<...>,         // per-item breakdown
}
```

> ✅ **Partial success is valid** — one failure never blocks the rest.

---

## Error Codes

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | Duplicate ID in batch |
| `2` | Invalid ID / out of bounds |
| `3` | Not approved *(milestones only)* |
| `4` | Already claimed |
| `PayrollError::*` | Standard payroll errors *(payroll only)* |

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