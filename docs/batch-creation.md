Batch Agreement Creation

- Adds batch creation entry points to the payroll contract
- Supports payroll and escrow agreement creation in one transaction
- Emits per-agreement events exactly as single-creation functions
- Uses partial-success semantics: invalid inputs do not abort the batch

Interfaces

- batch_create_payroll_agreements(env, employer, items) -> Result<BatchPayrollCreateResult, PayrollError>
- batch_create_escrow_agreements(env, employer, items) -> Result<BatchEscrowCreateResult, PayrollError>

Parameters

- PayrollCreateParams
  - token: Address
  - grace_period_seconds: u64
- EscrowCreateParams
  - contributor: Address
  - token: Address
  - amount_per_period: i128
  - period_seconds: u64
  - num_periods: u32

Return Types

- BatchPayrollCreateResult
  - total_created: u32
  - total_failed: u32
  - agreement_ids: Vec<u128>
  - results: Vec<PayrollCreateResult { agreement_id?, success, error_code }>
- BatchEscrowCreateResult
  - total_created: u32
  - total_failed: u32
  - agreement_ids: Vec<u128>
  - results: Vec<EscrowCreateResult { agreement_id?, success, error_code }>

Security

- Requires employer authentication at the batch level
- Individual agreement validation mirrors single-create functions
- No external calls during creation; events emitted after state writes

Semantics

- Empty item list returns PayrollError::InvalidData
- Escrow inputs validated per entry; failures recorded, batch continues
- Payroll creation has no extra checks beyond auth; expected to succeed

Gas and UX

- Batch reduces repeated authorization and client roundtrips
- Reuses internal single-create logic for readability and auditability
- Emits per-agreement events; downstream indexing unchanged

Examples

- Creating 3 payroll agreements with distinct grace periods
- Creating escrow entries with one invalid period to demonstrate partial success
