# Developer Tools

This page documents the developer tools that exist in this repository today.
The CLI command list is reconciled with `tools/cli/src/lib.rs`, where the
`Commands` and `WebhookCommands` enums are defined.

## Stellopay CLI

The CLI crate lives in [tools/cli](../../tools/cli/README.md). Build or install
it from source:

```sh
cd tools/cli
cargo build
cargo install --path .
```

Global flags:

| Flag | Description |
| --- | --- |
| `-c, --config <PATH>` | Configuration file path. Defaults to `~/.stellopay/config.toml`. |
| `-v, --verbose` | Enables verbose logging. |

Configuration example:

```toml
[network]
rpc_url = "https://soroban-testnet.stellar.org:443"
network_passphrase = "Test SDF Network ; September 2015"

[contract]
default_contract_id = "CONTRACT_ID_HERE"

[auth]
secret_key = "SECRET_KEY_HERE"

[defaults]
token = "TOKEN_ADDRESS_HERE"
frequency = "monthly"
```

Do not commit real secret keys, webhook secrets, RPC credentials, or production
contract identifiers in config files or shell history.

## Supported Commands

Only these top-level commands are currently defined in `tools/cli/src/lib.rs`.

| Command | Purpose |
| --- | --- |
| `deploy` | Deploy a new contract. |
| `info` | Get contract information. |
| `status` | Show CLI status and environment checks. |
| `emergency-withdraw` | Run the emergency withdrawal path. |
| `webhook` | Manage webhook registrations and test delivery. |

### `deploy`

```sh
stellopay-cli deploy \
  --network testnet \
  --owner <OWNER_ADDRESS> \
  --wasm <PATH_TO_WASM>
```

Flags:

| Flag | Required | Description |
| --- | --- | --- |
| `--network <NETWORK>` | No | Network name. Defaults to `testnet`. |
| `--owner <ADDRESS>` | Yes | Owner address for the deployed contract. |
| `--wasm <PATH>` | No | WASM file path. |

### `info`

```sh
stellopay-cli info --contract-id <CONTRACT_ID>
```

Flags:

| Flag | Required | Description |
| --- | --- | --- |
| `--contract-id <ID>` | No | Contract ID to inspect. Falls back to config where supported. |

### `status`

```sh
stellopay-cli status
```

`status` reads the configured environment and reports CLI/tooling status.

### `emergency-withdraw`

```sh
stellopay-cli emergency-withdraw \
  --contract-id <CONTRACT_ID> \
  --token <TOKEN_ADDRESS> \
  --recipient <RECIPIENT_ADDRESS> \
  --amount <AMOUNT>
```

Flags:

| Flag | Required | Description |
| --- | --- | --- |
| `--contract-id <ID>` | Yes | Contract ID for the withdrawal target. |
| `--token <ADDRESS>` | Yes | Token contract address. |
| `--recipient <ADDRESS>` | Yes | Recipient address. |
| `--amount <I128>` | Yes | Amount to withdraw. |

Use placeholder values in shared examples. Do not paste real emergency
withdrawal destinations, private keys, or production amounts into public logs.

## Webhook Commands

Webhook commands are nested under `stellopay-cli webhook`.

### `webhook register`

```sh
stellopay-cli webhook register \
  --name "Payroll events" \
  --description "Receives payroll lifecycle events" \
  --url "https://example.com/webhooks/stellopay" \
  --events "payroll.created,payment.sent" \
  --secret "$WEBHOOK_SECRET" \
  --contract-id <CONTRACT_ID>
```

Flags:

| Flag | Required | Description |
| --- | --- | --- |
| `--name <NAME>` | Yes | Webhook name. |
| `--description <TEXT>` | Yes | Webhook description. |
| `--url <URL>` | Yes | Delivery URL. |
| `--events <CSV>` | Yes | Comma-separated event names. |
| `--secret <SECRET>` | Yes | Webhook signing secret. Prefer an environment variable. |
| `--contract-id <ID>` | No | Contract ID override. |

### `webhook update`

```sh
stellopay-cli webhook update \
  --webhook-id <WEBHOOK_ID> \
  --name "Updated payroll events" \
  --description "Updated description" \
  --url "https://example.com/webhooks/stellopay" \
  --events "payment.sent" \
  --active true \
  --contract-id <CONTRACT_ID>
```

All fields except `--webhook-id` are optional updates.

### `webhook delete`

```sh
stellopay-cli webhook delete --webhook-id <WEBHOOK_ID> --contract-id <CONTRACT_ID>
```

### `webhook list`

```sh
stellopay-cli webhook list --owner <OWNER_ADDRESS> --contract-id <CONTRACT_ID>
```

### `webhook get`

```sh
stellopay-cli webhook get --webhook-id <WEBHOOK_ID> --contract-id <CONTRACT_ID>
```

### `webhook stats`

```sh
stellopay-cli webhook stats --contract-id <CONTRACT_ID>
```

### `webhook test`

```sh
stellopay-cli webhook test \
  --webhook-id <WEBHOOK_ID> \
  --event-type "payment.sent" \
  --contract-id <CONTRACT_ID>
```

## Commands Not Yet Exposed

The current `Commands` enum does not expose payroll creation, deposits,
payments, bulk payments, monitoring, event analysis, debug tracing, load tests,
or code generation subcommands. Add those to `tools/cli/src/lib.rs` first before
documenting user-facing syntax here.

## Keeping This Page In Sync

When CLI commands change:

1. Update `tools/cli/src/lib.rs`.
2. Run `cd tools/cli && cargo run -- --help` and, for nested commands,
   `cd tools/cli && cargo run -- webhook --help`.
3. Reconcile this page with the emitted help text.
4. Keep examples copy-pasteable with placeholders only.

This page should describe the CLI that exists on the current branch, not a
future command surface.
