## Tax Withholding Contract

The `tax_withholding` contract implements **configurable, multi‑jurisdiction tax withholding** for payroll payments. It computes per‑jurisdiction tax shares and the resulting net amount, without performing token transfers.

---

### Tax Model

- **Rates in basis points**:
  - Stored as integers in the range \[0, 10_000\], where 10_000 = 100%.
  - Withholding per jurisdiction:
    \[
    \text{tax} = \frac{\text{gross} \times \text{rate\_bps}}{10\,000}
    \]

- **Employee‑level configuration**:
  - Each employee can be associated with multiple jurisdictions.
  - Total tax is the sum of per‑jurisdiction amounts (bounded so it never exceeds `gross`).

---

### Storage Layout

- `StorageKey::Owner` – contract owner (global admin).
- `StorageKey::JurisdictionRate(Symbol) -> u32` – rate in basis points per jurisdiction.
- `StorageKey::EmployeeJurisdictions(Address) -> Vec<Symbol>` – jurisdictions applicable to an employee.

**Result types:**

- `TaxShare`
  - `jurisdiction: Symbol`
  - `amount: i128`

- `TaxComputation`
  - `gross_amount: i128`
  - `total_tax: i128`
  - `net_amount: i128`
  - `shares: Vec<TaxShare>`

---

### Initialization

```rust
pub fn initialize(env: Env, owner: Address)
```

- Sets `Owner`; only this account can configure tax rates and employee jurisdictions.

---

### Configuring Jurisdictions

```rust
pub fn set_jurisdiction_rate(
    env: Env,
    caller: Address,
    jurisdiction: Symbol,
    rate_bps: u32,
) -> Result<(), TaxError>

pub fn get_jurisdiction_rate(env: Env, jurisdiction: Symbol) -> Option<u32>
```

- **Access control**:
  - Only the `Owner` may call `set_jurisdiction_rate`.
- **Validation**:
  - `rate_bps` must be ≤ 10_000, otherwise `TaxError::InvalidRate`.

Configure employee‑specific jurisdiction sets:

```rust
pub fn set_employee_jurisdictions(
    env: Env,
    caller: Address,
    employee: Address,
    jurisdictions: Vec<Symbol>,
) -> Result<(), TaxError>

pub fn get_employee_jurisdictions(env: Env, employee: Address) -> Vec<Symbol>
```

- **Access control**:
  - Only the `Owner` may assign jurisdictions to an employee.

---

### Computing Withholding

```rust
pub fn calculate_withholding(
    env: Env,
    employee: Address,
    gross_amount: i128,
) -> Result<TaxComputation, TaxError>
```

**Behavior**

1. Loads the employee’s jurisdictions.
   - If none are configured → `TaxError::NotConfigured`.
2. For each jurisdiction:
   - Reads `JurisdictionRate`.
   - Computes `share = gross_amount * rate_bps / 10_000` using checked arithmetic.
3. Accumulates `total_tax` as the sum of `share.amount`.
4. Ensures `total_tax ≤ gross_amount`; otherwise `TaxError::ArithmeticError`.
5. Computes `net_amount = gross_amount - total_tax`.

**Errors**

- `Unauthorized` – configuration calls from non‑owner accounts.
- `InvalidRate` – rate outside \[0, 10_000\].
- `NotConfigured` – missing jurisdiction or employee configuration.
- `ArithmeticError` – any overflow/underflow or inconsistent totals.

---

### Example Flow

1. **Configure global rates:**

```rust
let fed = Symbol::new(&env, \"US_FED\");
let state = Symbol::new(&env, \"US_STATE\");
client.set_jurisdiction_rate(&owner, &fed, &1000u32);  // 10%
client.set_jurisdiction_rate(&owner, &state, &500u32); // 5%
```

2. **Assign employee jurisdictions:**

```rust
let jurisdictions = Vec::from_array(&env, [fed.clone(), state.clone()]);
client.set_employee_jurisdictions(&owner, &employee, &jurisditions);
```

3. **Calculate withholding for a payment:**

```rust
let gross = 20_000i128;
let result = client.calculate_withholding(&employee, &gross);
// result.total_tax == 3_000; result.net_amount == 17_000
```

The computed `TaxComputation` can then be used by the payroll contract to
route withheld amounts into jurisdiction‑specific tax accounts and send the
net amount to the employee.

