# Public entry-point convention

This document records the single convention the payroll module follows for
public entry points so that callers can tell **from the signature alone**
whether a failure is a recoverable contract error or a host trap. It closes the
ambiguity described in [issue #1317](https://github.com/Stellopay/stellopay-core/issues/1317).

## The rule

1. **Every fallible public entry point returns `Result<_, PayrollError>`.**
   Recoverable caller preconditions (unknown id, wrong lifecycle status,
   invalid argument, insufficient escrow, unauthorised caller, arithmetic
   overflow, …) are reported as a typed [`PayrollError`](../src/storage.rs)
   variant. They never abort the transaction with an opaque host trap, so a
   composing contract can branch on the error code or propagate it with `?`.

2. **Infallible entry points return a plain value and are documented as
   infallible.** Pure readers and existence probes return their value directly
   (`Option<_>`, `bool`, a count, …) and never fail.

3. **Host traps are reserved for failures the contract cannot recover from.**
   `Address::require_auth()` failures, Soroban token `transfer` failures and
   host-level resource errors still trap. Those are external/irrecoverable
   conditions, not caller-recoverable preconditions, and are called out in each
   entry point's `# Panics` section.

4. **Error discriminants are a stable, append-only ABI.** Converting a
   previously-trapping entry point reuses existing `PayrollError` variants; it
   never renumbers or repurposes one. See the convention note in
   [`src/storage.rs`](../src/storage.rs).

## Converted entry points

The following state transitions previously returned `()` and trapped with
`panic_with_error!`. They now return `Result<(), PayrollError>`:

| Entry point | Rejection paths and error variants |
| --- | --- |
| `fund_milestone_agreement` | `AgreementNotFound` (unknown agreement / missing status or token record), `Unauthorized` (`from` ≠ employer), `MilestoneAmountInvalid` (amount ≤ 0), `MilestoneAgreementInvalidStatus` (`Cancelled` / `Completed`), `InvalidData` (escrow overflow) |
| `activate_agreement` | `AgreementNotFound` (unknown agreement), `InvalidData` (not in `Created` status), `NoEmployee` (payroll mode with no employees) |
| `resume_agreement` | `AgreementNotFound` (unknown agreement), `InvalidData` (not in `Paused` status) |
| `cancel_agreement` | `AgreementNotFound` (unknown agreement), `InvalidData` (not in `Active` or `Created` status) |
| `finalize_grace_period` | `AgreementNotFound` (unknown agreement), `InvalidData` (not `Cancelled`, corrupt cancellation record, timestamp overflow, or grace period not yet expired) |

`finalize_grace_period` is idempotent: a second call after finalization returns
`Ok(())` without re-emitting the event or re-issuing the refund.

These join the entry points that already followed the convention, including
`add_milestone`, `raise_dispute`, `claim_time_based`, and `pause_agreement`.

## Infallible entry points

The following public entry points are **infallible**: they return a value and
never produce a `PayrollError` or trap on a caller-recoverable condition. They
do not return `Result`, and callers must not expect one.

| Entry point | Returns |
| --- | --- |
| `get_agreement` | `Option<Agreement>` |
| `get_agreement_employees` | `Vec<Address>` |
| `get_milestone_count` | `u32` |
| `get_milestone` | `Option<Milestone>` |
| `get_employee_claimed_periods` | `u32` |
| `get_claimed_periods` | `u32` |
| `get_arbiter` | `Option<Address>` |
| `get_multisig_contract` | `Option<Address>` |
| `get_dispute_status` | `DisputeStatus` |
| `is_grace_period_active` | `bool` |
| `get_grace_period_end` | `Option<u64>` |
| `get_grace_extension_policy` | `GracePeriodExtensionPolicy` |
| `get_grace_extension_seconds` | `u64` |
| `is_emergency_paused` | `bool` |
| `get_emergency_guardians` | `Option<Vec<Address>>` |
| `get_emergency_pause_state` | `Option<EmergencyPause>` |

## Value-returning constructors

Constructor entry points return the identifier (or batch result) they create
rather than `Result`: `create_payroll_agreement`, `create_escrow_agreement`,
`create_milestone_agreement`, `batch_create_payroll_agreements`, and
`batch_create_escrow_agreements`. They remain outside this convention because
their successful return value is not `()`; bad input is rejected by the guards
documented on each function.

## Verification

`tests/test_entry_point_conventions.rs` asserts the exact error variant for each
converted entry point's rejection paths, and that the previously-trapping call
sites now surface a typed code instead of an `InvokeError`.
