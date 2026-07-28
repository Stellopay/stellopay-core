# Slashing Penalty Contract

## Overview

The `slashing_penalty` Soroban contract encodes slashing rules for StelloPay validators and participants. It supports two slash trigger mechanisms — **signed attestations** and **on-chain evidence** — and includes safeguards against unjust confiscation through a **7-day appeal window** and admin-controlled dispute resolution.

---

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                  SlashingPenaltyContract              │
│                                                       │
│  Roles         │  Slashers (N addresses, quorum K)   │
│                │  Admin (appeal resolution only)      │
│                                                       │
│  Triggers      │  1. On-chain evidence (single slasher│
│                │     + unique evidence hash)           │
│                │  2. Attestation (K-of-N slashers)    │
│                                                       │
│  Lifecycle     │  Pending → Executed                  │
│                │  Pending → Reversed (appeal upheld)  │
│                │  Pending → AppealRejected             │
│                                                       │
│  Safeguards    │  Max per-event penalty cap (bps)     │
│                │  Per-period amount cap               │
│                │  Lifetime amount cap                 │
│                │  Appeal window: 7 days               │
│                │  Replay protection: evidence hash    │
│                │  Role separation: admin ≠ slasher    │
└─────────────────────────────────────────────────────┘
```

---

## Evidence Format

Every slash requires an `evidence_hash` — a **SHA-256 hash of the raw evidence payload**. The payload should be constructed off-chain and must include:

| Field             | Type      | Description                                      |
|-------------------|-----------|--------------------------------------------------|
| `offender`        | `Address` | Stellar address of the misbehaving party         |
| `offense`         | `u8`      | 0 = DoubleSigning, 1 = MissedDuty, 2 = FraudProof |
| `penalty_bps`     | `u32`     | Penalty in basis points (1 bps = 0.01%)          |
| `ledger_sequence` | `u32`     | Ledger at which the misbehaviour occurred        |
| `timestamp`       | `u64`     | Unix timestamp of the misbehaviour               |
| `extra`           | `Bytes`   | Optional: raw proof data (double-sign block headers, etc.) |

Hash construction (off-chain):
```rust
let payload = (offender, offense, penalty_bps, ledger_sequence, timestamp, extra);
let evidence_hash = sha256(encode(payload));
```

The `evidence_hash` is stored on-chain and acts as the primary key for the slash record. **It can only be used once** (replay protection).

---

## Quorum

For **attestation-based slashes**, a configurable quorum `K` of distinct slasher addresses must call `attest_slash()` with the same `evidence_hash` before the slash can be executed. The default is **2-of-N**.

- The first attestor creates the slash record and moves funds to escrow.
- Each subsequent attestor countersigns the existing record.
- Once `K` signatures are collected and the appeal window closes, anyone can call `execute_slash()`.

For **evidence-based slashes**, quorum is bypassed — a single slasher with a valid evidence hash is sufficient to initiate. This is appropriate for cryptographically verifiable offences (e.g. double-signing proofs).

---

## Slash Lifecycle

```
                    slash_with_evidence()
                    attest_slash() × K
                           │
                           ▼
                        Pending  ◄─── appeal window open (7 days)
                        │     │
              appeal    │     │  no appeal / appeal window closed
              raised    │     │
                        │     ▼
                        │  execute_slash()
                        │     │
                        │     ▼
                        │  [Executed]  ──► second execute_slash() → InvalidState (8)
                        │               (terminal: funds burned/redistributed)
                        │
                        ▼
                   resolve_appeal()
                   ┌────────┴─────────┐
               uphold              reject
                  │                   │
                  ▼                   ▼
              [Reversed]        [AppealRejected]
          (terminal: funds     (terminal: funds
            returned)             burned)
```

### Terminal States

Once a slash record enters one of the three terminal states below, no further
state transitions are possible. Any operation that requires `Pending` status
returns `SlashError::InvalidState (8)`.

| State           | How reached                        | Funds outcome              |
|-----------------|------------------------------------|----------------------------|
| `Executed`      | `execute_slash()` after appeal window expires | Escrowed amount burned/redistributed |
| `Reversed`      | `resolve_appeal(uphold=true)`      | Escrowed amount returned to offender stake |
| `AppealRejected`| `resolve_appeal(uphold=false)`     | Escrowed amount burned/redistributed |

### Double-Execution Guard (`Executed` state)

The `Executed` terminal state is the primary defence against applying a penalty
more than once to the same slash record.

**How it works:**

1. `execute_slash` reads the slash record and checks `status == Pending`.
2. If the check passes, it burns the escrowed amount and atomically writes
   `status = Executed` to storage.
3. Any subsequent `execute_slash` call for the same `evidence_hash` finds
   `status == Executed`, fails the `Pending` check, and returns
   `SlashError::InvalidState (8)` **before touching any balances**.

```
Call 1: status == Pending  → burns escrow, sets status = Executed  → Ok(())
Call 2: status == Executed → returns Err(InvalidState)             ← guard fires
Call N: status == Executed → returns Err(InvalidState)             ← guard fires
```

This guarantees the offender's stake is debited by the penalty **exactly once**,
regardless of how many callers retry `execute_slash` for the same record.

The guard applies equally to evidence-based and attestation-based slashes, since
both go through the same `execute_slash` entry point.

---

## Security Assumptions

### Role Separation
- The `admin` address can: grant/revoke slasher roles, resolve appeals.
- The `admin` address **cannot** initiate slashes.
- `slasher` addresses can: initiate slashes, countersign attestations.
- `slasher` addresses **cannot** resolve appeals.

This separation ensures no single actor can both slash and unilaterally decide appeals.

### Proportionality and Saturating Caps
- Penalties are expressed in **basis points** (bps) of the offender's current stake.
- The hard protocol ceiling is **5 000 bps (50%)**.
- A configurable `per_event_bps_cap` is enforced and must be `<= 5 000`.
- A configurable `per_period_amount_cap` is enforced over a rolling `period_secs` window.
- A configurable `lifetime_amount_cap` is enforced over full contract lifetime.
- Zero-penalty slashes are rejected at the contract level.
- Any slash exceeding cumulative caps is rejected (`PeriodCapExceeded` or `LifetimeCapExceeded`).

### Replay Protection
- Each `evidence_hash` is stored in a `used_evidence` set after first use.
- Submitting the same hash twice returns `DuplicateEvidence`.

### Escrow During Appeal
- Slashed funds are **not burned immediately**. They are moved from the offender's stake to an escrow map keyed by `evidence_hash`.
- Funds only leave escrow when `execute_slash()` or `resolve_appeal()` is called.
- This ensures the offender is not irreversibly harmed during the appeal window.

### No Self-Slashing
- The admin role and slasher role are distinct and assigned explicitly.
- A slasher cannot grant themselves the admin role.

---

## Public Interface

### Initialisation

```rust
pub fn initialize(
    env: Env,
    admin: Address,
    token: Address,
    quorum: u32,
    per_event_bps_cap: u32,
    per_period_amount_cap: i128,
    lifetime_amount_cap: i128,
    period_secs: u64,
)
```

Must be called once. Sets the admin, staking token, attestation quorum, and slashing cap bounds.

```rust
pub fn set_penalty_caps(
    env: Env,
    per_event_bps_cap: u32,
    per_period_amount_cap: i128,
    lifetime_amount_cap: i128,
    period_secs: u64,
)
pub fn get_penalty_caps(env: Env) -> PenaltyCaps
```

### Role Management (Admin only)

```rust
pub fn add_slasher(env: Env, slasher: Address)
pub fn remove_slasher(env: Env, slasher: Address)
```

### Stake Management

```rust
pub fn stake(env: Env, staker: Address, amount: i128)
pub fn unstake(env: Env, staker: Address, amount: i128)
```

### Slashing

```rust
// Single-slasher evidence-based slash
pub fn slash_with_evidence(
    env: Env,
    initiator: Address,
    offender: Address,
    offense: Offense,
    penalty_bps: u32,
    evidence_hash: BytesN<32>,
    offense_ts: u64,
) -> Result<BytesN<32>, SlashError>

// Multi-slasher attestation-based slash
pub fn attest_slash(
    env: Env,
    attestor: Address,
    offender: Address,
    offense: Offense,
    penalty_bps: u32,
    evidence_hash: BytesN<32>,
    offense_ts: u64,
) -> Result<(), SlashError>
```

### Appeal

```rust
// Offender raises appeal (within 7 days of slash)
pub fn raise_appeal(env: Env, offender: Address, evidence_hash: BytesN<32>)

// Admin resolves appeal: uphold=true returns funds, uphold=false burns them
pub fn resolve_appeal(env: Env, evidence_hash: BytesN<32>, uphold: bool)

// Anyone can finalise a slash after appeal window closes
pub fn execute_slash(env: Env, evidence_hash: BytesN<32>)
```

### Views

```rust
pub fn get_slash_record(env: Env, evidence_hash: BytesN<32>) -> Option<SlashRecord>
pub fn get_stake_balance(env: Env, staker: Address) -> i128
pub fn get_slashers(env: Env) -> Vec<Address>
pub fn get_quorum(env: Env) -> u32
```

---

## Offense Types

| Variant        | Description                                          | Typical Evidence              |
|----------------|------------------------------------------------------|-------------------------------|
| `DoubleSigning`| Validator signed two conflicting blocks at same height | Two signed block headers      |
| `MissedDuty`   | Validator missed a required attestation or proposal  | Duty schedule + absence proof |
| `FraudProof`   | Invalid state transition or verifiable fraud         | Merkle proof / STF trace      |

---

## Error Reference

| Code | Name                  | Cause                                              |
|------|-----------------------|----------------------------------------------------|
| 1    | `Unauthorized`        | Caller does not hold the required role             |
| 2    | `DuplicateEvidence`   | Evidence hash already used                         |
| 3    | `PenaltyTooHigh`      | Penalty exceeds configured/event maximum bps       |
| 4    | `InsufficientStake`   | Offender has no stake or stake < slash amount      |
| 5    | `AppealWindowOpen`    | Cannot execute — appeal window still active        |
| 6    | `AppealWindowClosed`  | Cannot raise appeal — deadline passed              |
| 7    | `RecordNotFound`      | No slash record for given evidence hash            |
| 8    | `InvalidState`        | Operation not valid in current slash status. Returned by `execute_slash` when the record is already `Executed`, `Reversed`, or `AppealRejected` — this is the **double-execution guard** |
| 9    | `QuorumNotMet`        | Not enough attestors have signed                   |
| 10   | `AlreadyAttested`     | Slasher already countersigned this slash           |
| 11   | `ZeroPenalty`         | Penalty basis points cannot be zero                |
| 12   | `AlreadyInitialized`  | Contract has already been initialised              |
| 13   | `InvalidConfig`       | Invalid cap config (zero/negative/inconsistent)    |
| 14   | `PeriodCapExceeded`   | Cumulative slashing exceeds configured period cap   |
| 15   | `LifetimeCapExceeded` | Cumulative slashing exceeds configured lifetime cap |
| 16   | `ArithmeticOverflow`  | Overflow/underflow protection triggered             |
| 17   | `ZeroQuorum`          | Quorum must be > 0; passing 0 is rejected rather than silently raised to the default |

---

## Deployment

```bash
# Build
cargo build --target wasm32-unknown-unknown --release

# Run tests
cargo test -- --nocapture

# Deploy to Stellar testnet
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/slashing_penalty.wasm \
  --source <deployer-keypair> \
  --network testnet

# Initialise
stellar contract invoke \
  --id <contract-id> \
  --source <admin-keypair> \
  --network testnet \
  -- initialize \
  --admin <admin-address> \
  --token <token-address> \
  --quorum 2
```

---

## Test Coverage

The test suite covers:

- Initialisation and double-init protection
- Zero-quorum rejection (`ZeroQuorum` error, never silently raised to default)
- Role management (add/remove slasher)
- Stake deposit and withdrawal including insufficient balance
- Evidence-based slash: happy path, zero slash, max slash, above-max, duplicate evidence, no stake
- Attestation-based slash: quorum enforcement, double attestation rejection, quorum-met execute
- Appeal window: execute before/after deadline, raise appeal in/out of window
- Appeal resolution: upheld (funds returned), rejected (funds burned), double-resolution rejection
- Repeated offences with distinct evidence hashes
- Edge cases: unknown hash, execute non-existent, appeal boundary at exact deadline
- Replay protection: O(1) keyed lookup, rejection independent of prior slash count
- **Double-execution guard (issue #938)**:
  - `test_execute_slash_double_execution_is_rejected` — second `execute_slash` on the same record returns `InvalidState`
  - `test_execute_slash_stake_balance_reflects_single_execution` — stake is debited by exactly one penalty; the rejected second call does not alter the balance
  - `test_attestation_slash_execute_slash_double_execution_is_rejected` — guard applies equally to attestation-based slashes

---

## Notes for Auditors

1. **Escrow isolation**: Each slash's escrowed amount is keyed by `evidence_hash`. Concurrent slashes against the same offender are independent and cannot interfere.
2. **Token transfer**: The `stake()` call triggers a real token transfer into the contract. Ensure the token contract is trusted and non-reentrant.
3. **Burn address**: The current `burn_escrow()` implementation retains funds in the contract as a treasury. For production, replace with a transfer to a designated burn address or distribution logic.
4. **Ledger timestamp**: All time comparisons use `env.ledger().timestamp()`. Validators control block timestamps within bounds — consider adding a tolerance margin for `offense_timestamp` validation.
5. **Quorum replay**: A slasher removed from the role list after attesting still counts toward quorum for that slash record. Consider snapshotting the slasher list per slash if this is a concern.
6. **Cap tuning**: Keep `per_period_amount_cap <= lifetime_amount_cap` and size caps below expected concentration risk to bound repeated-slash abuse.
7. **Threat model**: Period/lifetime checks are applied at slash creation, so repeated events (including burst submissions in one ledger window) saturate and reject once caps are reached.