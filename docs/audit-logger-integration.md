# Audit Logger Integration

## Overview

`stello_pay_contract` now records append-only lifecycle audit entries for the critical agreement transitions required by issue #441:

- `AgreementCreated`
- `AgreementActivated`
- `AgreementCancelled`
- `DisputeRaised`
- `DisputeResolved`

The contract stores a local `LifecycleAuditEntry` after each successful state mutation and, when configured, calls the shared `audit_logger` contract's append-only `append_log` entrypoint. The existing audit logger API in this repository exposes `append_log`; this integration treats that function as the canonical external record append operation.

## Configuration

The contract owner configures the shared logger with:

```rust
set_audit_logger(owner, audit_logger_address)
```

Only the initialized contract owner can set this address. If no logger is configured, lifecycle mutations still write local audit entries so forensic reconstruction remains available from `stello_pay_contract`.

## Audit Entry Shape

Each local entry contains:

- monotonic `id`
- ledger `timestamp`
- authenticated `actor`
- canonical `AuditEvent`
- `agreement_id`
- optional `subject`
- optional `amount`
- optional external `audit_logger` log id

## Security Notes

- Audit writes happen only after lifecycle state updates and event emission succeed.
- If a configured external audit logger rejects the append, the transaction reverts, preventing a committed lifecycle mutation without its audit record.
- Audit payloads avoid sensitive free-form text and store only structured lifecycle metadata.
- `set_audit_logger` is owner-gated and covered by tests.

## Validation

Run:

```bash
cd onchain
cargo test -p stello_pay_contract --test audit_logger_tests
```

## Expense reimbursement → audit logger flow (#519)

A separate cross-contract integration links an approved expense in
`expense_reimbursement` to an entry in `audit_logger`.

### Flow

1. Deploy both contracts.
2. Initialize the expense contract (`initialize(owner)`) and register approvers
   (`add_approver(owner, approver)`).
3. Initialize the audit logger (`initialize(owner, retention_limit)`).
4. Point the expense contract at the logger:
   `set_audit_logger(owner, audit_logger_address)`.
5. `submit_expense(...)` → `fund_expense(...)` →
   `approve_expense(approver, expense_id, amount)`.

On a successful approval the expense contract records an `expense_approved`
entry in the audit logger and stores the returned id in `Expense::audit_log_id`,
which is the on-chain link between the expense and its audit trail. Given an
approved expense, `audit_logger.get_log(audit_log_id)` returns the matching
record.

### Audit linkage invariants (tested)

- **Linkage consistency** — after approval, `audit_log_id` is `Some(id)` with
  `id > 0`, and `get_log(id)` has `actor == approver`,
  `action == "expense_approved"`, `subject == submitter`, and
  `amount == approved_amount`.
- **One entry per approval** — a successful approval increases
  `get_log_count()` by exactly one.
- **Unauthorized approval** — an approval by a non-approver fails, leaves the
  expense `Pending` with `audit_log_id == None`, and creates no audit entry.
- **Double approval** — re-approving an approved expense fails, creates no
  second entry, and leaves the original `audit_log_id` unchanged.
- **No logger configured** — approval still succeeds with `audit_log_id == None`.

### Tests

- `onchain/integration_tests/tests/test_expense_audit_integration.rs` — linkage
  consistency and negative paths (unauthorized / double approval).
- `onchain/integration_tests/tests/test_expense_audit_logger_integration.rs` —
  happy path, rejection, and unconfigured-logger cases.

```bash
cargo test -p integration_tests test_expense_audit
```
