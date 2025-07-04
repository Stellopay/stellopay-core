# Code Examples

Practical examples and common use cases for integrating with the StellopayCore contract.

## Table of Contents

1. [Basic Usage Examples](#basic-usage-examples)
2. [Common Use Cases](#common-use-cases)
3. [Integration Patterns](#integration-patterns)
4. [Frontend Examples](#frontend-examples)
5. [Backend Examples](#backend-examples)
6. [Testing Examples](#testing-examples)

## Basic Usage Examples

### 1. Contract Initialization

```rust
use soroban_sdk::{Env, Address};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn initialize_contract() -> Result<Address, Box<dyn std::error::Error>> {
    let env = Env::default();
    
    // Deploy contract
    let contract_address = env.register_contract(None, PayrollContract);
    let contract = PayrollContractClient::new(&env, &contract_address);
    
    // Create owner address
    let owner = Address::generate(&env);
    
    // Initialize contract
    contract.initialize(&owner);
    
    println!("Contract initialized with owner: {}", owner.to_string());
    Ok(contract_address)
}
```

### 2. Creating Employee Payroll

```rust
use soroban_sdk::{Env, Address};

fn create_employee_payroll(
    env: &Env,
    contract: &PayrollContractClient,
    employer: &Address,
    employee: &Address,
    token: &Address,
    monthly_salary: i128,
) -> Result<(), PayrollError> {
    // Monthly frequency (30 days in seconds)
    let monthly_frequency = 30 * 24 * 60 * 60; // 2,592,000 seconds
    
    // Create payroll
    let payroll = contract.create_or_update_escrow(
        employer,
        employee,
        token,
        &monthly_salary,
        &monthly_frequency, // interval (legacy)
        &monthly_frequency, // recurrence_frequency
    )?;
    
    println!("Created payroll for employee: {}", employee.to_string());
    println!("Monthly salary: {}", monthly_salary);
    println!("Next payment: {}", payroll.next_payout_timestamp);
    
    Ok(())
}
```

### 3. Funding Payroll

```rust
fn fund_payroll(
    env: &Env,
    contract: &PayrollContractClient,
    employer: &Address,
    token: &Address,
    amount: i128,
) -> Result<(), PayrollError> {
    // Deposit funds for salary payments
    contract.deposit_tokens(employer, token, &amount)?;
    
    // Check updated balance
    let balance = contract.get_employer_balance(employer, token);
    println!("Employer balance after deposit: {}", balance);
    
    Ok(())
}
```

### 4. Processing Salary Payment

```rust
fn process_salary_payment(
    env: &Env,
    contract: &PayrollContractClient,
    employer: &Address,
    employee: &Address,
) -> Result<(), PayrollError> {
    // Check if employee is eligible
    let eligible = contract.is_eligible_for_disbursement(employee);
    if !eligible {
        println!("Employee not eligible for payment yet");
        return Ok(());
    }
    
    // Process payment
    contract.disburse_salary(employer, employee)?;
    
    println!("Salary disbursed to employee: {}", employee.to_string());
    
    Ok(())
}
```

## Common Use Cases

### 1. HR Management System Integration

```rust
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Employee {
    pub id: String,
    pub name: String,
    pub email: String,
    pub address: Address,
    pub salary: i128,
    pub currency: String,
    pub pay_frequency: PayFrequency,
    pub start_date: u64,
    pub department: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PayFrequency {
    Weekly,
    BiWeekly,
    Monthly,
    Quarterly,
}

impl PayFrequency {
    pub fn to_seconds(&self) -> u64 {
        match self {
            PayFrequency::Weekly => 7 * 24 * 60 * 60,
            PayFrequency::BiWeekly => 14 * 24 * 60 * 60,
            PayFrequency::Monthly => 30 * 24 * 60 * 60,
            PayFrequency::Quarterly => 90 * 24 * 60 * 60,
        }
    }
}

pub struct HRPayrollSystem {
    contract: PayrollContractClient,
    employees: HashMap<String, Employee>,
    token_addresses: HashMap<String, Address>,
}

impl HRPayrollSystem {
    pub fn new(contract: PayrollContractClient) -> Self {
        Self {
            contract,
            employees: HashMap::new(),
            token_addresses: HashMap::new(),
        }
    }
    
    pub fn add_employee(&mut self, employee: Employee) -> Result<(), PayrollError> {
        let token_address = self.token_addresses.get(&employee.currency)
            .ok_or(PayrollError::InvalidData)?;
        
        // Create payroll on blockchain
        self.contract.create_or_update_escrow(
            &self.get_employer_address(),
            &employee.address,
            token_address,
            &employee.salary,
            &employee.pay_frequency.to_seconds(),
            &employee.pay_frequency.to_seconds(),
        )?;
        
        // Store employee data
        self.employees.insert(employee.id.clone(), employee);
        
        Ok(())
    }
    
    pub fn process_payroll(&self) -> Result<Vec<String>, PayrollError> {
        let mut processed_employees = Vec::new();
        
        for (employee_id, employee) in &self.employees {
            if self.contract.is_eligible_for_disbursement(&employee.address) {
                match self.contract.disburse_salary(&self.get_employer_address(), &employee.address) {
                    Ok(()) => {
                        processed_employees.push(employee_id.clone());
                        println!("Paid employee: {}", employee.name);
                    }
                    Err(e) => {
                        eprintln!("Failed to pay employee {}: {:?}", employee.name, e);
                    }
                }
            }
        }
        
        Ok(processed_employees)
    }
    
    pub fn get_employee_status(&self, employee_id: &str) -> Option<EmployeeStatus> {
        let employee = self.employees.get(employee_id)?;
        let payroll = self.contract.get_payroll(&employee.address)?;
        
        Some(EmployeeStatus {
            employee_id: employee_id.to_string(),
            name: employee.name.clone(),
            next_payment_due: payroll.next_payout_timestamp,
            salary: payroll.amount,
            can_be_paid: self.contract.is_eligible_for_disbursement(&employee.address),
        })
    }
    
    fn get_employer_address(&self) -> Address {
        // Return employer's address
        self.contract.get_owner().expect("Contract owner not set")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmployeeStatus {
    pub employee_id: String,
    pub name: String,
    pub next_payment_due: u64,
    pub salary: i128,
    pub can_be_paid: bool,
}
```

### 2. Freelancer Platform Integration

```rust
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub client: Address,
    pub freelancer: Address,
    pub total_budget: i128,
    pub milestones: Vec<Milestone>,
    pub token: Address,
}

#[derive(Debug, Clone)]
pub struct Milestone {
    pub id: String,
    pub description: String,
    pub amount: i128,
    pub due_date: u64,
    pub completed: bool,
}

pub struct FreelancerPayrollSystem {
    contract: PayrollContractClient,
    projects: HashMap<String, Project>,
}

impl FreelancerPayrollSystem {
    pub fn new(contract: PayrollContractClient) -> Self {
        Self {
            contract,
            projects: HashMap::new(),
        }
    }
    
    pub fn create_project_payroll(&mut self, project: Project) -> Result<(), PayrollError> {
        // Calculate total project timeline
        let project_duration = self.calculate_project_duration(&project);
        
        // Create recurring payroll for milestone-based payments
        let milestone_frequency = project_duration / project.milestones.len() as u64;
        
        self.contract.create_or_update_escrow(
            &project.client,
            &project.freelancer,
            &project.token,
            &(project.total_budget / project.milestones.len() as i128),
            &milestone_frequency,
            &milestone_frequency,
        )?;
        
        self.projects.insert(project.id.clone(), project);
        
        Ok(())
    }
    
    pub fn complete_milestone(&mut self, project_id: &str, milestone_id: &str) -> Result<(), PayrollError> {
        let project = self.projects.get_mut(project_id)
            .ok_or(PayrollError::PayrollNotFound)?;
        
        // Mark milestone as completed
        for milestone in &mut project.milestones {
            if milestone.id == milestone_id {
                milestone.completed = true;
                break;
            }
        }
        
        // Process payment for completed milestone
        self.contract.disburse_salary(&project.client, &project.freelancer)?;
        
        Ok(())
    }
    
    pub fn get_project_status(&self, project_id: &str) -> Option<ProjectStatus> {
        let project = self.projects.get(project_id)?;
        let payroll = self.contract.get_payroll(&project.freelancer)?;
        
        let completed_milestones = project.milestones.iter()
            .filter(|m| m.completed)
            .count();
        
        Some(ProjectStatus {
            project_id: project_id.to_string(),
            freelancer: project.freelancer.clone(),
            progress: (completed_milestones as f64 / project.milestones.len() as f64) * 100.0,
            next_milestone_payment: payroll.next_payout_timestamp,
            total_paid: completed_milestones as i128 * (project.total_budget / project.milestones.len() as i128),
        })
    }
    
    fn calculate_project_duration(&self, project: &Project) -> u64 {
        if project.milestones.is_empty() {
            return 30 * 24 * 60 * 60; // Default 30 days
        }
        
        let earliest = project.milestones.iter().map(|m| m.due_date).min().unwrap_or(0);
        let latest = project.milestones.iter().map(|m| m.due_date).max().unwrap_or(0);
        
        latest - earliest
    }
}

#[derive(Debug, Clone)]
pub struct ProjectStatus {
    pub project_id: String,
    pub freelancer: Address,
    pub progress: f64,
    pub next_milestone_payment: u64,
    pub total_paid: i128,
}
```

### 3. Subscription Service Integration

```rust
#[derive(Debug, Clone)]
pub struct Subscription {
    pub id: String,
    pub customer: Address,
    pub service_provider: Address,
    pub amount: i128,
    pub billing_cycle: BillingCycle,
    pub token: Address,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub enum BillingCycle {
    Monthly,
    Quarterly,
    Annually,
}

impl BillingCycle {
    pub fn to_seconds(&self) -> u64 {
        match self {
            BillingCycle::Monthly => 30 * 24 * 60 * 60,
            BillingCycle::Quarterly => 90 * 24 * 60 * 60,
            BillingCycle::Annually => 365 * 24 * 60 * 60,
        }
    }
}

pub struct SubscriptionPayrollSystem {
    contract: PayrollContractClient,
    subscriptions: HashMap<String, Subscription>,
}

impl SubscriptionPayrollSystem {
    pub fn new(contract: PayrollContractClient) -> Self {
        Self {
            contract,
            subscriptions: HashMap::new(),
        }
    }
    
    pub fn create_subscription(&mut self, subscription: Subscription) -> Result<(), PayrollError> {
        // Create recurring payroll for subscription
        self.contract.create_or_update_escrow(
            &subscription.customer,
            &subscription.service_provider,
            &subscription.token,
            &subscription.amount,
            &subscription.billing_cycle.to_seconds(),
            &subscription.billing_cycle.to_seconds(),
        )?;
        
        self.subscriptions.insert(subscription.id.clone(), subscription);
        
        Ok(())
    }
    
    pub fn process_subscription_payments(&self) -> Result<Vec<String>, PayrollError> {
        let mut processed = Vec::new();
        
        for (sub_id, subscription) in &self.subscriptions {
            if !subscription.active {
                continue;
            }
            
            if self.contract.is_eligible_for_disbursement(&subscription.service_provider) {
                match self.contract.disburse_salary(&subscription.customer, &subscription.service_provider) {
                    Ok(()) => {
                        processed.push(sub_id.clone());
                        println!("Processed subscription payment: {}", sub_id);
                    }
                    Err(e) => {
                        eprintln!("Failed to process subscription {}: {:?}", sub_id, e);
                    }
                }
            }
        }
        
        Ok(processed)
    }
    
    pub fn cancel_subscription(&mut self, subscription_id: &str) -> Result<(), PayrollError> {
        if let Some(subscription) = self.subscriptions.get_mut(subscription_id) {
            subscription.active = false;
            // Note: In a real implementation, you'd need a way to disable the payroll
            // This might require additional contract functionality
            Ok(())
        } else {
            Err(PayrollError::PayrollNotFound)
        }
    }
}
```

## Integration Patterns

### 1. Event-Driven Architecture

```rust
use tokio::sync::mpsc;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum PayrollEvent {
    PayrollCreated { employee: Address, amount: i128 },
    SalaryDisbursed { employee: Address, amount: i128, timestamp: u64 },
    DepositMade { employer: Address, amount: i128 },
    PayrollUpdated { employee: Address, new_amount: i128 },
}

pub struct PayrollEventSystem {
    contract: PayrollContractClient,
    event_sender: mpsc::UnboundedSender<PayrollEvent>,
}

impl PayrollEventSystem {
    pub fn new(contract: PayrollContractClient) -> (Self, mpsc::UnboundedReceiver<PayrollEvent>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        
        let system = Self {
            contract,
            event_sender: sender,
        };
        
        (system, receiver)
    }
    
    pub async fn monitor_events(&self) -> Result<(), PayrollError> {
        // Monitor blockchain events and forward to event channel
        loop {
            let events = self.get_recent_events().await?;
            
            for event in events {
                match event {
                    ContractEvent::SalaryDisbursed { employer, employee, amount, timestamp } => {
                        let _ = self.event_sender.send(PayrollEvent::SalaryDisbursed {
                            employee,
                            amount,
                            timestamp,
                        });
                    }
                    ContractEvent::Deposit { employer, amount } => {
                        let _ = self.event_sender.send(PayrollEvent::DepositMade {
                            employer,
                            amount,
                        });
                    }
                    // Handle other events...
                }
            }
            
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        }
    }
    
    async fn get_recent_events(&self) -> Result<Vec<ContractEvent>, PayrollError> {
        // Implementation to fetch recent events from blockchain
        Ok(Vec::new())
    }
}

// Event handler
pub async fn handle_payroll_events(mut receiver: mpsc::UnboundedReceiver<PayrollEvent>) {
    while let Some(event) = receiver.recv().await {
        match event {
            PayrollEvent::SalaryDisbursed { employee, amount, timestamp } => {
                // Send notification to employee
                notify_employee_payment(&employee, amount, timestamp).await;
                
                // Update internal records
                update_payment_records(&employee, amount, timestamp).await;
            }
            PayrollEvent::DepositMade { employer, amount } => {
                // Update employer balance records
                update_employer_balance(&employer, amount).await;
                
                // Send confirmation to employer
                notify_employer_deposit(&employer, amount).await;
            }
            PayrollEvent::PayrollCreated { employee, amount } => {
                // Send welcome message to employee
                notify_new_employee(&employee, amount).await;
            }
            PayrollEvent::PayrollUpdated { employee, new_amount } => {
                // Notify employee of salary change
                notify_salary_change(&employee, new_amount).await;
            }
        }
    }
}

async fn notify_employee_payment(employee: &Address, amount: i128, timestamp: u64) {
    // Implementation for employee notification
    println!("Notifying employee {} of payment: {}", employee.to_string(), amount);
}

async fn update_payment_records(employee: &Address, amount: i128, timestamp: u64) {
    // Implementation for updating internal records
    println!("Updating payment records for employee {}", employee.to_string());
}

async fn update_employer_balance(employer: &Address, amount: i128) {
    // Implementation for updating employer balance
    println!("Updating balance for employer {}", employer.to_string());
}

async fn notify_employer_deposit(employer: &Address, amount: i128) {
    // Implementation for employer notification
    println!("Notifying employer {} of deposit: {}", employer.to_string(), amount);
}

async fn notify_new_employee(employee: &Address, amount: i128) {
    // Implementation for new employee notification
    println!("Welcome new employee {}, salary: {}", employee.to_string(), amount);
}

async fn notify_salary_change(employee: &Address, new_amount: i128) {
    // Implementation for salary change notification
    println!("Salary updated for employee {}: {}", employee.to_string(), new_amount);
}
```

### 2. Caching and State Management

```rust
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct CachedPayroll {
    pub payroll: Payroll,
    pub cached_at: Instant,
}

#[derive(Debug, Clone)]
pub struct CachedBalance {
    pub balance: i128,
    pub cached_at: Instant,
}

pub struct PayrollCache {
    payrolls: Arc<RwLock<HashMap<Address, CachedPayroll>>>,
    balances: Arc<RwLock<HashMap<(Address, Address), CachedBalance>>>,
    cache_ttl: Duration,
}

impl PayrollCache {
    pub fn new(cache_ttl: Duration) -> Self {
        Self {
            payrolls: Arc::new(RwLock::new(HashMap::new())),
            balances: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl,
        }
    }
    
    pub fn get_payroll(
        &self,
        contract: &PayrollContractClient,
        employee: &Address,
    ) -> Option<Payroll> {
        // Try cache first
        {
            let payrolls = self.payrolls.read().unwrap();
            if let Some(cached) = payrolls.get(employee) {
                if cached.cached_at.elapsed() < self.cache_ttl {
                    return Some(cached.payroll.clone());
                }
            }
        }
        
        // Fetch from contract
        if let Some(payroll) = contract.get_payroll(employee) {
            // Update cache
            {
                let mut payrolls = self.payrolls.write().unwrap();
                payrolls.insert(employee.clone(), CachedPayroll {
                    payroll: payroll.clone(),
                    cached_at: Instant::now(),
                });
            }
            Some(payroll)
        } else {
            None
        }
    }
    
    pub fn get_balance(
        &self,
        contract: &PayrollContractClient,
        employer: &Address,
        token: &Address,
    ) -> i128 {
        let key = (employer.clone(), token.clone());
        
        // Try cache first
        {
            let balances = self.balances.read().unwrap();
            if let Some(cached) = balances.get(&key) {
                if cached.cached_at.elapsed() < self.cache_ttl {
                    return cached.balance;
                }
            }
        }
        
        // Fetch from contract
        let balance = contract.get_employer_balance(employer, token);
        
        // Update cache
        {
            let mut balances = self.balances.write().unwrap();
            balances.insert(key, CachedBalance {
                balance,
                cached_at: Instant::now(),
            });
        }
        
        balance
    }
    
    pub fn invalidate_payroll(&self, employee: &Address) {
        let mut payrolls = self.payrolls.write().unwrap();
        payrolls.remove(employee);
    }
    
    pub fn invalidate_balance(&self, employer: &Address, token: &Address) {
        let mut balances = self.balances.write().unwrap();
        balances.remove(&(employer.clone(), token.clone()));
    }
    
    pub fn clear_expired(&self) {
        let now = Instant::now();
        
        // Clear expired payrolls
        {
            let mut payrolls = self.payrolls.write().unwrap();
            payrolls.retain(|_, cached| now.duration_since(cached.cached_at) < self.cache_ttl);
        }
        
        // Clear expired balances
        {
            let mut balances = self.balances.write().unwrap();
            balances.retain(|_, cached| now.duration_since(cached.cached_at) < self.cache_ttl);
        }
    }
}

pub struct PayrollManager {
    contract: PayrollContractClient,
    cache: PayrollCache,
}

impl PayrollManager {
    pub fn new(contract: PayrollContractClient) -> Self {
        Self {
            contract,
            cache: PayrollCache::new(Duration::from_secs(300)), // 5 minutes
        }
    }
    
    pub fn get_employee_info(&self, employee: &Address) -> Option<EmployeeInfo> {
        let payroll = self.cache.get_payroll(&self.contract, employee)?;
        
        Some(EmployeeInfo {
            employee: employee.clone(),
            employer: payroll.employer,
            salary: payroll.amount,
            next_payment: payroll.next_payout_timestamp,
            frequency: payroll.recurrence_frequency,
            eligible_for_payment: self.contract.is_eligible_for_disbursement(employee),
        })
    }
    
    pub fn get_employer_info(&self, employer: &Address, token: &Address) -> EmployerInfo {
        let balance = self.cache.get_balance(&self.contract, employer, token);
        
        EmployerInfo {
            employer: employer.clone(),
            token: token.clone(),
            balance,
            is_owner: self.contract.get_owner().map_or(false, |owner| owner == *employer),
        }
    }
    
    pub fn process_payment(&self, employer: &Address, employee: &Address) -> Result<(), PayrollError> {
        let result = self.contract.disburse_salary(employer, employee);
        
        if result.is_ok() {
            // Invalidate cache for affected entities
            self.cache.invalidate_payroll(employee);
            
            if let Some(payroll) = self.contract.get_payroll(employee) {
                self.cache.invalidate_balance(&payroll.employer, &payroll.token);
            }
        }
        
        result
    }
}

#[derive(Debug, Clone)]
pub struct EmployeeInfo {
    pub employee: Address,
    pub employer: Address,
    pub salary: i128,
    pub next_payment: u64,
    pub frequency: u64,
    pub eligible_for_payment: bool,
}

#[derive(Debug, Clone)]
pub struct EmployerInfo {
    pub employer: Address,
    pub token: Address,
    pub balance: i128,
    pub is_owner: bool,
}
```

## Frontend Examples

### 1. React Component

```typescript
// components/PayrollDashboard.tsx
import React, { useState, useEffect } from 'react';
import { usePayrollContract } from '../hooks/usePayrollContract';

interface Employee {
  address: string;
  name: string;
  salary: number;
  nextPayment: number;
  canBePaid: boolean;
}

const PayrollDashboard: React.FC = () => {
  const { contract, loading, error } = usePayrollContract();
  const [employees, setEmployees] = useState<Employee[]>([]);
  const [selectedEmployee, setSelectedEmployee] = useState<string | null>(null);
  
  useEffect(() => {
    loadEmployees();
  }, [contract]);
  
  const loadEmployees = async () => {
    if (!contract) return;
    
    try {
      // Load employees from your backend/database
      const employeeList = await fetchEmployees();
      
      // Enrich with blockchain data
      const enrichedEmployees = await Promise.all(
        employeeList.map(async (emp) => {
          const payroll = await contract.get_payroll(emp.address);
          const canBePaid = await contract.is_eligible_for_disbursement(emp.address);
          
          return {
            ...emp,
            salary: payroll?.amount || 0,
            nextPayment: payroll?.next_payout_timestamp || 0,
            canBePaid,
          };
        })
      );
      
      setEmployees(enrichedEmployees);
    } catch (err) {
      console.error('Failed to load employees:', err);
    }
  };
  
  const handlePayEmployee = async (employeeAddress: string) => {
    if (!contract) return;
    
    try {
      await contract.disburse_salary(employeeAddress);
      await loadEmployees(); // Refresh data
    } catch (err) {
      console.error('Failed to pay employee:', err);
    }
  };
  
  const formatCurrency = (amount: number) => {
    return new Intl.NumberFormat('en-US', {
      style: 'currency',
      currency: 'USD',
    }).format(amount / 10000000); // Assuming 7 decimal places
  };
  
  const formatDate = (timestamp: number) => {
    return new Date(timestamp * 1000).toLocaleDateString();
  };
  
  if (loading) return <div>Loading...</div>;
  if (error) return <div>Error: {error}</div>;
  
  return (
    <div className="payroll-dashboard">
      <h2>Payroll Dashboard</h2>
      
      <div className="employees-grid">
        {employees.map((employee) => (
          <div key={employee.address} className="employee-card">
            <h3>{employee.name}</h3>
            <p>Salary: {formatCurrency(employee.salary)}</p>
            <p>Next Payment: {formatDate(employee.nextPayment)}</p>
            <button
              onClick={() => handlePayEmployee(employee.address)}
              disabled={!employee.canBePaid}
              className={employee.canBePaid ? 'pay-button' : 'pay-button disabled'}
            >
              {employee.canBePaid ? 'Pay Now' : 'Not Due'}
            </button>
          </div>
        ))}
      </div>
    </div>
  );
};

export default PayrollDashboard;
```

### 2. Vue.js Component

```vue
<!-- components/EmployeePayroll.vue -->
<template>
  <div class="employee-payroll">
    <h2>My Payroll</h2>
    
    <div v-if="loading" class="loading">Loading...</div>
    <div v-else-if="error" class="error">{{ error }}</div>
    
    <div v-else-if="payrollInfo" class="payroll-info">
      <div class="info-card">
        <h3>Salary Information</h3>
        <p><strong>Monthly Salary:</strong> {{ formatCurrency(payrollInfo.amount) }}</p>
        <p><strong>Next Payment:</strong> {{ formatDate(payrollInfo.nextPayment) }}</p>
        <p><strong>Payment Frequency:</strong> {{ formatFrequency(payrollInfo.frequency) }}</p>
        <p><strong>Employer:</strong> {{ payrollInfo.employer }}</p>
      </div>
      
      <div class="actions">
        <button 
          @click="withdrawSalary" 
          :disabled="!canWithdraw"
          class="withdraw-button"
        >
          {{ canWithdraw ? 'Withdraw Salary' : 'Not Due Yet' }}
        </button>
      </div>
      
      <div class="payment-history">
        <h3>Payment History</h3>
        <div v-for="payment in paymentHistory" :key="payment.id" class="payment-item">
          <span>{{ formatDate(payment.timestamp) }}</span>
          <span>{{ formatCurrency(payment.amount) }}</span>
        </div>
      </div>
    </div>
    
    <div v-else class="no-payroll">
      <p>No payroll information found.</p>
    </div>
  </div>
</template>

<script setup>
import { ref, onMounted, computed } from 'vue';
import { usePayrollContract } from '@/composables/usePayrollContract';
import { useUserStore } from '@/stores/user';

const { contract, loading, error } = usePayrollContract();
const userStore = useUserStore();

const payrollInfo = ref(null);
const paymentHistory = ref([]);

const canWithdraw = computed(() => {
  return payrollInfo.value && payrollInfo.value.eligibleForPayment;
});

onMounted(async () => {
  await loadPayrollInfo();
  await loadPaymentHistory();
});

const loadPayrollInfo = async () => {
  if (!contract.value || !userStore.userAddress) return;
  
  try {
    const payroll = await contract.value.get_payroll(userStore.userAddress);
    const eligible = await contract.value.is_eligible_for_disbursement(userStore.userAddress);
    
    if (payroll) {
      payrollInfo.value = {
        ...payroll,
        eligibleForPayment: eligible,
      };
    }
  } catch (err) {
    console.error('Failed to load payroll info:', err);
  }
};

const loadPaymentHistory = async () => {
  // Load payment history from events or backend
  try {
    const history = await fetchPaymentHistory(userStore.userAddress);
    paymentHistory.value = history;
  } catch (err) {
    console.error('Failed to load payment history:', err);
  }
};

const withdrawSalary = async () => {
  if (!contract.value || !userStore.userAddress) return;
  
  try {
    await contract.value.employee_withdraw(userStore.userAddress);
    await loadPayrollInfo();
    await loadPaymentHistory();
  } catch (err) {
    console.error('Failed to withdraw salary:', err);
  }
};

const formatCurrency = (amount) => {
  return new Intl.NumberFormat('en-US', {
    style: 'currency',
    currency: 'USD',
  }).format(amount / 10000000);
};

const formatDate = (timestamp) => {
  return new Date(timestamp * 1000).toLocaleDateString();
};

const formatFrequency = (seconds) => {
  const days = seconds / (24 * 60 * 60);
  if (days === 7) return 'Weekly';
  if (days === 14) return 'Bi-weekly';
  if (days === 30) return 'Monthly';
  if (days === 90) return 'Quarterly';
  return `Every ${days} days`;
};
</script>

<style scoped>
.employee-payroll {
  max-width: 800px;
  margin: 0 auto;
  padding: 20px;
}

.info-card {
  background: #f5f5f5;
  border-radius: 8px;
  padding: 20px;
  margin-bottom: 20px;
}

.withdraw-button {
  background: #4CAF50;
  color: white;
  border: none;
  padding: 12px 24px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 16px;
}

.withdraw-button:disabled {
  background: #ccc;
  cursor: not-allowed;
}

.payment-item {
  display: flex;
  justify-content: space-between;
  padding: 10px;
  border-bottom: 1px solid #eee;
}

.loading, .error {
  text-align: center;
  padding: 20px;
}

.error {
  color: #f44336;
}
</style>
```

## Backend Examples

### 1. Express.js API

```javascript
// routes/payroll.js
const express = require('express');
const { PayrollService } = require('../services/PayrollService');
const { authenticateToken } = require('../middleware/auth');

const router = express.Router();
const payrollService = new PayrollService();

// Get employee payroll information
router.get('/employee/:address', authenticateToken, async (req, res) => {
  try {
    const { address } = req.params;
    const payrollInfo = await payrollService.getEmployeePayroll(address);
    
    if (!payrollInfo) {
      return res.status(404).json({ error: 'Payroll not found' });
    }
    
    res.json(payrollInfo);
  } catch (error) {
    console.error('Error fetching payroll:', error);
    res.status(500).json({ error: 'Internal server error' });
  }
});

// Create new payroll
router.post('/create', authenticateToken, async (req, res) => {
  try {
    const { employeeAddress, salary, frequency, tokenAddress } = req.body;
    
    // Validate input
    if (!employeeAddress || !salary || !frequency || !tokenAddress) {
      return res.status(400).json({ error: 'Missing required fields' });
    }
    
    const payroll = await payrollService.createPayroll({
      employeeAddress,
      salary,
      frequency,
      tokenAddress,
      employerAddress: req.user.address,
    });
    
    res.status(201).json(payroll);
  } catch (error) {
    console.error('Error creating payroll:', error);
    res.status(500).json({ error: 'Failed to create payroll' });
  }
});

// Process salary payment
router.post('/pay/:employeeAddress', authenticateToken, async (req, res) => {
  try {
    const { employeeAddress } = req.params;
    const employerAddress = req.user.address;
    
    const result = await payrollService.processSalaryPayment(employerAddress, employeeAddress);
    
    res.json(result);
  } catch (error) {
    console.error('Error processing payment:', error);
    res.status(500).json({ error: 'Failed to process payment' });
  }
});

// Get employer dashboard data
router.get('/dashboard', authenticateToken, async (req, res) => {
  try {
    const employerAddress = req.user.address;
    const dashboard = await payrollService.getEmployerDashboard(employerAddress);
    
    res.json(dashboard);
  } catch (error) {
    console.error('Error fetching dashboard:', error);
    res.status(500).json({ error: 'Failed to fetch dashboard data' });
  }
});

// Bulk process payments
router.post('/bulk-pay', authenticateToken, async (req, res) => {
  try {
    const { employeeAddresses } = req.body;
    const employerAddress = req.user.address;
    
    if (!Array.isArray(employeeAddresses)) {
      return res.status(400).json({ error: 'employeeAddresses must be an array' });
    }
    
    const results = await payrollService.bulkProcessPayments(employerAddress, employeeAddresses);
    
    res.json(results);
  } catch (error) {
    console.error('Error bulk processing payments:', error);
    res.status(500).json({ error: 'Failed to process bulk payments' });
  }
});

module.exports = router;
```

### 2. FastAPI (Python) Service

```python
# payroll_api.py
from fastapi import FastAPI, HTTPException, Depends, BackgroundTasks
from fastapi.security import HTTPBearer, HTTPAuthorizationCredentials
from pydantic import BaseModel
from typing import List, Optional
import asyncio
from datetime import datetime

from services.payroll_service import PayrollService
from models.payroll_models import PayrollInfo, PaymentRequest, BulkPaymentRequest

app = FastAPI(title="Payroll API", version="1.0.0")
security = HTTPBearer()

payroll_service = PayrollService()

class PayrollCreateRequest(BaseModel):
    employee_address: str
    salary: int
    frequency: int
    token_address: str

class PaymentResponse(BaseModel):
    success: bool
    transaction_hash: Optional[str]
    error: Optional[str]

@app.get("/payroll/employee/{employee_address}")
async def get_employee_payroll(
    employee_address: str,
    credentials: HTTPAuthorizationCredentials = Depends(security)
):
    """Get payroll information for an employee"""
    try:
        # Validate token and get user
        user = await validate_token(credentials.credentials)
        
        payroll_info = await payroll_service.get_employee_payroll(employee_address)
        
        if not payroll_info:
            raise HTTPException(status_code=404, detail="Payroll not found")
            
        return payroll_info
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.post("/payroll/create")
async def create_payroll(
    request: PayrollCreateRequest,
    credentials: HTTPAuthorizationCredentials = Depends(security)
):
    """Create a new payroll for an employee"""
    try:
        user = await validate_token(credentials.credentials)
        
        payroll = await payroll_service.create_payroll(
            employer_address=user.address,
            employee_address=request.employee_address,
            salary=request.salary,
            frequency=request.frequency,
            token_address=request.token_address
        )
        
        return payroll
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.post("/payroll/pay/{employee_address}")
async def pay_employee(
    employee_address: str,
    background_tasks: BackgroundTasks,
    credentials: HTTPAuthorizationCredentials = Depends(security)
):
    """Process salary payment for an employee"""
    try:
        user = await validate_token(credentials.credentials)
        
        # Add payment processing to background tasks
        background_tasks.add_task(
            process_payment_async,
            user.address,
            employee_address
        )
        
        return {"message": "Payment processing started", "status": "pending"}
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.get("/payroll/dashboard")
async def get_dashboard(
    credentials: HTTPAuthorizationCredentials = Depends(security)
):
    """Get employer dashboard data"""
    try:
        user = await validate_token(credentials.credentials)
        
        dashboard = await payroll_service.get_employer_dashboard(user.address)
        
        return dashboard
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.post("/payroll/bulk-pay")
async def bulk_pay_employees(
    request: BulkPaymentRequest,
    background_tasks: BackgroundTasks,
    credentials: HTTPAuthorizationCredentials = Depends(security)
):
    """Process bulk salary payments"""
    try:
        user = await validate_token(credentials.credentials)
        
        # Add bulk payment processing to background tasks
        background_tasks.add_task(
            process_bulk_payments_async,
            user.address,
            request.employee_addresses
        )
        
        return {
            "message": "Bulk payment processing started",
            "employee_count": len(request.employee_addresses),
            "status": "pending"
        }
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.get("/payroll/events")
async def get_payroll_events(
    limit: int = 100,
    offset: int = 0,
    credentials: HTTPAuthorizationCredentials = Depends(security)
):
    """Get recent payroll events"""
    try:
        user = await validate_token(credentials.credentials)
        
        events = await payroll_service.get_payroll_events(
            user.address,
            limit=limit,
            offset=offset
        )
        
        return events
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

# Background task functions
async def process_payment_async(employer_address: str, employee_address: str):
    """Process individual payment in background"""
    try:
        result = await payroll_service.process_salary_payment(
            employer_address,
            employee_address
        )
        
        # Send notification
        await notify_payment_processed(employee_address, result)
        
    except Exception as e:
        # Log error and send failure notification
        await notify_payment_failed(employee_address, str(e))

async def process_bulk_payments_async(employer_address: str, employee_addresses: List[str]):
    """Process bulk payments in background"""
    try:
        results = await payroll_service.bulk_process_payments(
            employer_address,
            employee_addresses
        )
        
        # Send bulk notification
        await notify_bulk_payment_processed(employer_address, results)
        
    except Exception as e:
        # Log error and send failure notification
        await notify_bulk_payment_failed(employer_address, str(e))

# Helper functions
async def validate_token(token: str):
    """Validate authentication token"""
    # Implementation depends on your auth system
    pass

async def notify_payment_processed(employee_address: str, result):
    """Send notification about processed payment"""
    # Implementation for notifications
    pass

async def notify_payment_failed(employee_address: str, error: str):
    """Send notification about failed payment"""
    # Implementation for error notifications
    pass

async def notify_bulk_payment_processed(employer_address: str, results):
    """Send notification about bulk payment processing"""
    # Implementation for bulk notifications
    pass

async def notify_bulk_payment_failed(employer_address: str, error: str):
    """Send notification about bulk payment failure"""
    # Implementation for bulk error notifications
    pass
```

## Testing Examples

### 1. Unit Tests

```rust
#[cfg(test)]
mod payroll_tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};
    
    fn create_test_setup() -> (Env, PayrollContractClient, Address, Address, Address, Address) {
        let env = Env::default();
        let contract_address = env.register_contract(None, PayrollContract);
        let contract = PayrollContractClient::new(&env, &contract_address);
        
        let owner = Address::generate(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token = Address::generate(&env);
        
        // Initialize contract
        contract.initialize(&owner);
        
        (env, contract, owner, employer, employee, token)
    }
    
    #[test]
    fn test_complete_payroll_workflow() {
        let (env, contract, owner, employer, employee, token) = create_test_setup();
        
        // Test 1: Create payroll
        let salary = 5000_0000000; // 5000 with 7 decimals
        let monthly_freq = 30 * 24 * 60 * 60;
        
        let payroll = contract.create_or_update_escrow(
            &owner, // Only owner can create
            &employee,
            &token,
            &salary,
            &monthly_freq,
            &monthly_freq,
        ).unwrap();
        
        assert_eq!(payroll.amount, salary);
        assert_eq!(payroll.employer, owner);
        assert_eq!(payroll.recurrence_frequency, monthly_freq);
        
        // Test 2: Deposit funds
        contract.deposit_tokens(&owner, &token, &(salary * 3)).unwrap();
        
        let balance = contract.get_employer_balance(&owner, &token);
        assert_eq!(balance, salary * 3);
        
        // Test 3: Check eligibility (should not be eligible immediately)
        let eligible = contract.is_eligible_for_disbursement(&employee);
        assert!(!eligible);
        
        // Test 4: Fast forward time
        env.ledger().with_mut(|li| {
            li.timestamp = monthly_freq + 1;
        });
        
        let eligible_after = contract.is_eligible_for_disbursement(&employee);
        assert!(eligible_after);
        
        // Test 5: Process payment
        contract.disburse_salary(&owner, &employee).unwrap();
        
        // Verify balance decreased
        let balance_after = contract.get_employer_balance(&owner, &token);
        assert_eq!(balance_after, salary * 2);
        
        // Test 6: Verify next payment time updated
        let updated_payroll = contract.get_payroll(&employee).unwrap();
        assert_eq!(updated_payroll.next_payout_timestamp, (monthly_freq + 1) + monthly_freq);
        
        // Test 7: Employee cannot be paid again until next period
        let eligible_again = contract.is_eligible_for_disbursement(&employee);
        assert!(!eligible_again);
    }
    
    #[test]
    fn test_error_conditions() {
        let (env, contract, owner, employer, employee, token) = create_test_setup();
        
        // Test 1: Cannot create payroll with invalid data
        let result = contract.create_or_update_escrow(
            &owner,
            &employee,
            &token,
            &0, // Invalid amount
            &1000,
            &1000,
        );
        assert!(result.is_err());
        
        // Test 2: Cannot disburse without payroll
        let result = contract.disburse_salary(&owner, &employee);
        assert!(result.is_err());
        
        // Test 3: Cannot disburse without funds
        let salary = 1000_0000000;
        let monthly_freq = 30 * 24 * 60 * 60;
        
        contract.create_or_update_escrow(
            &owner,
            &employee,
            &token,
            &salary,
            &monthly_freq,
            &monthly_freq,
        ).unwrap();
        
        // Fast forward time
        env.ledger().with_mut(|li| {
            li.timestamp = monthly_freq + 1;
        });
        
        let result = contract.disburse_salary(&owner, &employee);
        assert!(result.is_err());
    }
    
    #[test]
    fn test_bulk_operations() {
        let (env, contract, owner, employer, employee1, token) = create_test_setup();
        let employee2 = Address::generate(&env);
        let employee3 = Address::generate(&env);
        
        let salary = 1000_0000000;
        let monthly_freq = 30 * 24 * 60 * 60;
        
        // Create payrolls for multiple employees
        for employee in [&employee1, &employee2, &employee3] {
            contract.create_or_update_escrow(
                &owner,
                employee,
                &token,
                &salary,
                &monthly_freq,
                &monthly_freq,
            ).unwrap();
        }
        
        // Deposit sufficient funds
        contract.deposit_tokens(&owner, &token, &(salary * 5)).unwrap();
        
        // Fast forward time
        env.ledger().with_mut(|li| {
            li.timestamp = monthly_freq + 1;
        });
        
        // Process bulk payments
        let employees = vec![&env, employee1, employee2, employee3];
        let processed = contract.process_recurring_disbursements(&owner, &employees);
        
        assert_eq!(processed.len(), 3);
        
        // Verify all employees were paid
        for employee in [&employee1, &employee2, &employee3] {
            let payroll = contract.get_payroll(employee).unwrap();
            assert_eq!(payroll.last_payment_time, monthly_freq + 1);
        }
    }
}
```

### 2. Integration Tests

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;
    use tokio_test;
    
    #[tokio::test]
    async fn test_payroll_system_integration() {
        // Set up test environment
        let env = Env::default();
        let contract = setup_contract(&env).await;
        
        // Test HR system integration
        let hr_system = HRPayrollSystem::new(contract.clone());
        
        // Create test employees
        let employee1 = create_test_employee("emp1", 5000_0000000);
        let employee2 = create_test_employee("emp2", 3000_0000000);
        
        // Add employees to HR system
        hr_system.add_employee(employee1.clone()).await.unwrap();
        hr_system.add_employee(employee2.clone()).await.unwrap();
        
        // Fund payroll
        fund_payroll(&contract, &get_employer_address(), 20000_0000000).await.unwrap();
        
        // Fast forward time
        advance_time(&env, 30 * 24 * 60 * 60).await;
        
        // Process payroll
        let processed = hr_system.process_payroll().await.unwrap();
        assert_eq!(processed.len(), 2);
        
        // Verify payments
        verify_employee_paid(&contract, &employee1.address).await;
        verify_employee_paid(&contract, &employee2.address).await;
    }
    
    #[tokio::test]
    async fn test_event_monitoring() {
        let env = Env::default();
        let contract = setup_contract(&env).await;
        
        // Set up event monitoring
        let (event_system, mut event_receiver) = PayrollEventSystem::new(contract.clone());
        
        // Start monitoring in background
        let _monitor_handle = tokio::spawn(async move {
            event_system.monitor_events().await.unwrap();
        });
        
        // Perform operations that generate events
        create_test_payroll(&contract).await;
        deposit_funds(&contract, 10000_0000000).await;
        process_payment(&contract).await;
        
        // Verify events received
        let mut events_received = 0;
        while let Ok(event) = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            event_receiver.recv()
        ).await {
            match event {
                Some(PayrollEvent::PayrollCreated { .. }) => events_received += 1,
                Some(PayrollEvent::DepositMade { .. }) => events_received += 1,
                Some(PayrollEvent::SalaryDisbursed { .. }) => events_received += 1,
                _ => {}
            }
        }
        
        assert!(events_received >= 3);
    }
    
    async fn setup_contract(env: &Env) -> PayrollContractClient {
        // Implementation for contract setup
        unimplemented!()
    }
    
    async fn create_test_employee(id: &str, salary: i128) -> Employee {
        // Implementation for creating test employee
        unimplemented!()
    }
    
    async fn fund_payroll(contract: &PayrollContractClient, employer: &Address, amount: i128) -> Result<(), PayrollError> {
        // Implementation for funding payroll
        unimplemented!()
    }
    
    async fn advance_time(env: &Env, seconds: u64) {
        // Implementation for advancing time
        unimplemented!()
    }
    
    async fn verify_employee_paid(contract: &PayrollContractClient, employee: &Address) {
        // Implementation for verifying payment
        unimplemented!()
    }
}
```

These examples provide a comprehensive foundation for integrating with the StellopayCore contract. They cover various use cases from basic payroll management to complex HR system integration, including proper error handling, caching, and event monitoring patterns.

For more specific implementation details or additional examples, please refer to the [API Documentation](../api/README.md) and [Best Practices](../best-practices/README.md) sections.
