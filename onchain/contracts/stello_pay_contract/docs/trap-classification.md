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

## Regression guard

`.github/workflows/contracts.yml` runs [`scripts/check-contract-no-traps.sh`](../../../../scripts/check-contract-no-traps.sh). The guard scans all contract source outside test modules and fails on new `.unwrap()`, `.expect(`, `panic!`, or `assert!` family (`assert!`, `assert_eq!`, `assert_ne!`) traps. Test surfaces are exempt: everything under `src/tests/`, in-file `#[cfg(test)]` modules (e.g. the discriminant-stability test at the bottom of `storage.rs`), and `src/mock_contract.rs`, which is native-only test scaffolding imported by integration tests and is documented at both survivor sites.

## Untyped `assert!` conversion (#1308)

Issue #1308 extended the trap guard to the `assert!` family and removed the remaining untyped assertions from deployable source. Every converted site now raises an existing typed `PayrollError`; no variant was added, so the discriminant ABI is unchanged.

| # | Location | Original guard | Typed error |
| ---: | --- | --- | --- |
| 1 | `src/payroll.rs` `fund_milestone_agreement` | `from == employer` | `Unauthorized` |
| 2 | `src/payroll.rs` `fund_milestone_agreement` | `amount > 0` | `MilestoneAmountInvalid` |
| 3 | `src/payroll.rs` `fund_milestone_agreement` | status not `Cancelled` | `MilestoneAgreementInvalidStatus` |
| 4 | `src/payroll.rs` `fund_milestone_agreement` | status not `Completed` | `MilestoneAgreementInvalidStatus` |
| 5 | `src/payroll.rs` `add_milestone` | total invariant (`cfg(debug_assertions)`) | `InvalidData` |
| 6 | `src/payroll.rs` `add_employee_to_agreement` | status is `Created` | `InvalidData` |
| 7 | `src/payroll.rs` `add_employee_to_agreement` | mode is `Payroll` | `InvalidAgreementMode` |
| 8 | `src/payroll.rs` `add_employee_to_agreement` | `salary_per_period > 0` | `InvalidData` |
| 9 | `src/payroll.rs` `activate_agreement` | status is `Created` | `InvalidData` |
| 10 | `src/payroll.rs` `activate_agreement` | at least one employee | `NoEmployee` |
| 11 | `src/payroll.rs` `claim_payroll_inner` | claimed-periods invariant | `InvalidData` |
| 12 | `src/payroll.rs` `batch_claim_payroll_inner` | claimed-periods invariant | `InvalidData` |
| 13 | `src/payroll.rs` `claim_time_based` | claimed-periods invariant | `InvalidData` |
| 14 | `src/payroll.rs` `resume_agreement` | status is `Paused` | `InvalidData` |
| 15 | `src/payroll.rs` `cancel_agreement` | status `Active`/`Created` | `InvalidData` |
| 16 | `src/payroll.rs` `finalize_grace_period` | status is `Cancelled` | `InvalidData` |
| 17 | `src/payroll.rs` `finalize_grace_period` | grace period expired | `NotInGracePeriod` |
| 18 | `src/lib.rs` `require_upgrade_admin` | RBAC `Admin` role held | `Unauthorized` |
| 19 | `src/lib.rs` `migrate_state` | `from_version == current` | `InvalidData` |

[`scripts/tests/check_contract_no_traps_test.sh`](../../../../scripts/tests/check_contract_no_traps_test.sh) exercises the guard against fixtures covering `assert!`, `assert_eq!`/`assert_ne!`, in-file test modules, and `mock_contract.rs`.
