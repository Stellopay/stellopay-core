# Salary Adjustment System

This document describes the salary adjustment smart contract added for issue `#218`.

## Scope

The `salary_adjustment` contract manages salary change requests with approval workflows:

- Salary increases and decreases
- Approval and rejection workflow before any adjustment is applied
- Effective date scheduling
- Cancellation for pending or rejected adjustments

## Contract Location

- Contract: `onchain/contracts/salary_adjustment/src/lib.rs`
- Tests: `onchain/contracts/salary_adjustment/tests/test_adjustment.rs`

## Security Model

- `initialize` is one-time only.
- Employers must authenticate to create, apply, or cancel adjustments.
- Only the configured approver can approve or reject an adjustment.
- Approved adjustments cannot be cancelled, preserving scheduling guarantees.
- Adjustments can only be applied after their effective date.
- Zero or negative salaries are rejected at creation time.
- No-op adjustments (same salary) are rejected.

## Data Model

Each adjustment stores:

- `kind`: `Increase` or `Decrease` (derived from salary comparison)
- `status`: `Pending`, `Approved`, `Rejected`, `Applied`, `Cancelled`
- roles: `employer`, `employee`, `approver`
- `current_salary` and `new_salary`
- `effective_date` and `created_at` timestamps

## Public API

- `initialize(owner)`
- `create_adjustment(employer, employee, approver, current_salary, new_salary, effective_date)`
- `approve_adjustment(approver, adjustment_id)`
- `reject_adjustment(approver, adjustment_id)`
- `apply_adjustment(employer, adjustment_id)`
- `cancel_adjustment(employer, adjustment_id)`
- `get_adjustment(adjustment_id)`
- `get_owner()`

## Workflow Summary

1. Employer creates a salary adjustment request specifying current and new salaries.
2. Approver reviews and either approves or rejects.
3. Employer applies the adjustment after the effective date has passed.
4. Employer can cancel adjustments only while status is `Pending` or `Rejected`.

## Testing Focus

The test suite covers:

- salary increase and decrease creation
- approval and application after effective date
- application at exact effective date boundary
- application before effective date fails
- approval access control
- rejection access control
- apply without approval fails
- cancel pending and rejected adjustments
- cancel approved adjustment fails
- employer-only access for apply and cancel
- zero/negative salary rejection
- same-salary no-op rejection
- double initialization failure
- nonexistent adjustment lookup
