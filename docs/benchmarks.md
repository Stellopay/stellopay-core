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

## Multi-Currency `claim_payroll_in_token` Benchmark

### `claim_payroll_in_token` (1 period, FX rate = 2.0)

The `critical_paths` bench now includes a case for `claim_payroll_in_token` with an active currency conversion. The setup follows the `test_multi_currency.rs` test pattern: the agreement is denominated in a `base_token`, an FX rate of 1 base = 2 payout is configured, escrow is funded in `payout_token`, one period is advanced, and the multi-currency claim is measured.

**Result (test host, Soroban SDK current):**

| Operation | CPU Instructions | Notes |
|-----------|------------------|-------|
| `claim_payroll_in_token` (multi-currency, 1 period) | 586,364 | Includes exchange-rate lookup, conversion math, escrow check, and payout-token transfer |

This benchmark can be re-run with:

```bash
cd onchain/contracts/stello_pay_contract
cargo bench --bench critical_paths
```

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

## Gas benchmark thresholds

In addition to the bench binary, `tests/gas_benchmarks.rs` provides regression-guarded instruction-count tests for high-traffic entrypoints. The committed baselines live in `benchmarks/stello_pay_contract_gas.json`, and the test fails when a measured value exceeds the recorded baseline by more than 5%.

Run the guard locally with:

```bash
cd onchain
cargo test -p stello_pay_contract gas_benchmark -- --nocapture
```

### Current thresholds

The current regression guard covers these entrypoints:

| Entrypoint | Scenario | Baseline CPU instructions | Max acceptable in CI (+5%) |
|------------|----------|---------------------------|----------------------------|
| `claim_payroll` | 1 elapsed period | 584,717 | 613,952 |
| `claim_payroll` | 10 elapsed periods | 584,717 | 613,952 |
| `claim_payroll` | 50 elapsed periods | 584,717 | 613,952 |
| `batch_claim_milestones` | 1 approved milestone | 408,917 | 429,362 |
| `batch_claim_milestones` | 5 approved milestones | 1,750,598 | 1,838,127 |
| `batch_claim_milestones` | 20 approved milestones | 8,965,821 | 9,414,112 |
| `batch_create_payroll_agreements` | 1 agreement | 223,146 | 234,303 |
| `batch_create_payroll_agreements` | 5 agreements | 1,117,375 | 1,173,243 |
| `batch_create_payroll_agreements` | 10 agreements | 2,450,605 | 2,573,135 |
| `batch_create_payroll_agreements` | 20 agreements | 5,808,763 | 6,099,201 |

At max batch size, the test also enforces these documented hard ceilings:

- `batch_claim_milestones(20)` must stay at or below `9,500,000` instructions
- `batch_create_payroll_agreements(20)` must stay at or below `7,000,000` instructions

### Updating thresholds intentionally

When a contract change is expected to increase cost:

1. Run `cargo test -p stello_pay_contract gas_benchmark -- --nocapture` and confirm the higher cost is intentional.
2. Refresh `benchmarks/stello_pay_contract_gas.json`:

```bash
cd onchain
UPDATE_GAS_BASELINES=1 cargo test -p stello_pay_contract gas_benchmark -- --nocapture
```

3. Update the threshold table above to match the new committed baselines and CI ceilings.
4. Mention the reason for the cost increase in the PR so reviewers know the threshold move was deliberate.

## CI

`.github/workflows/contracts.yml` runs the benchmark regression guard explicitly before the rest of the workspace tests:

```bash
cd onchain
cargo test -p stello_pay_contract gas_benchmark -- --nocapture
```

That step is the enforced comparison between current instruction counts and the recorded thresholds in `benchmarks/stello_pay_contract_gas.json`.

To compile the standalone bench target without executing it:

```bash
cd onchain/contracts/stello_pay_contract
cargo bench --bench critical_paths --no-run
```

## Related

- Stellar Soroban resource limits: [Soroban documentation](https://soroban.stellar.org/docs)
- Payment History contract: `onchain/contracts/payment_history/src/lib.rs`
