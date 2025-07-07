# Best Practices

Guidelines and recommended patterns for working with the StellopayCore contract.

## Table of Contents

1. [Security Best Practices](#security-best-practices)
2. [Performance Optimization](#performance-optimization)
3. [Error Handling](#error-handling)
4. [State Management](#state-management)
5. [Testing Strategies](#testing-strategies)
6. [Deployment Practices](#deployment-practices)
7. [Monitoring and Maintenance](#monitoring-and-maintenance)

## Security Best Practices

### 1. Access Control

Always implement proper access control patterns:

```rust
// ✅ Good: Check authorization before operations
pub fn secure_operation(
    env: &Env,
    contract: &PayrollContractClient,
    caller: &Address,
    operation: Operation,
) -> Result<(), PayrollError> {
    // Verify caller authorization
    caller.require_auth();
    
    // Check specific permissions
    match operation {
        Operation::CreatePayroll => {
            let owner = contract.get_owner().ok_or(PayrollError::Unauthorized)?;
            if caller != &owner {
                return Err(PayrollError::Unauthorized);
            }
        }
        Operation::DisbursePayroll { employee } => {
            let payroll = contract.get_payroll(&employee)
                .ok_or(PayrollError::PayrollNotFound)?;
            if caller != &payroll.employer {
                return Err(PayrollError::Unauthorized);
            }
        }
    }
    
    // Proceed with operation
    Ok(())
}

// ❌ Bad: No authorization check
pub fn insecure_operation(
    env: &Env,
    contract: &PayrollContractClient,
    operation: Operation,
) -> Result<(), PayrollError> {
    // Direct operation without checks - NEVER DO THIS
    // execute_operation(operation)
    Ok(())
}
```

### 2. Input Validation

Always validate inputs thoroughly:

```rust
// ✅ Good: Comprehensive input validation
pub fn validate_payroll_params(
    employer: &Address,
    employee: &Address,
    token: &Address,
    amount: i128,
    recurrence_frequency: u64,
) -> Result<(), PayrollError> {
    // Validate addresses
    if employer.to_string().is_empty() || 
       employee.to_string().is_empty() || 
       token.to_string().is_empty() {
        return Err(PayrollError::InvalidData);
    }
    
    // Validate amount
    if amount <= 0 {
        return Err(PayrollError::InvalidData);
    }
    
    // Validate frequency (minimum 1 day, maximum 1 year)
    if recurrence_frequency < 86400 || recurrence_frequency > 31536000 {
        return Err(PayrollError::InvalidRecurrenceFrequency);
    }
    
    // Prevent self-employment
    if employer == employee {
        return Err(PayrollError::InvalidData);
    }
    
    Ok(())
}

// ❌ Bad: No validation
pub fn no_validation(amount: i128) -> Result<(), PayrollError> {
    // Using amount directly without validation
    Ok(())
}
```

### 3. Safe Math Operations

Use safe arithmetic to prevent overflow/underflow:

```rust
// ✅ Good: Safe arithmetic operations
pub fn safe_balance_calculation(
    current_balance: i128,
    amount: i128,
    operation: BalanceOperation,
) -> Result<i128, PayrollError> {
    match operation {
        BalanceOperation::Add => {
            current_balance.checked_add(amount)
                .ok_or(PayrollError::InvalidData)
        }
        BalanceOperation::Subtract => {
            if current_balance < amount {
                return Err(PayrollError::InsufficientBalance);
            }
            current_balance.checked_sub(amount)
                .ok_or(PayrollError::InvalidData)
        }
    }
}

// ❌ Bad: Unchecked arithmetic
pub fn unsafe_calculation(balance: i128, amount: i128) -> i128 {
    balance + amount // Can overflow
}
```

### 4. Token Handling

Implement secure token operations:

```rust
// ✅ Good: Safe token transfers with verification
pub fn safe_token_transfer(
    env: &Env,
    token: &Address,
    from: &Address,
    to: &Address,
    amount: i128,
) -> Result<(), PayrollError> {
    let token_client = TokenClient::new(env, token);
    
    // Check initial balances
    let from_balance_before = token_client.balance(from);
    let to_balance_before = token_client.balance(to);
    
    // Perform transfer
    token_client.transfer(from, to, &amount);
    
    // Verify transfer success
    let from_balance_after = token_client.balance(from);
    let to_balance_after = token_client.balance(to);
    
    if from_balance_after != from_balance_before - amount ||
       to_balance_after != to_balance_before + amount {
        return Err(PayrollError::TransferFailed);
    }
    
    Ok(())
}
```

## Performance Optimization

### 1. Batch Operations

Use batch operations for better efficiency:

```rust
// ✅ Good: Batch processing
pub fn process_payroll_batch(
    env: &Env,
    contract: &PayrollContractClient,
    employees: Vec<Address>,
) -> Result<Vec<Address>, PayrollError> {
    const OPTIMAL_BATCH_SIZE: usize = 50;
    
    let mut processed = Vec::new();
    
    for chunk in employees.chunks(OPTIMAL_BATCH_SIZE) {
        let batch_result = contract.process_recurring_disbursements(
            &get_contract_owner(),
            &chunk.to_vec(),
        );
        processed.extend(batch_result);
    }
    
    Ok(processed)
}

// ❌ Bad: Individual operations
pub fn process_individually(
    env: &Env,
    contract: &PayrollContractClient,
    employees: Vec<Address>,
) -> Result<(), PayrollError> {
    for employee in employees {
        // Individual operation per employee - inefficient
        contract.disburse_salary(&get_employer(&employee), &employee)?;
    }
    Ok(())
}
```

### 2. Efficient Storage Access

Minimize storage operations:

```rust
// ✅ Good: Batch storage operations
pub fn efficient_data_access(
    env: &Env,
    contract: &PayrollContractClient,
    employees: Vec<Address>,
) -> Vec<PayrollSummary> {
    // Collect all data in a single pass
    employees.iter()
        .filter_map(|employee| {
            contract.get_payroll(employee).map(|payroll| {
                PayrollSummary {
                    employee: employee.clone(),
                    next_payment: payroll.next_payout_timestamp,
                    amount: payroll.amount,
                }
            })
        })
        .collect()
}

// ❌ Bad: Multiple storage calls
pub fn inefficient_access(
    env: &Env,
    contract: &PayrollContractClient,
    employee: &Address,
) -> Option<PayrollDetails> {
    // Multiple separate calls - inefficient
    let payroll = contract.get_payroll(employee)?;
    let next_payment = contract.get_next_payout_timestamp(employee)?;
    let frequency = contract.get_recurrence_frequency(employee)?;
    
    Some(PayrollDetails {
        payroll,
        next_payment,
        frequency,
    })
}
```

### 3. Smart Caching

Implement intelligent caching strategies:

```rust
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub struct PayrollCache {
    payroll_cache: HashMap<Address, (Payroll, Instant)>,
    balance_cache: HashMap<(Address, Address), (i128, Instant)>,
    cache_ttl: Duration,
}

impl PayrollCache {
    pub fn new(cache_ttl: Duration) -> Self {
        Self {
            payroll_cache: HashMap::new(),
            balance_cache: HashMap::new(),
            cache_ttl,
        }
    }
    
    pub fn get_payroll(
        &mut self,
        contract: &PayrollContractClient,
        employee: &Address,
    ) -> Option<Payroll> {
        // Check cache first
        if let Some((payroll, cached_at)) = self.payroll_cache.get(employee) {
            if cached_at.elapsed() < self.cache_ttl {
                return Some(payroll.clone());
            }
        }
        
        // Fetch from contract and cache
        if let Some(payroll) = contract.get_payroll(employee) {
            self.payroll_cache.insert(employee.clone(), (payroll.clone(), Instant::now()));
            Some(payroll)
        } else {
            None
        }
    }
    
    pub fn invalidate_payroll(&mut self, employee: &Address) {
        self.payroll_cache.remove(employee);
    }
}
```

## Error Handling

### 1. Comprehensive Error Handling

Implement proper error handling patterns:

```rust
// ✅ Good: Comprehensive error handling
pub fn robust_payroll_operation(
    env: &Env,
    contract: &PayrollContractClient,
    operation: PayrollOperation,
) -> Result<OperationResult, PayrollError> {
    // Pre-operation checks
    if contract.is_paused() {
        return Err(PayrollError::ContractPaused);
    }
    
    // Validate operation
    validate_operation(&operation)?;
    
    // Execute with retry logic
    let mut retries = 3;
    let mut last_error = None;
    
    while retries > 0 {
        match execute_operation(env, contract, &operation) {
            Ok(result) => {
                // Log successful operation
                log_operation_success(&operation, &result);
                return Ok(result);
            }
            Err(PayrollError::TransferFailed) if retries > 1 => {
                // Retry on transient errors
                retries -= 1;
                last_error = Some(PayrollError::TransferFailed);
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                // Log error and return
                log_operation_error(&operation, &e);
                return Err(e);
            }
        }
    }
    
    // All retries exhausted
    Err(last_error.unwrap_or(PayrollError::TransferFailed))
}

// ❌ Bad: No error handling
pub fn fragile_operation(
    env: &Env,
    contract: &PayrollContractClient,
    operation: PayrollOperation,
) -> OperationResult {
    // Direct operation without error handling
    execute_operation(env, contract, &operation).unwrap() // NEVER DO THIS
}
```

### 2. Error Context and Logging

Provide meaningful error context:

```rust
#[derive(Debug)]
pub struct PayrollError {
    pub kind: PayrollErrorKind,
    pub context: String,
    pub timestamp: u64,
}

impl PayrollError {
    pub fn new(kind: PayrollErrorKind, context: &str) -> Self {
        Self {
            kind,
            context: context.to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }
    
    pub fn with_context(mut self, context: &str) -> Self {
        self.context = format!("{}: {}", context, self.context);
        self
    }
}

// Usage
pub fn create_payroll_with_context(
    env: &Env,
    contract: &PayrollContractClient,
    employer: &Address,
    employee: &Address,
    // ... other params
) -> Result<Payroll, PayrollError> {
    contract.create_or_update_escrow(
        employer,
        employee,
        // ... other params
    )
    .map_err(|e| e.with_context(&format!(
        "Failed to create payroll for employee {} by employer {}",
        employee.to_string(),
        employer.to_string()
    )))
}
```

## State Management

### 1. Consistent State Updates

Ensure atomic state updates:

```rust
// ✅ Good: Atomic operations
pub fn atomic_payroll_update(
    env: &Env,
    contract: &PayrollContractClient,
    employee: &Address,
    new_amount: i128,
) -> Result<(), PayrollError> {
    // Get current state
    let current_payroll = contract.get_payroll(employee)
        .ok_or(PayrollError::PayrollNotFound)?;
    
    // Validate the update
    if new_amount <= 0 {
        return Err(PayrollError::InvalidData);
    }
    
    // Perform atomic update
    let updated_payroll = contract.create_or_update_escrow(
        &current_payroll.employer,
        employee,
        &current_payroll.token,
        &new_amount,
        &current_payroll.interval,
        &current_payroll.recurrence_frequency,
    )?;
    
    // Verify update
    if updated_payroll.amount != new_amount {
        return Err(PayrollError::InvalidData);
    }
    
    Ok(())
}
```

### 2. State Validation

Always validate state consistency:

```rust
// ✅ Good: State validation
pub fn validate_contract_state(
    env: &Env,
    contract: &PayrollContractClient,
) -> Result<(), PayrollError> {
    // Check critical invariants
    
    // 1. Owner must be set
    if contract.get_owner().is_none() {
        return Err(PayrollError::InvalidData);
    }
    
    // 2. Pause state should be consistent
    let is_paused = contract.is_paused();
    
    // 3. Validate payroll consistency
    // (Implementation depends on specific business logic)
    
    Ok(())
}
```

## Testing Strategies

### 1. Comprehensive Test Coverage

Write tests for all scenarios:

```rust
#[cfg(test)]
mod comprehensive_tests {
    use super::*;
    
    #[test]
    fn test_complete_payroll_lifecycle() {
        let env = Env::default();
        let contract = create_test_contract(&env);
        
        // Test initialization
        test_contract_initialization(&env, &contract);
        
        // Test payroll creation
        test_payroll_creation(&env, &contract);
        
        // Test funding
        test_employer_funding(&env, &contract);
        
        // Test disbursement
        test_salary_disbursement(&env, &contract);
        
        // Test edge cases
        test_edge_cases(&env, &contract);
        
        // Test error conditions
        test_error_conditions(&env, &contract);
    }
    
    #[test]
    fn test_concurrent_operations() {
        // Test handling of concurrent operations
        let env = Env::default();
        let contract = create_test_contract(&env);
        
        // Simulate concurrent disbursements
        // Implementation...
    }
    
    #[test]
    fn test_boundary_conditions() {
        // Test boundary values
        let env = Env::default();
        let contract = create_test_contract(&env);
        
        // Test minimum values
        test_minimum_values(&env, &contract);
        
        // Test maximum values
        test_maximum_values(&env, &contract);
        
        // Test overflow conditions
        test_overflow_conditions(&env, &contract);
    }
}
```

### 2. Property-Based Testing

Use property-based testing for robustness:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_payroll_properties(
        amount in 1i128..1_000_000_000_000i128,
        frequency in 86400u64..31536000u64,
    ) {
        let env = Env::default();
        let contract = create_test_contract(&env);
        
        // Property: Creating payroll with valid parameters should succeed
        let result = create_test_payroll(&env, &contract, amount, frequency);
        prop_assert!(result.is_ok());
        
        // Property: Amount and frequency should be preserved
        let payroll = result.unwrap();
        prop_assert_eq!(payroll.amount, amount);
        prop_assert_eq!(payroll.recurrence_frequency, frequency);
    }
}
```

## Deployment Practices

### 1. Phased Deployment

Deploy contracts in phases:

```rust
// Phase 1: Core functionality
pub fn deploy_core_contract(env: &Env) -> Result<Address, DeploymentError> {
    // Deploy basic payroll functionality
    let contract_address = deploy_contract(env, "core_payroll.wasm")?;
    
    // Initialize with minimal configuration
    initialize_basic_contract(env, &contract_address)?;
    
    Ok(contract_address)
}

// Phase 2: Advanced features
pub fn upgrade_to_advanced(env: &Env, contract: &Address) -> Result<(), DeploymentError> {
    // Add advanced features
    add_batch_processing(env, contract)?;
    add_monitoring_capabilities(env, contract)?;
    
    Ok(())
}
```

### 2. Configuration Management

Use environment-specific configurations:

```rust
#[derive(Debug, Clone)]
pub struct ContractConfig {
    pub owner: Address,
    pub supported_tokens: Vec<Address>,
    pub min_payment_amount: i128,
    pub max_payment_amount: i128,
    pub min_frequency: u64,
    pub max_frequency: u64,
    pub emergency_pause_enabled: bool,
}

impl ContractConfig {
    pub fn testnet() -> Self {
        Self {
            owner: Address::from_string("TESTNET_OWNER_ADDRESS"),
            supported_tokens: vec![
                Address::from_string("TESTNET_USDC"),
                Address::from_string("TESTNET_XLM"),
            ],
            min_payment_amount: 1_0000000, // 1 token with 7 decimals
            max_payment_amount: 1_000_000_0000000, // 1M tokens
            min_frequency: 86400, // 1 day
            max_frequency: 31536000, // 1 year
            emergency_pause_enabled: true,
        }
    }
    
    pub fn mainnet() -> Self {
        Self {
            owner: Address::from_string("MAINNET_OWNER_ADDRESS"),
            supported_tokens: vec![
                Address::from_string("MAINNET_USDC"),
                Address::from_string("MAINNET_XLM"),
            ],
            min_payment_amount: 1_0000000,
            max_payment_amount: 100_000_0000000, // 100K tokens (lower for mainnet)
            min_frequency: 86400,
            max_frequency: 31536000,
            emergency_pause_enabled: true,
        }
    }
}
```

## Monitoring and Maintenance

### 1. Health Monitoring

Implement contract health checks:

```rust
#[derive(Debug, Clone)]
pub struct ContractHealth {
    pub is_operational: bool,
    pub is_paused: bool,
    pub owner_exists: bool,
    pub supported_tokens_count: u32,
    pub active_payrolls_count: u32,
    pub total_locked_value: i128,
    pub last_payment_timestamp: u64,
}

pub fn check_contract_health(
    env: &Env,
    contract: &PayrollContractClient,
) -> ContractHealth {
    let is_paused = contract.is_paused();
    let owner_exists = contract.get_owner().is_some();
    
    // Additional health checks
    let supported_tokens_count = count_supported_tokens(env, contract);
    let active_payrolls_count = count_active_payrolls(env, contract);
    let total_locked_value = calculate_total_locked_value(env, contract);
    
    ContractHealth {
        is_operational: !is_paused && owner_exists,
        is_paused,
        owner_exists,
        supported_tokens_count,
        active_payrolls_count,
        total_locked_value,
        last_payment_timestamp: get_last_payment_timestamp(env, contract),
    }
}
```

### 2. Performance Monitoring

Track contract performance:

```rust
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    pub average_transaction_time: Duration,
    pub successful_operations: u64,
    pub failed_operations: u64,
    pub gas_usage_average: u64,
    pub throughput_per_second: f64,
}

pub fn collect_performance_metrics(
    env: &Env,
    contract: &PayrollContractClient,
    time_window: Duration,
) -> PerformanceMetrics {
    // Collect metrics from event logs
    let events = get_contract_events(env, contract, time_window);
    
    // Calculate metrics
    let (successful, failed) = categorize_operations(&events);
    let average_time = calculate_average_time(&events);
    let throughput = calculate_throughput(&events, time_window);
    
    PerformanceMetrics {
        average_transaction_time: average_time,
        successful_operations: successful,
        failed_operations: failed,
        gas_usage_average: calculate_average_gas(&events),
        throughput_per_second: throughput,
    }
}
```

### 3. Automated Maintenance

Set up automated maintenance tasks:

```rust
pub async fn run_maintenance_tasks(
    env: &Env,
    contract: &PayrollContractClient,
) -> Result<(), MaintenanceError> {
    // 1. Process due payments
    process_due_payments(env, contract).await?;
    
    // 2. Clean up expired data
    cleanup_expired_data(env, contract).await?;
    
    // 3. Update token metadata
    update_token_metadata(env, contract).await?;
    
    // 4. Generate reports
    generate_maintenance_report(env, contract).await?;
    
    Ok(())
}

pub async fn process_due_payments(
    env: &Env,
    contract: &PayrollContractClient,
) -> Result<(), MaintenanceError> {
    // Get all employees with due payments
    let due_employees = get_employees_with_due_payments(env, contract);
    
    if !due_employees.is_empty() {
        // Process in batches
        let processed = contract.process_recurring_disbursements(
            &get_contract_owner(),
            &due_employees,
        );
        
        log::info!("Processed {} due payments", processed.len());
    }
    
    Ok(())
}
```

## Summary

Following these best practices will help ensure your StellopayCore integration is:

- **Secure**: Proper authorization, input validation, and error handling
- **Efficient**: Optimized for gas usage and performance
- **Reliable**: Comprehensive testing and monitoring
- **Maintainable**: Clean code structure and automated maintenance
- **Scalable**: Designed to handle growing usage

Remember to:
1. Always validate inputs and check authorization
2. Use batch operations for better efficiency
3. Implement comprehensive error handling
4. Test extensively, including edge cases
5. Monitor contract health and performance
6. Keep security as a top priority

For specific implementation examples, see the [Examples](../examples/README.md) section.
