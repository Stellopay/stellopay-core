# StellopayCore Documentation

Welcome to the comprehensive documentation for the StellopayCore smart contract - a decentralized payroll system built on the Stellar blockchain using Soroban.

## Table of Contents

1. [API Documentation](./api/README.md) - Complete function reference
2. [Integration Guide](./integration/README.md) - How to integrate with the contract
3. [Best Practices](./best-practices/README.md) - Recommended patterns and practices
4. [Examples](./examples/README.md) - Common use cases and code examples
5. [Developer Tools](./dev-tools/README.md) - CLI tools and utilities
6. [Architecture](./architecture/README.md) - System design and architecture
7. [Migrations](./migrations.md) - Contract upgrade procedures, rollback, and data compatibility
8. [Building on Windows](./windows-build.md) - Fixing "export ordinal too large" (MinGW) and WASM-only build

## Quick Start

```rust
// Initialize contract
let contract = PayrollContract::new(&env);
contract.initialize(&env, &owner_address);

// Create payroll for an employee
let payroll = contract.create_or_update_escrow(
    &env,
    &employer_address,
    &employee_address,
    &token_address,
    &amount,
    &interval,
    &recurrence_frequency,
)?;

// Deposit tokens for salary payments
contract.deposit_tokens(&env, &employer_address, &token_address, &amount)?;

// Disburse salary
contract.disburse_salary(&env, &employer_address, &employee_address)?;
```

## Key Features

- **Automated Payroll Management**: Schedule and automate salary disbursements
- **Multi-Token Support**: Support for any Stellar asset
- **Recurring Payments**: Configurable payment intervals
- **Pause/Unpause**: Emergency controls for contract operations
- **Employee Self-Service**: Employees can withdraw their own salaries
- **Bulk Operations**: Process multiple payments in a single transaction
- **Comprehensive Events**: Full event logging for monitoring and analytics

## 🛠️ Developer Tools

StellopayCore provides comprehensive developer tools to help developers:

### CLI Tools
- **Contract Management** - Deploy, initialize, and manage contracts
- **Payroll Operations** - Create, update, and delete payroll entries
- **Payment Processing** - Process individual and bulk payments
- **Monitoring & Analytics** - Track contract performance and metrics

### Getting Started
```bash
# Install CLI tool
cargo install stellopay-cli
```

See the [Developer Tools](/docs/dev-tools/README.md) and [Integration Guide](/docs/integration/README.md) for detailed instructions.

## Getting Help

- Check the [API Documentation](./api/README.md) for detailed function references
- Review [Common Issues](./troubleshooting/README.md) for solutions to frequent problems
- See [Examples](./examples/README.md) for practical implementations
- Join our [Discord](https://discord.gg/stellopay) for community support

## Contributing

We welcome contributions! Please see our [Contributing Guide](../CONTRIBUTING.md) for details on how to contribute to the documentation.
