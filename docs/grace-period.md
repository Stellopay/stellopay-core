# Grace period and extensions

## Overview

When an employer **cancels** a payroll or escrow agreement, the contract enters `Cancelled` and starts a **grace period**. During this window:

- Employees/contributors may still **claim** owed amounts (same rules as before cancellation).
- Parties may **raise a dispute** while the cancellation grace window is open (see `raise_dispute` in the payroll contract).

The on-chain duration of that window is:

`effective_end = cancelled_at + agreement.grace_period_seconds + extension_seconds`

where `extension_seconds` is the cumulative total stored by the **grace period extension** mechanism (default `0`).

## Base grace vs extensions

- **`Agreement.grace_period_seconds`** — fixed at agreement creation (payroll parameter or derived for escrow). It is not mutated by extensions.
- **`GracePeriodExtensionSeconds(agreement_id)`** — persistent storage holding extra seconds added through `extend_grace_period`.

This split keeps the original agreement terms intact while allowing bounded, audited extensions.

## Contract API (Soroban name length limits)

| Function | Who may call | Purpose |
|----------|----------------|---------|
| `extend_grace_period(caller, agreement_id, additional_seconds)` | Contract **owner** or agreement **employer** | Add `additional_seconds` to the cumulative extension (must be `Cancelled`). |
| `set_grace_extension_policy(caller, policy)` | **Owner** only | Set caps (`GracePeriodExtensionPolicy`). |
| `get_grace_extension_policy()` | Anyone | Read current policy (defaults if unset). |
| `get_grace_extension_seconds(agreement_id)` | Anyone | Read cumulative extension for an agreement. |

Existing helpers `is_grace_period_active`, `get_grace_period_end`, `finalize_grace_period`, and the **cancelled** branch of `raise_dispute` all use the **effective** grace duration (base + extension).

## Policy caps (`GracePeriodExtensionPolicy`)

- **`max_cumulative_extension_bps`** — maximum allowed **extra** seconds, expressed as basis points of `agreement.grace_period_seconds` at extension time:  
  `max_extra = grace_period_seconds * max_cumulative_extension_bps / 10000`.  
  Example: base grace `86400`, bps `10000` → at most `86400` extra seconds (double the original window).
- **`max_extension_per_call_seconds`** — upper bound on `additional_seconds` for a single `extend_grace_period` call.

Defaults (if owner never sets policy):

- `max_cumulative_extension_bps = 10_000` (100% extra relative to base grace)
- `max_extension_per_call_seconds = 90 * 24 * 3600`

Owner updates are constrained by hard sanity limits inside `set_grace_extension_policy` (e.g. bps ≤ 500_000, per-call ≤ 730 days).

## Errors

| `PayrollError` | When |
|----------------|------|
| `GraceExtensionInvalid` | Zero `additional_seconds`, wrong status (not `Cancelled`), per-call cap exceeded, bad policy, overflow, etc. |
| `GraceExtensionCapExceeded` | New cumulative extension would exceed the policy-derived cap. |
| `Unauthorized` | Extender is neither owner nor employer; non-owner sets policy. |

## Events

`grace_period_extended_event` (`GracePeriodExtendedEvent`) records:

- `agreement_id`, `additional_seconds`, `total_extension_seconds`, `extended_by_owner` (bool).

Use it for indexers and employer/owner audits.

## Security assumptions

1. **Owner trust** — the owner can widen global caps within sanity bounds; treat owner keys as high-privilege.
2. **Employer** — can extend only for their own agreements; cannot extend others’ IDs.
3. **Emergency pause** — `extend_grace_period` returns `EmergencyPaused` when the contract is paused.
4. **Dispute window** — for **cancelled** agreements, the dispute deadline matches the extended cancellation grace. For non-cancelled agreements, extensions do **not** apply (legacy creation-based window only).

## Tests

See `onchain/contracts/stello_pay_contract/tests/test_grace_period_extension.rs` for employer/owner auth, caps, dispute integration, emergency pause, events, and finalization after an extended end.
