# Payment Dispute Escalation

The `dispute_escalation` contract manages the lifecycle of payment disputes within the Stello Pay ecosystem. It provides a structured, multi-tier resolution process designed to encourage fair and timely outcomes for all parties involved.

## Architecture & Integration

The escalation process works independently of the core payroll/payment contracts but links to them via `agreement_id`.

1. **Initiation**: A participant (employer or contributor) calls `file_dispute` on this contract if they cannot reach an agreement.
2. **Escalation Levels**: Disputes move through up to three levels (Level 1, Level 2, Level 3). Each level represents a higher degree of arbitration or authority.
3. **Appeals**: Once an arbiter (or automated system) resolves a dispute at a given level, the aggrieved party has a right to appeal the ruling (`appeal_ruling`) to the next level, provided they do so within the established time limit.

## Security Model

### Time-based Enforcement

All phases in the dispute have strict deadlines. An escalation or appeal valid only if performed within the acceptable time limit for that level (e.g., 7 days by default). Any attempt to escalate or appeal after `phase_deadline` will result in a `TimeLimitExpired` error. This guarantees finality for the participants.

### Caller Authorization

Only authenticated participants can open or escalate disputes. Furthermore, `resolve_dispute` can only be called by a pre-authorized admin/arbiter. An unauthorized participant attempting to resolve their own dispute will receive an `Unauthorized` error.

## Interacting with the Contract

- `file_dispute(caller, agreement_id)`: Opens the initial (Level 1) dispute.
- `escalate_dispute(caller, agreement_id)`: Moves an active dispute to the next higher level.
- `appeal_ruling(caller, agreement_id)`: If a dispute has been resolved, use this to reopen it at the next level for an appeal.
- `resolve_dispute(caller, agreement_id)`: Admin function to finalize the current level's ruling.
