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

The bench prints **CPU instruction** totals after each isolated operation (`initialize`, `create_payroll_agreement`, `create_escrow_agreement`, `get_agreement`, `create_milestone_agreement`, `get_arbiter`). It uses `env.cost_estimate().budget().reset_default()` before each timed call.

## Payment History Scaling Benchmarks

### `get_payments_by_employee` Scaling

The `payment_history` contract includes a benchmark for `get_payments_by_employee` to measure read cost scaling with large payment histories. This test records 10, 100, and 1000 payments for a single employee and measures the CPU instruction cost of reading the paginated results.

**Run the benchmark:**

```bash
cd onchain
cargo test -p payment_history benchmark_get_payments_by_employee_scaling -- --nocapture
```

**Results (test host, Soroban SDK current):**

| Payments | CPU Instructions | Notes |
|----------|------------------|-------|
| 10       | 448,537          | Baseline small history |
| 100      | 8,486,166        | At MAX_PAGE_SIZE cap |
| 1,000    | 9,173,832        | Capped at 100 records |

**Scaling Analysis:**

- The cost ratio from 100 to 1,000 payments is **1.08x**, demonstrating that the `MAX_PAGE_SIZE = 100` pagination cap effectively bounds read costs regardless of total history size.
- The pagination cap prevents runaway ledger-entry reads that could exhaust the resource budget on accounts with very large payment histories.
- The observed scaling confirms the pagination design: callers requesting a larger `limit` receive at most 100 records silently, with no error raised.

**Pagination Threshold Recommendation:**

The current `MAX_PAGE_SIZE = 100` is appropriate based on benchmark results:
- Provides sufficient records per page for efficient client-side rendering
- Keeps CPU instruction costs bounded (~9M instructions even with 1000+ total payments)
- Prevents resource exhaustion by adversarial callers
- Aligns with Soroban best practices for paginated queries

The contract's pagination implementation (`get_payments_by_employee`, `get_payments_by_agreement`, `get_payments_by_employer`) all use this cap, ensuring consistent behavior across all read paths.

### Security Notes

- The pagination cap is enforced at the contract level (`MAX_PAGE_SIZE = 100`), not just at the client level.
- Index counts can only increase, ensuring no entry can be silently replaced and no historical record can be pruned.
- Records are immutable with no update or delete path, preventing history tampering.

## Repeatability

- Run on a quiet machine; close heavy CPU consumers.
- Pin the same Rust and `stellar` CLI versions as CI (see `.github/workflows/contracts.yml`).
- Store a baseline file in your team's wiki or issue tracker; update it intentionally when the contract or SDK changes.

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
- Payment History contract: `onchain/contracts/payment_history/src/lib.rs`
