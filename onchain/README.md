# Stellopay Core – On-Chain Contracts

![Contracts CI](https://github.com/Stellopay/stellopay-core/actions/workflows/contracts.yml/badge.svg)

A decentralized payroll and agreement system built on the [Stellar](https://stellar.org) blockchain using [Soroban](https://soroban.stellar.org). This workspace contains **28 smart contracts** plus integration tests, covering payroll escrows, salary disbursement, multi-currency swaps, governance, compliance, and more.

| Toolchain | Version |
|-----------|---------|
| Soroban SDK | `23.4.1` (pinned in workspace `Cargo.toml`) |
| Stellar CLI | Latest stable (`cargo install stellar-cli --locked`) |
| WASM target | `wasm32-unknown-unknown` |
| Rust | Stable channel |

---

## Prerequisites

Install the Rust toolchain, the WASM compilation target, and the Stellar CLI:

```bash
# 1. Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup install stable

# 2. Add the WASM compilation target
rustup target add wasm32-unknown-unknown

# 3. Install the Stellar CLI
cargo install stellar-cli --locked
```

> **Note:** CI (`.github/workflows/contracts.yml`) uses exactly these steps. See [docs/ci.md](../docs/ci.md) for the full pipeline.

---

### Batch Size Ceiling

All public batch entrypoints are capped by `storage::MAX_BATCH_SIZE` (`20` items):
`batch_claim_payroll`, `batch_claim_milestones`,
`batch_create_payroll_agreements`, and `batch_create_escrow_agreements`.
Oversized batches fail before item processing with `PayrollError::BatchTooLarge`.

The ceiling is tied to `onchain/contracts/stello_pay_contract/tests/gas_benchmarks.rs`,
which records the cost of `batch_claim_milestones` at the max size and asserts it
stays under the documented instruction threshold. Run:

```sh
cargo test -p stello_pay_contract gas_benchmark -- --nocapture
```

## Getting Started

```bash
# Clone the repository
git clone https://github.com/Stellopay/stellopay-core.git
cd stellopay-core/onchain
```

---

## Building

### Build a specific contract (WASM)

```bash
cd contracts/stello_pay_contract
stellar contract build --verbose
```

`stellar contract build` automatically targets `wasm32-unknown-unknown` and applies the workspace release profile. See [docs/build-targets.md](../docs/build-targets.md) for why this target is required.

### Build all contracts

```bash
# From onchain/
for dir in contracts/*/; do
  (cd "$dir" && stellar contract build --verbose 2>/dev/null) || true
done
```

---

## Testing

### Run all workspace tests

```bash
# From onchain/
cargo test --workspace
```

### Run tests for a specific package

```bash
cargo test -p stello_pay_contract --verbose
cargo test -p integration_tests --verbose
cargo test -p template_versioning --verbose
```

### Run a specific test

```bash
cargo test -p stello_pay_contract test_payroll
cargo test -p stello_pay_contract test_create_or_update_escrow
```

Test snapshots are automatically generated in each contract's `test_snapshots` directory when tests run. These ensure contract behavior remains consistent across changes.

---

## Contract Inventory

All contracts live under `onchain/contracts/`. The workspace member glob `contracts/*` includes them all.

| Contract | Description | Docs |
|----------|-------------|------|
| `stello_pay_contract` | Core payroll & escrow agreement logic (payroll, time-based, milestone) | [architecture.md](../docs/architecture.md) |
| `audit_logger` | On-chain audit event logging for compliance trails | [audit-logging.md](../docs/audit-logging.md) |
| `bonus_system` | Employer bonus & incentive distribution | [bonus-system.md](../docs/bonus-system.md) |
| `compliance_checker` | Automated compliance rule validation | [compliance-checker.md](../docs/compliance-checker.md) |
| `compliance_reporting` | Compliance report generation and storage | [compliance-reporting.md](../docs/compliance-reporting.md) |
| `department_manager` | Organization & department hierarchy management | [department-management.md](../docs/department-management.md) |
| `dispute_escalation` | Dispute raising, arbitration, and resolution | [dispute-escalation.md](../docs/dispute-escalation.md) |
| `employee_roles` | Employee role assignment and lifecycle | [employee-roles.md](../docs/employee-roles.md) |
| `expense_reimbursement` | Expense submission and reimbursement processing | [expense-reimbursement.md](../docs/expense-reimbursement.md) |
| `fee_collector` | Protocol fee collection and distribution | [fee-collector.md](../docs/fee-collector.md) |
| `governance` | On-chain governance proposals and voting | [governance.md](../docs/governance.md) |
| `multisig` | Multi-signature authorization for sensitive ops | [multisig.md](../docs/multisig.md) |
| `nft_payroll_badge` | NFT-based payroll badges for employees | [nft-payroll-badge.md](../docs/nft-payroll-badge.md) |
| `payment_history` | Payment record storage and querying | [payment-history.md](../docs/payment-history.md) |
| `payment_retry` | Failed payment retry logic | [payment-retry.md](../docs/payment-retry.md) |
| `payment_scheduler` | Scheduled and recurring payment processing | [payment-scheduler.md](../docs/payment-scheduler.md) |
| `payment_splitter` | Payment splitting across multiple recipients | [payment-splitting.md](../docs/payment-splitting.md) |
| `payroll_escrow` | Token escrow for payroll fund custody | [payroll-escrow.md](../docs/payroll-escrow.md) |
| `price_oracle` | Price feed oracle for multi-currency conversion | [price-oracle.md](../docs/price-oracle.md) |
| `rate_limiter` | Rate limiting for contract entrypoints | [rate-limiter.md](../docs/rate-limiter.md) |
| `rbac` | Role-based access control implementation | [rbac.md](../docs/rbac.md) |
| `rbac-interface` | RBAC trait interface for cross-contract use | [rbac.md](../docs/rbac.md) |
| `salary_adjustment` | Salary modification and adjustment tracking | [salary-adjustment.md](../docs/salary-adjustment.md) |
| `slashing_penalty` | Penalty and slashing mechanism for violations | [slashing-penalty.md](../docs/slashing-penalty.md) |
| `tax_withholding` | Tax calculation and withholding at source | [tax-withholding.md](../docs/tax-withholding.md) |
| `template_versioning` | Contract template version management | [template-versioning.md](../docs/template-versioning.md) |
| `token_vesting` | Token vesting schedules with cliff and linear release | [vesting.md](../docs/vesting.md) |
| `withdrawal_timelock` | Time-locked withdrawal enforcement | [withdrawal-timelock.md](../docs/withdrawal-timelock.md) |

**Integration tests** (`onchain/integration_tests/`) — cross-contract workflow tests covering end-to-end scenarios.

---

## Coverage

Generate code coverage reports locally:

```bash
# Install coverage tooling (one-time)
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov

# Generate HTML coverage report
cd onchain
cargo llvm-cov test -p stello_pay_contract -p integration_tests --html

# Generate Codecov-compatible JSON
cargo llvm-cov test -p stello_pay_contract -p integration_tests --codecov --output-path codecov.json
```

CI uploads coverage to [Codecov](https://codecov.io) automatically. See [docs/ci.md](../docs/ci.md) for details.

---

## Benchmarks

Soroban cost benchmarks measure host resource usage for critical contract paths:

```bash
cd contracts/stello_pay_contract
cargo bench --bench critical_paths
```

To compile without running (as CI does):

```bash
cargo bench --bench critical_paths --no-run --verbose
```

See [docs/benchmarks.md](../docs/benchmarks.md) for interpreting results and regression guarding.

---

## Release Profile

The workspace defines a production release profile in [`onchain/Cargo.toml`](./Cargo.toml) optimized for minimal WASM size and deterministic builds:

```toml
[profile.release]
opt-level = "z"        # Optimize for size
debug = 0              # No debug info
strip = "symbols"      # Strip symbols
debug-assertions = false
overflow-checks = true # Keep overflow safety
lto = true             # Link-time optimization
panic = "abort"        # Abort on panic (no unwinding)
codegen-units = 1      # Single codegen unit for max optimization
```

A `release-with-logs` variant inherits all settings but re-enables `debug-assertions` for diagnostic builds.

> **Security note:** All production WASM artifacts must be built with this profile to ensure reproducible, deployment-safe binaries. Using different flags risks non-reproducible artifacts that may behave differently on-network.

---

## Building on Windows

If you see **"export ordinal too large"** when running `cargo test` or `cargo build` with the GNU/MinGW toolchain, use the WASM-only build approach:

```powershell
rustup target add wasm32-unknown-unknown
cd onchain\contracts\stello_pay_contract
cargo build -p stello_pay_contract --target wasm32-unknown-unknown --release
```

Run tests in WSL or rely on CI. See [docs/windows-build.md](../docs/windows-build.md) for full instructions (Option A: MSVC, Option B: GNU + WSL).

---

## Tooling

### `tools/cli` — StellopayCore CLI

Command-line interface for deploying, querying, and monitoring Stellopay contracts on any Stellar network (testnet, mainnet, futurenet).

```bash
cd tools/cli
cargo build --release
./target/release/stellopay-cli status
```

See [tools/cli/README.md](../tools/cli/README.md) for full usage.

## Project health

- [CONTRIBUTING.md](../CONTRIBUTING.md)
- [SECURITY.md](../SECURITY.md)
- [Issue templates](../.github/ISSUE_TEMPLATE/)

### `tools/doc_checker` — Documentation Checker

Scans all `#[contractimpl]` public functions for missing doc comments (params, returns, access control).

```bash
cd tools/doc_checker
cargo run
```

---

## Further Reading

| Topic | Link |
|-------|------|
| Full documentation index | [docs/README.md](../docs/README.md) |
| System architecture | [docs/architecture.md](../docs/architecture.md) |
| WASM build target rationale | [docs/build-targets.md](../docs/build-targets.md) |
| CI pipeline details | [docs/ci.md](../docs/ci.md) |
| Deployment guide | [docs/deployment.md](../docs/deployment.md) |
| Windows / WSL builds | [docs/windows-build.md](../docs/windows-build.md) |
| Benchmarks | [docs/benchmarks.md](../docs/benchmarks.md) |
| Upgrade & migration strategy | [docs/upgrade-migration-strategy.md](../docs/upgrade-migration-strategy.md) |
| Threat model | [docs/threat-model.md](../docs/threat-model.md) |

---

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
