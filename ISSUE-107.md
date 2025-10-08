# MR: Implement Advanced Rate Limiting and DDoS Protection (#107)

## Title
Implement Advanced Rate Limiting and DDoS Protection

## Description
This MR introduces advanced rate limiting and basic DDoS protection primitives into the `stello_pay_contract`:

- Per-user rate limiting with configurable windows
- Operation-type aware limits (e.g., disburse, deposit, create escrow)
- Adaptive throttling using simple security audit signal
- Exponential backoff and temporary block for repeated violations
- Events emitted on violations for off-chain monitoring

## Files Modified
- `onchain/contracts/stello_pay_contract/src/payroll.rs`

## Key Changes
- Implemented `_check_rate_limit` with persistent state using existing storage keys to avoid DataKey bloat.
- Integrated rate limit checks into critical public functions (`disburse_salary` already had hooks; preserved behavior and ensured tests remain green).
- Emitted `RATE_LIMIT_EXCEEDED_EVENT` and security audit logging when violations occur.

## Verification
- Ran `cargo build` and `cargo test --all` successfully in the `onchain` workspace.
- All existing tests pass (ensuring no regressions).

## Notes
- The implementation uses conservative defaults so normal flows are unaffected, but provides safe hooks for future tightening.
- No new public API introduced; storage schema remains compatible.

