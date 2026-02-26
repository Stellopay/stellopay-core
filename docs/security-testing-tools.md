# Security Testing Tools Integration

This document describes the security testing tooling integrated for issue #236.

## Overview

The security tooling integration provides:

- **Static analysis** using `cargo-audit` (dependency vulnerability scan).
- **Linting / code quality** using `cargo clippy` (with warnings reported in CI).
- **Automated CI integration** via GitHub Actions (`.github/workflows/security-scan.yml`).

## Workflow

### GitHub Actions: `security-scan.yml`

The workflow runs on every push and pull request to `main` and performs:

1. **Dependency vulnerability scan**
   - Installs `cargo-audit`.
   - Runs `cargo audit` against the workspace to detect known CVEs and insecure dependencies.

2. **Static analysis (Clippy)**
   - Runs `cargo clippy --all-targets --all-features`.
   - Reports lints and warnings; can be tightened to `-D warnings` once the codebase is fully clean.

3. **Summary step**
   - Emits a short summary so logs clearly show scan completion.

## Usage

- CI: The scans run automatically on GitHub Actions for pushes and pull requests.
- Local: Developers can run the same tools locally:

```bash
cargo install cargo-audit --locked
cargo audit
cargo clippy --all-targets --all-features
```

## Notes

- Tools like Slither/Mythril primarily target EVM/Solidity; for this Rust/Soroban codebase,
  `cargo-audit` and `clippy` are used as the primary static security analyzers.
- Dynamic security behavior is exercised via the existing `cargo test` suites, including
  dedicated tests for disputes, grace periods, and reentrancy behavior.
