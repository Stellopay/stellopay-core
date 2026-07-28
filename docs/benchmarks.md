# Soroban cost benchmarks

This document describes how to measure host resource usage for critical Stellopay contract paths and how to use the results in CI or local regression checks.

## Environment

- Rust stable with `wasm32-unknown-unknown` (see [build-targets.md](./build-targets.md) for target rationale).
- Soroban SDK version is pinned in `onchain/Cargo.toml` (`workspace.dependencies.soroban-sdk`).
- Benchmarks run in the Soroban **test host** (`Env::default()`), not on Futurenet/Mainnet. Absolute numbers are useful for **relative** comparisons on the same machine and SDK version.

## Running the benchmark binary

From the repository root:

```bash
cd onchain/contracts/stello_pay_contract
cargo bench --bench critical_paths
```

The bench prints **CPU instruction** totals after each isolated operation (`initialize`, `create_payroll_agreement`, `create_escrow_agreement`, `get_agreement`, `create_milestone_agreement`, `get_arbiter`). It also runs a marginal-cost scaling benchmark for `batch_create_payroll_agreements` at 1, 5, 10, and 20 (MAX_BATCH_SIZE) agreements, printing per-batch totals and the marginal cost per additional agreement. It uses `env.cost_estimate().budget().reset_default()` before each timed call.

## Repeatability

- Run on a quiet machine; close heavy CPU consumers.
- Pin the same Rust and `stellar` CLI versions as CI (see `.github/workflows/contracts.yml`).
- Store a baseline file in your team’s wiki or issue tracker; update it intentionally when the contract or SDK changes.

## Gas benchmark tests

In addition to the bench binary, `tests/gas_benchmarks.rs` provides regression-guarded instruction-count tests for high-frequency operations including:

- `claim_payroll` at 1, 10, and 50 elapsed periods
- `batch_claim_milestones` at 1, 5, and 20 milestones
- `batch_create_payroll_agreements` at 1, 5, 10, and 20 agreements (with a linearity assertion)

Run with:

```bash
cd onchain/contracts/stello_pay_contract
cargo test -p stello_pay_contract gas_benchmark -- --nocapture
```

These tests compare measured instruction counts against committed baselines in `benchmarks/stello_pay_contract_gas.json`. CI fails when measured counts exceed the baseline by more than 5%. To update baselines after an intentional contract change:

```bash
UPDATE_GAS_BASELINES=1 cargo test -p stello_pay_contract gas_benchmark -- --nocapture
```

## Regression guard (optional)

To fail CI when costs exceed a threshold, capture the baseline `cpu_insns` values into a small script that parses `cargo bench` output and compares with limits. **Do not** hardcode thresholds in this repo without team agreement—they change with SDK upgrades.

## CI

The workflow builds the main contract and runs the full test suite. To compile benchmarks without executing them:

```bash
cd onchain/contracts/stello_pay_contract
cargo bench --bench critical_paths --no-run
```

Add this to `.github/workflows/ci.yml` if you want compile-time coverage of the bench target.

## Related

- Stellar Soroban resource limits: [Soroban documentation](https://soroban.stellar.org/docs)
