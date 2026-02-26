# Payment Splitting

This document describes the Payment Splitting contract (issue #206): splitting a single payment across multiple recipients with configurable percentage or fixed amounts.

## Overview

The `payment_splitter` contract provides:

- **Split definitions** – Multiple recipients with either percentage (basis points) or fixed amounts.
- **Validation** – Percent splits must sum to 10000 (100%); fixed splits are validated against the total amount at execution time.
- **Computation** – View function to compute each recipient’s amount given a total.

## Contract Location

- **Contract**: `onchain/contracts/payment_splitter/src/lib.rs`
- **Tests**: `onchain/contracts/payment_splitter/tests/test_splitter.rs`

## API

### Initialization

- `initialize(admin)` – Sets the admin. Callable once.

### Splits

- `create_split(creator, recipients)` – Creates a split. `recipients` is a list of `RecipientShare { recipient, kind }` where `kind` is either `ShareKind::Percent(bps)` (10000 = 100%) or `ShareKind::Fixed(amount)`. If any share is percent, all percent shares must sum to 10000.
- `get_split(split_id)` – Returns the split definition.

### Validation and Computation

- `validate_split_for_amount(split_id, total_amount)` – Returns true if the split is valid for the given total (for fixed shares, sum of fixed amounts must equal `total_amount`).
- `compute_split(split_id, total_amount)` – Returns a list of `(recipient, amount)` for the given total. Percent shares are computed as `(bps * total_amount) / 10000`.

## Rules

- **Percent-only**: All shares are `Percent`; they must sum to 10000.
- **Fixed-only or mixed**: If any share is `Fixed`, `validate_split_for_amount` checks that the sum of fixed amounts equals the total.
- **Computation**: Percent shares are derived from `total_amount`; fixed shares are used as-is.

## Security

- Only the creator authenticates when creating a split.
- No token transfers in this contract; it only stores definitions and computes amounts. Actual disbursement is done by the caller (e.g. payroll or payment flow) using `compute_split` or `validate_split_for_amount`.
