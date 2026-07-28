# Salary Adjustment System

> **Contract path**: `onchain/contracts/salary_adjustment/src/lib.rs`
> **Test path**: `onchain/contracts/salary_adjustment/tests/test_adjustment.rs`

This document covers the `salary_adjustment` contract implementing issue `#337`.

## Overview

The `salary_adjustment` contract manages employer-driven salary change requests with structured approval workflows, effective date enforcement, salary cap controls, payroll-visible salary tracking, and an append-only audit stream for compliance review.

## Workflow

```
Employer creates adjustment (Pending)
         │
   Approver reviews
    ┌─────┴─────┐
  Approve     Reject
    │             │
  Approved    Rejected ──► Employer cancels (Cancelled)
    │
  Employer applies after effective_date
    │
  Applied  ──► employee salary updated for payroll
```

## Security Model

| Concern | Enforcement |
|---------|-------------|
| Only employer creates/applies/cancels | `employer.require_auth()` + identity check |
| Only designated approver approves/rejects | `approver.require_auth()` + `adjustment.approver == approver` |
| Retroactive abuse | `create_adjustment` is forward-only; retroactive edits require owner + employer authorization and a reason hash |
| Salary cap enforcement | `new_salary <= effective_salary_cap()` at creation |
| One-time initialization | Persistent flag; second call panics |
| Approved adjustments are immutable | Cancel blocked on `Approved`/`Applied` status |
| Conflicting edits | Same employee + same effective timestamp can only have one stored adjustment |
| Compliance auditability | Every mutating action appends a queryable audit record and emits an audit event |

## Constants

| Constant | Value | Meaning |
|----------|-------|---------|
| `DEFAULT_MAX_SALARY` | `1_000_000_000_000` | Default cap (1 trillion stroops) when none is set |

## Storage Layout

| Key | Type | Description |
|-----|------|-------------|
| `Initialized` | `bool` | One-time init guard |
| `Owner` | `Address` | Admin who can set salary cap |
| `NextAdjustmentId` | `u128` | Monotonic counter |
| `Adjustment(u128)` | `SalaryAdjustment` | Adjustment record by id |
| `SalaryCap` | `i128` | Optional salary ceiling set by owner |
| `EmployeeSalary(Address)` | `i128` | Last applied salary per employee |
| `NextAuditLogId` | `u128` | Monotonic audit log counter |
| `AuditLog(u128)` | `SalaryAdjustmentAuditEntry` | Append-only audit entry |
| `EmployeeEffectiveAdjustment(Address, u64)` | `u128` | Conflict sentinel for employee/effective-date pairs |

## Data Model

```rust
pub struct SalaryAdjustment {
    pub id: u128,
    pub employer: Address,
    pub employee: Address,
    pub approver: Address,
    pub kind: AdjustmentKind,       // Increase | Decrease
    pub status: AdjustmentStatus,   // Pending | Approved | Rejected | Applied | Cancelled
    pub current_salary: i128,
    pub new_salary: i128,
    pub effective_date: u64,        // Unix timestamp; must be >= created_at
    pub created_at: u64,
    pub retroactive: bool,
    pub retroactive_approved_by: Option<Address>,
    pub reason_hash: Option<BytesN<32>>,
}
```

Retroactive records store a contract-computed reason commitment rather than the raw caller-provided hash. The stored value is:

```text
sha256(
  "salary_adjustment:retroactive:v1" ||
  owner || employer || employee ||
  current_salary || new_salary || effective_date ||
  caller_supplied_reason_hash
)
```

This domain separates salary-adjustment reasons from hashes used by other contracts, binds the reason to the immutable adjustment parameters, and avoids storing plaintext HR rationale on-chain.

## Contract API

### `initialize(owner)`
One-time setup. Panics if called twice.

### `set_salary_cap(owner, cap)`
Owner-only. Sets a global ceiling enforced on all future `create_adjustment` calls.

**Panics**: `"Only owner can set salary cap"`, `"Salary cap must be positive"`

### `create_adjustment(employer, employee, approver, current_salary, new_salary, effective_date) -> u128`
Creates a new forward-only adjustment in `Pending` state. `effective_date` must be at or after the current ledger timestamp.

**Panics**:
- `"Current salary must be positive"`
- `"New salary must be positive"`
- `"New salary must differ from current salary"`
- `"New salary exceeds salary cap"`
- `"Effective date cannot be in the past"`
- `"Conflicting adjustment exists"`

### `create_retroactive_adjustment(owner, employer, employee, approver, current_salary, new_salary, effective_date, reason_hash) -> u128`
Creates a retroactive adjustment in `Pending` state using the dedicated authorization path.

Requirements:

- `owner` must match the initialized contract owner and authenticate
- `employer` must authenticate
- `effective_date` must be before the current ledger timestamp
- `reason_hash` must be non-zero
- the stored reason hash is domain-separated and bound to the adjustment parameters

**Panics**:
- `"Only owner can authorize retroactive adjustment"`
- `"Use create_adjustment for forward adjustments"`
- `"Retroactive reason hash required"`
- all standard `create_adjustment` validation panics except the forward-only date check

### `approve_adjustment(approver, adjustment_id)`
Moves status `Pending → Approved`. Only the configured `approver` may call.

**Panics**: `"Only approver can approve"`, `"Adjustment is not pending"`

### `reject_adjustment(approver, adjustment_id)`
Moves status `Pending → Rejected`.

**Panics**: `"Only approver can reject"`, `"Adjustment is not pending"`

### `apply_adjustment(employer, adjustment_id)`
Moves status `Approved → Applied`. Requires `ledger.timestamp() >= effective_date`.
Updates `EmployeeSalary(employee)` for payroll visibility.

**Panics**: `"Only employer can apply"`, `"Adjustment is not approved"`, `"Effective date not reached"`

### `cancel_adjustment(employer, adjustment_id)`
Moves status `Pending | Rejected → Cancelled`. Approved/Applied records are immutable.

**Panics**: `"Only employer can cancel"`, `"Adjustment cannot be cancelled"`

### `get_adjustment(adjustment_id) -> Option<SalaryAdjustment>`
Read-only lookup by id.

### `get_owner() -> Option<Address>`
Returns the contract owner.

### `get_salary_cap() -> i128`
Returns configured cap or `DEFAULT_MAX_SALARY` if none set.

### `get_employee_salary(employee) -> Option<i128>`
Returns the last applied salary for payroll claiming logic. `None` until first adjustment is applied.

### `get_audit_log(audit_id) -> Option<SalaryAdjustmentAuditEntry>`
Returns a stored append-only audit entry by id.

### `get_audit_log_count() -> u128`
Returns the number of audit entries written.

## Events

| Topic | Payload |
|-------|---------|
| `("adjustment_created", id)` | `AdjustmentCreatedEvent` |
| `("adjustment_approved", id)` | `AdjustmentApprovedEvent` |
| `("adjustment_rejected", id)` | `AdjustmentRejectedEvent` |
| `("adjustment_applied", id)` | `AdjustmentAppliedEvent` |
| `("adjustment_cancelled", id)` | `AdjustmentCancelledEvent` |
| `("salary_cap_set", cap)` | `SalaryCapSetEvent` |
| `("salary_adjustment_audit", audit_id)` | `AdjustmentAuditEvent` |

## Audit Stream

Every successful mutating action appends a `SalaryAdjustmentAuditEntry` and emits a matching audit event:

- `adjustment_created`
- `adjustment_approved`
- `adjustment_rejected`
- `adjustment_applied`
- `adjustment_cancelled`
- `salary_cap_set`

Audit records include the actor, action, optional adjustment id, optional employee, optional amount, optional reason hash, and ledger timestamp. There is no update or delete entrypoint for audit records.

## Test Coverage

49 tests covering:

- Initialization (one-time guard, owner stored)
- Double-init and pre-init panics
- Create: increase, decrease, timestamps, id increment
- Create validations: zero salary, same salary, retroactive date, cap exceeded, conflicting effective dates
- Retroactive authorization: default block, owner authorization, non-owner rejection, non-zero reason hash, domain-separated immutable reason storage
- Cap: default, set, boundary (`new_salary == cap`), cap tightened
- Approve: status change, wrong approver, double-approve, approve-after-reject
- Reject: status change, wrong approver, reject-after-approve
- Apply: happy path, exact effective date, before effective date, unapproved, wrong employer
- Cancel: pending, rejected-then-cancel, approved blocked, applied blocked, wrong employer
- Payroll visibility: `None` before apply, correct value after apply, tracks latest, independent per employee
- Audit visibility: audit count, audit entry fields, audit reason linkage
- Query: nonexistent adjustment, get_owner

## Invariants

1. An adjustment id is never reused.
2. `effective_date >= created_at` for standard adjustments.
3. Retroactive adjustments must store `retroactive = true`, owner approval, and a domain-separated `reason_hash`.
4. `new_salary <= salary_cap` for all stored adjustments.
5. `EmployeeSalary(employee)` reflects the `new_salary` of the most recently applied adjustment.
6. Only `Pending` and `Rejected` adjustments can be cancelled.
7. Only `Approved` adjustments can be applied.
8. Status transitions are one-way and irreversible (no rollback).
9. Audit log IDs are monotonic and records are append-only.
10. One employee/effective-date pair cannot be reused for conflicting unresolved adjustments.

## Security Considerations

- **Retroactive abuse**: `create_adjustment` rejects `effective_date < now`, preventing accidental or unauthorized backdated salary changes. Retroactive changes must use `create_retroactive_adjustment`, which requires both owner and employer auth plus a non-zero reason hash.
- **Reason privacy and integrity**: The raw HR reason is never stored. The contract stores a domain-separated SHA-256 commitment bound to the owner, employer, employee, salaries, and effective date, so the reason cannot be replayed across unrelated adjustments without changing the stored hash.
- **Auditability**: All successful state changes write append-only audit entries that can be queried by id and correlated with events.
- **Conflicts**: Duplicate adjustments for the same employee and effective timestamp are rejected to prevent ambiguous payroll interpretation.
- **Cap bypass**: Cap is read fresh on each `create_adjustment` call, so lowering the cap immediately restricts new requests.
- **Approver identity**: The approver is stored per-adjustment at creation. A global admin change does not affect outstanding adjustments.
- **Auth checks**: All state-mutating methods call `require_auth()` on the acting address before any reads or writes.
