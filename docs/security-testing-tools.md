# Security Testing Tools Integration

This document describes the security testing tooling integrated for the Rust/Soroban workspace.

## Overview

The security tooling integration provides:

- **Dependency policy enforcement** using `cargo-deny` (advisories, licenses, bans, and sources).
- **Static analysis** using `cargo-audit` (dependency vulnerability scan).
- **Linting / code quality** using `cargo clippy` (with warnings reported in CI).
- **Automated CI integration** via GitHub Actions (`.github/workflows/security-scan.yml`).

## Workflow

### GitHub Actions: `security-scan.yml`

The workflow runs on every push and pull request to `main` and performs:

1. **Dependency policy check**
   - Runs `cargo-deny` against the `onchain` workspace.
   - Blocks the workflow on vulnerability advisories, yanked crates, disallowed licenses, wildcard dependency versions, unknown registries, and unknown Git sources.

2. **Dependency vulnerability scan**
   - Installs `cargo-audit`.
   - Runs `cargo audit` against the workspace to detect known CVEs and insecure dependencies.

3. **Static analysis (Clippy)**
   - Runs `cargo clippy --all-targets --all-features`.
   - Reports lints and warnings; can be tightened to `-D warnings` once the codebase is fully clean.

4. **Summary step**
   - Emits a short summary so logs clearly show scan completion.

## Dependency Policy

The root `deny.toml` keeps the dependency policy intentionally small:

- Advisories: vulnerability and unsoundness advisories use the cargo-deny defaults, yanked crates fail CI, and unmaintained advisories fail only for direct workspace dependencies. There are no advisory ignores.
- Licenses: third-party crates must use permissive licenses already common in the Rust/Soroban ecosystem. Private workspace crates marked `publish = false` are ignored so the check stays focused on external dependencies.
- Bans: wildcard dependency versions fail. Duplicate transitive versions warn instead of failing because the current Soroban dependency graph contains upstream duplicates that should be reduced opportunistically rather than ignored.
- Sources: dependencies must come from crates.io. Git dependencies are denied unless an explicit future exception is reviewed and added.

When a new dependency fails this policy, prefer upgrading or replacing the dependency. Add exceptions only with a short reason tied to the dependency's license, source, or advisory impact.

## Usage

- CI: The scans run automatically on GitHub Actions for pushes and pull requests.
- Local: Developers can run the same tools locally:

```bash
cargo install cargo-deny --locked
cargo deny --manifest-path onchain/Cargo.toml check advisories bans licenses sources
cargo install cargo-audit --locked
(cd onchain && cargo audit)
(cd onchain && cargo clippy --all-targets --all-features)
```

## Notes

- Tools like Slither/Mythril primarily target EVM/Solidity; for this Rust/Soroban codebase,
  `cargo-deny`, `cargo-audit`, and `clippy` are used as the primary static security analyzers.
- Dynamic security behavior is exercised via the existing `cargo test` suites, including
  dedicated tests for disputes, grace periods, and reentrancy behavior.
