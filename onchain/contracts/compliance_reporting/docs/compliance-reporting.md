# Compliance Reporting Contract

## Overview
The Stellopay Compliance Reporting smart contract serves as an immutable, queryable ledger for tracking financial events across the protocol. It handles data aggregation for **Payroll**, **Tax**, and **Regulatory** requirements.

## Architecture
Because smart contracts operate under strict CPU and memory bounds, querying massive historical datasets on-chain is inefficient. This contract utilizes a chunked index structure. 

Authorized contracts (e.g., the Escrow or Scheduler) or the employer themselves log compliance metadata into this contract when an action occurs. Off-chain systems (like the Stellopay DApp frontend or a backend node) can then request filtered "Data Exports" within specific date bounds.

## Read/Write Asymmetry of the Emergency Pause

The `set_paused` / `is_paused` mechanism enforces an asymmetric access policy:

| Operation     | While Paused |
|---------------|-------------|
| `log_record`  | **Blocked** — returns `ComplianceError::ContractPaused` |
| `generate_report` | Allowed — reads pre-existing records normally |
| `get_withholding_records` | Allowed |
| `get_record` | Allowed |
| `get_record_count` | Allowed |

**Why?** Only write path (`log_record`) calls the `require_not_paused` guard. All read-only functions intentionally skip it so that off-chain indexers and dependent systems can continue to reconstruct history without interruption during an incident. See the [top-level design section](../lib.rs#L480-L508) in the contract source for the implementation.

> ✅ **Test coverage**: The `test_pause_read_write_asymmetry` test in `tests/test_compliance.rs` verifies this behavior end-to-end, including cross-contract reads via `generate_report` with mocked dependency contracts.

## Report Types (`ReportType`)
* `Payroll`: Standard salary, bonus, and wage disbursement records.
* `Tax`: Withheld amounts, government levies, or employer-side tax payments.
* `Regulatory`: Specialized compliance markers (e.g., KYC checkpoints, localized compliance fee deductions).

## Key Workflows

### 1. Logging a Record (`log_record`)
Records a new compliance event to the ledger. 
* **Auth Requirement**: Must be signed/authorized by the `employer`.
* **Metadata**: Accepts raw `Bytes`. This is ideal for storing IPFS CID hashes corresponding to physical PDF payslips, tax forms, or JSON metadata.

### 2. Exporting Data (`generate_report`)
Calculates totals and extracts raw records over a defined time window.
* **Date Range**: Specify `start_date` and `end_date` (UNIX timestamps).
* **Filters**: Provide an optional `ReportType` to isolate specific data (e.g., only `Tax`).
* **Pagination/Limits**: To ensure the RPC node does not hit Soroban instruction limits during iteration, `limit` must be `<= 100`. The query searches chronologically backwards (newest first).
* **Security / Isolation**: Reports are strictly scoped to the `employer` address specified in the parameters. Log records from different employers are cryptographically isolated in storage; a generated report for Employer A will mathematically never contain totals or logs belonging to Employer B, even when records are interleaved temporally across the contract.

## Example Export Output
When generating a report, the contract returns a structured object containing aggregated metrics alongside the raw list of transactions for easy CSV/PDF generation on the frontend:
```json
{
  "employer": "G...",
  "start_date": 1672531200,
  "end_date": 1675123200,
  "total_amount": 7500000000,
  "record_count": 50,
  "records": [ ... ]
}