# Conditional Payment Triggers

This document describes how conditional payment triggers work in StellopayCore,
covering all three trigger categories, their boundary conditions, failure modes,
and the test coverage added in `tests/test_conditional_triggers.rs`.

---

## Overview

A _conditional payment trigger_ is any on-chain condition that must be satisfied
before a payment is released.  StellopayCore supports three categories:

| Category | Contract function(s) | Condition |
|---|---|---|
| **Time-based** | `claim_time_based`, `claim_payroll` | A minimum number of whole periods must have elapsed since activation |
| **Milestone-based** | `approve_milestone` + `claim_milestone` | Employer must explicitly approve before contributor can claim |
| **Composite** | All of the above + emergency pause / dispute | Multiple conditions must hold simultaneously |

---

## 1. Time-Based Triggers

### How they work

Every escrow and payroll agreement has an _activation timestamp_ and a
_period duration_.  The number of claimable periods at any point in time is:

```
claimable_periods = floor((now - activated_at) / period_seconds) - claimed_periods
```

`claim_time_based` (escrow) and `claim_payroll` (payroll) each enforce this
check before executing a token transfer.

### Key invariants

* **No fractional periods** — only complete periods are counted.
* **Caps at `num_periods`** — escrow agreements cannot pay beyond the originally
  agreed number of periods even if extra time elapses.
* **Sequential** — claimed periods are tracked per-employee/contributor; a second
  call only covers periods that opened since the last successful claim.

### Boundary conditions

| Timestamp relative to period end | Expected result |
|---|---|
| `now == activated_at + period_seconds - 1` | **Blocked** — not yet a full period |
| `now == activated_at + period_seconds` | **Succeeds** — exactly one period claimable |
| `now == activated_at + N * period_seconds` | **Succeeds** — N periods claimable |
| `now >> activated_at + num_periods * period_seconds` | **Capped** at `num_periods` |

### Blocking conditions

A time-based claim is rejected if any of the following hold:

* Agreement status is `Paused` — `AgreementPaused` error.
* Agreement status is `Disputed` — `ActiveDispute` error.
* Contract-level emergency pause is active — `EmergencyPaused` error.
* Agreement is `Cancelled` and the grace period has already expired —
  `NotInGracePeriod` error.
* All periods have already been claimed — `AllPeriodsClaimed` error.
* Zero periods have elapsed since the last claim — `NoPeriodsToClaim` error.

---

## 2. Milestone-Based Triggers

### How they work

Milestone agreements use a two-step release:

1. **Employer approves** — calls `approve_milestone(agreement_id, milestone_id)`.
   Sets the `approved` flag in storage.  No funds move.
2. **Contributor claims** — calls `claim_milestone(agreement_id, milestone_id)`.
   Checks `approved == true && claimed == false`, then marks the milestone
   claimed and transfers funds.

### Key invariants

* **Approval is a prerequisite** — `claim_milestone` panics with
  `"Milestone not approved"` if the flag is false.
* **One-time claim** — a claimed milestone cannot be re-claimed
  (`"Milestone already claimed"`).
* **Any order** — milestones do not have to be approved or claimed in
  creation order.
* **Auto-complete** — when the last unclaimed milestone is claimed the
  agreement status transitions to `Completed`.

### Failure modes

| Caller | Operation | Result |
|---|---|---|
| Non-employer | `approve_milestone` | Auth failure (panic) |
| Non-contributor | `claim_milestone` | Auth failure (panic) |
| Either | `claim_milestone` before approval | `"Milestone not approved"` panic |
| Either | `claim_milestone` twice | `"Milestone already claimed"` panic |
| Either | Invalid milestone ID (0 or > count) | `"Invalid milestone ID"` panic |
| Contributor | `claim_milestone` while paused | `"Cannot claim when agreement is paused"` panic |

### Batch claiming

`batch_claim_milestones(agreement_id, milestone_ids)` iterates the supplied IDs
and attempts each individually.  Failures are **non-fatal** — processing
continues to the next ID.  The returned `BatchMilestoneResult` contains:

* `successful_claims` — count of IDs that were successfully claimed.
* `failed_claims` — count of IDs that failed (not approved, already claimed,
  invalid, duplicate).
* `total_claimed` — sum of amounts for successful claims.
* `results` — per-ID detail with an `error_code` (`0` = success).

Duplicate IDs in the input vector are detected in-memory and counted as failures.

---

## 3. Composite Triggers

Several contract-level states can override or gate individual trigger checks:

### Emergency pause

When `emergency_pause()` is called by the contract owner, **all** claim
operations are blocked regardless of the per-agreement state.  This includes:

* `claim_time_based`
* `claim_payroll` / `batch_claim_payroll`
* `claim_milestone` / `batch_claim_milestones`

The pause is lifted by `emergency_unpause()` (owner only).

### Dispute

Raising a dispute (`raise_dispute`) transitions the agreement to `Disputed`
status.  While disputed:

* New payroll claims are rejected.
* The arbiter may call `resolve_dispute` to distribute funds and move the
  agreement to `Completed`.

### Grace period

When an employer cancels an active agreement:

1. Status becomes `Cancelled`.
2. `cancelled_at` is recorded.
3. Employees/contributors may still claim for already-elapsed periods while
   `now < cancelled_at + grace_period_seconds` (i.e., while the grace period
   is active).
4. After the grace period expires, all claims are blocked.
5. The employer may then call `finalize_grace_period` to refund the remaining
   escrow balance.

---

## Test Coverage (`test_conditional_triggers.rs`)

The test file is organized into four sections:

### Section 1 — Time-based triggers (12 tests)

| Test name | What it validates |
|---|---|
| `test_time_based_claim_after_one_period` | Basic happy path: one period, correct amount |
| `test_time_based_claim_blocked_before_first_period` | Claim rejected before period elapses |
| `test_time_based_boundary_last_second_before_period_end` | Boundary: `t = period - 1s` → blocked |
| `test_time_based_boundary_first_second_after_period_end` | Boundary: `t = period` → succeeds |
| `test_time_based_multiple_periods_accumulate` | 3 periods claimed in one call |
| `test_time_based_cannot_over_claim_beyond_num_periods` | Caps at `num_periods` |
| `test_time_based_sequential_claims_correct_amounts` | Period 1 then period 2 accumulate |
| `test_time_based_claim_blocked_on_paused_agreement` | Paused agreement blocks claim |
| `test_time_based_claim_blocked_during_emergency_pause` | Emergency pause blocks claim |
| `test_time_based_partial_period_not_claimable` | 1.5 periods → only 1 claimable |
| `test_time_based_large_period_boundary` | One-week period boundary |
| `test_time_based_escrow_completes_after_all_periods` | Status → Completed on last claim |

### Section 2 — Milestone-based triggers (12 tests)

| Test name | What it validates |
|---|---|
| `test_milestone_claim_blocked_before_approval` | No claim without approval |
| `test_milestone_approval_does_not_transfer_funds` | Approval ≠ transfer |
| `test_milestone_claim_succeeds_immediately_after_approval` | Full lifecycle |
| `test_milestone_double_claim_rejected` | Already-claimed check |
| `test_milestone_out_of_order_approval_and_claim` | Non-sequential ordering |
| `test_milestone_wrong_caller_cannot_claim` | Auth check on claim |
| `test_milestone_wrong_caller_cannot_approve` | Auth check on approve |
| `test_milestone_claim_blocked_when_paused` | Paused agreement blocks claim |
| `test_milestone_batch_claim_only_approved` | Batch skips unapproved |
| `test_milestone_batch_claim_skips_duplicates` | Duplicate deduplication |
| `test_milestone_batch_claim_partial_success_correct_counts` | Mixed outcome counts |
| `test_milestone_invalid_id_rejected` | Invalid milestone ID panics |

### Section 3 — Payroll period triggers (6 tests)

| Test name | What it validates |
|---|---|
| `test_payroll_claim_blocked_before_first_period` | No periods elapsed → error |
| `test_payroll_claim_after_one_period_correct_amount` | Correct salary transfer |
| `test_payroll_batch_distributes_to_multiple_employees` | Three-employee batch |
| `test_payroll_wrong_employee_index_rejected` | Out-of-range index → error |
| `test_payroll_claim_blocked_if_agreement_not_active` | Not-activated → error |
| `test_payroll_claim_blocked_after_grace_period_expired` | Post-grace → error |
| `test_payroll_claim_in_token_applies_fx_rate` | FX conversion at claim time |

### Section 4 — Composite triggers (4 tests)

| Test name | What it validates |
|---|---|
| `test_composite_pause_resume_time_trigger_fires_after_resume` | Pause/resume lifecycle |
| `test_composite_emergency_pause_blocks_all_trigger_types` | Emergency blocks all three types |
| `test_composite_dispute_blocks_payroll_claims` | Disputed status blocks claims |
| `test_composite_grace_period_claim_succeeds_then_expires` | Grace window: succeeds then fails |

---

## Running the Tests

```bash
cd onchain
cargo test -p stello_pay_contract test_conditional_triggers
```

To run with output visible:

```bash
cargo test -p stello_pay_contract test_conditional_triggers -- --nocapture
```
