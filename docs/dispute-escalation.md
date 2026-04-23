# Dispute Escalation Contract

Three-tier dispute ladder with configurable per-level SLA deadlines, a
keeper-triggered `PendingReview` stage, binding outcome records, and
finality rules integrated with payroll state.

---

## State Machine

```text
file_dispute → Open @ Level1

  Open          + escalate_dispute  (now ≤ deadline)          → Escalated @ Level(N+1)
  Escalated     + escalate_dispute  (now ≤ deadline)          → Escalated @ Level(N+1)

  Open          + keeper_advance_stage (now > deadline)       → PendingReview @ LevelN
  Escalated     + keeper_advance_stage (now > deadline)       → PendingReview @ LevelN
  Appealed      + keeper_advance_stage (now > deadline)       → PendingReview @ LevelN

  *active*      + expire_dispute    (now > deadline)          → Expired   [terminal]
  PendingReview + expire_dispute    (now > review_deadline)   → Expired   [terminal]

  *active*      + resolve_dispute   (admin, L1/L2)            → Resolved  (appeal window = 3 days)
  PendingReview + resolve_dispute   (admin, L1/L2)            → Resolved  (appeal window = 3 days)

  Resolved      + appeal_ruling     (now ≤ appeal_deadline)   → Appealed  @ Level(N+1)

  *active*      + resolve_dispute   (admin, L3)               → Finalised [terminal]
  PendingReview + resolve_dispute   (admin, L3)               → Finalised [terminal]
```

**Terminal states:** `Finalised`, `Expired`.  
All further transitions are rejected with `AlreadyFinalised` or `AlreadyTerminal`.

---

## SLA Timer Design

Every dispute phase is governed by a **deterministic ledger timestamp** stored
in `DisputeDetails.phase_deadline`.  All comparisons use
`env.ledger().timestamp()` — the Stellar consensus timestamp, which is
manipulation-resistant and fully deterministic across validators.

### Phase deadline lifecycle

```text
t=0   file_dispute
        phase_started_at = t
        phase_deadline   = t + level_time_limit(Level1)     [default 7 days]

      ── within window (now ≤ deadline) ──────────────────────────────────────►
        escalate_dispute / resolve_dispute operate normally

      ── deadline passes (now > deadline) ────────────────────────────────────►
        keeper_advance_stage() triggers PendingReview
          phase_started_at = now            ← records exact breach timestamp
          phase_deadline   = now + pending_review_time_limit  [default 3 days]

      ── within review window (now ≤ review_deadline) ──────────────────────►
        resolve_dispute (admin) → Resolved or Finalised

      ── review deadline passes (now > review_deadline) ───────────────────►
        expire_dispute() → Expired [terminal]
```

### Boundary semantics

| Check performed | Condition | Result |
|-----------------|-----------|--------|
| `escalate_dispute` | `now ≤ deadline` | allowed |
| `escalate_dispute` | `now > deadline` | `TimeLimitExpired` |
| `expire_dispute` | `now ≤ deadline` | `DeadlineNotPassed` |
| `expire_dispute` | `now > deadline` | allowed |
| `keeper_advance_stage` | `now ≤ deadline` | `DeadlineNotPassed` |
| `keeper_advance_stage` | `now > deadline` | allowed |
| `appeal_ruling` | `now ≤ appeal_deadline` | allowed |
| `appeal_ruling` | `now > appeal_deadline` | `TimeLimitExpired` |

> **Note:** "at exactly the deadline" (`now == deadline`) is still *within*
> the window — the allowed side of every inequality.

---

## Escalation Tiers

| Level | Default SLA | Description |
|-------|-------------|-------------|
| Level1 | 7 days (604 800 s) | Initial dispute — primary arbiter |
| Level2 | 7 days (604 800 s) | Escalated review — senior arbiter |
| Level3 | 7 days (604 800 s) | Final appeal — committee / external oracle (binding) |

Admin can override any level SLA with `set_level_time_limit`.  
Admin can set the `PendingReview` window with `set_pending_review_time_limit` (default 3 days).

---

## Contract Functions

### Lifecycle

| Function | Caller | Permissionless? | Description |
|----------|--------|-----------------|-------------|
| `initialize(owner, admin)` | owner | — | One-time setup |
| `file_dispute(caller, agreement_id)` | any | ✓ | Open a Level1 dispute; SLA clock starts |
| `escalate_dispute(caller, agreement_id)` | any | ✓ | Move to next tier within the SLA window |
| `keeper_advance_stage(caller, agreement_id)` | any | ✓ | After SLA elapsed: `Open/Escalated/Appealed → PendingReview` |
| `resolve_dispute(caller, agreement_id, outcome)` | **admin** | ✗ | Issue binding ruling; opens 3-day appeal window at L1/L2 |
| `appeal_ruling(caller, agreement_id)` | any | ✓ | Appeal a Level1/2 ruling within the appeal window |
| `expire_dispute(caller, agreement_id)` | any | ✓ | Close a stuck dispute after its current deadline |

### Configuration

| Function | Caller | Description |
|----------|--------|-------------|
| `set_level_time_limit(caller, level, seconds)` | **admin** | Override SLA for a tier (affects future phases) |
| `set_pending_review_time_limit(caller, seconds)` | **admin** | Override the `PendingReview` window (affects next keeper call) |
| `get_dispute(agreement_id)` | any | Read full `DisputeDetails` |
| `get_pending_review_time_limit()` | any | Read configured `PendingReview` window |

---

## `keeper_advance_stage` — Detailed Semantics

`keeper_advance_stage` is the permissionless function that drives automatic
SLA enforcement.  Key invariants:

1. **Stage-skip prevention** — it only ever transitions to `PendingReview`.
   It can never jump to `Resolved`, `Finalised`, or any other state.
2. **Idempotency** — a second call on an already-`PendingReview` dispute
   returns `AlreadyPendingReview` rather than silently succeeding, preventing
   duplicate event emission.
3. **Level preservation** — the dispute's `level` and `outcome` are not
   mutated; only `status`, `phase_started_at`, and `phase_deadline` change.
4. **No outcome authority** — the keeper sets no outcome; only the admin
   can write a binding ruling via `resolve_dispute`.

### Valid source states

| Status | Can keeper advance? |
|--------|---------------------|
| `Open` | ✓ (if `now > phase_deadline`) |
| `Escalated` | ✓ (if `now > phase_deadline`) |
| `Appealed` | ✓ (if `now > phase_deadline`) |
| `PendingReview` | ✗ `AlreadyPendingReview` |
| `Resolved` | ✗ `AlreadyResolved` |
| `Finalised` | ✗ `AlreadyFinalised` |
| `Expired` | ✗ `AlreadyTerminal` |

---

## `PendingReview` State

`PendingReview` signals that an SLA deadline has elapsed without a ruling
and the dispute urgently requires admin attention.

### Entering `PendingReview`

Called by any keeper (permissionless) after `phase_deadline` passes:

```
dispute.status         = PendingReview
dispute.phase_started_at = now          ← exact breach timestamp on-chain
dispute.phase_deadline   = now + pending_review_time_limit
```

### Exiting `PendingReview`

| Action | Condition | New state |
|--------|-----------|-----------|
| `resolve_dispute` (admin, L1/L2) | any time within review window | `Resolved` |
| `resolve_dispute` (admin, L3) | any time within review window | `Finalised` |
| `expire_dispute` | `now > review_deadline` | `Expired` |

### Blocked actions from `PendingReview`

| Action | Error |
|--------|-------|
| `escalate_dispute` | `InvalidTransition` — original SLA window has passed |
| `appeal_ruling` | `InvalidTransition` — dispute is not `Resolved` |
| `keeper_advance_stage` (again) | `AlreadyPendingReview` |

---

## Binding Outcomes

When `resolve_dispute` is called the `outcome` field is written to `DisputeDetails`:

| Outcome | Payroll effect |
|---------|----------------|
| `UpholdPayment` | Escrow releases funds to employer / payer |
| `GrantClaim` | Escrow releases funds to employee / claimant |
| `PartialSettlement` | Off-chain split; escrow releases per agreed ratio |
| `Unset` | *(invalid as a resolve argument — returns `InvalidTransition`)* |

Downstream contracts (payroll escrow, payment splitter) listen for
`dispute_resolved`, `dispute_finalised`, and `dispute_expired` events and
act on the `outcome` field to release or redirect funds.

---

## Finality Rules

```
Level3 resolution → status = Finalised  (terminal; no appeal possible)
Level1/2 resolution → status = Resolved (3-day appeal window opens)
  │
  └─ appeal_ruling within window → Appealed @ Level(N+1)
  └─ window passes with no appeal → de-facto binding (status stays Resolved)
```

- `Finalised` is a hard terminal state. Both `appeal_ruling` and
  `resolve_dispute` return `AlreadyFinalised`.
- `Expired` is the other terminal state — reached via `expire_dispute` after
  any phase deadline (including the `PendingReview` review window) passes with
  no admin action.

---

## Security Model

| Invariant | Enforcement |
|-----------|-------------|
| Only admin resolves | `is_admin` check at the top of `resolve_dispute` |
| Cannot double-resolve | `AlreadyResolved` / `AlreadyFinalised` on every resolve path |
| No funds stuck | `expire_dispute` (anyone) closes abandoned disputes |
| No re-entry into terminal states | `assert_not_terminal` rejects all transitions on `Finalised`/`Expired` |
| Deadlines enforced on-chain | All time comparisons use `env.ledger().timestamp()` |
| Keeper cannot skip stages | `keeper_advance_stage` only reaches `PendingReview` — never `Resolved`/`Finalised` |
| Keeper is idempotent-safe | `AlreadyPendingReview` on repeated calls; no duplicate events |
| Level ordering enforced | `next_level` helper guarantees L1→L2→L3 sequence; `MaxEscalationReached` at L3 |
| `Unset` outcome rejected | `resolve_dispute` returns `InvalidTransition` if `outcome == Unset` |

---

## Events

| Topic | Payload | When |
|-------|---------|------|
| `dispute_filed` | `DisputeFiledEvent` | New dispute opened |
| `dispute_escalated` | `DisputeEscalatedEvent` | Moved to next tier |
| `dispute_sla_breached` | `DisputeSlaBreachedEvent` | SLA elapsed; keeper advances to `PendingReview` |
| `dispute_resolved` | `DisputeResolvedEvent` | Admin ruling at Level1/2 (appeal window open) |
| `dispute_finalised` | `DisputeFinalisedEvent` | Admin ruling at Level3 (binding, no appeal) |
| `dispute_appealed` | `DisputeAppealedEvent` | Ruling appealed to next level |
| `dispute_expired` | `DisputeExpiredEvent` | Deadline passed, closed without ruling |

### `DisputeSlaBreachedEvent` fields

| Field | Type | Description |
|-------|------|-------------|
| `agreement_id` | `u128` | Identifies the dispute |
| `level` | `EscalationLevel` | Level at which the SLA was breached |
| `breached_at` | `u64` | Ledger timestamp when `keeper_advance_stage` was called |
| `review_deadline` | `u64` | Timestamp by which admin must act before `expire_dispute` is valid |

---

## `DisputeDetails` Fields

| Field | Type | Description |
|-------|------|-------------|
| `agreement_id` | `u128` | ID of the agreement under dispute |
| `initiator` | `Address` | Party who filed or most recently appealed |
| `status` | `DisputeStatus` | Current status in the state machine |
| `level` | `EscalationLevel` | Current escalation tier |
| `phase_started_at` | `u64` | Ledger timestamp when the current phase began |
| `phase_deadline` | `u64` | Ledger timestamp at which the current phase expires |
| `outcome` | `DisputeOutcome` | Binding ruling once resolved; `Unset` while open |

> `phase_started_at` doubles as the **SLA breach timestamp** when
> `status == PendingReview`: it records the exact moment the keeper advanced
> the stage.

---

## Usage Examples

### Standard fast-path resolution

```rust
// 1. Initialize
client.initialize(&owner, &admin);

// 2. Employee files dispute — SLA clock starts immediately
client.file_dispute(&employee, &agreement_id);

// 3. Admin resolves at Level1 — 3-day appeal window opens
client.resolve_dispute(&admin, &agreement_id, &DisputeOutcome::UpholdPayment);

// 4. Appeal window passes with no action → de-facto binding
//    (no further calls required; downstream reads DisputeDetails.outcome)
```

### Full escalation to Level3

```rust
// 1. File
client.file_dispute(&employee, &agreement_id);

// 2. Escalate to Level2 (within SLA window)
client.escalate_dispute(&employee, &agreement_id);

// 3. Admin resolves at Level2 — appeal window opens
client.resolve_dispute(&admin, &agreement_id, &DisputeOutcome::UpholdPayment);

// 4. Employee appeals to Level3
client.appeal_ruling(&employee, &agreement_id);

// 5. Admin issues final binding ruling at Level3 → Finalised
client.resolve_dispute(&admin, &agreement_id, &DisputeOutcome::GrantClaim);
// status = Finalised, outcome = GrantClaim, no further appeal possible
```

### Keeper-driven SLA enforcement

```rust
// 1. File dispute
client.file_dispute(&employee, &agreement_id);
// phase_deadline = now + 604_800 (7 days)

// ...7 days pass, admin has not acted...

// 2. Any keeper (bot, cron job, anyone) advances the stage
client.keeper_advance_stage(&keeper_bot, &agreement_id);
// status = PendingReview
// phase_deadline = now + 259_200 (3-day review window)
// emits: DisputeSlaBreachedEvent { breached_at, review_deadline }

// 3a. Admin acts within the review window
client.resolve_dispute(&admin, &agreement_id, &DisputeOutcome::GrantClaim);

// — OR —

// 3b. Admin fails to act; anyone expires the dispute after review_deadline
client.expire_dispute(&anyone, &agreement_id);
// status = Expired → downstream escrow releases funds to payer
```

### Custom SLA configuration

```rust
// Shorten Level1 SLA to 1 hour for testing
client.set_level_time_limit(&admin, &EscalationLevel::Level1, &3600u64);

// Set a 6-hour pending-review window
client.set_pending_review_time_limit(&admin, &21_600u64);

client.file_dispute(&user, &agreement_id);
// phase_deadline = now + 3600

// After 1 hour + 1 second:
client.keeper_advance_stage(&keeper, &agreement_id);
// phase_deadline = now + 21_600
```

---

## Error Codes

| Code | Name | Meaning |
|------|------|---------|
| 1 | `Unauthorized` | Caller is not the admin |
| 2 | `DisputeNotFound` | No dispute exists for this agreement |
| 3 | `AlreadyResolved` | Cannot resolve / expire / advance an already-resolved dispute |
| 4 | `MaxEscalationReached` | Already at Level3; cannot escalate further |
| 5 | `TimeLimitExpired` | The SLA or appeal window for this action has passed |
| 6 | `InvalidTransition` | Illegal state transition (e.g. escalate from `PendingReview`, appeal non-resolved, resolve with `Unset` outcome) |
| 7 | `NotParty` | Reserved for party-restricted operations |
| 8 | `AlreadyFinalised` | Level3 ruling is binding; no further transitions allowed |
| 9 | `DeadlineNotPassed` | Cannot expire or advance a dispute before its current deadline |
| 10 | `AlreadyTerminal` | Dispute is already in `Expired` state |
| 11 | `AlreadyPendingReview` | `keeper_advance_stage` already called; repeated call rejected |

---

## Storage Keys

| Key | Type | Description |
|-----|------|-------------|
| `Owner` | `Address` | Upgrade authority |
| `Admin` | `Address` | Dispute resolution authority |
| `Dispute(u128)` | `DisputeDetails` | Per-dispute state keyed by `agreement_id` |
| `LevelTimeLimit(EscalationLevel)` | `u64` | SLA window in seconds per tier |
| `PendingReviewTimeLimit` | `u64` | Review window in seconds after SLA breach |
```

Now let me update the state-machines doc and write the implementation back to lib.rs: