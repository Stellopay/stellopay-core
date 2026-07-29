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
- **Approver Removal Semantics**: Removing an approver immediately prevents that address from approving or rejecting pending expenses, including expenses for which it was previously designated. Approval decisions already recorded before removal persist in the expense state and remain payable; role changes never retroactively discount or revoke them.
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
7. **Payment released** via `pay_expense`. The contract validates that the `receipt_hash` stored on the expense still matches the original binding in `ReceiptHash` storage, ensuring the payout is bound to the original submitted receipt. Employee gets their portion, Employer is refunded any unapproved surplus automatically. Status enters `Paid` — this is a **terminal state**: the expense status transitions to `Paid` and `escrow_amount` is zeroed **before** any token transfer occurs (checks-effects-interactions). Any subsequent `pay_expense` call for the same expense id is rejected, guaranteeing the expense cannot be paid more than once.

## Settlement Currency Model

Each expense carries its **own** token, chosen by the submitter at
`submit_expense` time and stored on the `Expense` record. Every later step
operates on that same stored token:

- `fund_expense` escrows the expense's `token`.
- `pay_expense` pays the employee, and refunds any unapproved surplus to the
  payer, in the expense's `token`.

There is no global or per-agreement "settlement currency" that an expense is
validated against; the contract is intentionally multi-token, with each expense
fully self-contained in a single currency. Because no step ever references a
second token, a paid-out expense can never mis-convert an amount or transfer a
different token than the one submitted, and distinct expenses in distinct tokens
cannot interfere with one another.

## Receipt Hashing Scheme

- Hash function: `SHA-256`
- Domain separation prefix: `stello.expense.receipt.v1`
- Preimage format: `domain || 0x00 || XDR(receipt_payload_string)`
- Stored value: `receipt_hash: BytesN<32>` per expense
- Replay protection: each `receipt_hash` is unique globally in contract storage (`ReceiptHash(hash) -> expense_id`)

This prevents reimbursing the same receipt payload twice, even when submitted by different users or in separate requests.

### Receipt-Hash Attestation Guarantee

The `receipt_hash` captured at `submit_expense` time is bound to the expense for its entire lifecycle. When `pay_expense` is called, it validates that the `receipt_hash` stored on the expense still matches the original binding recorded in the `ReceiptHash` global storage entry (`ReceiptHash(hash) -> expense_id`). This means:

- The receipt hash cannot be mutated after submission without invalidating the payout.
- Any code path that attempts to alter the stored `receipt_hash` will cause `pay_expense` to reject the payout with a `"Receipt hash binding invalid"` error.
- The attestation guarantee is enforced on-chain, providing cryptographic assurance that the payout corresponds to the original submitted receipt.

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
