## Slashing & Penalty Contract

The **SlashingPenaltyContract** provides a reusable, auditable mechanism for
enforcing penalties related to **late payments**, **breached agreements**, and
**policy violations** across the Stellopay protocol.

It is intentionally decoupled from the core payroll contract so that:

- Different products (payroll, escrow, disputes) can share a single slashing
  policy engine.
- Off-chain governance can evolve policies without redeploying the core logic.

---

### Core Concepts

- **Penalty Policy (`PenaltyPolicy`)**
  - Scoped optionally to a specific agreement id (`Option<u128>`).
  - Defines:
    - `max_penalty_bps` — percentage cap of the current locked amount
      (10000 = 100%).
    - `absolute_cap` — optional hard cap in token units.
    - `description` — human-readable identifier (tier, severity, etc).
    - `is_active` — can be toggled without deleting history.

- **Penalty Reason (`PenaltyReason`)**
  - High-level category for a penalty:
    - `LatePayment`
    - `BreachOfAgreement`
    - `PolicyViolation`
    - `Custom(u32)` for protocol-specific codes.

- **Slashing Record (`SlashingRecord`)**
  - Immutable audit trail for every penalty:
    - `policy_id`, `agreement_id`
    - `offender`, `beneficiary`, `token`, `amount`
    - `reason`, `timestamp`

---

### Invariants & Caps

For every successful `slash` call:

- **Non-negative amounts**
  - `amount > 0`
  - `current_locked_amount > 0`
- **Agreement scope (optional)**
  - If `policy.agreement_id` is `Some(id)`, then the caller-supplied
    `agreement_id` must be `Some(id)` as well.
- **Upper bounds**
  - Let:
    - `L = current_locked_amount`
    - `B = policy.max_penalty_bps`
    - `C = policy.absolute_cap` (optional)
  - Then:
    - `amount <= L`
    - `amount <= (B * L) / 10000`
    - If `C` is set, `amount <= C`

If any check fails, the operation reverts with `PenaltyError::CapExceeded` or
`PenaltyError::InvalidConfig`.

---

### Integration with Dispute & Escrow Flows

The contract is designed to be called from:

- **Dispute resolution flows** (e.g. arbiter contracts) when an arbiter decides
  a portion of locked funds should be penalized and redistributed.
- **Escrow flows** when service-level policies (e.g. repeated late payments)
  accumulate penalties.

Typical pattern:

1. **Initialization**
   - Protocol owner calls `initialize(owner)` and optionally sets an
     `operator` (e.g. dispute contract) via `set_operator`.
2. **Policy setup**
   - Owner or operator creates one or more policies using `create_policy`,
     optionally scoped to a specific agreement id.
3. **During disputes/escrow resolution**
   - The integration flow computes:
     - `current_locked_amount` for an agreement.
     - Intended penalty `amount`.
   - It then calls `slash` with:
     - `caller` = owner or delegated operator.
     - `agreement_id`, `amount`, `current_locked_amount`, and `reason`.
   - The contract:
     - Validates caps.
     - Optionally transfers tokens from its own escrow balance (if funded).
     - Persists a `SlashingRecord`.
     - Emits a `slash` event for downstream monitoring.

This design keeps **cap enforcement and audit trail** on-chain, while allowing
different products to choose where funds are physically held (core contract,
dedicated penalty pool, or this contract itself).  

