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

Execution failed in this environment due to host linker permission:

```text
error: could not exec the linker `link.exe`
  = note: Access is denied. (os error 5)
error: could not compile `thiserror` (build script) due to 1 previous error
```

## Notes

- Stress tests are fully implemented and instrumented with `println!` metrics.
- Once linker access is fixed, rerun the command above to generate live stress output.
