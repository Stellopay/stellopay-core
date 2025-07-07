# Developer Tools

Comprehensive set of tools and utilities for working with the StellopayCore contract.

## Table of Contents

1. [CLI Tools](#cli-tools)
2. [Monitoring and Debugging Tools](#monitoring-and-debugging-tools)
3. [Testing Utilities](#testing-utilities)
4. [Code Generation](#code-generation)
5. [Development Scripts](#development-scripts)

## CLI Tools

### StellopayCore CLI

A command-line interface for managing payroll operations.

#### Installation

```bash
# Install from source
git clone https://github.com/stellopay/stellopay-core
cd stellopay-core/tools/cli
cargo install --path .

# Or install from registry
cargo install stellopay-cli
```

#### Usage

```bash
# Initialize a new payroll contract
stellopay-cli deploy --network testnet --owner <OWNER_ADDRESS>

# Create employee payroll
stellopay-cli payroll create \
  --contract <CONTRACT_ID> \
  --employee <EMPLOYEE_ADDRESS> \
  --salary 5000 \
  --frequency monthly \
  --token <TOKEN_ADDRESS>

# Deposit funds
stellopay-cli deposit \
  --contract <CONTRACT_ID> \
  --amount 50000 \
  --token <TOKEN_ADDRESS>

# Process payments
stellopay-cli pay \
  --contract <CONTRACT_ID> \
  --employee <EMPLOYEE_ADDRESS>

# Bulk operations
stellopay-cli bulk-pay \
  --contract <CONTRACT_ID> \
  --employees employees.json

# Monitor contract
stellopay-cli monitor \
  --contract <CONTRACT_ID> \
  --watch

# Get contract info
stellopay-cli info \
  --contract <CONTRACT_ID>
```

#### Configuration

```toml
# ~/.stellopay/config.toml
[network]
rpc_url = "https://soroban-testnet.stellar.org:443"
network_passphrase = "Test SDF Network ; September 2015"

[contract]
default_contract_id = "CONTRACT_ID_HERE"

[auth]
secret_key = "SECRET_KEY_HERE"
# Or use environment variable: STELLOPAY_SECRET_KEY

[defaults]
token = "TOKEN_ADDRESS_HERE"
frequency = "monthly"
```

### Contract Management Commands

```bash
# Contract deployment and management
stellopay-cli contract deploy [OPTIONS]
stellopay-cli contract initialize --owner <ADDRESS>
stellopay-cli contract pause
stellopay-cli contract unpause
stellopay-cli contract transfer-ownership --new-owner <ADDRESS>

# Token management
stellopay-cli token add --address <TOKEN_ADDRESS>
stellopay-cli token remove --address <TOKEN_ADDRESS>
stellopay-cli token list

# Payroll management
stellopay-cli payroll list
stellopay-cli payroll create [OPTIONS]
stellopay-cli payroll update [OPTIONS]
stellopay-cli payroll delete --employee <ADDRESS>

# Payment operations
stellopay-cli payment process --employee <ADDRESS>
stellopay-cli payment process-all
stellopay-cli payment schedule --employee <ADDRESS> --when <TIMESTAMP>
stellopay-cli payment history --employee <ADDRESS>

# Reporting
stellopay-cli report payroll --format json
stellopay-cli report payments --from <DATE> --to <DATE>
stellopay-cli report balances
```


### Event Log Analyzer

```bash
# Analyze contract events
stellopay-cli analyze events --contract <CONTRACT_ID> --from <DATE>

# Generate reports
stellopay-cli analyze report --format json --output report.json

# Real-time event streaming
stellopay-cli stream events --contract <CONTRACT_ID>
```

### Debug Tools

```bash
# Debug transaction
stellopay-cli debug transaction <TRANSACTION_HASH>

# Trace contract calls
stellopay-cli debug trace --contract <CONTRACT_ID> --function <FUNCTION_NAME>

# Contract state inspection
stellopay-cli debug state --contract <CONTRACT_ID>

# Gas usage analysis
stellopay-cli debug gas --contract <CONTRACT_ID> --function <FUNCTION_NAME>
```

## Testing Utilities

### Test Environment Setup

```bash
# Create test environment
stellopay-cli test setup --network testnet

# Deploy test contract
stellopay-cli test deploy

# Create test accounts
stellopay-cli test accounts create --count 5

# Fund test accounts
stellopay-cli test accounts fund --all

# Create test tokens
stellopay-cli test tokens create --symbol USDC --name "USD Coin"
```

### Test Data Generator

```bash
# Generate test payroll data
stellopay-cli test generate payrolls --count 10 --output test-payrolls.json

# Generate test employees
stellopay-cli test generate employees --count 50 --output test-employees.json

# Generate test scenarios
stellopay-cli test generate scenarios --type regression --output test-scenarios.json
```

### Integration Test Runner

```bash
# Run integration tests
stellopay-cli test run --suite integration

# Run specific test
stellopay-cli test run --test payroll_lifecycle

# Run performance tests
stellopay-cli test run --suite performance --duration 5m

# Generate test report
stellopay-cli test report --format html --output test-report.html
```

### Load Testing

```bash
# Run load tests
stellopay-cli load-test --contract <CONTRACT_ID> --duration 5m --rate 10/s

# Stress test
stellopay-cli stress-test --contract <CONTRACT_ID> --concurrent 50 --duration 2m

# Benchmark operations
stellopay-cli benchmark --contract <CONTRACT_ID> --operation disburse_salary
```

## Code Generation

### Contract Bindings Generator

```bash
# Generate TypeScript bindings
stellopay-cli generate bindings --language typescript --output ./src/bindings

# Generate Python bindings
stellopay-cli generate bindings --language python --output ./bindings

# Generate Rust bindings
stellopay-cli generate bindings --language rust --output ./src/bindings.rs
```

### Client Code Generator

```bash
# Generate client code
stellopay-cli generate client --language typescript --template react
stellopay-cli generate client --language python --template fastapi
stellopay-cli generate client --language rust --template tokio
```

### Documentation Generator

```bash
# Generate API documentation
stellopay-cli generate docs --format markdown --output ./docs/api

# Generate OpenAPI spec
stellopay-cli generate openapi --output ./api-spec.yaml

# Generate contract documentation
stellopay-cli generate contract-docs --format html --output ./contract-docs
```

## Development Scripts

### Build and Deployment Scripts

```bash
#!/bin/bash
# scripts/deploy.sh

set -e

echo "Building contract..."
cd onchain/contracts/stello_pay_contract
soroban contract build

echo "Deploying to testnet..."
CONTRACT_ID=$(soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/stello_pay_contract.wasm \
  --source-account $DEPLOY_ACCOUNT \
  --rpc-url https://soroban-testnet.stellar.org:443 \
  --network testnet)

echo "Contract deployed: $CONTRACT_ID"

echo "Initializing contract..."
soroban contract invoke \
  --id $CONTRACT_ID \
  --source-account $DEPLOY_ACCOUNT \
  --rpc-url https://soroban-testnet.stellar.org:443 \
  --network testnet \
  -- initialize \
  --owner $OWNER_ADDRESS

echo "Contract initialized successfully!"
echo "Contract ID: $CONTRACT_ID"
```

### Testing Scripts

```bash
#!/bin/bash
# scripts/test.sh

set -e

echo "Running unit tests..."
cd onchain/contracts/stello_pay_contract
cargo test

echo "Running integration tests..."
cd ../../../
cargo test --test integration_tests

echo "Running end-to-end tests..."
npm test -- --testPathPattern=e2e

echo "Generating test report..."
cargo tarpaulin --out Html --output-dir coverage

echo "All tests passed!"
```

### Monitoring Scripts

```bash
#!/bin/bash
# scripts/monitor.sh

CONTRACT_ID=${1:-$DEFAULT_CONTRACT_ID}

if [ -z "$CONTRACT_ID" ]; then
    echo "Usage: $0 <CONTRACT_ID>"
    exit 1
fi

echo "Monitoring contract: $CONTRACT_ID"

# Check contract health
stellopay-cli health --contract $CONTRACT_ID || {
    echo "Contract health check failed!"
    exit 1
}

# Monitor events
stellopay-cli stream events --contract $CONTRACT_ID &
EVENT_PID=$!

# Monitor performance
stellopay-cli monitor performance --contract $CONTRACT_ID --interval 30s &
PERF_PID=$!

# Handle shutdown
trap "kill $EVENT_PID $PERF_PID" EXIT

echo "Monitoring started. Press Ctrl+C to stop."
wait
```

### Backup Scripts

```bash
#!/bin/bash
# scripts/backup.sh

CONTRACT_ID=${1:-$DEFAULT_CONTRACT_ID}
BACKUP_DIR="backups/$(date +%Y%m%d_%H%M%S)"

mkdir -p $BACKUP_DIR

echo "Creating backup for contract: $CONTRACT_ID"

# Export contract state
stellopay-cli export state --contract $CONTRACT_ID --output $BACKUP_DIR/state.json

# Export payroll data
stellopay-cli export payrolls --contract $CONTRACT_ID --output $BACKUP_DIR/payrolls.json

# Export events
stellopay-cli export events --contract $CONTRACT_ID --output $BACKUP_DIR/events.json

# Create compressed archive
tar -czf $BACKUP_DIR.tar.gz $BACKUP_DIR
rm -rf $BACKUP_DIR

echo "Backup created: $BACKUP_DIR.tar.gz"
```


## Getting Started

1. **Install CLI Tools**:
   ```bash
   cargo install stellopay-cli
   ```

2. **Set up Development Environment**:
   ```bash
   git clone https://github.com/stellopay/stellopay-core
   cd stellopay-core
   docker-compose -f docker-compose.dev.yml up -d
   ```

3. **Deploy Test Contract**:
   ```bash
   stellopay-cli deploy --network testnet
   ```

4. **Run Examples**:
   ```bash
   stellopay-cli test run --suite examples
   ```

5. **Start Monitoring**:
   ```bash
   stellopay-cli monitor --contract <CONTRACT_ID>
   ```

For detailed usage instructions, see the [CLI Reference](./cli-reference.md) and [Integration Guide](../integration/README.md).
