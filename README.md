# Stellopay Core

Stellopay Core is a Soroban-based payroll contract workspace for Stellar. It contains the on-chain contracts, documentation, and supporting scripts for payroll escrow, recurring salary disbursement, multi-currency payroll flows, governance, compliance, and related operational modules.

## Repository Layout

| Path | Purpose |
| --- | --- |
| `docs/` | Project documentation, API notes, integration guides, architecture docs, migration guidance, examples, and Windows build notes. |
| `onchain/` | Rust/Soroban workspace for smart contracts and integration tests. |
| `onchain/contracts/` | Individual contract crates for payroll, governance, compliance, payment scheduling, vesting, withdrawal controls, and related modules. |
| `scripts/` | Repository helper scripts, including migration/build helpers. |
| `tools/` | Supporting tooling for repository workflows. |

Start with the [documentation index](docs/README.md) for product and integration context, then use the [on-chain workspace guide](onchain/README.md) for contract build and test details.

## On-Chain Workspace

The Soroban workspace is defined in [`onchain/Cargo.toml`](onchain/Cargo.toml). It includes all crates under `onchain/contracts/*` plus `onchain/integration_tests`.

### Core payroll

| Contract | Focus |
| --- | --- |
| [`stello_pay_contract`](onchain/contracts/stello_pay_contract/) | Core contract: payroll agreements, milestone escrow, multi-currency, disputes. |
| [`payroll_escrow`](onchain/contracts/payroll_escrow/) | Escrowed salary funding and release flows. |
| [`payment_scheduler`](onchain/contracts/payment_scheduler/) | Scheduled and recurring payment support. |
| [`payment_retry`](onchain/contracts/payment_retry/) | Retry handling for failed payment attempts. |
| [`payment_splitter`](onchain/contracts/payment_splitter/) | Split-payment logic for multi-recipient payouts. |
| [`payment_history`](onchain/contracts/payment_history/) | Immutable on-chain payment history log. |

### Compliance and reporting

| Contract | Focus |
| --- | --- |
| [`compliance_checker`](onchain/contracts/compliance_checker/) | Rule-based action compliance checks. |
| [`compliance_reporting`](onchain/contracts/compliance_reporting/) | Structured compliance report emission. |
| [`tax_withholding`](onchain/contracts/tax_withholding/) | On-chain tax-withholding deductions. |
| [`audit_logger`](onchain/contracts/audit_logger/) | Cross-contract audit-trail emission. |

### Access control and governance

| Contract | Focus |
| --- | --- |
| [`rbac`](onchain/contracts/rbac/) + [`rbac-interface`](onchain/contracts/rbac-interface/) | Role-based access control and typed cross-contract interface. |
| [`governance`](onchain/contracts/governance/) | On-chain proposal and voting system. |
| [`multisig`](onchain/contracts/multisig/) | Multi-signature approval for high-stakes operations. |
| [`employee_roles`](onchain/contracts/employee_roles/) | Per-employee role and permission registry. |
| [`department_manager`](onchain/contracts/department_manager/) | Org-unit grouping for payroll operations. |

### Financial controls

| Contract | Focus |
| --- | --- |
| [`price_oracle`](onchain/contracts/price_oracle/) | FX rates and pricing for multi-currency flows. |
| [`fee_collector`](onchain/contracts/fee_collector/) | Protocol fee collection and routing. |
| [`rate_limiter`](onchain/contracts/rate_limiter/) | Per-caller claim rate limiting. |
| [`salary_adjustment`](onchain/contracts/salary_adjustment/) | Dynamic salary override hooks. |
| [`bonus_system`](onchain/contracts/bonus_system/) | On-chain bonus calculation and distribution. |
| [`expense_reimbursement`](onchain/contracts/expense_reimbursement/) | Employee expense claim and approval. |

### Vesting and lifecycle

| Contract | Focus |
| --- | --- |
| [`token_vesting`](onchain/contracts/token_vesting/) | Time-based and cliff vesting schedules. |
| [`withdrawal_timelock`](onchain/contracts/withdrawal_timelock/) | Withdrawal delay enforcement. |
| [`slashing_penalty`](onchain/contracts/slashing_penalty/) | Penalty slashing on policy violations. |
| [`dispute_escalation`](onchain/contracts/dispute_escalation/) | Escalated dispute handling beyond the core arbiter. |
| [`nft_payroll_badge`](onchain/contracts/nft_payroll_badge/) | NFT badge issuance for payroll milestones. |

### Tooling crates (rlib only)

| Crate | Purpose |
| --- | --- |
| [`rbac-interface`](onchain/contracts/rbac-interface/) | Typed cross-contract RBAC client (no cdylib dependency). |
| [`milestone-interface`](onchain/contracts/milestone-interface/) | Typed cross-contract milestone query client. |
| [`template_versioning`](onchain/contracts/template_versioning/) | Contract schema versioning utilities. |

## Documentation Map

- [API documentation](docs/api/README.md)
- [Integration guide](docs/integration/README.md)
- [Architecture](docs/architecture.md)
- [Examples](docs/examples/README.md)
- [Best practices](docs/best-practices/README.md)
- [Developer tools](docs/dev-tools/README.md)
- [CI and local checks](docs/ci.md)
- [Migrations](docs/migrations.md)
- [Upgrade and migration strategy](docs/upgrade-migration-strategy.md)
- [Windows build notes](docs/windows-build.md)

## Build And Test

Install Rust and the Soroban CLI before running contract commands locally. The on-chain README pins the Soroban CLI example to `20.0.0-rc.1`:

```sh
rustup install stable
cargo install --locked --version 20.0.0-rc.1 soroban-cli
```

Common local checks from the on-chain workspace:

```sh
cd onchain
cargo build
cargo test
```

For Soroban contract builds, use:

```sh
cd onchain
stellar contract build
```

On Windows GNU/MinGW, the repository documents a WASM-only path for the known `export ordinal too large` linker issue:

```powershell
rustup target add wasm32-unknown-unknown
.\scripts\migrations\build_wasm_only.ps1
```

See [Building on Windows](docs/windows-build.md) for the full Windows guidance.

## CI

The on-chain workspace uses GitHub Actions to build and test Soroban contracts on pull requests and pushes to the main branches. See [`onchain/README.md`](onchain/README.md) for the CI overview and local setup notes. For the exact list of commands CI runs and how to run them locally, see [CI and local checks](docs/ci.md).

## Contributing and security

- [Contributing guide](CONTRIBUTING.md) — workspace layout, build/test workflow, and PR expectations
- [Security policy](SECURITY.md) — responsible disclosure for contracts under `onchain/contracts/`
- [Open an issue](.github/ISSUE_TEMPLATE/) — bug, feature, or security report templates

## Safety Notes

This repository contains smart contract code. Review migrations, upgrades, and deployment steps carefully before using any live network or production asset. Keep private keys, RPC credentials, wallet secrets, and production database or ledger data out of commits, issue comments, and logs.

### Dispute payout conservation

`resolve_dispute` / `resolve_dispute_multisig` (in `onchain/contracts/stello_pay_contract`) conserve funds deterministically:

- `pay_employee` is split equally across employees; the integer-division remainder (dust) is added to the **last** employee so the employee transfers sum to `pay_employee` exactly and no tokens are stranded.
- `pay_employee` and `refund_employer` must be non-negative, and their sum must not exceed the agreement's `total_amount` nor (when tracked) its real per-agreement escrow balance; the escrow balance is decremented by the distributed total after transfers. Out-of-range or negative payouts return `PayrollError::InvalidPayout`.

For upgrade and migration planning, start with [Migrations](docs/migrations.md) and [Upgrade and migration strategy](docs/upgrade-migration-strategy.md).

## License

This project is licensed under the MIT License. See [`onchain/README.md`](onchain/README.md#license) for the existing license note.

