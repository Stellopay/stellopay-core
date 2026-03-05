## Governance and Voting Contract

The `governance` contract provides on-chain governance primitives for Stellopay
deployments. It supports configurable quorum and voting periods, proposal
creation and voting, and time-locked execution of approved proposals.

### Contract Location

- Contract: `onchain/contracts/governance/src/lib.rs`
- Tests: `onchain/contracts/governance/tests/test_governance.rs`

### Design Goals

- **Configurable quorum and timing** – Owners can set quorum (in basis points),
  voting period duration, and an execution timelock.
- **Simple voting model** – Explicit voter weights (voting power) managed by
  the owner; each voter can vote once per proposal.
- **Time-locked execution** – Successful proposals enter a queued state and can
  only be executed after a configured timelock.
- **Integration with existing admin/owner roles** – The same owner that
  controls upgrade or admin flows for other contracts can be used as the
  governance owner or delegate voting power to additional addresses.
- **Auditability** – Proposals and votes are stored on-chain with clear
  configuration and outcome state.

### Data Model

- `ProposalKind`
  - `ParameterChange { key: Symbol, value: i128 }` – Generic on-chain parameter
    slot stored under the given key.
  - `UpgradeContract { target: Address, new_wasm_hash: BytesN<32> }` –
    Governance approval for a contract upgrade; the actual upgrade is performed
    by an owner/upgrader using this recorded hash.
  - `ArbiterChange { new_arbiter: Address }` – Records an approved arbiter
    address for downstream dispute-resolution flows.
- `ProposalStatus`
  - `Active`, `Succeeded`, `Defeated`, `Cancelled`, `Executed`
- `Proposal`
  - `id`, `proposer`, `kind`, `status`
  - `for_votes`, `against_votes`, `abstain_votes`
  - `start_time`, `end_time`, `eta` (execution time after timelock)
- `VoteChoice`
  - `For`, `Against`, `Abstain`
- Voter weights
  - `VoterPower(Address) -> i128`
  - `TotalVotingPower` aggregate for quorum calculations

Quorum is expressed in basis points (`quorum_bps`) over
`TotalVotingPower`. Participation and approvals are computed from the sum of
`for_votes`, `against_votes`, and `abstain_votes`.

### Public API

Initialization and configuration:

- `initialize(owner, quorum_bps, voting_period_seconds, timelock_seconds)`
- `update_config(caller, quorum_bps, voting_period_seconds, timelock_seconds)`

Voter management:

- `set_voter_power(caller, voter, power)`
- `get_voter_power(voter) -> i128`
- `get_total_voting_power() -> i128`

Governance workflow:

- `propose(proposer, kind) -> proposal_id`
- `vote(voter, proposal_id, choice)`
- `queue(proposal_id)` – Finalizes outcome and, on success, sets `eta`.
- `execute(proposal_id)` – After timelock expiry, applies the intent:
  - `ParameterChange` writes `key -> value` into governance storage.
  - `ArbiterChange` updates the stored arbiter address.
  - `UpgradeContract` records an approved hash per target address.
- `cancel(caller, proposal_id)` – Owner-only cancellation before execution.

Read helpers:

- `get_config() -> (owner, quorum_bps, voting_period_seconds, timelock_seconds)`
- `get_proposal(proposal_id) -> Option<Proposal>`
- `get_vote(proposal_id, voter) -> Option<VoteChoice>`
- `get_parameter(key) -> Option<i128>`
- `get_arbiter() -> Option<Address>`
- `get_approved_upgrade(target) -> Option<BytesN<32>>`

### Governance Flow

1. **Configuration** – Contract owner initializes and optionally updates
   quorum, voting period, and timelock.
2. **Voter setup** – Owner assigns voting power to authorized addresses, which
   can reflect RBAC roles, multisig signers, or token-weighted governance.
3. **Proposal creation** – Any address with non-zero voting power can create a
   proposal.
4. **Voting** – Voters cast a single `For`, `Against`, or `Abstain` vote while
   the proposal is `Active` and within its `[start_time, end_time]` window.
5. **Queue** – After the voting period ends, anyone can call `queue` to compute
   quorum and approval and, if satisfied, transition the proposal to
   `Succeeded` and set its `eta`.
6. **Execution** – After `eta` and the configured timelock, `execute` applies
   the proposal’s effect in governance storage for integration or off-chain
   tooling.
7. **Cancellation** – The owner can cancel `Active` or `Succeeded` proposals as
   an emergency override.

### Integration Notes

- **Admin/owner roles** – The governance owner should typically be the same
  principal (wallet, multisig, or RBAC-admin) that controls upgrades and admin
  operations for the broader Stellopay deployment.
- **Upgrade and arbiter intents** – Other contracts or operational tooling
  should read:
  - `get_approved_upgrade(target)` before performing an upgrade on `target`.
  - `get_arbiter()` when configuring arbiters in dispute or payroll contracts.
- **Parameter reads** – Contracts can integrate by reading specific keys from
  `get_parameter(key)` for feature flags, risk limits, or rate bounds.

### Security Considerations

- Keep `quorum_bps` and voter power assignments aligned with your risk model.
- Use multisig and/or RBAC for the governance owner where possible.
- Prefer long enough `voting_period_seconds` and `timelock_seconds` to allow
  monitoring and intervention for sensitive proposals (e.g. upgrades).
- Snapshot-style behavior is implicit: voting power is read at vote time; if
  using tokenized governance externally, ensure power changes are coordinated to
  avoid unexpected outcomes.

