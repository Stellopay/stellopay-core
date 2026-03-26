# Soroban cost benchmarks

This document describes how to measure host resource usage for critical Stellopay contract paths and how to use the results in CI or local regression checks.

## Environment

- Rust stable with `wasm32v1-none` (or `wasm32-unknown-unknown` per your Soroban toolchain).
- Soroban SDK version is pinned in `onchain/Cargo.toml` (`workspace.dependencies.soroban-sdk`).
- Benchmarks run in the Soroban **test host** (`Env::default()`), not on Futurenet/Mainnet. Absolute numbers are useful for **relative** comparisons on the same machine and SDK version.

## Running the benchmark binary

From the repository root:

```bash
cd onchain/contracts/stello_pay_contract
cargo bench --bench critical_paths
```

The bench prints **CPU instruction** totals after each isolated operation (`initialize`, `create_payroll_agreement`, `create_escrow_agreement`, `get_agreement`, `create_milestone_agreement`, `get_arbiter`). It uses `env.cost_estimate().budget().reset_default()` before each timed call.

## Repeatability

- Run on a quiet machine; close heavy CPU consumers.
- Pin the same Rust and `stellar` CLI versions as CI (see `.github/workflows/contracts.yml`).
- Store a baseline file in your team’s wiki or issue tracker; update it intentionally when the contract or SDK changes.

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
