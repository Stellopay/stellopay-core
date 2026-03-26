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
- `approve_early_release` caps the released amount at the unvested remainder,
  so the admin cannot over-release.
- Schedule IDs are auto-incremented and never reused.

**Input validation:**

- `total_amount` must be > 0.
- Linear: `end_time > start_time`; optional `cliff_time` must be within
  `[start_time, end_time]`.
- Custom: checkpoints must be sorted by time with non-decreasing cumulative
  amounts; last checkpoint must equal `total_amount`.
- All state-mutating functions require `require_initialized` before proceeding.

**Known limitations:**

- No event emission — unlike other contracts in this workspace, `token_vesting`
  does not publish Soroban events. This limits off-chain indexing and
  auditability. Recommended as a follow-up improvement.
- No cross-contract integration tests with `stello_pay_contract` yet.

### Bug Fixes

- **Linear + cliff gate** (issue #198): The `VestingKind::Linear` branch previously
  ignored `cliff_time`, allowing tokens to vest linearly before the cliff was
  reached. A cliff guard was added so that `compute_vested_amount` returns 0
  when `now < cliff_time`, matching the documented behavior for cliff schedules.

### Testing Focus

The test suite contains **42 tests** across 10 categories:

| Category | Count | What it covers |
|---|---|---|
| A. Initialization | 4 | `initialize` idempotency, pre-init guards, missing schedule, owner before init |
| B. Linear | 7 | Exact start/end boundaries, past-end cap, cliff gate (before/at/after), full claim flow |
| C. Cliff | 4 | 1 s before cliff (=0), exact cliff (=total), full claim, revoke-before-cliff refund |
| D. Custom | 4 | Before first checkpoint, between checkpoints, at final checkpoint, early release |
| E. Claim Security | 5 | Non-beneficiary rejected, double-claim fails, completed schedule rejected, released_amount accumulates, token balance verification |
| F. Revocation | 4 | Non-revocable rejected, non-employer rejected, double-revoke rejected, partial-vesting split (employer refund + beneficiary claim remainder) |
| G. Early Release | 3 | Non-owner rejected, amount capped at unvested, revoked schedule rejected |
| H. State Consistency | 2 | Claim after revoke gets frozen vested remainder, schedule IDs are sequential |
| I. Input Validation | 5 | Zero amount, end < start, cliff outside range, empty checkpoints, unsorted checkpoints |
| J. Edge Cases | 3 | Minimal-duration linear schedule, custom vested cap, invalid schedule_id |

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
| Before first checkpoint | Custom | 0 |
| Between checkpoints | Custom | last passed `cumulative_amount` |
| After revocation (`now > revoked_at`) | Any | vested amount frozen at `revoked_at` |

