# Continuous integration (contracts)

## Workflow

The GitHub Actions workflow **Contracts CI** (`.github/workflows/contracts.yml`) runs on pushes and pull requests targeting `main`.

It performs:

1. **Rust toolchain** — stable channel, `wasm32-unknown-unknown` target, `llvm-tools-preview` (for coverage instrumentation).
2. **Formatting gate** — `cargo fmt --all --check` from `onchain/`, using the repository-level `rustfmt.toml`.
3. **Stellar CLI** — `cargo install stellar-cli --locked` for `stellar contract build`.
4. **Unit / integration tests**
   - `cargo test -p payroll_escrow --verbose`
   - `cargo test -p stello_pay_contract --verbose`
   - `cargo test -p integration_tests --verbose`
   - `cargo test -p template_versioning --verbose`
5. **WASM build** — `stellar contract build` in `onchain/contracts/stello_pay_contract` and `onchain/contracts/template_versioning`.
6. **Coverage** — `cargo llvm-cov` over the same two packages; produces `onchain/codecov.json` and uploads it as a workflow artifact.

## Formatting and lint conventions

The repository root contains `rustfmt.toml` and `clippy.toml`. Keep these files
at the root so every contract crate in `onchain/` inherits the same formatting
and lint thresholds when commands run from the Cargo workspace.

- Run `cargo fmt --all --check` from `onchain/` before opening a PR.
- Run `cargo clippy --workspace --all-targets` from `onchain/` when changing Rust logic.
- Prefer updating the root config over adding per-crate formatting or lint exceptions.
- Document any future threshold changes here so reviewers know whether the change is
  a style policy update or a source-code fix.

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
cargo fmt --all --check
cargo test -p payroll_escrow --verbose
cargo test -p stello_pay_contract --verbose
cargo test -p integration_tests --verbose
cd contracts/stello_pay_contract && stellar contract build --verbose
cd ../.. && cargo llvm-cov test -p stello_pay_contract -p integration_tests --html
```

## Legacy workflow

`.github/workflows/ci.yml` is limited to **manual** runs (`workflow_dispatch`) so PRs are not duplicated. Its formatting job mirrors the automated **Contracts CI** formatting gate for maintainers who want an on-demand check.
