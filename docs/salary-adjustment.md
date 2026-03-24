# Salary Adjustment System

> **Contract path**: `onchain/contracts/salary_adjustment/src/lib.rs`
> **Test path**: `onchain/contracts/salary_adjustment/tests/test_adjustment.rs`

This document covers the `salary_adjustment` contract implementing issue `#337`.

## Overview

The `salary_adjustment` contract manages employer-driven salary change requests with structured approval workflows, effective date enforcement, salary cap controls, and payroll-visible salary tracking — with a full event log for auditors.

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
| Retroactive abuse | `effective_date >= ledger.timestamp()` at creation |
| Salary cap enforcement | `new_salary <= effective_salary_cap()` at creation |
| One-time initialization | Persistent flag; second call panics |
| Approved adjustments are immutable | Cancel blocked on `Approved`/`Applied` status |

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
}
```

## Contract API

### `initialize(owner)`
One-time setup. Panics if called twice.

### `set_salary_cap(owner, cap)`
Owner-only. Sets a global ceiling enforced on all future `create_adjustment` calls.

**Panics**: `"Only owner can set salary cap"`, `"Salary cap must be positive"`

### `create_adjustment(employer, employee, approver, current_salary, new_salary, effective_date) -> u128`
Creates a new adjustment in `Pending` state.

**Panics**:
- `"Current salary must be positive"`
- `"New salary must be positive"`
- `"New salary must differ from current salary"`
- `"New salary exceeds salary cap"`
- `"Effective date cannot be in the past"`

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

## Events

| Topic | Payload |
|-------|---------|
| `("adjustment_created", id)` | `AdjustmentCreatedEvent` |
| `("adjustment_approved", id)` | `AdjustmentApprovedEvent` |
| `("adjustment_rejected", id)` | `AdjustmentRejectedEvent` |
| `("adjustment_applied", id)` | `AdjustmentAppliedEvent` |
| `("adjustment_cancelled", id)` | `AdjustmentCancelledEvent` |
| `("salary_cap_set", cap)` | `SalaryCapSetEvent` |

## Test Coverage

35 tests covering:

- Initialization (one-time guard, owner stored)
- Double-init and pre-init panics
- Create: increase, decrease, timestamps, id increment
- Create validations: zero salary, same salary, retroactive date, cap exceeded
- Cap: default, set, boundary (`new_salary == cap`), cap tightened
- Approve: status change, wrong approver, double-approve, approve-after-reject
- Reject: status change, wrong approver, reject-after-approve
- Apply: happy path, exact effective date, before effective date, unapproved, wrong employer
- Cancel: pending, rejected-then-cancel, approved blocked, applied blocked, wrong employer
- Payroll visibility: `None` before apply, correct value after apply, tracks latest, independent per employee
- Query: nonexistent adjustment, get_owner

## Invariants

1. An adjustment id is never reused.
2. `effective_date >= created_at` for all stored adjustments.
3. `new_salary <= salary_cap` for all stored adjustments.
4. `EmployeeSalary(employee)` reflects the `new_salary` of the most recently applied adjustment.
5. Only `Pending` and `Rejected` adjustments can be cancelled.
6. Only `Approved` adjustments can be applied.
7. Status transitions are one-way and irreversible (no rollback).

## Security Considerations

- **Retroactive abuse**: `effective_date < now` is rejected at creation, preventing backdated salary increases that could exploit prior payroll periods.
- **Cap bypass**: Cap is read fresh on each `create_adjustment` call, so lowering the cap immediately restricts new requests.
- **Approver identity**: The approver is stored per-adjustment at creation. A global admin change does not affect outstanding adjustments.
- **Auth checks**: All state-mutating methods call `require_auth()` on the acting address before any reads or writes.
