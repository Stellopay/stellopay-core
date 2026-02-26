# Expense Reimbursement Contract

## Overview

The Expense Reimbursement Contract provides a secure, auditable system for managing employee expense reimbursements with approval workflows and receipt verification on the Stellar blockchain.

## Features

- **Expense Submission**: Employees submit expenses with receipt documentation
- **Approval Workflow**: Designated approvers review and approve/reject expenses
- **Receipt Verification**: Hash-based receipt tracking for audit trails
- **Role-Based Access**: Owner manages approvers, approvers handle approvals
- **Status Tracking**: Complete lifecycle management (Pending → Approved/Rejected → Paid/Cancelled)
- **Event Emission**: All state changes emit events for transparency

## Architecture

### Core Components

1. **Expense Structure**: Contains all expense details including submitter, approver, amount, receipt hash, and status
2. **Approval System**: Role-based approver management with designated approvers per expense
3. **Payment Processing**: Secure token transfers after approval
4. **State Management**: Persistent storage with unique expense IDs

### Security Model

- **Owner Controls**: Only owner can add/remove approvers
- **Approver Authorization**: Only designated approvers can approve/reject specific expenses
- **Submitter Rights**: Only submitters can cancel their pending expenses
- **Status Validation**: State transitions enforce valid workflow progression
- **Authentication**: All sensitive operations require caller authentication

## Usage

### Initialization

```rust
// Initialize contract with owner
contract.initialize(&owner);

// Add approvers
contract.add_approver(&approver_address);
```

### Submitting Expenses

```rust
let expense_id = contract.submit_expense(
    &submitter,           // Employee submitting expense
    &approver,            // Designated approver
    &token_address,       // Token for reimbursement
    &amount,              // Reimbursement amount
    &receipt_hash,        // Hash of receipt document
    &description          // Expense description
);
```

**Requirements**:
- Amount must be positive
- Approver must have approver role
- Receipt hash should be cryptographic hash (e.g., SHA-256) of receipt document

### Approval Workflow

```rust
// Approve expense
contract.approve_expense(&approver, &expense_id);

// Or reject expense
contract.reject_expense(&approver, &expense_id);
```

**Requirements**:
- Caller must be the designated approver for the expense
- Expense must be in Pending status

### Payment Processing

```rust
// Pay approved expense
contract.pay_expense(&payer, &expense_id);
```

**Requirements**:
- Expense must be in Approved status
- Payer must have sufficient token balance
- Transfers tokens from payer to submitter

### Cancellation

```rust
// Cancel pending expense
contract.cancel_expense(&submitter, &expense_id);
```

**Requirements**:
- Caller must be the original submitter
- Expense must be in Pending status

### Querying

```rust
// Get expense details
let expense = contract.get_expense(&expense_id);

// Check approver status
let is_approver = contract.is_approver(&address);
```

## Expense Lifecycle

```
┌─────────────┐
│   PENDING   │ ← Initial state after submission
└──────┬──────┘
       │
       ├─────────────┐
       │             │
       ▼             ▼
┌──────────┐   ┌──────────┐
│ APPROVED │   │ REJECTED │
└────┬─────┘   └──────────┘
     │
     ▼
┌─────────┐
│  PAID   │ ← Final state
└─────────┘

Alternative path:
PENDING → CANCELLED (by submitter)
```

## Events

### ExpenseSubmittedEvent
Emitted when an expense is submitted.
- `expense_id`: Unique expense identifier
- `submitter`: Employee address
- `approver`: Designated approver address
- `amount`: Reimbursement amount
- `receipt_hash`: Receipt document hash

### ExpenseApprovedEvent
Emitted when an expense is approved.
- `expense_id`: Expense identifier
- `approver`: Approver address

### ExpenseRejectedEvent
Emitted when an expense is rejected.
- `expense_id`: Expense identifier
- `approver`: Approver address

### ExpensePaidEvent
Emitted when an expense is paid.
- `expense_id`: Expense identifier
- `submitter`: Recipient address
- `amount`: Amount paid

### ExpenseCancelledEvent
Emitted when an expense is cancelled.
- `expense_id`: Expense identifier
- `submitter`: Submitter address

## Receipt Verification

The contract stores receipt hashes for verification purposes. Best practices:

1. **Hash Generation**: Use SHA-256 or similar cryptographic hash of receipt document
2. **Off-Chain Storage**: Store actual receipt documents off-chain (IPFS, cloud storage)
3. **Verification**: Compare stored hash with hash of provided document to verify authenticity
4. **Immutability**: Receipt hashes cannot be modified after submission

Example workflow:
```
1. Employee uploads receipt to secure storage → gets document URL
2. Employee computes hash: hash = SHA256(receipt_document)
3. Employee submits expense with hash
4. Approver retrieves document from storage
5. Approver verifies: SHA256(retrieved_document) == stored_hash
6. Approver approves/rejects based on verification
```

## Security Considerations

### Access Control
- **Owner Privileges**: Limited to approver management only
- **Approver Scope**: Can only approve/reject expenses assigned to them
- **Submitter Rights**: Can only cancel their own pending expenses

### State Validation
- All state transitions validate current status
- Prevents double-payment or invalid workflow progression
- Immutable expense details after submission

### Token Safety
- Uses Soroban token interface for secure transfers
- Requires explicit payer authorization
- No automatic fund withdrawals

### Audit Trail
- All operations emit events
- Complete expense history preserved on-chain
- Receipt hashes provide verification capability

## Testing

The contract includes comprehensive tests covering:

- Initialization and configuration
- Approver management
- Expense submission with validation
- Approval/rejection workflows
- Payment processing
- Cancellation scenarios
- Edge cases and error conditions
- Multi-expense scenarios
- Full end-to-end workflows

Run tests:
```bash
cd onchain/contracts/expense_reimbursement
cargo test
```

## Integration Example

```rust
use expense_reimbursement::{ExpenseReimbursementContractClient, ExpenseStatus};
use soroban_sdk::{Address, Env, String};

// Initialize
let client = ExpenseReimbursementContractClient::new(&env, &contract_id);
client.initialize(&owner);
client.add_approver(&manager);

// Employee submits expense
let expense_id = client.submit_expense(
    &employee,
    &manager,
    &usdc_token,
    &15000, // $150.00 in cents
    &String::from_str(&env, "a3f5b8c9d2e1..."), // SHA-256 hash
    &String::from_str(&env, "Client dinner meeting")
);

// Manager approves
client.approve_expense(&manager, &expense_id);

// Finance pays
client.pay_expense(&finance_account, &expense_id);

// Verify completion
let expense = client.get_expense(&expense_id).unwrap();
assert_eq!(expense.status, ExpenseStatus::Paid);
```

## Gas Optimization

The contract is optimized for efficiency:
- Minimal storage operations
- Direct state access without unnecessary copies
- Efficient event emission
- No redundant validations

## Future Enhancements

Potential improvements for future versions:
- Multi-level approval workflows
- Spending limits per approver
- Category-based expense tracking
- Automatic payment scheduling
- Batch payment processing
- Integration with accounting systems

## License

This contract is part of the StelloPay Core system.
