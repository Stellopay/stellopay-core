# Stellopay Core

Stellopay Core is a Soroban smart-contract workspace for payroll, escrow,
compliance, governance, and payment automation on Stellar. The repository is
organized around an on-chain Rust workspace plus a documentation set for
architecture, deployment, operations, and contract-specific behavior.

For detailed build, test, benchmark, and release-profile guidance, start with
[`onchain/README.md`](onchain/README.md). This root README is the project entry
point and index.

## Repository Map

| Path | Purpose |
| --- | --- |
| [`onchain/`](onchain/) | Rust workspace for all Soroban contracts and integration tests |
| [`onchain/contracts/`](onchain/contracts/) | Contract crates included by the workspace `contracts/*` member glob |
| [`onchain/integration_tests/`](onchain/integration_tests/) | Cross-contract workflow tests |
| [`docs/`](docs/) | Architecture, deployment, security, operations, and contract reference docs |
| [`docs/README.md`](docs/README.md) | Documentation index |
| [`scripts/`](scripts/) | Repository helper scripts, including migration and build helpers |
| [`tools/`](tools/) | Supporting tooling for repository workflows |

## Architecture

The system centers on payroll and escrow contracts that custody and release
payments according to agreement terms. Supporting contracts add audit logging,
role-based access control, compliance checks, dispute escalation, payment
scheduling, payment splitting, fee collection, rate limiting, governance, and
operational safety controls.

Recommended architecture reading:

- [`docs/architecture.md`](docs/architecture.md)
- [`docs/payroll-escrow.md`](docs/payroll-escrow.md)
- [`docs/multi-currency.md`](docs/multi-currency.md)
- [`docs/deployment.md`](docs/deployment.md)
- [`docs/build-targets.md`](docs/build-targets.md)
- [`docs/README.md`](docs/README.md)

## Toolchain

The on-chain workspace pins `soroban-sdk = "23.4.1"` in
[`onchain/Cargo.toml`](onchain/Cargo.toml). Use stable Rust, the
`wasm32-unknown-unknown` target, and the Stellar CLI as described in
[`onchain/README.md`](onchain/README.md).

```bash
rustup install stable
rustup target add wasm32-unknown-unknown
cargo install stellar-cli --locked
```

## Build And Test

Run commands from the on-chain workspace:

```bash
cd onchain
cargo test --workspace
cargo build --target wasm32-unknown-unknown --release
```

For contract-optimized WASM builds, use `stellar contract build` inside the
contract crate as documented in [`onchain/README.md`](onchain/README.md) and
[`docs/build-targets.md`](docs/build-targets.md).

## Contract Inventory

All contract crates live in [`onchain/contracts/`](onchain/contracts/).

| Contract crate | Purpose | Primary docs |
| --- | --- | --- |
| [`audit_logger`](onchain/contracts/audit_logger/) | On-chain audit event logging for compliance trails | [`docs/audit-logging.md`](docs/audit-logging.md) |
| [`bonus_system`](onchain/contracts/bonus_system/) | Employer bonus and incentive distribution | [`docs/bonus-system.md`](docs/bonus-system.md) |
| [`compliance_checker`](onchain/contracts/compliance_checker/) | Automated compliance rule validation | [`docs/compliance-checker.md`](docs/compliance-checker.md) |
| [`compliance_reporting`](onchain/contracts/compliance_reporting/) | Compliance report generation and storage | [`docs/compliance-reporting.md`](docs/compliance-reporting.md) |
| [`department_manager`](onchain/contracts/department_manager/) | Organization and department hierarchy management | [`docs/department-management.md`](docs/department-management.md) |
| [`dispute_escalation`](onchain/contracts/dispute_escalation/) | Dispute raising, arbitration, and resolution | [`docs/dispute-escalation.md`](docs/dispute-escalation.md) |
| [`employee_roles`](onchain/contracts/employee_roles/) | Employee role assignment and lifecycle support | [`docs/employee-roles.md`](docs/employee-roles.md) |
| [`expense_reimbursement`](onchain/contracts/expense_reimbursement/) | Expense submission and reimbursement processing | [`docs/expense-reimbursement.md`](docs/expense-reimbursement.md) |
| [`fee_collector`](onchain/contracts/fee_collector/) | Protocol fee collection and distribution | [`docs/fee-collector.md`](docs/fee-collector.md) |
| [`governance`](onchain/contracts/governance/) | On-chain governance proposals and voting | [`docs/governance.md`](docs/governance.md) |
| [`multisig`](onchain/contracts/multisig/) | Multi-signature authorization for sensitive operations | [`docs/multisig.md`](docs/multisig.md) |
| [`nft_payroll_badge`](onchain/contracts/nft_payroll_badge/) | NFT-based payroll badges for employees | [`docs/nft-payroll-badge.md`](docs/nft-payroll-badge.md) |
| [`payment_history`](onchain/contracts/payment_history/) | Payment record storage and querying | [`docs/payment-history.md`](docs/payment-history.md) |
| [`payment_retry`](onchain/contracts/payment_retry/) | Failed payment retry logic | [`docs/payment-retry.md`](docs/payment-retry.md) |
| [`payment_scheduler`](onchain/contracts/payment_scheduler/) | Scheduled and recurring payment processing | [`docs/payment-scheduler.md`](docs/payment-scheduler.md) |
| [`payment_splitter`](onchain/contracts/payment_splitter/) | Payment splitting across multiple recipients | [`docs/payment-splitting.md`](docs/payment-splitting.md) |
| [`payroll_escrow`](onchain/contracts/payroll_escrow/) | Token escrow for payroll fund custody | [`docs/payroll-escrow.md`](docs/payroll-escrow.md) |
| [`price_oracle`](onchain/contracts/price_oracle/) | Price feed oracle for multi-currency conversion | [`docs/price-oracle.md`](docs/price-oracle.md) |
| [`rate_limiter`](onchain/contracts/rate_limiter/) | Rate limiting for contract entrypoints | [`docs/rate-limiter.md`](docs/rate-limiter.md) |
| [`rbac`](onchain/contracts/rbac/) | Role-based access control implementation | [`docs/rbac.md`](docs/rbac.md) |
| [`rbac-interface`](onchain/contracts/rbac-interface/) | RBAC trait interface for cross-contract use | [`docs/rbac.md`](docs/rbac.md) |
| [`salary_adjustment`](onchain/contracts/salary_adjustment/) | Salary modification and adjustment tracking | [`docs/salary-adjustment.md`](docs/salary-adjustment.md) |
| [`slashing_penalty`](onchain/contracts/slashing_penalty/) | Penalty and slashing mechanisms for violations | [`docs/slashing-penalty.md`](docs/slashing-penalty.md) |
| [`stello_pay_contract`](onchain/contracts/stello_pay_contract/) | Core payroll and escrow agreement logic | [`docs/architecture.md`](docs/architecture.md) |
| [`tax_withholding`](onchain/contracts/tax_withholding/) | Tax calculation and withholding at source | [`docs/tax-withholding.md`](docs/tax-withholding.md) |
| [`template_versioning`](onchain/contracts/template_versioning/) | Contract template version management | [`docs/template-versioning.md`](docs/template-versioning.md) |
| [`token_vesting`](onchain/contracts/token_vesting/) | Token vesting schedules with cliff and linear release | [`docs/vesting.md`](docs/vesting.md) |
| [`withdrawal_timelock`](onchain/contracts/withdrawal_timelock/) | Time-locked withdrawal enforcement | [`docs/withdrawal-timelock.md`](docs/withdrawal-timelock.md) |

## Documentation Index

- API documentation: [`docs/api/README.md`](docs/api/README.md)
- Integration guide: [`docs/integration/README.md`](docs/integration/README.md)
- System design: [`docs/architecture.md`](docs/architecture.md)
- Payroll escrow: [`docs/payroll-escrow.md`](docs/payroll-escrow.md)
- Deployment: [`docs/deployment.md`](docs/deployment.md)
- WASM build targets: [`docs/build-targets.md`](docs/build-targets.md)
- Multi-currency support: [`docs/multi-currency.md`](docs/multi-currency.md)
- Upgrade and migration strategy: [`docs/upgrade-migration-strategy.md`](docs/upgrade-migration-strategy.md)
- Windows build notes: [`docs/windows-build.md`](docs/windows-build.md)
- Full documentation index: [`docs/README.md`](docs/README.md)

## Security Notes

Examples in this repository use placeholder addresses and local commands. Do
not commit production secrets, private keys, seed phrases, RPC credentials, or
deployment identities. Review [`docs/deployment.md`](docs/deployment.md) before
deploying to testnet or mainnet.

For upgrade and migration planning, start with
[`docs/migrations.md`](docs/migrations.md) and
[`docs/upgrade-migration-strategy.md`](docs/upgrade-migration-strategy.md).

## License

This project is licensed under the MIT License. See
[`onchain/README.md`](onchain/README.md#license) for the existing license note.
