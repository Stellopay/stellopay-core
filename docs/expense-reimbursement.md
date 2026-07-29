# Expense Reimbursement Contract

## Overview

The Expense Reimbursement Contract provides a secure, auditable system for managing employee expense reimbursements with approval workflows, partial approvals, exact escrow guarantees, and receipt verification on the Stellar blockchain via Soroban.

## Features

- **Expense Submission**: Employees submit expenses with receipt payloads that are hashed on-chain.
- **Escrowing**: Payer (Employer) locks the funds into the contract, maintaining strict guarantees over balances prior to approval.
- **Approval Workflow**: Designated approvers review and approve/reject expenses. Includes support for partial approvals.
- **Receipt Verification**: Domain-separated SHA-256 receipt hashing with replay protection across all requests.
- **Role-Based Access**: Owner manages approvers, approvers handle approvals. Self-approval is explicitly disabled.
- **Status Tracking**: Complete lifecycle management (Pending → Approved/Rejected/Cancelled → Paid). `Paid` is a **terminal** state: once set, `pay_expense` cannot be called again for the same expense id, enforcing a double-payment guard via checks-effects-interactions.
- **Refund Guarantees**: Escrowed funds dynamically return to the originator on rejection, cancellation, or partial approval surpluses.
- **Event Emission**: All state changes emit events for transparency.

## Architecture

### Core Components

1. **Expense Structure**: Contains all expense details including submitter, approver, amount, escrow amount, exact payer, receipt hash, and status.
2. **Approval System**: Role-based approver management with designated approvers per expense and partial approval limits.
3. **Escrow Guarantee**: The contract natively holds employer tokens during the `Pending` state.
4. **Payment Processing**: Secure token transfers from contract balance to employee after approval. Excess refunded.

### Security Model

- **Owner Controls**: Only owner can add/remove approvers.
- **Approver Authorization**: Only designated approvers can approve/reject specific expenses. Approvers cannot approve their own expenses.
- **Submitter Rights**: Only submitters can cancel their pending expenses.
- **Refund Assurances**: Escrows naturally return to the `payer` recorded upon funding, avoiding owner confiscation.
- **Authentication**: All sensitive operations require strict caller authentication via Soroban `require_auth`.

## Escrow and Reimbursement Workflow

1. **Employee uploads receipt** to secure storage to get a document URL. Computes `SHA-256(receipt_document)`.
2. **Employee submits expense** using the contract with the token amount and hash. Expense enters `Pending`.
3. **Employer funds expense** via `fund_expense`. Tokens are transferred to the contract's escrow.
4. **Approver reviews** and validates `SHA256(retrieved_document) == stored_hash`.
5. **Approver triggers approval** using `approve_expense`. (Can approve partially). Status enters `Approved`.
6. **Optional audit linkage**: if `audit_logger` is configured, approval writes `append_log(actor=approver, action="expense_approved", subject=submitter, amount=approved_amount)` and stores returned `audit_log_id`.
7. **Payment released** via `pay_expense`. Employee gets their portion, Employer is refunded any unapproved surplus automatically. Status enters `Paid` — this is a **terminal state**: the expense status transitions to `Paid` and `escrow_amount` is zeroed **before** any token transfer occurs (checks-effects-interactions). Any subsequent `pay_expense` call for the same expense id is rejected, guaranteeing the expense cannot be paid more than once.

## Audit Logging

The expense reimbursement contract supports optional audit logging via integration with an external audit logger contract. This provides traceability for approval decisions while maintaining privacy for other operations.

### Audit Logger Configuration

The contract owner can configure an audit logger using `set_audit_logger(owner, audit_logger_address)`. Once configured:

- Only the `approve_expense` operation creates audit log entries
- Submit, fund, reject, cancel, and pay operations do not create audit entries
- The audit log ID is stored in `expense.audit_log_id` for traceability

### Audit Entry Schema

When an expense is approved, the contract appends a single audit log entry with the following schema:

| Field | Type | Description |
|-------|------|-------------|
| `action` | Symbol | `"expense_approved"` |
| `actor` | Address | The approver's address who performed the approval |
| `subject` | Option\<Address\> | The submitter's address (the expense beneficiary) |
| `amount` | Option\<i128\> | The approved amount (may be less than the requested amount for partial approvals) |

### Audit Invariants

- **Exactly one entry per approval**: Each call to `approve_expense` appends exactly one audit log entry
- **No duplicate entries**: The audit log ID is stored in the expense and cannot be modified
- **Amount accuracy**: The audit entry amount reflects the approved amount, not the original requested amount
- **Sequential IDs**: Multiple approvals generate audit entries with sequential IDs
- **No entries for other operations**: Submit, fund, reject, cancel, and pay operations do not create audit entries

### Audit-Linkage Completeness

The contract includes comprehensive tests to verify audit log completeness across the full expense lifecycle:

- **Submit → Approve → Pay**: Verifies exactly one audit entry is created for approval, with correct actor, subject, and amount
- **Submit → Reject**: Verifies no audit entries are created for the rejection lifecycle
- **Partial Approval**: Verifies the audit entry contains the approved amount (not the requested amount)
- **Multiple Expenses**: Verifies each approval generates a unique audit entry with sequential IDs

See `onchain/contracts/expense_reimbursement/tests/test_expense.rs` for the complete test suite.

## Receipt Hashing Scheme

- Hash function: `SHA-256`
- Domain separation prefix: `stello.expense.receipt.v1`
- Preimage format: `domain || 0x00 || XDR(receipt_payload_string)`
- Stored value: `receipt_hash: BytesN<32>` per expense
- Replay protection: each `receipt_hash` is unique globally in contract storage (`ReceiptHash(hash) -> expense_id`)

This prevents reimbursing the same receipt payload twice, even when submitted by different users or in separate requests.

## Privacy and Security Notes

- Only the 32-byte commitment is stored on-chain; raw receipts should remain in off-chain systems.
- Use high-entropy receipt payloads (e.g., canonical document digest or immutable URI+digest tuple) to reduce metadata leakage.
- Collision resistance relies on SHA-256 security; practical second-preimage and collision attacks are infeasible for this use case.
- Empty payloads are rejected.

## Payload Size and Cost Limits

- `MAX_RECEIPT_PAYLOAD_BYTES = 4096`
- Oversized payloads are rejected to cap hashing cost and avoid unbounded compute usage.
- Very short payloads are valid but can increase accidental replay collisions; use canonical, sufficiently specific receipt content.

## Double-Payment Guard

`pay_expense` implements a strict checks-effects-interactions pattern to prevent the same approved expense from being paid twice:

1. **Check**: Verifies the expense is in `Approved` status.
2. **Effect**: Atomically sets `status = Paid` and `escrow_amount = 0` in storage **before** any token transfer.
3. **Interaction**: Only then performs token transfers (payout to submitter, surplus refund to payer).

Once `Paid`, the status check in step 1 rejects any subsequent call. This guard holds for both full and partial approval scenarios.

## Gas Optimization and Edge Cases

- **Receipt Replay**: Duplicate receipt payloads now fail fast with `Receipt already reimbursed`.
- **Zero Values**: Validations assert all fund operations involve strictly positive integers.

## Usage Commands (Integration Examples)

```rust
// Employer Escrowing Phase
client.fund_expense(&employer_account, &expense_id, &15000); // 150 USDC tokens

// Manager reviews and partially approves 125 out of 150
client.approve_expense(&manager, &expense_id, &12500);

// Disburse (Anyone can push this operation)
client.pay_expense(&expense_id);
// At this stage: Employer gets back 2500, Employee receives 12500 natively.
```

## Testing

The contract includes comprehensive tests covering:
- Initialization and configuration
- Funding and escrow tracking
- Approval/rejection workflows with partial overrides
- Native refund workflows
- Cargo coverage maintains minimum 95% threshold.

Run tests:
```bash
cd onchain/contracts/expense_reimbursement
cargo test
```

## License
This contract is part of the StelloPay Core system.
