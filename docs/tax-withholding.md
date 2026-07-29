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
| `set_jurisdiction_rate(caller, jurisdiction, rate_bps, version)` | Set tax rate (0–10 000 bps) for a jurisdiction in a specific ruleset version |
| `set_jurisdiction_treasury(caller, jurisdiction, treasury)` | Bind a fixed treasury address to a jurisdiction |
| `set_employee_jurisdictions(caller, employee, jurisdictions)` | Assign applicable jurisdictions to an employee (forward-only; historical accruals remain intact) |
| `publish_ruleset_version(caller, description)` | Publishes a new ruleset version |
| `lock_ruleset_version(caller, version)` | Freezes a specific ruleset version against further rate edits (scoped strictly to target version) |
| `set_active_ruleset_version(caller, version)` | Sets active default ruleset version for new employees |
| `deprecate_ruleset_version(caller, version)` | Deprecates a version so it cannot be activated or migrated to |
| `migrate_employee_to_version(caller, employee, version)` | Pin an employee to a specific active/historical ruleset version |

> [!NOTE]
> **Per-Version Lock Scoping:** `lock_ruleset_version(version)` locks rate modifications (`set_jurisdiction_rate`) strictly for the targeted version (e.g. version N). Locking version N does not block rate edits for other active or newly published ruleset versions (e.g. version N+1).

> [!NOTE]
> **Forward-Only Jurisdiction Assignment:** Calling `set_employee_jurisdictions` updates the set of jurisdictions evaluated for future pay periods. It never retroactively alters or deletes existing `AccruedWithholding` records or event logs. Historical tax liabilities accrued under previously assigned jurisdictions remain intact, queryable for annual summaries and audit reporting, and eligible for remittance.

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
remit_withholding(caller, jurisdiction, token, amount) → i128
```

Call periodically (monthly/quarterly) to settle the employer's tax liability. Internally:

1. Reads the treasury address from owner-controlled storage.
2. Reads the `AccruedWithholding` balance for the jurisdiction.
3. Rejects a non-positive amount or one greater than the outstanding balance.
4. Decreases the balance by `amount` **before** the token transfer (state-before-interaction).
5. Transfers `amount` of `token` from `caller` to `treasury`.
6. Emits `("withholding_remitted", WithholdingRemittedEvent)`.
7. Returns the remitted amount.

The caller must hold at least `amount` of `token` in their account. `amount` may
not exceed the accrued balance, so cumulative remittances cannot exceed
cumulative recorded liability.

### Liability conservation

`calculate_withholding` is a read-only quote and never creates a remittable
balance. A completed payroll period must be recorded with `accrue_withholding`.
For each jurisdiction, the outstanding liability is conserved as:

```
outstanding = cumulative accrued withholding - cumulative remittances
```

`remit_withholding` accepts only a positive `amount` no greater than
`outstanding` and subtracts exactly that amount before transferring tokens.
Consequently, a duplicated, stale, or oversized remittance request fails with
`AmountExceedsAccrued` without changing the liability balance or moving funds.

### View Functions

| Function | Returns |
|----------|---------|
| `calculate_withholding(employee, gross_amount)` | `TaxComputation` (no state change) |
| `get_jurisdiction_rate(jurisdiction, version)` | `Option<u32>` — rate in bps for version |
| `get_jurisdiction_treasury(jurisdiction)` | `Option<Address>` |
| `get_employee_jurisdictions(employee)` | `Vec<Symbol>` |
| `get_accrued_balance(jurisdiction)` | `i128` — unremitted balance |
| `is_ruleset_locked(version)` | `bool` — returns true if target version is locked |
| `get_active_ruleset_version()` | `u32` — default ruleset version |
| `get_latest_ruleset_version()` | `u32` — highest published version number |
| `get_ruleset_metadata(version)` | `Option<RulesetMetadata>` |

## Rounding Policy (NatSpec)

Withholding is computed as:

```
withheld = floor(gross_amount × rate_bps / 10_000)
```

Floor division means any sub-unit fractional remainder stays with the employee in their net pay. Rounding always favours the employee, never the treasury. This prevents systematic over-withholding across many small pay periods.

**Example:** 15% of 10_001 = 1500.15 → withheld = 1500, net = 8501.

## Zero-Percent Bracket Safety

The contract safely supports 0% tax rates (tax-exempt jurisdictions) without risk of division-by-zero panics.

### Implementation Safety

The withholding calculation divides by the constant `10_000` (basis points), not by the rate itself:

```rust
let part = gross_amount
    .checked_mul(rate_bps as i128)
    .ok_or(TaxError::ArithmeticError)?
    .checked_div(10_000)  // Constant divisor, never zero
    .ok_or(TaxError::ArithmeticError)?;
```

When `rate_bps = 0`, the calculation becomes `gross_amount * 0 / 10_000 = 0`, which is mathematically correct and safe.

### Use Cases

- **Tax-exempt jurisdictions**: Some regions may have 0% income tax for certain income types
- **Blended calculations**: Employees can have multiple jurisdictions where some are 0% and others are non-zero
- **Future-proofing**: The design allows legitimate 0% brackets without special handling

### Test Coverage

The test suite includes:
- `test_zero_percent_bracket_division_safety`: Verifies 0% bracket returns zero withholding without panic
- `test_zero_percent_bracket_blended_with_non_zero_brackets`: Verifies correct blended calculation when 0% brackets are mixed with non-zero brackets

## Security Model

| Invariant | Enforcement |
|-----------|-------------|
| Only owner can configure rates, treasuries, and employee jurisdictions | `require_owner` helper checks caller == stored owner before any write |
| Withheld funds cannot be redirected to arbitrary addresses | `remit_withholding` reads treasury from owner-controlled `JurisdictionTreasury` storage — the caller supplies only the token, never the destination |
| No re-entrancy on remittance | Accrued balance is decreased before `token.transfer` is called |
| No over-remittance | Requested amount must be positive and no greater than the currently accrued balance |
| Overflow-safe arithmetic | All multiplications and additions use `checked_*` and return `ArithmeticError` on overflow |
| Total withholding ≤ gross | Validated after summation; returns `ArithmeticError` if combined rates exceed 100% |
| Historical accrual preservation | `set_employee_jurisdictions` changes are forward-only; prior `AccruedWithholding` balances and event logs are never erased or retroactively modified |
| Per-version ruleset locking | `lock_ruleset_version` freezes `set_jurisdiction_rate` edits for targeted version N only, keeping other versions (e.g. N+1) editable |

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
client.remit_withholding(&owner, &Symbol::new(&env, "US_FED"), &token, &1_000i128);
client.remit_withholding(&owner, &Symbol::new(&env, "US_STATE"), &token, &500i128);
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
| 9 | `AmountExceedsAccrued` | Requested amount is non-positive or exceeds the accrued balance |
