# Payroll Escrow Documentation

The `payroll_escrow` contract serves as a secure, per-agreement token vault. It is designed to be managed by a higher-level contract (the "Manager"), which dictates when funds should be released to participants or refunded to the employer.

## Roles

| Role | Responsibility |
|------|----------------|
| **Admin** | Can initialize the contract and perform upgrades. |
| **Manager** | The only address authorized to call `release` and `refund_remaining`. Typically a payroll or agreement management contract. |
| **Employer** | The address that funds an agreement. Receives any remaining balance upon refund. |

## Core Invariants

### 1. Only Manager (Access Control)
Only the designated `Manager` address can authorize the movement of funds out of the escrow contract. Any attempt by other addresses (including the Admin or the Employer directly) to call `release` or `refund_remaining` will fail.

### 2. Per-Agreement Balance Isolation
Funds deposited for `agreement_id: A` cannot be used to satisfy a release request for `agreement_id: B`. The contract maintains strict internal accounting to prevent cross-agreement fund mixing.

### 3. Employer Consistency
Once an agreement ID is associated with an employer address via `fund_agreement`, that association is immutable. Subsequent funding for the same agreement ID must come from the same employer address, preventing different entities from accidentally or maliciously interfering with an existing agreement's lifecycle.

### 4. No Balance Drift
All fund movements are protected by checked arithmetic. The `AgreementBalance` is reduced by the exact amount transferred out, ensuring that the contract never attempts to send more tokens than it actually holds for a specific agreement.

### 5. Atomic Refunds
The `refund_remaining` operation is atomic: it transfers the entire remaining balance of an agreement back to the registered employer and resets the internal balance to zero in a single transaction.

## Interaction Flow

1. **Initialization**: Admin sets the token address and the Manager contract address.
2. **Funding**: Employer calls `fund_agreement`. The contract transfers tokens from the employer and records the balance and employer address for the given `agreement_id`.
3. **Release**: The Manager contract calls `release` to send a specific amount to a recipient (e.g., an employee).
4. **Refund**: If an agreement is cancelled or completed with a surplus, the Manager calls `refund_remaining` to return all leftover funds to the employer.

## Security Considerations

- **Authentication**: All state-changing functions require `require_auth()` for the appropriate caller.
- **Token Transfers**: The contract uses the standard Soroban Token interface. If a transfer fails (e.g., due to a frozen balance or insufficient contract funds), the entire transaction reverts.
- **Storage**: Most data is stored in `persistent` storage to ensure it remains available throughout the agreement's lifecycle.

---

## Milestone Agreement Funding (`fund_milestone_agreement`)

Milestone agreements are created via `create_milestone_agreement` but require an explicit, employer-authenticated deposit before any milestone can be approved or claimed. The `fund_milestone_agreement` entrypoint provides this funded path.

### Motivation

Prior to this entrypoint, the only way to satisfy the `approve_milestone` / `claim_milestone` balance invariant was to transfer tokens to the contract address out-of-band. Such transfers are undiscoverable to external observers and cannot be attributed to a specific agreement, making auditing impossible.

### Signature

```rust
fund_milestone_agreement(env: Env, agreement_id: u128, from: Address, amount: i128)
```

| Parameter      | Description |
|----------------|-------------|
| `agreement_id` | ID of the milestone agreement to fund. |
| `from`         | Employer address. Must match the employer stored for `agreement_id`. |
| `amount`       | Strictly positive token amount to deposit. |

### Accounted Escrow Balance

`fund_milestone_agreement` does **not** rely on the raw `token.balance()` of the contract address. Instead it maintains a per-agreement accounted balance under `MilestoneKey::MilestoneEscrowBalance(agreement_id)` in instance storage.

- **Funded**: balance incremented by `amount` (Checks-Effects-Interactions — state written before `token.transfer` call).
- **Approve**: `approve_milestone` compares the accounted balance against `sum_unclaimed_milestones`.
- **Claim**: `claim_milestone` / `batch_claim_milestones` decrement the accounted balance by the claimed amount after marking the milestone claimed and before executing the token transfer.

This design means that any unrelated token deposits into the contract address are ignored by the milestone accounting layer and cannot be used to satisfy funding requirements.

### Validation Rules

| Condition | Error message |
|-----------|---------------|
| `agreement_id` not a known milestone agreement | "Agreement not found" |
| `from` ≠ stored employer | "Unauthorized: only the employer can fund a milestone agreement" |
| `amount <= 0` | "Amount must be positive" |
| Agreement status is `Cancelled` | "Cannot fund a Cancelled agreement" |
| Agreement status is `Completed` | "Cannot fund a Completed agreement" |
| `current_balance + amount` overflows `i128` | "Escrow balance overflow" |

### Event

`MilestoneFundedEvent` is emitted on every successful call:

```json
{
  "agreement_id": "<u128>",
  "from": "<Address>",
  "amount": "<i128>",
  "total_escrow_balance": "<i128>"
}
```

`total_escrow_balance` is the **new** accounted balance for the agreement after this deposit.

### Complexity

- **Time**: O(1) — one instance-storage read, one write, one token transfer.
- **Space**: O(1) — one additional instance-storage slot per milestone agreement (`MilestoneEscrowBalance`).

