# Runtime Invariant Assertions

This document describes the runtime invariant assertions added to the `stello_pay_contract` to ensure accounting integrity and prevent funds from being over-claimed.

## Core Invariants

### 1. Period Accounting Invariant
**Invariant**: `claimed_periods <= num_periods`

This invariant ensures that no employee or contributor can claim more periods than defined in the agreement. It is checked at the start of every mutating function that handles period-based claims:
- `claim_payroll`
- `claim_payroll_in_token`
- `batch_claim_payroll`
- `claim_time_based`

### 2. Escrow Balance Invariant
**Invariant**: `escrow_balance >= sum(unclaimed_milestone_amounts)`

This invariant ensures that the contract always holds sufficient funds to cover all milestones that have been added to an agreement but not yet claimed. It is checked before any operation that could lead to an inconsistent state:
- `approve_milestone`
- `claim_milestone`
- `batch_claim_milestones`
- `add_milestone` (Post-check in debug mode)

## Implementation Details

The invariants are implemented using Soroban's `assert!` and `panic!` mechanisms. Violation of an invariant indicates a logic error or external state manipulation (e.g., direct token transfer out of the contract) and results in an immediate transaction revert.

### Helpers
Internal helpers were added to `payroll.rs` to support these checks:
- `sum_unclaimed_milestones`: Calculates the sum of all milestones not yet marked as claimed.
- `sum_all_milestones`: Calculates the total amount of all milestones added to an agreement.

## Security Considerations

The addition of these assertions provides a "defense-in-depth" layer. Even if a bug is discovered in the period calculation or status transition logic, the runtime assertions will prevent unauthorized fund release by reverting the transaction.

## Verification

The invariants are verified through a comprehensive test suite in `tests/invariant_tests.rs`, which includes:
- Boundary tests for period limits.
- Stress tests for milestone balance integrity.
- Simulation of fund depletion to verify assertion failure.
