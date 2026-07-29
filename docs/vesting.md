## Token Vesting Contract

This document describes the `token_vesting` smart contract added for issue `#198`.

### Scope

The token vesting contract manages time-based release of tokens for employees and other beneficiaries:

- linear vesting over a time range
- single-time cliff vesting
- custom step schedules
- early release with admin approval
- revocation of unvested tokens for terminated employees

### Contract Location

- Contract: `onchain/contracts/token_vesting/src/lib.rs`
- Tests: `onchain/contracts/token_vesting/tests/test_vesting.rs`

### Security Model

- `initialize` is **one-time only** and sets the contract owner (admin).
- Employers must **escrow the full vesting amount up front** at schedule creation.
- Only the **beneficiary** can claim vested tokens for their schedule.
- Only the **contract owner** can approve early release of unvested tokens.
- Only the **employer** that created a revocable schedule can revoke it.
- Revocation refunds only the **unvested** portion; vested amounts remain claimable by the beneficiary.

### Data Model

Core types:

- `VestingKind`
  - `Linear`
  - `Cliff`
  - `Custom`
- `VestingStatus`
  - `Active`, `Revoked`, `Completed`
- `CustomCheckpoint`
  - `time`: absolute timestamp
  - `cumulative_amount`: total vested amount at `time`
- `VestingSchedule`
  - `id`, `employer`, `beneficiary`, `token`
  - `kind`, `status`, `revocable`, `revoked_at`
  - `total_amount`, `released_amount`
  - `start_time`, `end_time`, optional `cliff_time`
  - `checkpoints`: used for `Custom` schedules

Storage keys:

- `Owner`: contract owner/admin
- `Initialized`: one-time initialization flag
- `NextScheduleId`: auto-incrementing schedule id
- `Schedule(id)`: stored `VestingSchedule`

### Vesting Logic

- **Linear**
  - Vesting grows proportionally between `start_time` and `end_time`.
  - If `cliff_time` is set, nothing vests until `now >= cliff_time`; once the cliff is reached, the normal linear formula applies retroactively from `start_time`.
  - Vested amount = `total * (now - start) / (end - start)`, capped at `total`.
- **Cliff**
  - No vesting before `cliff_time`.
  - 100% vests at `cliff_time`.
- **Custom**
  - Uses ordered `CustomCheckpoint` entries.
  - Vested amount = last `cumulative_amount` with `time <= now`, capped at `total`.
- When a schedule is **revoked**, `revoked_at` freezes further vesting; vested amount at that time remains claimable.

### Public API

- `initialize(owner)`
- `create_linear_schedule(employer, beneficiary, token, total_amount, start_time, end_time, cliff_time, revocable) -> id`
- `create_cliff_schedule(employer, beneficiary, token, total_amount, cliff_time, revocable) -> id`
- `create_custom_schedule(employer, beneficiary, token, total_amount, checkpoints, revocable) -> id`
- `claim(beneficiary, schedule_id) -> amount`
- `approve_early_release(admin, schedule_id, amount) -> released`
- `revoke(employer, schedule_id) -> refunded_amount`
- `get_schedule(id) -> Option<VestingSchedule>`
- `get_vested_amount(id) -> i128`
- `get_releasable_amount(id) -> i128`
- `get_owner() -> Option<Address>`

### Workflow Summary

1. Admin calls `initialize(owner)`.
2. Employer funds and creates a vesting schedule (linear, cliff, or custom).
3. Beneficiary monitors `get_vested_amount` / `get_releasable_amount` and calls `claim` to pull vested tokens.
4. Admin can use `approve_early_release` to unlock part of the **unvested** portion ahead of schedule.
5. Employer can call `revoke` on revocable schedules to reclaim unvested tokens when an employee is terminated; beneficiary can still claim any vested remainder.

### Security Notes

**Escrow-first design** — Tokens are transferred into the contract at schedule
creation. There is no "promise to pay"; the contract always holds sufficient
balance for active schedules.

**Error codes:**

| Error Code | Name | Description |
|---|---|---|
| 1 | ContractNotInitialized | Contract has not been initialized yet |
| 2 | AlreadyInitialized | Contract has already been initialized |
| 3 | Unauthorized | Caller is not authorized to perform this action |
| 4 | InvalidInput | Invalid input parameter provided |
| 5 | ScheduleNotFound | Vesting schedule with given ID does not exist |
| 6 | NothingToClaim | No vested tokens are currently claimable |
| 7 | ScheduleCompleted | Schedule has already been fully claimed and marked as completed |
| 8 | RevokedSchedule | Schedule has been revoked and has no releasable amount |

**Authentication model:**

| Action | Authorized caller |
|---|---|
| `initialize` | Owner (one-time) |
| `create_*_schedule` | Employer |
| `claim` | Beneficiary only |
| `approve_early_release` | Contract owner/admin only |
| `revoke` | Employer that created the schedule |
| `get_*` (read-only) | No auth required |

**Invariants enforced:**

- `released_amount` can never exceed `total_amount`; `claim` marks the schedule
  `Completed` once equality is reached, preventing further claims.
- Double-claim at the same timestamp returns 0 releasable and panics with
  "Nothing to claim".
- Revocation freezes the vesting clock at `revoked_at`; the beneficiary can
  still claim the already-vested portion, but no further tokens accrue.
- **Fully-vested revoke is a safe no-op**: if `get_vested_amount` already
  equals `total_amount` when `revoke` is called, no tokens are transferred
  (`refunded_amount` is `0`), and schedule bookkeeping is unaffected —
  `get_vested_amount` and `get_releasable_amount` return the same values
  before and after the call, since `revoked_at` is only ever set at or after
  the point vesting had already completed. The schedule's `status` still
  moves to `Revoked` (revocation is recorded as a real event even when there
  is nothing left to claw back), and a second `revoke` call is rejected
  (`"Schedule not active"`) rather than accepted as another no-op. See
  `test_revoke_after_fully_vested_is_safe_noop` and related tests in
  `tests/test_vesting.rs` (issue #1066).
- `approve_early_release` caps the released amount at the unvested remainder
  (`total_amount - vested`), so the admin cannot over-release even when a
  prior `claim` has already consumed part of the vested portion. The cap
  operates independently of prior early releases and claims, guaranteeing
  that `released_amount` never exceeds `total_amount`.
- The invariant `released_amount <= total_amount` holds across any sequence
  of `claim` and `approve_early_release` calls. At any point, the maximum
  extra amount the admin can early-release is `total_amount - vested`,
  and the maximum the beneficiary can claim is `vested - released_amount`.
  The two are disjoint: the first draws from the unvested pool, the second
  from the vested pool, and neither exceeds its respective bound.
- **Monotonicity** — `get_vested_amount` is non-decreasing as ledger time
  advances. For any two timestamps `t1 <= t2`:
  `get_vested_amount(id) @ t1  <=  get_vested_amount(id) @ t2`.
  This invariant holds for all three schedule kinds (Linear, Cliff, Custom)
  and is preserved by revocation (frozen clock). See
  [Monotonicity guarantee](#monotonicity-guarantee) below for a full
  statement and rationale.
- Schedule IDs are auto-incremented and never reused.

**Input validation:**

- `total_amount` must be > 0.
- Linear: `end_time > start_time`; optional `cliff_time` must be within
  `[start_time, end_time]`.
- Custom: checkpoints must be sorted by time with non-decreasing cumulative
  amounts; last checkpoint must equal `total_amount`.
- All state-mutating functions require `require_initialized` before proceeding.

**Known limitations:**

- No cross-contract integration tests with `stello_pay_contract` yet.

### Monotonicity Guarantee

**Core invariant (issue #886):** `get_vested_amount` is guaranteed to be
monotonically non-decreasing as ledger time advances. For any schedule `id`
and any two ledger timestamps `t1 <= t2`:

```
get_vested_amount(id) @ t1  <=  get_vested_amount(id) @ t2
```

This invariant holds unconditionally for all three schedule kinds and all
schedule states.

#### Per-kind proof sketch

| Kind | Why it is monotonic |
|---|---|
| **Linear** | The vested amount equals `total * (now - start) / (end - start)`, which is a non-decreasing linear function of `now` once `now > start` (and `now >= cliff` when a cliff is set). Before `start` (or before the cliff) the value is 0; after `end` it is `total`. The function is piecewise linear and each piece is non-decreasing. |
| **Cliff** | The function is 0 for `now < cliff_time` and `total_amount` for `now >= cliff_time`. The single step is upward only; once it reaches `total_amount` it stays there. |
| **Custom** | Checkpoints are validated at creation to be sorted by `time` with non-decreasing `cumulative_amount`. The step-function evaluator scans checkpoints in order, so the returned value can only stay flat or increase as `now` advances. The cap at `total_amount` is also non-decreasing. |

#### Revoked schedules

When a schedule is revoked, `revoked_at` is set and the effective timestamp
used by `compute_vested_amount` is frozen at `revoked_at` for all future
queries. The vested amount is therefore constant (non-decreasing) for all
`now >= revoked_at`, and still non-decreasing for `now < revoked_at` because
the schedule was Active up to that point.

#### Why this matters for security

Monotonicity is a prerequisite for the anti-double-claim invariant enforced by
`claim`. The releasable amount is computed as `vested - released_amount`. If
the vested amount could ever decrease, a beneficiary's `released_amount` might
exceed the (erroneously lower) vested amount, causing `releasable` to become
negative. This would either silently skip a payment or, in a buggy
implementation, allow re-claiming tokens that were already withdrawn. The
monotonicity guarantee closes this class of vulnerability entirely.

#### Test coverage (issue #886)

Category O in `tests/test_vesting.rs` adds eleven dedicated property tests:

| Test | Schedule kind | What is verified |
|---|---|---|
| `prop_linear_vested_amount_is_monotonic` | Linear (no cliff) | 10 LCG seeds × 50 timestamps each, full lifecycle range |
| `prop_linear_with_cliff_vested_amount_is_monotonic` | Linear + cliff | 5 seeds × 60 timestamps, cliff straddle |
| `prop_linear_cliff_boundary_monotonic` | Linear + cliff | Dense 1-second sweep [0, 110] |
| `prop_cliff_vested_amount_is_monotonic` | Cliff | 5 seeds × 50 timestamps, pre/post cliff |
| `prop_cliff_boundary_step_is_monotonic` | Cliff | Dense sweep [995, 1005], exact boundary check |
| `prop_custom_vested_amount_is_monotonic` | Custom (3 checkpoints) | 8 seeds × 60 timestamps |
| `prop_custom_checkpoint_boundaries_monotonic` | Custom (3 checkpoints) | Dense sweep [0, 200], spot-check step values |
| `prop_custom_single_checkpoint_is_monotonic` | Custom (1 checkpoint) | LCG sequence [0, 200] |
| `prop_linear_large_total_is_monotonic` | Linear, near i128::MAX/4 | Overflow-safe path, 80 timestamps |
| `prop_custom_dense_checkpoints_monotonic` | Custom (5 checkpoints, 1 s apart) | Dense sweep [0, 10] |

All tests use only Soroban SDK primitives (no external `proptest` or
`quickcheck` crates) and are fully reproducible with fixed LCG seeds.

### Bug Fixes

- **Linear + cliff gate** (issue #198): The `VestingKind::Linear` branch previously
  ignored `cliff_time`, allowing tokens to vest linearly before the cliff was
  reached. A cliff guard was added so that `compute_vested_amount` returns 0
  when `now < cliff_time`, matching the documented behavior for cliff schedules.
- **RevokedSchedule distinct error** (issue #885): Added a dedicated `RevokedSchedule`
  error variant returned by `claim` when attempting to claim against a revoked
  schedule with no releasable amount. This distinguishes "nothing vested yet"
  from "this schedule was revoked and will never pay out", helping integrators
  handle the two cases differently.

### Testing Focus

The test suite contains **43 tests** across 10 categories:
The test suite contains **55 tests** across 15 categories:

| Category | Count | What it covers |
|---|---|---|
| A. Initialization | 4 | `initialize` idempotency, pre-init guards, missing schedule, owner before init |
| B. Linear | 7 | Exact start/end boundaries, past-end cap, cliff gate (before/at/after), full claim flow |
| C. Cliff | 5 | 1 s before cliff (=0), exact cliff (=total), 1 s after cliff (still total — no linear accrual), full claim, revoke-before-cliff refund |
| D. Custom | 4 | Before first checkpoint, between checkpoints, at final checkpoint, early release |
| E. Claim Security | 5 | Non-beneficiary rejected, double-claim fails, completed schedule rejected, released_amount accumulates, token balance verification |
| F. Revocation | 5 | Non-revocable rejected, non-employer rejected, double-revoke rejected, partial-vesting split (employer refund + beneficiary claim remainder), claim after revoke with nothing to claim returns distinct error |
| G. Early Release | 3 | Non-owner rejected, amount capped at unvested, revoked schedule rejected |
| H. State Consistency | 2 | Claim after revoke gets frozen vested remainder, schedule IDs are sequential |
| I. Input Validation | 5 | Zero amount, end < start, cliff outside range, empty checkpoints, unsorted checkpoints |
| J. Edge Cases | 3 | Minimal-duration linear schedule, custom vested cap, invalid schedule_id |
| K. Events | 4 | Create, claim, revoke, and early release event data correctness |
| L. Cliff + Linear | 5 | Full spectrum, cliff=end, cliff=start, small amount, revoked (issue #516) |
| M. Overflow Safety | 3 | Large total near i128::MAX, long duration, vested never exceeds total |
| N. Early Release Bound | 2 | Prior claim + capped early release, bounded early release + claim exact transfers (issue #884) |
| O. Monotonicity Property | 10 | Non-decreasing vested amount for Linear (with/without cliff), Cliff, and Custom schedules across pseudo-random timestamp sequences and dense boundary sweeps (issue #886) |

### Edge Case Reference

| Scenario | Kind | Expected result |
|---|---|---|
| `now == start_time` | Linear | 0 (boundary is `<=`) |
| `now == end_time` | Linear | `total_amount` (boundary is `>=`) |
| `now > end_time` | Linear | `total_amount` (capped) |
| `now < cliff_time` (Linear w/ cliff) | Linear | 0 |
| `now == cliff_time` (Linear w/ cliff) | Linear | proportional from `start_time` |
| `now == cliff_time - 1` | Cliff | 0 |
| `now == cliff_time` | Cliff | `total_amount` |
| `now == cliff_time + 1` | Cliff | `total_amount` (no further linear accrual) |
| Before first checkpoint | Custom | 0 |
| Between checkpoints | Custom | last passed `cumulative_amount` |
| After revocation (`now > revoked_at`) | Any | vested amount frozen at `revoked_at` |
| `revoke` called once fully vested | Any | safe no-op: `refunded_amount == 0`, no transfer, `status` → `Revoked`, vested/releasable amounts unchanged |
| `revoke` called twice (second call after fully-vested revoke) | Any | panics `"Schedule not active"` — not treated as a further no-op |

### Soroban Events

The contract emits events for key lifecycle actions to support off-chain indexing.

#### `vesting_created`
Emitted when a new linear, cliff, or custom schedule is successfully created and funded.
- **Topic 1**: `Symbol("vesting_created")`
- **Topic 2**: `schedule_id` (u128)
- **Data**: `CreatedEvent` struct
  - `id`: u128
  - `employer`: Address
  - `beneficiary`: Address
  - `token`: Address
  - `kind`: VestingKind (Linear, Cliff, or Custom)
  - `amount`: i128 (Total vesting amount)

#### `vesting_claimed`
Emitted when a beneficiary claims vested tokens.
- **Topic 1**: `Symbol("vesting_claimed")`
- **Topic 2**: `schedule_id` (u128)
- **Data**: `ClaimedEvent` struct
  - `id`: u128
  - `beneficiary`: Address
  - `amount`: i128 (Amount just released)

#### `vesting_revoked`
Emitted when an employer revokes a revocable schedule.
- **Topic 1**: `Symbol("vesting_revoked")`
- **Topic 2**: `schedule_id` (u128)
- **Data**: `RevokedEvent` struct
  - `id`: u128
  - `employer`: Address
  - `refunded`: i128 (Amount returned to employer)
  - `at`: u64 (Ledger timestamp of revocation)

#### `vesting_early_release`
Emitted when the contract owner approves an early release of unvested tokens.
- **Topic 1**: `Symbol("vesting_early_release")`
- **Topic 2**: `schedule_id` (u128)
- **Data**: `EarlyReleaseEvent` struct
  - `id`: u128
  - `admin`: Address
  - `amount`: i128 (Amount released ahead of schedule)

