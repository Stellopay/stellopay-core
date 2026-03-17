# Stellopay Core

Stellopay Core is a Soroban-based payroll and escrow system built on the [Stellar](https://stellar.org) blockchain. This repository contains the on-chain smart contract workspace, project documentation, and supporting tooling for payroll escrow, recurring salary disbursement, multi-currency payroll flows, governance, compliance, and related operational modules.

## Repository Layout

| Path | Purpose |
| --- | --- |
| `docs/` | Project documentation, API notes, integration guides, architecture docs, deployment guide, and more. |
| `onchain/` | Rust/Soroban workspace (`soroban-sdk` 23.4.1) for 28 smart contracts and integration tests. |
| `scripts/` | Repository helper scripts, including migration/build helpers. |
| `tools/` | CLI tooling and doc-checking utilities. |
| `benchmarks/` | Benchmark data and analysis. |

Start with the [documentation index](docs/README.md) for product and integration context, then use the [on-chain workspace guide](onchain/README.md) for contract build, test, and deployment details.

## On-Chain Contracts

The Soroban workspace is defined in [`onchain/Cargo.toml`](onchain/Cargo.toml) and includes all crates under `onchain/contracts/*` plus `onchain/integration_tests`. Each contract crate maps to a documented module:

| Contract | Description | Docs |
|----------|-------------|------|
| `stello_pay_contract` | Core payroll & escrow agreement logic (payroll, time-based, milestone) | [architecture.md](docs/architecture.md) |
| `audit_logger` | On-chain audit event logging for compliance trails | [audit-logging.md](docs/audit-logging.md) |
| `bonus_system` | Employer bonus & incentive distribution | [bonus-system.md](docs/bonus-system.md) |
| `compliance_checker` | Automated compliance rule validation | [compliance-checker.md](docs/compliance-checker.md) |
| `compliance_reporting` | Compliance report generation and storage | [compliance-reporting.md](docs/compliance-reporting.md) |
| `department_manager` | Organization & department hierarchy management | [department-management.md](docs/department-management.md) |
| `dispute_escalation` | Dispute raising, arbitration, and resolution | [dispute-escalation.md](docs/dispute-escalation.md) |
| `employee_roles` | Employee role assignment and lifecycle | [employee-roles.md](docs/employee-roles.md) |
| `expense_reimbursement` | Expense submission and reimbursement processing | [expense-reimbursement.md](docs/expense-reimbursement.md) |
| `fee_collector` | Protocol fee collection and distribution | [fee-collector.md](docs/fee-collector.md) |
| `governance` | On-chain governance proposals and voting | [governance.md](docs/governance.md) |
| `multisig` | Multi-signature authorization for sensitive ops | [multisig.md](docs/multisig.md) |
| `nft_payroll_badge` | NFT-based payroll badges for employees | [nft-payroll-badge.md](docs/nft-payroll-badge.md) |
| `payment_history` | Payment record storage and querying | [payment-history.md](docs/payment-history.md) |
| `payment_retry` | Failed payment retry logic | [payment-retry.md](docs/payment-retry.md) |
| `payment_scheduler` | Scheduled and recurring payment processing | [payment-scheduler.md](docs/payment-scheduler.md) |
| `payment_splitter` | Payment splitting across multiple recipients | [payment-splitting.md](docs/payment-splitting.md) |
| `payroll_escrow` | Token escrow for payroll fund custody | [payroll-escrow.md](docs/payroll-escrow.md) |
| `price_oracle` | Price feed oracle for multi-currency conversion | [price-oracle.md](docs/price-oracle.md) |
| `rate_limiter` | Rate limiting for contract entrypoints | [rate-limiter.md](docs/rate-limiter.md) |
| `rbac` | Role-based access control implementation | [rbac.md](docs/rbac.md) |
| `rbac-interface` | RBAC trait interface for cross-contract use | [rbac.md](docs/rbac.md) |
| `salary_adjustment` | Salary modification and adjustment tracking | [salary-adjustment.md](docs/salary-adjustment.md) |
| `slashing_penalty` | Penalty and slashing mechanism for violations | [slashing-penalty.md](docs/slashing-penalty.md) |
| `tax_withholding` | Tax calculation and withholding at source | [tax-withholding.md](docs/tax-withholding.md) |
| `template_versioning` | Contract template version management | [template-versioning.md](docs/template-versioning.md) |
| `token_vesting` | Token vesting schedules with cliff and linear release | [vesting.md](docs/vesting.md) |
| `withdrawal_timelock` | Time-locked withdrawal enforcement | [withdrawal-timelock.md](docs/withdrawal-timelock.md) |

See [`onchain/README.md`](onchain/README.md) for the full workspace guide, including integration tests, coverage, benchmarks, and release profile details.

## Documentation Map

Key documentation topics:

- [Documentation index](docs/README.md)
- [System architecture](docs/architecture.md)
- [Payroll escrow](docs/payroll-escrow.md)
- [Deployment guide](docs/deployment.md)
- [WASM build targets](docs/build-targets.md)
- [Multi-currency support](docs/multi-currency.md)
- [API reference](docs/api/README.md)
- [Integration guide](docs/integration/README.md)
- [CI pipeline](docs/ci.md)
- [Benchmarks](docs/benchmarks.md)
- [Upgrade & migration strategy](docs/upgrade-migration-strategy.md)
- [Windows build notes](docs/windows-build.md)

## Build And Test

The workspace is pinned to `soroban-sdk` 23.4.1 (see [`onchain/Cargo.toml`](onchain/Cargo.toml)). Install the Rust toolchain, WASM target, and Stellar CLI:

```sh
rustup install stable
rustup target add wasm32-unknown-unknown
cargo install stellar-cli --locked
```

From the `onchain/` directory, common commands:

```sh
# Build all contracts (WASM)
for dir in contracts/*/; do (cd "$dir" && stellar contract build) || true; done

# Run all workspace tests
cargo test --workspace

# Test a specific contract
cargo test -p stello_pay_contract --verbose
```

See [`onchain/README.md`](onchain/README.md) for detailed build, test, coverage, and benchmark instructions.

## CI

The on-chain workspace uses GitHub Actions to build and test Soroban contracts on pull requests and pushes. See [`onchain/README.md`](onchain/README.md) for the CI overview and [`docs/ci.md`](docs/ci.md) for pipeline details.

## Safety Notes

This repository contains smart contract code. Review migrations, upgrades, and deployment steps carefully before using any live network or production asset. Keep private keys, RPC credentials, wallet secrets, and production data out of commits, issue comments, and logs.

For upgrade and migration planning, see [Migrations](docs/migrations.md) and [Upgrade and migration strategy](docs/upgrade-migration-strategy.md).

## License

This project is licensed under the MIT License. See [`onchain/README.md`](onchain/README.md#license) for details.
