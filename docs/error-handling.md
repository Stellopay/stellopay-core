## Error Handling Guide

This guide summarizes the main error types and patterns used in the Stellopay
payroll contracts, with a focus on **error codes**, **caller responsibilities**,
and **recovery strategies**.

It is intentionally concise and is meant to complement the inline doc comments
and tests.

---

### Core Error Type: `PayrollError`

On-chain type: `PayrollError`
Source: [`onchain/contracts/stello_pay_contract/src/storage.rs`](../onchain/contracts/stello_pay_contract/src/storage.rs)

The enum is annotated with `#[contracterror]` and `#[repr(u32)]`, so each
variant has a **stable numeric discriminant** that is surfaced to clients via
the Soroban host.

---

### Complete Variant Table

> **Append-only convention** — new variants are always appended to the end of
> the enum; existing discriminants are never renumbered.  Integrators can
> therefore cache the mapping below indefinitely without risk of a discriminant
> shifting under them across upgrades.  See the source enum for the authoritative
> list:
> [`storage.rs` — `PayrollError`](../onchain/contracts/stello_pay_contract/src/storage.rs).

| # | Variant | Meaning |
|---|---------|---------|
| 1 | `DisputeAlreadyRaised` | A dispute is already active for this agreement; raise is a no-op. |
| 2 | `NotInGracePeriod` | The operation requires the agreement to be in its grace/cancellation window. |
| 3 | `NotParty` | Caller is neither the employer nor an employee of this agreement. |
| 4 | `NotArbiter` | Caller is not the configured arbiter for this agreement. |
| 5 | `InvalidPayout` | `pay_employee + refund_employer` exceeds total locked funds, or amounts are negative. |
| 6 | `ActiveDispute` | Operation is blocked while an unresolved dispute is in progress. |
| 7 | `AgreementNotFound` | The referenced agreement ID does not exist in storage. |
| 8 | `NoDispute` | Attempted to resolve or query a dispute that has not been raised. |
| 9 | `NoEmployee` | Employee index or address is not present in the agreement. |
| 10 | `NotActivated` | Agreement must be in `Active` status for this operation. |
| 11 | `Unauthorized` | Generic access-control violation — caller does not have permission. |
| 12 | `InvalidEmployeeIndex` | Supplied employee index is out of range for this agreement. |
| 13 | `InvalidData` | Malformed, inconsistent, or out-of-range input or stored data. |
| 14 | `TransferFailed` | Token transfer call returned an error. |
| 15 | `InsufficientEscrowBalance` | Agreement escrow does not hold enough to cover the requested payment. |
| 16 | `NoPeriodsToClaim` | Elapsed time has not yet produced any new claimable periods. |
| 17 | `AgreementNotActivated` | Agreement must be activated before this operation is allowed. |
| 18 | `InvalidAgreementMode` | Operation is incompatible with the agreement's mode (Payroll vs Escrow). |
| 19 | `AgreementPaused` | Operation is not allowed while the agreement is in `Paused` status. |
| 20 | `AllPeriodsClaimed` | All configured time-based periods have already been claimed. |
| 21 | `ZeroAmountPerPeriod` | Configuration error: `amount_per_period` must be strictly positive. |
| 22 | `ZeroPeriodDuration` | Configuration error: `period_seconds` must be strictly positive. |
| 23 | `ZeroNumPeriods` | Configuration error: `num_periods` must be strictly positive. |
| 24 | `EmergencyPaused` | The contract-level emergency pause is active; all claims are blocked. |
| 25 | `NotGuardian` | Caller is not one of the configured emergency guardians. |
| 26 | `TimelockActive` | A withdrawal timelock is still in effect; the operation cannot proceed yet. |
| 27 | `InvalidTimelock` | The supplied timelock parameters are invalid (e.g. zero duration). |
| 28 | `MultisigApprovalRequired` | The operation exceeds the large-payment or dispute threshold and requires multisig approval before execution. |
| 29 | `ExchangeRateNotFound` | No FX rate is configured for the requested currency pair, or the stored rate has expired (exceeded `max_age`). |
| 30 | `ExchangeRateOverflow` | `amount × rate` overflows `i128` during FX conversion. |
| 31 | `ExchangeRateInvalid` | FX rate is non-positive, the timestamp is inconsistent, or the converted amount rounds down to zero (dust guard). |
| 32 | `GraceExtensionInvalid` | Grace-period extension arguments are invalid: zero duration, overflow, wrong agreement status, or unauthorized caller. |
| 33 | `GraceExtensionCapExceeded` | The requested extension would push the cumulative extension beyond the owner-configured cap (basis-point ceiling). |
| 34 | `RateLimited` | Rate limiter rejected the call — the caller has exceeded the allowed request rate. |
| 35 | `BatchTooLarge` | The batch contains more items than `MAX_BATCH_SIZE` allows. |
| 36 | `MilestoneAmountInvalid` | Milestone amount must be strictly positive. |
| 37 | `MilestoneAgreementInvalidStatus` | The milestone agreement is not in a valid status for the requested operation. |
| 38 | `MilestoneNotFound` | The referenced milestone (or its agreement) was not found in storage. |
| 39 | `MilestoneAlreadyApproved` | Attempted to approve a milestone that is already in `Approved` status. |
| 40 | `MilestoneNotApproved` | Attempted to claim a milestone that has not been approved yet. |
| 41 | `MilestoneAlreadyClaimed` | Attempted to claim a milestone that has already been paid out. |
| 42 | `EmployeeAlreadyExists` | An employee with the same address is already registered in the agreement; duplicate adds are rejected to preserve the 1:1 index mapping. |
| 43 | `ReentrancyDetected` | A reentrant call into a guarded claim path was detected. The in-progress claim re-entered the contract (e.g. via a hostile token during transfer). |
| 44 | `InvalidArbiter` | `set_arbiter` rejected the assignment: the caller attempted to self-appoint, or the supplied arbiter is identical to the currently-set one. |
| 45 | `MilestoneAlreadyRejected` | The milestone has already been rejected by the employer. |
| 46 | `MilestoneAlreadyApprovedCannotReject` | Cannot reject a milestone that has already been approved. |
| 47 | `MilestoneAlreadyClaimedCannotReject` | Cannot reject a milestone that has already been claimed. |

---

### Discriminant Stability Convention

Discriminants are assigned sequentially and **never renumbered**. When a new
error variant is added it is appended after the last existing variant, receiving
the next integer. This means:

- Off-chain clients can cache or hard-code the numeric → name mapping.
- SDKs and indexers do not need to refresh their error tables on upgrade unless
  they want to decode newly added variants.
- The authoritative source of truth is always the `PayrollError` enum in
  [`storage.rs`](../onchain/contracts/stello_pay_contract/src/storage.rs).

---

### How Error Codes Surface to Clients

`PayrollError` values appear in two places:

1. **Direct `Result` return** — public contract functions return
   `Result<_, PayrollError>`. The Soroban host converts the error to a
   `ContractError(discriminant)` host value which clients decode.

2. **Batch result structs** — for batch operations the per-item error code is
   embedded in result types:

   | Struct | Field | Value |
   |--------|-------|-------|
   | `PayrollClaimResult` | `error_code: u32` | 0 = success; otherwise the `PayrollError` discriminant |
   | `MilestoneClaimResult` | `error_code: u32` | 0 = success; otherwise the `PayrollError` discriminant |
   | `PayrollCreateResult` | `error_code: u32` | 0 = success; otherwise the `PayrollError` discriminant |
   | `EscrowCreateResult` | `error_code: u32` | 0 = success; otherwise the `PayrollError` discriminant |

---

### Error Handling Patterns

The contracts follow a small number of consistent patterns:

- **Typed errors for public functions** — where practical, functions return
  `Result<_, PayrollError>` and use the enum above.  Clients should branch on
  `error_code` (for batches) or the `Result` error in direct calls.

- **`assert!`-style guards for internal invariants** — many helper functions
  use `assert!(...)` with a descriptive message.  These represent **programmer
  errors or violated assumptions** and are not intended as recoverable API
  contracts.

- **Access control failures** — expressed as either `PayrollError::Unauthorized`
  or explicit `assert!(caller == expected, "...")`.  Recovery: call from the
  correct address (employer, employee, arbiter) or adjust integration logic.

- **Mode and status checks** — functions that depend on `AgreementMode` or
  `AgreementStatus` validate them first.  Typical responses:
  `AgreementNotActivated`, `AgreementPaused`, `InvalidAgreementMode`.
  Recovery: ensure the agreement is created/activated and not paused before
  calling; model the full lifecycle rather than calling arbitrarily.

---

### Recovery Strategies (By Category)

- **Authentication / Authorization**
  - Errors: `Unauthorized (11)`, `NotParty (3)`, `NotArbiter (4)`, `NotGuardian (25)`
  - Recovery: re-issue the transaction signed by the correct account.  For UIs,
    hide actions the current account cannot validly perform.

- **Configuration / Input Validation**
  - Errors: `ZeroAmountPerPeriod (21)`, `ZeroPeriodDuration (22)`,
    `ZeroNumPeriods (23)`, `InvalidEmployeeIndex (12)`, `MilestoneAmountInvalid (36)`,
    `InvalidArbiter (44)`
  - Recovery: validate inputs client-side before submitting.  For batch
    operations, inspect `error_code` per item and surface field-specific messages.

- **Lifecycle / Mode Mismatch**
  - Errors: `AgreementNotActivated (17)`, `AgreementPaused (19)`,
    `ActiveDispute (6)`, `NotInGracePeriod (2)`, `EmergencyPaused (24)`,
    `TimelockActive (26)`
  - Recovery: wait for the agreement to transition to the required state, or
    call the appropriate transition first (e.g. `activate_agreement`,
    `resume_agreement`, `finalize_grace_period`).

- **Funds and Transfers**
  - Errors: `InsufficientEscrowBalance (15)`, `TransferFailed (14)`,
    `InvalidPayout (5)`
  - Recovery: fund the escrow or token balances before retrying.  Verify payout
    splits do not exceed the total locked amount.

- **FX / Multi-currency**
  - Errors: `ExchangeRateNotFound (29)`, `ExchangeRateOverflow (30)`,
    `ExchangeRateInvalid (31)`
  - Recovery: ensure an exchange rate is configured for the currency pair, that
    the rate has not expired, and that the salary amount is large enough that
    the converted value is ≥ 1 quote-token unit (accumulate periods if needed).

- **Batch Operations**
  - Errors: `BatchTooLarge (35)`, per-item errors in `error_code`
  - Recovery: split large batches at `MAX_BATCH_SIZE`.  On partial failure,
    inspect each item's `error_code` independently.

---

### Example: Handling Batch Payroll Claims

When calling `batch_claim_payroll`, the contract returns a `BatchPayrollResult`:

- Check `total_claimed`, `successful_claims`, `failed_claims`.
- Iterate over `results: Vec<PayrollClaimResult>`:
  - If `error_code == 0` → success.
  - Otherwise, map the numeric code back to `PayrollError` using the table above
    for display or logging.

This pattern generalises to milestone batches (`BatchMilestoneResult`) and
creation batches (`BatchPayrollCreateResult`, `BatchEscrowCreateResult`).
It provides a **single transaction** with **per-item diagnostics**, which is
recommended for off-chain orchestration and dashboards.
