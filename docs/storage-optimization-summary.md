# Storage TTL Optimization

## Overview

Under Soroban's state-archival model, long-lived but infrequently-accessed persistent storage entries can be archived by the protocol, breaking later claims. To prevent this, the stello_pay_contract now calls xtend_ttl on every write to critical persistent keys.

## Constants

Defined in onchain/contracts/stello_pay_contract/src/storage.rs:

| Constant | Value | Rationale |
|----------|-------|-----------|
| TTL_LIVE_THRESHOLD | 86400 seconds (1 day) | Entries are considered "live" within 1 day of their last write; TTL is bumped to TTL_EXTEND_TO on each access. |
| TTL_EXTEND_TO | 5184000 seconds (60 days) | Each write extends the entry TTL to 60 days from the extension time. |

## Key Coverage

### DataKey (employee payroll data) — all set_* methods in storage.rs

| Storage Key | Description |
|-------------|-------------|
| AgreementEmployeeCount(agreement_id) | Number of employees per agreement |
| AgreementEmployee(agreement_id, index) | Employee addresses |
| EmployeeSalary(agreement_id, index) | Salary per period |
| EmployeeClaimedPeriods(agreement_id, index) | Periods already claimed |
| AgreementActivationTime(agreement_id) | Activation timestamp |
| AgreementPeriodDuration(agreement_id) | Period length in seconds |
| AgreementToken(agreement_id) | Agreement token address |
| AgreementPaidAmount(agreement_id) | Cumulative paid amount |
| AgreementEscrowBalance(agreement_id, token) | Escrow balance |
| ExchangeRate(base, quote) | FX rate info |

### StorageKey (contract-level and agreement data) — all set calls in payroll.rs

| Storage Key | Description |
|-------------|-------------|
| Agreement(agreement_id) | Core agreement struct |
| AgreementEmployees(agreement_id) | Employee info vector |
| MultisigContract | Multisig contract address |
| LargePaymentThreshold | Large payment threshold |
| DisputeResolutionThreshold | Dispute resolution threshold |
| Arbiter | Arbiter address |
| GracePeriodExtensionPolicy | Grace extension policy |
| ExchangeRateAdmin | FX rate admin |
| EmergencyGuardians | Emergency pause guardians |
| PendingPause | Pending pause proposal |
| PauseApprovals | Pause approvals |
| EmergencyPause | Emergency pause state |

## Implementation

All TTL bumps are applied **after** the set() call to the same key, ensuring the write is completed before the TTL is extended. The helper function xtend_data_key_ttl(env, key) centralizes the TTL parameters for DataKey entries; StorageKey entries use the same parameters directly via nv.storage().persistent().extend_ttl(...).

## Test Coverage

The file 	ests/test_state_machine.rs contains three TTL persistence tests:
- 	est_storage_ttl_persistence_across_ledger_advance — verifies payroll agreement, employees, and employee data survive ledger advance
- 	est_storage_ttl_on_cancel_and_grace_period — verifies cancelled agreement and grace period data survive ledger advance
- 	est_storage_ttl_on_escrow_and_salary_updates — verifies escrow balance and claimed periods survive repeated ledger advances

## Security Considerations

- xtend_ttl is only called on write paths, ensuring that the TTL is refreshed when data is modified.
- TTL bumps do not enable storage griefing: the maximum TTL is bounded to 60 days, so adversarial entries cannot be kept alive indefinitely.
- The accounted escrow balance (MilestoneEscrowBalance) uses instance storage which is managed by the Soroban platform; no manual TTL extension is needed for instance keys.
