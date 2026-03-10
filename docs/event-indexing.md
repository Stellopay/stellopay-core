# Event Indexing and Subgraph Documentation

This document provides a comprehensive guide to indexing events emitted by the Stellopay Soroban contract. Indexing these events is crucial for building real-time dashboards, transaction history, and detailed analytics.

## Overview

The Stellopay contract uses the Soroban Event system to broadcast state changes. Events are emitted with specific structures that include identifying IDs, involved addresses, and financial amounts.

### Key Indexing Entities
- **Agreement**: The central entity (Escrow, Payroll, or Milestone).
- **Milestone**: Specific tasks within a Milestone agreement.
- **Employee**: Human-readable mapping for addresses within Payroll.
- **Payment**: Financial movement records.

---

## Event Catalog

### Agreement Lifecycle
| Event | Trigger | Payload Summary |
|---|---|---|
| `AgreementCreated` | `create_xxx_agreement` | `agreement_id`, `employer`, `mode` |
| `AgreementActivated` | `activate_agreement` | `agreement_id` |
| `AgreementPaused` | `pause_agreement` | `agreement_id` |
| `AgreementResumed` | `resume_agreement` | `agreement_id` |
| `AgreementCancelled` | `cancel_agreement` | `agreement_id` |
| `GracePeriodFinalized` | `finalize_grace_period` | `agreement_id` |

### Milestone Management
| Event | Trigger | Payload Summary |
|---|---|---|
| `MilestoneAdded` | `add_milestone` | `agreement_id`, `milestone_id`, `amount` |
| `MilestoneApproved` | `approve_milestone` | `agreement_id`, `milestone_id` |
| `MilestoneClaimed` | `claim_milestone` | `agreement_id`, `milestone_id`, `amount`, `to` |
| `BatchMilestoneClaimed` | `batch_claim_milestones` | `agreement_id`, `total_claimed`, `successful_claims` |

### Payroll and Payments
| Event | Trigger | Payload Summary |
|---|---|---|
| `EmployeeAdded` | `add_employee` | `agreement_id`, `employee`, `salary_per_period` |
| `PayrollClaimed` | `claim_payroll(_in_token)` | `agreement_id`, `employee`, `amount` |
| `BatchPayrollClaimed` | `batch_claim_payroll` | `agreement_id`, `total_claimed`, `successful_count` |
| `PaymentSent` | Token transfer out | `agreement_id`, `from`, `to`, `amount`, `token` |
| `PaymentReceived` | Token transfer in | `agreement_id`, `to`, `amount`, `token` |

### Disputes
| Event | Trigger | Payload Summary |
|---|---|---|
| `ArbiterSet` | `initialize` | `arbiter` address |
| `DisputeRaised` | `raise_dispute` | `agreement_id` |
| `DisputeResolved` | `resolve_dispute` | `agreement_id`, `pay_contributor`, `refund_employer` |

---

## Best Practices for Indexing

### 1. Idempotency and Replay Safety
- **Event Uniqueness**: Use the pair of `(LedgerSequence, EventId)` as a unique identifier for each event to prevent duplicate processing.
- **Atomic Updates**: When updating your database, ensure that the state change and the recorded "last indexed ledger" are updated in a single transaction.

### 2. Handling Numeric Types
- **i128 and u128**: Soroban uses 128-bit integers for token amounts and IDs. Many databases (like PostgreSQL) require `Numeric` or `Decimal` types to store these without precision loss.
- **JSON Precision**: When consuming JSON schemas, ensure your parser does not truncate large integers to 64-bit floats.

### 3. Re-org Handling
- If you are not using a managed subgraph service, your indexer should be able to "roll back" to a known good state if a ledger re-organization occurs. Wait for at least 1-3 ledger confirmations for high-value data.

### 4. Schema and Examples
- Refer to [events-schema.json](./events-schema.json) for the exact structure of each event payload.
- See the [Example Indexer Script](./example-indexer.js) for a Node.js implementation using the Soroban RPC.

---

## Example Query (Conceptual Subgraph)

To fetch all completed milestones for a specific agreement:

```graphql
{
  milestoneClaimeds(where: { agreement_id: "123..." }) {
    milestone_id
    amount
    to
    blockNumber
    timestamp
  }
}
```

To monitor total payout for an employer:

```graphql
{
  paymentSents(where: { from: "雇主地址" }) {
    amount
    token
    agreement_id
  }
}
```
