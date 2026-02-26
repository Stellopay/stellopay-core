# Load Testing Infrastructure

## Overview

This repository now includes dedicated load tests for on-chain contract behavior under high transaction volumes.

Location:
- `onchain/integration_tests/tests/load.rs`
- `onchain/integration_tests/tests/load/test_load.rs`

## Scenarios Covered

1. High agreement creation rate
- Creates 500 payroll agreements and measures throughput/latency.

2. Large employee volume in a single agreement
- Adds 1000 employees to one agreement and measures throughput/latency.

3. High claim transaction rate
- Executes 1000 payroll claim transactions across many agreements/employees.

4. Performance degradation profile
- Runs small/medium/large claim workloads and compares latency trends.

## Metrics

Each scenario prints:
- `duration_ms`
- `throughput_tps`
- `latency_us_per_tx`

These values can be collected from CI logs or local runs to track degradation over time.

## How To Run

```bash
cd onchain
cargo test -p integration_tests --test load -- --nocapture
```

## Performance Characteristics

Expected behavior under increasing load:
- Absolute execution time increases with transaction volume.
- Per-transaction latency may increase at larger scales.
- No catastrophic latency spike (guardrail assertion in degradation test).

## Security/Correctness Notes

- Tests use authenticated mocked environments (`mock_all_auths`) to focus on performance paths.
- Internal funding setup mirrors required claim storage state for realistic execution paths.
- Tests assert workload completion and consistency while measuring performance.
