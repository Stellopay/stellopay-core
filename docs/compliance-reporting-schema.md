# Compliance Reporting Schema

## Overview

The `ComplianceReport` struct provides a versioned, aggregated view of an
employer/employee compliance relationship within the StelloPay ecosystem. It
aggregates data from three sources:

1. **Withholding Records** — logged directly to the ComplianceReporting contract.
2. **Payment History** — fetched from the PaymentHistory contract.
3. **Agreement Events** — fetched from the AuditLogger contract.

A flat, CSV-friendly projection of the same data is available via
`generate_flat_report`, which returns a `Vec<FlatReportRow>` — one row per
logical record, with all nested objects expanded into scalar fields.

---

## Structured Report: `ComplianceReport`

```rust
pub struct ComplianceReport {
    pub employer:          Address,
    pub employee:          Address,
    pub start_date:        u64,               // UNIX timestamp (inclusive)
    pub end_date:          u64,               // UNIX timestamp (inclusive)
    pub total_amount:      i128,              // sum of ComplianceRecord::amount
    pub record_count:      u32,              // equals records.len()
    pub records:           Vec<ComplianceRecord>,
    pub payment_history:   Vec<PaymentRecord>,
    pub agreement_events:  Vec<AuditLogEntry>,
    pub schema_version:    u32,              // currently 1
}
```

### `ComplianceRecord` fields

| Field | Type | Description |
|---|---|---|
| `id` | `u32` | Per-employer monotonic ID (1-based) |
| `global_seq` | `u64` | Contract-wide monotonic sequence |
| `employer` | `Address` | Employer address |
| `employee` | `Address` | Employee address |
| `token` | `Address` | Stellar asset contract address |
| `amount` | `i128` | Token amount (> 0) |
| `timestamp` | `u64` | Ledger timestamp at write time |
| `report_type` | `ReportType` | `Payroll` / `Tax` / `Regulatory` |
| `metadata` | `Bytes` | Off-chain reference (e.g. IPFS CID) |
| `publisher` | `Address` | Address that submitted the record |

---

## Flat Export: `FlatReportRow`

`generate_flat_report` returns `Vec<FlatReportRow>`. Each row represents one
record from one of the three data sources. Report-level header fields are
repeated in every row so the export is self-contained.

### Row ordering

1. All `"complianc"` rows (compliance withholding records), in the order
   returned by `generate_report`.
2. All `"payment"` rows (payment history records).
3. All `"audit"` rows (audit log entries).

`row_index` is 1-based and resets at the start of each section.

### Field definitions

#### Header fields (all rows)

| Column | Type | Description |
|---|---|---|
| `section` | `Symbol` | `"complianc"`, `"payment"`, or `"audit"` |
| `employer` | `Address` | Employer from `ComplianceReport::employer` |
| `employee` | `Address` | Employee from `ComplianceReport::employee` |
| `start_date` | `u64` | Reporting period start (UNIX timestamp) |
| `end_date` | `u64` | Reporting period end (UNIX timestamp) |
| `total_amount` | `i128` | Sum of withholding amounts in the period |
| `record_count` | `u32` | Number of compliance records in the report |
| `schema_version` | `u32` | Schema version (currently `1`) |

#### Common per-row fields

| Column | Type | Description |
|---|---|---|
| `row_index` | `u32` | 1-based index within this section |
| `timestamp_row` | `u64` | Record timestamp |
| `amount_row` | `i128` | Record amount |

#### Compliance-section fields (`section == "complianc"`)

| Column | Type | Description | Default for other sections |
|---|---|---|---|
| `compliance_id` | `u32` | `ComplianceRecord::id` | `0` |
| `global_seq` | `u64` | `ComplianceRecord::global_seq` | `0` |
| `token` | `Address` | Token contract address | zero-value |
| `report_type_u32` | `u32` | `0`=Payroll, `1`=Tax, `2`=Regulatory | `0` |
| `publisher` | `Address` | `ComplianceRecord::publisher` | zero-value |
| `metadata_len` | `u32` | Byte length of metadata blob | `0` |

#### Payment-section fields (`section == "payment"`)

| Column | Type | Description | Default for other sections |
|---|---|---|---|
| `payment_id` | `u128` | `PaymentRecord::id` | `0` |
| `agreement_id` | `u128` | `PaymentRecord::agreement_id` | `0` |
| `payer` | `Address` | `PaymentRecord::from` | zero-value |
| `token` | `Address` | `PaymentRecord::token` | zero-value |

#### Audit-section fields (`section == "audit"`)

| Column | Type | Description | Default for other sections |
|---|---|---|---|
| `audit_id` | `u64` | `AuditLogEntry::id` | `0` |
| `audit_action` | `Symbol` | `AuditLogEntry::action` | `"none"` |
| `audit_subject_set` | `bool` | `AuditLogEntry::subject.is_some()` | `false` |
| `payer` | `Address` | `AuditLogEntry::actor` | zero-value |

---

## Mapping from `ComplianceReport` to `FlatReportRow`

| `ComplianceReport` field | `FlatReportRow` column | Notes |
|---|---|---|
| `employer` | `employer` | Repeated in every row |
| `employee` | `employee` | Repeated in every row |
| `start_date` | `start_date` | Repeated in every row |
| `end_date` | `end_date` | Repeated in every row |
| `total_amount` | `total_amount` | Repeated in every row |
| `record_count` | `record_count` | Repeated in every row |
| `schema_version` | `schema_version` | Repeated in every row |
| `records[i].id` | `compliance_id` | `section = "complianc"` |
| `records[i].global_seq` | `global_seq` | `section = "complianc"` |
| `records[i].token` | `token` | `section = "complianc"` |
| `records[i].amount` | `amount_row` | `section = "complianc"` |
| `records[i].timestamp` | `timestamp_row` | `section = "complianc"` |
| `records[i].report_type` | `report_type_u32` | 0/1/2 |
| `records[i].publisher` | `publisher` | `section = "complianc"` |
| `records[i].metadata.len()` | `metadata_len` | Raw bytes not exposed |
| `payment_history[i].id` | `payment_id` | `section = "payment"` |
| `payment_history[i].agreement_id` | `agreement_id` | `section = "payment"` |
| `payment_history[i].from` | `payer` | `section = "payment"` |
| `payment_history[i].token` | `token` | `section = "payment"` |
| `payment_history[i].amount` | `amount_row` | `section = "payment"` |
| `payment_history[i].timestamp` | `timestamp_row` | `section = "payment"` |
| `agreement_events[i].id` | `audit_id` | `section = "audit"` |
| `agreement_events[i].action` | `audit_action` | `section = "audit"` |
| `agreement_events[i].subject.is_some()` | `audit_subject_set` | `section = "audit"` |
| `agreement_events[i].actor` | `payer` | `section = "audit"` |
| `agreement_events[i].amount.unwrap_or(0)` | `amount_row` | `section = "audit"` |
| `agreement_events[i].timestamp` | `timestamp_row` | `section = "audit"` |

---

## CSV Serialization Guidance

Column order for CSV export (21 columns):

```
section,employer,employee,start_date,end_date,total_amount,record_count,
schema_version,row_index,timestamp_row,amount_row,compliance_id,global_seq,
token,report_type_u32,publisher,metadata_len,payment_id,agreement_id,
payer,audit_action,audit_subject_set,audit_id
```

- **`Address`** values: serialize as Stellar strkey (`G...`) strings.
- **`Symbol`** values: serialize as plain ASCII strings (`complianc`, `payment`, `audit`, `none`).
- **`bool`** values: `true` / `false`.
- **Zero-value `Address`** (for fields not applicable to a section): the address
  placeholder used is the `employer` address — consumers **must** use `section`
  as the discriminator and treat cross-section Address fields as opaque
  placeholders.
- **`metadata_len`** carries the byte count of the raw metadata; the raw bytes
  are not included to keep rows flat. Fetch the raw metadata separately via
  `get_record(employer, id)` if needed.

### Example rows (pseudo-CSV)

```
# compliance row
complianc,G_EMPLOYER,G_EMPLOYEE,1700000000,1700086400,5000,1,1,1,1700042000,5000,1,42,G_TOKEN,0,G_PUBLISHER,0,0,0,G_EMPLOYER,none,false,0

# payment row
payment,G_EMPLOYER,G_EMPLOYEE,1700000000,1700086400,5000,1,1,1,1700041000,5000,0,0,G_TOKEN,0,G_EMPLOYER,0,7,99,G_PAYER,none,false,0

# audit row
audit,G_EMPLOYER,G_EMPLOYEE,1700000000,1700086400,5000,1,1,1,1700040000,0,0,0,G_EMPLOYER,0,G_EMPLOYER,0,0,0,G_ACTOR,create_agr,true,3
```

---

## Schema Versioning

`get_report_schema_version()` returns the current schema version (currently `1`).
The same value is embedded in every `ComplianceReport` as `schema_version`, so
indexers can read it from either the accessor or the report payload.

### Migration contract for downstream indexers

A schema version bump (`N → N+1`) is signalled by deploying a new contract
binary in which `get_report_schema_version()` returns `N+1`.  The following
invariants are guaranteed across any version advance:

| Guarantee | Details |
|---|---|
| **Existing records remain readable** | Records written under schema version N are stored in `persistent` storage keyed by `DataKey::Record(employer, id)`. A new binary never rewrites or deletes existing entries, so `get_record(employer, id)` and `get_withholding_records` will continue to deserialize them correctly. |
| **Field values are preserved exactly** | All scalar fields (`id`, `global_seq`, `amount`, `timestamp`, `report_type`, `publisher`, `metadata`) retain their original values as written. No back-fill or default-value substitution occurs on read. |
| **schema_version reflects the active binary** | After an upgrade, `get_report_schema_version()` and the `schema_version` field inside every newly-generated `ComplianceReport` both return `N+1`. Records fetched from storage that were written under version N still carry their original field values, but the wrapping `ComplianceReport` will have `schema_version = N+1`. |
| **Monotonic counters are not reset** | `global_seq` and per-employer record counts continue from where they were; the upgrade does not reset or reorder them. |

**Recommended indexer pattern:**

1. On startup, call `get_report_schema_version()` and store the value.
2. On each ingested event or report, compare the embedded `schema_version`.
   - Same version → parse as usual.
   - Higher version → apply the field mapping documented for that version before persisting.
3. Records whose raw `ComplianceRecord` fields you already snapshotted do not
   need to be re-fetched after an upgrade; their on-chain bytes are unchanged.

---

## Security

`generate_flat_report` exposes **exactly** the same data as `generate_report`.
No additional fields are introduced. The raw `metadata` bytes are intentionally
excluded from the flat export; only `metadata_len` is exposed, consistent with
the structured report's existing surface.

---

## Data Sourcing

- **Withholding Records** — stored in `ComplianceReportingContract` storage,
  keyed by `employer` and sequential `id`.
- **Payment History** — fetched from the `PaymentHistoryContract` configured
  via `set_contract_addresses`.
- **Agreement Events** — fetched from the `AuditLoggerContract` configured
  via `set_contract_addresses`.

`generate_flat_report` fails closed with `DependencyUnavailable` if either
external contract is not configured or unavailable — identical to
`generate_report`.
