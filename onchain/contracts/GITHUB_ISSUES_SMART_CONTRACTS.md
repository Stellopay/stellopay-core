---
title: "Implement and harden Stello Pay Contract (payroll, escrow, disputes)"
labels: ["soroban", "contracts", "stello-pay", "security"]
type: Task
assignees: ""
---

## Description

Deliver production-grade behavior for the central payroll and escrow orchestration contract: agreement lifecycle, funding, claims, disputes, grace periods, and milestone flows, with tests and documentation aligned to the threat model.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/stello-pay-hardening`
- Implement changes
  - Write or refine contract: `onchain/contracts/stello_pay_contract/src/lib.rs`
  - Write comprehensive tests: extend `onchain/contracts/stello_pay_contract/tests/` (add focused modules if needed for payroll vs escrow vs disputes)
  - Add or update documentation: `docs/architecture.md`, `docs/threat-model.md` (as applicable)
  - Include NatSpec-style comments (Rust `///` / `//!` on public entrypoints and invariants)
  - Validate security assumptions (auth, double-claim, refund vs claim ordering, dispute windows)
- Test and commit
  - Run `cargo test` for the crate and relevant integration tests
  - Cover edge cases (zero amounts, paused state, cancelled agreements, milestone ordering)
  - Include test output and security notes in the PR description

## Example commit message

`feat(stello-pay): harden payroll/escrow flows with tests and docs`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement and test Payroll Escrow custody and release invariants"
labels: ["soroban", "contracts", "escrow", "security"]
type: Task
assignees: ""
---

## Description

Ensure the payroll escrow contract correctly holds per-agreement token balances and only allows the designated manager contract to release or refund per policy, with full test and doc coverage.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/payroll-escrow-invariants`
- Implement changes
  - Write contract: `onchain/contracts/payroll_escrow/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/payroll_escrow/tests/` (add or extend `test_*.rs`)
  - Add documentation: `docs/architecture.md` (escrow section) or a dedicated `docs/payroll-escrow.md` if missing
  - Include NatSpec-style comments on release/refund entrypoints and balance accounting
  - Validate security assumptions (only manager, no balance drift, failed transfer handling)
- Test and commit
  - Run tests with `cargo test -p payroll_escrow`
  - Cover edge cases (empty agreement, partial release sequences, max amounts)
  - Include test output and security notes

## Example commit message

`feat(payroll-escrow): strengthen custody rules with tests and documentation`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Department Manager (orgs, departments, assignments) with tests"
labels: ["soroban", "contracts", "department-manager"]
type: Task
assignees: ""
---

## Description

Complete or harden hierarchical organization and department management, including employee-to-department assignment, with predictable auth and storage layout for integrators.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/department-manager`
- Implement changes
  - Write contract: `onchain/contracts/department_manager/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/department_manager/tests/`
  - Add documentation: `docs/` (department management doc if present; otherwise add `docs/department-management.md`)
  - Include NatSpec-style comments for admin vs org-owner operations
  - Validate security assumptions (who can create orgs, assign roles, reassign employees)
- Test and commit
  - Run `cargo test -p department_manager`
  - Cover edge cases (deep hierarchy, reassignment, revoked access)
  - Include test output and security notes

## Example commit message

`feat(department-manager): complete org hierarchy flows with tests and docs`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Payment Splitter (percent vs fixed) with arithmetic safety tests"
labels: ["soroban", "contracts", "payment-splitter", "testing"]
type: Task
assignees: ""
---

## Description

Deliver correct split computation (percentage and fixed allocations) with rounding discipline, overflow checks, and validation helpers for callers that perform token movement off-chain or in another contract.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/payment-splitter-safety`
- Implement changes
  - Write contract: `onchain/contracts/payment_splitter/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/payment_splitter/tests/`
  - Add documentation: `docs/` (payment splitting) or `docs/payment-splitting.md`
  - Include NatSpec-style comments on rounding and `validate_split_for_amount` behavior
  - Validate security assumptions (split sums to amount, no negative weights, duplicate recipients)
- Test and commit
  - Run property-style edge cases (dust, 1 stroop, uneven splits)
  - Include test output and security notes

## Example commit message

`feat(payment-splitter): harden split math with exhaustive tests and docs`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Token Vesting schedule with time-locked release"
labels: ["soroban", "contracts", "token-vesting", "security"]
type: Feature
assignees: ""
---

## Description

Develop a contract with a time-locked release mechanism for vesting schedules (cliff, linear segments, or documented schedule types supported by the implementation), integrated with the project’s token patterns.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/token-vesting`
- Implement changes
  - Write contract: `onchain/contracts/token_vesting/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/token_vesting/tests/test_vesting.rs` (split into modules if large)
  - Add documentation: `docs/token-vesting.md` (or align with existing doc name under `docs/`)
  - Include NatSpec-style comments on schedule creation, revoke rules (if any), and claim math
  - Validate security assumptions (no double-claim, correct remaining balance, token transfer failures)
- Test and commit
  - Run `cargo test -p token_vesting`
  - Cover edge cases (claim at cliff boundary, schedule end, partial revokes if applicable)
  - Include test output and security notes

## Example commit message

`feat: implement token vesting with tests and docs`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Compliance Reporting contract with auditable exports"
labels: ["soroban", "contracts", "compliance"]
type: Task
assignees: ""
---

## Description

Provide on-chain compliance reporting structures (events, snapshots, or attestations as designed) so off-chain indexers can reconstruct reporting periods without trusting centralized databases alone.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/compliance-reporting`
- Implement changes
  - Write contract: `onchain/contracts/compliance_reporting/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/compliance_reporting/tests/`
  - Add documentation: `docs/compliance-reporting.md` (create if absent)
  - Include NatSpec-style comments for authorized publishers and data retention semantics
  - Validate security assumptions (only compliant roles can write; tamper-evident event ordering)
- Test and commit
  - Cover edge cases (empty reports, large batches, replay concerns)
  - Include test output and security notes

## Example commit message

`feat(compliance-reporting): add auditable reporting with tests and docs`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Price Oracle contract with freshness and admin controls"
labels: ["soroban", "contracts", "oracle", "security"]
type: Task
assignees: ""
---

## Description

Implement FX or reference rate publication with timestamps, staleness checks, and clear admin/operator roles suitable for payroll conversion flows.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/price-oracle`
- Implement changes
  - Write contract: `onchain/contracts/price_oracle/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/price_oracle/tests/`
  - Add documentation: `docs/price-oracle.md` or a section in architecture docs
  - Include NatSpec-style comments on update frequency, decimals, and failure modes
  - Validate security assumptions (oracle compromise blast radius, stale rate rejection)
- Test and commit
  - Cover edge cases (zero rate, future timestamps, max deviation if enforced)
  - Include test output and security notes

## Example commit message

`feat(price-oracle): add rate publication with staleness checks and tests`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Compliance Checker rules engine and negative tests"
labels: ["soroban", "contracts", "compliance", "testing"]
type: Task
assignees: ""
---

## Description

Encode compliance validation logic (allow/deny/reason codes) for payroll actions, with exhaustive negative tests so invalid transitions cannot bypass checks.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/compliance-checker`
- Implement changes
  - Write contract: `onchain/contracts/compliance_checker/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/compliance_checker/tests/`
  - Add documentation: `docs/compliance-checker.md`
  - Include NatSpec-style comments describing rule precedence
  - Validate security assumptions (cannot be bypassed by auxiliary contracts unless explicitly allowed)
- Test and commit
  - Cover edge cases (conflicting rules, default deny, policy updates)
  - Include test output and security notes

## Example commit message

`feat(compliance-checker): complete rule checks with negative test suite`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Employee Roles contract and permission matrix tests"
labels: ["soroban", "contracts", "rbac", "testing"]
type: Task
assignees: ""
---

## Description

Model employee-scoped roles and permissions used by payroll flows, with explicit tests for each role’s allowed and forbidden actions.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/employee-roles`
- Implement changes
  - Write contract: `onchain/contracts/employee_roles/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/employee_roles/tests/`
  - Add documentation: `docs/employee-roles.md`
  - Include NatSpec-style comments mapping roles to capabilities
  - Validate security assumptions (role escalation, delegation if any)
- Test and commit
  - Matrix tests for allow/deny paths
  - Include test output and security notes

## Example commit message

`feat(employee-roles): document permission matrix and expand tests`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Multisig for critical operations (threshold, emergency guardian)"
labels: ["soroban", "contracts", "multisig", "security"]
type: Task
assignees: ""
---

## Description

Harden threshold-based proposals and approvals for upgrades, large payments, or dispute resolutions, including optional emergency guardian behavior as documented.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/multisig-hardening`
- Implement changes
  - Write contract: `onchain/contracts/multisig/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/multisig/tests/test_multisig.rs` (and additional files if needed)
  - Add documentation: `docs/multisig.md`
  - Include NatSpec-style comments on propose/approve/execute and guardian semantics
  - Validate security assumptions (replay, duplicate approvals, threshold changes mid-flight if allowed)
- Test and commit
  - Cover edge cases (1-of-n, n-of-n, guardian-only rescue)
  - Include test output and security notes

## Example commit message

`feat(multisig): strengthen operation lifecycle with tests and docs`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Fee Collector with transparent routing and accounting tests"
labels: ["soroban", "contracts", "fees"]
type: Task
assignees: ""
---

## Description

Ensure fees are accrued and routed per policy (treasury, burn, or split) with clear events and no ambiguous balance leftovers.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/fee-collector`
- Implement changes
  - Write contract: `onchain/contracts/fee_collector/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/fee_collector/tests/`
  - Add documentation: `docs/fee-collector.md`
  - Include NatSpec-style comments on fee basis points and recipients
  - Validate security assumptions (only authorized collectors; rounding)
- Test and commit
  - Cover edge cases (minimum fee, zero fee, multi-token if supported)
  - Include test output and security notes

## Example commit message

`feat(fee-collector): add fee routing invariants and tests`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement NFT Payroll Badge (soulbound or transferable per spec) with metadata safety"
labels: ["soroban", "contracts", "nft", "security"]
type: Feature
assignees: ""
---

## Description

Deliver NFT issuance tied to employment or agreement milestones with safe mint/burn rules and clear integration points for payroll status.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/nft-payroll-badge`
- Implement changes
  - Write contract: `onchain/contracts/nft_payroll_badge/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/nft_payroll_badge/tests/`
  - Add documentation: `docs/nft-payroll-badge.md`
  - Include NatSpec-style comments on mint authority and revocation
  - Validate security assumptions (no unauthorized mint, metadata injection risks)
- Test and commit
  - Cover edge cases (duplicate token id, transfer restrictions)
  - Include test output and security notes

## Example commit message

`feat(nft-payroll-badge): implement badge lifecycle with tests and docs`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Governance contract (proposals, voting, execution guards)"
labels: ["soroban", "contracts", "governance", "security"]
type: Task
assignees: ""
---

## Description

Provide on-chain governance primitives aligned with Stellopay upgrade and parameter changes, including timelocks or execution windows if required by design.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/governance`
- Implement changes
  - Write contract: `onchain/contracts/governance/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/governance/tests/`
  - Add documentation: `docs/governance.md`
  - Include NatSpec-style comments on proposal lifecycle
  - Validate security assumptions (double execution, vote inflation, snapshot rules)
- Test and commit
  - Cover edge cases (tie votes, late votes, cancelled proposals)
  - Include test output and security notes

## Example commit message

`feat(governance): add proposal lifecycle tests and documentation`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Salary Adjustment workflow with audit trail"
labels: ["soroban", "contracts", "payroll"]
type: Task
assignees: ""
---

## Description

Support employer-driven salary changes with effective dates, caps, and visibility to payroll claiming logic, including event logs for auditors.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/salary-adjustment`
- Implement changes
  - Write contract: `onchain/contracts/salary_adjustment/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/salary_adjustment/tests/`
  - Add documentation: `docs/salary-adjustment.md`
  - Include NatSpec-style comments on effective time and reversions
  - Validate security assumptions (only employer; retroactive abuse)
- Test and commit
  - Cover edge cases (mid-period adjustments, rollback if supported)
  - Include test output and security notes

## Example commit message

`feat(salary-adjustment): implement adjustments with tests and docs`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Payment Scheduler (cron-like triggers) with determinism tests"
labels: ["soroban", "contracts", "scheduler"]
type: Task
assignees: ""
---

## Description

Encode scheduled payment triggers with deterministic IDs and idempotent execution semantics suitable for Soroban’s execution model.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/payment-scheduler`
- Implement changes
  - Write contract: `onchain/contracts/payment_scheduler/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/payment_scheduler/tests/`
  - Add documentation: `docs/payment-scheduler.md`
  - Include NatSpec-style comments on schedule creation and cancellation
  - Validate security assumptions (replay of scheduled fires, owner auth)
- Test and commit
  - Cover edge cases (same timestamp, overlapping schedules)
  - Include test output and security notes

## Example commit message

`feat(payment-scheduler): add deterministic schedule tests and documentation`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Tax Withholding accrual and remittance hooks"
labels: ["soroban", "contracts", "tax", "compliance"]
type: Task
assignees: ""
---

## Description

Model withholding amounts per jurisdiction or policy bucket, with clear separation between employee net pay and withheld liabilities.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/tax-withholding`
- Implement changes
  - Write contract: `onchain/contracts/tax_withholding/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/tax_withholding/tests/`
  - Add documentation: `docs/tax-withholding.md`
  - Include NatSpec-style comments on rounding toward treasury vs employee protection
  - Validate security assumptions (cannot redirect withholdings to arbitrary addresses)
- Test and commit
  - Cover edge cases (changing rates mid-period if applicable)
  - Include test output and security notes

## Example commit message

`feat(tax-withholding): implement withholding math with tests and docs`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Dispute Escalation ladder with timeout and binding outcomes"
labels: ["soroban", "contracts", "disputes", "security"]
type: Task
assignees: ""
---

## Description

Extend dispute handling beyond single-arbiter resolution where required: escalation tiers, deadlines, and finality rules integrated with payroll state.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/dispute-escalation`
- Implement changes
  - Write contract: `onchain/contracts/dispute_escalation/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/dispute_escalation/tests/` plus `onchain/integration_tests/tests/test_dispute_escalation_integration.rs` alignment
  - Add documentation: `docs/dispute-escalation.md`
  - Include NatSpec-style comments on state machine transitions
  - Validate security assumptions (no funds stuck; cannot double-resolve)
- Test and commit
  - Cover edge cases (missed escalation window, concurrent disputes)
  - Include test output and security notes

## Example commit message

`feat(dispute-escalation): complete escalation flows with integration tests`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Expense Reimbursement approvals and payout guarantees"
labels: ["soroban", "contracts", "expenses"]
type: Task
assignees: ""
---

## Description

Support submission, approval, and payout of expense claims with escrowed funds and rejection paths that preserve employer and employee guarantees.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/expense-reimbursement`
- Implement changes
  - Write contract: `onchain/contracts/expense_reimbursement/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/expense_reimbursement/tests/`
  - Add documentation: `docs/expense-reimbursement.md`
  - Include NatSpec-style comments on receipt commitments (hashes) if used
  - Validate security assumptions (approver collusion bounds, refund rules)
- Test and commit
  - Cover edge cases (partial approval, duplicate claim ids)
  - Include test output and security notes

## Example commit message

`feat(expense-reimbursement): add approval workflow tests and documentation`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Payment Retry policy for failed transfers"
labels: ["soroban", "contracts", "reliability"]
type: Task
assignees: ""
---

## Description

Define retry counters, backoff or manual retry entrypoints, and interaction with payroll completion state when token sends fail.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/payment-retry`
- Implement changes
  - Write contract: `onchain/contracts/payment_retry/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/payment_retry/tests/`
  - Add documentation: `docs/payment-retry.md`
  - Include NatSpec-style comments on idempotency
  - Validate security assumptions (cannot drain via infinite retries)
- Test and commit
  - Cover edge cases (max retries exceeded, alternate payout address if supported)
  - Include test output and security notes

## Example commit message

`feat(payment-retry): implement retry policy with tests and docs`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Rate Limiter for sensitive entrypoints"
labels: ["soroban", "contracts", "security", "dos"]
type: Task
assignees: ""
---

## Description

Add per-address or global rate limits for abuse-prone operations (e.g., spam proposals, rapid policy toggles) appropriate to Soroban costs and fairness.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/rate-limiter`
- Implement changes
  - Write contract: `onchain/contracts/rate_limiter/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/rate_limiter/tests/`
  - Add documentation: `docs/rate-limiter.md`
  - Include NatSpec-style comments on windows and burst handling
  - Validate security assumptions (cannot lock out legitimate admins permanently)
- Test and commit
  - Cover edge cases (boundary timestamps, clock skew assumptions documented)
  - Include test output and security notes

## Example commit message

`feat(rate-limiter): add throttling helpers with tests and documentation`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Audit Logger (tamper-evident event chain or snapshots)"
labels: ["soroban", "contracts", "audit", "compliance"]
type: Task
assignees: ""
---

## Description

Record security-relevant actions for later forensic reconstruction, minimizing PII while preserving traceability of privileged operations.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/audit-logger`
- Implement changes
  - Write contract: `onchain/contracts/audit_logger/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/audit_logger/tests/`
  - Add documentation: `docs/audit-logger.md`
  - Include NatSpec-style comments on retention and access control
  - Validate security assumptions (log injection, unauthorized writers)
- Test and commit
  - Cover edge cases (max log size, pagination if exposed)
  - Include test output and security notes

## Example commit message

`feat(audit-logger): implement append-only audit trail with tests`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement RBAC core (roles, admins, guards) with negative tests"
labels: ["soroban", "contracts", "rbac", "security"]
type: Task
assignees: ""
---

## Description

Centralize role-based access control primitives reused across modules, with explicit tests proving forbidden paths cannot succeed.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/rbac-core`
- Implement changes
  - Write contract: `onchain/contracts/rbac/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/rbac/tests/`
  - Add documentation: `docs/rbac.md` (or threat-model linkage)
  - Include NatSpec-style comments on role grants/revokes
  - Validate security assumptions (admin takeover, role cycling)
- Test and commit
  - Matrix tests for role combinations
  - Include test output and security notes

## Example commit message

`feat(rbac): expand role guards and negative test coverage`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Slashing Penalty for provable violations"
labels: ["soroban", "contracts", "slashing", "security"]
type: Task
assignees: ""
---

## Description

Encode slashing rules tied to evidence or signed attestations (as per design), with safeguards against unjust confiscation and appeal windows if applicable.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/slashing-penalty`
- Implement changes
  - Write contract: `onchain/contracts/slashing_penalty/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/slashing_penalty/tests/`
  - Add documentation: `docs/slashing-penalty.md`
  - Include NatSpec-style comments on evidence format and quorum
  - Validate security assumptions (only slasher role; proportionality)
- Test and commit
  - Cover edge cases (zero slash, max slash, repeated offenses)
  - Include test output and security notes

## Example commit message

`feat(slashing-penalty): implement slashing flows with tests and docs`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Bonus System (one-time and recurring) with escrow guarantees"
labels: ["soroban", "contracts", "bonus", "security"]
type: Task
assignees: ""
---

## Description

Align bonus and incentive flows with approval gates and escrow-backed payouts as described in project documentation.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/bonus-system`
- Implement changes
  - Write contract: `onchain/contracts/bonus_system/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/bonus_system/tests/test_bonus.rs`
  - Add documentation: `docs/bonus-system.md`
  - Include NatSpec-style comments on approve/reject/claim ordering
  - Validate security assumptions (employer cannot claw back after approval)
- Test and commit
  - Cover edge cases (recurring intervals, partial claims)
  - Include test output and security notes

## Example commit message

`feat(bonus-system): harden incentive lifecycle with tests and documentation`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Withdrawal Timelock for privileged withdrawals"
labels: ["soroban", "contracts", "timelock", "security"]
type: Task
assignees: ""
---

## Description

Add delay between proposal and execution for sensitive withdrawals or parameter changes, reducing incident blast radius.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/withdrawal-timelock`
- Implement changes
  - Write contract: `onchain/contracts/withdrawal_timelock/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/withdrawal_timelock/tests/`
  - Add documentation: `docs/withdrawal-timelock.md`
  - Include NatSpec-style comments on delay configuration and cancellation
  - Validate security assumptions (cannot fast-path; queue ordering)
- Test and commit
  - Cover edge cases (delay reduction attempts, queued ops)
  - Include test output and security notes

## Example commit message

`feat(withdrawal-timelock): add execution delay with full test suite`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Implement Payment History indexing-friendly events and queries"
labels: ["soroban", "contracts", "history", "indexing"]
type: Task
assignees: ""
---

## Description

Expose a stable history model for payments (ids, hashes, pointers) so indexers can build UIs without recomputing payroll math.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/payment-history`
- Implement changes
  - Write contract: `onchain/contracts/payment_history/src/lib.rs`
  - Write comprehensive tests: `onchain/contracts/payment_history/tests/`
  - Add documentation: `docs/payment-history.md`
  - Include NatSpec-style comments on pagination keys and event payloads
  - Validate security assumptions (history tampering, unauthorized pruning)
- Test and commit
  - Cover edge cases (large history, boundary reads)
  - Include test output and security notes

## Example commit message

`feat(payment-history): add query surface and history tests`

## Guidelines

- Minimum 95 percent test coverage
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Expand on-chain integration tests for cross-contract workflows"
labels: ["soroban", "contracts", "integration-tests", "testing"]
type: Task
assignees: ""
---

## Description

Strengthen `onchain/integration_tests` to cover realistic multi-contract sequences (escrow + payroll + dispute + optional module), including failure injection where Soroban testutils allow.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b feature/integration-tests-workflows`
- Implement changes
  - Orchestrate tests in: `onchain/integration_tests/tests/test_workflows.rs`, `test_dispute_escalation_integration.rs`, `load.rs` (as appropriate)
  - Add notes in: `docs/architecture.md` or `docs/threat-model.md` for covered workflows
  - Include comments describing each workflow’s threat assumptions
  - Validate security assumptions (ordering, cross-contract auth, token conservation)
- Test and commit
  - Run `cargo test -p integration_tests` (package name per `onchain/integration_tests/Cargo.toml`)
  - Cover edge cases (partial failures mid-workflow)
  - Include test output and security notes

## Example commit message

`test(integration): expand cross-contract workflow coverage`

## Guidelines

- Minimum 95 percent test coverage (for new test code paths and helpers added)
- Clear documentation
- Timeframe: 96 hours

++++++

---
title: "Security review pass: reentrancy, auth, and token conservation across contracts"
labels: ["soroban", "contracts", "security", "audit"]
type: Task
assignees: ""
---

## Description

Perform a focused security review of the contract suite under `onchain/contracts/*`: document invariants, add targeted regression tests for any gaps found, and align `docs/threat-model.md` with actual entrypoints.

## Requirements and context

- Must be secure, tested, and documented
- Should be efficient and easy to review

## Suggested execution

- Fork the repo and create a branch
- `git checkout -b chore/contracts-security-review`
- Implement changes
  - Review and patch contracts: `onchain/contracts/*/src/lib.rs` as needed (minimal diffs)
  - Add regression tests next to each affected crate’s `tests/`
  - Update documentation: `docs/threat-model.md`, `docs/architecture.md`
  - Include NatSpec-style comments where invariants were unclear
  - Validate security assumptions (token conservation, authorization completeness)
- Test and commit
  - Full `cargo test` for on-chain workspace scope used in CI
  - Summarize findings and mitigations in PR (test output + security notes)

## Example commit message

`chore(contracts): security review findings and regression tests`

## Guidelines

- Minimum 95 percent test coverage on new/changed test code
- Clear documentation
- Timeframe: 96 hours
