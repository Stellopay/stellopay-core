## Withdrawal Delay and Timelock Contract

The `withdrawal_timelock` contract enforces a configurable delay before large
withdrawals or critical admin operations can be executed. It provides a
queue-and-execute workflow that integrates cleanly with off-chain orchestration
or higher-level treasury/escrow contracts.

### Contract Location

- Contract: `onchain/contracts/withdrawal_timelock/src/lib.rs`
- Tests: `onchain/contracts/withdrawal_timelock/tests/test_timelock.rs`

### Design Overview

- **Queued operations** – Admin queues an operation, which becomes executable
  only after the configured minimum delay.
- **Global minimum delay** – A single `min_delay_seconds` applies to all
  operations; encoded at initialization time.
- **Operation kinds** – Built-in support for:
  - `Withdrawal(token, to, amount)` – Large outbound payment intent.
  - `AdminChange(target_contract, payload_hash)` – Generic admin change intent
    (e.g. new configuration, upgraded implementation).
- **Execution as intent recording** – The contract records that an operation
  has been executed and emits an event; actual token transfers or admin changes
  are expected to be performed by off-chain tooling or other contracts using
  this recorded intent.

### Data Model

#### Types

- `TimelockError`
  - `NotInitialized`, `AlreadyInitialized`, `NotAdmin`, `QueueTooSmall`,
    `InvalidDelay`, `OperationNotFound`, `NotReady`,
    `AlreadyExecutedOrCancelled`
- `OperationKind`
  - `Withdrawal(Address, Address, i128)` – `(token, to, amount)`
  - `AdminChange(Address, BytesN<32>)` – `(target_contract, payload_hash)`
- `OperationStatus`
  - `Queued`, `Executed`, `Cancelled`
- `TimelockedOperation`
  - `id: u128`
  - `kind: OperationKind`
  - `creator: Address`
  - `eta: u64` – earliest executable timestamp
  - `created_at: u64`
  - `executed_at: Option<u64>`
  - `status: OperationStatus`

#### Storage

- `Initialized` – one-time initialization flag
- `Admin` – timelock admin address
- `MinDelaySeconds` – global minimum delay between queue and execute
- `NextOpId` – auto-incrementing operation id
- `Operation(id)` – stored `TimelockedOperation`
- `OperationsFor(admin)` – `Vec<u128>` of operation ids created by `admin`

### Public API

Initialization:

- `initialize(admin, min_delay_seconds) -> Result<(), TimelockError>`

Timelock workflow:

- `queue(caller, kind) -> Result<u128, TimelockError>`
  - Admin-only; computes `eta = now + min_delay_seconds`, stores the operation,
    and returns the `op_id`.
- `execute(caller, op_id) -> Result<(), TimelockError>`
  - Admin-only; requires current time `>= eta` and `status == Queued`.
  - Marks the operation as `Executed`, sets `executed_at`, and emits:
    - `("timelock_executed", op_id) -> kind`
- `cancel(caller, op_id) -> Result<(), TimelockError>`
  - Admin-only; marks a queued operation as `Cancelled` and emits:
    - `("timelock_cancelled", op_id) -> ()`

Read helpers:

- `get_config() -> Result<(admin, min_delay_seconds), TimelockError>`
- `get_operation(op_id) -> Option<TimelockedOperation>`
- `get_operations_for(admin) -> Vec<u128>`

### Typical Workflow

1. **Initialize** the timelock with an admin address and a minimum delay
   (e.g. 1 hour or 24 hours).
2. **Queue** operations when large withdrawals or sensitive admin changes are
   requested.
3. **Monitor** queued operations off-chain via events and `get_operation`.
4. **Execute** operations once the delay has elapsed and they have been
   reviewed/approved via off-chain processes.
5. **Cancel** operations if risk is detected or approvals are withdrawn.

### Integration Patterns

- **Treasury / escrow contracts**
  - Use `OperationKind::Withdrawal` entries as a guardrail for large outbound
    payments; the timelock records intent and timing, and an off-chain
    operator (or another contract) performs the actual token transfer after
    the timelock is satisfied.
- **Admin / configuration changes**
  - Encode a hash of the intended configuration or upgrade payload in
    `AdminChange` and require that off-chain tooling verifies the hash before
    applying the change.
- **Governance interoperability**
  - Governance proposals (e.g. from the `governance` contract) can require
    that certain actions be represented as timelocked operations, creating a
    two-stage safety net: governance approval followed by a timelocked delay.

### Security Considerations

- Protect the `admin` key with multisig or governance; compromise of this key
  allows queueing and executing large withdrawals after the delay.
- Choose `min_delay_seconds` based on operational risk tolerance:
  - Short delays for low-risk operations.
  - Longer delays (hours/days) for large withdrawals or upgrades.
- Monitor `timelock_executed` and `timelock_cancelled` events to audit the
  lifecycle of critical operations and detect unexpected activity.

