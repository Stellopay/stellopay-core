## Snapshot (Golden) Testing for Contracts

This document describes the snapshot (golden) testing approach for the
`stello_pay_contract` and how to work with and update snapshots.

### Goals

- **Detect unintended behavior changes** in public functions by comparing
  stable, human-readable snapshots.
- **Document expected outputs** for key flows in a way that is easy to review.
- **Keep tests maintainable** by centralizing snapshot infrastructure in a
  single helper.

### Implementation Overview

- Contract: `onchain/contracts/stello_pay_contract/src/lib.rs`
- Snapshot tests: `onchain/contracts/stello_pay_contract/tests/snapshot/mod.rs`
- Snapshot files: `onchain/contracts/stello_pay_contract/tests/snapshot/__snapshots__/*.snap`

The snapshot test module provides a helper:

- `assert_snapshot(name: &str, content: &str)`
  - Writes a new snapshot file if none exists.
  - By default, compares `content` to the existing snapshot and fails the test
    if they differ.
  - Respects the `UPDATE_SNAPSHOTS=1` environment variable to overwrite
    existing snapshots.

Snapshots currently cover:

- **Agreement creation and getters**
  - Payroll and escrow agreement creation
  - `get_agreement` and `get_agreement_employees`
- **Payroll claiming and batch result shape**
  - `batch_claim_payroll` and per-employee claimed periods
- **Dispute lifecycle and FX helpers**
  - `set_arbiter`, `raise_dispute`, `resolve_dispute`
  - `set_exchange_rate_admin`, `set_exchange_rate`, `convert_currency`
- **Emergency pause configuration and state**
  - `set_emergency_guardians`, `propose_emergency_pause`,
    `approve_emergency_pause`, `is_emergency_paused`,
    `get_emergency_pause_state`

### Regression Scenarios (lifecycle stability)

The following scenarios were added to strengthen regression protection.
Each pins the ledger timestamp and captures only stable (non-timestamp)
fields so snapshots are fully deterministic across runs.

| Scenario | Snapshot name | What it covers |
|---|---|---|
| 1 | `payroll_lifecycle_created_funded_first_claim` | Created → add employee → activate → fund → claim first period; idempotency of same-period re-claim |
| 2 | `dispute_opened_escalation_resolution` | Cancel → raise dispute (mid-grace) → resolve; duplicate raise rejected; boundary: dispute after grace expires rejected |
| 3 | `emergency_pause_blocks_and_unblocks_operations` | Emergency pause blocks payroll + milestone claims; unpause restores both |
| 4 | `milestone_completion_all_claimed` | Claim while paused rejected; claim m1 → idempotency; claim m2 → auto-complete |
| 5 | `pause_resume_preserves_agreement_fields` | Pause → resume is a no-op on all stable fields |
| 6 | `repeated_transitions_rejected` | Double-activate, double-pause, double-resume, double-cancel all rejected |
| 7 | `escrow_lifecycle_created_funded_first_claim` | Escrow created → activate → first period claim → all periods → auto-complete |

### Security invariants asserted by snapshots

- **Blocked operations during pause**: Scenarios 3 and 4 assert that
  `claim_payroll` and `claim_milestone` both return errors while the
  contract is paused (emergency or agreement-level). The snapshot value
  `payroll_claim_blocked: true` / `milestone_claim_blocked: true` is
  committed to disk and will fail CI if the guard is ever removed.
- **Dispute idempotency**: Scenario 2 asserts `duplicate_raise_rejected: true`
  — a second `raise_dispute` on an already-disputed agreement must fail.
- **Grace boundary**: Scenario 2 asserts `dispute_outside_grace_rejected: true`
  — disputes raised after the grace window expires must be rejected.
- **Field preservation**: Scenario 5 asserts that pause → resume leaves all
  stable agreement fields unchanged (`stable_fields_preserved_across_pause_resume: true`).

Each snapshot test builds a representative state using existing public
functions, then serializes a `Debug` representation into a deterministic,
multi-line string for comparison.

### Running Snapshot Tests

From the `onchain` workspace:

```bash
cargo test -p stello_pay_contract --test snapshot
```

or to run the full test suite including snapshots:

```bash
cargo test -p stello_pay_contract
```

### Updating Snapshots

Snapshots must be committed to version control. To intentionally update them:

1. Make your code change.
2. Run tests with the update flag:

   ```bash
   UPDATE_SNAPSHOTS=1 cargo test -p stello_pay_contract --test snapshot
   ```

3. Inspect the changed `*.snap` files under
   `tests/snapshot/__snapshots__/` to confirm the new behavior is expected.
4. Commit the updated snapshots together with the code change.

If a snapshot test fails in CI:

- Re-run the snapshot test locally without `UPDATE_SNAPSHOTS` to confirm the
  mismatch.
- Decide whether the behavior change is intended:
  - **Intended change**: update snapshots as described above.
  - **Unintended change**: fix the code and re-run tests until snapshots pass
    without modification.

### Organization and Conventions

- Snapshot names follow a feature-oriented convention:
  - `agreement_creation_and_getters`
  - `payroll_claim_and_batch_result`
  - `dispute_and_fx_helpers`
  - `emergency_pause_state`
- Snapshot files use the `.snap` extension and live under
  `tests/snapshot/__snapshots__/`.
- Snapshots are plain text and should be readable and reviewable in code
  review; prefer concise, structured `Debug` output over verbose dumps.

