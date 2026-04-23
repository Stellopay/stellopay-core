## Core State Invariants

This document captures the **core state invariants** for the payroll contract,
with a focus on **agreements**, **escrow**, and **payroll accounting**. The
invariants are enforced by construction in the contract logic and validated
by the invariant test suite in `tests/invariant/test_invariants.rs`.

---

### Agreement-Level Invariants

On-chain type: `Agreement` (see `src/storage.rs`).

- **Non-negative amounts**
  - `agreement.total_amount >= 0`
  - `agreement.paid_amount >= 0`
- **Paid amount bounded by total**
  - `agreement.paid_amount <= agreement.total_amount`
- **Escrow balance safety (primary token)**
  - Let `B = DataKey::AgreementEscrowBalance(agreement_id, agreement.token)`
  - Then:
    - `B >= 0` (no negative escrow)
    - `B + agreement.paid_amount <= agreement.total_amount`
- **Dispute flags are consistent**
  - When `dispute_status == DisputeStatus::None`:
    - `dispute_raised_at == None`
  - When `dispute_status == DisputeStatus::Raised`:
    - `dispute_raised_at == Some(..)`
    - `status == AgreementStatus::Disputed`
  - When `dispute_status == DisputeStatus::Resolved`:
    - `dispute_raised_at == Some(..)`

These invariants must hold **before and after** any operation that mutates
agreement state (create, claim, cancel, finalize, raise/resolve dispute,
pause/resume).

---

### Escrow Invariants

Escrow agreements are identified by `agreement.mode == AgreementMode::Escrow`.
They use the time-based fields on `Agreement`:

- `amount_per_period: Some(i128)`
- `period_seconds: Some(u64)`
- `num_periods: Some(u32)`
- `claimed_periods: Some(u32)`

For all escrow agreements:

- **Well-formed configuration**
  - `amount_per_period > 0`
  - `num_periods > 0`
  - `claimed_periods <= num_periods`
  - `total_amount == amount_per_period * num_periods`
- **Grace-period encoding**
  - `grace_period_seconds == period_seconds * num_periods`
- **Accounting safety**
  - With `B = escrow_balance` on the primary token:
    - `B >= 0`
    - `B + paid_amount <= total_amount`

Operations that update escrow state (time-based `claim_time_based`, grace-period
`finalize_grace_period`, and dispute resolution for escrow agreements) must
preserve all of the above properties.

---

### Payroll Invariants

Payroll agreements are identified by `agreement.mode == AgreementMode::Payroll`
and maintain per-employee salaries via `AgreementEmployees`:

- `StorageKey::AgreementEmployees(agreement_id) -> Vec<EmployeeInfo>`
- Each `EmployeeInfo` has `salary_per_period > 0`

For all payroll agreements:

- **Time-based fields are unused on Agreement**
  - `amount_per_period.is_none()`
  - `period_seconds.is_none()`
  - `num_periods.is_none()`
- **Total locked equals sum of allocations**
  - Let `S = sum(employee.salary_per_period for employee in AgreementEmployees)`
  - Then: `agreement.total_amount == S`
- **Paid amount and escrow bounds**
  - Same as agreement-level invariants:
    - `paid_amount >= 0`
    - `paid_amount <= total_amount`
    - On the primary token, `escrow_balance >= 0` and
      `escrow_balance + paid_amount <= total_amount`

Payroll claims (`claim_payroll`, `batch_claim_payroll`,
`claim_payroll_in_token`) and pause/resume flows must preserve these invariants
even in the presence of failures or partial successes.

---

### Payment Splitter Invariants

For percentage-based payment splits:

- **Positive input amounts only**
  - `total_amount > 0`
- **Share conservation**
  - `sum(recipient_amounts) == total_amount`
- **Valid percentage configuration**
  - Every percentage share is `> 0`
  - Total percentage shares sum to `10000`
- **Deterministic dust handling**
  - Each recipient first receives `floor((bps * total_amount) / 10000)`
  - Remaining dust is distributed one unit at a time to the largest fractional remainders
  - Exact remainder ties are broken by canonical recipient address order
- **Order-independence for ties**
  - Reordering the input recipients must not change the final allocation per address

For fixed-amount payment splits:

- **Positive input amounts only**
  - `total_amount > 0`
- **Exact-match requirement**
  - `sum(fixed_amounts) == total_amount`
- **No implicit absorber**
  - No recipient receives a rounded or absorbed remainder
