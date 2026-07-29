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

## Baseline Performance (captured 2026-07-29)

The following baselines were recorded from a clean CI run and serve as the
performance regression reference. A "clean run" means:
- A freshly checked-out `main` branch
- `cargo test -p integration_tests --test load -- --nocapture` executed in the
  `onchain/` directory
- No other processes competing for CPU or I/O

### Baseline figures

| Scenario                               | Throughput (TPS) | Latency (μs/tx) |
|----------------------------------------|------------------|-----------------|
| High agreement creation (500 agreements) | ≥ 500            | ≤ 2,000         |
| Large employee set (1,000 employees)     | ≥ 200            | ≤ 5,000         |
| High claim rate (120 claims)             | ≥ 1,000          | ≤ 1,000         |
| Degradation profile — small (15 claims)   | ≥ 500            | ≤ 2,000         |
| Degradation profile — medium (48 claims)  | ≥ 200            | ≤ 5,000         |
| Degradation profile — large (100 claims)  | ≥ 100            | ≤ 10,000        |

> **Note:** Throughput and latency vary by CI runner hardware. A run is
> considered healthy when all scenarios meet or exceed the throughput floor
> **and** stay at or below the latency ceiling. Any run that falls below the
> throughput floor by more than 30% **or** exceeds the latency ceiling by more
> than 50% will produce a visible warning in the test output.

### How to re-measure

```bash
cd onchain
cargo test -p integration_tests --test load -- --nocapture
```

Look for lines prefixed with `[load]` in the stdout:

```text
[load] agreements=500 duration_ms=… throughput_tps=… latency_us_per_tx=…
[load] employees=1000 duration_ms=… throughput_tps=… latency_us_per_tx=…
[load] claim_tx=120 duration_ms=… throughput_tps=… latency_us_per_tx=…
[load] profile small_tx=15 small_us_per_tx=… medium_tx=48 medium_us_per_tx=… large_tx=100 large_us_per_tx=…
```

If the test prints any `[load-warn]` lines, the run is significantly below
baseline and warrants investigation (see **Regression detection** below).

## Regression detection

Each load test now compares its measured throughput and latency against the
documented baseline. A **warning** (not a hard failure) is printed when:

- Throughput drops below 70% of the baseline throughput floor, **or**
- Per-transaction latency exceeds 150% of the baseline latency ceiling.

The test itself still passes — the warning is a soft signal for the team to
investigate before merging.

Example warning output:
```text
[load-warn] throughput_tps=320 is below 70% of baseline floor (350); possible regression
[load-warn] latency_us_per_tx=8200 exceeds 150% of baseline ceiling (5000); possible regression
```

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
