# Payroll trap classification

This table is the complete 34-site audit for [issue #1266](https://github.com/Stellopay/stellopay-core/issues/1266). It was performed against `onchain/contracts/stello_pay_contract/src` outside test modules. Recoverable preconditions now return an existing typed `PayrollError` (or use `panic_with_error!` where the public ABI still returns `()`); genuine test-fixture behavior is explicitly isolated below.

| # | Location | Original operation | Classification | Resolution |
| ---: | --- | --- | --- | --- |
| 1 | `src/lib.rs:89` | owner `.unwrap()` in upgrade-admin fallback | Recoverable precondition | Shared `contract_owner` accessor; missing owner becomes `Unauthorized`. |
| 2 | `src/lib.rs:109` | duplicate-initialization `panic!` | Recoverable precondition | Returns typed `InvalidData`; `try_initialize` asserts the exact error. |
| 3 | `src/lib.rs:123` | owner `.unwrap()` in RBAC configuration | Recoverable precondition | Shared `contract_owner` accessor; missing owner becomes `Unauthorized`. |
| 4 | `src/lib.rs:140` | owner `.unwrap()` in rate-limiter configuration | Recoverable precondition | Shared `contract_owner` accessor; missing owner becomes `Unauthorized`. |
| 5 | `src/lib.rs:164` | owner `.unwrap()` in salary-adjustment configuration | Recoverable precondition | Shared `contract_owner` accessor; missing owner becomes `Unauthorized`. |
| 6 | `src/lib.rs:189` | owner `.unwrap()` in milestone-hook configuration | Recoverable precondition | Shared `contract_owner` accessor; missing owner becomes `Unauthorized`. |
| 7 | `src/lib.rs:274` | unsupported migration `panic!` | Recoverable precondition | Returns typed `InvalidData` through the existing migration error path. |
| 8 | `src/lib.rs:1492` | negative paid amount `panic!` | Recoverable precondition | Returns typed `InvalidData` without changing the admin setter ABI. |
| 9 | `src/lib.rs:1524` | negative escrow amount `panic!` | Recoverable precondition | Returns typed `InvalidData` without changing the admin setter ABI. |
| 10 | `src/lib.rs:1598` | zero period duration `panic!` | Recoverable precondition | Returns typed `InvalidData` without changing the admin setter ABI. |
| 11 | `src/payroll.rs:371` | missing milestone employer `.expect()` | Recoverable precondition | Typed `AgreementNotFound` trap. |
| 12 | `src/payroll.rs:386` | missing milestone status `.expect()` | Recoverable precondition | Typed `AgreementNotFound` trap. |
| 13 | `src/payroll.rs:403` | escrow balance checked-add `.expect()` | Recoverable precondition | Typed `InvalidData` on overflow. |
| 14 | `src/payroll.rs:413` | missing milestone token `.expect()` | Recoverable precondition | Typed `AgreementNotFound` trap. |
| 15 | `src/payroll.rs:540` | milestone total checked-add `.expect()` | Recoverable precondition | Typed `InvalidData` on overflow. |
| 16 | `src/payroll.rs:737` | bounded reason-byte `.unwrap()` | Recoverable precondition | Handles the optional byte without an opaque host trap. |
| 17 | `src/payroll.rs:1806` | missing agreement `.expect()` while adding employee | Recoverable precondition | Typed `AgreementNotFound` trap. |
| 18 | `src/payroll.rs:1902` | missing agreement `.expect()` while activating | Recoverable precondition | Typed `AgreementNotFound` trap. |
| 19 | `src/payroll.rs:2047` | owner `.unwrap()` in grace-policy configuration | Recoverable precondition | Shared `contract_owner` accessor with `Unauthorized`. |
| 20 | `src/payroll.rs:2098` | owner `.unwrap()` in grace extension | Recoverable precondition | Shared `contract_owner` accessor with `Unauthorized`. |
| 21 | `src/payroll.rs:2361` | missing arbiter `.expect()` | Recoverable precondition | Typed `NotArbiter` result. |
| 22 | `src/payroll.rs:3727` | missing agreement `panic!` in claimed-period read | Recoverable precondition | Typed `AgreementNotFound` trap. |
| 23 | `src/payroll.rs:3870` | missing agreement `.expect()` while pausing | Recoverable precondition | Typed `AgreementNotFound` result. |
| 24 | `src/payroll.rs:3925` | missing agreement `.expect()` while resuming | Recoverable precondition | Typed `AgreementNotFound` trap. |
| 25 | `src/payroll.rs:4111` | missing agreement `.expect()` while cancelling | Recoverable precondition | Typed `AgreementNotFound` trap. |
| 26 | `src/payroll.rs:4153` | missing agreement `.expect()` while finalizing grace | Recoverable precondition | Typed `AgreementNotFound` trap. |
| 27 | `src/payroll.rs:4164` | missing cancellation timestamp `.expect()` | Recoverable precondition | Typed `InvalidData`; a corrupt lifecycle record is reported, not trapped opaquely. |
| 28 | `src/payroll.rs:4174` | grace-end checked-add `.expect()` | Recoverable precondition | Typed `InvalidData` on timestamp overflow. |
| 29 | `src/payroll.rs:4463` | owner `.unwrap()` in emergency-guardian setup | Recoverable precondition | Shared `contract_owner` accessor with `Unauthorized`. |
| 30 | `src/payroll.rs:4609` | owner `.unwrap()` in emergency pause | Recoverable precondition | Shared `contract_owner` accessor with `Unauthorized`. |
| 31 | `src/payroll.rs:4634` | owner `.unwrap()` in emergency unpause | Recoverable precondition | Shared `contract_owner` accessor with `Unauthorized`. |
| 32 | `src/audit.rs:83` | audit-logger owner `.expect()` | Recoverable precondition | Shared `contract_owner` accessor with typed `Unauthorized`. |
| 33 | `src/mock_contract.rs:63` | mock admin `.expect()` | Test-only scaffolding | Retained because integration tests import this native-only mock; adjacent comment documents the initialization invariant. Excluded by the production guard. |
| 34 | `src/mock_contract.rs:66` | mock unauthorized-upgrade `panic!` | Test-only scaffolding | Retained to model the mock's legacy external failure; adjacent comment documents the invariant. Excluded by the production guard. |

## ABI compatibility

No `PayrollError` variant was added, removed, renumbered, or reordered. All conversions reuse existing discriminants, especially `Unauthorized`, `AgreementNotFound`, `NotArbiter`, and `InvalidData`. The existing discriminant stability test remains unchanged and continues to lock the public error-code ABI.

## Follow-up: entry points converted from traps to `Result` (#1317)

The five lifecycle entry points that still returned `()` and trapped with
`panic_with_error!` after this audit — `fund_milestone_agreement`,
`activate_agreement`, `resume_agreement`, `cancel_agreement`, and
`finalize_grace_period` — were subsequently converted to return
`Result<(), PayrollError>` so callers can branch on the typed error instead of
catching a host trap. The mapping is documented in
[`entry-point-conventions.md`](./entry-point-conventions.md) and pinned by
`tests/test_entry_point_conventions.rs`.

## Regression guard

`.github/workflows/contracts.yml` runs [`scripts/check-contract-no-traps.sh`](../../../../scripts/check-contract-no-traps.sh). The guard scans all contract source outside test modules and fails on new `.unwrap()`, `.expect()`, or `panic!`. The only exclusion is `src/mock_contract.rs`, which is native-only test scaffolding imported by integration tests and is documented at both survivor sites.
