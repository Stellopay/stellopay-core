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
