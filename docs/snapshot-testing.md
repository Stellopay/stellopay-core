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

