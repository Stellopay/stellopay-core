# Payroll Smart Contract

This repository contains the smart contracts for the decentralized payroll system built on the Stellar blockchain using Soroban. The smart contracts manage payroll escrows, disburse salaries, and handle multi-currency swaps.

## CI/CD for Soroban Rust Smart Contracts

This repository uses GitHub Actions to automatically build and test Soroban Rust smart contracts on every push and pull request to the main branches.

### CI/CD Status

![CI](https://github.com/Stellopay/stellopay-core/actions/workflows/ci.yml/badge.svg)

### How it works
- On every push or pull request to `main` or `master`, the workflow:
  1. Checks out the code
  2. Sets up Rust
  3. Installs the Soroban CLI
  4. Builds the smart contracts in `onchain/contracts/stello_pay_contract`
  5. Runs all tests
- Build and test results are reported directly in pull requests.

### Requirements
- No manual setup is needed for CI/CD. All dependencies are handled in the workflow.
- To run tests locally, ensure you have Rust and the Soroban CLI installed:
  ```sh
  rustup install stable
  cargo install --locked --version 20.0.0-rc.1 soroban-cli
  ```

### Building on Windows (GNU / MinGW)
If you see **"export ordinal too large"** when running `cargo test` or `cargo build`, use **Option B**: build only the WASM on Windows and run tests in WSL or CI.

1. `rustup target add wasm32-unknown-unknown`
2. From repo root: `.\scripts\migrations\build_wasm_only.ps1`

WASM output: `onchain/contracts/stello_pay_contract/target/wasm32-unknown-unknown/release/stello_pay_contract.wasm`.  
See [docs/windows-build.md](../docs/windows-build.md) for Option A (MSVC) and test instructions.

### References
- [GitHub Actions Documentation](https://docs.github.com/en/actions)
- [Stellar Soroban Rust Docs](https://soroban.stellar.org/docs)

## Contracts

### Payroll Escrow Contract

The Payroll Escrow Contract allows employers to deposit salaries into an escrow account. The funds are released to employees at predefined intervals (weekly, monthly, etc.).

## Getting Started

To get started with the Payroll Smart Contract, follow the instructions below:

1. **Clone the repository:**

   ```bash
   git clone https://github.com/your-repo/payroll-smart-contract.git
   cd payroll-smart-contract
   ```

2. **Install dependencies:**

   Ensure you have Rust and Soroban SDK installed. Follow the [Soroban SDK installation guide](https://soroban.stellar.org/docs/getting-started/installation).

3. **Build the contract:**

   ```bash
   cargo build
   ```

   or

   ```bash 
   stellar contract build
   ```

4. **Run tests:**

   The project includes comprehensive tests for all contract functionality. To run the tests:

   ```bash
   # Run all tests
   cargo test

   # Run a specific test file
   cargo test test_payroll
   cargo test test_create_or_update_escrow

   # Run a specific test
   cargo test test_get_payroll_success
   ```

   The tests cover various scenarios including:
   - Creating and updating payroll escrows
   - Disbursing salaries
   - Employee withdrawals
   - Error cases and edge conditions
   - Multiple payment cycles
   - Boundary value testing

   Test snapshots are automatically generated in the `test_snapshots` directory when running tests. These snapshots help ensure contract behavior remains consistent across changes.

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.