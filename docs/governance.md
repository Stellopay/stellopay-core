## Governance Contract

The `governance` contract implements an on-chain proposal lifecycle for
Stellopay. It is designed to work with three existing contracts:

- `rbac` decides who is allowed to propose and vote.
- `withdrawal_timelock` delays execution after a proposal passes.
- `multisig` decides who is allowed to trigger final execution.

### Contract Location

- Contract: `onchain/contracts/governance/src/lib.rs`
- Tests: `onchain/contracts/governance/tests/governance_tests.rs`

### Core Flow

1. An address with the RBAC `Admin` or `Employer` role calls
   `create_proposal`.
2. Eligible voters cast `For`, `Against`, or `Abstain` votes with
   `cast_vote`.
3. After the voting window closes, anyone can call `finalize_proposal`.
4. If quorum is met and `for_votes > against_votes`, governance queues an
   `AdminChange` operation in `withdrawal_timelock`.
5. After the timelock `eta` is reached, a configured multisig signer calls
   `execute_proposal`.
6. Governance executes the timelock operation and then applies the proposal’s
   state change.

### Proposal Types

- `ParameterChange(Symbol, i128)`
  Stores a generic governance parameter under a symbol key.
- `UpgradeContract(Address, BytesN<32>)`
  Records an approved WASM hash for a target contract.
- `ArbiterChange(Address)`
  Records an approved arbiter address for downstream integrations.

### Public Entrypoints

- `initialize(owner, rbac_contract, multisig_contract, timelock_contract, quorum_votes, voting_period_seconds)`
- `update_config(caller, quorum_votes, voting_period_seconds)`
- `create_proposal(proposer, kind)`
- `cast_vote(voter, proposal_id, choice)`
- `finalize_proposal(proposal_id)`
- `execute_proposal(executor, proposal_id)`
- `cancel_proposal(caller, proposal_id)`

Backward-compatible aliases are also present for earlier local names:
`propose`, `vote`, `queue`, `execute`, and `cancel`.

### Configuration Model

- `quorum_votes` is an absolute participation threshold, not a percentage.
- Governance uses a **proposal-creation snapshot** for quorum. Each proposal
  stores the configured `quorum_votes` value when `create_proposal` succeeds,
  and `finalize_proposal` evaluates participation against that stored value.
- `update_config` affects only proposals created after the update. Raising or
  lowering `quorum_votes` while a proposal is active cannot retroactively make
  that proposal pass or fail.
- The quorum threshold is not recomputed from the live RBAC voter set, staking,
  or any other external voting-power source. Deployments that change the voter
  population should update `quorum_votes`; the new value will apply to future
  proposals.
- `voting_period_seconds` controls how long proposals stay open. Both `initialize`
  and `update_config` enforce that it falls within
  `[MIN_VOTING_PERIOD_SECONDS, MAX_VOTING_PERIOD_SECONDS]`:
  - `MIN_VOTING_PERIOD_SECONDS = 3600` (1 hour) ensures voters have a realistic
    window to participate.
  - `MAX_VOTING_PERIOD_SECONDS = 2_592_000` (30 days) prevents a misconfigured
    admin from setting a value near `u64::MAX`, which would trap proposals in
    effectively perpetual voting and freeze governance.
  - Values outside this range (including zero) are rejected with
    `GovernanceError::VotingPeriodOutOfBounds`.
- The timelock delay is owned by the linked `withdrawal_timelock` contract.
- The governance contract does not store a separate execution delay.

### RBAC Integration

Governance eligibility is checked live against the linked `rbac` contract.

- `Admin` can propose and vote.
- `Employer` can propose and vote.
- Any other role, or no role, is rejected.

Because checks are live, role changes take effect immediately for future
proposal creation and future votes that have not yet been cast.
They do not change the quorum snapshot of an existing proposal. A vote that was
validly cast before a role is revoked remains included in that proposal's stored
totals.

### Timelock Integration

When a proposal succeeds, governance queues a timelock operation and stores:

- `timelock_operation_id`
- `eta`

`execute_proposal` refuses to proceed before the timelock is ready.

Important deployment requirement:

- The `withdrawal_timelock` contract must be initialized with the governance
  contract address as its `admin`, otherwise governance will not be able to
  queue, execute, or cancel timelock operations.

### Multisig Integration

Proposal execution is restricted to addresses returned by
`multisig.get_signers()`.

This means a passed and matured proposal still cannot be executed by an
arbitrary account. Only configured multisig signers can trigger the final
state transition.

### Security Notes

- Voting eligibility is role-based, so RBAC integrity is critical.
- Execution is intentionally split into two gates:
  RBAC for governance participation, and multisig signers for execution.
- The timelock creates a review window between approval and execution.
- Cancelling a succeeded proposal also cancels its queued timelock operation.
- Quorum is absolute, so deployments should set `quorum_votes` to reflect the
  expected number of active governance participants.
- Snapshotting quorum prevents configuration or voter-population changes from
  changing the rules of an active proposal. This avoids both last-minute quorum
  inflation that blocks a proposal and last-minute quorum reduction that makes
  an under-participated proposal pass.

### get_approved_upgrade() — Quorum and Majority Gating

The `get_approved_upgrade(target)` function returns an approved WASM hash only
when a proposal passes **both** quorum and majority thresholds simultaneously.

#### Approval Conditions

Both conditions must be satisfied:

| Condition | Formula | Significance |
|-----------|---------|--------------|
| **Quorum** | `total_votes >= proposal.quorum_votes` | Ensures sufficient participation |
| **Majority** | `for_votes > against_votes` | Ensures clear directional consensus |

Where `total_votes = for_votes + against_votes + abstain_votes`.

Note: Abstain votes count toward quorum but not majority. Only For and Against
votes participate in the majority calculation.

#### Conditional Approval Matrix

| Quorum Met | Majority Met | get_approved_upgrade() Returns |
|------------|--------------|--------------------------------|
| ❌ No     | ❌ No        | `None` (proposal defeated)     |
| ✅ Yes    | ❌ No        | `None` (majority failed)       |
| ❌ No     | ✅ Yes       | `None` (quorum failed)         |
| ✅ Yes    | ✅ Yes       | `Some(hash)` (approved)        |

#### Why Both Conditions Matter

- **Quorum alone** would allow a small minority to approve upgrades when most
  token holders are absent or unengaged.
- **Majority alone** (without quorum) would allow a tiny group — even two
  voters, one yes — to approve with 51%.
- **Both together** ensure meaningful participation AND clear directional
  consensus before governance decisions take effect.

#### Approval Lifecycle

1. A proposal of kind `UpgradeContract(target, wasm_hash)` is created.
2. Eligible voters cast For, Against, or Abstain votes.
3. After voting closes, `finalize_proposal` checks both conditions:
   - If `total_votes < quorum_votes` OR `for_votes <= against_votes`, proposal
     is marked `Defeated`.
   - Otherwise, proposal is marked `Succeeded` and queued in timelock.
4. After the timelock delay, a multisig signer calls `execute_proposal`.
5. Execution persists the approved hash to storage under the target address.
6. Calling `get_approved_upgrade(target)` returns `Some(hash)` if and only if
   the proposal reached both thresholds.

#### Configuration

- `quorum_votes` — absolute number of votes required (not percentage)
- Default in tests: 2 votes
- Can be updated via `update_config` (affects only future proposals)

#### Test Coverage

Quorum and majority gating is tested with 11 dedicated tests:

1. `get_approved_upgrade_neither_quorum_nor_majority()` — No votes → None
2. `get_approved_upgrade_quorum_met_majority_not_met()` — Quorum ✓ Majority ✗ → None
3. `get_approved_upgrade_majority_met_quorum_not_met()` — Quorum ✗ Majority ✓ → None
4. `get_approved_upgrade_both_quorum_and_majority_met()` — Both ✓ → Some(hash)
5. `get_approved_upgrade_abstain_votes_count_toward_quorum_not_majority()` — Validates abstain behavior
6. `get_approved_upgrade_quorum_boundary_one_short()` — One vote short of quorum
7. `get_approved_upgrade_quorum_boundary_at_threshold()` — Exactly at quorum
8. `get_approved_upgrade_majority_boundary_tie_fails()` — For=Against (tie) → None
9. `get_approved_upgrade_majority_boundary_loss_one_vote()` — For<Against → None
10. `get_approved_upgrade_majority_boundary_win_by_one()` — For>Against (barely) → Some(hash)
11. `get_approved_upgrade_multiple_proposals_independent()` — Multiple targets tracked independently

#### Security Notes

- Both conditions are checked atomically in `finalize_proposal`.
- A proposal meeting only one condition is never surfaced.
- Zero votes: `total_votes = 0 < quorum` → proposal defeated (no division by zero).
- Thresholds use integer arithmetic to avoid floating-point precision issues.
- Once persisted to storage, an approved upgrade hash represents governance
  consensus backed by both gates and cannot be modified.

### Test Coverage

The governance test suite covers:

- initialization and dependency wiring
- RBAC-gated proposal creation and voting
- double-vote prevention
- quorum failure and rejection paths
- timelock queueing and early-execution rejection
- multisig signer enforcement
- proposal cancellation after success
- parameter, arbiter, and upgrade execution paths
- live RBAC role revocation impact on future voting
- proposal-time quorum snapshots when configuration and voting power change
  during an active vote
- **quorum and majority gating for get_approved_upgrade** (11 dedicated tests)

Run locally with:

```bash
cargo test -p governance
```
