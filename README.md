# Stellopay Core

Stellopay Core is a Soroban payroll and escrow workspace for Stellar. It
contains on-chain contracts, integration tests, documentation, and supporting
tools for payroll escrow, recurring salary disbursement, multi-currency payroll,
governance, compliance, and operational safety modules.

Use this README as the repository entry point. Detailed build and contract
guidance stays in the existing [on-chain workspace guide](onchain/README.md)
and product documentation stays in the [docs index](docs/README.md).

## Repository Layout

| Path | Purpose |
| --- | --- |
| [docs/](docs/README.md) | Architecture, contract guides, deployment notes, examples, CI notes, and integration documentation. |
| [onchain/](onchain/README.md) | Rust/Soroban workspace containing contract crates and integration tests. |
| [onchain/contracts/](onchain/contracts/) | Individual Soroban contract crates for payroll, governance, compliance, scheduling, escrow, and related modules. |
| [scripts/](scripts/migrations/README.md) | Repository helper scripts, including migration/build helpers. |
| [tools/](tools/cli/README.md) | Supporting CLI and documentation-checker tooling. |

## Architecture Map

The on-chain workspace is defined in [onchain/Cargo.toml](onchain/Cargo.toml)
and uses Soroban SDK `23.4.1`. The main product flow is:

1. Payroll and agreement state lives in the core payroll contracts.
2. Supporting contracts provide escrow, scheduling, payment history, retry,
   compliance, audit logging, governance, and role management.
3. Integration tests in [onchain/integration_tests](onchain/integration_tests/)
   exercise cross-contract behavior.
4. Operational docs in [docs/](docs/README.md) describe deployment,
   migration, monitoring, and security expectations.

Start with these documents:

- [Architecture](docs/architecture.md)
- [Payroll escrow](docs/payroll-escrow.md)
- [Deployment](docs/deployment.md)
- [Build targets](docs/build-targets.md)
- [Multi-currency payroll](docs/multi-currency.md)
- [CI pipeline](docs/ci.md)
- [Documentation index](docs/README.md)

## Contract Inventory

All contract crates live under [onchain/contracts/](onchain/contracts/). The
workspace member glob in [onchain/Cargo.toml](onchain/Cargo.toml) includes each
crate plus the integration test crate.

| Contract | Purpose | Primary Doc |
| --- | --- | --- |
| `stello_pay_contract` | Core payroll and escrow agreement logic. | [architecture.md](docs/architecture.md) |
| `audit_logger` | On-chain audit event logging for compliance trails. | [audit-logging.md](docs/audit-logging.md) |
| `bonus_system` | Employer bonus and incentive distribution. | [bonus-system.md](docs/bonus-system.md) |
| `compliance_checker` | Automated compliance rule validation. | [compliance-checker.md](docs/compliance-checker.md) |
| `compliance_reporting` | Compliance report generation and storage. | [compliance-reporting.md](docs/compliance-reporting.md) |
| `department_manager` | Organization and department hierarchy management. | [department-management.md](docs/department-management.md) |
| `dispute_escalation` | Dispute raising, arbitration, and resolution. | [dispute-escalation.md](docs/dispute-escalation.md) |
| `employee_roles` | Employee role assignment and lifecycle. | [employee-roles.md](docs/employee-roles.md) |
| `expense_reimbursement` | Expense submission and reimbursement processing. | [expense-reimbursement.md](docs/expense-reimbursement.md) |
| `fee_collector` | Protocol fee collection and distribution. | [fee-collector.md](docs/fee-collector.md) |
| `governance` | On-chain governance proposals and voting. | [governance.md](docs/governance.md) |
| `multisig` | Multi-signature authorization for sensitive operations. | [multisig.md](docs/multisig.md) |
| `nft_payroll_badge` | NFT-based payroll badges for employees. | [nft-payroll-badge.md](docs/nft-payroll-badge.md) |
| `payment_history` | Payment record storage and querying. | [payment-history.md](docs/payment-history.md) |
| `payment_retry` | Failed payment retry logic. | [payment-retry.md](docs/payment-retry.md) |
| `payment_scheduler` | Scheduled and recurring payment processing. | [payment-scheduler.md](docs/payment-scheduler.md) |
| `payment_splitter` | Payment splitting across multiple recipients. | [payment-splitting.md](docs/payment-splitting.md) |
| `payroll_escrow` | Token escrow for payroll fund custody. | [payroll-escrow.md](docs/payroll-escrow.md) |
| `price_oracle` | Price feed oracle for multi-currency conversion. | [price-oracle.md](docs/price-oracle.md) |
| `rate_limiter` | Rate limiting for contract entrypoints. | [rate-limiter.md](docs/rate-limiter.md) |
| `rbac` | Role-based access control implementation. | [rbac.md](docs/rbac.md) |
| `rbac-interface` | RBAC trait interface for cross-contract use. | [rbac.md](docs/rbac.md) |
| `salary_adjustment` | Salary modification and adjustment tracking. | [salary-adjustment.md](docs/salary-adjustment.md) |
| `slashing_penalty` | Penalty and slashing mechanism for violations. | [slashing-penalty.md](docs/slashing-penalty.md) |
| `tax_withholding` | Tax calculation and withholding at source. | [tax-withholding.md](docs/tax-withholding.md) |
| `template_versioning` | Contract template version management. | [template-versioning.md](docs/template-versioning.md) |
| `token_vesting` | Token vesting schedules with cliff and linear release. | [vesting.md](docs/vesting.md) |
| `withdrawal_timelock` | Time-locked withdrawal enforcement. | [withdrawal-timelock.md](docs/withdrawal-timelock.md) |

## Build And Test

Install Rust, the WASM target, and the Stellar CLI before running contract
commands locally:

```sh
rustup install stable
rustup target add wasm32-unknown-unknown
cargo install stellar-cli --locked
```

Common checks from the on-chain workspace:

```sh
cd onchain
cargo test --workspace
cargo build --target wasm32-unknown-unknown --release
```

Build a contract with the Stellar CLI:

```sh
cd onchain/contracts/stello_pay_contract
stellar contract build --verbose
```

For platform-specific build notes, see [Build targets](docs/build-targets.md)
and [Windows build notes](docs/windows-build.md).

## CI And Tooling

GitHub Actions build and test the Soroban workspace on pull requests and pushes
to `main`. See [docs/ci.md](docs/ci.md) for the workflow structure, coverage
upload, benchmark compilation, and local parity notes.

Supporting tools:

- [tools/cli](tools/cli/README.md) for repository CLI operations.
- [tools/doc_checker](tools/doc_checker/Cargo.toml) for documentation checks.
- [docs/dev-tools](docs/dev-tools/README.md) for developer tool usage.

## Safety Notes

This repository contains smart contract code. Review migrations, upgrades, and
deployment steps carefully before using any live network or production asset.
Keep private keys, RPC credentials, wallet secrets, and production database or
ledger data out of commits, issue comments, docs examples, and logs.

For upgrade and migration planning, start with [Migrations](docs/migrations.md)
and [Upgrade and migration strategy](docs/upgrade-migration-strategy.md).

## License

This project is licensed under the MIT License. See
[onchain/README.md](onchain/README.md#license) for the existing license note.
