---
title: Upgrade and Migration Strategy
---

# Upgrade and Migration Strategy

This page describes the admin-gated upgrade flow for `stello_pay_contract` and how to apply safe, versioned storage migrations before production deployments.

Upgrading a Soroban contract replaces the contract’s WASM while keeping the same contract ID and persistent storage. If the new WASM changes how existing storage keys are interpreted, the contract can silently corrupt existing state unless a migration is applied.

## Preconditions

1. You have a deployed RBAC contract (see [`docs/rbac.md`](./rbac.md)).
2. The payroll contract has been initialized (owner set).
3. The payroll contract has been linked to RBAC by the owner:

   - `set_rbac_contract(owner, rbac_contract_id)`

4. The upgrade operator address has the `Admin` role in RBAC.

When RBAC is configured, upgrades and migrations are gated to RBAC Admin. When RBAC is not configured, the contract falls back to owner-only authorization (legacy behavior).

## High-level flow

1. Backup state (pre-upgrade)
2. Build and install the new WASM (obtain the new WASM hash)
3. Apply storage migration (if needed)
4. Perform the upgrade
5. Verify post-upgrade
6. Roll back (only if verification fails)

The repository also has a general guide at [`docs/migrations.md`](./migrations.md) and helper scripts under `scripts/migrations/`.

## Storage versioning

`stello_pay_contract` maintains a persistent `ContractVersion` value.

- Legacy deployments start at version `0` (unset means `0`).
- `migrate_state(from_version)` requires `from_version` to match the currently stored version.
- A migration must be monotonic (no downgrades).

## When to add a migration

Add a migration step whenever a new contract version:

- Changes an existing `StorageKey` definition, meaning, or type.
- Changes any serialized structs/enums that are stored under existing keys.
- Changes how agreement mode, milestone state, or agreement state machine fields are interpreted.

Additive changes (new keys, new fields stored under new keys) typically do not require migration.

## Rollback

Rollback requires the previous WASM hash for the currently deployed contract.

If verification fails after upgrade:

1. Re-authorize the prior WASM hash.
2. Re-run `upgrade(previous_wasm_hash)` as RBAC Admin.

See the rollback section in [`docs/migrations.md`](./migrations.md) for CLI/script details.

## Security notes

- The upgrade and migration entrypoints require explicit authorization:
  - RBAC Admin when RBAC is configured.
  - Owner when RBAC is not configured.
- Operators should treat WASM hashes as immutable release artifacts.
- Always back up state before applying upgrades or migrations.

---
title: Upgrade and Migration Strategy
---

# Upgrade and Migration Strategy

This page describes the **admin-gated** upgrade flow for `stello_pay_contract` and how to apply safe, versioned storage migrations before production deployments.

Upgrading a Soroban contract replaces the contract’s WASM while keeping the **same contract ID** and **persistent storage**. If the new WASM changes how existing storage keys are interpreted, the contract can silently corrupt existing state unless a migration is applied.

## Preconditions

1. You have a deployed RBAC contract (see [`docs/rbac.md`](./rbac.md)).
2. The payroll contract has been initialized (owner set).
3. The payroll contract has been linked to RBAC by the owner:

   - `set_rbac_contract(owner, rbac_contract_id)`

4. The upgrade operator address has the `Admin` role in RBAC.

When RBAC is configured, upgrades and migrations are gated to **RBAC Admin**. When RBAC is not configured, the contract falls back to **owner-only** authorization (legacy behavior).

## High-level flow

1. Backup state (pre-upgrade)
2. Build and install the new WASM (obtain the new WASM hash)
3. Apply storage migration (if needed)
4. Perform the upgrade
5. Verify post-upgrade
6. Roll back (only if verification fails)

The repository also has a general guide at [`docs/migrations.md`](./migrations.md) and helper scripts under `scripts/migrations/`.

## Storage versioning

`stello_pay_contract` maintains a persistent `ContractVersion` value.

- Legacy deployments start at version `0` (unset means `0`).
- `migrate_state(from_version)` requires `from_version` to match the currently stored version.
- A migration must be monotonic (no downgrades).

## When to add a migration

Add a migration step whenever a new contract version:

- Changes an existing `StorageKey` definition, meaning, or type.
- Changes any serialized structs/enums that are stored under existing keys.
- Changes how agreement mode, milestone state, or agreement state machine fields are interpreted.

Additive changes (new keys, new fields stored under new keys) typically do not require migration.

## Rollback

Rollback requires the **previous WASM hash** for the currently deployed contract.

If verification fails after upgrade:

1. Re-authorize the prior WASM hash.
2. Re-run `upgrade(previous_wasm_hash)` as RBAC Admin.

See the rollback section in [`docs/migrations.md`](./migrations.md) for CLI/script details.

## Security notes

- The upgrade and migration entrypoints require explicit authorization:
  - RBAC Admin when RBAC is configured.
  - Owner when RBAC is not configured.
- Operators should treat WASM hashes as immutable release artifacts.
- Always back up state before applying upgrades or migrations.

