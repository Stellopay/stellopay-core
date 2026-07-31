# Continuous Integration

> **Canonical source:** `.github/workflows/contracts.yml`
>
> This document mirrors that workflow. If the workflow changes, update this
> file in the same pull request.

---

## Workflow: Contracts CI

**File:** `.github/workflows/contracts.yml`  
**Triggers:** push and pull_request to `main`

The workflow runs three jobs. The `lint` job checks formatting, builds the
workspace, and runs all workspace tests. The `coverage` job uses a build
matrix to generate and enforce line-coverage thresholds per contract crate.

### Job: `list-crates`

Discovers all contract crates via `cargo metadata` and exposes a JSON matrix
for the downstream `coverage` job.

### Job: `lint`

| Step | Command | Working directory |
|---|---|---|
| 1. Install Rust (stable + rustfmt) | _managed by `dtolnay/rust-toolchain@stable`_ | — |
| 2. Cache Cargo registry | _managed by `Swatinem/rust-cache@v2`_ | — |
| 3. Check formatting | `cargo fmt --all -- --check` | `onchain/` |
| 4. Build workspace | `cargo build --workspace --verbose` | `onchain/` |
| 5. Test workspace | `cargo test --workspace --verbose` | `onchain/` |

Steps 1-2 are handled automatically. Steps 3-5 are checks contributors must
pass.

### Job: `coverage` (matrix)

| Step | Command | Working directory |
|---|---|---|
| 1. Install Rust (stable + llvm-tools-preview) | _managed by `dtolnay/rust-toolchain@stable`_ | — |
| 2. Install `cargo-llvm-cov` | _managed by `taiki-e/install-action@v2`_ | — |
| 3. Cache Cargo registry | _managed by `Swatinem/rust-cache@v2`_ | — |
| 4. Generate coverage per crate | `cargo llvm-cov -p <crate> --json` | `onchain/` |
| 5. Enforce 95% gate | `.github/scripts/check_coverage.py --crate <name>` parses JSON and fails if below threshold | `onchain/` |

Each crate in `contracts/` runs in its own matrix entry. If any crate's line
coverage is below 95%, its matrix entry fails, which fails the overall job.

---

## Run Locally

Run the same checks CI executes, in the same order, before opening a PR.

### Prerequisites

| Requirement | How to install |
|---|---|
| Rust (stable) | `rustup install stable && rustup default stable` |
| `rustfmt` component | `rustup component add rustfmt` |
| `llvm-tools-preview` component | `rustup component add llvm-tools-preview` |
| `cargo-llvm-cov` | `cargo install cargo-llvm-cov` |

### Commands

```bash
cd onchain

# 1. Formatting - must produce no diff
cargo fmt --all -- --check

# 2. Build - all workspace crates must compile
cargo build --workspace --verbose

# 3. Tests - all workspace tests must pass
cargo test --workspace --verbose

# 4. Coverage gate per crate - repeat for each contract crate
cargo llvm-cov -p <crate> --json --output-path coverage.json
python3 ../.github/scripts/check_coverage.py coverage.json
```

To check all crates at once (faster locally), run workspace coverage and
validate with the same script:

```bash
cargo llvm-cov --workspace --exclude integration_tests --json --output-path coverage.json
python3 ../.github/scripts/check_coverage.py coverage.json
```

### Coverage gate

Each contract crate under `contracts/` must have at least **95% line
coverage**. CI enforces this via a build matrix: every crate gets its own
`cargo llvm-cov -p <crate>` invocation followed by
`.github/scripts/check_coverage.py --crate <name>`, which parses
`coverage.json` and exits non-zero if the crate's coverage falls below the
threshold. Pure-interface crates with no executable lines (e.g.
`rbac-interface`, `milestone-interface`) pass automatically.

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

**Coverage failure**

If the coverage gate fails, generate the report locally and check which
crates are below 95%:

```bash
cd onchain
cargo llvm-cov --workspace --exclude integration_tests --json --output-path coverage.json
python3 ../.github/scripts/check_coverage.py coverage.json
```

Add or improve tests for the failing crate(s) until the threshold is met.

---

## What CI does not check

The following are **not** part of the automated CI pipeline and are therefore
not required to pass before merging:

- `cargo clippy` - linting is not enforced by the workflow.
- WASM contract builds - `stellar contract build` is not run by CI.
- Per-package test runs beyond coverage - CI uses `--workspace` for testing;
  per-crate steps exist only for coverage measurement.
- `tools/doc_checker` - the documentation linter (undocumented public
  functions, undocumented error-enum variants, and orphaned `docs/*.md`
  files) is a standalone tool contributors can run manually; it is not
  wired into `.github/workflows/contracts.yml`. See
  [`tools/doc_checker/README.md`](../tools/doc_checker/README.md) for usage,
  including its `--strict` flag for promoting warnings to hard failures.

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
