# API Documentation

Complete reference for all StellopayCore contract functions, events, and data structures.

## Contract Functions

### Administrative Functions

#### `initialize(env: Env, owner: Address)`

Initializes the contract with an owner/admin address. This should be called once when deploying the contract.

**Parameters:**
- `env`: Environment context
- `owner`: Address of the contract owner

**Authorization:** Requires `owner` signature

**Errors:**
- Panics if contract is already initialized

**Example:**
```rust
contract.initialize(&env, &owner_address);
```

#### `pause(env: Env, caller: Address) -> Result<(), PayrollError>`

Pauses all contract operations. Only callable by the contract owner.

**Parameters:**
- `env`: Environment context
- `caller`: Address attempting to pause (must be owner)

**Authorization:** Requires `caller` signature

**Returns:** 
- `Ok(())` on success
- `PayrollError::Unauthorized` if caller is not the owner

**Events:** Emits `PAUSED_EVENT`

#### `unpause(env: Env, caller: Address) -> Result<(), PayrollError>`

Resumes contract operations. Only callable by the contract owner.

**Parameters:**
- `env`: Environment context
- `caller`: Address attempting to unpause (must be owner)

**Authorization:** Requires `caller` signature

**Returns:** 
- `Ok(())` on success
- `PayrollError::Unauthorized` if caller is not the owner

**Events:** Emits `UNPAUSED_EVENT`

#### `is_paused(env: Env) -> bool`

Checks if the contract is currently paused.

**Parameters:**
- `env`: Environment context

**Returns:** `true` if paused, `false` otherwise

#### `get_owner(env: Env) -> Option<Address>`

Returns the current owner of the contract.

**Parameters:**
- `env`: Environment context

**Returns:** `Some(Address)` if owner is set, `None` otherwise

#### `transfer_ownership(env: Env, caller: Address, new_owner: Address) -> Result<(), PayrollError>`

Transfers ownership to a new address.

**Parameters:**
- `env`: Environment context
- `caller`: Current owner address
- `new_owner`: New owner address

**Authorization:** Requires `caller` signature

**Returns:** 
- `Ok(())` on success
- `PayrollError::Unauthorized` if caller is not the current owner

### Token Management Functions

#### `add_supported_token(env: Env, token: Address) -> Result<(), PayrollError>`

Adds a new supported token for payroll payments.

**Parameters:**
- `env`: Environment context
- `token`: Token contract address

**Authorization:** Requires owner signature

**Returns:** 
- `Ok(())` on success
- `PayrollError::Unauthorized` if caller is not the owner

#### `remove_supported_token(env: Env, token: Address) -> Result<(), PayrollError>`

Removes a token from the supported tokens list.

**Parameters:**
- `env`: Environment context
- `token`: Token contract address

**Authorization:** Requires owner signature

**Returns:** 
- `Ok(())` on success
- `PayrollError::Unauthorized` if caller is not the owner

#### `is_token_supported(env: Env, token: Address) -> bool`

Checks if a token is supported for payroll payments.

**Parameters:**
- `env`: Environment context
- `token`: Token contract address

**Returns:** `true` if supported, `false` otherwise

#### `get_token_metadata(env: Env, token: Address) -> Option<u32>`

Returns token metadata (decimals).

**Parameters:**
- `env`: Environment context
- `token`: Token contract address

**Returns:** `Some(decimals)` if metadata exists, `None` otherwise

### Payroll Management Functions

#### `create_or_update_escrow(env: Env, employer: Address, employee: Address, token: Address, amount: i128, interval: u64, recurrence_frequency: u64) -> Result<Payroll, PayrollError>`

Creates or updates a payroll escrow for an employee.

**Parameters:**
- `env`: Environment context
- `employer`: Employer address
- `employee`: Employee address
- `token`: Payment token address
- `amount`: Payment amount per disbursement
- `interval`: Legacy parameter (kept for compatibility)
- `recurrence_frequency`: Time between payments in seconds

**Authorization:** Requires `employer` signature

**Returns:** 
- `Ok(Payroll)` on success
- `PayrollError::Unauthorized` if not authorized
- `PayrollError::InvalidData` if parameters are invalid
- `PayrollError::ContractPaused` if contract is paused

**Events:** Emits `UPDATED_EVENT`

**Requirements:**
- Contract must not be paused
- Only owner can create new payrolls
- Only owner or existing employer can update payrolls
- Amount must be positive
- Interval and recurrence_frequency must be greater than 0

#### `get_payroll(env: Env, employee: Address) -> Option<Payroll>`

Retrieves payroll information for an employee.

**Parameters:**
- `env`: Environment context
- `employee`: Employee address

**Returns:** `Some(Payroll)` if exists, `None` otherwise

#### `get_next_payout_timestamp(env: Env, employee: Address) -> Option<u64>`

Gets the next scheduled payout timestamp for an employee.

**Parameters:**
- `env`: Environment context
- `employee`: Employee address

**Returns:** `Some(timestamp)` if payroll exists, `None` otherwise

#### `get_recurrence_frequency(env: Env, employee: Address) -> Option<u64>`

Gets the recurrence frequency for an employee's payroll.

**Parameters:**
- `env`: Environment context
- `employee`: Employee address

**Returns:** `Some(frequency)` in seconds if payroll exists, `None` otherwise

#### `is_eligible_for_disbursement(env: Env, employee: Address) -> bool`

Checks if an employee is eligible for salary disbursement.

**Parameters:**
- `env`: Environment context
- `employee`: Employee address

**Returns:** `true` if eligible (next payout time reached), `false` otherwise

### Financial Functions

#### `deposit_tokens(env: Env, employer: Address, token: Address, amount: i128) -> Result<(), PayrollError>`

Deposits tokens to an employer's salary pool.

**Parameters:**
- `env`: Environment context
- `employer`: Employer address
- `token`: Token contract address
- `amount`: Amount to deposit

**Authorization:** Requires `employer` signature

**Returns:** 
- `Ok(())` on success
- `PayrollError::InvalidData` if amount is not positive
- `PayrollError::TransferFailed` if token transfer fails
- `PayrollError::ContractPaused` if contract is paused

**Events:** Emits `DEPOSIT_EVENT`

#### `get_employer_balance(env: Env, employer: Address, token: Address) -> i128`

Returns an employer's token balance in the contract.

**Parameters:**
- `env`: Environment context
- `employer`: Employer address
- `token`: Token contract address

**Returns:** Balance amount (0 if no balance)

#### `disburse_salary(env: Env, caller: Address, employee: Address) -> Result<(), PayrollError>`

Disburses salary to an employee.

**Parameters:**
- `env`: Environment context
- `caller`: Address initiating the disbursement (must be employer)
- `employee`: Employee address

**Authorization:** Requires `caller` signature

**Returns:** 
- `Ok(())` on success
- `PayrollError::Unauthorized` if caller is not the employer
- `PayrollError::PayrollNotFound` if no payroll exists
- `PayrollError::NextPayoutTimeNotReached` if payout time not reached
- `PayrollError::InsufficientBalance` if employer has insufficient balance
- `PayrollError::TransferFailed` if token transfer fails
- `PayrollError::ContractPaused` if contract is paused

**Events:** Emits `SalaryDisbursed` event

#### `employee_withdraw(env: Env, employee: Address) -> Result<(), PayrollError>`

Allows an employee to withdraw their salary.

**Parameters:**
- `env`: Environment context
- `employee`: Employee address

**Authorization:** Requires `employee` signature

**Returns:** Same as `disburse_salary`

#### `process_recurring_disbursements(env: Env, caller: Address, employees: Vec<Address>) -> Vec<Address>`

Processes recurring disbursements for multiple employees.

**Parameters:**
- `env`: Environment context
- `caller`: Address initiating the process (must be owner)
- `employees`: List of employee addresses to process

**Authorization:** Requires `caller` signature (must be owner)

**Returns:** List of successfully processed employee addresses

**Events:** Emits `RECUR_EVENT` and individual `SalaryDisbursed` events

## Data Structures

### `Payroll`

Represents a payroll configuration for an employee.

```rust
pub struct Payroll {
    pub employer: Address,           // Employer address
    pub token: Address,             // Payment token address
    pub amount: i128,               // Payment amount per disbursement
    pub interval: u64,              // Legacy interval field
    pub last_payment_time: u64,     // Timestamp of last payment
    pub recurrence_frequency: u64,   // Frequency in seconds
    pub next_payout_timestamp: u64, // Next scheduled payout
}
```

### `PayrollError`

Error types that can be returned by contract functions.

```rust
pub enum PayrollError {
    Unauthorized = 1,                    // Non-authorized access
    IntervalNotReached = 2,              // Payment interval not reached
    InvalidData = 3,                     // Invalid input data
    PayrollNotFound = 4,                 // Payroll record not found
    TransferFailed = 5,                  // Token transfer failed
    InsufficientBalance = 6,             // Insufficient employer balance
    ContractPaused = 7,                  // Contract is paused
    InvalidRecurrenceFrequency = 8,      // Invalid recurrence frequency
    NextPayoutTimeNotReached = 9,        // Next payout time not reached
    NoEligibleEmployees = 10,            // No eligible employees
}
```

## Events

### `PAUSED_EVENT`

Emitted when the contract is paused.

**Topics:** `("paused",)`
**Data:** `caller: Address`

### `UNPAUSED_EVENT`

Emitted when the contract is unpaused.

**Topics:** `("unpaused",)`
**Data:** `caller: Address`

### `DEPOSIT_EVENT`

Emitted when tokens are deposited to an employer's balance.

**Topics:** `("deposit", employer: Address, token: Address)`
**Data:** `amount: i128`

### `UPDATED_EVENT`

Emitted when a payroll is created or updated.

**Topics:** `("updated",)`
**Data:** `(employer: Address, employee: Address, recurrence_frequency: u64)`

### `RECUR_EVENT`

Emitted when recurring disbursements are processed.

**Topics:** `("recur",)`
**Data:** `(caller: Address, processed_count: u32)`

### `SalaryDisbursed`

Emitted when salary is disbursed to an employee.

**Topics:** `("SalaryDisbursed",)`
**Data:** 
```rust
pub struct SalaryDisbursed {
    pub employer: Address,
    pub employee: Address,
    pub token: Address,
    pub amount: i128,
    pub timestamp: u64,
}
```

## Storage Keys

The contract uses the following storage keys:

```rust
pub enum DataKey {
    // Payroll data, keyed by employee address
    PayrollEmployer(Address),
    PayrollToken(Address),
    PayrollAmount(Address),
    PayrollInterval(Address),
    PayrollLastPayment(Address),
    PayrollRecurrenceFrequency(Address),
    PayrollNextPayoutTimestamp(Address),
    
    // Employer balance, keyed by (employer, token)
    Balance(Address, Address),
    
    // Admin data
    Owner,
    Paused,
    
    // Token support
    SupportedToken(Address),
    TokenMetadata(Address),
}
```

## Common Patterns

### Checking Contract Status

```rust
// Always check if contract is paused before operations
if contract.is_paused(&env) {
    return Err(PayrollError::ContractPaused);
}
```

### Authorization Pattern

```rust
// Most functions require caller authentication
caller.require_auth();

// Owner-only functions check ownership
let owner = contract.get_owner(&env).ok_or(PayrollError::Unauthorized)?;
if caller != owner {
    return Err(PayrollError::Unauthorized);
}
```

### Token Support Pattern

```rust
// Check token support before operations
if !contract.is_token_supported(&env, &token) {
    return Err(PayrollError::InvalidData);
}
```
