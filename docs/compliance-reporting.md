# Compliance Reporting Contract

## Overview

The compliance reporting contract provides on-chain, tamper-evident structures so off-chain indexers can reconstruct reporting periods without trusting centralized databases alone. It stores immutable compliance records keyed by employer and exposes a queryable report interface with date and type filtering.

## Security Model

| Concern | Mechanism |
|---|---|
| Authorized publishers only | Only the employer themselves or an admin-allowlisted publisher address may write records |
| Admin immutability | Admin is set once at `initialize`; no transfer path exists |
| Tamper-evident ordering | Per-employer monotonic `id` + contract-wide `global_seq` let indexers detect gaps and replay attempts |
| Timestamp integrity | `timestamp` is set from `env.ledger().timestamp()` — callers cannot back-date records |
| Emergency pause | Admin can halt all writes while reads remain available for indexers |
| Amount validation | Zero and negative amounts are rejected at the contract level |

## Data Retention

Records are stored in `persistent` storage. Callers are responsible for extending ledger TTLs if long-term on-chain retention is required. Off-chain indexers should consume events and snapshot data independently of on-chain storage.

## Types

### `ReportType`

| Variant | Description |
|---|---|
| `Payroll` | Standard salary, bonus, and wage disbursement records |
| `Tax` | Withheld amounts, government levies, or employer-side tax payments |
| `Regulatory` | KYC checkpoints, localized compliance fee deductions, etc. |

### `ComplianceRecord`

| Field | Type | Description |
|---|---|---|
| `id` | `u32` | Per-employer monotonic identifier (1-based). Gaps indicate missing records. |
| `global_seq` | `u64` | Contract-wide monotonic counter. Enables cross-employer ordering for indexers. |
| `employer` | `Address` | Employer on whose behalf this record was logged |
| `employee` | `Address` | Payment recipient |
| `token` | `Address` | Soroban token contract address |
| `amount` | `i128` | Token amount (always > 0) |
| `timestamp` | `u64` | Ledger timestamp at write time (set by the contract) |
| `report_type` | `ReportType` | Classification of this record |
| `metadata` | `Bytes` | Off-chain reference data (e.g. IPFS CID of a payslip PDF) |
| `publisher` | `Address` | Address that submitted this record |

### `ComplianceReport`

Returned by `generate_report`. Contains aggregated totals and the matching record list.

## Entry Points

### `initialize(admin: Address)`

One-time setup. Sets the immutable admin and auto-registers admin as an authorized publisher.

- Errors: `AlreadyInitialized`

### `set_publisher(caller, publisher, authorized)`

Grants or revokes publisher authorization. Only the admin may call this.

- Authorized publishers may log records on behalf of any employer.
- Employers can always log their own records without being explicitly added.
- Errors: `NotAuthorized`

### `is_publisher(publisher) -> bool`

Returns whether an address is currently an authorized publisher.

### `set_paused(caller, paused)`

Pauses or unpauses the contract. Only the admin may call this.

- When paused, `log_record` is blocked. All read operations remain available.
- Errors: `NotAuthorized`

### `is_paused() -> bool`

Returns the current pause state.

### `log_record(publisher, employer, employee, token, amount, report_type, metadata) -> u32`

Logs a new compliance record. Returns the per-employer record `id`.

- `publisher` must be either the `employer` or an allowlisted publisher.
- `amount` must be > 0.
- Assigns a monotonically increasing per-employer `id` and a contract-wide `global_seq`.
- Emits a `log_comp` event with `(id, global_seq, timestamp, amount, report_type_u32)` for indexers.
- Errors: `NotInitialized`, `ContractPaused`, `NotAuthorized`, `InvalidAmount`

### `get_record_count(employer) -> u32`

Returns the total number of records logged for an employer.

### `get_record(employer, id) -> Option<ComplianceRecord>`

Fetches a single record by employer and per-employer ID.

### `get_global_seq() -> u64`

Returns the current contract-wide global sequence counter. Useful for indexers to detect gaps or missed events.

### `generate_report(employer, start_date, end_date, filter_type, limit) -> ComplianceReport`

Generates an aggregated report for a given employer and time window.

- Iterates backwards (newest-first) through the employer's records.
- Stops early when a record's timestamp falls below `start_date`, saving instruction budget.
- `limit` must be between 1 and 100 (inclusive).
- Errors: `NotInitialized`, `InvalidDateRange`, `QueryLimitExceeded`

## Events

| Topic | Data | Description |
|---|---|---|
| `("init",)` | `(admin,)` | Contract initialized |
| `("pub_set",)` | `(publisher, authorized)` | Publisher allowlist changed |
| `("paused",)` | `(paused,)` | Pause state changed |
| `("log_comp", employer)` | `(id, global_seq, timestamp, amount, report_type_u32)` | New compliance record logged |

The `log_comp` event encodes all fields needed for off-chain reconstruction without a separate storage read. `report_type_u32` maps as: `0 = Payroll`, `1 = Tax`, `2 = Regulatory`.

## Indexer Reconstruction Guide

An off-chain indexer can reconstruct any reporting period by:

1. Subscribing to `log_comp` events filtered by `employer`.
2. Verifying `id` values are contiguous (no gaps = no missing records).
3. Verifying `global_seq` values are strictly increasing across all employers.
4. Filtering by `timestamp` to isolate a reporting period.
5. Summing `amount` values for totals.

If gaps are detected in `id` or `global_seq`, the indexer should flag the period for manual review and fall back to direct storage reads via `get_record`.

## Example Report Output

```json
{
  "employer": "GABC...",
  "start_date": 1672531200,
  "end_date": 1675123200,
  "total_amount": 7500000000,
  "record_count": 50,
  "records": [
    {
      "id": 50,
      "global_seq": 312,
      "employer": "GABC...",
      "employee": "GXYZ...",
      "token": "GTOKEN...",
      "amount": 150000000,
      "timestamp": 1675100000,
      "report_type": "Payroll",
      "metadata": "0x516d...",
      "publisher": "GABC..."
    }
  ]
}
```

## Security Notes

- The admin address is immutable after initialization. There is no admin transfer function, preventing privilege escalation via social engineering.
- Publisher revocation takes effect immediately; revoked publishers cannot log new records.
- The emergency pause is a last-resort mechanism. It does not delete or modify existing records.
- Records have no update or delete path. Once written, a record is permanent for the lifetime of the contract's storage.
- The `generate_report` limit cap (100) prevents instruction-limit overflows on Soroban. For larger datasets, use multiple paginated calls or rely on indexed off-chain data.
