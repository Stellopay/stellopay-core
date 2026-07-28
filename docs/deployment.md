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

### 6. Rollback a Faulty Deployment

Soroban upgrades replace the contract WASM in-place via `env.deployer().update_current_contract_wasm(...)`. The contract ID and all persistent storage stay on-chain, so a rollback is another upgrade — you redeploy the previous known-good WASM hash.

> **Read this section before you need it.** Attempting a rollback under incident pressure without understanding the caveats below can cause irreversible data corruption.

#### 6.1 Identify the previous known-good WASM hash

Every deployed WASM is content-addressed. The hash is logged at deploy time in the `stellar contract deploy` output. Keep these records — a git tag, a deployment log, or a release notes entry — against the commit/release that produced each binary.

To find the hash of a prior release:

```bash
# Rebuild from the known-good git ref
git checkout <previous-release-tag>
cd onchain/contracts/<contract_name>
cargo build --target wasm32-unknown-unknown --release

# The hash stellar uses is the SHA-256 of the WASM bytes
stellar contract install \
  --wasm target/wasm32-unknown-unknown/release/<contract_name>.wasm \
  --source deployer \
  --network <network>
# outputs: <wasm_hash>
```

`stellar contract install` uploads the WASM if not already present and prints the hash. If the binary was already on-chain (which it will be if you deployed this version before) it returns the existing hash immediately without re-uploading.

#### 6.2 Redeploy the previous WASM

Use the same `upgrade` entrypoint that forward upgrades use:

```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source deployer \
  --network <network> \
  --func upgrade \
  -- \
  --new_wasm_hash <previous_wasm_hash> \
  --operator <deployer_address>
```

This replaces the live WASM and takes effect for all subsequent invocations. No re-initialization is needed.

Repeat for each affected contract. All 29 contracts in this workspace are independently upgradeable, so only roll back the ones that are misbehaving.

#### 6.3 Verify the rollback

Run the same post-deployment checks from step 5:

```bash
# Confirm the contract now runs the old code
stellar contract inspect --id <CONTRACT_ID>

# Smoke-test key read paths
stellar contract invoke --id <CONTRACT_ID> --func get_owner ...
```

#### 6.4 State migration after rollback

If the faulty version ran `migrate_state` and wrote a new `ContractVersion` to storage, the rolled-back WASM may encounter a version number it doesn't recognize. Check the current version before rolling back:

```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source deployer \
  --network <network> \
  --func get_contract_version   # if exposed, or read via storage inspection
```

If the version was bumped by the faulty release, you may need to manually reset it or write a one-off migration. Do not proceed with the rollback if you are unsure — assess the state first.

---

### ⚠️ Changes that make rollback unsafe

The following categories of change are **one-way**. Rolling back WASM after these changes will produce undefined behaviour or data corruption.

| Category | Why rollback is unsafe |
|---|---|
| **Storage key added and written** | The old WASM doesn't know the new key exists. Data written under the new key is silently ignored, which can cause stale or inconsistent state. |
| **Storage key type changed** | The old WASM will deserialize the value using the old type. This is undefined behaviour and will likely panic or silently corrupt state. |
| **Enum variant appended and persisted** | Soroban encodes enums by discriminant index. If the faulty version stored a value using the new variant, the old WASM will misinterpret the discriminant on read. This applies to any enum stored in persistent, instance, or temporary storage (e.g. `StorageKey`, `DisputeStatus`, `AgreementStatus`). |
| **`migrate_state` bumped `ContractVersion`** | The old WASM's migration logic will either panic (`"Unsupported migration version"`) or re-run a migration on data that has already been migrated, likely double-incrementing counters or overwriting state. |
| **Storage entry deleted** | Data removed by the faulty version is gone. The old WASM may expect that key to be present and panic. |
| **Cross-contract interface changed** | If the faulty version changed a function signature on a shared interface (e.g. `rbac-interface`, `milestone-interface`) and other contracts were upgraded to call it, rolling back only one side breaks the call. Rollback all affected contracts together or none. |

**Safe to roll back** (no caveats): pure logic fixes, fee calculation changes, event emission changes, access-control tightening — anything that doesn't touch the storage layout or cross-contract interfaces.

