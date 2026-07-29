# Fix Report — `cancel_payment` must permanently stop a queued retry

**Branch:** `feature/payment-retry-cancel-mid-backoff-test`
**Commit:** `01f12c0` — *fix: ensure payment_retry cancel_payment blocks execution past the original backoff time*
**Confidence:** ~97 % (mutation-verified: the new tests fail 8/8 against a deliberately broken variant and pass 19/19 against the fix)

---

## STEP 1–2 — What the developer is building

`stellopay-core` is a Soroban/Stellar payroll + escrow protocol. `onchain/contracts/payment_retry`
is the retry-policy contract: when `payment_scheduler` cannot settle a payroll transfer (insufficient
escrow), it delegates to `payment_retry`, which records the failure, arms a backoff from a
per-request `retry_intervals` list, and exposes `process_due_payments` (permissionless keeper hook)
and `process_retry` (single-record trigger). Payers fund a per-request escrow ledger
(`Escrow(id)`) and may cancel, which refunds that deposit.

## STEP 3 — The defined issue, located precisely

The issue asks for a test proving a cancelled retry can never fire after the ledger crosses its
already-scheduled `next_retry_at`. Investigating the guard that this test must exercise revealed
two real weaknesses in `src/lib.rs`:

1. **No distinct terminal cancellation state.** `cancel_payment` set `state = RetryState::Failed`
   with the comment *"Treat cancellation as a terminal failure state"*. `get_payment` therefore
   could **not** "reflect a terminal cancelled state" as the issue requires — a payer opt-out was
   indistinguishable from retry exhaustion for every indexer and dashboard consuming the contract.

2. **Terminality was checked by open-coded, duplicated comparisons.** Three separate sites each
   wrote `state != Success && state != Failed`. Any state added later (exactly what a `Cancelled`
   variant is) would silently be treated as *non*-terminal by whichever site was missed — a latent
   footgun directly on the security-critical path. There was no single source of truth.

The ordering in `process_payment_if_due` was already correct (terminal check before the
`now < next_retry_at` check), which is why funds were not actually at risk on `main` — but that
ordering was **undocumented and unpinned by any test**, so it was one careless refactor away from
regressing into the exact race the issue describes.

## STEP 4 — The fix

### `onchain/contracts/payment_retry/src/lib.rs`

- **Added `RetryState::Cancelled`** — a terminal variant distinct from `Failed`, so `get_payment`
  now reports a genuine cancelled state ("payer opted out") separately from exhaustion ("protocol
  gave up"). Both equally block processing.
- **Added `RetryState::is_terminal()`** — one central predicate, now the single source of truth.
  All three previously-duplicated guards (`process_payment_if_due`, `fund_payment`,
  `cancel_payment`) route through it, so a future terminal variant cannot be omitted from a check.
- **`cancel_payment` now writes `Cancelled`** and additionally calls `mark_processed(id)`, adding a
  third independent barrier.
- **Pinned and documented the guard ordering** in `process_payment_if_due` with a
  "cancel-beats-backoff" invariant comment: terminality is resolved *before* the due-time
  comparison and *before* any token client is constructed.
- **NatSpec-style docs** added/expanded on the enum, `is_terminal`, `cancel_payment` (new
  "Permanence guarantee" section), `get_payment` (terminal state overrides `next_retry_at`), and
  the module header security model.

### `onchain/contracts/payment_scheduler/src/lib.rs`

The scheduler holds a local mirror of `RetryState` for cross-contract XDR decoding. Added the
matching `Cancelled` variant so the discriminants stay aligned — without this the mirror would
mis-decode a cancelled record. (One-line variant + doc comment.)

### Defence in depth — four barriers now stop a cancelled retry

| # | Barrier | Blocks |
|---|---|---|
| 1 | id removed from `PendingPayments` | batch keeper never iterates it |
| 2 | `Processed(id)` flag set | first short-circuit, before the record is even read |
| 3 | `state = Cancelled` (terminal) | authoritative check on the direct `process_retry` path |
| 4 | `Escrow(id)` zeroed + refunded | nothing payable even if 1–3 were bypassed |

## STEP 5–6 — Validation (mutation testing)

To prove the new tests actually have teeth rather than passing vacuously, I introduced a deliberate
regression (`cancel_payment` leaving the record `Retrying` and still tracked) and re-ran:

```
BROKEN variant:  test result: FAILED. 11 passed; 8 failed
FIXED  variant:  test result: ok.     19 passed; 0 failed
```

All 8 new cancellation tests fail against the broken variant and pass against the fix. Confidence
is therefore evidence-based, not assumed.

## STEP 7–8 — Build integrity, no conflicts

```
cargo test  -p payment_retry                                  → 19 passed, 0 failed
cargo test  -p payment_scheduler                              → 31 passed, 0 failed, 2 ignored
cargo clippy -p payment_retry --all-targets                   → no errors, no new warnings
cargo fmt   --check -p payment_retry -p payment_scheduler     → clean
cargo build -p payment_retry --release --target wasm32v1-none → Finished (deployable wasm)
```

**Pre-existing breakage, explicitly not caused by this change.** `cargo check -p integration_tests
--tests` reports **295 errors on clean `HEAD`** and **295 errors after my change** — an identical
count, with **zero** of them referencing `payment_retry` or `payment_scheduler`. They originate in
`stello_pay_contract`, `multisig` and `rbac`. The repo's own HEAD commit is
*"ci: temporarily scope CI to fmt-only until compile errors resolved"*, confirming this is known,
unrelated, upstream breakage. My change is net-neutral on it.

## STEP 9 — Findings & fix features

- The behaviour the issue worries about was *latent-correct but untested and undocumented*; the
  real defects were the missing terminal `Cancelled` state (which made the issue's third
  requirement literally unsatisfiable) and the duplicated terminality checks.
- Fix is small and reviewable: one new enum variant, one predicate, one reordering comment, one
  extra `mark_processed` call. No storage migration, no API break for existing callers.
- Behavioural note for reviewers: `get_payment(...).state` for a cancelled record now returns
  `Cancelled` instead of `Failed`. One pre-existing assertion was updated accordingly. Off-chain
  consumers that pattern-match on `Failed` to mean "will not retry" should be updated to use
  "is terminal" semantics.
- `next_retry_at` is deliberately **not** cleared on cancellation — retained for audit; it is inert
  because `state` is authoritative. This is now documented in three places.

## STEP 10 — Test coverage

19/19 pass. 8 new tests, all in `onchain/contracts/payment_retry/tests/test_retry.rs`, driven by a
shared `CancelRaceFixture` harness to keep them terse:

| Test | Property pinned |
|---|---|
| `test_cancel_mid_backoff_blocks_retry_past_scheduled_time` | **Core issue test.** t=1000 schedule → attempt #1 fails, arms `next_retry_at=1300` → **escrow deliberately funded** so a leaked retry *would* visibly succeed → cancel at t=1100 → assert no fire at t=1300 (exact boundary) and t=5000 (late keeper); `retry_count` frozen, recipient balance 0 |
| `test_cancel_before_first_attempt_blocks_later_processing` | Cancel from `Scheduled`, advance to t=100 000 |
| `test_cancelled_payment_survives_repeated_late_keeper_calls` | 12 successive keeper wake-ups past the armed time |
| `test_cancelled_record_does_not_block_sibling_in_same_batch` | Cancelled leg inert while a live sibling settles; no cross-escrow leakage |
| `test_double_cancel_is_rejected` | Refund path cannot be replayed to drain escrow |
| `test_funding_a_cancelled_payment_is_rejected` | Terminal record cannot be resurrected by re-funding |
| `test_non_payer_cannot_cancel_payment` | Third-party denial-of-payment griefing |
| `test_get_payment_reports_terminal_cancelled_state` | Issue requirement 3: terminal `Cancelled`, `!= Failed`, still unprocessable at `u32::MAX` |

Every test asserts on **token balances**, not just state flags, so "does not transfer funds" is
verified directly.

## STEP 11 — Files changed

| File | Change |
|---|---|
| `onchain/contracts/payment_retry/src/lib.rs` | **Modified** — `Cancelled` variant, `is_terminal()`, guard consolidation, `mark_processed` on cancel, NatSpec docs (+130 / −25) |
| `onchain/contracts/payment_retry/tests/test_retry.rs` | **Modified** — 8 new tests + fixture; 1 existing assertion updated `Failed`→`Cancelled` (+377) |
| `onchain/contracts/payment_scheduler/src/lib.rs` | **Modified** — mirrored `Cancelled` variant for XDR alignment (+4) |
| `docs/payment-retry.md` | **Modified** — retry-state table, "Cancellation permanence" section with guard-order listing and barrier table, `payment_cancelled` event row, security + testing sections (+72) |
| `FINDINGS.md` | **Created** — this report |
