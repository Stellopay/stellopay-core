# Continuous integration (contracts)

## Workflow

The GitHub Actions workflow **Contracts CI** (`.github/workflows/contracts.yml`) runs on pushes and pull requests targeting `main`.

It performs:

1. **Rust toolchain** — nightly channel, `wasm32-unknown-unknown` target, `rustfmt` and `llvm-tools-preview` (the latter is required by `cargo-llvm-cov` for coverage instrumentation).
2. **Stellar CLI** — `cargo install stellar-cli --locked` for `stellar contract build`.
3. **Unit / integration tests**
   - `cargo test -p payroll_escrow --verbose`
   - `cargo test -p stello_pay_contract --verbose`
   - `cargo test -p integration_tests --verbose`
   - `cargo test -p template_versioning --verbose`
4. **WASM build** — `stellar contract build` in `onchain/contracts/stello_pay_contract` and `onchain/contracts/template_versioning`.
5. **Coverage** — `cargo llvm-cov` over the same two packages; produces `onchain/codecov.json` and uploads it as a workflow artifact.

### Optional Codecov

To publish reports to [Codecov](https://codecov.io), add a repository secret `CODECOV_TOKEN` and uncomment (or enable) the Codecov step in `contracts.yml`. The job is configured so missing token does not fail the workflow by default.

### Coverage thresholds

**Contracts CI enforces a per-crate line-coverage gate of 95%.** The `Per-crate coverage gate` step instruments the whole workspace in a single `cargo llvm-cov` run (so the gate stays cheap) and then attributes the result back to each crate under `onchain/contracts/`. The job fails if **any** crate falls below the threshold — a workspace-wide average is deliberately *not* used, because a single uncovered crate can otherwise hide behind well-covered ones.

Only library sources count towards a crate's score: files under a crate's `tests/` directory, or under a `src/tests/` module, are the test code itself and are excluded so they cannot inflate a crate towards 100%.

Every run prints a per-crate table to the job log and to the job summary, sorted worst-first:

| crate | lines | covered | line % | status |
|---|---:|---:|---:|---|
| rate_limiter | 200 | 100 | 50.00 | **below 95%** |
| multisig | 400 | 400 | 100.00 | pass |

The threshold is the `COVERAGE_THRESHOLD` environment variable on that step in `.github/workflows/contracts.yml`; change it there to tune the gate.

#### Reproducing the gate locally

```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov

cd onchain
# the exact command the gate runs
cargo llvm-cov --workspace --json --output-path coverage.json

# the per-crate breakdown the gate evaluates (requires jq)
jq -r '
  [ .data[0].files[]
    | select(.filename | test("/contracts/[^/]+/src/"))
    | select(.filename | test("/src/tests?/") | not)
    | { crate:   (.filename | capture("/contracts/(?<c>[^/]+)/").c),
        count:   .summary.lines.count,
        covered: .summary.lines.covered } ]
  | group_by(.crate)
  | map({ crate: .[0].crate, count: (map(.count) | add), covered: (map(.covered) | add) })
  | map(. + { pct: (if .count == 0 then 100 else 100 * .covered / .count end) })
  | sort_by(.pct) | .[] | [.crate, .count, .covered, .pct] | @tsv
' coverage.json
```

To check a single crate quickly, `cargo-llvm-cov` can enforce the threshold directly:

```bash
cargo llvm-cov -p <crate> --fail-under-lines 95
```

### Disabled tests

Tests on `main` must be either active or deleted. Do not leave Rust test files
with a `.disabled` suffix or similar opt-out extension in contract test
directories. If a test breaks during SDK or API migration, either update it in
the same change, merge the still-useful cases into an active suite, or delete it
when active coverage already supersedes it.

## Local environment

Align with CI for reproducible runs:

| Requirement | Notes |
|-------------|--------|
| Rust | Stable, edition 2021 (see workspace `Cargo.toml`). |
| Target | `rustup target add wasm32-unknown-unknown` |
| Stellar CLI | Same major line as Soroban SDK in the workspace (e.g. install via `cargo install stellar-cli --locked`). |
| Coverage | `rustup component add llvm-tools-preview` and `cargo install cargo-llvm-cov` |

### Commands

```bash
cd onchain
cargo test -p payroll_escrow --verbose
cargo test -p stello_pay_contract --verbose
cargo test -p integration_tests --verbose
cd contracts/stello_pay_contract && stellar contract build --verbose
cd ../.. && cargo llvm-cov test -p stello_pay_contract -p integration_tests --html
```

## Legacy workflow

`.github/workflows/ci.yml` is limited to **manual** runs (`workflow_dispatch`) so PRs are not duplicated. Use **Contracts CI** for branch protection checks.
