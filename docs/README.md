# StellopayCore Documentation

Welcome to the comprehensive documentation for the StellopayCore smart contract - a decentralized payroll system built on the Stellar blockchain using Soroban.

## Table of Contents

1. [API Documentation](./api/README.md) - Complete function reference
2. [Integration Guide](./integration/README.md) - How to integrate with the contract
3. [Best Practices](./best-practices/README.md) - Recommended patterns and practices
4. [Examples](./examples/README.md) - Common use cases and code examples
5. [Developer Tools](./dev-tools/README.md) - CLI tools and utilities
6. [Architecture](./architecture.md) - System design and architecture
7. [Build Targets](./build-targets.md) - WASM target rationale (`wasm32-unknown-unknown`)
8. [CI Pipeline](./ci.md) - Contracts CI workflow, coverage, and local environment
9. [Deployment](./deployment.md) - Deploy contracts to testnet/mainnet
10. [Benchmarks](./benchmarks.md) - Soroban cost benchmarks and regression guarding
11. [Migrations](./migrations.md) - Contract upgrade procedures, rollback, and data compatibility
12. [Upgrade & migration strategy](./upgrade-migration-strategy.md) - RBAC-admin-gated upgrades and `migrate_state`
13. [Building on Windows](./windows-build.md) - Fixing "export ordinal too large" (MinGW) and WASM-only build

## Additional Key Topics

- [Audit Logger Integration](./audit-logger-integration.md)
- [Automated Compliance](./automated-compliance.md)
- [Batch Creation](./batch-creation.md)
- [Batch Payments](./batch-payments.md)
- [Chaos Testing](./chaos-testing.md)
- [Compliance Reporting Schema](./compliance-reporting-schema.md)
- [Conditional Triggers](./conditional-triggers.md)
- [Emergency Pause Quick Reference](./emergency-pause-quick-reference.md)
- [Employee Lifecycle Implementation](./employee-lifecycle-implementation.md)
- [Encrypted Backup Recovery](./encrypted-backup-recovery.md)
- [Event Indexing](./event-indexing.md)
- [Gas Optimization Summary](./gas-optimization-summary.md)
- [Grace Period](./grace-period.md)
- [Integration Guide](./integration/comprehensive-guide.md)
- [Invariant Assertions](./invariant-assertions.md)
- [Invariants](./invariants.md)
- [Load Testing](./load-testing.md)
- [Multi-Currency](./multi-currency.md)
- [Payment Scheduling](./payment-scheduling.md)
- [Rate Limiting](./rate-limiting.md)
- [Reporting Compliance Summary](./reporting_compliance_summary.md)
- [Snapshot Testing](./snapshot-testing.md)
- [State Machines](./state-machines.md)
- [Storage Optimization Summary](./storage-optimization-summary.md)

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

We welcome contributions. See [CONTRIBUTING.md](../CONTRIBUTING.md) for the build and test workflow, and [SECURITY.md](../SECURITY.md) for vulnerability reporting.
