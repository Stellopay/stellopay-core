# Contributing to stellopay-core

Thank you for contributing to stellopay-core — a decentralized payroll system built on the Stellar blockchain using Soroban.

Read [SECURITY.md](SECURITY.md) before reporting vulnerabilities. Use private [GitHub security advisories](https://github.com/Stellopay/stellopay-core/security/advisories/new) instead of public issues when disclosure could harm users.

---

## Repository layout

| Path | Purpose |
| --- | --- |
| `onchain/` | Rust workspace root (`Cargo.toml`) for Soroban contracts and integration tests |
| `onchain/contracts/` | Individual contract crates (payroll, escrow, governance, compliance, etc.) |
| `onchain/integration_tests/` | Cross-contract workflow tests |
| `tools/cli/` | `stellopay-cli` for deploy, query, and operational commands |
| `tools/doc_checker/` | Scans contract impls for missing documentation |
| `docs/` | Architecture, API notes, CI, deployment, and migration guides |
| `scripts/` | Migration and helper scripts |

Entry points:

- [README.md](README.md) — repository overview
- [onchain/README.md](onchain/README.md) — contract inventory, build, test, and coverage
- [docs/README.md](docs/README.md) — product and integration documentation

---

## Local setup

Align with [docs/ci.md](docs/ci.md) and `.github/workflows/contracts.yml`:

```sh
rustup install stable
rustup target add wasm32-unknown-unknown
rustup component add llvm-tools-preview
cargo install stellar-cli --locked
```

Optional (coverage locally):

```sh
cargo install cargo-llvm-cov
```

---

## Build and test workflow

All contract work happens from the `onchain/` workspace.

### Formatting

```sh
cd onchain
cargo fmt --all
cargo fmt --all -- --check
```

### Run tests

```sh
cd onchain
cargo test -p stello_pay_contract --verbose
cargo test -p integration_tests --verbose
cargo test -p template_versioning --verbose
```

Or the full workspace:

```sh
cd onchain
cargo test --workspace
```

### Build WASM (Soroban)

Preferred (matches CI):

```sh
cd onchain/contracts/stello_pay_contract
stellar contract build --verbose
```

Windows / GNU toolchain fallback:

```sh
rustup target add wasm32-unknown-unknown
cd onchain/contracts/stello_pay_contract
cargo build -p stello_pay_contract --target wasm32-unknown-unknown --release
```

See [docs/windows-build.md](docs/windows-build.md) for MSVC and WSL options.

### CLI tooling

```sh
cd tools/cli
cargo test
cargo build --release
```

### Documentation checker

```sh
cd tools/doc_checker
cargo run
```

---

## Branch naming

| Prefix | Use for |
|--------|---------|
| `feat/` | New features |
| `fix/` | Bug fixes |
| `chore/` | Tooling, config, dependency updates |
| `docs/` | Documentation only |
| `devops/` | CI/CD and infrastructure |

Example: `docs/project-health-files`, `feat/bulk-payment-v2`

---

## Pull request expectations

1. Fork from `main`, keep your branch up to date, and open a PR against `Stellopay/stellopay-core`.
2. Fill in [.github/pull_request_template.md](.github/pull_request_template.md).
3. Run the same checks CI runs (format, tests, WASM build for touched contracts).
4. Add or update tests for behaviour changes. Contract suites live beside each crate under `onchain/contracts/*/tests/` and in `onchain/integration_tests/`.
5. Update relevant docs under `docs/` when behaviour, deployment, or operator workflows change.
6. Do not commit secrets, private keys, or production credentials.

### PR checklist

- [ ] Branch is up to date with `main`
- [ ] `cargo fmt --all -- --check` passes in `onchain/`
- [ ] `cargo test` passes for affected packages
- [ ] `stellar contract build` succeeds for modified contracts
- [ ] Docs updated when needed
- [ ] PR description explains what changed and why

---

## Dependency updates

stellopay-core uses **GitHub Dependabot** for Rust crates and GitHub Actions. See [docs/dependency-update-policy.md](docs/dependency-update-policy.md) for grouping rules and review expectations.

---

## Code review

Changes under `onchain/contracts/` and `tools/` are routed to maintainers via [.github/CODEOWNERS](.github/CODEOWNERS).

---

## Getting help

- [Documentation index](docs/README.md)
- [Discord](https://discord.gg/stellopay) for community questions (not for security vulnerabilities)
