# Compliance Reporting Schema

## Overview

The `ComplianceReport` struct provides a versioned, aggregated view of an employee's compliance and payment status within the StelloPay ecosystem. It aggregates data from multiple sources:

1.  **Withholding Records**: Logged directly to the ComplianceReporting contract.
2.  **Payment History**: Aggregated from the PaymentHistory contract.
3.  **Agreement Events**: Aggregated from the AuditLogger contract.

## Schema Definition

```rust
pub struct ComplianceReport {
    pub employer: Address,
    pub employee: Address,
    pub start_date: u64,
    pub end_date: u64,
    pub total_withholding: i128,
    pub withholding_count: u32,
    pub withholding_records: Vec<ComplianceRecord>,
    pub payment_history: Vec<PaymentRecord>,
    pub agreement_events: Vec<AuditLogEntry>,
    pub schema_version: u32,
}
```

## Schema Versioning

- **`get_report_schema_version()`**: Returns the current schema version (currently `1`).
- Off-chain consumers should check this version to ensure they are using the correct parser for the `ComplianceReport` struct fields.

## Data Sourcing

- **Withholding Records**: Stored directly in `ComplianceReportingContract` storage, indexed by `employer` and sequential `id`.
- **Payment History**: Fetched from the `PaymentHistoryContract` configured in `ComplianceReportingContract`. Specifically, records matching the `employee` address are retrieved.
- **Agreement Events**: Fetched from the `AuditLoggerContract` configured in `ComplianceReportingContract`.

## Metadata Length Limit

The metadata field in ComplianceRecord is limited to **2048 bytes** (MAX_METADATA_LENGTH).
This prevents storage griefing via unbounded per-record metadata.

- **Maximum:** 2048 bytes — accepted.
- **Over maximum:** 2049+ bytes — rejected with ComplianceError::MetadataTooLong.
- **Empty** (0 bytes) — always accepted.
