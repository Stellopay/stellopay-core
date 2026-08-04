# Continuous Integration

> **Canonical source:** `.github/workflows/contracts.yml`
>
> This document mirrors that workflow. If the workflow changes, update this
> file in the same pull request.

---

## Workflow: Contracts CI

**File:** `.github/workflows/contracts.yml`  
**Triggers:** push and pull_request to `main`

The workflow runs the following jobs on `ubuntu-latest`:

### Job: `contracts`

Smoke check for formatting, building, and testing the onchain contracts tree.

| # | Step | Command | Working directory |
|---|---|---|---|
| 1 | Install Rust (nightly + rustfmt) | _managed by `dtolnay/rust-toolchain@nightly`_ | — |
| 2 | Cache Cargo registry | _managed by `Swatinem/rust-cache@v2`_ | — |
| 3 | Check formatting | `cargo fmt --all -- --check` | `onchain/` |
| 4 | Build workspace | `cargo build --workspace --verbose` | `onchain/` |
| 5 | Test workspace | `cargo test --workspace --verbose` | `onchain/` |

### Job: `coverage-list`

Discovers the contract package names and exposes them as the coverage matrix.
This is kept dynamic so a newly added contract crate is measured and gated
without editing the workflow file.

| Step | Command | Working directory |
|---|---|---|
| 1. Install Rust (nightly + llvm-tools-preview) | _managed by `dtolnay/rust-toolchain@nightly`_ | — |
| 2. Cache Cargo registry | _managed by `Swatinem/rust-cache@v2`_ | — |
| 3. Compute contract matrix | `cargo metadata --format-version 1 --no-deps` + `jq` filter for `*/contracts/*` packages | `onchain/` |

### Job: `coverage` (matrix)

One job per contract crate. Each leg measures line coverage for its crate and
**fails if that crate is below 95%**. The JSON report is uploaded so the
aggregate gate can verify no crate was skipped.

| Step | Command / Action | Working directory |
|---|---|---|
| 1. Install Rust (nightly + llvm-tools-preview) | _managed by `dtolnay/rust-toolchain@nightly`_ | — |
| 2. Cache Cargo registry | _managed by `Swatinem/rust-cache@v2`_ | — |
| 3. Install `cargo-llvm-cov` | `taiki-e/install-action@cargo-llvm-cov` | — |
| 4. Measure crate coverage | `cargo llvm-cov -p <pkg> --json --output-path /tmp/coverage-<pkg>.json` | `onchain/` |
| 5. Enforce threshold | `python3 tools/coverage_check/check_coverage.py --report /tmp/coverage-<pkg>.json --workspace onchain --min-line-pct 95 --crate <pkg>` | repo root |
| 6. Upload report | _managed by `actions/upload-artifact@v4`_ (`coverage-report-<pkg>`) | — |

### Job: `coverage-summary`

Runs after every per-crate leg completes. Verifies that **every** contract
crate in `onchain/contracts/` produced a report (a silently skipped crate
fails the gate) and prints the full per-crate table to the job summary.

| Step | Command / Action |
|---|---|
| 1. Download all per-crate reports | `actions/download-artifact@v4` with `pattern: coverage-report-*`, `merge-multiple: true` |
| 2. Aggregate gate | `python3 tools/coverage_check/check_coverage.py --report coverage-reports --workspace onchain --min-line-pct 95` |
| 3. Upload aggregate report | _managed by `actions/upload-artifact@v4`_ (`coverage-reports-aggregate`) |

---

## Coverage Gate Policy

> Source of truth: `tools/coverage_check/check_coverage.py` (committed). The
> checker is a pure gate: it parses `cargo llvm-cov --json` output and does
> not invoke cargo itself. Measurement is done by the workflow; the gate only
> filters, aggregates, and evaluates.

Every contract crate under `onchain/contracts/` must hold **at least 95% line
coverage** of its `src/` files. Coverage of test files and dependencies does
not count — tests drive the measurement but are not the metric.

### Why line coverage?

`docs/ci.md` previously noted that "no `cargo llvm-cov` step exists in the
current workflow." This gate closes that gap. Line coverage is a coarse but
reliable first gate: it catches wholesale untested code paths (new entrypoints,
new branches, new error paths) before they merge. It is intentionally not a
substitute for the invariant-level tests in the contract crates — it enforces
that those tests keep being written and kept honest.

### Policy

1. The `coverage` job measures each contract crate independently with
   `cargo llvm-cov -p <pkg> --json`. The report is filtered to files under
   `contracts/<dir>/src/` — a crate is only as good as the source it ships.
2. The gate passes a crate when its line coverage is **`>= 95.00%`** (a crate
   at exactly `95.00%` passes).
3. The gate **fails** when any crate:
   - Drops below `95.00%` line coverage (the failure lists the offending
     source files and their per-file coverage so a developer can see exactly
     what is uncovered).
   - Has **no measurable lines** in `src/` (a crate that cannot be measured).
   - Is **absent from the reports entirely** — enforced by the aggregate
     `coverage-summary` job, which requires every discovered crate to be
     present so a crate accidentally omitted from the matrix cannot pass
     silently.
4. Prefix package names never collide: `rbac` and `rbac-interface` are
   attributed by their directory + `/src/` marker, not by name prefix.
5. A new contract crate is automatically added to the matrix by
   `coverage-list` and therefore must ship with tests that keep it above the
   threshold on day one.

### Running the gate locally

Install `cargo-llvm-cov` once:

```bash
cargo +nightly install cargo-llvm-cov --locked
```

Measure the whole workspace and run the gate:

```bash
cd onchain
cargo llvm-cov --workspace --json --output-path /tmp/coverage.json
cd ..
python3 tools/coverage_check/check_coverage.py \
  --report /tmp/coverage.json \
  --workspace onchain
```

Gate a single crate (mirrors one CI matrix leg):

```bash
cd onchain
cargo llvm-cov -p rbac-interface --json --output-path /tmp/rbac-interface.json
cd ..
python3 tools/coverage_check/check_coverage.py \
  --report /tmp/rbac-interface.json \
  --workspace onchain \
  --min-line-pct 95 \
  --crate rbac-interface
```

### Tool reference

See `tools/coverage_check/README.md` for the full flag list:

| Flag | Default | Effect |
|---|---|---|
| `--report <path>` | _required_ | A single `cargo llvm-cov --json` file, or a directory of them |
| `--workspace <path>` | `onchain` | Cargo workspace root **containing** the `contracts/` directory |
| `--min-line-pct <n>` | `95` | Minimum line-coverage percentage per crate |
| `--crate <package>` | all | Evaluate only this one package (used by matrix legs) |

Exit code is `0` if every checked crate passes, `1` if any crate is below the
threshold / unmeasurable / absent from the reports, and `2` on a usage error.
When `GITHUB_STEP_SUMMARY` is set, the Markdown summary is also appended to
that file for the Actions job summary.

### Tests

```bash
python3 -m unittest discover -s tools/coverage_check/tests -v
```

### Threshold tuning

Bumping the threshold is a policy change and must:

1. Be justified in the PR description.
2. Update `--min-line-pct` in **both** the per-crate and aggregate steps of
   `.github/workflows/contracts.yml` in the same PR.
3. Update this document in the same PR.

The `onchain/integration_tests/tests/test_workflows.rs` `ci_coverage_gate_guard`
module asserts the workflow still enforces the gate (measurement, `--min-line-pct 95`,
per-crate matrix, aggregate check) and that the checker tooling remains in the
tree — editing the workflow in a way that silently weakens the gate fails CI.

---

## Run Locally

Run the same checks CI executes, in the same order, before opening a PR.

### Prerequisites

| Requirement | How to install |
|---|---|
| Rust (nightly) | `rustup install nightly` (the workspace pins `nightly` via `onchain/rust-toolchain.toml`) |
| `rustfmt` component | `rustup component add rustfmt --toolchain nightly` |
| `cargo-llvm-cov` | `cargo +nightly install cargo-llvm-cov --locked` |
| Python 3 | Used to run `tools/coverage_check/check_coverage.py` and its tests |

### Commands

**1. Contract checks (formatting, build, test)**

```bash
cd onchain

# Formatting — must produce no diff
cargo fmt --all -- --check

# Build — all workspace crates must compile
cargo build --workspace --verbose

# Tests — all workspace tests must pass
cargo test --workspace --verbose
```

**2. Coverage gate**

```bash
# From the repository root
python3 tools/coverage_check/check_coverage.py --help
```

Then follow the local coverage commands under **Coverage Gate Policy** above.
All commands must exit with code `0` for a PR to be mergeable.

### Fixing common failures

**Formatting failure**

`cargo fmt --all -- --check` exits non-zero when any file would be
reformatted. Fix by running the formatter without `--check`:

```bash
cd onchain
cargo fmt --all
```

Then commit the result before pushing.

**Build failure**

Resolve compiler errors reported by `cargo build`. The workspace uses
`edition = "2021"`; ensure your toolchain is up to date:

```bash
rustup update nightly
```

**Test failure**

Test output is printed with `--verbose`. Read the failure message and fix
the broken test or the code under test.

**Coverage gate failure**

The gate fails when a contract crate's line coverage on `src/` drops below
`95%`, or when a crate is missing from the reports. Run the local coverage
commands to reproduce the failure and look at the per-file breakdown in the
gate output to find the uncovered source. Write the missing tests, then
re-run. A crate with no tests at all must gain tests before it can merge.

---

## Documented checks not yet wired into a workflow

The following checks are **documented but are not run by any workflow today**.
Tools and committed baselines exist; wiring them into CI is tracked separately.
Treat the sections below as the policy to enforce once the corresponding steps
are added back to `.github/workflows/contracts.yml`.

### WASM Size Budget Policy

> Source of truth: `benchmarks/wasm_sizes.json` (committed). The
> `tools/wasm_size_check` binary is a pure checker; it does not invoke
> `cargo build`.

The Soroban host enforces a hard upper bound on contract bytecode size at
deployment time. An unnoticed size regression can push a contract closer
to (or past) that limit and only surface as a deployment failure —
potentially on `mainnet`. A future CI step should therefore catch
regressions before they merge.

1. Build every contract in the `onchain` workspace to
   `wasm32-unknown-unknown` in release mode.
2. The committed `benchmarks/wasm_sizes.json` file records the size,
   SHA-256 (`sha256:<hex>`), and capture date for every successfully
   built `.wasm`.
3. After the build, invoke `wasm_size_check` and compare observed sizes
   against the baseline.
4. The job should fail (`exit 1`) if **any** contract:
   - Grows by more than the configured tolerance (currently **5 %** of
     the baseline size), **without a refresh of its
     `benchmarks/wasm_sizes.json` entry in the same PR**.
   - Has the same size as its baseline but a different SHA-256.
   - Has no entry in the baseline (gated by `--fail-on-new`).
   - Has a baseline entry but no `.wasm` on disk (override with
     `--allow-missing` only for temporary experiments).
5. The job should **pass** for any contract that is exactly at its
   baseline, grew within tolerance, or **shrank**.

Updating the baseline:

```bash
cargo build --workspace --release --target wasm32-unknown-unknown
cargo run --release --manifest-path tools/wasm_size_check/Cargo.toml -- \
    --baseline  benchmarks/wasm_sizes.json \
    --wasm-dir  onchain/target/wasm32-unknown-unknown/release \
    --update-baseline
git diff benchmarks/wasm_sizes.json
```

Commit the refreshed baseline in the **same PR** as the source change.

### `doc-checker`

`tools/doc_checker` checks documentation coverage (with `--strict` and
`--events` flags) across `docs/` and `onchain/contracts/`. There is currently
no `doc-checker` job in `.github/workflows/contracts.yml`; it should be run as
`./tools/doc_checker/run_ci.py` when wired back in.

### Scheduled Semver Checks

`cargo-semver-checks check-release` compares each contract crate against its
last tagged release (tag format `<crate_name>-v<semver>`, e.g.
`rbac-v0.1.0`). A scheduled `security-scan.yml` workflow is planned but not
committed. Until then, check locally:

```bash
cd onchain
cargo semver-checks check-release -p stello_pay_contract \
    --baseline-rev stello_pay_contract-v0.0.0
```

---

## auto-assign workflow

**File:** `.github/workflows/auto-assign.yml`  
**Triggers:** `issue_comment` (created)

This workflow automatically assigns an issue to a contributor when they
comment with an assignment phrase (e.g. `/assign`, `I'd like to work on this`).
It is a repository-management workflow only and does **not** perform any code
quality checks. Contributors do not need to run anything locally to satisfy it.

---

## Disabled tests policy

Tests on `main` must be either active or deleted. Do not leave Rust test files
with a `.disabled` suffix or similar opt-out extension in contract test
directories. If a test breaks during SDK or API migration, either update it in
the same change or delete it when active coverage already supersedes it.
