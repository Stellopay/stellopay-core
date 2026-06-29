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

