# Contributing to Stellopay Core

Thank you for your interest in contributing! Stellopay Core is a Soroban-based payroll and escrow system on the Stellar network.

## Getting Started

### Prerequisites

- Rust (stable, minimum 1.75.0)
- WASM target: `rustup target add wasm32-unknown-unknown`
- Soroban CLI: `cargo install --locked soroban-cli`
- For testing: `cargo install cargo-deny cargo-audit`

### Workspace Layout

```
onchain/contracts/       # Soroban smart contracts
├── payroll/             # Payroll contract
├── escrow/              # Escrow contract
└── governance/          # Governance/multi-sig
tools/                   # Off-chain tooling
├── doc_checker/         # Documentation compliance checker
├── compliance_checker/  # Compliance rule engine
└── ...
```

### Build & Test

```bash
# Build all contracts
cargo build --target wasm32-unknown-unknown --release

# Run all tests
cargo test --workspace

# Format code
cargo fmt --check

# Lint
cargo clippy --workspace -- -D warnings
```

### Pull Request Process

1. Fork the repository and create a feature branch from `main`.
2. If your change touches a contract, add or update tests.
3. Ensure `cargo test --workspace` passes.
4. Run `cargo fmt --check` before committing.
5. Update documentation if your change affects public interfaces.
6. Open a PR with a clear title and description referencing the related issue.

### Commit Conventions

We use conventional commits: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, etc.

## Code of Conduct

All participants are expected to maintain a respectful and constructive environment.
