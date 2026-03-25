# Dispute Escalation Contract

Three-tier dispute ladder with configurable deadlines, binding outcomes, and finality rules integrated with payroll state.

## State Machine

```
file_dispute  →  Open @ Level1
  Open        + escalate_dispute  (within deadline)  →  Escalated @ Level2
  Escalated   + escalate_dispute  (within deadline)  →  Escalated @ Level3
  *active*    + expire_dispute    (deadline passed)  →  Expired   [terminal]
  *active*    + resolve_dispute   (admin, L1/L2)     →  Resolved  (appeal window = 3 days)
  Resolved    + appeal_ruling     (within window)    →  Appealed  @ next level
  *active*    + resolve_dispute   (admin, L3)        →  Finalised [terminal — binding]
```

**Terminal states:** `Finalised`, `Expired`. All further transitions are rejected.

## Escalation Tiers

| Level | Default deadline | Description |
|-------|-----------------|-------------|
| Level1 | 7 days | Initial dispute — primary arbiter |
| Level2 | 7 days | Escalated review — senior arbiter |
| Level3 | 7 days | Final appeal — committee / oracle |

Admin can override any deadline with `set_level_time_limit`.

## Contract Functions

### Lifecycle

| Function | Caller | Description |
|----------|--------|-------------|
| `initialize(owner, admin)` | owner | One-time setup |
| `file_dispute(caller, agreement_id)` | any | Open a Level1 dispute |
| `escalate_dispute(caller, agreement_id)` | any | Move to next tier (within deadline) |
| `resolve_dispute(caller, agreement_id, outcome)` | admin | Issue binding ruling |
| `appeal_ruling(caller, agreement_id)` | any | Appeal a Level1/2 ruling (within appeal window) |
| `expire_dispute(caller, agreement_id)` | any | Close a stuck dispute after deadline |

### Configuration

| Function | Caller | Description |
|----------|--------|-------------|
| `set_level_time_limit(caller, level, seconds)` | admin | Override deadline for a tier |
| `get_dispute(agreement_id)` | any | Read dispute details |

## Binding Outcomes

When `resolve_dispute` is called the `outcome` field is written to `DisputeDetails`:

| Outcome | Payroll effect |
|---------|---------------|
| `UpholdPayment` | Escrow releases funds to employer / payer |
| `GrantClaim` | Escrow releases funds to employee / claimant |
| `PartialSettlement` | Off-chain split; escrow releases per agreed ratio |

Downstream contracts (payroll escrow, payment splitter) listen for `dispute_resolved` and `dispute_finalised` events and act on `outcome`.

## Finality Rules (NatSpec)

```
Level3 resolution → status = Finalised
```

* `Finalised` is a terminal state. Both `appeal_ruling` and `resolve_dispute` return `AlreadyFinalised`.
* Level1/Level2 resolutions open a **3-day appeal window**. If no appeal is filed, the `Resolved` state becomes de-facto binding.
* `Expired` is the other terminal state — reached via `expire_dispute` after a deadline passes with no action.

## Security Model

| Invariant | Enforcement |
|-----------|-------------|
| Only admin resolves | `is_admin` check before any `resolve_dispute` |
| Cannot double-resolve | `AlreadyResolved` / `AlreadyFinalised` on every resolve path |
| No funds stuck | `expire_dispute` (callable by anyone) closes abandoned disputes |
| No re-entry into terminal states | `assert_not_terminal` helper rejects all transitions on `Finalised` / `Expired` |
| Deadlines enforced on-chain | All time comparisons use `env.ledger().timestamp()` |

## Events

| Topic | Payload | When |
|-------|---------|------|
| `dispute_filed` | `DisputeFiledEvent` | New dispute opened |
| `dispute_escalated` | `DisputeEscalatedEvent` | Moved to next tier |
| `dispute_resolved` | `DisputeResolvedEvent` | Admin ruling at Level1/2 |
| `dispute_finalised` | `DisputeFinalisedEvent` | Admin ruling at Level3 (binding) |
| `dispute_appealed` | `DisputeAppealedEvent` | Ruling appealed |
| `dispute_expired` | `DisputeExpiredEvent` | Deadline passed, closed without ruling |

## Usage Example

```rust
// 1. Initialize
client.initialize(&owner, &admin);

// 2. Shorten Level1 window for testing
client.set_level_time_limit(&admin, &EscalationLevel::Level1, &3600u64);

// 3. Employee files dispute
client.file_dispute(&employee, &agreement_id);

// 4. Escalate to Level2
client.escalate_dispute(&employee, &agreement_id);

// 5. Admin resolves at Level2 — appeal window opens
client.resolve_dispute(&admin, &agreement_id, &DisputeOutcome::UpholdPayment);

// 6. Employee appeals to Level3
client.appeal_ruling(&employee, &agreement_id);

// 7. Admin issues final binding ruling at Level3
client.resolve_dispute(&admin, &agreement_id, &DisputeOutcome::GrantClaim);
// → status = Finalised, outcome = GrantClaim, no further appeal possible

// 8. Abandoned dispute — anyone can expire after deadline
client.expire_dispute(&anyone, &stale_agreement_id);
```

## Error Codes

| Code | Name | Meaning |
|------|------|---------|
| 1 | `Unauthorized` | Caller is not the admin |
| 2 | `DisputeNotFound` | No dispute exists for this agreement |
| 3 | `AlreadyResolved` | Cannot resolve an already-resolved dispute |
| 4 | `MaxEscalationReached` | Already at Level3 |
| 5 | `TimeLimitExpired` | Window for this action has passed |
| 6 | `InvalidTransition` | Illegal state transition (e.g. appeal non-resolved) |
| 7 | `NotParty` | Reserved for party-restricted operations |
| 8 | `AlreadyFinalised` | Level3 ruling is binding; no further action |
| 9 | `DeadlineNotPassed` | Cannot expire a dispute before its deadline |
| 10 | `AlreadyTerminal` | Dispute is already in `Expired` state |
