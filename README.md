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

Notable contract modules include:

| Contract | Focus |
| --- | --- |
| `stello_pay_contract` | Core payroll contract implementation. |
| `payroll_escrow` | Escrowed salary funding and release flows. |
| `payment_scheduler` | Scheduled and recurring payment support. |
| `payment_retry` | Retry handling for failed payment attempts. |
| `payment_splitter` | Split-payment logic. |
| `price_oracle` | Pricing and conversion support for multi-currency flows. |
| `governance`, `multisig`, `rbac` | Administrative controls and permissioning. |
| `compliance_checker`, `compliance_reporting`, `tax_withholding` | Compliance and reporting support. |
| `token_vesting`, `withdrawal_timelock`, `slashing_penalty` | Vesting, withdrawal constraints, and penalty flows. |
| `audit_logger`, `payment_history` | Audit and historical payment records. |

See [`onchain/README.md`](onchain/README.md#contract-inventory) for the full
contract inventory with primary documentation links.

## Documentation Map

- [API documentation](docs/api/README.md)
- [Integration guide](docs/integration/README.md)
- [Architecture](docs/architecture.md)
- [Payroll escrow](docs/payroll-escrow.md)
- [Deployment](docs/deployment.md)
- [Build targets](docs/build-targets.md)
- [Multi-currency payroll](docs/multi-currency.md)
- [Examples](docs/examples/README.md)
- [Best practices](docs/best-practices/README.md)
- [Developer tools](docs/dev-tools/README.md)
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
cargo build --target wasm32-unknown-unknown
cargo test --workspace
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

The on-chain workspace uses GitHub Actions to build and test Soroban contracts on pull requests and pushes to the main branches. See [`onchain/README.md`](onchain/README.md) for the CI overview and local setup notes.

## Safety Notes

This repository contains smart contract code. Review migrations, upgrades, and deployment steps carefully before using any live network or production asset. Keep private keys, RPC credentials, wallet secrets, and production database or ledger data out of commits, issue comments, and logs.

For upgrade and migration planning, start with [Migrations](docs/migrations.md) and [Upgrade and migration strategy](docs/upgrade-migration-strategy.md).

## License

This project is licensed under the MIT License. See [`onchain/README.md`](onchain/README.md#license) for the existing license note.

