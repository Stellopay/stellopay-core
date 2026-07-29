Agreement State Machines
This document provides a high-level, implementation-aligned view of the main state machines in the payroll contract, focusing on agreement lifecycle, disputes, and grace-period cancellation.

It is intentionally minimal, but accurate enough to guide reviews, audits, and integration work.

Core Agreement Lifecycle
On-chain type: AgreementStatus (see onchain/contracts/stello_pay_contract/src/storage.rs).

States:

Created: agreement exists but is not yet activated
Active: agreement is live, and payments/claims are allowed
Paused: agreement temporarily suspended
Cancelled: employer has cancelled; grace-period refund flow applies
Completed: agreement fully settled (all funds distributed or refunded)
Disputed: a dispute has been raised and must be resolved
State Diagram
mermaid

stateDiagram-v2
    [*] --> Created

    Created --> Active: activate_agreement\n(preconditions satisfied)
    Active --> Paused: pause_agreement\n(employer)
    Paused --> Active: resume_agreement\n(employer)

    Created --> Cancelled: cancel_agreement\n(employer)
    Active --> Cancelled: cancel_agreement\n(employer)

    Active --> Completed: all payments claimed\nor resolved
    Cancelled --> Completed: finalize_grace_period\n(refund employer)

    Created --> Disputed: raise_dispute\n(party)
    Active --> Disputed: raise_dispute\n(party)
    Disputed --> Completed: resolve_dispute\n(arbiter)
Main Transitions and Conditions
Created → Active

Trigger: activate_agreement
Conditions:
Agreement exists
For payroll mode: at least one employee added
Caller is employer
Effects:
status = Active
activated_at set to current ledger timestamp
Active ↔ Paused

Active → Paused
Trigger: pause_agreement
Conditions: caller is employer; status is Active
Effects: status = Paused
Paused → Active
Trigger: resume_agreement
Conditions: caller is employer; status is Paused
Effects: status = Active
Invalid transitions: resume_agreement panics if called when status is Created, Active, Cancelled, Completed, or Disputed. This precondition is enforced by an explicit `assert!` in the contract implementation and is covered by regression tests (see `test_resume_rejects_all_non_paused_statuses` and individual `test_resume_*_agreement_panics` tests in `test_state_machine.rs`).
Created/Active → Cancelled

Trigger: cancel_agreement
Conditions:
Caller is employer
Status is Created or Active
Effects:
status = Cancelled
cancelled_at set to current timestamp
Grace period window becomes active (grace_period_seconds)
Cancelled → Completed

Trigger: finalize_grace_period
Conditions:
Status is Cancelled
Grace period has fully elapsed
Effects:
Remaining escrow refunded to employer
If no claims were made, the refund equals the full escrow balance; if earlier claims already paid out some periods, the refund equals the unclaimed remainder only
Agreement marked logically complete (no further claims expected)
Active → Completed

Trigger: last payment / last milestone claim / dispute resolution
Conditions:
All funds have been distributed according to the agreement
Effects:
status = Completed
Dispute Lifecycle (Payroll Contract — Simple Path)
On-chain type: DisputeStatus and the dispute_status field on Agreement.

States:

None: default, no active dispute
Raised: dispute opened by employer or employee
Resolved: dispute resolved by arbiter
State Diagram
mermaid

stateDiagram-v2
    [*] --> None
    None --> Raised: raise_dispute\n(employer or employee)
    Raised --> Resolved: resolve_dispute\n(arbiter)
    Resolved --> [*]
Transitions and Conditions
None → Raised

Trigger: raise_dispute
Conditions:
Caller is employer or participant in the agreement
No existing active dispute
Effects:
dispute_status = Raised
DisputeStatus storage updated; dispute_raised_at set
Raised → Resolved

Trigger: resolve_dispute
Conditions:
Caller is configured arbiter
Payout split (pay_employee, refund_employer) does not exceed total locked funds
Effects:
Funds distributed according to arbiter decision
dispute_status = Resolved
Agreement typically transitions to Completed
Error states (see PayrollError):

DisputeAlreadyRaised, NotParty, NotArbiter, InvalidPayout, ActiveDispute, NoDispute
These correspond to:
double-raise attempts
unauthorized callers
over-allocating beyond total locked amount
conflicting lifecycle (e.g., trying to finalize while dispute is active)
Dispute Escalation Contract State Machine
On-chain type: DisputeStatus in onchain/contracts/dispute_escalation.

This is the full three-tier escalation state machine with per-level SLA timers,
a keeper-triggered PendingReview stage, and binding outcome records.

States
State	Terminal?	Description
Open	No	Dispute filed at Level1; SLA clock running
Escalated	No	Moved to next tier; fresh SLA clock running
Appealed	No	Level1/2 ruling appealed; fresh SLA at next level
PendingReview	No	SLA elapsed; keeper advanced stage; admin review window open
Resolved	No	Admin ruled at Level1/2; 3-day appeal window open
Finalised	Yes	Admin ruled at Level3; binding, no further appeal
Expired	Yes	Phase deadline passed with no admin action; closed without ruling
State Diagram
mermaid

stateDiagram-v2
    [*] --> Open : file_dispute

    Open --> Escalated       : escalate_dispute\n(now ≤ deadline)
    Escalated --> Escalated  : escalate_dispute\n(now ≤ deadline, level < 3)

    Open --> PendingReview      : keeper_advance_stage\n(now > deadline)
    Escalated --> PendingReview : keeper_advance_stage\n(now > deadline)
    Appealed --> PendingReview  : keeper_advance_stage\n(now > deadline)

    Open --> Resolved      : resolve_dispute\n(admin, L1/L2)
    Escalated --> Resolved : resolve_dispute\n(admin, L1/L2)
    Appealed --> Resolved  : resolve_dispute\n(admin, L1/L2)
    PendingReview --> Resolved : resolve_dispute\n(admin, L1/L2)

    Open --> Finalised      : resolve_dispute\n(admin, L3)
    Escalated --> Finalised : resolve_dispute\n(admin, L3)
    Appealed --> Finalised  : resolve_dispute\n(admin, L3)
    PendingReview --> Finalised : resolve_dispute\n(admin, L3)

    Resolved --> Appealed : appeal_ruling\n(now ≤ appeal_deadline)

    Open --> Expired         : expire_dispute\n(now > deadline)
    Escalated --> Expired    : expire_dispute\n(now > deadline)
    Appealed --> Expired     : expire_dispute\n(now > deadline)
    PendingReview --> Expired : expire_dispute\n(now > review_deadline)

    Finalised --> [*]
    Expired --> [*]
SLA Timer Design
Every phase is governed by a deterministic ledger timestamp stored in
DisputeDetails.phase_deadline, computed from env.ledger().timestamp():

text

file_dispute         →  phase_deadline = now + level_time_limit(Level1)   [default 7 days]
escalate_dispute     →  phase_deadline = now + level_time_limit(next_level)
resolve_dispute      →  phase_deadline = now + 259_200 (3-day appeal window) [L1/L2 only]
appeal_ruling        →  phase_deadline = now + level_time_limit(next_level)
keeper_advance_stage →  phase_deadline = now + pending_review_time_limit   [default 3 days]
Boundary semantics — now == deadline is still within the window:

Action	Passes when	Blocked when
escalate_dispute	now ≤ deadline	now > deadline → TimeLimitExpired
expire_dispute	now > deadline	now ≤ deadline → DeadlineNotPassed
keeper_advance_stage	now > deadline	now ≤ deadline → DeadlineNotPassed
appeal_ruling	now ≤ appeal_deadline	now > appeal_deadline → TimeLimitExpired
PendingReview — Keeper-Triggered SLA Enforcement
keeper_advance_stage is permissionless: any address may call it once
the current phase_deadline has elapsed. Security invariants:

No stage skipping — transitions only to PendingReview, never directly
to Resolved or Finalised.
Idempotent-safe — a second call returns AlreadyPendingReview;
no duplicate event is emitted.
Level preserved — level and outcome are not mutated.
No outcome authority — only the admin can write a binding ruling.
Transitions Detail
(none) → Open

Trigger: file_dispute(caller, agreement_id)
Conditions: no existing dispute for agreement_id
Effects: status = Open, level = Level1, phase_deadline = now + L1_limit
Open/Escalated → Escalated

Trigger: escalate_dispute(caller, agreement_id)
Conditions: not terminal, not resolved, now ≤ phase_deadline
Effects: level++, status = Escalated, fresh SLA window for new level
Open/Escalated/Appealed → PendingReview

Trigger: keeper_advance_stage(caller, agreement_id) (permissionless)
Conditions: not terminal, not resolved, not already PendingReview, now > phase_deadline
Effects: status = PendingReview, phase_started_at = now, phase_deadline = now + review_limit
Emits: sla_violation_advanced event with breached_at and review_deadline
Open/Escalated/Appealed/PendingReview → Resolved (L1/L2)

Trigger: resolve_dispute(admin, agreement_id, outcome) — admin only
Conditions: not terminal, not already resolved, outcome ≠ Unset
Effects: status = Resolved, outcome written, 3-day appeal window set
Open/Escalated/Appealed/PendingReview → Finalised (L3)

Trigger: resolve_dispute(admin, agreement_id, outcome) — admin only
Conditions: level == Level3, not terminal, not resolved, outcome ≠ Unset
Effects: status = Finalised, outcome written — terminal, no appeal
Resolved → Appealed

Trigger: appeal_ruling(caller, agreement_id)
Conditions: status == Resolved, now ≤ phase_deadline, level < Level3
Effects: level++, status = Appealed, outcome = Unset (re-review), fresh SLA
Open/Escalated/Appealed/PendingReview → Expired

Trigger: expire_dispute(caller, agreement_id) (permissionless)
Conditions: not terminal, not resolved, now > phase_deadline
Effects: status = Expired — terminal
Emits: dispute_expired event (downstream escrow releases funds to payer)
Error States
Error	Code	When
Unauthorized	1	Non-admin calls resolve_dispute or configuration functions
DisputeNotFound	2	agreement_id has no dispute
AlreadyResolved	3	Double-resolve attempt; keeper/expire on resolved dispute
MaxEscalationReached	4	escalate_dispute or appeal_ruling at Level3
TimeLimitExpired	5	SLA/appeal window elapsed; action no longer valid
InvalidTransition	6	Illegal state change (escalate from PendingReview, appeal non-resolved, Unset outcome)
NotParty	7	Reserved
AlreadyFinalised	8	Any action on a Finalised dispute
DeadlineNotPassed	9	Early expire or keeper call before deadline
AlreadyTerminal	10	Any action on an Expired dispute
AlreadyPendingReview	11	keeper_advance_stage called twice on same dispute
Grace-Period and Cancellation Flow
The grace-period flow ties cancellation and finalization together.

High-level:

Employer cancels an Active or Created agreement:
status becomes Cancelled
cancelled_at set
Grace period (grace_period_seconds) begins:
Claims may still be allowed during this window
Refunds are blocked until the window has expired
Finalization:
After the grace period, employer calls finalize_grace_period
Remaining escrow is refunded and the agreement is effectively completed
This flow is validated in the test_grace_period.rs suite and underpins all time-based termination behavior for agreements.

Milestone-Interface Conformance
The milestone query functions (get_milestone, get_milestone_count, and the on_milestone_expired hook) are declared as a shared trait in onchain/contracts/milestone-interface/src/lib.rs. Any contract can depend on this crate (rlib) and use the generated MilestoneContractClient to call milestone queries on any deployed stello_pay_contract.

Conformance test suite
See test_milestone_interface_conformance in onchain/contracts/stello_pay_contract/tests/test_milestones.rs. It verifies:

Function-level parity — every call through MilestoneContractClient returns the same result as the equivalent direct PayrollContractClient call, for every milestone lifecycle state (created, approved, claimed, rejected, expired).
Edge-case parity — out-of-range IDs, zero IDs, and non-existent agreements return None through both clients.
Count accuracy — get_milestone_count is consistent after every lifecycle transition.
Field fidelity — all MilestoneView scalar fields (id, amount, approved, claimed) match their Milestone counterparts bit-for-bit.
Adding the MilestoneContractClient to the test harness is a single import:

Rust

use milestone_interface::{MilestoneContractClient, MilestoneView};
Future milestone-capable contracts should add a similar conformance test to confirm they satisfy the trait contract.

Unauthorized-approval invariant
The most security-critical conformance rule is:

> An approve_milestone call whose invoker is not the recorded employer for that agreement must be rejected.

This invariant must hold for every contract that implements MilestoneContractInterface.  Integrators depend on a single, stable error behaviour across all milestone-capable contracts so they can write uniform error-handling code.

Error behaviour
The reference implementation (stello_pay_contract) enforces this by calling employer.require_auth() at the top of approve_milestone before any write.  This produces a host-level auth failure (ScError::Auth / InvokeContractError) rather than a typed PayrollError variant.

Callers using the try_approve_milestone variant will receive Err(Err(InvokeContractError)).  Conformance tests therefore assert result.is_err() rather than matching a specific PayrollError discriminant, which keeps the tests valid even if the rejection mechanism is changed to a structured error in a future upgrade.

Conformance tests
Three test functions collectively verify this invariant end-to-end:

| Test function | File | What it checks |
|---|---|---|
| test_approve_unauthorized_fails | tests/test_milestones.rs | Legacy #[should_panic] guard — baseline regression |
| test_milestone_interface_unauthorized_approval_conformance | tests/test_milestones.rs | Canonical fixture: try_ variant, error assertion, no partial-write |
| test_milestone_interface_wrong_caller_approval_conformance | tests/test_milestones.rs | Identity-based check: a stranger with valid Soroban auth is still rejected |
| test_mock_unauthorized_approval_conformance | milestone-interface/tests/mock_contract.rs | Same invariant on the minimal in-crate mock implementer |
| test_mock_state_unchanged_after_unauthorized_approval | milestone-interface/tests/mock_contract.rs | State idempotency after multiple failed attempts |

The canonical fixture (test_milestone_interface_unauthorized_approval_conformance) is the reference implementation for future implementers.  Its structure must be reproduced for any new contract that implements MilestoneContractInterface:

1. Create a milestone agreement and add at least one milestone.
2. Clear the auth context (env.mock_auths(&[])).
3. Call try_approve_milestone from the cleared-auth context.
4. Assert result.is_err().
5. Assert the milestone's approved flag is still false.

Security assumptions validated
* Caller identity, not just auth context — employer.require_auth() checks that the specific employer address stored in the agreement has signed the transaction, not just any address.
* No partial-write on failure — because require_auth() is called before any persistent write, a failed auth cannot leave the agreement in a partially mutated state.
* Discriminant stability — PayrollError::Unauthorized has discriminant 11 and must never be renumbered.  The conformance tests are deliberately agnostic to this value (they use is_err()) so they remain valid across upgrades.

Mock implementer
onchain/contracts/milestone-interface/tests/mock_contract.rs contains a minimal in-crate mock (MockMilestoneContract) that implements MilestoneContractInterface and carries the full set of conformance tests.  Running cargo test -p milestone-interface exercises these tests without depending on stello_pay_contract.

Adding the MilestoneContractClient to the test harness is a single import:

Rust

use milestone_interface::{MilestoneContractClient, MilestoneView};
Future milestone-capable contracts should add a similar conformance test to confirm they satisfy the trait contract.

Admin-Only Agreement Storage Setters (#849)
Overview
Five low-level storage-setter entrypoints are exposed under the admin_set_agreement_* prefix. These allow a privileged operator (contract owner or RBAC Admin) to perform emergency maintenance writes directly on agreement storage fields, bypassing the normal agreement state machine.

Every setter is gated behind require_upgrade_admin, which calls operator.require_auth() and then checks either:

The operator matches the stored owner address, or
An RBAC contract is configured and the operator holds the Admin role.
A non-admin caller always gets an "Unauthorized" panic — the write never reaches persistent storage.

Setter Surface
Entrypoint	Field Modified	Additional Validation
admin_set_agreement_paid_amount	AgreementPaidAmount(id)	amount >= 0
admin_set_agreement_escrow_balance	AgreementEscrowBalance(id, token)	amount >= 0
admin_set_agreement_token	AgreementToken(id)	none
admin_set_agreement_activation_time	AgreementActivationTime(id)	none
admin_set_agreement_period_duration	AgreementPeriodDuration(id)	duration > 0
Security Assumptions
The admin is trusted. These functions bypass the state-machine invariants. An admin that calls them incorrectly (e.g., setting a zero escrow balance while claims are still outstanding) can corrupt state.
Only for verified maintenance. These entrypoints must only be used after an off-chain audit has confirmed the exact correction needed (e.g. a known bug caused an accounting discrepancy).
Input guards prevent obviously-invalid writes. Negative paid amounts, negative escrow balances, and zero-duration periods are rejected even for the admin to prevent typos that would break downstream arithmetic.
Not part of the normal workflow. The standard way to manage agreement state is through the lifecycle entrypoints (activate_agreement, fund_milestone_agreement, claim_payroll, etc.). The admin setters are emergency tools only.
Access Control Flow
text

caller → admin_set_agreement_* → require_upgrade_admin(env, operator)
                                      │
                          ┌──── RBAC configured? ────┐
                          │ Yes                       │ No
                          ▼                           ▼
              operator.require_auth()     operator.require_auth()
              rbac.has_role(Admin)?       operator == stored_owner?
              Yes → proceed              Yes → proceed
              No  → panic "Missing role" No  → panic "Unauthorized"
Regression Tests
All five setters are covered in tests/test_boundary_conditions.rs under the ADMIN-ONLY AGREEMENT SETTER TESTS (#849) section:

Negative tests: Each setter panics when called by a non-admin address.
Positive tests: Each setter succeeds when called by the contract owner.
Validation tests: admin_set_agreement_paid_amount(-1) and admin_set_agreement_period_duration(0) panic even for the admin.
Try-variant tests: Using try_admin_set_* confirms non-admin callers receive errors rather than silently succeeding.


## Milestone Interface Versioning and Backward-Compatibility Policy

### Overview

`onchain/contracts/milestone-interface/src/lib.rs` is the **stable contract
surface** that third-party milestone-capable implementations build against.
This section defines what changes to the trait are considered breaking versus
additive, how implementors should track compatibility over time, and how the
versioning scheme maps to the test and documentation artifacts in this
repository.

For the authoritative machine-readable policy (stability labels, XDR encoding
notes, NatSpec-style comments) see the crate-level documentation in
[`onchain/contracts/milestone-interface/src/lib.rs`](../onchain/contracts/milestone-interface/src/lib.rs).

---

### Current interface version

```
INTERFACE_VERSION = 1   (pub const u32 in milestone_interface crate)
```

The constant `INTERFACE_VERSION` is the single source of truth for the
interface generation. It is a compile-time `u32` exported from the
`milestone_interface` crate. Off-chain tooling, CI pipelines, and third-party
contracts may read it to assert they are compiled against the expected revision.

The test `test_interface_version_is_1` in
`stello_pay_contract/tests/test_milestones.rs` hard-codes this value. Any
major version bump must update that test, add a changelog entry to this
section, and follow the upgrade procedure described below.

---

### Method surface (version 1)

| Method | Stability | Signature |
|--------|-----------|-----------|
| `get_milestone` | `@stable` | `(Env, u128, u32) -> Option<MilestoneView>` |
| `get_milestone_count` | `@stable` | `(Env, u128) -> u32` |
| `on_milestone_expired` | `@stable-default` | `(Env, u128, u32) -> ()` — no-op default |

**Stability labels**

| Label | Meaning |
|-------|---------|
| `@stable` | Signature, semantics, and XDR encoding are frozen. Changes require a major version bump and a deprecation cycle. |
| `@stable-default` | Method has a provided default body (no-op). The *presence* of the method is stable; the *default body* may evolve between minor versions as long as the observable no-op contract is preserved. |
| `@unstable` | May change in any release without notice. Not suitable for third-party production use. |

---

### Breaking changes (require a major version bump)

The following changes **require incrementing `INTERFACE_VERSION`**, keeping
the old version accessible for one full release cycle, and adding a changelog
entry to this document:

1. **Removing any trait method** — existing implementors no longer compile.
2. **Changing a method signature** — parameter type, parameter order, or
   return type change breaks both callers and implementors at compile time.
3. **Changing XDR-encoded type layouts** — adding, removing, or reordering
   fields on `#[contracttype]` structs or enums used as method parameters or
   return values breaks cross-contract calls at runtime even when Rust code
   compiles cleanly.
4. **Narrowing a method's documented contract** — e.g. changing a guaranteed
   "returns `None` on unknown id" to "panics on unknown id" is a semantic
   breaking change even when the signature is unchanged.
5. **Changing the discriminant value of an existing enum variant** — XDR
   decoding on the calling side will misinterpret the value. Variants must
   never be reordered and new variants must always be appended.

---

### Additive (non-breaking) changes

The following changes are **backward-compatible** and do not require a major
version bump:

1. **Adding a new method with a provided default body** — existing implementors
   inherit the default silently and continue to compile without change.
   `on_milestone_expired` was introduced this way in version 1 and is the
   canonical example.
2. **Widening a method's documented contract** — e.g. changing "may panic on
   unknown id" to "returns `None` on unknown id" is strictly more permissive.
3. **Appending a new variant to an enum** — provided (a) it is appended at the
   end so existing discriminants are unchanged, and (b) all call-site match
   arms include a wildcard `_` arm.
4. **Adding new `#[contracttype]` structs** not yet used as method parameters
   or return values — they are inert until referenced.
5. **Improving or expanding doc comments** — no runtime impact.

---

### Shared type stability

#### `MilestoneView` fields (version 1)

| Field | Type | Since |
|-------|------|-------|
| `id` | `u32` | 1 |
| `amount` | `i128` | 1 |
| `approved` | `bool` | 1 |
| `claimed` | `bool` | 1 |

Field declaration order must not change. New fields may only be appended in a
future major version. The conformance test `test_milestone_view_fields_stable`
locks these fields and their types.

#### `MilestoneAgreementStatus` variants (version 1)

| Variant | XDR discriminant (0-based) | Since |
|---------|---------------------------|-------|
| `Created` | 0 | 1 |
| `Active` | 1 | 1 |
| `Paused` | 2 | 1 |
| `Cancelled` | 3 | 1 |
| `Completed` | 4 | 1 |
| `Disputed` | 5 | 1 |

The test `test_milestone_agreement_status_variants_stable` locks all six
variants. Any removal causes a compile error; any rename or reorder causes the
test to fail.

---

### Upgrade procedure

When a breaking change is unavoidable:

1. Increment `INTERFACE_VERSION` by 1 in `milestone-interface/src/lib.rs`.
2. Keep the previous interface accessible under a versioned re-export or
   sibling crate (`milestone-interface-v1`) for at least one release cycle so
   existing implementors can migrate at their own pace.
3. Update `stello_pay_contract` to implement the new interface version.
4. Update the conformance test `test_milestone_interface_conformance` and the
   version lock test `test_interface_version_is_1` in
   `stello_pay_contract/tests/test_milestones.rs` to reflect the new version.
5. Add a changelog entry to this section (see format below).

---

### Implementor guidance

Third-party contracts that implement `MilestoneContractInterface` should:

1. **Pin the version** in `Cargo.toml` with an exact or `~` specifier:
   ```toml
   milestone-interface = { version = "=0.0.0", path = "../../milestone-interface" }
   ```
   Open `*` or `^` ranges silently pick up breaking changes on re-build.

2. **Add a conformance test** that exercises every `@stable` method via
   `MilestoneContractClient` and compares output against direct client calls.
   Use `test_milestone_interface_conformance` in
   `stello_pay_contract/tests/test_milestones.rs` as the reference template.

3. **Pin `INTERFACE_VERSION`** in a test:
   ```rust
   #[test]
   fn interface_version_unchanged() {
       assert_eq!(milestone_interface::INTERFACE_VERSION, 1u32);
   }
   ```
   This test fails immediately when the upstream interface is bumped, giving
   the implementor a clear signal to review the changelog and migrate.

4. **Do not override `on_milestone_expired` with a panicking body** unless
   your contract can guarantee the hook is always called in a valid state. A
   panic inside the hook rolls back the entire `expire_milestone` transaction
   in the calling contract.

5. **Handle `None` and `0` defensively** — `get_milestone` returns `None` and
   `get_milestone_count` returns `0` for unknown agreements. These are
   documented semantic guarantees, not implementation details; do not assume
   they imply contract absence.

---

### Cross-reference

| Resource | Location |
|----------|----------|
| Trait definition and inline policy | [`onchain/contracts/milestone-interface/src/lib.rs`](../onchain/contracts/milestone-interface/src/lib.rs) |
| Milestone workflow and data structures | [`onchain/contracts/stello_pay_contract/MILESTONE_DOCS.md`](../onchain/contracts/stello_pay_contract/MILESTONE_DOCS.md) |
| Conformance and versioning tests | [`onchain/contracts/stello_pay_contract/tests/test_milestones.rs`](../onchain/contracts/stello_pay_contract/tests/test_milestones.rs) |
| Mock implementer and conformance tests | [`onchain/contracts/milestone-interface/tests/mock_contract.rs`](../onchain/contracts/milestone-interface/tests/mock_contract.rs) |
| `PayrollError` discriminant stability convention | [`onchain/contracts/stello_pay_contract/src/storage.rs`](../onchain/contracts/stello_pay_contract/src/storage.rs) |
| Hook integration tests (`on_milestone_expired`) | [`onchain/contracts/stello_pay_contract/tests/test_expire_milestone.rs`](../onchain/contracts/stello_pay_contract/tests/test_expire_milestone.rs) |

---

### Changelog

| Version | Date | Change summary |
|---------|------|----------------|
| 1 | 2026-07-28 | Initial stable release. Defines `get_milestone` (`@stable`), `get_milestone_count` (`@stable`), and `on_milestone_expired` hook (`@stable-default`, no-op default). Exports `INTERFACE_VERSION = 1`, `MilestoneView`, `MilestoneAgreementView`, `MilestoneAgreementStatus`. |
| 1 | 2026-07-29 | Unauthorized-approval conformance tests added. Added `test_milestone_interface_unauthorized_approval_conformance` (canonical `try_` fixture), `test_milestone_interface_wrong_caller_approval_conformance` (identity-based check), and matching mock tests (`test_mock_unauthorized_approval_conformance`, `test_mock_state_unchanged_after_unauthorized_approval`) in `milestone-interface/tests/mock_contract.rs`. State-machine docs updated with the full conformance test table and path corrections. No interface version bump — tests are additive. |
