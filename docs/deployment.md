## Deployment Guide

This guide provides a minimal, step‑by‑step overview for deploying Stellopay contracts to Soroban‑enabled Stellar networks using the `stellar` CLI.

It is focused on **practical steps** rather than exhaustive coverage.

---

### Prerequisites

- Rust toolchain installed (matching the version used in CI)
- `stellar` CLI installed (see Stellar docs)
- Access to a funded account on the target network (Futurenet/Testnet/Mainnet)

Ensure you can run:

```bash
stellar --version
```

---

### 1. Build the Contract

From the repository root:

```bash
cd onchain/contracts/stello_pay_contract

# Build Rust target
cargo build --target wasm32-unknown-unknown --release

# Or use the CLI helper (mirrors CI)
stellar contract build --verbose
```

The compiled WASM artifact will be placed under `target/wasm32-unknown-unknown/release/`.

---

### 2. Configure Network and Account

Set the active network and default account in the `stellar` CLI:

```bash
# Example: Futurenet (adjust as needed)
stellar network add futurenet \
  --rpc-url https://rpc-futurenet.stellar.org \
  --friendbot-url https://friendbot-futurenet.stellar.org

stellar network use futurenet

# Import or generate a keypair
stellar keys generate deployer
stellar account fund deployer --network futurenet
```

For Testnet/Mainnet, use the appropriate RPC endpoints and fund accounts via a faucet (Testnet) or normal funding flows (Mainnet).

---

### 3. Deploy the Contract

Deploy the compiled WASM to the selected network:

```bash
CONTRACT_WASM=./target/wasm32-unknown-unknown/release/stello_pay_contract.wasm

stellar contract deploy \
  --wasm $CONTRACT_WASM \
  --source deployer \
  --network futurenet
```

The command will output a **contract ID**; record it for subsequent interactions and configuration.

Repeat similar steps for other contracts (e.g., `payment_history`, `bonus_system`, `multisig`, `token_vesting`, `payment_scheduler`) by changing the contract directory and WASM path.

---

### 4. Initialize and Configure

Most contracts require an explicit initialization call:

```bash
# Example: initialize payroll contract owner
stellar contract invoke \
  --id <PAYROLL_CONTRACT_ID> \
  --source deployer \
  --network futurenet \
  --func initialize \
  --arg address:deployer
```

For auxiliary contracts (escrow, history, bonus system, multisig, vesting, scheduler), follow a similar pattern:

- call `initialize(...)` with the appropriate admin/owner addresses
- configure any linked contract addresses as required by their public API

---

### 5. Verification and Post‑Deployment Checks

After deployment and initialization:

- **Contract presence**
  - Use `stellar contract inspect --id <CONTRACT_ID>` to verify that the contract is registered.
- **Basic read calls**
  - Call simple getters (e.g., `get_owner`, `get_agreement`, `get_employer_payment_count`) to confirm storage is initialized correctly.
- **Test a minimal workflow**
  - On a non‑production network, run a small end‑to‑end scenario:
    - create a payroll or escrow agreement
    - fund escrow and perform a claim
    - verify events and state transitions

Keeping these checks small but systematic helps ensure that deployments behave the same way as your local tests and CI.

---

### 6. Rollback (if a deployed version misbehaves)

Stellopay uses Soroban **upgradeable** contracts: an upgrade swaps the contract's WASM but keeps the **same contract ID** and existing persistent storage. Rollback is therefore just *another upgrade* to the previous known-good WASM — there is no separate "downgrade" RPC. See [Migrations](./migrations.md#rollback-procedures) and [Contract Upgrade Entrypoint](./upgrade-entrypoint.md) for the full access-control and safety model.

#### 6.1 Identify the previous known-good version/hash

You need the WASM hash of the last good deployment:

- **At deploy time (recommended):** record the WASM hash produced by `stellar contract build` and the contract ID printed by `stellar contract deploy` (step 3). Keep this alongside the deploy record.
- **From a backup:** the migration helper `scripts/migrations/01_backup_state.sh` records the current WASM hash with the backup — use that if you followed [Migrations](./migrations.md).
- **Live on-chain:** query the currently installed WASM hash for a contract ID, e.g.

  ```bash
  stellar contract inspect --id <CONTRACT_ID>
  # or, depending on CLI version:
  stellar contract info --id <CONTRACT_ID>
  ```

  This tells you the *current* (possibly bad) hash; pair it with your deploy history to find the prior good one.

#### 6.2 Redeploy / roll back

Rollback re-installs the previous WASM and calls the contract's `upgrade(<PREVIOUS_WASM_HASH>)` entrypoint. Authorization follows [upgrade-entrypoint.md](./upgrade-entrypoint.md): the caller must be the stored `Owner` (or a `Role::Admin` in the linked RBAC contract) and must `require_auth()`.

```bash
# Option A — migration helper (upgrades to the given hash)
export CONTRACT_ID="C..."
export SOURCE_ACCOUNT="S..."   # owner / admin key
./scripts/migrations/rollback.sh "<PREVIOUS_WASM_HASH>"

# Option B — manual invoke
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <SOURCE_ACCOUNT> \
  --network <NETWORK> \
  --func upgrade \
  --arg new_wasm_hash:<PREVIOUS_WASM_HASH>
```

Then re-run the post-deployment checks from step 5 (`stellar contract inspect`, simple getters, a small end-to-end flow on a non-production network) to confirm behavior is restored.

#### 6.3 Storage-migration caveats (when rollback is unsafe)

Rollback only swaps **code**; it does **not** rewind **data**. If the bad version already wrote or read state under a changed layout, reverting the code alone will not restore correctness and may break reads. Treat rollback as unsafe (or insufficient) when the deployed change involved any of:

- **Appended-but-referenced enum variants** — e.g. a new `PayrollError`, `DataKey`, or `StorageKey` variant that the bad code already stored or matched on. Reverting the code removes the variant, so any data written under it can no longer be decoded/matched.
- **Storage key changes** — renamed, removed, or **retyped** `StorageKey`/`DataKey` variants, or a changed value type under an existing key. Old data may become unreadable or misinterpreted.
- **One-off data migrations** — if the bad version executed a migration guarded by a "migrated" flag, the flag/data change persists after code rollback; re-running the new version may double-apply or conflict.
- **Changed event schemas** — removals/renames of event topics or fields that downstream indexers depend on (indexers are not rolled back with the contract).

In those cases, prefer a **forward fix** (a new compatible version that repairs the data) over a pure code rollback, and rehearse the recovery on testnet/staging first.

> See also [Migrations → Data Compatibility](./migrations.md#data-compatibility) for the storage-key rules that govern what is safe to upgrade or roll back.  

