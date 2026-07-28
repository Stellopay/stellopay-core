# Runtime Invariant Assertions

This document details the runtime invariant assertions implemented in `stello_pay_contract` to prevent accounting bugs and over-claiming of funds.

## Rationale
Smart contracts managing financial value must maintain strict internal consistency. Runtime invariants act as an automated safety net, ensuring that:
1.  **Accounting remains valid**: No period can be claimed twice, and the total claimed cannot exceed the agreed limit.
2.  **Solvency is guaranteed**: The contract's escrow balance must always cover its outstanding milestone obligations.

## Implemented Invariants

### 1. Period Accounting Invariant
**Location**: `claim_payroll`, `claim_time_based`
**Assertion**: `claimed_periods <= num_periods`
**Security Benefit**: Prevents vulnerabilities where a contributor could claim more funds than the agreement allows, even if high-level logic contains arithmetic errors.

### 2. Milestone Balance Invariant
**Location**: `approve_milestone`, `claim_milestone`
**Assertion**: `escrow_balance >= sum(unclaimed_milestones)`
**Security Benefit**: Ensures that the contract always has sufficient funds to fulfill all pending milestone obligations. This prevents a "first-come, first-served" failure mode if the escrow balance is somehow depleted or mismanaged.

## Implementation Details
The invariants are implemented using `assert!` and specific error returns. While business logic provides user-friendly error messages (e.g., `PayrollError::InsufficientEscrowBalance`), the `assert!` statements provide a final, immutable barrier that triggers a panic (transaction revert) if violated.

### Helper Functions
To support efficient validation, the following internal helpers were added to `payroll.rs`:
- `sum_all_milestones`: Calculates the total value of all milestones in an agreement.
- `sum_unclaimed_milestones`: Calculates the total value of milestones that are approved for payment but not yet claimed.

## Verification
These invariants are covered by a dedicated test suite in `onchain/contracts/stello_pay_contract/tests/invariant_tests.rs`.
