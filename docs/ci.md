# Continuous integration (contracts)

## Workflow

The GitHub Actions workflow **Contracts CI** (`.github/workflows/contracts.yml`) runs on pushes and pull requests targeting `main`.

It performs:

1. **Rust toolchain** — stable channel, `wasm32v1-none` target, `llvm-tools-preview` (for coverage instrumentation).
2. **Stellar CLI** — `cargo install stellar-cli --locked` for `stellar contract build`.
3. **Unit / integration tests**
   - `cargo test -p stello_pay_contract --verbose`
   - `cargo test -p integration_tests --verbose`
4. **WASM build** — `stellar contract build` in `onchain/contracts/stello_pay_contract`.
5. **Coverage** — `cargo llvm-cov` over the same two packages; produces `onchain/codecov.json` and uploads it as a workflow artifact.

### Optional Codecov

To publish reports to [Codecov](https://codecov.io), add a repository secret `CODECOV_TOKEN` and uncomment (or enable) the Codecov step in `contracts.yml`. The job is configured so missing token does not fail the workflow by default.

### Coverage thresholds

The workflow does **not** enforce a minimum coverage percentage by default (Soroban contract tests mix host and WASM targets; thresholds are easier to tune locally first). To fail CI below a threshold, add for example:

```bash
cargo llvm-cov test -p stello_pay_contract --fail-under-lines 95
```

after validating numbers in your environment.

## Local environment

Align with CI for reproducible runs:

| Requirement | Notes |
|-------------|--------|
| Rust | Stable, edition 2021 (see workspace `Cargo.toml`). |
| Target | `rustup target add wasm32v1-none` |
| Stellar CLI | Same major line as Soroban SDK in the workspace (e.g. install via `cargo install stellar-cli --locked`). |
| Coverage | `rustup component add llvm-tools-preview` and `cargo install cargo-llvm-cov` |

### Commands

```bash
cd onchain
cargo test -p stello_pay_contract --verbose
cargo test -p integration_tests --verbose
cd contracts/stello_pay_contract && stellar contract build --verbose
cd ../.. && cargo llvm-cov test -p stello_pay_contract -p integration_tests --html
```

## Legacy workflow

`.github/workflows/ci.yml` is limited to **manual** runs (`workflow_dispatch`) so PRs are not duplicated. Use **Contracts CI** for branch protection checks.
