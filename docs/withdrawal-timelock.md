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
  subsequently queued operations; encoded at initialization time and adjustable
  via `update_delay`.
- **Maximum delay cap** – `MAX_DELAY_SECONDS = 2_592_000` (30 days). Neither
  `initialize` nor `update_delay` may set a delay exceeding this cap.
  This prevents an admin from permanently locking the queue by setting an
  arbitrarily large delay.
- **Non-retroactive delay updates** – Calling `update_delay` changes the delay
  for operations queued *after* the call. Already-queued operations retain their
  original `eta`, which is frozen at queue time. This is a critical security
  invariant: reducing the delay cannot make previously-queued operations
  immediately executable.
- **Operation kinds** – Built-in support for:
  - `Withdrawal(token, to, amount)` – Large outbound payment intent.
  - `AdminChange(target_contract, payload_hash)` – Generic admin change intent
    (e.g. new configuration, upgraded implementation).
- **Execution as intent recording** – The contract records that an operation
  has been executed and emits an event; actual token transfers or admin changes
  are expected to be performed by off-chain tooling or other contracts using
  this recorded intent.
- **Queue monitoring** – The contract maintains a `QueuedCount` counter (O(1)
  read) for off-chain monitors to check queue depth without iterating all ops.

### Data Model

#### Types

- `TimelockError`
  - `NotInitialized`, `AlreadyInitialized`, `NotAdmin`, `DelayTooLarge`,
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
  - `executed_at: Option<u64>` – set on execute; `None` otherwise
  - `cancelled_at: Option<u64>` – set on cancel; `None` otherwise
  - `status: OperationStatus`

#### Storage

- `Initialized` – one-time initialization flag
- `Admin` – timelock admin address
- `MinDelaySeconds` – current global minimum delay between queue and execute
- `NextOpId` – auto-incrementing operation id
- `QueuedCount` – count of currently active (`Queued`) operations
- `Operation(id)` – stored `TimelockedOperation`
- `OperationsFor(admin)` – `Vec<u128>` of operation ids created by `admin`
  (includes executed and cancelled ids)

### Public API

Initialization:

- `initialize(admin, min_delay_seconds) -> Result<(), TimelockError>`
  - One-time call; `0 < min_delay_seconds <= MAX_DELAY_SECONDS`.

Timelock workflow:

- `queue(caller, kind) -> Result<u128, TimelockError>`
  - Admin-only; computes `eta = now + min_delay_seconds`, stores the operation,
    and returns the `op_id`. Emits:
    - `("timelock_queued", op_id) -> kind`
- `execute(caller, op_id) -> Result<(), TimelockError>`
  - Admin-only; requires `now >= eta` and `status == Queued`.
  - Marks the operation as `Executed`, sets `executed_at`, and emits:
    - `("timelock_executed", op_id) -> kind`
- `cancel(caller, op_id) -> Result<(), TimelockError>`
  - Admin-only; marks a queued operation as `Cancelled`, sets `cancelled_at`,
    and emits:
    - `("timelock_cancelled", op_id) -> ()`
- `update_delay(caller, new_delay) -> Result<(), TimelockError>`
  - Admin-only; updates `min_delay_seconds` for **future** operations only.
  - `0 < new_delay <= MAX_DELAY_SECONDS`.
  - Does **not** retroactively alter the `eta` of already-queued operations.
  - Emits: `("timelock_delay_updated", old_delay) -> new_delay`

Read helpers:

- `get_config() -> Result<(Address, u64), TimelockError>`
  - Returns `(admin, min_delay_seconds)`.
- `get_operation(op_id) -> Option<TimelockedOperation>`
- `get_operations_for(owner) -> Vec<u128>`
  - Returns all op ids (including executed and cancelled) created by `owner`.
- `get_queued_count() -> u32`
  - O(1) count of currently active (`Queued`) operations.

### Typical Workflow

1. **Initialize** the timelock with an admin address and a minimum delay
   (e.g. 1 hour = 3600s or 24 hours = 86400s).
2. **Queue** operations when large withdrawals or sensitive admin changes are
   requested.
3. **Monitor** queued operations off-chain via the `timelock_queued` event and
   `get_operation` / `get_queued_count`.
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

### Governance Integration Details

When integrating with the governance contract for admin changes, follow this pattern:

#### Payload Hash Computation

For `AdminChange` operations triggered by governance, compute payload hashes deterministically:

```rust
fn create_admin_change_payload_hash(
    env: &Env,
    target_contract: &Address,
    new_admin: &Address,
    nonce: u64,
) -> BytesN<32> {
    let mut payload = Vec::new(env);
    
    // Domain separation prefix
    payload.push_back(Symbol::new(env, "ADMIN_CHANGE").to_val());
    
    // Target contract address
    payload.push_back(target_contract.to_val());
    
    // New admin address  
    payload.push_back(new_admin.to_val());
    
    // Nonce for uniqueness
    payload.push_back(nonce.to_val());
    
    // Compute SHA-256 hash
    env.crypto().sha256(&payload)
}
```

#### Integration Flow

1. **Governance Proposal**: Create an admin change proposal in the governance contract
2. **Vote & Queue**: Standard governance voting and queuing process
3. **Timelock Queue**: After proposal success, queue an `AdminChange` operation
4. **Execute**: After the timelock delay, execute the operation

#### Security Benefits

- **Double Timelock**: Governance timelock + withdrawal_timelock delay
- **Domain Separation**: Payload hashes prevent collision attacks
- **Deterministic Verification**: Off-chain tooling can verify payload hashes
- **Access Control**: Only authorized governance execution can queue timelock ops

### Security Considerations

- Protect the `admin` key with multisig or governance; compromise of this key
  allows queueing and executing large withdrawals after the delay.
- Choose `min_delay_seconds` based on operational risk tolerance:
  - Short delays for low-risk operations.
  - Longer delays (hours/days) for large withdrawals or upgrades.
- `update_delay` is non-retroactive by design. Reducing the delay does **not**
  make previously-queued operations immediately executable; their `eta` is
  frozen at queue time. This prevents a compromised admin from fast-pathing
  previously-queued operations by first lowering the delay.
- `MAX_DELAY_SECONDS` (30 days) is enforced at both `initialize` and
  `update_delay`. An admin cannot lock the queue indefinitely by setting an
  arbitrarily large delay.
- Monitor `timelock_queued`, `timelock_executed`, and `timelock_cancelled`
  events to audit the full lifecycle of critical operations and detect
  unexpected activity.
