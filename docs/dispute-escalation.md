Dispute Escalation Contract
Three-tier dispute ladder with configurable per-level SLA deadlines, a
keeper-triggered PendingReview stage, binding outcome records, and
finality rules integrated with payroll state.

State Machine
text

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
Terminal states: Finalised, Expired.
All further transitions are rejected with AlreadyFinalised or AlreadyTerminal.

SLA Timer Design
Every dispute phase is governed by a deterministic ledger timestamp stored
in DisputeDetails.phase_deadline. All comparisons use
env.ledger().timestamp() — the Stellar consensus timestamp, which is
manipulation-resistant and fully deterministic across validators.

Phase deadline lifecycle
text

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
Boundary semantics
Check performed	Condition	Result
escalate_dispute	now ≤ deadline	allowed
escalate_dispute	now > deadline	TimeLimitExpired
expire_dispute	now ≤ deadline	DeadlineNotPassed
expire_dispute	now > deadline	allowed
keeper_advance_stage	now ≤ deadline	DeadlineNotPassed
keeper_advance_stage	now > deadline	allowed
appeal_ruling	now ≤ appeal_deadline	allowed
appeal_ruling	now > appeal_deadline	TimeLimitExpired
Note: "at exactly the deadline" (now == deadline) is still within
the window — the allowed side of every inequality.

Escalation Tiers
Level	Default SLA	Description
Level1	7 days (604 800 s)	Initial dispute — primary arbiter
Level2	7 days (604 800 s)	Escalated review — senior arbiter
Level3	7 days (604 800 s)	Final appeal — committee / external oracle (binding)
Admin can override any level SLA with set_level_time_limit.
Admin can set the PendingReview window with set_pending_review_time_limit (default 3 days).

Contract Functions
Lifecycle
Function	Caller	Permissionless?	Description
initialize(owner, admin)	owner	—	One-time setup
file_dispute(caller, agreement_id)	any	✓	Open a Level1 dispute; SLA clock starts
escalate_dispute(caller, agreement_id)	any	✓	Move to next tier within the SLA window
keeper_advance_stage(caller, agreement_id)	any	✓	After SLA elapsed: Open/Escalated/Appealed → PendingReview
resolve_dispute(caller, agreement_id, outcome)	admin	✗	Issue binding ruling; opens 3-day appeal window at L1/L2
appeal_ruling(caller, agreement_id)	any	✓	Appeal a Level1/2 ruling within the appeal window
expire_dispute(caller, agreement_id)	any	✓	Close a stuck dispute after its current deadline
Configuration
Function	Caller	Description
set_level_time_limit(caller, level, seconds)	admin	Override SLA for a tier (affects future phases)
set_pending_review_time_limit(caller, seconds)	admin	Override the PendingReview window (affects next keeper call)
get_dispute(agreement_id)	any	Read full DisputeDetails
get_pending_review_time_limit()	any	Read configured PendingReview window
Sequential Escalation Ordering
The contract strictly enforces a Level1 → Level2 → Level3 walk. There is
no legitimate way to bypass an intermediate tier, and the public API offers no
surface that would let a caller jump two levels in a single transaction.

Why ordering matters
Risk	Mitigation
A party could escalate straight to Level3 and skip the Level2 SLA window, depriving senior arbiters of their review window.	escalate_dispute is a one-tier step function — see next_level.
A future maintainer could accidentally introduce a target_level parameter that allows skipping.	The contract has no target_level parameter; only agreement_id is accepted.
An admin could mutate Dispute storage directly to land at Level3.	The admin role only gates resolve_dispute, set_level_time_limit, and set_pending_review_time_limit; DisputeDetails storage is written solely from the state-machine entry points.
A higher-tier LA would be reached without paying the lower-tier SLA tax.	The Level2 SLA must elapse (or be observed as elapsed) before Level3 is reachable.
How the guard works
The contract derives the new tier purely from the current tier via a single
closed mapping — the next_level helper on DisputeEscalationContract:

text

Level1 ──next_level()──► Level2 ──next_level()──► Level3 ──next_level()──► Err(MaxEscalationReached)
escalate_dispute reads dispute.level, asks next_level for the next
single tier, and writes that tier back. There is no parameter accepting a
target level, so any caller — admin, keeper, or external party — is bound by
this mapping. The only way to reach Level3 is:

file_dispute → Open @ Level1
escalate_dispute → Escalated @ Level2 (must respect Level1 SLA)
escalate_dispute → Escalated @ Level3 (must respect Level2 SLA)
…or, equivalently for the appeal path:

file_dispute → Open @ Level1
resolve_dispute → Resolved @ Level1
appeal_ruling → Appealed @ Level2
resolve_dispute → Resolved @ Level2
appeal_ruling → Appealed @ Level3
Negative-space guarantees
The contract actively rejects any path that would skip a level:

Attempt	Result
One escalate_dispute call from Level1	Lands at Level2 (NOT Level3).
One escalate_dispute call from Level2	Lands at Level3.
One escalate_dispute call from Level3	Returns MaxEscalationReached.
escalate_dispute from PendingReview	Returns InvalidTransition (SLA window already declared breached).
Skip via a future target_level parameter	Impossible — no such parameter exists in the API surface.
These guarantees are locked in by regression tests in §13 and the new tests
added under issue #890 in
onchain/contracts/dispute_escalation/tests/test_escalation.rs. The
relevant new tests are:

the negative-space test that proves a single escalate_dispute call from
Level1 lands at Level2 (asserting both the positive post-condition and
the ! Level3 invariant);
the positive-path test that walks Level1 → Level2 → Level3 and asserts
a fourth escalate_dispute returns MaxEscalationReached.
Security review checklist
When modifying any code path that touches dispute.level, confirm:

 The only source of a new level value remains next_level(...).
 No public function accepts a caller-supplied EscalationLevel.
 assert_not_terminal still runs before next_level is consulted.
 No test reaches Level3 in fewer than two escalate_dispute calls
(or fewer than the appeal equivalent).
keeper_advance_stage — Detailed Semantics
keeper_advance_stage is the permissionless function that drives automatic
SLA enforcement. Key invariants:

Stage-skip prevention — it only ever transitions to PendingReview.
It can never jump to Resolved, Finalised, or any other state.
Idempotency — a second call on an already-PendingReview dispute
returns AlreadyPendingReview rather than silently succeeding, preventing
duplicate event emission.
Level preservation — the dispute's level and outcome are not
mutated; only status, phase_started_at, and phase_deadline change.
No outcome authority — the keeper sets no outcome; only the admin
can write a binding ruling via resolve_dispute.
Dual event emission — every successful call emits both:
dispute_sla_breached (DisputeSlaBreachedEvent) for backward
compatibility, and
sla_violation_advanced (SlaViolationAdvancedEvent) as the primary
SLA-violation signal for off-chain monitoring systems.
Valid source states
Status	Can keeper advance?
Open	✓ (if now > phase_deadline)
Escalated	✓ (if now > phase_deadline)
Appealed	✓ (if now > phase_deadline)
PendingReview	✗ AlreadyPendingReview
Resolved	✗ AlreadyResolved
Finalised	✗ AlreadyFinalised
Expired	✗ AlreadyTerminal
PendingReview State
PendingReview signals that an SLA deadline has elapsed without a ruling
and the dispute urgently requires admin attention.

Entering PendingReview
Called by any keeper (permissionless) after phase_deadline passes:

text

dispute.status         = PendingReview
dispute.phase_started_at = now          ← exact breach timestamp on-chain
dispute.phase_deadline   = now + pending_review_time_limit
Exiting PendingReview
Action	Condition	New state
resolve_dispute (admin, L1/L2)	any time within review window	Resolved
resolve_dispute (admin, L3)	any time within review window	Finalised
expire_dispute	now > review_deadline	Expired
Blocked actions from PendingReview
Action	Error
escalate_dispute	InvalidTransition — original SLA window has passed
appeal_ruling	InvalidTransition — dispute is not Resolved
keeper_advance_stage (again)	AlreadyPendingReview
Binding Outcomes
When resolve_dispute is called the outcome field is written to DisputeDetails:

Outcome	Payroll effect
UpholdPayment	Escrow releases funds to employer / payer
GrantClaim	Escrow releases funds to employee / claimant
PartialSettlement	Off-chain split; escrow releases per agreed ratio
Unset	(invalid as a resolve argument — returns InvalidTransition)
Downstream contracts (payroll escrow, payment splitter) listen for
dispute_resolved, dispute_finalised, and dispute_expired events and
act on the outcome field to release or redirect funds.

Finality Rules
text

Level3 resolution → status = Finalised  (terminal; no appeal possible)
Level1/2 resolution → status = Resolved (3-day appeal window opens)
  │
  └─ appeal_ruling within window → Appealed @ Level(N+1)
  └─ window passes with no appeal → de-facto binding (status stays Resolved)
Finalised is a hard terminal state. Both appeal_ruling and
resolve_dispute return AlreadyFinalised.
Expired is the other terminal state — reached via expire_dispute after
any phase deadline (including the PendingReview review window) passes with
no admin action.
Security Model
Invariant	Enforcement
Only admin resolves	is_admin check at the top of resolve_dispute
Cannot double-resolve	AlreadyResolved / AlreadyFinalised on every resolve path
No funds stuck	expire_dispute (anyone) closes abandoned disputes
No re-entry into terminal states	assert_not_terminal rejects all transitions on Finalised/Expired
Deadlines enforced on-chain	All time comparisons use env.ledger().timestamp()
Keeper cannot skip stages	keeper_advance_stage only reaches PendingReview — never Resolved/Finalised
Keeper is idempotent-safe	AlreadyPendingReview on repeated calls; no duplicate events
Level ordering enforced	next_level helper guarantees L1→L2→L3 sequence; MaxEscalationReached at L3
Unset outcome rejected	resolve_dispute returns InvalidTransition if outcome == Unset
Events
Topic	Payload	When
dispute_filed	DisputeFiledEvent	New dispute opened
dispute_escalated	DisputeEscalatedEvent	Moved to next tier (normal-flow)
dispute_sla_breached	DisputeSlaBreachedEvent	SLA elapsed; keeper advances to PendingReview
sla_violation_advanced	SlaViolationAdvancedEvent	SLA elapsed; emitted only by keeper_advance_stage — the primary signal for off-chain SLA-compliance monitoring
dispute_resolved	DisputeResolvedEvent	Admin ruling at Level1/2 (appeal window open)
dispute_finalised	DisputeFinalisedEvent	Admin ruling at Level3 (binding, no appeal)
dispute_appealed	DisputeAppealedEvent	Ruling appealed to next level
dispute_expired	DisputeExpiredEvent	Deadline passed, closed without ruling
Distinguishing SLA violations from normal-flow escalations
keeper_advance_stage emits two events when it fires due to an SLA timeout:

dispute_sla_breached (DisputeSlaBreachedEvent) — backward-compatible event for existing off-chain systems.
sla_violation_advanced (SlaViolationAdvancedEvent) — a distinct event emitted only on SLA timeout, not on normal-flow escalation.
Off-chain SLA-compliance monitors should listen solely for sla_violation_advanced to unambiguously identify every SLA-violation trigger without false positives from dispute_escalated events (which are emitted by escalate_dispute during normal flow).

DisputeSlaBreachedEvent fields
Field	Type	Description
agreement_id	u128	Identifies the dispute
level	EscalationLevel	Level at which the SLA was breached
breached_at	u64	Ledger timestamp when keeper_advance_stage was called
review_deadline	u64	Timestamp by which admin must act before expire_dispute is valid
SlaViolationAdvancedEvent fields
Field	Type	Description
agreement_id	u128	Identifies the dispute whose SLA was violated
level	EscalationLevel	Escalation level at which the SLA was breached
breached_at	u64	Ledger timestamp when the violation was observed and the stage was advanced
review_deadline	u64	Timestamp by which admin must act before expire_dispute is valid
previous_status	DisputeStatus	Dispute status before the keeper advanced the stage (Open, Escalated, or Appealed). Enables monitoring systems to distinguish the source state
DisputeDetails Fields
Field	Type	Description
agreement_id	u128	ID of the agreement under dispute
initiator	Address	Party who filed or most recently appealed
status	DisputeStatus	Current status in the state machine
level	EscalationLevel	Current escalation tier
phase_started_at	u64	Ledger timestamp when the current phase began
phase_deadline	u64	Ledger timestamp at which the current phase expires
outcome	DisputeOutcome	Binding ruling once resolved; Unset while open
phase_started_at doubles as the SLA breach timestamp when
status == PendingReview: it records the exact moment the keeper advanced
the stage.

Usage Examples
Standard fast-path resolution
Rust

// 1. Initialize
client.initialize(&owner, &admin);

// 2. Employee files dispute — SLA clock starts immediately
client.file_dispute(&employee, &agreement_id);

// 3. Admin resolves at Level1 — 3-day appeal window opens
client.resolve_dispute(&admin, &agreement_id, &DisputeOutcome::UpholdPayment);

// 4. Appeal window passes with no action → de-facto binding
//    (no further calls required; downstream reads DisputeDetails.outcome)
Full escalation to Level3
Rust

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
Keeper-driven SLA enforcement
Rust

// 1. File dispute
client.file_dispute(&employee, &agreement_id);
// phase_deadline = now + 604_800 (7 days)

// ...7 days pass, admin has not acted...

// 2. Any keeper (bot, cron job, anyone) advances the stage
client.keeper_advance_stage(&keeper_bot, &agreement_id);
// status = PendingReview
// phase_deadline = now + 259_200 (3-day review window)
// emits: DisputeSlaBreachedEvent { breached_at, review_deadline }
// emits: SlaViolationAdvancedEvent { level, breached_at, review_deadline, previous_status }

// Off-chain SLA monitors should listen for `sla_violation_advanced` to
// unambiguously identify SLA violations without false positives from
// normal-flow `dispute_escalated` events.

// 3a. Admin acts within the review window
client.resolve_dispute(&admin, &agreement_id, &DisputeOutcome::GrantClaim);

// — OR —

// 3b. Admin fails to act; anyone expires the dispute after review_deadline
client.expire_dispute(&anyone, &agreement_id);
// status = Expired → downstream escrow releases funds to payer
Custom SLA configuration
Rust

// Shorten Level1 SLA to 1 hour for testing
client.set_level_time_limit(&admin, &EscalationLevel::Level1, &3600u64);

// Set a 6-hour pending-review window
client.set_pending_review_time_limit(&admin, &21_600u64);

client.file_dispute(&user, &agreement_id);
// phase_deadline = now + 3600

// After 1 hour + 1 second:
client.keeper_advance_stage(&keeper, &agreement_id);
// phase_deadline = now + 21_600
Error Codes
Code	Name	Meaning
1	Unauthorized	Caller is not the admin
2	DisputeNotFound	No dispute exists for this agreement
3	AlreadyResolved	Cannot resolve / expire / advance an already-resolved dispute
4	MaxEscalationReached	Already at Level3; cannot escalate further
5	TimeLimitExpired	The SLA or appeal window for this action has passed
6	InvalidTransition	Illegal state transition (e.g. escalate from PendingReview, appeal non-resolved, resolve with Unset outcome)
7	NotParty	Reserved for party-restricted operations
8	AlreadyFinalised	Level3 ruling is binding; no further transitions allowed
9	DeadlineNotPassed	Cannot expire or advance a dispute before its current deadline
10	AlreadyTerminal	Dispute is already in Expired state
11	AlreadyPendingReview	keeper_advance_stage already called; repeated call rejected
Storage Keys
Key	Type	Description
Owner	Address	Upgrade authority
Admin	Address	Dispute resolution authority
Dispute(u128)	DisputeDetails	Per-dispute state keyed by agreement_id
LevelTimeLimit(EscalationLevel)	u64	SLA window in seconds per tier
PendingReviewTimeLimit	u64	Review window in seconds after SLA breach
text


Now let me update the state-machines doc and write the implementation back to lib.rs: