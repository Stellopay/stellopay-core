# Continuous integration (contracts)

## Workflow

The GitHub Actions workflow **Contracts CI** (`.github/workflows/contracts.yml`) runs on pushes and pull requests targeting `main`.

It performs:

1. **Rust toolchain** — stable channel, `wasm32-unknown-unknown` target, `llvm-tools-preview` (for coverage instrumentation).
2. **Stellar CLI** — `cargo install stellar-cli --locked` for `stellar contract build`.
3. **Unit / integration tests**
   - `cargo test -p payroll_escrow --verbose`
   - `cargo test -p stello_pay_contract --verbose`
   - `cargo test -p integration_tests --verbose`
   - `cargo test -p template_versioning --verbose`
4. **WASM build** — `stellar contract build` in `onchain/contracts/stello_pay_contract` and `onchain/contracts/template_versioning`.
5. **Coverage** — `cargo llvm-cov` over the same two packages; produces `onchain/codecov.json` and uploads it as a workflow artifact.

## Run locally (matches Contracts CI)

The commands below mirror **exactly** what the **Contracts CI** job runs in
[`.github/workflows/contracts.yml`](.github/workflows/contracts.yml) on every push
and pull request targeting `main`. Run them from the `onchain` workspace root so the
working directory matches CI.

```bash
cd onchain

# 1. Formatting gate — must be clean (identical to CI)
cargo fmt --all -- --check

# 2. Build every contract crate in the workspace
cargo build --workspace --verbose

# 3. Run the full test suite across the workspace
cargo test --workspace --verbose
```

> **Keep this section in sync with `.github/workflows/contracts.yml`.**
> If the workflow changes its command sequence, update the three steps above to match.

The `wasm32-unknown-unknown` target and the `stellar` CLI are **not** part of the CI
gate above — they are only needed when you also want to produce a deployable WASM. For
those supplementary commands see the [Local environment](#local-environment) →
[Commands](#commands) section below.

### Optional Codecov

To publish reports to [Codecov](https://codecov.io), add a repository secret `CODECOV_TOKEN` and uncomment (or enable) the Codecov step in `contracts.yml`. The job is configured so missing token does not fail the workflow by default.

### Coverage thresholds

The workflow does **not** enforce a minimum coverage percentage by default (Soroban contract tests mix host and WASM targets; thresholds are easier to tune locally first). To fail CI below a threshold, add for example:

```bash
cargo llvm-cov test -p stello_pay_contract --fail-under-lines 95
```

after validating numbers in your environment.

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
