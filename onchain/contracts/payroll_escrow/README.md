# Payroll Escrow Contract

Secure per-agreement token vault managed by an authorized **Manager** contract (typically the core payroll contract). Employers fund agreements; the manager releases funds to recipients or refunds remaining balances to the employer.

## Escrow conservation invariant

For every `agreement_id`, cumulative accounting must satisfy:

```text
total_funded == total_released + total_refunded + remaining_balance
```

No valid sequence of `fund_agreement`, `release`, and `refund_remaining` calls may cause total outflow to exceed total deposits for that agreement.

## Testing

```bash
cd onchain
cargo test -p payroll_escrow --verbose
```

| Suite | Location | Focus |
|-------|----------|-------|
| Unit tests | `src/tests/test_escrow.rs` | Initialization, auth, events, edge cases |
| Fuzz / property tests | `tests/fuzz/test_fuzzing.rs` | Proptest sequences asserting conservation |
| Integration tests | `onchain/integration_tests/tests/test_workflows.rs` | Cross-contract fund → release → refund flows |

See also [payroll-escrow.md](../../docs/payroll-escrow.md) for role and security documentation.
