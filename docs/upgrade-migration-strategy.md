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


---

## Schema-Version Downgrade Guard (#851)

### Invariant

`migrate_state` enforces **monotonic version advancement**:

```
from_version == current_stored_version
```

This assertion is checked at the start of every `migrate_state` call. Because the migration bumps the stored version before returning, the same `from_version` can never be used twice.

### What is prevented

| Scenario | Result | Why it matters |
|---|---|---|
| `from_version < current_version` (downgrade) | Panic `"Invalid migration version"` | Prevents re-running an old migration against a newer schema, which could overwrite or corrupt v1+ data with v0 logic. |
| Repeated call with same `from_version` after successful migration | Panic `"Invalid migration version"` | The stored version has already been bumped; the call is equivalent to a downgrade. |
| `from_version > current_version` (future version) | Panic `"Invalid migration version"` | Prevents the operator from skipping migration steps. |
| Non-admin caller | Panic via `require_upgrade_admin` | Migrations are as privileged as upgrades. |

### Guard implementation

In `onchain/contracts/stello_pay_contract/src/lib.rs`:

```rust
pub fn migrate_state(env: Env, operator: Address, from_version: u32) {
    Self::require_upgrade_admin(&env, &operator);

    let current: u32 = env
        .storage()
        .persistent()
        .get(&StorageKey::ContractVersion)
        .unwrap_or(0u32);

    // Monotonicity guard: from_version must exactly match the stored version.
    // Any value lower (downgrade) or higher (skip) is rejected.
    assert!(from_version == current, "Invalid migration version");

    // Migration logic …
}
```

### Version progression

```
Legacy deployment  →  ContractVersion = 0 (unset, defaults to 0)
migrate_state(0)   →  ContractVersion = 1
migrate_state(1)   →  ContractVersion = 2  (when v1→v2 is added)
```

Attempting `migrate_state(0)` after the first migration has already run returns the `"Invalid migration version"` panic because the stored version is now `1`, not `0`.

### Regression Tests

All downgrade-guard scenarios are covered in `tests/upgrade_migration_tests.rs`:

| Test | Scenario | Expected outcome |
|---|---|---|
| `test_migrate_state_rejects_downgrade_from_version` | Run v0→v1, then call `migrate_state(from=0)` | Rejected (downgrade) |
| `test_migrate_state_same_version_repeated_call_is_rejected` | Two consecutive calls with `from=0` | Second call rejected |
| `test_migrate_state_forward_migration_updates_contract_version` | v0→v1 succeeds; follow-up call confirms version bumped | Version is now 1 |
| `test_migrate_state_rejects_non_admin_caller` | Non-admin calls `migrate_state` | Rejected (unauthorized) |
| `test_migrate_state_rejects_future_from_version` | `from_version = 999` on a fresh contract | Rejected (future version) |
| `test_migrate_state_forward_preserves_existing_agreements` | Create agreement, migrate v0→v1, verify data | Agreement intact |
