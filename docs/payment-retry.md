# Payment Retry Mechanism

## Overview

The `payment_retry` contract adds automatic retry handling for failed payments.
A payment request remains `Pending` until it either succeeds or reaches terminal failure.

Core capabilities:
- Configurable retry intervals (`retry_intervals`)
- Configurable maximum retry attempts (`max_retry_attempts`)
- Failure notifications via `payment_failed` event with a configured `failure_notifier`

## Contract Location

- `onchain/contracts/payment_retry/src/lib.rs`
- Tests: `onchain/contracts/payment_retry/tests/test_retry.rs`

## Flow

1. Initialize contract once with `initialize(owner)`.
2. Create a request with `create_payment_request(...)`.
3. Fund escrow with `fund_payment(payer, payment_id, amount)`.
4. Trigger processing via `process_due_payments(max_payments)`.
5. On insufficient escrow balance, contract schedules retry using policy.
6. If retries exceed policy, status becomes `Failed` and `payment_failed` is emitted.

## Retry Policy

- `max_retry_attempts` limits failed retry cycles before terminal failure.
- `retry_intervals` is a list of retry delays (seconds).
- If retries exceed the list length, the last interval is reused.
- `max_retry_attempts = 0` disables retries (first failure is terminal).

## Failure Notifications

Terminal failure emits:
- Topic: `("payment_failed", payment_id)`
- Data:
  - `payment_id`
  - `retry_count`
  - `max_retry_attempts`
  - `notifier`

Consumers can monitor this event and route alerts (email/webhook/off-chain queue).

## Security Notes

- `initialize` is one-time.
- `create_payment_request`, `fund_payment`, and `cancel_payment` require payer auth.
- Only the original payer can fund or cancel that payment request.
- Retry configuration is bounded to prevent abuse:
  - max retry attempts
  - max interval count
  - max single interval duration
- Processing is pull-based (`process_due_payments`) to avoid unbounded automatic loops.

## Testing

`test_retry.rs` covers:
- success path
- retries and eventual success
- terminal failure + notification event
- interval reuse behavior
- cancellation behavior
- max processing limit behavior
- validation and access controls

Run:

```bash
cd onchain
cargo test -p payment_retry
```
