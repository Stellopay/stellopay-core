# Tax Withholding Contract

Configurable per-jurisdiction tax withholding with accrual tracking and remittance hooks. Withheld liabilities are clearly separated from employee net pay.

## Overview

The contract tracks two distinct amounts for every pay period:

| Amount | Description |
|--------|-------------|
| **Net pay** | `gross - total_tax` — what the employee receives |
| **Withheld liability** | Accumulated per jurisdiction until remitted to the tax authority |

Separation is enforced at the storage level: accrued balances (`AccruedWithholding`) are only ever transferred to owner-configured treasury addresses via `remit_withholding`.

## Contract Functions

### Configuration (owner only)

| Function | Description |
|----------|-------------|
| `initialize(owner)` | Deploy-time setup; sets the contract owner |
| `set_jurisdiction_rate(caller, jurisdiction, rate_bps)` | Set tax rate (0–10 000 bps) for a jurisdiction |
| `set_jurisdiction_treasury(caller, jurisdiction, treasury)` | Bind a fixed treasury address to a jurisdiction |
| `set_employee_jurisdictions(caller, employee, jurisdictions)` | Assign applicable jurisdictions to an employee |

### Accrual Hook

```
accrue_withholding(caller, employee, gross_amount) → TaxComputation
```

Call once per pay period after the gross amount is finalised. Internally:

1. Computes per-jurisdiction withholding (`floor(gross × rate_bps / 10_000)`).
2. Adds each jurisdiction's share to its `AccruedWithholding` balance.
3. Emits `("withholding_accrued", WithholdingAccruedEvent)`.
4. Returns a `TaxComputation` with `gross_amount`, `total_tax`, `net_amount`, and per-jurisdiction `shares`.

### Remittance Hook

```
remit_withholding(caller, jurisdiction, token) → i128
```

Call periodically (monthly/quarterly) to settle the employer's tax liability. Internally:

1. Reads the treasury address from owner-controlled storage.
2. Reads the `AccruedWithholding` balance for the jurisdiction.
3. Resets the balance to `0` **before** the token transfer (state-before-interaction).
4. Transfers `amount` of `token` from `caller` to `treasury`.
5. Emits `("withholding_remitted", WithholdingRemittedEvent)`.
6. Returns the remitted amount.

The caller must hold at least `accrued_balance` of `token` in their account.

### View Functions

| Function | Returns |
|----------|---------|
| `calculate_withholding(employee, gross_amount)` | `TaxComputation` (no state change) |
| `get_jurisdiction_rate(jurisdiction)` | `Option<u32>` — rate in bps |
| `get_jurisdiction_treasury(jurisdiction)` | `Option<Address>` |
| `get_employee_jurisdictions(employee)` | `Vec<Symbol>` |
| `get_accrued_balance(jurisdiction)` | `i128` — unremitted balance |

## Rounding Policy (NatSpec)

Withholding is computed as:

```
withheld = floor(gross_amount × rate_bps / 10_000)
```

Floor division means any sub-unit fractional remainder stays with the employee in their net pay. Rounding always favours the employee, never the treasury. This prevents systematic over-withholding across many small pay periods.

**Example:** 15% of 10_001 = 1500.15 → withheld = 1500, net = 8501.

## Security Model

| Invariant | Enforcement |
|-----------|-------------|
| Only owner can configure rates, treasuries, and employee jurisdictions | `require_owner` helper checks caller == stored owner before any write |
| Withheld funds cannot be redirected to arbitrary addresses | `remit_withholding` reads treasury from owner-controlled `JurisdictionTreasury` storage — the caller supplies only the token, never the destination |
| No re-entrancy on remittance | Accrued balance is reset to `0` before `token.transfer` is called |
| Overflow-safe arithmetic | All multiplications and additions use `checked_*` and return `ArithmeticError` on overflow |
| Total withholding ≤ gross | Validated after summation; returns `ArithmeticError` if combined rates exceed 100% |

## Usage Example

```rust
// 1. Deploy and initialize
client.initialize(&owner);

// 2. Configure jurisdictions (10% federal, 5% state)
client.set_jurisdiction_rate(&owner, &Symbol::new(&env, "US_FED"), &1000u32);
client.set_jurisdiction_rate(&owner, &Symbol::new(&env, "US_STATE"), &500u32);

// 3. Bind treasury addresses (owner-controlled)
client.set_jurisdiction_treasury(&owner, &Symbol::new(&env, "US_FED"), &fed_treasury);
client.set_jurisdiction_treasury(&owner, &Symbol::new(&env, "US_STATE"), &state_treasury);

// 4. Assign employee to jurisdictions
client.set_employee_jurisdictions(&owner, &employee, &Vec::from_array(&env, [
    Symbol::new(&env, "US_FED"),
    Symbol::new(&env, "US_STATE"),
]));

// 5. Each pay period — accrue withholding
let computation = client.accrue_withholding(&owner, &employee, &10_000i128);
// computation.net_amount = 8_500  (employee take-home)
// computation.total_tax  = 1_500  (accrued liability)

// 6. Monthly/quarterly — remit to tax authorities
client.remit_withholding(&owner, &Symbol::new(&env, "US_FED"), &token);
client.remit_withholding(&owner, &Symbol::new(&env, "US_STATE"), &token);
```

## Error Codes

| Code | Name | Meaning |
|------|------|---------|
| 1 | `Unauthorized` | Caller is not the contract owner |
| 2 | `InvalidRate` | `rate_bps > 10_000` |
| 3 | `NotConfigured` | Employee has no jurisdictions, or a jurisdiction has no rate |
| 4 | `ArithmeticError` | Overflow, underflow, or non-positive gross amount |
| 5 | `TreasuryNotSet` | No treasury configured for the jurisdiction |
| 6 | `NothingToRemit` | Accrued balance is zero |
