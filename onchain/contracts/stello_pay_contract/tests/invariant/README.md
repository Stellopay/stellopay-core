# Invariant Tests: State Consistency and Conservation

## Overview

This directory contains deterministic invariant tests that assert critical state consistency and conservation properties across agreement lifecycles. These tests complement the property-based tests by providing concrete, reproducible scenarios.

## Core Invariants Tested

### 1. Agreement State Invariants

**Invariant**: Agreement fields maintain consistency:
```rust
paid_amount >= 0
paid_amount <= total_amount
escrow_balance >= 0
escrow_balance + paid_amount == total_amount  // For funded agreements

// Escrow mode
amount_per_period > 0
num_periods > 0
claimed_periods <= num_periods
total_amount == amount_per_period * num_periods

// Payroll mode
total_amount == sum(all_employee_salaries)
```

**Tests**:
- `assert_agreement_core_invariants()` - Core validation helper
- `test_invariants_escrow_create_claim_flow()` - Escrow lifecycle
- `test_invariants_payroll_create_claim_flow()` - Payroll lifecycle

### 2. Conservation of Funds Across Operations

**Invariant**: Total funds are conserved through all operations:
```rust
// At every state transition:
deposited == paid_out + remaining_escrow

// Specifically:
initial_deposit == sum(all_claims) + sum(all_refunds) + current_escrow
```

**Tests**:
- `test_conservation_payroll_multi_claim_sequence()` - Sequential claims
- `test_conservation_batch_claim_payroll()` - Batch operations
- `test_invariant_escrow_conservation_across_lifecycle()` - Full lifecycle

### 3. Multi-Employee Dispute Resolution

**Invariant**: Integer division in dispute splits preserves funds:
```rust
// When employee_payout is split among N employees:
sum(individual_payouts) + dust == employee_payout
dust < employee_count
total_distributed == employee_payout + employer_refund
```

**Tests**:
- `test_conservation_multi_employee_dispute_integer_division()` - Division correctness
- `test_invariant_dispute_resolution_bounds()` - Bounds checking

**Why this matters**: The trickiest accounting bug is in multi-employee dispute resolution where integer division creates dust. For example:
- 3 employees, 1000 token payout
- 1000 / 3 = 333 each, with 1 token remainder
- Bug: Remainder might be lost or double-distributed

### 4. Claimed Periods Monotonicity

**Invariant**: Once claimed, periods can't be "unclaimed":
```rust
claimed_periods(t+1) >= claimed_periods(t)  // Never decreases
claimed_periods <= max_allowed_periods      // Never exceeds limit
```

**Tests**:
- `test_invariant_claimed_periods_never_exceeds_available()` - Upper bound
- `test_invariant_claimed_periods_monotonic_bounded()` - Monotonicity

### 5. Grace Period Conservation

**Invariant**: Cancellation + grace period + finalization preserves total:
```rust
// During grace period:
claims_allowed == true  // Contributors can still claim

// After finalization:
total_claimed_in_grace + employer_refund == remaining_escrow_at_cancel

// Monotonicity:
paid_amount never decreases
```

**Tests**:
- `test_invariants_escrow_refund_flow()` - Refund correctness
- `test_invariant_cancelled_agreement_grace_period_conservation()` - Grace conservation

### 6. Dispute Status Consistency

**Invariant**: Dispute flags remain consistent:
```rust
dispute_status == None => dispute_raised_at == None
dispute_status == Raised => {
    dispute_raised_at.is_some()
    agreement.status == Disputed
}
dispute_status == Resolved => dispute_raised_at.is_some()
```

**Tests**:
- `test_invariants_dispute_raise_and_resolve()` - Dispute lifecycle
- Embedded in `assert_agreement_core_invariants()`

### 7. Pause/Resume State Preservation

**Invariant**: Pausing doesn't corrupt state:
```rust
// During pause:
all_accounting_invariants_hold == true
no_claims_allowed == true

// After resume:
state == state_before_pause  // Except pause flag
conservation_still_holds == true
```

**Tests**:
- `test_invariants_pause_and_resume_flow()` - Pause correctness

## Test Helpers

### `assert_agreement_core_invariants()`

Core validation function called after every state transition:

```rust
fn assert_agreement_core_invariants(env: &Env, contract_id: &Address, agreement_id: u128)
```

**Checks**:
1. Non-negativity: `paid_amount >= 0`, `total_amount >= 0`, `escrow_balance >= 0`
2. Bounds: `paid_amount <= total_amount`
3. Mode-specific validation (escrow vs payroll vs milestone)
4. Dispute status consistency
5. Conservation equation

**Usage**: Call after **every operation** that modifies agreement state.

### Test Setup Helpers

```rust
create_test_env() -> Env              // Fresh environment
create_address(env) -> Address         // Random address
create_token(env) -> Address           // Deploy token
setup_contract(env) -> (Address, Client)  // Deploy contract
mint(env, token, to, amount)          // Mint tokens
```

## Integration with Known Bugs

### Bug: Multi-Employee Dispute Integer Division

**Location**: `resolve_dispute_core()` when splitting employee payout

**Scenario**:
```rust
// 3 employees, 1000 token employee_payout
// Expected: 333, 333, 334 (or similar split)
// Bug: Might lose the 1 token dust or over-distribute
```

**Test catching it**: `test_conservation_multi_employee_dispute_integer_division()`

**Expected behavior**:
- Before fix: Test **fails** with conservation violation
- After fix: Test **passes** with all funds accounted

### Bug: Claim During Grace Period Not Reflected in Refund

**Location**: `finalize_grace_period()` refund calculation

**Scenario**:
```rust
// Agreement cancelled with 5000 escrow
// Contributor claims 1000 during grace period
// Bug: Employer refund still 5000 instead of 4000
```

**Test catching it**: `test_invariant_cancelled_agreement_grace_period_conservation()`

**Expected behavior**:
- Before fix: Conservation fails (6000 total distributed > 5000 deposited)
- After fix: Conservation holds (4000 refund + 1000 claimed = 5000 deposited)

## Running Tests

### All Invariant Tests

```bash
# Run all invariant tests
cargo test -p stello_pay_contract --test invariant

# Specific test file
cargo test -p stello_pay_contract --test test_invariants

# Single test
cargo test -p stello_pay_contract test_conservation_multi_employee_dispute_integer_division
```

### With Verbose Output

```bash
cargo test -p stello_pay_contract --test invariant -- --nocapture
```

### With Coverage

```bash
cargo tarpaulin --packages stello_pay_contract --test invariant --out Html
```

## Test Organization

```
tests/invariant/
├── test_invariants.rs       # Core invariant tests
└── README.md               # This file

tests/
└── invariant_tests.rs      # Additional invariant tests (legacy structure)
```

## Expected Test Status

### Current Status (Before Fixes)

Several tests are **expected to fail** due to known accounting bugs:

- ❌ `test_conservation_multi_employee_dispute_integer_division` - Dust bug
- ❌ `test_invariant_cancelled_agreement_grace_period_conservation` - Grace refund bug
- ✅ Other tests should pass

### After Bug Fixes

All tests should pass with:
1. **Dispute resolution fix**: Handle integer division dust correctly
2. **Grace period fix**: Account for grace claims in refund calculation
3. **Claim payroll fix**: Atomic escrow updates

## Writing New Invariant Tests

When adding new agreement features:

### 1. Identify Core Invariants

What must **always** be true?
- Conservation of funds
- State consistency
- Bounds on counters
- Monotonicity of claims

### 2. Create Lifecycle Test

Test the feature through a complete lifecycle:
```rust
#[test]
fn test_invariant_new_feature() {
    // 1. Setup
    let (env, contract, client) = setup();
    
    // 2. Create agreement
    let agreement_id = client.create_...();
    assert_agreement_core_invariants(...);  // ← After creation
    
    // 3. Execute operations
    client.some_operation(...);
    assert_agreement_core_invariants(...);  // ← After each operation
    
    // 4. Assert feature-specific invariants
    assert_eq!(expected, actual);
}
```

### 3. Test Edge Cases

Add focused tests for edge cases:
- Zero amounts
- Single employee vs multiple employees
- Minimum/maximum values
- State transitions

### 4. Document Invariants

Add doc comments explaining:
```rust
/// **Invariant: Feature X preserves conservation**
///
/// Tests that feature X maintains the fundamental equation:
///   `deposited == paid + remaining`
///
/// **Why it matters**: Catches bugs in [specific operations]
#[test]
fn test_invariant_feature_x_conservation() {
    // ...
}
```

## Coverage Requirements

Invariant tests must cover:

- ✅ **100%** of money movement paths (claim, refund, dispute resolution)
- ✅ **100%** of state transitions (create → activate → claim → complete/cancel)
- ✅ **All** agreement modes (escrow, payroll, milestone)
- ✅ **All** edge cases identified in security reviews

## Maintenance

### On Contract Modifications

1. **Before changes**: Run all invariant tests
2. **After changes**: Add new invariant tests for new functionality
3. **Verify**: All existing invariants still hold
4. **Update**: This README with new invariants

### On Test Failures

If an invariant test fails:

1. **Investigate immediately**: Invariant violations indicate bugs
2. **Don't modify test to pass**: Fix the underlying contract bug
3. **Add regression test**: Create focused unit test for the bug
4. **Document**: Update README with lessons learned

## Security Notes

> **Critical**: Invariant tests are part of the security boundary. Failures must be treated as potential vulnerabilities.

### When Tests Fail

- ❌ **Never** disable failing invariant tests
- ❌ **Never** weaken invariant assertions to make tests pass
- ✅ **Always** investigate root cause
- ✅ **Always** fix the contract, not the test

### Coverage Monitoring

Track coverage with:
```bash
cargo tarpaulin --packages stello_pay_contract --out Html --output-dir coverage/
```

Minimum requirements:
- **95%** line coverage on money paths
- **100%** coverage on invariant helper functions
- **100%** coverage on state transition functions

## References

- Property-based tests: `../property/`
- Main contract: `../../src/lib.rs`
- Storage module: `../../src/storage.rs`
- Security documentation: `../../docs/security.md`
