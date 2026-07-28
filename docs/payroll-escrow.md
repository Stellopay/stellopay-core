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

### 6. Escrow Conservation Invariant
For each `agreement_id`, the contract maintains:

```text
total_funded == total_released + total_refunded + remaining_balance
```

Property-based fuzz tests in `onchain/contracts/payroll_escrow/tests/fuzz/test_fuzzing.rs` generate randomized fund / release / refund sequences and assert this invariant after every successful step. Integration tests in `onchain/integration_tests` verify the same property across a multi-contract lifecycle (fund → partial release → refund).

### 7. Cumulative Release Cap

Repeated calls to `release` can never, in aggregate, release more than the originally funded amount for a given `agreement_id`. This is enforced by the `assert!(balance >= amount, "Insufficient balance")` guard inside `release`, which compares the live `AgreementBalance` counter against the requested amount before any token transfer occurs.

**Key properties of this invariant:**

- `get_agreement_balance(agreement_id)` equals `funded - cumulative_released` after every individual `release` call.
- A release that would push the cumulative total past `funded` fails immediately with `"Insufficient balance"`.
- On failure, **no partial or truncated transfer occurs** — the requested amount is either transferred in full or nothing moves.
- Contract token custody (actual on-chain balance) and the internal `AgreementBalance` counter stay in lock-step at all times.

**Test coverage** — three dedicated tests in `src/tests/test_escrow.rs`:

| Test | What it verifies |
|------|-----------------|
| `test_cumulative_release_cap_many_small_releases_never_exceed_funded` | 10 micro-releases of 100 each from a 1 000-funded agreement; cap invariant and counter checked after every step |
| `test_cumulative_release_cap_over_funded_amount_errors_not_truncates` | After a 300/500 partial release, an attempt to release 201 (total would be 501 > 500) errors; no partial transfer; subsequent valid 200 release succeeds |
| `test_cumulative_release_internal_balance_tracks_funded_minus_released_after_every_call` | Variable-step sequence [50, 75, 25, 100, 200, 50]; `get_agreement_balance == funded - released` asserted after every individual call |

## Interaction Flow

1. **Initialization**: Admin sets the token address and the Manager contract address.
2. **Funding**: Employer calls `fund_agreement`. The contract transfers tokens from the employer and records the balance and employer address for the given `agreement_id`.
3. **Release**: The Manager contract calls `release` to send a specific amount to a recipient (e.g., an employee).
4. **Refund**: If an agreement is cancelled or completed with a surplus, the Manager calls `refund_remaining` to return all leftover funds to the employer.

## Security Considerations

- **Authentication**: All state-changing functions require `require_auth()` for the appropriate caller.
- **Token Transfers**: The contract uses the standard Soroban Token interface. If a transfer fails (e.g., due to a frozen balance or insufficient contract funds), the entire transaction reverts.
- **Storage**: Most data is stored in `persistent` storage to ensure it remains available throughout the agreement's lifecycle.

## Storage Key Layout

### AgreementBalance

The contract uses `StorageKey::AgreementBalance(u128)` to store per-agreement escrowed balances in persistent storage.

**Key Derivation:**
- `AgreementBalance(0)` → unique storage slot for agreement ID 0
- `AgreementBalance(1)` → unique storage slot for agreement ID 1
- `AgreementBalance(u128::MAX)` → unique storage slot for maximum agreement ID

**Security Invariant:**
Two distinct agreement IDs must never resolve to the same storage slot. The Soroban SDK's `#[contracttype]` derive macro ensures that distinct `u128` values always resolve to distinct storage keys, preventing cross-agreement balance collisions.

**Regression Tests:**
The test suite includes regression tests that verify this invariant for:
- Adjacent agreement IDs (e.g., 1000, 1001, 1002)
- Edge values (0, 1, u128::MAX)
- Structurally similar IDs (e.g., 12345, 12346, 12347)
- Release and refund operations to ensure one agreement's operations never mutate another's balance

A key-derivation bug here would let one agreement's funding silently overwrite another's, enabling fund theft or loss.

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

