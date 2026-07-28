# Storage Optimization Summary

## Overview
This document summarizes the comprehensive storage optimization changes made to the StelloPay payroll contract to improve gas efficiency, scalability, and query performance.

## Key Improvements

### 1. Consolidated Storage Structure

**Before:** Payroll data was fragmented across 7 separate storage keys per employee:
- `PayrollEmployer(Address)`
- `PayrollToken(Address)`
- `PayrollAmount(Address)`
- `PayrollInterval(Address)`
- `PayrollLastPayment(Address)`
- `PayrollRecurrenceFrequency(Address)`
- `PayrollNextPayoutTimestamp(Address)`

**After:** Complete Payroll struct stored in a single key:
- `Payroll(Address)` → `Payroll` struct

**Benefits:**
- Reduced storage operations from 7 to 1 per employee
- Improved gas efficiency for read/write operations
- Simplified data management and consistency

### 2. Storage Compression

**New CompactPayroll Structure:**
```rust
pub struct CompactPayroll {
    pub employer: Address,
    pub token: Address,
    pub amount: i128,
    pub interval: u32,           // Reduced from u64
    pub last_payment_time: u64,
    pub recurrence_frequency: u32, // Reduced from u64
    pub next_payout_timestamp: u64,
}
```

**Benefits:**
- Reduced storage size by using u32 instead of u64 for interval and recurrence_frequency
- Maintains compatibility with existing Payroll struct through conversion functions
- Automatic fallback to full Payroll struct for backward compatibility

### 3. Indexing System

**New Index Storage Keys:**
- `EmployerEmployees(Address)` → `Vec<Employee>` - Maps employer to all their employees
- `TokenEmployees(Address)` → `Vec<Employee>` - Maps token to all employees using it

**Benefits:**
- Efficient queries by employer or token
- No need to scan all payrolls to find specific relationships
- Enables batch operations on related data

### 4. Batch Processing Functions

**New Functions:**
- `batch_create_escrows(employer, Vec<PayrollInput>)` - Create multiple payrolls in one transaction
- `batch_disburse_salaries(caller, Vec<Address>)` - Disburse salaries to multiple employees

**Benefits:**
- Reduced gas costs for bulk operations
- Better transaction throughput
- Simplified client-side operations

### 5. Enhanced Query Functions

**New Query Functions:**
- `get_employer_employees(employer)` - Get all employees for an employer
- `get_token_employees(token)` - Get all employees using a specific token
- `remove_payroll(caller, employee)` - Remove payroll with automatic index cleanup

## Implementation Details

### Storage Migration Strategy
The implementation includes backward compatibility:
1. New payrolls are stored using the compact format
2. Existing payrolls can be read in both formats
3. Automatic conversion between Payroll and CompactPayroll structs

### Index Management
- Automatic index updates when payrolls are created/updated/removed
- Duplicate prevention in indexes
- Clean removal of empty indexes

### Gas Optimization
- Single storage operation per payroll instead of 7
- Compact data types reduce storage costs
- Batch operations reduce transaction overhead
- Efficient indexing reduces query costs

## API Changes

### New Functions Added
```rust
// Batch operations
pub fn batch_create_escrows(env: Env, employer: Address, payroll_inputs: Vec<PayrollInput>) -> Result<Vec<Payroll>, PayrollError>
pub fn batch_disburse_salaries(env: Env, caller: Address, employees: Vec<Address>) -> Result<Vec<Address>, PayrollError>

// Query functions
pub fn get_employer_employees(env: Env, employer: Address) -> Vec<Address>
pub fn get_token_employees(env: Env, token: Address) -> Vec<Address>

// Management functions
pub fn remove_payroll(env: Env, caller: Address, employee: Address) -> Result<(), PayrollError>
```

### New Data Structures
```rust
pub struct PayrollInput {
    pub employee: Address,
    pub token: Address,
    pub amount: i128,
    pub interval: u64,
    pub recurrence_frequency: u64,
}

pub struct CompactPayroll {
    pub employer: Address,
    pub token: Address,
    pub amount: i128,
    pub interval: u32,
    pub last_payment_time: u64,
    pub recurrence_frequency: u32,
    pub next_payout_timestamp: u64,
}
```

## Performance Improvements

### Storage Efficiency
- **Before:** 7 storage operations per employee
- **After:** 1 storage operation per employee
- **Improvement:** ~85% reduction in storage operations

### Gas Costs
- Reduced storage read/write costs
- More efficient batch operations
- Optimized data types reduce storage size

### Query Performance
- O(1) lookup for employer → employees mapping
- O(1) lookup for token → employees mapping
- No need to scan all payrolls for specific queries

## Backward Compatibility

The implementation maintains full backward compatibility:
- Existing payrolls continue to work
- All existing functions remain unchanged
- Automatic conversion between old and new formats
- Gradual migration path available

## Testing Recommendations

1. **Unit Tests:** Test all new batch functions
2. **Integration Tests:** Verify index consistency
3. **Gas Tests:** Measure actual gas savings
4. **Migration Tests:** Ensure backward compatibility
5. **Performance Tests:** Verify query performance improvements

## Future Enhancements

1. **Pagination:** Add pagination to large result sets
2. **Filtering:** Add filtering capabilities to queries
3. **Caching:** Implement client-side caching strategies
4. **Analytics:** Add analytics functions for payroll insights
5. **Bulk Operations:** Add more bulk operation types

## State Archival / TTL Strategy

Soroban archives persistent entries once their time-to-live (TTL) lapses. Long-lived
but infrequently-accessed payroll data (agreements, escrow balances, employee salary
and claimed-period records) could otherwise be archived mid-lifecycle, breaking later
claims and refunds.

To prevent this, TTL constants are centralized in `storage.rs` and the contract bumps
TTL on access of these long-lived keys:

- `PERSISTENT_TTL_THRESHOLD` (~30 days of ledgers): when a key's remaining TTL drops
  below this, it is extended.
- `PERSISTENT_BUMP_AMOUNT` (~90 days of ledgers): the target TTL a bump extends to.
- `extend_persistent_ttl(env, key)`: a no-op when the key is absent; otherwise calls
  `extend_ttl(PERSISTENT_TTL_THRESHOLD, PERSISTENT_BUMP_AMOUNT)`.

Bumps are applied in the centralized accessors and `get_agreement`:

- `get_agreement` / `get_agreement_escrow_balance` (read paths)
- `set_agreement_escrow_balance`, `set_employee_salary`,
  `set_employee_claimed_periods` (write paths)

**Anti-griefing.** TTL bumps only ever extend keys the contract already owns and writes,
and the caller pays the rent for the extension, so they cannot be abused to keep
adversarial entries alive cheaply or to inflate another party's storage rent.

Coverage: `tests/test_state_machine.rs` (`test_get_agreement_bumps_ttl`,
`test_escrow_balance_ttl_survives_ledger_advance`) configures a finite TTL window,
advances the ledger near expiry, and asserts the entries remain live and are re-bumped.

## Conclusion

These storage optimizations provide significant improvements in:
- **Gas Efficiency:** Reduced storage operations and optimized data types
- **Scalability:** Better handling of large numbers of employees
- **Query Performance:** Fast lookups through indexing
- **Developer Experience:** Simplified batch operations and queries

The changes maintain backward compatibility while providing a solid foundation for future growth and feature additions. 