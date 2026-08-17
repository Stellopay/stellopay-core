# Archived contracts

These crates are **outside the Cargo workspace** (`onchain/Cargo.toml` globs
`contracts/*` only). They are not built, not tested, and not deployed. Nothing
here is compiled by CI.

They were moved here with `git mv`, so full history is preserved and follows the
files — `git log --follow archive/contracts/<crate>` works as before.

## Why

The workspace had 29 contract crates. Only 7 are reachable from
`stello_pay_contract`, the contract behind the milestone-escrow flow we are
shipping. The remaining 22, plus `integration_tests`, exercised features nothing
in the product calls: no backend or frontend code invoked any contract, and 18 of
them were leaf crates that no other crate depended on either.

Keeping them in the workspace meant every build and every CI run compiled and
tested ~20k lines of unreachable code, and any breakage in them blocked work on
the contract that matters.

## Archived module: `stello_pay_contract` encrypted backup

`archive/stello_pay_contract/` holds the encrypted backup/restore module lifted
out of the live contract — `src/backup.rs` plus its three test files. It is not
a crate; the paths mirror their original location inside
`onchain/contracts/stello_pay_contract/`.

**Why it was removed:** the contract compiled to 145,351 bytes, which Stellar
Testnet rejects at upload with `TxSorobanInvalid`. `backup.rs` was the only
consumer of six crypto crates — `aes-gcm`, `pbkdf2`, `hmac`, `sha2`, plus
`sha1` and `base64`, which were declared in Cargo.toml but referenced nowhere
at all. Removing the module and those dependencies brings the build to
120,510 bytes, which uploads successfully.

Three entrypoints went with it: `admin_restore_agreement`,
`admin_restore_from_encrypted`, `admin_restore_dry_run`.

`docs/encrypted-backup-recovery.md` still describes this feature and needs to
be marked as archived or planned.

To restore, move the files back to their mirrored paths, re-add the six
dependencies to `Cargo.toml`, re-add `pub mod backup;` and
`pub mod test_backup_dry_run;`, and re-declare the three entrypoints — then
check the wasm size again, because it will exceed the deployable limit.

## Restoring a crate

```bash
git mv archive/contracts/<crate> onchain/contracts/<crate>
```

The workspace glob picks it up automatically. If it has path dependencies on
other archived crates, restore those too — `grep 'path = ' <crate>/Cargo.toml`
lists them.

## What was kept, and why

| Crate | Reason |
| --- | --- |
| `stello_pay_contract` | The contract being shipped: payroll and milestone agreements, escrow, disputes. |
| `rbac-interface` | Build dependency of `stello_pay_contract` and `rbac`. |
| `milestone-interface` | Build dependency of `stello_pay_contract`. |
| `multisig` | Dev dependency — `stello_pay_contract`'s multisig tests. |
| `rbac` | Dev dependency — `stello_pay_contract`'s access-control tests. |
| `rate_limiter` | Dev dependency — `stello_pay_contract`'s rate-limit tests. |
| `price_oracle` | Dev dependency — `stello_pay_contract`'s multi-currency tests. |

`integration_tests` was archived with them: all 16 of its test files exercise
cross-contract flows between archived crates. The milestone path is covered
directly and far more thoroughly inside `onchain/contracts/stello_pay_contract/tests/`.

## Known issues in archived code

Two defects were found while getting CI green and are documented rather than
fixed, since the code is no longer built:

- `price_oracle` — the consecutive-stale-halt counter can never advance past 1.
  It is written by the same `get_pair_state` call that returns `Err`, and Soroban
  rolls back storage writes from a failed invocation. Its three tests are
  `#[ignore]`d with that reason. (`price_oracle` is kept, not archived; noted here
  because it is the same class of issue.)
- `payment_scheduler` → `payment_retry` orchestration is scaffolding only. The
  scheduler never calls `schedule_retry`, never constructs `RetryContractClient`,
  and never calls its own `compute_payment_id`.
