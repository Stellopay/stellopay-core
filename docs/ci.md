# Continuous Integration

> **Canonical source:** `.github/workflows/contracts.yml`
>
> This document mirrors that workflow. If the workflow changes, update this
> file in the same pull request.

---

## Workflow: Contracts CI

**File:** `.github/workflows/contracts.yml`  
**Triggers:** push and pull_request to `main`

The workflow runs two parallel/independent jobs on `ubuntu-latest`:

### Job: `contracts`
This job runs a smoke check on formatting, building, and testing the onchain contracts tree.

| # | Step | Command | Working directory |
|---|---|---|---|
| 1 | Install Rust (stable + rustfmt) | _managed by `dtolnay/rust-toolchain@stable`_ | — |
| 2 | Cache Cargo registry | _managed by `Swatinem/rust-cache@v2`_ | — |
| 3 | Check formatting | `cargo fmt --all -- --check` | `onchain/` |
| 4 | Build workspace | `cargo build --workspace --verbose` | `onchain/` |
| 5 | Test workspace | `cargo test --workspace --verbose` | `onchain/` |
| 6 | Install wasm32 target | `rustup target add wasm32-unknown-unknown` | — |
| 7 | Build contracts to WASM | `cargo build --workspace --release --target wasm32-unknown-unknown --verbose` | `onchain/` |
| 8 | Run WASM size regression check | `cargo run --release --manifest-path tools/wasm_size_check/Cargo.toml -- --baseline … --wasm-dir … --tolerance-pct 5 --fail-on-new --report …` | repo root |
| 9 | Upload size report artifact | _managed by `actions/upload-artifact@v4`_ | — |

Steps 1–2 are handled automatically by GitHub Actions and have no equivalent
local command. Steps 3–8 are the checks contributors must pass. Step 9 is a
diagnostic convenience — its presence is gated on `if: always()` so it is
preserved on failure for post-mortem download.

### Job: `doc-checker`
This job builds and runs `tools/doc_checker` against the full `docs/` and `onchain/contracts/` tree.
It runs with the `--strict` and `--events` flags to promote any documentation gaps into hard failures.

| Step | Command | Working directory |
|---|---|---|
| 1. Install Rust | _managed by `dtolnay/rust-toolchain@stable`_ | — |
| 2. Cache Cargo registry | _managed by `Swatinem/rust-cache@v2`_ | — |
| 3. Run doc_checker | `./tools/doc_checker/run_ci.py` | — |

---

## Run Locally

Run the same checks CI executes, in the same order, before opening a PR.

### Prerequisites

| Requirement | How to install |
|---|---|
| Rust (stable) | `rustup install stable && rustup default stable` |
| `rustfmt` component | `rustup component add rustfmt` |
| WASM target | `rustup target add wasm32-unknown-unknown` |

No Stellar CLI is required to run the WASM build because we delegate to
`cargo build --target wasm32-unknown-unknown` directly. This is the only
target the Soroban host accepts; see `docs/build-targets.md` for the
rationale.

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

# 4. WASM build (step 7 in CI)
cargo build --workspace --release --target wasm32-unknown-unknown
```

And then from the repository root:

```bash
# 5. WASM size regression check (step 8 in CI)
cargo run --release --manifest-path tools/wasm_size_check/Cargo.toml -- \
    --baseline  benchmarks/wasm_sizes.json \
    --wasm-dir  onchain/target/wasm32-unknown-unknown/release \
    --tolerance-pct 5 \
    --fail-on-new
```

All five commands must exit with code `0` for a PR to be mergeable.

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
`edition = "2021"` and the stable Rust channel; ensure your toolchain is
up to date:

```bash
rustup update stable
```

**Test failure**

Test output is printed with `--verbose`. Read the failure message and fix
the broken test or the code under test.

**WASM size regression failure**

The `wasm_size_check` step exits non-zero when any contract's compiled
size grew beyond the configured tolerance without a corresponding
baseline refresh. See **WASM Size Budget Policy** below for the policy
and update procedure.

---

## WASM Size Budget Policy

> Source of truth: `benchmarks/wasm_sizes.json` (committed). The
> `wasm_size_check` binary is a pure checker; it does not invoke
> `cargo build`.

The Soroban host enforces a hard upper bound on contract bytecode size at
deployment time. An unnoticed size regression can push a contract closer
to (or past) that limit and only surface as a deployment failure —
potentially on `mainnet`. CI must therefore catch regressions before they
merge.

### Policy

1. CI builds every contract in the `onchain` workspace to
   `wasm32-unknown-unknown` in release mode (step 7 above).
2. The committed `benchmarks/wasm_sizes.json` file records the size,
   SHA-256 (`sha256:<hex>`), and capture date for every successfully
   built `.wasm`.
3. After the build, CI invokes `wasm_size_check` (step 8 above) and
   compares observed sizes against the baseline.
4. The job fails (`exit 1`) if **any** contract:
   - Grows by more than the configured tolerance (currently **5 %** of
     the baseline size, computed as `delta_bytes / baseline_bytes`,
     strictly greater than the threshold), **without a refresh of its
     `benchmarks/wasm_sizes.json` entry in the same PR**.
   - Has the same size as its baseline but a different SHA-256 — a
     strong signal the baseline entry was copy/pasted from a stale run.
   - Has no entry in the baseline (a brand-new contract that has not
     been bootstrapped yet), gated by `--fail-on-new`.
   - Has a baseline entry but no `.wasm` on disk — the contract was
     removed without pruning the baseline (override with
     `--allow-missing` only for temporary experiments).
5. The job **passes** for any contract that:
   - Exactly equals its baseline.
   - Grew but stays within the tolerance.
   - **Shrank** — shrinking is always a pass but reported in the table
     so reviewers are aware code was removed.

### Updating the baseline

When a PR legitimately changes a contract's compiled size, refresh the
baseline and commit the result **in the same PR**:

```bash
# 1. Build to wasm32 as usual.
cargo build --workspace --release --target wasm32-unknown-unknown

# 2. Refresh the committed baseline.
cargo run --release --manifest-path tools/wasm_size_check/Cargo.toml -- \
    --baseline  benchmarks/wasm_sizes.json \
    --wasm-dir  onchain/target/wasm32-unknown-unknown/release \
    --update-baseline

# 3. Verify the change is intentional.
git diff benchmarks/wasm_sizes.json

# 4. Commit + push (in the same PR as the source change).
git add benchmarks/wasm_sizes.json
git commit -m "chore(wasm-size): refresh baseline for <list-of-changed-contracts>"
git push
```

A PR that introduces a regression **without** a matching baseline
refresh will fail CI at step 8 with a clear table showing which
contract(s) regressed, by how many bytes, and the percent delta.

### Bootstrap

The baseline file `benchmarks/wasm_sizes.json` is committed and may be
empty on first merge of this feature. After the first PR lands, follow
the update procedure above to populate it. A PR that adds a brand-new
contract crate must include a baseline entry for it (otherwise
`--fail-on-new` will trip CI).

### Tolerance tuning

The 5 % tolerance is a starting point chosen to allow genuine
algorithmic improvements without forcing a baseline refresh on every
minor change. Bumping the tolerance is a policy change and must:

1. Be justified in the PR description with a size delta report.
2. Be reviewed by a maintainer who understands the Soroban size budget.
3. Update this document in the same PR.

The checker accepts a `--tolerance-pct` value at the command line so
individual PRs can override the default without modifying the workflow
file (use sparingly; prefer updating the central default).

### Tool reference

See `tools/wasm_size_check/README.md` for the full set of flags:

| Flag | Default | Effect |
|---|---|---|
| `--tolerance-pct <n>` | `5` | Maximum allowed percent growth |
| `--update-baseline` | off | Refresh the baseline with current measurements |
| `--fail-on-new` | off | Fail when a `.wasm` has no baseline entry |
| `--allow-missing` | off | Skip baseline entries with no `.wasm` (instead of failing) |
| `--report <path>` | stdout | Also write the Markdown report to this path |

### Markdown report artifact

Step 8 also writes `artifacts/wasm_size_report.md` (and uploads it as
the `wasm-size-report` artifact on every run, including failed ones).
This is intended for post-mortem inspection when CI fails — open the
artifact in the GitHub Actions UI to see the full regression table.

---

## Workflow: Scheduled Semver Checks

**File:** `.github/workflows/security-scan.yml`  
**Triggers:** schedule (weekly, Monday 06:00 UTC) and `workflow_dispatch`

The workflow runs a single job (`semver-checks`) on `ubuntu-latest` that
installs `cargo-semver-checks` and runs `check-release` against every
contract crate under `onchain/contracts/`.

Each crate is compared against its last tagged release (e.g.
`stello_pay_contract-v0.1.0`).  If no tag exists for the current
`Cargo.toml` version, the crate is skipped (first release).

| Step | Command / Action |
|---|---|
| 1. Checkout full history | `actions/checkout@v7` with `fetch-depth: 0` |
| 2. Install Rust stable | `dtolnay/rust-toolchain@stable` |
| 3. Cache Cargo artifacts | `Swatinem/rust-cache@v2` |
| 4. Install `cargo-semver-checks` | `taiki-e/install-action@v2` |
| 5. Semver check per crate | `cargo semver-checks check-release -p <crate> --baseline-rev <tag>` |

### Breaking change policy

Any of the following is a **breaking change** and must be accompanied by a
version bump in `Cargo.toml`:

- Removing or renaming a `#[contractimpl]` method.
- Adding, removing, or reordering parameters.
- Changing a parameter or return type.
- Removing or renaming a public struct, enum, or variant.
- Narrowing the visibility of a public item.

Additive changes (new methods, new types) are allowed without a version bump.

### Run locally

Prerequisites:

```bash
cargo install cargo-semver-checks
```

Check a single crate against its last tagged release:

```bash
cd onchain
cargo semver-checks check-release -p stello_pay_contract \
    --baseline-rev stello_pay_contract-v0.0.0
```

Compare against the previous commit (useful during development):

```bash
cargo semver-checks check-release -p stello_pay_contract \
    --baseline-rev HEAD~1
```

### Tagging a release

After bumping a crate's version in `Cargo.toml`, create a matching tag so
the scheduled workflow can use it as a baseline:

```bash
git tag stello_pay_contract-v0.1.0
git push origin stello_pay_contract-v0.1.0
```

Tag format: `<crate_name>-v<semver>` (e.g. `rbac-v0.1.0`,
`compliance_checker-v0.1.0`).

---

## What CI does not check

The following are **not** part of the automated CI pipeline and are therefore
not required to pass before merging:

- `cargo clippy` — linting is not enforced by the workflow.
- Coverage reporting — no `cargo llvm-cov` step exists in the current workflow.
- `stellar contract build` — CI uses raw `cargo build --target wasm32-unknown-unknown`.
  `stellar contract build` is functionally equivalent but is not a dependency of CI.
- Per-package test runs — CI uses `--workspace`; there are no per-crate steps.

> If any of the above are added to `.github/workflows/contracts.yml` in the
> future, this section and the **Run locally** section above must both be
> updated.

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
