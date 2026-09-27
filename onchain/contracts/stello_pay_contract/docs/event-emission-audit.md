# `stello_pay_contract` event-emission audit

Status: review deliverable for issue #1274. This is a source audit, not a
claim that the current event surface is complete or audited. The table covers
the 77 public entrypoints in `src/lib.rs` on the audited `main` commit. Event
behavior is traced through the wrapper in `src/lib.rs` into `src/payroll.rs`,
and event definitions are listed in `src/events.rs`.

## Method and counting note

The public-entrypoint count is the number of `pub fn` declarations in
`onchain/contracts/stello_pay_contract/src/lib.rs`: 77. The current source
contains 17 direct `.publish(...)` calls in `src/lib.rs` and `src/payroll.rs`,
plus 28 calls to the event helper functions in `src/events.rs`. Counting only
literal `.publish` calls therefore undercounts emissions made through helpers.
The issue's reported 41-call snapshot should be reconciled against the exact
commit used for implementation before any event additions are made.

For this audit:

* **Present** means a successful state-changing path emits an event already
  visible in the implementation.
* **Silent/read** means the entrypoint is a getter or pure conversion and is
  intentionally not an event candidate.
* **Candidate — discuss** means the entrypoint changes configuration,
  upgrade/migration authority, emergency state, or accounting directly but
  has no corresponding event observed in the current path. These are
  prioritized for maintainer discussion; this PR does not add new event
  types or change event payloads.

## Complete entrypoint table

| # | Entrypoint (`src/lib.rs`) | Path class | Observed event / decision |
|---:|---|---|---|
| 1 | `initialize` | configuration mutation | Candidate — discuss initialization event. |
| 2 | `set_rbac_contract` | configuration mutation | Candidate — discuss linked-contract change event. |
| 3 | `set_rate_limiter_contract` | configuration mutation | Candidate — discuss linked-contract change event. |
| 4 | `get_rate_limiter_contract` | read | Silent/read — no event expected. |
| 5 | `set_salary_adjustment_contract` | configuration mutation | Candidate — discuss linked-contract change event. |
| 6 | `get_salary_adjustment_contract` | read | Silent/read — no event expected. |
| 7 | `set_milestone_hook_contract` | configuration mutation | Candidate — discuss hook change event. |
| 8 | `get_milestone_hook_contract` | read | Silent/read — no event expected. |
| 9 | `upgrade` | code mutation | Candidate — discuss upgrade event and payload policy. |
| 10 | `migrate_state` | storage mutation | Present — `ContractMigratedEvent`. |
| 11 | `create_payroll_agreement` | agreement mutation | Present — `AgreementCreatedEvent`. |
| 12 | `batch_create_payroll_agreements` | agreement mutation | Present — one `AgreementCreatedEvent` per created agreement. |
| 13 | `create_escrow_agreement` | agreement mutation | Present — agreement creation and employee-added emissions in delegated path. |
| 14 | `batch_create_escrow_agreements` | agreement mutation | Present — creation and employee-added emissions per created agreement. |
| 15 | `create_milestone_agreement` | agreement mutation | Present — `AgreementCreatedEvent`. |
| 16 | `fund_milestone_agreement` | escrow mutation | Present — `MilestoneFundedEvent`. |
| 17 | `add_milestone` | milestone mutation | Present — `MilestoneAdded` direct publish. |
| 18 | `approve_milestone` | milestone mutation | Present — `MilestoneApproved` direct publish. |
| 19 | `reject_milestone` | milestone mutation | Present — `MilestoneRejectedEvent`. |
| 20 | `expire_milestone` | milestone mutation | Present — `MilestoneExpiredEvent`. |
| 21 | `claim_milestone` | escrow mutation | Present — `MilestoneClaimed` direct publish. |
| 22 | `batch_claim_milestones` | escrow mutation | Present — `BatchMilestoneClaimedEvent`. |
| 23 | `get_milestone_count` | read | Silent/read — no event expected. |
| 24 | `get_milestone` | read | Silent/read — no event expected. |
| 25 | `add_employee_to_agreement` | agreement mutation | Present — `EmployeeAddedEvent`. |
| 26 | `activate_agreement` | agreement mutation | Present — `AgreementActivatedEvent`. |
| 27 | `get_agreement` | read | Silent/read — no event expected. |
| 28 | `get_agreement_employees` | read | Silent/read — no event expected. |
| 29 | `set_arbiter` | agreement mutation | Present — `ArbiterSetEvent`. |
| 30 | `get_arbiter` | read | Silent/read — no event expected. |
| 31 | `set_audit_logger` | configuration mutation | Candidate — discuss audit-logger address event. |
| 32 | `get_audit_logger` | read | Silent/read — no event expected. |
| 33 | `get_audit_entry_count` | read | Silent/read — no event expected. |
| 34 | `get_audit_entry` | read | Silent/read — no event expected. |
| 35 | `get_audit_entries_by_employer` | read | Silent/read — no event expected. |
| 36 | `raise_dispute` | dispute mutation | Present — `DisputeRaisedEvent`. |
| 37 | `resolve_dispute` | dispute mutation | Present — `DisputeResolvedEvent`. |
| 38 | `resolve_dispute_multisig` | dispute mutation | Present — delegated resolution emits `DisputeResolvedEvent`. |
| 39 | `set_multisig_config` | configuration mutation | Present — `MultisigConfigChangedEvent`. |
| 40 | `get_multisig_contract` | read | Silent/read — no event expected. |
| 41 | `get_dispute_status` | read | Silent/read — no event expected. |
| 42 | `set_exchange_rate_admin` | oracle mutation | Present — `ExchangeRateUpdatedEvent`. |
| 43 | `set_exchange_rate` | oracle mutation | Present — `ExchangeRateUpdatedEvent`. |
| 44 | `set_fx_rate_sanity_bound` | oracle configuration mutation | Candidate — discuss bound-change event. |
| 45 | `convert_currency` | pure conversion | Silent/read — no persistent state change. |
| 46 | `claim_payroll` | escrow mutation | Present — `PayrollClaimed`, `PaymentSent`, `PaymentReceived`. |
| 47 | `claim_payroll_multisig` | escrow mutation | Present — delegated claim path emits the payroll/payment events. |
| 48 | `claim_payroll_in_token` | escrow mutation | Present — base payroll plus payout-token payment events. |
| 49 | `batch_claim_payroll` | escrow mutation | Present — per-success payroll/payment events plus `BatchPayrollClaimedEvent`. |
| 50 | `get_employee_claimed_periods` | read | Silent/read — no event expected. |
| 51 | `pause_agreement` | agreement mutation | Present — `AgreementPausedEvent` on supported paths. |
| 52 | `resume_agreement` | agreement mutation | Present — `AgreementResumedEvent` on supported paths. |
| 53 | `claim_time_based` | escrow mutation | Present — `PaymentSent` and `PaymentReceived` on transfer. |
| 54 | `get_claimed_periods` | read | Silent/read — no event expected. |
| 55 | `cancel_agreement` | agreement mutation | Present — `AgreementCancelledEvent`. |
| 56 | `finalize_grace_period` | escrow mutation | Present — `GracePeriodFinalizedEvent`. |
| 57 | `is_grace_period_active` | read | Silent/read — no event expected. |
| 58 | `get_grace_period_end` | read | Silent/read — no event expected. |
| 59 | `extend_grace_period` | agreement mutation | Present — `GracePeriodExtendedEvent`. |
| 60 | `set_grace_extension_policy` | configuration mutation | Candidate — discuss policy-change event. |
| 61 | `get_grace_extension_policy` | read | Silent/read — no event expected. |
| 62 | `get_grace_extension_seconds` | read | Silent/read — no event expected. |
| 63 | `pause_employer_agreements` | bulk agreement mutation | Present — individual pause events plus bulk summary. |
| 64 | `unpause_employer_agreements` | bulk agreement mutation | Present — individual resume events plus bulk summary. |
| 65 | `set_emergency_guardians` | emergency configuration mutation | Present — `EmergencyGuardiansSetEvent` includes the new guardian count. |
| 66 | `get_emergency_guardians` | read | Silent/read — no event expected. |
| 67 | `propose_emergency_pause` | emergency state mutation | Candidate — discuss proposal event. |
| 68 | `approve_emergency_pause` | emergency state mutation | Candidate — discuss approval event. |
| 69 | `emergency_pause` | emergency state mutation | Candidate — discuss pause event. |
| 70 | `emergency_unpause` | emergency state mutation | Candidate — discuss unpause event. |
| 71 | `is_emergency_paused` | read | Silent/read — no event expected. |
| 72 | `get_emergency_pause_state` | read | Silent/read — no event expected. |
| 73 | `admin_set_agreement_paid_amount` | privileged accounting mutation | Candidate — discuss maintenance-write event with old/new values. |
| 74 | `admin_set_escrow_balance` | privileged accounting mutation | Candidate — discuss maintenance-write event with old/new values. |
| 75 | `admin_set_agreement_token` | privileged configuration mutation | Candidate — discuss token-change event. |
| 76 | `admin_set_activation_time` | privileged accounting mutation | Candidate — discuss maintenance-write event. |
| 77 | `admin_set_period_duration` | privileged accounting mutation | Candidate — discuss maintenance-write event. |

## Prioritized candidates for maintainer decision

The highest-risk silent mutations are the five emergency entrypoints
(`propose_emergency_pause`, `approve_emergency_pause`, `emergency_pause`, and
`emergency_unpause`, together with guardian configuration), because indexers
cannot reconstruct the emergency authorization timeline from the current
event surface. Next are `upgrade`/`migrate_state` and the admin storage
setters, because they can change executable code or accounting without an
event that identifies the operator and old/new values. Linked-contract and
policy setters follow because indexers may otherwise miss configuration drift.

These priorities are based on the mutation and authorization behavior in
`src/lib.rs` and `src/payroll.rs`, not on an exploitability claim. Before
implementation, maintainers should decide:

1. which of these mutations are part of the supported indexer contract;
2. whether event topics/payloads must preserve existing generated names and
   field shapes;
3. whether emergency proposals and approvals need distinct event identities;
4. what operator, old value, new value, timestamp, and agreement scope are
   allowed in each new payload; and
5. whether direct maintenance setters are intentionally invisible because they
   are migration-only operations, or must be monitored like normal mutations.

## Current test and size baseline

Existing event assertions live in
`onchain/contracts/stello_pay_contract/tests/test_event_emissions.rs`, with
additional path-specific assertions in the milestone, dispute, grace-period,
and upgrade tests. This audit does not change event behavior, so the expected
WASM delta for this PR is **0 bytes**. The issue's stated 10,562-byte headroom
therefore remains unchanged; any future event implementation must report its
measured release-WASM delta and re-run the repository size checks before
merging.

## Findings and limitations

The table records observed emissions, not a formal guarantee that every
delegated branch emits exactly once. Batch operations and the dual legacy/new
agreement paths require targeted tests when maintainers approve candidate
events. The existing event helper names `emit_dsipute_raised` and
`emit_dsipute_resolved` are misspelled in source; changing them would be an
API/internal refactor and is intentionally outside this audit. Existing event
payloads must remain compatible with consumers unless maintainers explicitly
approve a versioning plan.
