# Multisig Operation Types: LargePayment and DisputeResolution

This document describes how `LargePayment` and `DisputeResolution` operation types
are wired into the stello_pay payroll contract to enforce M-of-N multi-party approval
before high-value actions execute.

## Overview

Without multisig integration, a single authorized key (employee or arbiter) can
trigger large payroll claims or dispute resolutions unilaterally. The integration
described here adds a configurable on-chain gate: if the action amount meets or
exceeds a configured threshold, a corresponding multisig operation must have
reached `Executed` status before the payroll contract proceeds.

## Architecture

```
stello_pay_contract
  ├── claim_payroll(…, multisig_operation_id: Option<u128>)
  │     └── if amount >= large_payment_threshold
  │           └── multisig.is_operation_approved(op_id) == true  ← gate
  ├── resolve_dispute(…, multisig_operation_id: Option<u128>)
  │     └── if total_payout >= dispute_threshold
  │           └── multisig.is_operation_approved(op_id) == true  ← gate
  └── batch_claim_payroll(…, multisig_operation_id: Option<u128>)
        └── per-item: if amount >= large_payment_threshold → same gate

multisig contract
  ├── propose_operation(signer, LargePayment | DisputeResolution)
  ├── approve_operation(signer, op_id)   ← auto-executes at threshold
  ├── set_operation_thresholds(owner, OperationThresholds)
  └── is_operation_approved(op_id) → bool
```

The check is **opt-in**: if no `MultisigConfig` is stored in stello_pay, all
existing behaviour is preserved and no multisig call is made.

## Configuration

### 1. Deploy and initialize the multisig contract

```rust
multisig.initialize(
    owner,
    signers,   // Vec<Address> — the M-of-N signer set
    threshold, // u32 — global minimum approvals
    Some(emergency_guardian),
);
```

### 2. (Optional) Set per-kind thresholds

Per-kind thresholds allow stricter requirements for specific operation types.
The effective threshold is `max(global_threshold, kind_threshold)`.

```rust
multisig.set_operation_thresholds(
    owner,
    OperationThresholds {
        large_payment: 3,       // require 3-of-N for LargePayment
        dispute_resolution: 2,  // require 2-of-N for DisputeResolution
    },
);
```

### 3. Configure stello_pay to use the multisig contract

```rust
stello_pay.set_multisig_config(
    owner,
    MultisigConfig {
        contract: multisig_address,
        large_payment_threshold: 10_000_0000000, // 10,000 tokens (7 decimals)
        dispute_threshold:        5_000_0000000, // 5,000 tokens
    },
);
```

Setting `large_payment_threshold` or `dispute_threshold` to `i128::MAX` effectively
disables the check for that operation type while keeping the other active.

## Approval Flow

### LargePayment (claim_payroll)

1. Off-chain tooling detects a pending large payroll claim.
2. A signer proposes the operation:
   ```rust
   let op_id = multisig.propose_operation(
       signer,
       OperationKind::LargePayment(token, employee, amount),
   );
   ```
3. Additional signers approve until the threshold is met:
   ```rust
   multisig.approve_operation(signer2, op_id);
   // auto-executes when approvals >= effective_threshold
   ```
4. The employee calls `claim_payroll` with the executed operation ID:
   ```rust
   stello_pay.claim_payroll(caller, agreement_id, employee_index, Some(op_id));
   ```
5. stello_pay calls `multisig.is_operation_approved(op_id)`. If `true`, the
   transfer proceeds. If `false` or `op_id` is `None`, the call returns
   `PayrollError::MultisigApprovalRequired`.

### DisputeResolution (resolve_dispute)

1. A signer proposes the dispute resolution:
   ```rust
   let op_id = multisig.propose_operation(
       signer,
       OperationKind::DisputeResolution(
           payroll_contract, agreement_id, pay_employee, refund_employer,
       ),
   );
   ```
2. Signers approve until threshold is met (operation auto-executes on-chain as a
   state transition — no token transfer happens in the multisig contract itself).
3. The arbiter calls `resolve_dispute` with the executed operation ID:
   ```rust
   stello_pay.resolve_dispute(
       arbiter, agreement_id, pay_employee, refund_employer, Some(op_id),
   );
   ```

### Below-threshold actions

If the amount is below the configured threshold, `multisig_operation_id` is
ignored and the call proceeds normally:

```rust
// Small claim — no multisig needed
stello_pay.claim_payroll(caller, agreement_id, employee_index, None);
```

## Security Properties

| Property | Mechanism |
|---|---|
| Single-key bypass prevention | `is_operation_approved` requires `Executed` status, which requires M approvals |
| Replay protection | Each multisig operation has a unique monotonic ID; once executed, status is immutable |
| Threshold integrity | Effective threshold = `max(global, per-kind)`; cannot be lowered below global |
| Opt-in safety | No `MultisigConfig` → no cross-contract call, no regression for existing deployments |
| Guardian break-glass | Emergency guardian can execute a pending operation without threshold, for incident response |
| Authorization | All multisig state changes require `require_auth()` on the caller |

## Error Codes

| Code | Variant | Meaning |
|---|---|---|
| 33 | `MultisigApprovalRequired` | Amount meets threshold but no approved operation was provided |
| 34 | `MultisigOperationMismatch` | Reserved for future parameter-binding validation |

## New API Surface

### multisig contract

| Function | Description |
|---|---|
| `set_operation_thresholds(owner, OperationThresholds)` | Owner-only: set per-kind threshold overrides |
| `get_operation_thresholds() -> Option<OperationThresholds>` | Returns current per-kind thresholds |
| `is_operation_approved(op_id) -> bool` | Returns `true` iff the operation is in `Executed` state |

### stello_pay_contract

| Function | Change |
|---|---|
| `set_multisig_config(owner, MultisigConfig)` | New: configure multisig integration |
| `get_multisig_config() -> Option<MultisigConfig>` | New: read current config |
| `claim_payroll(…, multisig_operation_id: Option<u128>)` | Added optional op ID parameter |
| `batch_claim_payroll(…, multisig_operation_id: Option<u128>)` | Added optional op ID parameter |
| `resolve_dispute(…, multisig_operation_id: Option<u128>)` | Added optional op ID parameter |

## Testing

Integration tests are in `onchain/contracts/multisig/tests/multisig_integration_tests.rs`.

```bash
cd onchain
cargo test -p multisig multisig_integration_tests
```

### Test Coverage

- `set_and_get_operation_thresholds` — owner can configure per-kind thresholds
- `set_thresholds_non_owner_rejected` — non-owner cannot update thresholds
- `set_thresholds_out_of_range_rejected` — threshold 0 or > signer count rejected
- `is_approved_unknown_operation` — returns false for unknown IDs
- `is_approved_reflects_execution_state` — false while pending, true after execution
- `is_approved_cancelled_operation` — cancelled operations are not approved
- `large_payment_requires_2of3_approvals` — standard 2-of-3 flow
- `large_payment_requires_3of3_approvals` — per-kind override to 3-of-3
- `large_payment_duplicate_approval_ignored` — idempotent approvals
- `large_payment_non_signer_cannot_approve` — access control
- `approve_already_executed_operation_rejected` — post-execution approval blocked
- `dispute_resolution_requires_2of3_approvals` — standard dispute flow
- `dispute_resolution_3of3_threshold_override` — per-kind 3-of-3 override
- `dispute_resolution_can_be_cancelled_before_threshold` — cancellation path
- `guardian_can_bypass_threshold_for_dispute_resolution` — break-glass
- `per_kind_threshold_equal_to_global_no_change` — no regression when equal
- `per_kind_threshold_lower_than_global_clamped` — effective threshold clamped to global
