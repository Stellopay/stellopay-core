# Payment History Contract

The Payment History Contract provides an immutable ledger for all payments within the StelloPay ecosystem. It allows efficient querying of payment records by Agreement, Employer, and Employee.

## Features

- **Immutable Records**: Once recorded, payment details cannot be altered.
- **Efficient Querying**: Supports pagination and filtering by:
  - Agreement ID
  - Employer Address
  - Employee Address
- **Storage Optimized**: Uses `Persistent` storage with an ID-referencing index strategy to minimize costs while maintaining query speed.

## Interface

### Initialization

```rust
fn initialize(env: Env, owner: Address, payroll_contract: Address)
```

- `owner`: Admin address.
- `payroll_contract`: The ONLY address authorized to record payments.

### Recording Payments

```rust
fn record_payment(
    env: Env,
    agreement_id: u128,
    token: Address,
    amount: i128,
    from: Address,
    to: Address,
    timestamp: u64,
) -> u128
```

- Returns the unique Global Payment ID.
- Emits `payment_recorded` event.

### Querying

All query functions support pagination via `start_index` (1-based) and `limit`.

```rust
fn get_payments_by_agreement(env: Env, agreement_id: u128, start_index: u32, limit: u32) -> Vec<PaymentRecord>
fn get_payments_by_employer(env: Env, employer: Address, start_index: u32, limit: u32) -> Vec<PaymentRecord>
fn get_payments_by_employee(env: Env, employee: Address, start_index: u32, limit: u32) -> Vec<PaymentRecord>
```

## Events

### `payment_recorded`

Topics:

- `Symbol("payment_recorded")`
- `agreement_id` (u128)

Data:

- `token` (Address)
- `amount` (i128)
- `from` (Address)
- `to` (Address)
- `timestamp` (u64)
