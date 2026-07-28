# Continuous Integration

> **Canonical source:** `.github/workflows/contracts.yml`
>
> This document mirrors that workflow. If the workflow changes, update this
> file in the same pull request.

---

## Workflow: Contracts CI

**File:** `.github/workflows/contracts.yml`  
**Triggers:** push and pull_request to `main`

The workflow runs a single job (`contracts`) on `ubuntu-latest` with the
following steps, in order:

| Step | Command | Working directory |
|---|---|---|
| 1. Install Rust (stable + rustfmt) | _managed by `dtolnay/rust-toolchain@stable`_ | — |
| 2. Cache Cargo registry | _managed by `Swatinem/rust-cache@v2`_ | — |
| 3. Check formatting | `cargo fmt --all -- --check` | `onchain/` |
| 4. Build workspace | `cargo build --workspace --verbose` | `onchain/` |
| 5. Test workspace | `cargo test --workspace --verbose` | `onchain/` |

Steps 1–2 are handled automatically by GitHub Actions and have no equivalent
local command. Steps 3–5 are the checks contributors must pass.

---

## Run Locally

Run the same three checks CI executes, in the same order, before opening a PR.

### Prerequisites

| Requirement | How to install |
|---|---|
| Rust (stable) | `rustup install stable && rustup default stable` |
| `rustfmt` component | `rustup component add rustfmt` |

No Stellar CLI, `llvm-tools-preview`, or `cargo-llvm-cov` is required to
pass the CI checks listed above.

### Commands

```bash
cd onchain

# 1. Formatting — must produce no diff
cargo fmt --all -- --check

# 2. Build — all workspace crates must compile
cargo build --workspace --verbose

# 3. Tests — all workspace tests must pass
cargo test --workspace --verbose
```

All three commands must exit with code `0` for a PR to be mergeable.

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

---

## What CI does not check

The following are **not** part of the automated CI pipeline and are therefore
not required to pass before merging:

- `cargo clippy` — linting is not enforced by the workflow.
- Coverage reporting — no `cargo llvm-cov` step exists in the current workflow.
- WASM contract builds — `stellar contract build` is not run by CI.
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
