# Integration Guide

This guide shows how to integrate your application with the StellopayCore contract.

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Contract Deployment](#contract-deployment)
3. [Basic Integration](#basic-integration)
4. [Advanced Integration](#advanced-integration)
5. [Frontend Integration](#frontend-integration)
6. [Backend Integration](#backend-integration)
7. [Testing Integration](#testing-integration)

## Prerequisites

Before integrating with StellopayCore, ensure you have:

- **Rust toolchain** installed (for contract interaction)
- **Soroban CLI** installed
- **Stellar SDK** for your preferred language
- **Basic understanding** of Stellar accounts and transactions
- **Test tokens** for development

### Installation

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Soroban CLI
cargo install --locked soroban-cli

# Verify installation
soroban --version
```

## Contract Deployment

### 1. Deploy to Testnet

```bash
# Build the contract
cd onchain/contracts/stello_pay_contract
soroban contract build

# Deploy to testnet
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/stello_pay_contract.wasm \
  --source-account <YOUR_ACCOUNT> \
  --rpc-url https://soroban-testnet.stellar.org:443 \
  --network testnet
```

### 2. Initialize Contract

```bash
# Initialize with owner
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source-account <OWNER_ACCOUNT> \
  --rpc-url https://soroban-testnet.stellar.org:443 \
  --network testnet \
  -- initialize \
  --owner <OWNER_ADDRESS>
```

## Basic Integration

### 1. Setting Up Your Environment

```rust
use soroban_sdk::{Env, Address, Symbol, Vec};
use stellar_sdk::*;

// Create environment
let env = Env::default();

// Contract address (from deployment)
let contract_address = Address::from_string("YOUR_CONTRACT_ADDRESS");

// Initialize contract client
let contract = PayrollContractClient::new(&env, &contract_address);
```

### 2. Basic Payroll Operations

```rust
// Create a payroll for an employee
pub fn create_employee_payroll(
    env: &Env,
    contract: &PayrollContractClient,
    employer: &Address,
    employee: &Address,
    token: &Address,
    monthly_salary: i128,
) -> Result<(), PayrollError> {
    // Monthly frequency (30 days in seconds)
    let recurrence_frequency = 30 * 24 * 60 * 60; // 2,592,000 seconds
    
    contract.create_or_update_escrow(
        &employer,
        &employee,
        &token,
        &monthly_salary,
        &recurrence_frequency, // interval (legacy)
        &recurrence_frequency,
    )
}

// Deposit funds for salary payments
pub fn deposit_salary_funds(
    env: &Env,
    contract: &PayrollContractClient,
    employer: &Address,
    token: &Address,
    amount: i128,
) -> Result<(), PayrollError> {
    contract.deposit_tokens(&employer, &token, &amount)
}

// Process salary payment
pub fn pay_employee(
    env: &Env,
    contract: &PayrollContractClient,
    employer: &Address,
    employee: &Address,
) -> Result<(), PayrollError> {
    contract.disburse_salary(&employer, &employee)
}
```

### 3. Querying Contract State

```rust
// Check employee's payroll details
pub fn get_employee_info(
    env: &Env,
    contract: &PayrollContractClient,
    employee: &Address,
) -> Option<PayrollInfo> {
    if let Some(payroll) = contract.get_payroll(&employee) {
        Some(PayrollInfo {
            employer: payroll.employer,
            token: payroll.token,
            amount: payroll.amount,
            next_payout: payroll.next_payout_timestamp,
            frequency: payroll.recurrence_frequency,
        })
    } else {
        None
    }
}

// Check if employee can be paid
pub fn can_pay_employee(
    env: &Env,
    contract: &PayrollContractClient,
    employee: &Address,
) -> bool {
    contract.is_eligible_for_disbursement(&employee)
}

// Check employer's balance
pub fn get_employer_funds(
    env: &Env,
    contract: &PayrollContractClient,
    employer: &Address,
    token: &Address,
) -> i128 {
    contract.get_employer_balance(&employer, &token)
}
```

## Advanced Integration

### 1. Bulk Operations

```rust
// Process multiple employees at once
pub fn process_payroll_batch(
    env: &Env,
    contract: &PayrollContractClient,
    owner: &Address,
    employees: Vec<Address>,
) -> Vec<Address> {
    contract.process_recurring_disbursements(&owner, &employees)
}

// Create multiple payrolls
pub fn setup_team_payroll(
    env: &Env,
    contract: &PayrollContractClient,
    employer: &Address,
    team_members: Vec<(Address, i128)>, // (employee, salary)
    token: &Address,
) -> Result<(), PayrollError> {
    let monthly_frequency = 30 * 24 * 60 * 60;
    
    for (employee, salary) in team_members {
        contract.create_or_update_escrow(
            &employer,
            &employee,
            &token,
            &salary,
            &monthly_frequency,
            &monthly_frequency,
        )?;
    }
    
    Ok(())
}
```

### 2. Event Monitoring

```rust
use soroban_sdk::events::Event;

// Monitor salary disbursement events
pub fn monitor_salary_payments(
    env: &Env,
    contract_address: &Address,
) -> Vec<SalaryDisbursed> {
    let events = env.events().get_all();
    let mut salary_events = Vec::new();
    
    for event in events {
        if event.contract_address == *contract_address {
            if let Ok(salary_event) = event.data.try_into::<SalaryDisbursed>() {
                salary_events.push(salary_event);
            }
        }
    }
    
    salary_events
}

// Monitor deposits
pub fn monitor_deposits(
    env: &Env,
    contract_address: &Address,
) -> Vec<DepositEvent> {
    // Similar implementation for deposit events
    // ...
}
```

### 3. Error Handling

```rust
pub fn handle_payroll_errors(error: PayrollError) -> String {
    match error {
        PayrollError::Unauthorized => {
            "Access denied. Check your authorization.".to_string()
        }
        PayrollError::InsufficientBalance => {
            "Insufficient funds. Please deposit more tokens.".to_string()
        }
        PayrollError::PayrollNotFound => {
            "Employee payroll not found. Create payroll first.".to_string()
        }
        PayrollError::NextPayoutTimeNotReached => {
            "Payment not due yet. Check next payout time.".to_string()
        }
        PayrollError::ContractPaused => {
            "Contract is paused. Please try again later.".to_string()
        }
        PayrollError::InvalidData => {
            "Invalid input data. Check your parameters.".to_string()
        }
        PayrollError::TransferFailed => {
            "Token transfer failed. Check token approval.".to_string()
        }
        _ => "Unknown error occurred.".to_string(),
    }
}
```

## Frontend Integration

### 1. React Integration Example

```typescript
// hooks/usePayroll.ts
import { useState, useEffect } from 'react';
import { Contract, SorobanRpc } from '@stellar/stellar-sdk';

interface PayrollHook {
  createPayroll: (employeeId: string, salary: number) => Promise<void>;
  getEmployeeInfo: (employeeId: string) => Promise<PayrollInfo | null>;
  processPayment: (employeeId: string) => Promise<void>;
  loading: boolean;
  error: string | null;
}

export function usePayroll(contractAddress: string): PayrollHook {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const createPayroll = async (employeeId: string, salary: number) => {
    setLoading(true);
    setError(null);
    
    try {
      // Implementation for creating payroll
      const server = new SorobanRpc.Server('https://soroban-testnet.stellar.org');
      const contract = new Contract(contractAddress);
      
      // Build and submit transaction
      // ...
      
    } catch (err) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  };

  const getEmployeeInfo = async (employeeId: string) => {
    // Implementation for getting employee info
    // ...
  };

  const processPayment = async (employeeId: string) => {
    // Implementation for processing payment
    // ...
  };

  return {
    createPayroll,
    getEmployeeInfo,
    processPayment,
    loading,
    error,
  };
}
```

### 2. Vue.js Integration Example

```vue
<!-- components/PayrollManager.vue -->
<template>
  <div class="payroll-manager">
    <h2>Payroll Management</h2>
    
    <div class="employee-list">
      <div 
        v-for="employee in employees" 
        :key="employee.address"
        class="employee-card"
      >
        <h3>{{ employee.name }}</h3>
        <p>Salary: {{ formatCurrency(employee.salary) }}</p>
        <p>Next Payment: {{ formatDate(employee.nextPayment) }}</p>
        <button 
          @click="processPayment(employee.address)"
          :disabled="!employee.canPay"
        >
          Pay Now
        </button>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, onMounted } from 'vue';
import { usePayrollContract } from '@/composables/payroll';

const { 
  getEmployees, 
  processPayment, 
  loading, 
  error 
} = usePayrollContract();

const employees = ref([]);

onMounted(async () => {
  employees.value = await getEmployees();
});

const formatCurrency = (amount) => {
  return new Intl.NumberFormat('en-US', {
    style: 'currency',
    currency: 'USD'
  }).format(amount);
};

const formatDate = (timestamp) => {
  return new Date(timestamp * 1000).toLocaleDateString();
};
</script>
```

## Backend Integration

### 1. Node.js Service

```javascript
// services/payrollService.js
const { SorobanRpc, Contract, Keypair } = require('@stellar/stellar-sdk');

class PayrollService {
  constructor(contractAddress, ownerKeypair) {
    this.contractAddress = contractAddress;
    this.ownerKeypair = ownerKeypair;
    this.server = new SorobanRpc.Server('https://soroban-testnet.stellar.org');
  }

  async createEmployeePayroll(employeeAddress, salary, tokenAddress) {
    try {
      const contract = new Contract(this.contractAddress);
      
      // Build transaction
      const operation = contract.call(
        'create_or_update_escrow',
        this.ownerKeypair.publicKey(),
        employeeAddress,
        tokenAddress,
        salary,
        2592000, // 30 days
        2592000
      );

      // Submit transaction
      const result = await this.submitTransaction(operation);
      return result;
    } catch (error) {
      throw new Error(`Failed to create payroll: ${error.message}`);
    }
  }

  async processMonthlyPayroll() {
    try {
      // Get all employees
      const employees = await this.getEligibleEmployees();
      
      // Process in batches
      const batchSize = 10;
      const results = [];
      
      for (let i = 0; i < employees.length; i += batchSize) {
        const batch = employees.slice(i, i + batchSize);
        const batchResult = await this.processBatch(batch);
        results.push(...batchResult);
      }
      
      return results;
    } catch (error) {
      throw new Error(`Failed to process payroll: ${error.message}`);
    }
  }

  async submitTransaction(operation) {
    // Implementation for submitting transactions
    // ...
  }
}

module.exports = PayrollService;
```

### 2. Python Service

```python
# payroll_service.py
import asyncio
from stellar_sdk import Server, Keypair, TransactionBuilder
from stellar_sdk.soroban import SorobanServer

class PayrollService:
    def __init__(self, contract_address: str, owner_keypair: Keypair):
        self.contract_address = contract_address
        self.owner_keypair = owner_keypair
        self.soroban_server = SorobanServer("https://soroban-testnet.stellar.org")
        
    async def create_payroll(self, employee_address: str, salary: int, token_address: str):
        """Create a new payroll for an employee"""
        try:
            # Build contract invocation
            # Implementation details...
            pass
        except Exception as e:
            raise Exception(f"Failed to create payroll: {str(e)}")
    
    async def process_recurring_payments(self, employee_addresses: list):
        """Process recurring payments for multiple employees"""
        try:
            # Batch process employees
            # Implementation details...
            pass
        except Exception as e:
            raise Exception(f"Failed to process payments: {str(e)}")
    
    async def monitor_events(self):
        """Monitor contract events"""
        # Implementation for event monitoring
        pass
```

## Testing Integration

### 1. Unit Tests

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};

    #[test]
    fn test_full_payroll_cycle() {
        let env = Env::default();
        let contract = PayrollContractClient::new(&env, &env.register_contract(None, PayrollContract));
        
        let owner = Address::generate(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token = Address::generate(&env);
        
        // Initialize contract
        contract.initialize(&owner);
        
        // Create payroll
        let salary = 5000_0000000; // 5000 with 7 decimals
        let monthly_freq = 30 * 24 * 60 * 60;
        
        let payroll = contract.create_or_update_escrow(
            &employer,
            &employee,
            &token,
            &salary,
            &monthly_freq,
            &monthly_freq,
        ).unwrap();
        
        assert_eq!(payroll.amount, salary);
        assert_eq!(payroll.employer, employer);
        
        // Deposit funds
        contract.deposit_tokens(&employer, &token, &salary).unwrap();
        
        // Check balance
        let balance = contract.get_employer_balance(&employer, &token);
        assert_eq!(balance, salary);
        
        // Fast forward time
        env.ledger().with_mut(|li| {
            li.timestamp = monthly_freq + 1;
        });
        
        // Process payment
        contract.disburse_salary(&employer, &employee).unwrap();
        
        // Verify payment
        let updated_balance = contract.get_employer_balance(&employer, &token);
        assert_eq!(updated_balance, 0);
    }
}
```

### 2. Integration Test Suite

```rust
// tests/integration_test.rs
use soroban_sdk::Env;

#[tokio::test]
async fn test_end_to_end_payroll() {
    // Set up test environment
    let env = setup_test_env().await;
    
    // Test contract deployment
    let contract_address = deploy_contract(&env).await;
    
    // Test initialization
    initialize_contract(&env, &contract_address).await;
    
    // Test payroll creation
    create_test_payroll(&env, &contract_address).await;
    
    // Test payment processing
    process_test_payment(&env, &contract_address).await;
    
    // Verify final state
    verify_final_state(&env, &contract_address).await;
}

async fn setup_test_env() -> TestEnvironment {
    // Implementation for setting up test environment
}
```

## Best Practices

### 1. Error Handling

Always implement comprehensive error handling:

```rust
pub fn safe_payroll_operation(
    env: &Env,
    contract: &PayrollContractClient,
    operation: PayrollOperation,
) -> Result<(), PayrollError> {
    // Check contract status
    if contract.is_paused() {
        return Err(PayrollError::ContractPaused);
    }
    
    // Validate inputs
    if !validate_operation(&operation) {
        return Err(PayrollError::InvalidData);
    }
    
    // Execute operation with retry logic
    let mut retries = 3;
    while retries > 0 {
        match execute_operation(env, contract, &operation) {
            Ok(result) => return Ok(result),
            Err(PayrollError::TransferFailed) if retries > 1 => {
                retries -= 1;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(e) => return Err(e),
        }
    }
    
    Err(PayrollError::TransferFailed)
}
```

### 2. Gas Optimization

```rust
// Batch operations for better gas efficiency
pub fn optimize_bulk_operations(
    env: &Env,
    contract: &PayrollContractClient,
    employees: Vec<Address>,
) -> Result<Vec<Address>, PayrollError> {
    // Process in optimal batch sizes
    const OPTIMAL_BATCH_SIZE: usize = 50;
    
    let mut results = Vec::new();
    for chunk in employees.chunks(OPTIMAL_BATCH_SIZE) {
        let batch_result = contract.process_recurring_disbursements(
            &get_owner(),
            &chunk.to_vec(),
        );
        results.extend(batch_result);
    }
    
    Ok(results)
}
```

### 3. Security Considerations

```rust
// Always validate addresses
pub fn validate_address(address: &Address) -> Result<(), PayrollError> {
    if address.to_string().is_empty() {
        return Err(PayrollError::InvalidData);
    }
    // Additional validation...
    Ok(())
}

// Implement access control
pub fn check_authorization(
    caller: &Address,
    required_role: Role,
    contract: &PayrollContractClient,
) -> Result<(), PayrollError> {
    match required_role {
        Role::Owner => {
            let owner = contract.get_owner().ok_or(PayrollError::Unauthorized)?;
            if caller != &owner {
                return Err(PayrollError::Unauthorized);
            }
        }
        Role::Employer => {
            // Check if caller is an employer
            // Implementation...
        }
    }
    Ok(())
}
```

## Next Steps

1. **Deploy to testnet** and verify all functions work correctly
2. **Implement monitoring** for production deployment
3. **Set up automated testing** with CI/CD pipeline
4. **Create backup procedures** for contract data
5. **Plan for upgrades** using proxy patterns
6. **Monitor gas usage** and optimize operations

For more detailed examples, see the [Examples](../examples/README.md) section.
