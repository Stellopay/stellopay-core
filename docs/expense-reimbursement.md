# Expense Reimbursement Contract

## Overview

The Expense Reimbursement Contract provides a secure, auditable system for managing employee expense reimbursements with approval workflows, partial approvals, exact escrow guarantees, and receipt verification on the Stellar blockchain via Soroban.

## Features

- **Expense Submission**: Employees submit expenses with receipt documentation (via off-chain hashes).
- **Escrowing**: Payer (Employer) locks the funds into the contract, maintaining strict guarantees over balances prior to approval.
- **Approval Workflow**: Designated approvers review and approve/reject expenses. Includes support for partial approvals.
- **Receipt Verification**: Hash-based receipt tracking for audit trails. NatSpec compliant hashes.
- **Role-Based Access**: Owner manages approvers, approvers handle approvals. Self-approval is explicitly disabled.
- **Status Tracking**: Complete lifecycle management (Pending → Approved/Rejected/Cancelled → Paid).
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
6. **Payment released** via `pay_expense`. Employee gets their portion, Employer is refunded any unapproved surplus automatically. Status enters `Paid`.

## Gas Optimization and Edge Cases

- **Duplicates**: Overlapping IDs are sidestepped by incremental counters. Same claim hashes are allowed uniquely scoped by IDs.
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
