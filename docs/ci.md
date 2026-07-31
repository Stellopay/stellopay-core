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
|---|---|---|---|
| 1. Install Rust (stable + rustfmt + llvm-tools-preview) | _managed by `dtolnay/rust-toolchain@stable`_ | — |
| 2. Install `cargo-llvm-cov` | _managed by `taiki-e/install-action@v2`_ | — |
| 3. Cache Cargo registry | _managed by `Swatinem/rust-cache@v2`_ | — |
| 4. Check formatting | `cargo fmt --all -- --check` | `onchain/` |
| 5. Build workspace | `cargo build --workspace --verbose` | `onchain/` |
| 6. Test workspace | `cargo test --workspace --verbose` | `onchain/` |
| 7. Generate coverage | `cargo llvm-cov --workspace --exclude integration_tests --json` | `onchain/` |
| 8. Enforce coverage gate | Python script checks each contract crate ≥ 95 % line coverage | `onchain/` |

Steps 1–3 are handled automatically by GitHub Actions and have no equivalent
local command. Steps 4–8 are the checks contributors must pass.

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

# 1. Formatting — must produce no diff
cargo fmt --all -- --check

# 2. Build — all workspace crates must compile
cargo build --workspace --verbose

# 3. Tests — all workspace tests must pass
cargo test --workspace --verbose

# 4. Coverage — generate JSON report for all contract crates
cargo llvm-cov --workspace --exclude integration_tests --json --output-path coverage.json
```

### Coverage gate

After generating coverage data, each contract crate under `contracts/` must
have at least **95 % line coverage**. CI enforces this automatically by
parsing `coverage.json` and failing the build if any crate falls below the
threshold.

To check coverage locally after step 4:

```bash
python3 -c "
import json, re

with open('onchain/coverage.json') as f:
    data = json.load(f)

crates = {}
for file_data in data['data'][0]['files']:
    filename = file_data.get('filename', '')
    m = re.search(r'/contracts/([^/]+)/', filename.replace('\\\\', '/'))
    if not m:
        continue
    crate = m.group(1)
    lines = file_data.get('summary', {}).get('lines', {})
    total = lines.get('count', 0)
    covered = lines.get('covered', 0)
    if total == 0:
        continue
    if crate not in crates:
        crates[crate] = {'lines': 0, 'covered': 0}
    crates[crate]['lines'] += total
    crates[crate]['covered'] += covered

ok = True
for crate in sorted(crates):
    info = crates[crate]
    pct = (info['covered'] / info['lines']) * 100
    status = 'PASS' if pct >= 95 else 'FAIL'
    print(f'  {status}: {crate}: {pct:.2f}%')
    if pct < 95:
        ok = False

if not ok:
    exit(1)
"
```

**Note:** Running `cargo llvm-cov` gathers coverage for *every* test in the
workspace (excluding `integration_tests`), so you only need the single
`--workspace` invocation rather than per-crate runs.

For CLI-specific regression coverage around the verify subcommand, also run:

```bash
cargo test --manifest-path tools/cli/Cargo.toml
```

This explicitly exercises the tampered-WASM and matching-WASM verification paths so the CLI remains secure even when a rebuilt artifact is mutated by a single byte.

All commands above must exit with code `0` for a PR to be mergeable.

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
- WASM contract builds — `stellar contract build` is not run by CI.
- Per-package test runs — CI uses `--workspace`; there are no per-crate steps.
- `tools/doc_checker` — the documentation linter (undocumented public
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
