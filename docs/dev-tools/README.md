# Developer Tools

Comprehensive set of tools and utilities for working with the StellopayCore contract.

## Table of Contents

1. [CLI Tools](#cli-tools)
2. [Monitoring and Debugging Tools](#monitoring-and-debugging-tools)
3. [Testing Utilities](#testing-utilities)
4. [Code Generation](#code-generation)
5. [Development Scripts](#development-scripts)

## CLI Tools

### StellopayCore CLI

A command-line interface for managing payroll operations.

#### Installation

```bash
# Install from source
git clone https://github.com/stellopay/stellopay-core
cd stellopay-core/tools/cli
cargo install --path .

# Or install from registry
cargo install stellopay-cli
```

#### Real Commands

The CLI exposes five top-level commands. Run `stellopay-cli <COMMAND> --help` for per-command flags.

| Command | Description |
|---|---|
| `deploy` | Deploy a new contract |
| `info` | Get contract information |
| `status` | Show CLI status |
| `emergency-withdraw` | Emergency withdrawal of tokens |
| `webhook` | Webhook management (see subcommands below) |

##### `deploy`

```bash
stellopay-cli deploy --network testnet --owner <OWNER_ADDRESS>
stellopay-cli deploy --network testnet --owner <OWNER_ADDRESS> --wasm ./target/release/contract.wasm
```

| Flag | Required | Description |
|---|---|---|
| `--network` | No (default: testnet) | Network to deploy to |
| `--owner` | Yes | Owner address |
| `--wasm` | No | WASM file path |

##### `info`

```bash
stellopay-cli info --contract-id <CONTRACT_ID>
```

| Flag | Required | Description |
|---|---|---|
| `--contract-id` | No | Contract ID to inspect |

##### `status`

```bash
stellopay-cli status
```

No flags. Displays current CLI configuration status.

##### `emergency-withdraw`

```bash
stellopay-cli emergency-withdraw --contract-id <CONTRACT_ID> --token <TOKEN_ADDRESS> --recipient <ADDRESS> --amount <AMOUNT>
```

| Flag | Required | Description |
|---|---|---|
| `--contract-id` | No | Contract ID |
| `--token` | Yes | Token address |
| `--recipient` | Yes | Recipient address |
| `--amount` | Yes | Amount to withdraw (i128) |

##### `webhook`

Webhook subcommands manage event subscriptions:

```bash
stellopay-cli webhook register --name <NAME> --description <DESC> --url <URL> --events <EVENTS> --secret <SECRET>
stellopay-cli webhook update --webhook-id <ID> [--name <NAME>] [--url <URL>] ...
stellopay-cli webhook delete --webhook-id <ID>
stellopay-cli webhook list --owner <ADDRESS>
stellopay-cli webhook get --webhook-id <ID>
stellopay-cli webhook stats
stellopay-cli webhook test --webhook-id <ID> --event-type <TYPE>
```

| Subcommand | Description |
|---|---|
| `register` | Register a new webhook |
| `update` | Update an existing webhook |
| `delete` | Delete a webhook |
| `list` | List webhooks for an owner |
| `get` | Get webhook information |
| `stats` | Get webhook statistics |
| `test` | Test webhook delivery |

#### Configuration

```toml
# ~/.stellopay/config.toml
[network]
rpc_url = "https://soroban-testnet.stellar.org:443"
network_passphrase = "Test SDF Network ; September 2015"

[contract]
default_contract_id = "CONTRACT_ID_HERE"

[auth]
secret_key = "SECRET_KEY_HERE"
# Or use environment variable: STELLOPAY_SECRET_KEY

[defaults]
token = "TOKEN_ADDRESS_HERE"
frequency = "monthly"
```

#### Keeping Docs in Sync

The authoritative source for available commands is the `Commands` enum in `tools/cli/src/lib.rs`. To regenerate this reference after changing the CLI definition:

```bash
cargo run -p stellopay-cli -- --help
cargo run -p stellopay-cli webhook --help
```

## Phantom Commands (Not Implemented)

The following commands documented in earlier versions of this file **do not exist** in the current CLI:

- `payroll create / update / delete / list`
- `deposit`, `pay`, `bulk-pay`
- `contract deploy / initialize / pause / unpause / transfer-ownership`
- `token add / remove / list`
- `payment process / process-all / schedule / history`
- `report payroll / payments / balances`
- `analyze events / report`
- `debug transaction / trace / state / gas`
- `test setup / deploy / accounts / generate / run / report`
- `load-test`, `stress-test`, `benchmark`
- `generate bindings / client / docs / openapi / contract-docs`
- `health`, `stream`, `export`, `monitor`

These are aspirational features not yet implemented. If you need them, please open a feature request.

## Monitoring and Debugging Tools

For real-time event monitoring, query the contract via the Soroban RPC or a block explorer:

```bash
stellar contract id --id <CONTRACT_ID>
```

The CLI does not ship built-in `analyze`, `stream`, `debug`, or `monitor` subcommands.

## Testing Utilities

### Test Environment Setup

```bash
# Run unit tests
cargo test

# Run integration tests
cargo test --test integration_tests

# Run end-to-end tests (if configured)
npm test -- --testPathPattern=e2e
```

Existing test scripts are available at `scripts/test.sh`.

## Code Generation

Contract bindings can be generated using the Soroban CLI directly:

```bash
# Generate TypeScript bindings (requires soroban-cli)
soroban contract bindings typescript --contract-id <CONTRACT_ID> --output-dir ./src/bindings
```

The CLI does not ship a built-in `generate` subcommand.

## Development Scripts

Build, test, and monitoring scripts are available in the `scripts/` directory as shell scripts with descriptive comments.

## Getting Started

1. **Install CLI Tools**:
   ```bash
   cargo install stellopay-cli
   ```

2. **Set up Development Environment**:
   ```bash
   git clone https://github.com/stellopay/stellopay-core
   cd stellopay-core
   ```

3. **Deploy Test Contract**:
   ```bash
   stellopay-cli deploy --network testnet --owner <OWNER_ADDRESS>
   ```

4. **Run Tests**:
   ```bash
   cargo test
   ```

For detailed usage instructions, see the [Integration Guide](../integration/README.md).

> **Accuracy note:** This document was reconciled against the `Commands` enum in `tools/cli/src/lib.rs`. If the CLI gains new subcommands, update this file to match. Run `cargo run -p stellopay-cli -- --help` to verify.
