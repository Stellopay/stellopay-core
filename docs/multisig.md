## Multisig Contract for Critical Operations

This document describes the `multisig` smart contract added for issue `#202`.

### Scope

The multisig contract acts as a governance and safety layer in front of critical operations:

- contract upgrade approvals
- large outbound token payments from a shared wallet
- approvals for dispute resolution flows

The contract focuses on **threshold-based approvals**, clear **event logs** for off-chain automation, and a **break-glass emergency guardian**.

### Contract Location

- Contract: `onchain/contracts/multisig/src/lib.rs`
- Tests: `onchain/contracts/multisig/tests/test_multisig.rs`
- Edge case tests: `onchain/contracts/multisig/tests/test_multisig_edge_cases.rs`

### Security Model

- `initialize` is **one-time only** and must be called by the designated owner.
- A **fixed signer set** and default **threshold** are stored on-chain.
- Each operation type can have an optional threshold override. Types without
  an override continue to use the default threshold.
- Only configured **signers** can:
  - propose new operations
  - approve existing operations
- Operations auto-execute once `approvals >= threshold`.
- An optional **emergency guardian** can execute pending operational actions
  without satisfying the threshold (break-glass override), but cannot execute
  threshold-override changes below their current threshold.
- Large token payments are executed directly from the multisig contract balance using the Soroban token client.

### Data Model

Core types:

- `OperationKind`
  - `ContractUpgrade(Address, BytesN<32>)`
  - `LargePayment(Address, Address, i128)` as `(token, to, amount)`
  - `DisputeResolution(Address, u128, i128, i128)` as `(payroll_contract, agreement_id, pay_employee, refund_employer)`
  - `SetThresholdOverride(OperationType, Option<u32>)` as `(operation_type, threshold)`; `None` removes the override
- `OperationType`
  - `ContractUpgrade`
  - `LargePayment`
  - `DisputeResolution`
- `OperationStatus`
  - `Pending`, `Executed`, `Cancelled`
- `Operation`
  - `id`, `kind`, `creator`, `status`, `created_at`, `executed_at`

Storage keys:

- `Owner`: configuration owner
- `Signers`: vector of signer addresses
- `Threshold`: required signatures count
- `EmergencyGuardian`: optional guardian address
- `OperationCounter`: auto-incrementing id
- `Operation(id)`: stored operation
- `Approvals(id)`: vector of signer addresses that approved
- `ThresholdOverride(operation_type)`: optional required signature count for one operation type

### Public API

- `initialize(owner, signers, threshold, emergency_guardian)`
- `propose_operation(proposer, kind) -> operation_id`
- `approve_operation(signer, operation_id)`
- `cancel_operation(caller, operation_id)`
- `emergency_execute(guardian, operation_id)`
- `get_operation(operation_id) -> Option<Operation>`
- `get_signers() -> Vec<Address>`
- `get_threshold() -> u32`
- `get_threshold_override(operation_type) -> Option<u32>`
- `get_effective_threshold(operation_type) -> u32`
- `get_approvals(operation_id) -> Vec<Address>`

### Workflow Summary

1. Owner calls `initialize` with signer set, threshold, and optional guardian.
2. Any signer can call `propose_operation` to create a new operation (auto-approving as creator).
3. Additional signers call `approve_operation` until the approval count meets the threshold.
4. When `approvals >= threshold`, the contract:
   - executes `LargePayment` operations by transferring tokens from its balance
   - marks `ContractUpgrade` and `DisputeResolution` operations as executed for off-chain tooling to act on
5. Creator or owner can cancel a pending operation via `cancel_operation`.
6. The emergency guardian can call `emergency_execute` to force execution of a
   pending operational action in break-glass scenarios. Threshold-override
   changes are excluded from this bypass.

### Per-operation Threshold Overrides

Signers configure an override by proposing a `SetThresholdOverride` operation.
The proposal is auto-approved by its creator and other signers approve it using
the normal workflow. The configuration write occurs only after the approval
count reaches the target operation type's currently active, pre-change
threshold. For example, lowering `ContractUpgrade` from 3-of-3 to 2-of-3 still
requires three approvals. The emergency guardian cannot bypass this check.

Override values must be between `1` and the number of configured signers. To
restore the default threshold, propose `SetThresholdOverride(type, None)`; that
removal must also meet the type's current override. Pending operations are
evaluated against the effective threshold at approval time, so an approved
configuration change applies consistently to subsequent approvals.

### Threshold Configurations

| Config | Use Case |
|--------|----------|
| 1-of-1 | Single signer, auto-execute on propose |
| 2-of-3 | Standard multisig (balanced safety/ops) |
| 3-of-3 | Maximum security, all must agree |
| 1-of-N with guardian | Operational with break-glass safety net |

### Security Properties

#### Replay Protection
Each operation has a monotonically increasing ID. Once executed or cancelled, the status is immutable. Re-approving an executed operation is a no-op.

#### Duplicate Approval Prevention
The `has_approved` check ensures each signer can only contribute one approval per operation, regardless of how many times `approve_operation` is called.

#### Threshold Integrity
The effective threshold is checked at execution time. An operation-type
override takes precedence over the default, while approvals are stored
independently of threshold changes. Override changes are themselves operations
and use the target type's pre-change effective threshold, preventing a signer
or emergency guardian from unilaterally weakening the approval requirement.

#### Authorization
All state-changing functions require `require_auth()` on the caller. The Soroban host enforces cryptographic signature verification.

#### Guardian Security
- Guardian address should be a cold wallet or hardware-secured key
- Guardian actions are logged via events for audit trails
- Guardian cannot execute already-executed or cancelled operations
- Guardian cannot bypass the active threshold for an override change

### Events

| Event | Fields | When Emitted |
|-------|--------|--------------|
| `operation_proposed` | `operation_id`, `creator` | On propose |
| `operation_approved` | `operation_id`, `signer`, `approvals`, `threshold` | On each approval |
| `operation_executed` | `operation_id` | On execution |
| `operation_cancelled` | `operation_id` | On cancellation |

### Testing

Run the test suite:

```bash
cd onchain
cargo test -p multisig
```

#### Test Coverage

The test suite covers:

- Initialization validation (invalid threshold, duplicate signers, re-init)
- 1-of-1 auto-execution
- 3-of-3 all-approvals-required
- 2-of-3 standard threshold flow
- Duplicate approval prevention
- Non-signer rejection (propose and approve)
- Already-executed rejection
- Cancel by creator and owner
- Guardian-only rescue
- Guardian cannot execute executed/cancelled ops
- Multiple independent operations
- Zero-amount payment rejection
- ContractUpgrade and DisputeResolution flows
- Query function correctness
- Per-operation override enforcement and default fallback
- Adversarial threshold lowering and guardian-bypass prevention
- Override removal and invalid override rejection

### Observability: payroll multisig threshold changes

The `stello_pay_contract` payroll contract gates large payments and dispute
resolutions behind multisig approval, using two thresholds configured via
`set_multisig_config(owner, multisig_contract, large_payment_threshold,
dispute_resolution_threshold)`.

Because changing these thresholds alters the contract's security posture, every
successful `set_multisig_config` call now:

- emits a `MultisigConfigChanged` event (see `docs/events-schema.json`) carrying
  the `caller`, the `multisig_contract`, and the old vs new values for both
  thresholds, so off-chain monitors can detect approval-requirement changes
  mid-lifecycle; and
- records a tamper-evident audit entry through the contract's existing audit
  path (`AuditEvent::MultisigConfigChanged`, action `multisig_config_changed`,
  contract-level `agreement_id = 0`).

The event exposes only public configuration; it never includes multisig signer
secrets. Emission and audit recording are covered by
`onchain/contracts/stello_pay_contract/tests/test_event_emissions.rs`
(`test_multisig_config_changed_event*`).
