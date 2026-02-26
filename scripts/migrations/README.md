# Migration Scripts

Migration scripts for upgrading StellopayCore contracts and migrating data safely.

## Overview

- **01_backup_state.sh** — Export contract state and config before upgrade (pre-migration backup).
- **02_build_and_install_wasm.sh** — Build new WASM and install it on the network (returns WASM hash).
- **03_authorize_and_upgrade.sh** — Authorize upgrade (owner or via multisig) and perform contract upgrade.
- **04_verify_post_upgrade.sh** — Run post-upgrade checks (owner, version, critical reads).
- **rollback.sh** — Rollback procedure: re-deploy previous WASM and re-authorize upgrade.
- **run_migration_tests.sh** — Run in-repo upgrade and data-persistence tests (see [Testing](#testing)).
- **build_wasm_only.ps1** — (Windows) Build contract WASM only; use when host build fails with "export ordinal too large". See [docs/windows-build.md](../../docs/windows-build.md).

See [docs/migrations.md](../../docs/migrations.md) for full procedures, data compatibility, and rollback.

## Prerequisites

- Rust, `wasm32-unknown-unknown` target, and Stellar CLI (`stellar` or `soroban`).
- Environment variables (or flags) for network, source account, and contract ID.

## Quick Run (testnet)

```bash
# Set once
export NETWORK=testnet
export RPC_URL="https://soroban-testnet.stellar.org"
export SOURCE_ACCOUNT="S..."   # Owner key for authorize + upgrade
export CONTRACT_ID="C..."      # Existing payroll contract to upgrade
export BACKUP_DIR="./backups/$(date +%Y%m%d_%H%M%S)"

# 1) Backup
./scripts/migrations/01_backup_state.sh

# 2) Build and install new WASM (from repo root)
./scripts/migrations/02_build_and_install_wasm.sh
# → sets NEW_WASM_HASH in ./scripts/migrations/.env.migration

# 3) Authorize and upgrade
source ./scripts/migrations/.env.migration 2>/dev/null || true
./scripts/migrations/03_authorize_and_upgrade.sh "$NEW_WASM_HASH"

# 4) Verify
./scripts/migrations/04_verify_post_upgrade.sh
```

## Rollback

If something goes wrong after upgrade:

```bash
# Use the WASM hash from your pre-upgrade backup (e.g. PREVIOUS_WASM_HASH)
./scripts/migrations/rollback.sh "$PREVIOUS_WASM_HASH"
```

Always keep a backup of the last known-good WASM hash and state before upgrading.

## Testing

Run migration-related unit and integration tests before upgrading:

```bash
./scripts/migrations/run_migration_tests.sh
```

This runs the payroll contract tests (including upgrade and data-persistence tests in `test_upgrade.rs`) and, when available, integration tests. See [docs/migrations.md](../../docs/migrations.md#testing-migration-scenarios) for the full testing checklist.
