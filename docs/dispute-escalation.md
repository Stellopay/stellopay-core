# Dispute Escalation Contract

Three-tier dispute ladder with configurable per-level SLA deadlines, a
keeper-triggered `PendingReview` stage, binding outcome records, and
finality rules integrated with payroll state.

> **⚠ Breaking change (vNEXT):** The `DisputeSlaBreachedEvent` and
> `DisputeSlaViolationAdvancedEvent` events now include a `keeper` field
> recording the address of the keeper who triggered the advance.  Indexers
> that deserialize these events will need to update their schemas to include
> the new field.  `DisputeDetails` now includes a `keeper_advances` field;
> existing disputes migrated from prior contract versions will require a
> migration path (resolve/expire before upgrade, or implement a storage
> migration).

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

## Duplicate-Filing Guard

Only one **active** (non-terminal) dispute may exist per `agreement_id` at any
time. If a caller invokes `file_dispute` while an existing dispute for the same
`agreement_id` is in any non-terminal state, the call is rejected immediately
with `DisputeDuplicateFiling` (error code 14).

### Why this matters

Allowing a second filing while the first is still in-flight would produce **two
independent SLA timers** for the same underlying claim. Downstream payroll and
escrow contracts listen for `dispute_resolved` / `dispute_finalised` /
`dispute_expired` events to decide how to release funds; two competing records
would leave them in an undefined state.

### Non-terminal states that block re-filing

| Status | Blocked? |
|--------|----------|
| `Open` | ✓ `DisputeDuplicateFiling` |
| `Escalated` | ✓ `DisputeDuplicateFiling` |
| `Appealed` | ✓ `DisputeDuplicateFiling` |
| `PendingReview` | ✓ `DisputeDuplicateFiling` |
| `Resolved` | ✓ `DisputeDuplicateFiling` (appeal window still open) |

### Terminal states that allow re-filing

| Status | Allowed? |
|--------|----------|
| `Finalised` | ✓ Re-filing permitted |
| `Expired` | ✓ Re-filing permitted |

When re-filing is permitted the new dispute **overwrites** the terminal record
in storage (same `StorageKey::Dispute(agreement_id)`) and starts a completely
fresh Level1 SLA window from the current ledger timestamp.

### Example: re-file after Finalised

```rust
// Prior dispute fully adjudicated at Level3
client.resolve_dispute(&admin, &agreement_id, &DisputeOutcome::GrantClaim);
// → status = Finalised

// New claim on the same agreement — re-filing now allowed
client.file_dispute(&employee, &agreement_id, &DisputeReason::QualityIssue);
// → status = Open @ Level1, fresh SLA clock
```

### Example: re-file after Expired

```rust
// Prior dispute never resolved — expired after deadline
client.expire_dispute(&anyone, &agreement_id);
// → status = Expired

// Re-file for the same agreement
client.file_dispute(&employee, &agreement_id, &DisputeReason::NonDelivery);
// → status = Open @ Level1, fresh SLA clock
```

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
| `get_level_time_limit(level)` | any | Read the configured SLA window for a given escalation level |
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
| No duplicate active disputes | `file_dispute` returns `DisputeDuplicateFiling` when a non-terminal dispute already exists for the same `agreement_id`; re-filing allowed after `Finalised` or `Expired` |
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
| `dispute_sla_breached` | `DisputeSlaBreachedEvent` | SLA elapsed; keeper advances to `PendingReview` (legacy topic, kept for backward compatibility) |
| `sla_violation_advanced` | `DisputeSlaViolationAdvancedEvent` | Same moment as `dispute_sla_breached`; forward-looking consumers should prefer this topic |
| `dispute_resolved` | `DisputeResolvedEvent` | Admin ruling at Level1/2 (appeal window open) |
| `dispute_finalised` | `DisputeFinalisedEvent` | Admin ruling at Level3 (binding, no appeal) |
| `dispute_appealed` | `DisputeAppealedEvent` | Ruling appealed to next level |
| `dispute_expired` | `DisputeExpiredEvent` | Deadline passed, closed without ruling |

### `DisputeSlaBreachedEvent` fields

| Field | Type | Description |
|-------|------|-------------|
| `agreement_id` | `u128` | Identifies the dispute |
| `level` | `EscalationLevel` | Level at which the SLA was breached |
| `keeper` | `Address` | Address of the keeper who triggered the advance |
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
| `reason` | `DisputeReason` | Why the dispute was raised; immutable after filing |
| `keeper_advances` | `Vec<KeeperAdvance>` | Ordered history of every `keeper_advance_stage` call — records which keeper triggered each automatic advance, the timestamp, and the escalation level at the time |

> `phase_started_at` doubles as the **SLA breach timestamp** when
> `status == PendingReview`: it records the exact moment the keeper advanced
> the stage.

### Keeper Accountability

Each `KeeperAdvance` entry stored in `DisputeDetails.keeper_advances` provides
a full accountability trail of who triggered each automatic SLA-driven stage
advance and when:

| Field | Type | Description |
|-------|------|-------------|
| `keeper` | `Address` | Address of the keeper who called `keeper_advance_stage` |
| `advanced_at` | `u64` | Ledger timestamp at which the advance was triggered |
| `level` | `EscalationLevel` | Escalation level at the time of the advance |

The `keeper` field is also included in both `DisputeSlaBreachedEvent` and
`DisputeSlaViolationAdvancedEvent` so that off-chain indexers and monitoring
systems can track which keeper triggered each advance without reading the
full dispute record.

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
client.file_dispute(&employee, &agreement_id, &DisputeReason::PaymentDispute);

// 3. Admin resolves at Level1 — 3-day appeal window opens
client.resolve_dispute(&admin, &agreement_id, &DisputeOutcome::UpholdPayment);

// 4. Appeal window passes with no action → de-facto binding
//    (no further calls required; downstream reads DisputeDetails.outcome)
```

### Full escalation to Level3

```rust
// 1. File
client.file_dispute(&employee, &agreement_id, &DisputeReason::PaymentDispute);

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
client.file_dispute(&employee, &agreement_id, &DisputeReason::PaymentDispute);
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

client.file_dispute(&user, &agreement_id, &DisputeReason::PaymentDispute);
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
| 12 | `SlaDeadlineOverflow` | SLA deadline computation overflowed; keeper cannot proceed |
| 13 | `ReasonTooLong` | `Other` text exceeds 256 bytes |
| 14 | `DisputeDuplicateFiling` | A non-terminal dispute already exists for this `agreement_id`; re-filing allowed after `Finalised` or `Expired` |

---

## Audit Logger Integration

Every dispute lifecycle transition can optionally record a compliance entry in
the shared `audit_logger` contract. Once wired via `set_audit_logger(admin, addr)`:

| Transition | Audit action | Actor |
|---|---|---|
| `file_dispute` | `dispute_filed` | Caller |
| `escalate_dispute` | `dispute_escalated` | Caller |
| `keeper_advance_stage` | `dispute_sla_breached` | Keeper |
| `resolve_dispute` (L1/L2) | `dispute_resolved` | Admin |
| `resolve_dispute` (L3) | `dispute_finalised` | Admin |
| `appeal_ruling` | `dispute_appealed` | Appellant |
| `expire_dispute` | `dispute_expired` | Caller |

Unauthorized callers cannot create audit entries — failed transitions return an
error without recording. When no audit logger is configured, the contract
operates normally without external audit.

Configuration is admin-gated via `set_audit_logger`; the current logger address
is visible via `get_audit_logger()`.

---

## Audit Logger Integration

Every dispute lifecycle transition can optionally record a compliance entry in
the shared `audit_logger` contract. Once wired via `set_audit_logger(admin, addr)`:

| Transition | Audit action | Actor |
|---|---|---|
| `file_dispute` | `dispute_filed` | Caller |
| `escalate_dispute` | `dispute_escalated` | Caller |
| `keeper_advance_stage` | `dispute_sla_breached` | Keeper |
| `resolve_dispute` (L1/L2) | `dispute_resolved` | Admin |
| `resolve_dispute` (L3) | `dispute_finalised` | Admin |
| `appeal_ruling` | `dispute_appealed` | Appellant |
| `expire_dispute` | `dispute_expired` | Caller |

Unauthorized callers cannot create audit entries — failed transitions return an
error without recording. When no audit logger is configured, the contract
operates normally without external audit.

Configuration is admin-gated via `set_audit_logger`; the current logger address
is visible via `get_audit_logger()`.

## Storage Keys

| Key | Type | Description |
|-----|------|-------------|
| `Owner` | `Address` | Upgrade authority |
| `Admin` | `Address` | Dispute resolution authority |
| `Dispute(u128)` | `DisputeDetails` | Per-dispute state keyed by `agreement_id` |
| `LevelTimeLimit(EscalationLevel)` | `u64` | SLA window in seconds per tier |
| `PendingReviewTimeLimit` | `u64` | Review window in seconds after SLA breach |
| `AuditLogger` | `Address` | Optional audit logger contract for compliance recording |
```

---

## Early-Expiry Rejection

`expire_dispute` is guarded by the strict check `now > phase_deadline`.
Any call made while `now <= phase_deadline` is rejected with
`DeadlineNotPassed`, regardless of the dispute's current status.

### Why this matters

Because `expire_dispute` is **permissionless**, without the deadline guard an
adversary could expire a dispute immediately after filing it — bypassing
resolution entirely and preventing either party from obtaining a ruling.  The
guard ensures the SLA window remains inviolable from outside the normal
resolution flow.

### Boundary semantics for `expire_dispute`

| Timestamp | Status | Result |
|-----------|--------|--------|
| `now < phase_deadline` | `Open / Escalated / Appealed` | `DeadlineNotPassed` |
| `now == phase_deadline` | `Open / Escalated / Appealed` | `DeadlineNotPassed` (equal is still inside the window) |
| `now > phase_deadline` | `Open / Escalated / Appealed` | succeeds → `Expired` |
| `now < review_deadline` | `PendingReview` | `DeadlineNotPassed` |
| `now == review_deadline` | `PendingReview` | `DeadlineNotPassed` |
| `now > review_deadline` | `PendingReview` | succeeds → `Expired` |

### Test coverage (§16)

The test suite in `onchain/contracts/dispute_escalation/tests/test_escalation.rs`
covers these scenarios exhaustively in **§16 EARLY-EXPIRY REJECTION TESTS**:

| Test | What it proves |
|------|---------------|
| `test_expire_open_dispute_premature_rejected` | Calling `expire_dispute` on an `Open` dispute with no time elapsed is rejected |
| `test_expire_open_dispute_at_exact_deadline_rejected` | `now == deadline` is still inside the window; expiry rejected |
| `test_expire_open_dispute_one_second_past_deadline_succeeds` | `now == deadline + 1` is accepted; dispute becomes `Expired` |
| `test_expire_escalated_dispute_premature_rejected` | Same trio for `Escalated` status (Level2 SLA) |
| `test_expire_escalated_dispute_at_exact_deadline_rejected` | |
| `test_expire_escalated_dispute_one_second_past_deadline_succeeds` | |
| `test_expire_appealed_dispute_premature_rejected` | Same trio for `Appealed` status (after `appeal_ruling`) |
| `test_expire_appealed_dispute_at_exact_deadline_rejected` | |
| `test_expire_appealed_dispute_one_second_past_deadline_succeeds` | |
| `test_expire_pending_review_dispute_premature_rejected` | Immediately after `keeper_advance_stage` — inside review window |
| `test_expire_pending_review_dispute_at_exact_review_deadline_rejected` | `now == review_deadline` is inside the window |
| `test_expire_pending_review_dispute_one_second_past_review_deadline_succeeds` | `now == review_deadline + 1` succeeds |
| `test_expire_premature_leaves_dispute_fully_unchanged` | Failed call makes no state mutation |
| `test_expire_premature_emits_no_event` | Failed call emits no `dispute_expired` event |
| `test_expire_premature_by_third_party_rejected_with_deadline_error` | Permissionless callers are still subject to the deadline guard |
| `test_expire_premature_rejected_across_all_escalation_levels` | Boundary holds at Level1, Level2, and Level3 in a single sweep |

## Payroll Contract: Dispute Re-filing Guard

The `payroll.rs` `raise_dispute` function includes a guard that prevents raising a
dispute when the agreement already has an **active** dispute
(`dispute_status == DisputeStatus::Raised`).  This prevents:

- Resetting downstream dispute timers on an already-Disputed agreement.
- Producing a confusing duplicate record in the dispute escalation contract.

Once the active dispute is **resolved** via `resolve_dispute` (or
`resolve_dispute_multisig`), `dispute_status` transitions to `Resolved` and the
guard permits a fresh dispute to be raised — the `== DisputeStatus::Raised` check
is more permissive than the original `!= DisputeStatus::None` check.

### Lifecycle

```text
                    raise_dispute              resolve_dispute
None / Resolved ──────────────────► Raised ──────────────────────► Resolved
     │                                                                │
     └───────── raise_dispute succeeds ───────────────────────────────┘
                     (re-filing allowed)

Raised ────── raise_dispute ──────► DisputeAlreadyRaised (rejected)
```

### Tests

| Test | Scenario |
|------|----------|
| `test_raise_dispute_rejects_duplicate_on_active_dispute` | Second `raise_dispute` on an agreement with `dispute_status == Raised` is rejected with `DisputeAlreadyRaised` |
| `test_raise_dispute_succeeds_after_resolution` | After `resolve_dispute` sets `dispute_status` to `Resolved`, a fresh `raise_dispute` succeeds (within the same grace window) |