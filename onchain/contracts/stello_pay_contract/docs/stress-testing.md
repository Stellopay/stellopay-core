# Stress Testing Infrastructure (Issue #228)

## Scope

Implemented stress scenarios in:

- `onchain/contracts/stello_pay_contract/tests/stress/test_stress.rs`

Loaded via:

- `onchain/contracts/stello_pay_contract/tests/test_stress.rs`
- `onchain/contracts/stello_pay_contract/tests/stress/mod.rs`

## Implemented stress scenarios

1. **Maximum values**
- `stress_max_values_and_overflow_boundaries`
- Validates `u32::MAX` periods with safe arithmetic.
- Validates overflow rejection path for `i128::MAX * 2`.

2. **Rapid transactions**
- `stress_rapid_transactions_single_window`
- Executes 300 immediate claim attempts in one ledger window after initial successful claim.
- Measures and reports failure distribution (`NoPeriodsToClaim` / `AllPeriodsClaimed`).

3. **Network congestion**
- `stress_network_congestion_mixed_batch`
- Runs a 200-item mixed milestone batch:
  - valid approved
  - duplicates
  - unapproved
  - invalid IDs
- Measures success/failure counts and batch runtime.

4. **Failure-point detection**
- `stress_failure_point_detection`
- Detects first failed attempt index under repeated claims after full accrual.
- Verifies failure occurs on attempt 2 and captures error type.

## Command used

```powershell
cargo test -p stello_pay_contract --test test_stress -- --nocapture
```

## Latest execution result

Date: **February 26, 2026**

Execution completed successfully:

```text
[stress][failure-point] first_failure_attempt=2 error=AllPeriodsClaimed
[stress][max-values] max_safe_create_us=1814 overflow_rejected=true
[stress][rapid] attempts=300 duration_ms=1125 no_period_errors=300 all_periods_errors=0
[stress][congestion] batch_size=200 duration_ms=452 success=60 failed=140
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

## Notes

- Stress tests are fully implemented and instrumented with `println!` metrics.
- The earlier `link.exe` access error was transient; rerunning the same command completed successfully.
