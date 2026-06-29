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

Run locally with:

```bash
cargo test -p governance
```
