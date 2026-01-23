# Milestone-Based Payment System Documentation

## Overview

The milestone-based payment system enables employers to break down project payments into discrete deliverables. This provides better cash flow management and risk mitigation by ensuring payments are released only when specific project milestones are completed and approved.

## Architecture

### Data Structures

#### Milestone

```rust
pub struct Milestone {
    pub id: u32,           // Unique identifier
    pub amount: i128,      // Payment amount in token units
    pub approved: bool,    // Employer approval status
    pub claimed: bool,     // Contributor claim status
}
```

#### PaymentType

```rust
pub enum PaymentType {
    LumpSum,           // Single payment
    MilestoneBased,    // Multiple milestone payments
}
```

#### AgreementStatus

```rust
pub enum AgreementStatus {
    Created,    // Milestones can be added
    Active,     // Agreement funded and running
    Completed,  // All milestones claimed
    Cancelled,  // Agreement terminated
}
```

## Workflow

### 1. Agreement Creation

**Function**: `create_milestone_agreement(env, employer, contributor, token) -> u128`

**Process**:

- Employer creates a new milestone-based agreement
- System generates unique agreement ID
- Agreement starts in `Created` status
- Returns agreement ID for reference

**Security**:

- Requires employer authorization
- Employer address is recorded for access control

**Example**:

```rust
let agreement_id = client.create_milestone_agreement(
    &employer_address,
    &contributor_address,
    &token_address
);
```

### 2. Adding Milestones

**Function**: `add_milestone(env, agreement_id, amount)`

**Process**:

- Employer defines project milestones with payment amounts
- Each milestone receives sequential ID (1, 2, 3, ...)
- Total agreement amount is calculated automatically
- Milestones start as unapproved and unclaimed

**Access Control**:

- Only the employer can add milestones
- Agreement must be in `Created` status
- Amount must be positive (> 0)

**Example**:

```rust
// Add project setup milestone
client.add_milestone(&agreement_id, &1000);

// Add development milestone
client.add_milestone(&agreement_id, &3000);

// Add final delivery milestone
client.add_milestone(&agreement_id, &1500);
```

### 3. Milestone Approval

**Function**: `approve_milestone(env, agreement_id, milestone_id)`

**Process**:

- Employer reviews contributor's work
- If satisfactory, employer approves the milestone
- Approval enables the contributor to claim payment
- Emits `MilestoneApproved` event

**Validation**:

- Only employer can approve
- Milestone ID must be valid (1 to milestone_count)
- Cannot approve already approved milestones
- Cannot approve non-existent milestones

**Example**:

```rust
// Approve milestone #1 after reviewing deliverable
client.approve_milestone(&agreement_id, &1);
```

### 4. Milestone Claiming

**Function**: `claim_milestone(env, agreement_id, milestone_id)`

**Process**:

- Contributor claims payment for approved milestone
- Funds transfer from escrow to contributor
- Milestone marked as claimed
- Agreement auto-completes when all milestones claimed
- Emits `MilestoneClaimed` event

**Security Checks**:

- Only contributor can claim
- Milestone must be approved first
- Cannot claim already claimed milestones
- Cannot claim unapproved milestones

**Example**:

```rust
// Claim approved milestone payment
client.claim_milestone(&agreement_id, &1);
```

### 5. Querying Milestone Information

**Get Milestone Count**:

```rust
let count = client.get_milestone_count(&agreement_id);
```

**Get Milestone Details**:

```rust
let milestone = client.get_milestone(&agreement_id, &1);
match milestone {
    Some(m) => {
        println!("Amount: {}", m.amount);
        println!("Approved: {}", m.approved);
        println!("Claimed: {}", m.claimed);
    },
    None => println!("Milestone not found")
}
```

## State Transitions

```
Agreement Creation
    ↓
Created (add milestones)
    ↓
Active (approve & claim milestones)
    ↓
Completed (all milestones claimed)
```

### Milestone States

```
Created → Approved → Claimed
   ↓         ↓         ✓
(added)  (employer)  (contributor)
          approves    receives payment
```

## Events

### MilestoneAdded

Emitted when employer adds a new milestone.

**Fields**:

- `agreement_id`: Agreement identifier
- `milestone_id`: New milestone identifier
- `amount`: Payment amount for milestone

### MilestoneApproved

Emitted when employer approves a milestone.

**Fields**:

- `agreement_id`: Agreement identifier
- `milestone_id`: Approved milestone identifier

### MilestoneClaimed

Emitted when contributor claims milestone payment.

**Fields**:

- `agreement_id`: Agreement identifier
- `milestone_id`: Claimed milestone identifier
- `amount`: Payment amount transferred
- `to`: Recipient address (contributor)
