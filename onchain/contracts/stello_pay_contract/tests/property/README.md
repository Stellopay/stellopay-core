# Property-Based Tests: Conservation of Funds

## Overview

This directory contains property-based tests using `proptest` that assert critical financial invariants for the stello_pay_contract. These tests are the **primary automated safety net** against fund leakage, double-claims, and over-distribution bugs.

## Core Invariants

### 1. Conservation of Funds

**Invariant**: For every `(agreement, token)` pair:

```
sum(escrow deposits) == sum(payouts) + remaining escrow balance
```

This fundamental accounting equation must hold at **all times** and across **all operations**.

**Why it matters**: This catches:
- Fund leakage (tokens disappearing from the system)
- Double-claims (same funds claimed multiple times)
- Over-distribution (paying out more than deposited)
- Integer division dust bugs in multi-employee payouts

### 2. Monotonicity of Claimed Periods

**Invariant**: For each employee in a payroll agreement:

```
claimed_periods(t+1) >= claimed_periods(t)  // Non-decreasing
claimed_periods <= num_periods              // Upper bound
```

**Why it matters**: Prevents:
- Rollback attacks where periods are "unclaimed"
- Overflow bugs where claimed_periods wraps around
- Claims beyond the agreement's defined period limit

### 3. Dispute Resolution Bounds

**Invariant**: During dispute resolution:

```
employee_payout + employer_refund <= escrow_balance
sum(individual_employee_payouts) + dust <= employee_payout
dust < employee_count  // Integer division remainder
```

**Why it matters**: Ensures:
- Dispute resolution never over-distributes funds
- Multi-employee splits handle integer division correctly
- Dust from division is minimal and bounded

### 4. Grace Period Claim Conservation

**Invariant**: For cancelled agreements:

```
claims_during_grace + refund_after_grace == total_escrow
paid_amount(t+1) >= paid_amount(t)  // Never decreases
```

**Why it matters**: Protects against:
- Double-refund bugs (employer gets refund + claims still process)
- Retroactive claim invalidation
- Fund loss during grace period transitions

## Test Configuration

### Case Counts

The number of test cases is configurable via the `PROPTEST_CASES` environment variable:

```bash
# Default (CI-friendly): 32 cases per test
cargo test -p stello_pay_contract

# Extended local testing: 256 cases
PROPTEST_CASES=256 cargo test -p stello_pay_contract

# Deep exhaustive testing: 1000+ cases
PROPTEST_CASES=1000 cargo test -p stello_pay_contract
```

**Recommendation**: 
- CI: 32 cases (fast feedback)
- Pre-commit: 128 cases (reasonable coverage)
- Release validation: 1000+ cases (comprehensive)

## Test Structure

### Strategies

Property tests use the following strategies to generate randomized inputs:

1. **`payroll_agreement_strategy()`**: Generates payroll configurations
   - Employee count: 1-5
   - Salaries: 100-5000 per employee
   - Period duration: 1 hour to 1 day
   - Grace period: 0 to 1 week

2. **`escrow_agreement_strategy()`**: Generates escrow configurations
   - Amount per period: 100-10000
   - Period duration: 1 hour to 1 day
   - Number of periods: 1-10

3. **`operation_sequence_strategy()`**: Generates operation sequences
   - Advance time (1-3 periods)
   - Individual claims
   - Batch claims
   - Raise dispute
   - Resolve dispute (with split ratios)

### Test Coverage

| Test | Invariants Checked | Bug Classes Caught |
|------|-------------------|-------------------|
| `prop_payroll_conservation_of_funds` | Conservation, escrow bounds | Fund leakage, over-distribution |
| `prop_escrow_conservation_of_funds` | Conservation, time-based claims | Double-claims, claim overflow |
| `prop_claimed_periods_monotonic_and_bounded` | Monotonicity, bounds | Period rollback, overflow |
| `prop_resolve_dispute_bounds_respected` | Dispute bounds, conservation | Over-distribution in disputes |
| `prop_multi_employee_dispute_no_dust_leakage` | Integer division, dust bounds | Multi-employee split bugs |
| `prop_grace_period_claim_conservation` | Grace period conservation | Double-refund, grace violations |

## Integration with Existing Bugs

These property tests are designed to catch **existing accounting bugs** in the contract:

### Bug 1: `resolve_dispute_core` Integer Division Dust

**Location**: Multi-employee dispute resolution  
**Issue**: When `employee_payout` is split among N employees using integer division, the remainder (dust) may be lost or cause over-distribution.

**Property test catching this**: `prop_multi_employee_dispute_no_dust_leakage`

**How**: Generates agreements with 2-5 employees and non-divisible payout amounts, then asserts:
```rust
sum(individual_payouts) + dust == employee_payout
dust < employee_count
```

### Bug 2: `claim_payroll` Accounting Errors

**Location**: Individual and batch payroll claims  
**Issue**: Potential for paid_amount to not match actual transfers, or escrow to become negative.

**Property tests catching this**: 
- `prop_payroll_conservation_of_funds`
- `prop_claimed_periods_monotonic_and_bounded`

**How**: Executes random sequences of claims and verifies:
```rust
paid + remaining == deposited  // At every step
claimed_periods[i+1] >= claimed_periods[i]
```

### Bug 3: Grace Period Double-Claim

**Location**: Claims during grace period after cancellation  
**Issue**: Claims during grace might not be properly accounted in refunds.

**Property test catching this**: `prop_grace_period_claim_conservation`

**How**: Cancels agreement mid-lifecycle, allows grace period claims, then verifies:
```rust
grace_claims + finalized_refund == total_escrow
```

## Running Tests

### Full Suite

```bash
# Run all property tests
cargo test -p stello_pay_contract --test property

# With verbose output
cargo test -p stello_pay_contract --test property -- --nocapture

# Single test with detailed output
cargo test -p stello_pay_contract prop_payroll_conservation_of_funds -- --nocapture
```

### With Coverage

```bash
# Generate coverage report (requires tarpaulin)
cargo tarpaulin --packages stello_pay_contract --out Html
```

### Shrinking on Failure

When a property test fails, `proptest` automatically **shrinks** the failing input to produce a minimal counterexample:

```
proptest: test failed during shrinking; minimal failing case found:
  employee_count = 3
  salaries = [1000, 1500, 2000]
  operations = [AdvanceTime(1), ClaimPayroll(0), RaiseDispute, ResolveDispute(70)]
```

This minimal case can be used to:
1. Reproduce the bug deterministically
2. Debug with simplified inputs
3. Create a focused regression test

## Expected Test Behavior

### After Accounting Bug Fixes

These tests are **expected to fail** on the current contract due to known bugs. After the following fixes are applied, all tests should pass:

1. **Fix `resolve_dispute_core`**: Properly handle integer division dust in multi-employee splits
2. **Fix `claim_payroll`**: Ensure atomic update of `paid_amount` and `escrow_balance`
3. **Fix grace period logic**: Correctly account for claims during grace in refund calculation

### Test as Documentation

These property tests serve as:
- **Specification**: Formal definition of invariants that must hold
- **Safety net**: Catch regressions when modifying money paths
- **Examples**: Demonstrate correct usage patterns and edge cases

## Maintenance

When modifying the contract:

1. **Before changes**: Run property tests with high case count (500+) to establish baseline
2. **During development**: Run with default case count (32) for quick feedback
3. **Before PR**: Run with extended case count (256+) to catch edge cases
4. **After merge**: Add new property tests for new invariants

## Security Notes

> **Critical**: These tests are the **primary defense** against financial bugs. Any failure indicates a potential fund loss or security vulnerability.

### Invariant Violations

If a property test fails:
1. **DO NOT IGNORE**: Even with low probability, failures indicate real bugs
2. **REPRODUCE**: Use the shrunk input to create a deterministic unit test
3. **ROOT CAUSE**: Understand why the invariant was violated
4. **FIX**: Address the underlying bug, not just the test case
5. **VERIFY**: Confirm the fix with extended case counts

### Coverage Requirements

- **Minimum 95% test coverage** on money paths:
  - `claim_payroll`, `claim_payroll_in_token`, `batch_claim_payroll`
  - `resolve_dispute`, `resolve_dispute_core`
  - `claim_time_based`, `finalize_grace_period`

## References

- [Proptest Documentation](https://docs.rs/proptest/)
- [Conservation Laws in Financial Systems](https://en.wikipedia.org/wiki/Conservation_law)
- Main contract: `../src/lib.rs`
- Storage definitions: `../src/storage.rs`
