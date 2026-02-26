# Contract Migrations and Upgrades

This document describes how to upgrade StellopayCore contracts and migrate data safely, including procedures, rollback, and data compatibility.

## Table of Contents

1. [Overview](#overview)
2. [Prerequisites](#prerequisites)
3. [Migration Procedure](#migration-procedure)
4. [Data Compatibility](#data-compatibility)
5. [Rollback Procedures](#rollback-procedures)
6. [Testing Migration Scenarios](#testing-migration-scenarios)
7. [Multisig-Upgrade Flow](#multisig-upgrade-flow)

---

## Overview

StellopayCore uses **Soroban upgradeable contracts**. Upgrading replaces the contract’s WASM code while keeping the **same contract ID** and **existing persistent storage**. Only the contract owner (or the multisig, when configured) can authorize an upgrade.

- **Contracts that are upgradeable:** `stello_pay_contract` (main payroll), and optionally others that use the same pattern.
- **Scripts:** All migration scripts live under `scripts/migrations/`. See [scripts/migrations/README.md](../scripts/migrations/README.md) for a short usage guide.

---

## Prerequisites

- **Rust** and `wasm32-unknown-unknown` target.
- **Stellar CLI** (`stellar` or `soroban`) for build, install, and invoke.
- **Environment variables** (or equivalent):
  - `CONTRACT_ID` – existing payroll contract instance to upgrade.
  - `SOURCE_ACCOUNT` – owner secret key (or identity) for authorization and upgrade.
  - `NETWORK` – e.g. `testnet`, `mainnet`, `futurenet`.
  - `RPC_URL` – Soroban RPC URL (e.g. `https://soroban-testnet.stellar.org`).

---

## Migration Procedure

Follow these steps in order. Run all scripts from the repository root unless stated otherwise.

### Step 1: Backup state (pre-upgrade)

Export state and metadata so you can verify post-upgrade and roll back if needed.

```bash
export CONTRACT_ID="C..."      # Your payroll contract ID
export NETWORK=testnet
export RPC_URL="https://soroban-testnet.stellar.org"
export BACKUP_DIR="./backups/$(date +%Y%m%d_%H%M%S)"

./scripts/migrations/01_backup_state.sh
```

- **Important:** Record the **current WASM hash** for this contract (from deploy records or network) and store it with the backup. You need it for rollback.

### Step 2: Build and install new WASM

Build the new contract and install it on the network to obtain the new WASM hash.

```bash
export SOURCE_ACCOUNT="S..."   # Owner key (needed for install)

./scripts/migrations/02_build_and_install_wasm.sh
```

- The script writes `NEW_WASM_HASH` to `scripts/migrations/.env.migration`. Do not commit this file.

### Step 3: Authorize and perform upgrade

Using the owner account, invoke the contract’s upgrade function with the new WASM hash. The contract enforces owner auth and then calls `deployer().update_current_contract_wasm(new_wasm_hash)`.

```bash
source ./scripts/migrations/.env.migration
./scripts/migrations/03_authorize_and_upgrade.sh "$NEW_WASM_HASH"
```

- If your CLI uses different invoke syntax (e.g. `--arg`), adjust the script or run the equivalent `upgrade(new_wasm_hash)` invoke manually.

### Step 4: Verify post-upgrade

Run basic checks to ensure the contract is invokable and behavior is correct.

```bash
./scripts/migrations/04_verify_post_upgrade.sh
```

- Manually confirm owner, active agreements, and at least one critical path (e.g. a read or a small disburse) if possible.

---

## Data Compatibility

### Storage layout and persistence

- Upgrades **do not** clear or migrate storage. Existing persistent and instance storage keys are left unchanged.
- **Compatible upgrades** are those that do not change the meaning or layout of existing storage keys. New code must continue to read/write the same keys and types as before (or only add new keys).

### Storage keys (main payroll contract)

The main contract uses two key namespaces:

- **`StorageKey`** (e.g. in `storage.rs`): `Owner`, `Agreement(u128)`, `AgreementEmployees(u128)`, `NextAgreementId`, `EmployerAgreements(Address)`, `DisputeStatus(u128)`, `DisputeRaisedAt(u128)`, `Arbiter`.
- **`DataKey`**: agreement/employee counts, employee addresses, salaries, claimed periods, activation time, period duration, token, paid amount, escrow balance.

**Compatibility rules:**

- Do **not** remove or rename existing enum variants or change the type of values stored under existing keys.
- Adding new `StorageKey` or `DataKey` variants and writing new keys is safe.
- Changing the Rust type (or Soroban contract type) of an existing key can break reading existing data; treat such changes as breaking and consider a one-off migration (e.g. a contract function that rewrites data once, guarded by a “migrated” flag).

### Cross-contract references

- Contract **addresses** (e.g. payroll, escrow, payment history, multisig) do not change on upgrade of a single contract. Only the code (WASM) of the upgraded contract changes.
- Ensure new code remains compatible with any external contracts that call this one or rely on its event shapes.

### Versioning and events

- If you add a **version** or **migration** field in storage, document it in the release notes and in this doc.
- Event schemas: adding new event topics/fields is backward compatible; changing or removing existing ones can break indexers and integrations.

---

## Rollback Procedures

If an upgrade causes incorrect behavior or failures:

1. **Stop** using the upgraded contract for critical operations if possible.
2. **Locate** the last known-good WASM hash (from backup or deploy records).
3. **Run the rollback script** with that hash (owner must sign):

   ```bash
   export CONTRACT_ID="C..."
   export SOURCE_ACCOUNT="S..."
   ./scripts/migrations/rollback.sh "<PREVIOUS_WASM_HASH>"
   ```

4. **Verify** with `04_verify_post_upgrade.sh` and manual checks.
5. **Investigate** the failed upgrade (tests, staging, storage layout) before re-attempting.

Rollback is simply another upgrade to the previous WASM; no separate “downgrade” RPC exists. Storage is unchanged by both upgrade and rollback.

---

## Testing Migration Scenarios

### Unit / integration tests (in-repo)

- **Upgrade and data persistence:** `onchain/contracts/stello_pay_contract/src/tests/test_upgrade.rs` includes tests that upgrade the mock contract and assert that agreement, employee, balance, and settings data persist.
- Run before and after changing contract code or migration scripts:

  ```bash
  cd onchain/contracts/stello_pay_contract
  cargo test
  ```

### Staging / testnet checklist

1. Deploy the **current** production version (or current testnet version) and record its WASM hash.
2. Create agreements, employees, and balances; optionally process a few claims.
3. Run **01_backup_state.sh** (and any CLI export you use) and store the backup.
4. Build and install the **new** WASM with **02_build_and_install_wasm.sh**.
5. Run **03_authorize_and_upgrade.sh** with the new hash.
6. Run **04_verify_post_upgrade.sh** and verify:
   - Owner unchanged.
   - Agreements and employees readable and consistent with backup.
   - One or more critical flows (e.g. claim, disburse) work.
7. Optionally run **rollback.sh** with the old hash, then verify again and re-upgrade to confirm the rollback path.

### CI

- Existing CI builds and runs tests for the payroll contract. Extend CI as needed to run migration-related tests (e.g. upgrade + persistence) on every change.

---

## Multisig-Upgrade Flow

When upgrades are gated by the **multisig** contract:

1. **Propose:** A signer proposes a `ContractUpgrade(payroll_contract_id, new_wasm_hash)` operation via `propose_operation`.
2. **Approve:** Other signers call `approve_operation` until the threshold is met (or the emergency guardian executes).
3. **Execute:** Once the operation is executed on-chain, off-chain tooling (or the owner) must perform the actual upgrade by invoking the payroll contract’s `upgrade(new_wasm_hash)` with the **owner** account (or as required by the contract). The multisig does not replace the need for the contract’s own upgrade authorization; it only records approval for the hash.
4. **Verify:** Run **04_verify_post_upgrade.sh** and your usual checks after the upgrade.

See [multisig.md](./multisig.md) for multisig API and workflow details.

---

## Summary

| Step | Script | Purpose |
|------|--------|--------|
| 1 | `01_backup_state.sh` | Backup state and record current WASM hash |
| 2 | `02_build_and_install_wasm.sh` | Build new WASM, install on network, get new hash |
| 3 | `03_authorize_and_upgrade.sh` | Owner authorizes and runs upgrade (same contract ID) |
| 4 | `04_verify_post_upgrade.sh` | Basic post-upgrade verification |
| Rollback | `rollback.sh` | Upgrade back to a previous WASM hash |

- **Data compatibility:** Preserve existing storage key layout and types across upgrades; document any one-off migrations.
- **Rollback:** Always keep the previous WASM hash and backup; rollback is an upgrade to that hash.
- **Testing:** Use in-repo upgrade tests and a testnet/staging run before production upgrades.
