# StellopayCore CLI

A command-line interface for managing StellopayCore contracts on the Stellar network.

## Features

- **Deploy contracts** to any Stellar network (testnet, mainnet, futurenet)
- **Query contract information** and details
- **Configuration management** with persistent settings
- **Status monitoring** for dependencies and contract builds
- **Comprehensive logging** and error handling

## Installation

### Prerequisites

- Rust 1.70+ (for building from source)
- Soroban CLI (automatically checked and prompted for installation)

### From Source

```bash
cd tools/cli
cargo build --release
```

The binary will be available at `target/release/stellopay-cli`.

## Usage

### Basic Commands

```bash
# Show CLI status and check dependencies
stellopay-cli status

# Deploy a contract
stellopay-cli deploy --owner <STELLAR_ADDRESS>

# Get contract information
stellopay-cli info --contract-id <CONTRACT_ID>

# Show help
stellopay-cli --help
```

### Configuration

The CLI uses a configuration file to store settings. By default, it's located at `~/.stellopay/config.toml`.

You can specify a custom configuration file:

```bash
stellopay-cli --config /path/to/config.toml status
```

#### Configuration Options

```toml
# Network configuration
rpc_url = "https://soroban-testnet.stellar.org:443"
network_passphrase = "Test SDF Network ; September 2015"

# Optional: Default contract ID
contract_id = "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAE"

# Optional: Default secret key (for deployments)
secret_key = "SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX"
```

### Commands

#### Deploy

Deploy a new StellopayCore contract:

```bash
stellopay-cli deploy --owner <STELLAR_ADDRESS>
```

Options:
- `--owner <ADDRESS>`: The Stellar address that will own the contract (required)
- `--network <NETWORK>`: Network to deploy to (testnet, mainnet, futurenet) [default: testnet]
- `--wasm <PATH>`: Path to the WASM file [default: auto-detected]

Examples:
```bash
# Deploy to testnet
stellopay-cli deploy --owner GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF

# Deploy to mainnet
stellopay-cli deploy --owner GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF --network mainnet

# Deploy with custom WASM
stellopay-cli deploy --owner GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF --wasm ./custom_contract.wasm
```

#### Info

Get detailed information about a deployed contract:

```bash
stellopay-cli info --contract-id <CONTRACT_ID>
```

Options:
- `--contract-id <ID>`: The contract ID to query (required)

Example:
```bash
stellopay-cli info --contract-id CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAE
```

#### Status

Show CLI status and check system dependencies:

```bash
stellopay-cli status
```

This command checks:
- ✅ Configuration file status
- ✅ Soroban CLI availability
- ✅ Contract WASM build status
- ✅ Network connectivity

### Global Options

- `--config <PATH>`: Specify configuration file path
- `--verbose`: Enable verbose logging
- `--help`: Show help information
- `--version`: Show version information

### Environment Variables

- `STELLOPAY_CONFIG`: Override default configuration file path
- `STELLOPAY_NETWORK`: Override default network
- `STELLOPAY_RPC_URL`: Override default RPC URL

## Development

### Running Tests

```bash
# Run all tests
cargo test

# Run only unit tests
cargo test --lib

# Run only integration tests
cargo test --test integration_tests

# Run with verbose output
cargo test -- --nocapture
```

### Building

```bash
# Development build
cargo build

# Release build
cargo build --release

# Build with specific features
cargo build --features "custom-feature"
```

### Code Structure

```
src/
├── lib.rs          # CLI structure definitions
├── main.rs         # Main entry point
├── commands.rs     # Command implementations
├── config.rs       # Configuration management
└── utils.rs        # Utility functions

tests/
└── integration_tests.rs  # Integration tests
```

## Contract Integration

The CLI automatically detects and uses the contract WASM from the onchain workspace:

```
../../onchain/target/wasm32v1-none/release/stello_pay_contract.wasm
```

Make sure to build the contract first:

```bash
cd ../../onchain
soroban contract build
```

## Error Handling

The CLI provides comprehensive error messages and logging:

- **Configuration errors**: Issues with config file or settings
- **Network errors**: Connection or RPC issues
- **Contract errors**: Deployment or interaction failures
- **Validation errors**: Invalid addresses or parameters

Use `--verbose` for detailed error information and debugging.

## Network Support

| Network | RPC URL | Passphrase |
|---------|---------|------------|
| testnet | https://soroban-testnet.stellar.org:443 | Test SDF Network ; September 2015 |
| mainnet | https://soroban-mainnet.stellar.org:443 | Public Global Stellar Network ; September 2015 |
| futurenet | https://soroban-futurenet.stellar.org:443 | Test SDF Future Network ; October 2022 |

## Examples

### Complete Deployment Flow

```bash
# 1. Check status
stellopay-cli status

# 2. Deploy contract
stellopay-cli deploy --owner GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF

# 3. Get contract info
stellopay-cli info --contract-id CXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
```

### Working with Different Networks

```bash
# Deploy to futurenet
stellopay-cli deploy --network futurenet --owner GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF

# Use custom config for mainnet
stellopay-cli --config ./mainnet-config.toml deploy --network mainnet --owner GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF
```

## Troubleshooting

### Common Issues

1. **Soroban CLI not found**
   ```bash
   # Install Soroban CLI
   cargo install --locked soroban-cli
   ```

2. **Contract WASM not found**
   ```bash
   # Build the contract
   cd ../../onchain
   soroban contract build
   ```

3. **Network connectivity issues**
   ```bash
   # Check RPC URL
   curl -X POST https://soroban-testnet.stellar.org:443 \
     -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}'
   ```

4. **Configuration file issues**
   ```bash
   # Reset configuration
   rm ~/.stellopay/config.toml
   stellopay-cli status  # This will recreate the config
   ```

### Debug Mode

Enable verbose logging for detailed information:

```bash
stellopay-cli --verbose deploy --owner GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality
4. Ensure all tests pass: `cargo test`
5. Submit a pull request

## License

This project is licensed under the MIT License - see the LICENSE file for details.

```toml
[network]
rpc_url = "https://soroban-testnet.stellar.org:443"
network_passphrase = "Test SDF Network ; September 2015"

[contract]
default_contract_id = "CONTRACT_ID_HERE"

[auth]
secret_key = "SECRET_KEY_HERE"

[defaults]
token = "TOKEN_ADDRESS_HERE"
frequency = "monthly"
```

## Commands

### Contract Management

```bash
# Deploy new contract
stellopay-cli deploy --network testnet --owner <OWNER>

# Initialize contract
stellopay-cli contract initialize --owner <OWNER>

# Pause/unpause contract
stellopay-cli contract pause
stellopay-cli contract unpause

# Transfer ownership
stellopay-cli contract transfer-ownership --new-owner <ADDRESS>

# Get contract status
stellopay-cli contract status
```

### Payroll Management

```bash
# List payrolls
stellopay-cli payroll list

# Create new payroll
stellopay-cli payroll create \
  --employee <ADDRESS> \
  --salary 5000 \
  --frequency monthly \
  --token <TOKEN>

# Get payroll info
stellopay-cli payroll get --employee <ADDRESS>

# Update payroll
stellopay-cli payroll update --employee <ADDRESS> --salary 6000
```

### Payment Processing

```bash
# Process individual payment
stellopay-cli payment process --employee <ADDRESS>

# Process all eligible payments
stellopay-cli payment process-all

# Get payment history
stellopay-cli payment history --employee <ADDRESS>
```

### Token Management

```bash
# List supported tokens
stellopay-cli token list

# Add supported token
stellopay-cli token add --address <TOKEN_ADDRESS>

# Remove token
stellopay-cli token remove --address <TOKEN_ADDRESS> --confirm
```

### Monitoring

```bash
# Monitor contract health
stellopay-cli monitor health

# Watch events
stellopay-cli monitor watch --events SalaryDisbursed,PayrollCreated

# Get performance metrics
stellopay-cli monitor metrics --duration 1h

# Debug transaction
stellopay-cli monitor debug --transaction <TX_HASH>
```

### Testing

```bash
# Set up test environment
stellopay-cli test setup --network testnet

# Generate test data
stellopay-cli test generate employees --count 10 --output employees.json

# Run load test
stellopay-cli test load-test --duration 5m --rate 10
```

## Examples

### Complete Payroll Setup

```bash
# 1. Deploy contract
CONTRACT_ID=$(stellopay-cli deploy --network testnet --owner $OWNER)

# 2. Add supported token
stellopay-cli -c $CONTRACT_ID token add --address $USDC_TOKEN

# 3. Create payroll for employee
stellopay-cli -c $CONTRACT_ID payroll create \
  --employee $EMPLOYEE \
  --salary 5000 \
  --frequency monthly \
  --token $USDC_TOKEN

# 4. Deposit funds
stellopay-cli -c $CONTRACT_ID deposit \
  --amount 50000 \
  --token $USDC_TOKEN

# 5. Process payment (when due)
stellopay-cli -c $CONTRACT_ID pay --employee $EMPLOYEE
```

### Bulk Operations

```bash
# Create employees.json with list of addresses
echo '["ADDR1", "ADDR2", "ADDR3"]' > employees.json

# Process bulk payments
stellopay-cli bulk-pay --employees employees.json --limit 50
```

### Monitoring Setup

```bash
# Start health monitoring
stellopay-cli monitor health --interval 30 --threshold 5000 &

# Stream events to file
stellopay-cli stream --events all --format json > events.log &

# Generate daily report
stellopay-cli monitor analyze --from "2025-01-01" --to "2025-01-02" --output report.json
```

## Environment Variables

- `STELLOPAY_SECRET_KEY`: Secret key for signing transactions
- `STELLOPAY_CONTRACT_ID`: Default contract ID
- `STELLOPAY_RPC_URL`: RPC endpoint URL
- `STELLOPAY_NETWORK`: Network name (testnet/mainnet)

## Troubleshooting

### Common Issues

1. **"No contract ID specified"**
   - Set `--contract-id` flag or add to config file
   - Set `STELLOPAY_CONTRACT_ID` environment variable

2. **"Contract not found"**
   - Verify contract is deployed on the specified network
   - Check RPC URL is correct

3. **"Unauthorized"**
   - Ensure secret key is set correctly
   - Verify the account has the required permissions

4. **"Insufficient balance"**
   - Deposit more funds using `stellopay-cli deposit`
   - Check token balance with `stellopay-cli info --detailed`

### Debug Mode

Enable verbose logging:

```bash
stellopay-cli -v <command>
```

Or set log level:

```bash
RUST_LOG=debug stellopay-cli <command>
```

## Development

### Building

```bash
cargo build --release
```

### Testing

```bash
cargo test
```

### Adding New Commands

1. Add command definition to `src/lib.rs`
2. Implement command handler in `src/commands.rs`
3. Add tests in `tests/` directory

## License

MIT License - see [LICENSE](../../LICENSE) file for details.
