# Slashing Penalty Contract

`onchain/contracts/slashing_penalty`

---

## Overview

The Slashing Penalty contract enforces on-chain penalties against network
participants who commit verifiable misbehaviour (e.g. double-signing, missed
duties, fraud proofs).  Penalties are proportional, capped, and subject to a
7-day appeal window before funds are burned or redistributed.

---

## Roles

| Role    | Capabilities                                                    |
|---------|-----------------------------------------------------------------|
| `admin` | `add_slasher`, `remove_slasher`, `resolve_appeal`, `set_penalty_caps` |
| `slasher` | `slash_with_evidence`, `attest_slash`                         |
| Anyone  | `stake`, `unstake`, `raise_appeal`, `execute_slash`             |

Admin and slasher are separate roles.  An admin cannot slash directly; a slasher
cannot administer the contract.

---

## Slash Lifecycle

```
slash_with_evidence()          attest_slash() × N
        │                              │
        └──────────┬───────────────────┘
                   ▼
                Pending   ◄─── appeal window open (7 days)
                │     │
     raise_appeal│     │  no appeal / window expired
                │     │
                │     ▼
                │  execute_slash()  →  Executed  (terminal)
                │
                ▼
          resolve_appeal()
          ┌──────┴───────┐
      uphold           reject
         │                │
         ▼                ▼
      Reversed      AppealRejected
   (funds returned)  (funds burned)
```

Once a record reaches `Executed`, `Reversed`, or `AppealRejected` it is
permanently terminal.  Any further call to `execute_slash` or `resolve_appeal`
returns `SlashError::InvalidState (8)`.

---

## Attestation-Based Slashes

When on-chain evidence is unavailable, a slash may be initiated by collecting
signed attestations from `quorum_threshold` distinct slashers.

### Flow

1. The first slasher calls `attest_slash` — a `SlashRecord` is created,
   funds are moved to escrow, and the appeal clock starts.
2. Additional slashers call `attest_slash` to countersign.
3. Once at least `quorum_threshold` unique attestors have signed, anyone may
   call `execute_slash` after the appeal window closes.

### Quorum check in `execute_slash`

```rust
if (record.attestors.len() as u32) < quorum && record.attestors.len() > 0 {
    return Err(SlashError::QuorumNotMet);
}
```

Evidence-based slashes bypass this check (`attestors` is empty, so the
`len() > 0` branch is never entered).

---

### Evidence-Hash-Mismatch Guard

The contract **re-validates** the evidence hash against the originally attested reference at both execution time (`execute_slash`) and countersign time (`attest_slash`):

- **`execute_slash`**: After looking up the `SlashRecord` by the submitted `evidence_hash` (map key), the contract explicitly checks that `record.evidence_hash == evidence_hash`. Under normal operation these always match (the record is stored with the same hash as its key), so this is a **defense-in-depth** check that guards against storage-corruption edge cases.
- **`attest_slash` (countersign path)**: When a slasher countersigns an existing record, the same check is applied: `record.evidence_hash == evidence_hash`. This ensures the slasher's submitted evidence hash matches the one recorded at initial attestation.

If either check fails, the contract returns `SlashError::EvidenceHashMismatch (18)` **before** performing any state mutation.

```
Create:  attest_slash(H1)  → store record with key=H1, record.evidence_hash=H1
Execute: execute_slash(H1) → lookup by H1, check H1 == record.evidence_hash → ✓
Corrupt: (direct storage edit) → record.evidence_hash ← H2
Execute: execute_slash(H1) → lookup by H1, check H1 == record.evidence_hash → ✗ (EvidenceHashMismatch)
```

**Why it matters:** The evidence hash serves as both the record identifier (map key) and a field within the record. While these are always consistent under normal contract operation, this explicit validation protects against:
- Storage corruption from low-level bugs in Soroban host functions.
- Abnormal state transitions from edge-case reentrancy or interrupted writes.

---

## Security Assumptions

**Principle:** authorisation for `attest_slash` is evaluated at the ledger in
which the call is made, not at the ledger in which the slash was first proposed.

### Rules

| Situation | Outcome |
|-----------|---------|
| `attestor` is in `get_slashers()` at call time | Call proceeds normally |
| `attestor` was removed via `remove_slasher` before this call | Rejected — `SlashError::Unauthorized` |
| `attestor` was removed *after* a prior attestation was accepted | The prior attestation remains in `record.attestors`; future attempts are rejected |

### Why forward-only removal?

Retroactively invalidating already-recorded attestations would complicate the
quorum calculation (the quorum could silently drop below threshold at any future
ledger) and open a denial-of-service vector where an admin can prevent execution
of a legitimate slash by removing one of the attesting slashers after quorum was
met.

The chosen model is simpler and more secure:

- Every entry in `record.attestors` was **valid at the time it was recorded**.
- `execute_slash` checks `attestors.len() >= quorum` against the frozen list; it
  does **not** re-validate whether each attestor is still in `get_slashers()`.
- Removal is **prospective only**: it prevents the removed address from submitting
  or countersigning future attestations, including countersignatures on slash
  records that are still `Pending`.

### Security rationale

- An attacker who briefly compromises a slasher account, submits an attestation,
  and is then removed via `remove_slasher` cannot add further attestations to
  push the slash closer to quorum.
- An admin cannot silently kill a legitimate slash-in-progress by removing one
  attesting slasher after quorum is reached — the recorded attestors still count.
- A removed address has zero influence over any *future* slash, regardless of its
  prior history.

---

## Penalty Caps

All slashes are bounded by a layered cap system:

| Cap | Description |
|-----|-------------|
| `per_event_bps_cap` | Maximum per-event penalty in basis points (hard ceiling: `MAX_PENALTY_BPS = 5 000`). |
| `per_period_amount_cap` | Maximum cumulative amount slashed from one offender within a rolling period. |
| `lifetime_amount_cap` | Maximum cumulative amount slashed from one offender across the contract lifetime. |
| `period_secs` | Length of the rolling period used for `per_period_amount_cap`. |

Caps are validated at `initialize` and `set_penalty_caps`; invalid configurations
(zero values, period cap > lifetime cap, event cap > `MAX_PENALTY_BPS`) are
rejected with `SlashError::InvalidConfig`.

---

## Replay Protection

Each `evidence_hash` (SHA-256 of the raw evidence payload) may be used **once**.
A keyed `Map<BytesN<32>, bool>` stores every consumed hash; lookup is O(1)
regardless of slash history.  Reusing a hash returns `SlashError::DuplicateEvidence`.

---

## Double-Execution Guard

`execute_slash` atomically transitions the slash record from `Pending` to
`Executed` on the first successful call.  Any subsequent call for the same hash
finds the record in `Executed` state and returns `SlashError::InvalidState (8)`
before touching any balances.  This ensures the penalty is applied **exactly
once**.

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
| 18   | `EvidenceHashMismatch`| The evidence hash submitted at execution does not match the reference recorded at attestation time. Defense-in-depth check that guards against storage-corruption edge cases where the map key and stored `evidence_hash` could diverge |

---

## Related Tests

All behavioural requirements are covered in
`onchain/contracts/slashing_penalty/tests/integration_test.rs`.

Key test groups:

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
- **Maximum slash percentage cap**:
  - `test_slash_at_percentage_cap_succeeds` — slash exactly at a custom per-event bps cap succeeds
  - `test_slash_above_percentage_cap_fails` — slash 1 bps above a custom cap is rejected with `PenaltyTooHigh`
  - `test_execute_slash_respects_percentage_cap` — full lifecycle: slash at cap, execute, verify correct amount end-to-end
  - `test_attestation_slash_at_percentage_cap_succeeds` — attestation-based slash at cap succeeds
  - `test_attestation_slash_above_percentage_cap_fails` — attestation-based slash above cap fails
  - `test_zero_per_event_bps_cap_rejected` — zero per-event cap is rejected at init
  - `test_per_event_bps_cap_exceeds_max_rejected` — cap above `MAX_PENALTY_BPS` (5 000) is rejected
  - `test_max_bps_boundary_slash_succeeds` — slash at hard `MAX_PENALTY_BPS` through full slash-with-evidence path
  - `test_update_cap_then_enforce` — lowering the cap via `set_penalty_caps` correctly rejects previously-valid slashes
- **Evidence-hash-mismatch rejection (issue reference)**:
  - `test_execute_slash_matching_evidence_hash_succeeds` — evidence-based slash executes when the hash matches the recorded reference
  - `test_execute_slash_attestation_matching_evidence_hash_succeeds` — attestation-based slash executes when the hash matches
  - `test_execute_slash_wrong_hash_key_returns_record_not_found` — submitting a different (unused) hash at execute time returns `RecordNotFound`
  - `test_execute_slash_rejects_storage_corrupted_evidence_hash` — defense-in-depth: storage corruption that diverges the stored `evidence_hash` from the map key triggers `EvidenceHashMismatch`
  - `test_attest_slash_countersign_rejects_mismatched_evidence_hash` — countersign path rejects a corrupted record whose stored `evidence_hash` diverges from the submitted hash
  - `test_attest_slash_countersign_matching_hash_succeeds` — countersign with matching hash succeeds under normal operation

---

## Notes for Auditors

1. **Escrow isolation**: Each slash's escrowed amount is keyed by `evidence_hash`. Concurrent slashes against the same offender are independent and cannot interfere.
2. **Token transfer**: The `stake()` call triggers a real token transfer into the contract. Ensure the token contract is trusted and non-reentrant.
3. **Burn address**: The current `burn_escrow()` implementation retains funds in the contract as a treasury. For production, replace with a transfer to a designated burn address or distribution logic.
4. **Ledger timestamp**: All time comparisons use `env.ledger().timestamp()`. Validators control block timestamps within bounds — consider adding a tolerance margin for `offense_timestamp` validation.
5. **Quorum replay**: A slasher removed from the role list after attesting still counts toward quorum for that slash record. Consider snapshotting the slasher list per slash if this is a concern.
6. **Cap tuning**: Keep `per_period_amount_cap <= lifetime_amount_cap` and size caps below expected concentration risk to bound repeated-slash abuse.
7. **Threat model**: Period/lifetime checks are applied at slash creation, so repeated events (including burst submissions in one ledger window) saturate and reject once caps are reached.