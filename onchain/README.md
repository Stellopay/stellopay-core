# Payroll Smart Contract

This repository contains the smart contracts for the decentralized payroll system built on the Stellar blockchain using Soroban. The smart contracts manage payroll escrows, disburse salaries, and handle multi-currency swaps.

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

4. **Run tests:**

   ```bash
   cargo test
   ```

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
```` ▋