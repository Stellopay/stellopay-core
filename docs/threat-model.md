# StellopayCore Threat Model

> **Scope**: This document threat-models the **StellopayCore Soroban contract suite** (the contracts under `onchain/contracts/*`) and related operational tooling (deployment, upgrades, and the CLI under `tools/cli`).
>
> **Goal**: Identify **assets**, **actors**, **trust boundaries**, **threats**, and **mitigations**. This document is intended to be updated as new features/entrypoints are added.

## 1) System overview

StellopayCore implements a decentralized payroll/escrow system on Stellar using **Soroban** smart contracts.

Key components in this repo:

- **Contract suite**: `onchain/contracts/*`
  - Primary orchestration contract: `onchain/contracts/stello_pay_contract`.
  - Supporting modules (examples): RBAC, multisig, audit logging, payment scheduler/splitter, dispute escalation, compliance reporting.
- **Developer tooling**: `tools/cli` (`stellopay-cli`) for deployment and querying.
- **Operational scripts**: `scripts/migrations/*` for upgrades / rollbacks.

### 1.1 Intended security properties

- Only authorized parties can create/modify agreements and initiate sensitive actions.
- Funds held/escrowed by contracts cannot be stolen or redirected.
- Payments execute deterministically according to agreement parameters.
- Emergency controls exist to limit blast radius during incidents.
- Upgrades are restricted and auditable.

---

## 2) Actors

| Actor | Description | Typical capabilities |
|---|---|---|
| Employer | Creates agreements, deposits funds, activates/pauses/cancels where allowed | Can call employer-authorized entrypoints and sign transactions |
| Employee / Contributor | Receives funds; may claim/withdraw where supported | Can call employee claim/withdraw entrypoints |
| Arbiter | Dispute mediator (where enabled) | Can resolve disputes per rules |
| Contract Owner / Admin | Contract initializer and upgrade authority; may control emergency features | Can call privileged admin entrypoints; can upgrade |
| Guardian(s) | Multi-party emergency pause approvers (where enabled) | Can approve emergency pause proposals |
| Oracle / FX admin | Entity allowed to set exchange rates (if multi-currency conversion enabled) | Can publish FX rates; critical integrity role |
| Attacker | Any unauthorized party attempting to steal funds, corrupt state, or DoS | Can submit transactions, replay calls, exploit auth/storage bugs |

---

## 3) Assets

| Asset | Why it matters |
|---|---|
| Escrowed token balances | Primary target: attacker may attempt to transfer to themselves |
| Agreement state (status, amounts, periods, milestones) | Tampering can cause over/under-payment or lock funds |
| Administrative privileges / upgrade authority | Compromise enables code replacement or policy bypass |
| Emergency pause controls | Compromise enables stopping the system (DoS) or preventing pause during incident |
| Oracle/FX rates (if used) | Manipulation can cause incorrect conversions and value loss |
| Audit logs / events | Used for monitoring, compliance, forensics; integrity helps detection |
| CLI config / secrets (optional) | Local risk: stolen secret keys used for deployments |

---

## 4) Trust boundaries & data flows

### 4.1 Trust boundaries

1. **On-chain contract boundary**
   - Soroban runtime enforces authorization (`Address::require_auth()`), storage access, and call semantics.
2. **User wallet boundary**
   - Users sign transactions externally. Compromised wallets = compromised actor.
3. **Token contract boundary**
   - Payments depend on token contract behavior (`soroban_sdk::token`). Token contracts are external dependencies.
4. **Oracle boundary (if enabled)**
   - Any exchange-rate admin/oracle is a trusted external input.
5. **Off-chain ops boundary**
   - Migrations, upgrades, deployment scripts, and CLI handling of secrets.

### 4.2 High-level diagram

```text
+---------------------+          +------------------------+
| Employer / Employee |          |   Admin / Guardians     |
| (wallets)           |          | (wallets / governance)  |
+----------+----------+          +-----------+------------+
           |                                 |
           | signed tx                        | signed tx
           v                                 v
   +------------------- Soroban / Stellar Network -------------------+
   |                                                                 |
   |  +-------------------+        +------------------------------+  |
   |  | stello_pay_contract|<------>| token contracts (external)   |  |
   |  | (agreements, claims|        | (transfers, balances)        |  |
   |  | dispute, pause)    |        +------------------------------+  |
   |  +-------------------+                                         |
   |           ^                                                     |
   |           | events/logs                                         |
   +-----------+-----------------------------------------------------+
               |
               | monitoring / indexing
               v
        +--------------+
        | Off-chain ops|
        | (CLI, scripts|
        |  indexers)    |
        +--------------+
```

---

## 5) Existing security measures in code (cross-references)

This section references concrete mechanisms present in the codebase.

### 5.1 Authorization checks

- Many entrypoints require the caller to authenticate using `require_auth()`.
  - Example: `PayrollContract::initialize` in `onchain/contracts/stello_pay_contract/src/lib.rs` stores `StorageKey::Owner` after owner auth.
  - Example: employer auth in milestone creation/approval flows in `onchain/contracts/stello_pay_contract/src/payroll.rs`.

### 5.2 Upgrade restrictions

- `stello_pay_contract` uses an upgradeable pattern:
  - `#[derive(Upgradeable)]` on `PayrollContract` in `onchain/contracts/stello_pay_contract/src/lib.rs`.
  - `UpgradeableInternal::_require_auth` gates upgrades by `StorageKey::Owner`.

### 5.3 Emergency pause / guardians

- Emergency pause state stored in `StorageKey::EmergencyPause` (persistent storage).
- Guardian approvals and threshold-based pause are implemented via:
  - `StorageKey::EmergencyGuardians`, `StorageKey::PendingPause`, `StorageKey::PauseApprovals`
  - Functions in `onchain/contracts/stello_pay_contract/src/payroll.rs` (e.g., `approve_emergency_pause`, `emergency_pause`, `emergency_unpause`).

### 5.4 Structured errors and batch safety

- Contract errors are enumerated (`PayrollError`) in `onchain/contracts/stello_pay_contract/src/storage.rs`.
- Batch claim operations are designed to be partially successful (errors captured per item) (see batch claim result types in `storage.rs`).

### 5.5 Entry points reviewed in security pass #351

The following externally callable entry points were reviewed and aligned to
auth/composition expectations:

- `expense_reimbursement::initialize` now requires `owner.require_auth()`.
- `expense_reimbursement::pay_expense` updates status to `Paid` before token transfer.
- `payroll_escrow::release` decrements per-agreement balance before transfer.
- `payroll_escrow::refund_remaining` zeroes per-agreement balance before transfer.
- `payment_scheduler::process_due_payments` commits execution counters and next schedule before transfer.
- `bonus_system::claim_incentive` commits claimed payout counters/status before transfer.
- `token_vesting::{claim, approve_early_release, revoke}` commit vesting state before transfer.

Security invariants for these paths:

- Auth completeness: every privileged or identity-bound operation gates with
  explicit `require_auth()`.
- Token conservation: each transfer path preserves per-object accounting
  (`released + remaining == funded`, modulo expected refunds/claims).
- Reentrancy resilience: mutable accounting is committed before external token
  interaction.

---

## 6) Threats and mitigations

### 6.1 Access control failures

**Threat**: Unauthorized party calls privileged function (e.g., upgrades, agreement state transitions, dispute resolution).

**Impact**: Funds theft, agreement tampering, system-wide compromise.

**Mitigations**:
- Enforce `Address::require_auth()` for all privileged actions.
- Centralize role model where possible (RBAC / multisig contracts) instead of ad-hoc checks.
- Add/maintain tests for unauthorized calls.
- Prefer explicit checks against stored employer/contributor addresses for per-agreement authorization.

**Code references**:
- Owner gating for upgrades: `UpgradeableInternal::_require_auth` in `onchain/contracts/stello_pay_contract/src/lib.rs`.
- Employer auth in `create_milestone_agreement` / `approve_milestone` in `onchain/contracts/stello_pay_contract/src/payroll.rs`.

---

### 6.2 Reentrancy / unexpected cross-contract call behavior

**Threat**: A malicious token contract or other invoked contract re-enters during transfer-like flows.

**Notes (Soroban)**:
- Soroban’s execution model differs from EVM; however, **cross-contract calls exist** and state can be mutated before/after these calls.

**Mitigations**:
- Apply a *checks-effects-interactions* style: validate and update state before external calls where safe.
- Avoid calling untrusted contracts during critical invariants without guards.
- Keep escrow balances / agreement paid tracking consistent even if token transfer fails; surface `TransferFailed`.

**Code references**:
- Token interactions use `soroban_sdk::token::TokenClient` (e.g., in `onchain/contracts/stello_pay_contract/src/payroll.rs`).

---

### 6.3 Oracle / FX rate manipulation (multi-currency)

**Threat**: Manipulated exchange rates cause incorrect payouts.

**Impact**: Value loss, unfair compensation, drain of escrow.

**Mitigations**:
- Restrict who can set rates (dedicated `ExchangeRateAdmin`).
- Consider timelocks, multi-sig, or bounded rate changes.
- Add sanity checks (non-zero, non-negative, overflow-checked fixed-point math).

**Code references**:
- Storage keys include `StorageKey::ExchangeRateAdmin` and FX scaling constant `FX_SCALE` in `onchain/contracts/stello_pay_contract/src/payroll.rs`.

---

### 6.4 Emergency pause abuse / governance failure

**Threat**: Attacker pauses system (DoS) or prevents pausing during exploit.

**Mitigations**:
- Restrict immediate pause to owner.
- Require guardian threshold for certain pause paths.
- Use timelock for pending pause execution.
- Emit events for pause requests/approvals/execution (recommended).

**Code references**:
- Guardian threshold computed as `(guardians.len() / 2) + 1` in `approve_emergency_pause` (`payroll.rs`).

---

### 6.5 Agreement lifecycle bugs (state machine errors)

**Threat**: Incorrect transitions (e.g., claim in paused state; approve milestone after completion) lead to loss or locked funds.

**Mitigations**:
- Enforce `AgreementStatus` checks at every transition.
- Use explicit invariants: `paid_amount <= total_amount`, claimed periods bounded, milestones count bounded.
- Define dispute/grace windows precisely and test boundary conditions against ledger time.
- Comprehensive test coverage for edge cases (pause, cancel, dispute, repeated claim).

**Code references**:
- Status checks in milestone claim/approval functions in `onchain/contracts/stello_pay_contract/src/payroll.rs`.
- Dispute window enforcement in `raise_dispute` in `onchain/contracts/stello_pay_contract/src/payroll.rs` (uses `cancelled_at` as window start when cancelled, otherwise `created_at`).
- Grace period refund transfer authorization in `finalize_grace_period` in `onchain/contracts/stello_pay_contract/src/payroll.rs` (contract-authorized token `transfer`).

---

### 6.6 Arithmetic overflow/underflow

**Threat**: Overflow in fixed-point FX math or payout calculations.

**Mitigations**:
- Use checked math patterns where possible.
- Keep scaling factors documented and consistent.
- Use `PayrollError::ExchangeRateOverflow` / `ExchangeRateInvalid` (see `storage.rs`) to fail safely.

---

### 6.7 Denial of service (storage bloat / large vectors)

**Threat**: Attacker triggers worst-case loops (large employee lists, milestone lists, batch inputs) causing high resource use.

**Mitigations**:
- Bound list sizes (employees per agreement, milestones per agreement, batch sizes).
- Prefer pagination for getters.
- Use batch APIs that are resilient and return partial results.

**Code references**:
- Batch result types: `BatchPayrollResult`, `BatchMilestoneResult` (`storage.rs`).

---

### 6.8 Cross-contract workflow orchestration drift

**Threat**: A realistic payout flow spans multiple contracts, but one step is
executed out of order or by the wrong authority. Examples include:

- Recording payment history from a non-payroll address
- Releasing escrow from a non-manager address
- Escalating a dispute after the allowed deadline
- Claiming an optional bonus before approval or unlock

**Impact**: Operators can mis-read workflow progress, retry successful steps,
or strand funds in module-specific escrows while the primary agreement moves
forward.

**Mitigations**:
- Keep each contract's auth boundary explicit in integration tests and
  orchestration code.
- Treat failed intermediate calls as state-preserving unless a success result
  is observed and indexed.
- Verify token conservation across employer, employee, and contract balances
  for multi-step workflows.
- Verify dispute deadlines and escalation levels with explicit ledger-time
  manipulation in integration tests.

**Integration coverage added in `onchain/integration_tests`**:
- Payroll plus `payment_history` only records successfully when executed as the
  payroll contract.
- Payroll plus `payroll_escrow` rejects unauthorized release attempts without
  mutating the tracked agreement balance.
- Payroll disputes can be mirrored into `dispute_escalation`, including both
  deadline-expiry failures and successful escalation/resolution sequences.
- Optional `bonus_system` claims are covered both before unlock and after
  approval/unlock.

---

### 6.9 Misconfiguration / key compromise (off-chain)

**Threat**: Leaked `secret_key` in CLI config; compromised admin wallet upgrades malicious code.

**Mitigations**:
- Do not store secrets in plaintext when possible.
- Use hardware wallets / multisig for owner/admin accounts.
- For upgrades, require multi-party review and staged rollout.
- Lock down CI artifacts and deployment environment.

**Repo references**:
- CLI config format in `tools/cli/README.md`.
- Migration scripts in `scripts/migrations/`.

---

## 7) Security checklist for new features

When adding new entrypoints or modules, update this document and confirm:

- [ ] Assets and trust boundaries impacted are listed.
- [ ] Authorization rules documented and enforced (`require_auth`).
- [ ] State-machine transitions validated for all paths.
- [ ] External calls reviewed for reentrancy / interaction risk.
- [ ] Arithmetic checked (especially FX / scaling).
- [ ] Events emitted for critical actions (auditability).
- [ ] Batch operations bounded and tested.
- [ ] Upgrade / admin changes documented.

---

## 8) Document maintenance

- Update `docs/threat-model.md` whenever:
  - a new contract is added under `onchain/contracts/*`,
  - a new privileged role is introduced,
  - payment flows, FX/oracles, disputes, or pause mechanics change.

- Prefer to add **explicit code links** (file + symbol name) and keep them current.
