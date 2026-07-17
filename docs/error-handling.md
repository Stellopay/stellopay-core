## Error Handling Guide

This guide summarizes the main error types and patterns used in the Stellopay payroll contracts, with a focus on **error codes**, **caller responsibilities**, and **recovery strategies**.

It is intentionally concise and is meant to complement the inline NatSpec-style comments and tests.

---

### Core Error Type: `PayrollError`

On-chain type: `PayrollError` (see `onchain/contracts/stello_pay_contract/src/storage.rs`).

This enum is annotated with `#[contracterror]` and `#[repr(u32)]`, so each variant has a stable numeric code surfaced to clients.

Selected variants and intended meaning:

- `DisputeAlreadyRaised (1)` – a dispute is already active for this agreement
- `NotInGracePeriod (2)` – operation requires the agreement to be in grace period
- `NotParty (3)` – caller is not employer/employee for this agreement
- `NotArbiter (4)` – caller is not the configured arbiter
- `InvalidPayout (5)` – `pay_employee + refund_employer` exceeds total locked funds
- `ActiveDispute (6)` – operation blocked while dispute is active
- `AgreementNotFound (7)` – referenced agreement id does not exist
- `NoDispute (8)` – attempting to resolve or query a non‑existent dispute
- `NoEmployee (9)` – employee index or address not present in agreement
- `NotActivated (10)` / `AgreementNotActivated (17)` – agreement must be active
- `Unauthorized (11)` – generic access control violation (e.g., wrong caller)
- `InvalidEmployeeIndex (12)` – out‑of‑range employee index
- `InvalidData (13)` – malformed or inconsistent stored data
- `TransferFailed (14)` – token transfer client call returned an error
- `InsufficientEscrowBalance (15)` – agreement escrow does not cover requested payment
- `NoPeriodsToClaim (16)` – time‑based escrow has no newly claimable periods
- `InvalidAgreementMode (18)` – operation incompatible with agreement mode
- `AgreementPaused (19)` – operation not allowed while agreement is `Paused`
- `AllPeriodsClaimed (20)` – all time‑based periods already claimed
- `ZeroAmountPerPeriod (21)` – invalid configuration, amount per period must be > 0
- `ZeroPeriodDuration (22)` – invalid configuration, duration per period must be > 0
- `ZeroNumPeriods (23)` – invalid configuration, number of periods must be > 0

### Full `PayrollError` catalogue (all variants)

The complete, current set of `PayrollError` variants with their stable `#[repr(u32)]` discriminants. Source of truth: [`onchain/contracts/stello_pay_contract/src/storage.rs`](onchain/contracts/stello_pay_contract/src/storage.rs).

| Discriminant | Variant | Meaning |
|-------------:|---------|---------|
| 1 | `DisputeAlreadyRaised` | A dispute is already active for this agreement. |
| 2 | `NotInGracePeriod` | Operation requires the agreement to be in its grace period. |
| 3 | `NotParty` | Caller is not the employer or employee for this agreement. |
| 4 | `NotArbiter` | Caller is not the configured arbiter. |
| 5 | `InvalidPayout` | `pay_employee + refund_employer` exceeds total locked funds. |
| 6 | `ActiveDispute` | Operation blocked while a dispute is active. |
| 7 | `AgreementNotFound` | Referenced agreement id does not exist. |
| 8 | `NoDispute` | Attempting to resolve or query a non-existent dispute. |
| 9 | `NoEmployee` | Employee index or address not present in the agreement. |
| 10 | `NotActivated` | Agreement (or related state) must be active. |
| 11 | `Unauthorized` | Generic access-control violation (wrong caller/role). |
| 12 | `InvalidEmployeeIndex` | Out-of-range employee index. |
| 13 | `InvalidData` | Malformed or inconsistent stored data. |
| 14 | `TransferFailed` | Token transfer client call returned an error. |
| 15 | `InsufficientEscrowBalance` | Agreement escrow does not cover the requested payment. |
| 16 | `NoPeriodsToClaim` | Time-based escrow has no newly claimable periods. |
| 17 | `AgreementNotActivated` | Agreement must be activated before the operation. |
| 18 | `InvalidAgreementMode` | Operation incompatible with the agreement mode. |
| 19 | `AgreementPaused` | Operation not allowed while the agreement is `Paused`. |
| 20 | `AllPeriodsClaimed` | All time-based periods have already been claimed. |
| 21 | `ZeroAmountPerPeriod` | Config invalid: amount per period must be > 0. |
| 22 | `ZeroPeriodDuration` | Config invalid: duration per period must be > 0. |
| 23 | `ZeroNumPeriods` | Config invalid: number of periods must be > 0. |
| 24 | `EmergencyPaused` | Operation not allowed while the emergency pause is active. |
| 25 | `NotGuardian` | Caller is not the emergency guardian. |
| 26 | `TimelockActive` | A withdrawal/action timelock is still active. |
| 27 | `InvalidTimelock` | Supplied timelock configuration/value is invalid. |
| 28 | `MultisigApprovalRequired` | Operation requires multisig approval before execution. |
| 29 | `ExchangeRateNotFound` | Missing or unconfigured FX rate for a currency pair. |
| 30 | `ExchangeRateOverflow` | Arithmetic overflow/underflow during FX conversion. |
| 31 | `ExchangeRateInvalid` | Invalid FX rate (e.g. non-positive). |
| 32 | `GraceExtensionInvalid` | Grace-extension args invalid (zero, overflow, wrong status, unauthorized). |
| 33 | `GraceExtensionCapExceeded` | Extension would exceed the owner-configured cumulative cap. |
| 34 | `RateLimited` | Rate limiter rejected the call (too many requests for the caller). |
| 35 | `BatchTooLarge` | Caller supplied more than `MAX_BATCH_SIZE` batch items. |
| 36 | `MilestoneAmountInvalid` | Milestone amount must be strictly positive. |
| 37 | `MilestoneAgreementInvalidStatus` | Milestone agreement not in a valid status for the operation. |
| 38 | `MilestoneNotFound` | Referenced milestone (or its agreement record) was not found. |
| 39 | `MilestoneAlreadyApproved` | Milestone has already been approved. |
| 40 | `MilestoneNotApproved` | Milestone has not been approved yet. |
| 41 | `MilestoneAlreadyClaimed` | Milestone has already been claimed. |
| 42 | `EmployeeAlreadyExists` | Employee address already present; duplicate add would break the 1:1 mapping. |
| 43 | `ReentrancyDetected` | Reentrant call into a guarded claim path was detected. |

> **Append-only convention:** new variants are always **appended** with the next sequential `#[repr(u32)]` discriminant (the list currently ends at `43`). Never reorder, renumber, or insert a variant in the middle — existing on-chain clients and stored discriminants depend on this stability. Always update the table above when adding or removing a variant (source: [`storage.rs`](onchain/contracts/stello_pay_contract/src/storage.rs)).

These codes are surfaced in batch results:

- `PayrollClaimResult.error_code` (0 = success, otherwise `PayrollError` discriminant)
- `MilestoneClaimResult.error_code` (compact codes for milestone flows)

This allows off‑chain clients to distinguish **recoverable** issues (e.g., bad inputs, insufficient funds) from **hard** invariants (e.g., unauthorized caller).

---

### Error Handling Patterns

The contracts follow a small number of consistent patterns:

- **Typed errors for public functions**
  - Where practical, functions return `Result<_, PayrollError>` and use the enum above.
  - Clients should branch on `error_code` (for batches) or the `Result` error in direct calls.

- **`assert!`-style guards for internal invariants**
  - Many helper functions and validation checks use `assert!(...)` with a descriptive message.
  - These represent **programmer errors or violated assumptions** and are not intended as recoverable API contracts.

- **Access control failures**
  - Expressed as either `PayrollError::Unauthorized` or explicit `assert!(caller == expected, "...")`.
  - Recovery strategy: call from the proper address (e.g., employer, employee, arbiter) or adjust integration logic.

- **Mode and status checks**
  - Functions that depend on `AgreementMode` or `AgreementStatus` validate them first.
  - Typical responses:
    - `AgreementNotActivated`, `AgreementPaused`, `InvalidAgreementMode`
  - Recovery strategy: ensure agreement is created/activated and not paused before calling; for integrations, model the full lifecycle rather than calling arbitrarily.

---

### Recovery Strategies (By Category)

- **Authentication / Authorization**
  - Errors: `Unauthorized`, `NotParty`, `NotArbiter`
  - Recovery:
    - Re‑issue the transaction signed by the correct account (employer, employee, arbiter).
    - For UIs, hide actions that the current account cannot validly perform.

- **Configuration / Input Validation**
  - Errors: `ZeroAmountPerPeriod`, `ZeroPeriodDuration`, `ZeroNumPeriods`, `InvalidEmployeeIndex`
  - Recovery:
    - Validate inputs client‑side before submitting.
    - For batch operations, inspect `error_code` per item and surface field‑specific messages.

- **Lifecycle / Mode Mismatch**
  - Errors: `AgreementNotActivated`, `AgreementPaused`, `ActiveDispute`, `NotInGracePeriod`
  - Recovery:
    - Wait for the agreement to transition to the required state, or call the appropriate transition first (e.g., `activate_agreement`, `resume_agreement`, `finalize_grace_period`).
    - For disputes, ensure the dispute is raised or resolved in the correct order.

- **Funds and Transfers**
  - Errors: `InsufficientEscrowBalance`, `TransferFailed`, `InvalidPayout`
  - Recovery:
    - Fund the escrow or token balances appropriately before retrying.
    - Double‑check payout splits so that they do not exceed the total locked amount.

---

### Example: Handling Batch Payroll Claims

When calling `batch_claim_payroll`, the contract returns a `BatchPayrollResult`:

- check `total_claimed`, `successful_claims`, `failed_claims`
- iterate over `results: Vec<PayrollClaimResult>`
  - if `error_code == 0`, treat as success
  - otherwise, map the numeric code back to `PayrollError` for display or logging

This pattern generalizes to milestone batches (`BatchMilestoneResult`) and provides a **single transaction** with **per‑item diagnostics**, which is recommended for off‑chain orchestration and dashboards.  

