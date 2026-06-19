# Continuous integration (contracts)

## Workflow

The GitHub Actions workflow **Contracts CI** (`.github/workflows/contracts.yml`) runs on pushes and pull requests targeting `main`.

It performs:

1. **Rust toolchain** — stable channel, `wasm32-unknown-unknown` target, `llvm-tools-preview` (for coverage instrumentation).
2. **Stellar CLI** — `cargo install stellar-cli --locked` for `stellar contract build`.
3. **Unit / integration tests**
   - `cargo test -p stello_pay_contract --verbose`
   - `cargo test -p integration_tests --verbose`
   - `cargo test -p template_versioning --verbose`
4. **WASM build** — `stellar contract build` in `onchain/contracts/stello_pay_contract` and `onchain/contracts/template_versioning`.
5. **Coverage** — `cargo llvm-cov` over the same two packages; produces `onchain/codecov.json` and uploads it as a workflow artifact.

## Rust cache

`contracts.yml` and `security-scan.yml` cache Cargo downloads and the
`onchain/target` build directory with `actions/cache@v4`. The cache paths are
limited to:

- `~/.cargo/registry`
- `~/.cargo/git`
- `onchain/target`

The cache key includes the runner OS, workflow purpose, the Rust toolchain
marker (`stable`), and a hash of `onchain/Cargo.lock` plus any
`rust-toolchain`/`rust-toolchain.toml` files. Changing dependencies, pinning a
different toolchain file, or moving a workflow to a different toolchain marker
creates a new cache entry instead of reusing old build artifacts.

The cache intentionally excludes `~/.cargo/bin`, so tools such as
`stellar-cli`, `cargo-audit`, and `cargo-llvm-cov` are installed or provisioned
by their workflow steps instead of restored from cache. GitHub Actions also
scopes caches by branch and pull request merge ref; untrusted fork PRs can read
eligible base-branch caches but cannot replace the trusted `main` cache used by
pushes to `main`. The workflows use `pull_request`, not `pull_request_target`,
so forked PRs do not receive repository secrets.

The legacy manual `.github/workflows/ci.yml` workflow does not run Cargo and
therefore has no Rust cache step.

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
cargo test -p stello_pay_contract --verbose
cargo test -p integration_tests --verbose
cd contracts/stello_pay_contract && stellar contract build --verbose
cd ../.. && cargo llvm-cov test -p stello_pay_contract -p integration_tests --html
```

## Legacy workflow

`.github/workflows/ci.yml` is limited to **manual** runs (`workflow_dispatch`) so PRs are not duplicated. Use **Contracts CI** for branch protection checks.
