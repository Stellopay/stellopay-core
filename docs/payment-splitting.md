# Payment Splitting

This document describes the hardened **Payment Splitter** contract, which allows splitting a single token payment across multiple recipients with deterministic arithmetic and explicit dust handling.

## Overview

The `payment_splitter` contract provides:

- **Split definitions** – Multiple recipients with either percentage-based (basis points) or fixed-amount allocations.
- **Rounding Discipline** – Percentage splits use a deterministic largest-remainder policy so dust is neither lost nor biased by caller-provided recipient ordering.
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
    - Rejects `total_amount <= 0`.
    - For percentage splits, floors each exact share and then distributes any leftover dust to the recipients with the largest fractional remainders.
    - Exact remainder ties are broken by canonical recipient address order, not input list order.
    - For fixed splits, the call rejects totals that do not exactly match the sum of fixed shares.
- `validate_split_for_amount(split_id, total_amount)` – Returns true if the split matches the intended amount.
    - For Fixed splits: `sum(fixed_amounts) == total_amount`.
    - For Percent splits: Always true (sum is validated at creation).

## Implementation Details

### Rounding Discipline
When calculating percentage-based splits, integer division produces fractional dust. The contract applies the following deterministic policy:

1. Compute each exact share as `(bps * total_amount) / 10000`.
2. Allocate the floored integer portion to every recipient.
3. Compute `dust = total_amount - sum(floored_shares)`.
4. Give one extra unit to the `dust` recipients with the largest fractional remainders.
5. If multiple recipients have the same fractional remainder, break the tie by canonical address order.

This guarantees:

- `sum(outputs) == total_amount` for every successful split.
- No value is lost to truncation.
- Caller-supplied recipient ordering cannot bias dust allocation.

### Example: Prime Number Split
If splitting **107 units** between 2 recipients with a **60%/40%** ratio:
1. Recipient A (60%): exact `64.2`, floor `64`, remainder `0.2`
2. Recipient B (40%): exact `42.8`, floor `42`, remainder `0.8`
3. Dust = `107 - (64 + 42) = 1`, so Recipient B receives the extra unit
**Total: 107**

### Example: 1-Stroop Split
If splitting **1 stroop** 50/50:
1. Both recipients have an exact share of `0.5`, so both floor to `0`
2. Dust = `1`
3. Because the remainders are tied, the extra unit goes to the recipient with the lower canonical address encoding
**Total: 1**

## Security Considerations
- **Non-zero Weights**: The contract prevents creating splits with zero-weight percentages or zero-amount fixed shares.
- **Duplicate Prevention**: Prevents unintentional double-allocation to the same address within one split.
- **Ordering Resistance**: Dust assignment depends on remainders and canonical address order, not the caller-provided recipient list order.
- **Fixed Split Integrity**: Fixed splits reject mismatched totals instead of silently shifting the difference onto the final recipient.
- **Off-chain Integration**: This contract does not handle token movements directly. It provides the logic for other contracts or off-chain systems to perform safe token transfers.
