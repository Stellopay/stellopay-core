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

## Point-in-Time Authorisation for Attestations

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

| Code | Name | Meaning |
|------|------|---------|
| 1 | `Unauthorized` | Caller is not an authorised slasher (or admin for admin-only calls). |
| 2 | `DuplicateEvidence` | `evidence_hash` has already been used. |
| 3 | `PenaltyTooHigh` | `penalty_bps` exceeds the cap or `MAX_PENALTY_BPS`. |
| 4 | `InsufficientStake` | Offender has no stake or insufficient stake to cover the penalty. |
| 5 | `AppealWindowOpen` | `execute_slash` called before the appeal deadline. |
| 6 | `AppealWindowClosed` | `raise_appeal` called after the deadline. |
| 7 | `RecordNotFound` | No slash record for the given `evidence_hash`. |
| 8 | `InvalidState` | Operation not valid in the record's current state (double-execution guard). |
| 9 | `QuorumNotMet` | Attestation count is below `quorum_threshold`. |
| 10 | `AlreadyAttested` | Slasher has already attested to this slash. |
| 11 | `ZeroPenalty` | `penalty_bps` is zero, or computed slash amount rounds to zero. |
| 12 | `AlreadyInitialized` | `initialize` called more than once. |
| 13 | `InvalidConfig` | Penalty cap configuration is invalid. |
| 14 | `PeriodCapExceeded` | Slash would exceed the rolling period cap for this offender. |
| 15 | `LifetimeCapExceeded` | Slash would exceed the lifetime cap for this offender. |
| 16 | `ArithmeticOverflow` | An intermediate calculation overflowed. |
| 17 | `ZeroQuorum` | `quorum` argument to `initialize` was zero. |

---

## Related Tests

All behavioural requirements are covered in
`onchain/contracts/slashing_penalty/tests/integration_test.rs`.

Key test groups:

| Test name | What it proves |
|-----------|----------------|
| `test_removed_slasher_attestation_rejected` | A slasher removed via `remove_slasher` cannot submit new attestations or countersign existing pending records. |
| `test_pre_removal_attestations_count_toward_quorum` | Attestations accepted before removal remain counted and allow `execute_slash` to succeed once quorum was met. |
| `test_attestation_requires_quorum_before_execute` | `execute_slash` fails with `QuorumNotMet` when attestor count is below the threshold. |
| `test_attestation_quorum_met_allows_execute` | `execute_slash` succeeds once quorum is met and the appeal window has closed. |
| `test_double_attestation_by_same_slasher_fails` | A slasher cannot countersign the same slash twice. |
| `test_execute_slash_double_execution_is_rejected` | The double-execution guard fires on a second `execute_slash` call. |
