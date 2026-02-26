# Compliance Reporting Contract

## Overview
The Stellopay Compliance Reporting smart contract serves as an immutable, queryable ledger for tracking financial events across the protocol. It handles data aggregation for **Payroll**, **Tax**, and **Regulatory** requirements.

## Architecture
Because smart contracts operate under strict CPU and memory bounds, querying massive historical datasets on-chain is inefficient. This contract utilizes a chunked index structure. 

Authorized contracts (e.g., the Escrow or Scheduler) or the employer themselves log compliance metadata into this contract when an action occurs. Off-chain systems (like the Stellopay DApp frontend or a backend node) can then request filtered "Data Exports" within specific date bounds.

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