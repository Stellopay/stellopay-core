# Payment Splitting

This document describes the hardened **Payment Splitter** contract, which allows splitting a single token payment across multiple recipients with high arithmetic precision and rounding discipline.

## Overview

The `payment_splitter` contract provides:

- **Split definitions** – Multiple recipients with either percentage-based (basis points) or fixed-amount allocations.
- **Rounding Discipline** – Implementation of the "remainder absorber" pattern to ensure zero dust loss.
- **Arithmetic Safety** – Checked math operations to prevent overflows and allocation underflows.
- **Validation** – Strict checks for duplicate recipients, zero weights, and mutual exclusivity between split modes.

## Contract Location

- **Contract**: `onchain/contracts/payment_splitter/src/lib.rs`
- **Tests**: `onchain/contracts/payment_splitter/tests/test_splitter.rs`

## API

### Initialization
- `initialize(admin)` – Sets the admin for the contract. Callable only once.

### Split Creation
- `create_split(creator, recipients)` – Creates a new split definition.
    - `recipients`: A list of `RecipientShare { recipient, kind }`.
    - `kind`: Either `ShareKind::Percent(bps)` (1-10000) or `ShareKind::Fixed(amount)`.
    - **Constraint**: All recipient shares in a single split must be of the same type (either all Percent or all Fixed).
    - **Constraint**: Recipients must be unique; duplicate addresses are not allowed.

### Computation and Validation
- `compute_split(split_id, total_amount)` – Returns a list of `(recipient, amount)` for the given total.
    - Uses the **Remainder Absorber** pattern: the final recipient receives the difference between `total_amount` and the sum of all previous slices. This ensures the total allocated always matches the input exactly.
- `validate_split_for_amount(split_id, total_amount)` – Returns true if the split matches the intended amount.
    - For Fixed splits: `sum(fixed_amounts) == total_amount`.
    - For Percent splits: Always true (sum is validated at creation).

## Implementation Details

### Rounding Discipline
When calculating percentage-based splits, integer division typically loses "dust". The contract handles this by iterating through the recipients and tracking the `total_allocated`. The last recipient always receives `total_amount - total_allocated`, effectively absorbing any rounding remainder.

### Example: Prime Number Split
If splitting **107 units** between 2 recipients with a **60%/40%** ratio:
1. Recipient A (60%): `(6000 * 107) / 10000 = 64.2` → **64**
2. Recipient B (40% - Absorber): `107 - 64` → **43**
**Total: 107**

### Example: 1-Stroop Split
If splitting **1 stroop** 50/50:
1. Recipient A (50%): `(5000 * 1) / 10000 = 0.5` → **0**
2. Recipient B (50% - Absorber): `1 - 0` → **1**
**Total: 1**

## Security Considerations
- **Non-zero Weights**: The contract prevents creating splits with zero-weight percentages or zero-amount fixed shares.
- **Duplicate Prevention**: Prevents unintentional double-allocation to the same address within one split.
- **Off-chain Integration**: This contract does not handle token movements directly. It provides the logic for other contracts or off-chain systems to perform safe token transfers.
