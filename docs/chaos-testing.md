## Chaos and Fault Injection Testing

This document describes the chaos and fault injection tests for the
`stello_pay_contract` and how they complement the existing functional and
property-based test suites.

### Goals

- **Exercise failure and recovery paths** that are hard to hit in normal
  scenarios.
- **Verify state consistency** when token transfers or storage expectations are
  violated.
- **Document expected behavior** under degraded conditions (partial completion,
  misconfiguration, transient failures).

### Test Location

- Chaos tests: `onchain/contracts/stello_pay_contract/tests/chaos/test_fault_injection.rs`

These tests run alongside the standard test suite:

```bash
cargo test -p stello_pay_contract
```

### Fault Models

The chaos tests currently model three classes of faults:

1. **Token transfer failures**
   - Escrow metadata indicates funds are available while the actual on-chain
     token balance is zero.
2. **Storage misconfiguration / partial writes**
   - DataKey-based escrow balance is inconsistent with real token holdings.
3. **Partial completion in batch operations**
   - Multiple claims share a single limited escrow balance, leading to mixed
     success and failure within a batch.

### Scenarios

#### 1. Token Transfer Failure Does Not Corrupt State

- **Test**: `chaos_token_transfer_failure_does_not_corrupt_state`
- **Setup**:
  - Create a payroll agreement and activate it.
  - Seed `DataKey` storage with a positive escrow balance and employee
    metadata, but **do not mint any tokens** to the contract address.
- **Behavior**:
  - A payroll claim via `try_claim_payroll` fails due to an underlying token
    transfer error.
  - Agreement status remains `Active`.
  - Claimed periods remain unchanged.
  - DataKey escrow balance remains unchanged.

This verifies that failed external calls do not partially mutate agreement
state or counters.

#### 2. Escrow Misconfiguration and Recovery

- **Test**: `chaos_escrow_misconfiguration_then_recovery`
- **Setup**:
  - Create and activate a payroll agreement.
  - Mint tokens to the contract, but set `DataKey::set_agreement_escrow_balance`
    to `0` (misconfigured).
- **Behavior**:
  - First `try_claim_payroll` returns `PayrollError::InsufficientEscrowBalance`.
  - After correcting the escrow balance in storage, a second
    `try_claim_payroll` succeeds and increments the employee’s claimed periods.

This models a misconfigured or partially written escrow state that is later
fixed, ensuring the contract can recover cleanly.

#### 3. Batch Partial Completion and Rollback Semantics

- **Test**: `chaos_batch_partial_completion_and_rollback`
- **Setup**:
  - Create a payroll agreement with two employees and equal salaries.
  - Activate the agreement and fund escrow for **exactly one** period.
  - Seed `DataKey` storage accordingly.
- **Behavior**:
  - A `batch_claim_payroll` call for both employees:
    - Succeeds for the first employee.
    - Fails for the second with `PayrollError::InsufficientEscrowBalance`.
  - Batch result reports `successful_claims = 1` and `failed_claims = 1`.
  - Employee 1’s claimed periods increment; Employee 2’s remain `0`.

This validates partial completion semantics and per-employee error reporting
under constrained escrow.

### Running Chaos Tests

Chaos tests are regular Rust tests and run as part of the `stello_pay_contract`
package:

```bash
cargo test -p stello_pay_contract --test test_fault_injection
```

or:

```bash
cargo test -p stello_pay_contract
```

### Extending Chaos Coverage

When adding new features or entrypoints, consider:

- Introducing tests that:
  - Flip flags mid-transaction (e.g. pause or cancel during active flows).
  - Simulate delayed or missing oracle/rate updates.
  - Exercise edge-of-window timing (right before/after grace periods).
- Verifying invariants after each injected failure:
  - Agreement `status` remains valid.
  - `paid_amount`, `claimed_periods`, and escrow accounting are unchanged on
    failure.
  - Batch APIs report granular error codes for each item.

These patterns help ensure that even under adverse conditions, the contract
remains safe and predictable.

