# Test Implementation Guide: Property-Based Conservation-of-Funds Tests

## Quick Start

Branch created: `test/fund-conservation-property-invariants`

### Run Tests

```bash
# Compile and run property tests (default 32 cases)
cargo test -p stello_pay_contract --test property

# Run with more cases for deeper coverage
PROPTEST_CASES=256 cargo test -p stello_pay_contract --test property

# Run invariant tests
cargo test -p stello_pay_contract --test invariant
cargo test -p stello_pay_contract --test invariant_tests

# Run all tests
cargo test -p stello_pay_contract
```

### Configuration

Set the number of test cases via environment variable:
```bash
export PROPTEST_CASES=100  # Default is 32
```

## What Was Implemented

### 1. Property-Based Tests (7 tests)

Located in: `tests/property/test_properties.rs`

| Test Name | Purpose | Invariant |
|-----------|---------|-----------|
| `prop_payroll_conservation_of_funds` | Multi-employee payroll lifecycle | `deposited == paid + remaining` |
| `prop_escrow_conservation_of_funds` | Time-based escrow claims | `escrow + paid == total` |
| `prop_claimed_periods_monotonic_and_bounded` | Period claim bounds | `claimed[t+1] >= claimed[t] <= max` |
| `prop_resolve_dispute_bounds_respected` | Dispute resolution limits | `payout + refund <= escrow` |
| `prop_multi_employee_dispute_no_dust_leakage` | Integer division correctness | `sum(payouts) + dust == total` |
| `prop_grace_period_claim_conservation` | Cancellation + grace | `grace_claims + refund == escrow` |
| `prop_convert_currency_matches_scaled_multiplication` | FX conversion | `result == amount * rate / scale` |

### 2. Invariant Tests (10+ tests)

Located in: `tests/invariant/test_invariants.rs` and `tests/invariant_tests.rs`

**Core Conservation Tests:**
- `test_conservation_payroll_multi_claim_sequence` - Sequential claims
- `test_conservation_multi_employee_dispute_integer_division` - Dust bug test
- `test_conservation_batch_claim_payroll` - Batch operations
- `test_invariant_escrow_conservation_across_lifecycle` - Full lifecycle
- `test_invariant_payroll_multi_employee_conservation` - Multi-employee

**Bounds Tests:**
- `test_invariant_claimed_periods_never_exceeds_available`
- `test_invariant_claimed_periods_monotonic_bounded`
- `test_invariant_dispute_resolution_bounds`

**Grace Period Tests:**
- `test_invariant_cancelled_agreement_grace_period_conservation`
- `test_invariants_escrow_refund_flow`

**Lifecycle Tests:**
- `test_invariants_escrow_create_claim_flow`
- `test_invariants_payroll_create_claim_flow`
- `test_invariants_dispute_raise_and_resolve`
- `test_invariants_pause_and_resume_flow`

### 3. Documentation

- **`tests/property/README.md`**: Complete guide to property-based tests
- **`tests/invariant/README.md`**: Invariant test documentation
- **`PROPERTY_INVARIANT_TESTS_SUMMARY.md`**: Implementation summary

## Key Features

### Randomized Test Generation

Property tests use `proptest` strategies to generate:

1. **Payroll Agreements**
   - 1-5 employees
   - Salaries: 100-5000 per employee
   - Period: 1 hour to 1 day
   - Grace: 0 to 1 week

2. **Escrow Agreements**
   - Amount per period: 100-10000
   - Period: 1 hour to 1 day
   - Periods: 1-10

3. **Operation Sequences**
   - Time advances (1-3 periods)
   - Individual claims
   - Batch claims
   - Disputes and resolutions

### Conservation Invariant

The fundamental equation tested everywhere:

```rust
// At all times and for all operations:
total_deposited == total_paid_out + remaining_escrow_balance

// Specifically:
initial_escrow == sum(all_claims) + sum(all_refunds) + current_escrow
```

### Shrinking on Failure

When a test fails, `proptest` automatically finds the minimal failing case:

```
proptest: minimal failing case:
  employee_count = 3
  salaries = [1000, 1500, 2000]  
  operations = [AdvanceTime(1), ClaimPayroll(0), ResolveDispute(70)]
```

This makes debugging much easier!

## Expected Test Behavior

### Before Bug Fixes

⚠️ **These tests are EXPECTED to fail** due to known bugs:

1. **Multi-employee dispute dust bug**
   - Tests: `prop_multi_employee_dispute_no_dust_leakage`, `test_conservation_multi_employee_dispute_integer_division`
   - Issue: Integer division in `resolve_dispute_core` loses or over-distributes dust
   - Example: 1000 tokens / 3 employees = 333 + 333 + 333 + 1 dust

2. **Grace period accounting bug**
   - Tests: `prop_grace_period_claim_conservation`, `test_invariant_cancelled_agreement_grace_period_conservation`
   - Issue: Claims during grace period not reflected in employer refund
   - Example: 5000 escrow → 1000 claimed in grace → employer gets 5000 refund (should be 4000)

3. **Claim payroll accounting errors**
   - Tests: `prop_payroll_conservation_of_funds`
   - Issue: Potential mismatch between `paid_amount` and actual transfers

### After Bug Fixes

✅ **All tests should pass** after applying fixes to:
1. `resolve_dispute_core` - Handle integer division dust correctly
2. `claim_payroll` - Ensure atomic escrow updates
3. `finalize_grace_period` - Account for grace period claims in refund

## Bug Detection Examples

### Example 1: Dust Leakage

```rust
// Setup: 3 employees, 1000 token employee_payout
// 1000 / 3 = 333 remainder 1

// Bug: Contract distributes 333 + 333 + 333 = 999
// Expected: 333 + 333 + 334 = 1000 (or similar distribution)

// Test catches this:
assert_eq!(
    total_distributed,
    employee_payout + employer_refund,
    "Conservation violated: {} != {}",
    total_distributed,
    escrow_balance
);
// FAILS: 999 != 1000
```

### Example 2: Grace Period Double-Pay

```rust
// Setup: 5000 escrow, cancelled after 2000 claimed
// During grace: contributor claims 1000 more

// Bug: Employer refund = 5000 - 2000 = 3000 (ignores grace claim)
// Total distributed: 2000 + 1000 + 3000 = 6000
// Expected: 2000 + 1000 + 2000 = 5000

// Test catches this:
assert_eq!(
    remaining + paid,
    total_deposited,
    "Conservation violated: {} + {} != {}",
    remaining, paid, total_deposited  
);
// FAILS: 6000 != 5000
```

### Example 3: Claimed Periods Overflow

```rust
// Setup: Agreement with 5 periods max
// Bug: Employee claims 6 periods

// Test catches this:
assert!(
    claimed_periods <= max_periods,
    "Claimed {} exceeds max {}",
    claimed_periods, max_periods
);
// FAILS: 6 > 5
```

## Test Output Examples

### Successful Test

```
test prop_payroll_conservation_of_funds ... ok
test prop_claimed_periods_monotonic_and_bounded ... ok
test test_conservation_payroll_multi_claim_sequence ... ok
```

### Failed Test with Shrinking

```
test prop_multi_employee_dispute_no_dust_leakage ... FAILED

proptest: test failed during shrinking
  minimal failing case:
    employee_count = 3
    base_salary = 100
    employee_payout_ratio = 70
  
  thread 'prop_multi_employee_dispute_no_dust_leakage' panicked at:
  assertion failed: remaining <= 2
    left: 100,
    right: 2
  Excessive remaining balance: 100 (employee_count: 3)
```

## CI Integration

### GitHub Actions Example

```yaml
- name: Run property tests
  run: |
    # Quick run for PR checks (32 cases)
    cargo test -p stello_pay_contract --test property
    
    # Extended run for main branch (256 cases)
    if [ "$GITHUB_REF" == "refs/heads/main" ]; then
      PROPTEST_CASES=256 cargo test -p stello_pay_contract --test property
    fi
```

### Coverage Requirements

```bash
# Generate coverage report
cargo tarpaulin --packages stello_pay_contract --out Html

# Requirements:
# - 95% coverage on money paths (claim, dispute, refund)
# - 100% coverage on invariant helpers
```

## Debugging Failed Tests

### Step 1: Identify the Minimal Case

Proptest provides a minimal failing input:
```
employee_count = 3
salaries = [1000, 1500, 2000]
operations = [AdvanceTime(1), ClaimPayroll(0)]
```

### Step 2: Create a Deterministic Unit Test

```rust
#[test]
fn test_bug_repro_multi_employee_claim() {
    let (env, contract_id, _owner, client) = setup_contract();
    // ... use the exact values from minimal case
}
```

### Step 3: Debug with Logging

```rust
env.as_contract(&contract_id, || {
    let remaining = DataKey::get_agreement_escrow_balance(...);
    let paid = DataKey::get_agreement_paid_amount(...);
    eprintln!("DEBUG: remaining={}, paid={}, expected={}", remaining, paid, total);
    assert_eq!(remaining + paid, total);
});
```

### Step 4: Fix the Contract

Fix the underlying bug in the contract, not the test!

## Maintenance

### Adding New Tests

When adding new money-path features:

1. **Add property test**:
   ```rust
   proptest! {
       #[test]
       fn prop_new_feature_conservation(...) {
           // Generate random inputs
           // Execute operations
           // Assert conservation
       }
   }
   ```

2. **Add invariant test**:
   ```rust
   #[test]
   fn test_invariant_new_feature() {
       // Concrete scenario
       // Assert invariants at each step
   }
   ```

3. **Update documentation**:
   - Add to README
   - Document the invariant
   - Explain why it matters

### Regression Tests

When a bug is found:

1. Add the minimal failing case as a deterministic test
2. Keep the property test to catch similar bugs
3. Document the bug and fix in the README

## Performance

### Test Duration

- **Property tests** (32 cases): ~30 seconds
- **Property tests** (256 cases): ~3-5 minutes
- **Invariant tests**: ~10 seconds
- **Full suite**: ~5-10 minutes

### Optimization Tips

1. Use default 32 cases for development
2. Use 256+ cases for pre-commit
3. Use 1000+ cases for release validation
4. Run invariant tests first (faster feedback)

## Security Checklist

Before deploying changes to money paths:

- [ ] All property tests pass with PROPTEST_CASES=256
- [ ] All invariant tests pass
- [ ] No conservation violations in any test
- [ ] New features have property tests
- [ ] Coverage report shows 95%+ on money paths
- [ ] Security review completed
- [ ] Audit documentation updated

## Support

### Common Issues

**Issue**: Tests timeout during compilation
**Solution**: Rust compilation of large projects takes time. Wait for initial compilation, subsequent runs are faster.

**Issue**: Proptest finds a failure
**Solution**: This is expected! Use the minimal case to debug. Don't ignore failures.

**Issue**: Want to run just one test
**Solution**: `cargo test -p stello_pay_contract test_name`

### Questions?

Refer to:
- `tests/property/README.md` - Property test details
- `tests/invariant/README.md` - Invariant test details  
- `PROPERTY_INVARIANT_TESTS_SUMMARY.md` - Implementation summary

## Conclusion

This test suite provides comprehensive coverage of conservation-of-funds invariants and serves as the primary automated safety net against financial bugs. The combination of property-based and deterministic invariant tests ensures both broad coverage and specific scenario validation.

**Key Takeaway**: Any test failure indicates a potential fund loss bug. Never ignore test failures, always investigate and fix the root cause.
