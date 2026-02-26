## Multi-Currency Support

StelloPay’s payroll contract now supports **multi-currency payouts** on top of the existing single-token agreements. Each agreement still has a **base token** for accounting, but employees can be paid out in any token that has:

- Sufficient escrow balance for the agreement, and  
- A configured **FX rate** against the base token.

All accounting (totals, paid amounts, grace-period logic) remains in the base token, while actual transfers can occur in alternate payout tokens.

---

### Fixed-Point FX Representation

Exchange rates are stored on-chain in a fixed-point format as:

\[
\text{rate} = \text{quote\_per\_base} \times 10^6
\]

- `FX_SCALE = 1_000_000` (1e6 precision)
- If 1 base token = 2 payout tokens, the stored rate is `2_000_000`.

Conversion is performed as:

\[
\text{amount\_quote} = \frac{\text{amount\_base} \times \text{rate}}{10^6}
\]

Overflow, zero, or negative rates are rejected with `PayrollError::ExchangeRateInvalid` / `PayrollError::ExchangeRateOverflow`.

---

### Storage Layout

FX data is stored using the existing `DataKey` enum:

- `DataKey::ExchangeRate(base, quote) -> i128`  
  - Value: fixed-point rate `quote_per_base * 10^6`

Access control for FX updates uses `StorageKey`:

- `StorageKey::Owner` – contract owner (set by `initialize`)  
- `StorageKey::ExchangeRateAdmin` – optional designated FX/oracle admin

---

### Public API

#### `set_exchange_rate_admin`

```rust
pub fn set_exchange_rate_admin(
    env: Env,
    caller: Address,
    admin: Address,
) -> Result<(), PayrollError>
```

- **Purpose**: Designate a global FX admin (e.g. an oracle contract) allowed to push FX updates.
- **Access control**: `caller` must be the contract owner.
- **Effect**: Stores `admin` under `StorageKey::ExchangeRateAdmin`.

#### `set_exchange_rate`

```rust
pub fn set_exchange_rate(
    env: Env,
    caller: Address,
    base: Address,
    quote: Address,
    rate: i128,
) -> Result<(), PayrollError>
```

- **Purpose**: Configure `rate` for a `(base, quote)` pair.
- **Access control**:
  - `caller == owner` (from `StorageKey::Owner`), **or**
  - `caller == exchange_rate_admin` (from `StorageKey::ExchangeRateAdmin`)
- **Validation**:
  - `base != quote`
  - `rate > 0`
- **Storage**:
  - Writes `DataKey::ExchangeRate(base, quote) = rate`.

To support both directions (base→quote and quote→base), callers can set two rates:

- `set_exchange_rate(base, quote, rate_bq)`
- `set_exchange_rate(quote, base, rate_qb)`

#### `convert_currency`

```rust
pub fn convert_currency(
    env: Env,
    from_token: Address,
    to_token: Address,
    amount: i128,
) -> Result<i128, PayrollError>
```

- **Purpose**: Pure conversion helper for off-chain estimation and validation.
- **Behavior**:
  - If `from_token == to_token` or `amount == 0`, returns `amount`.
  - Otherwise, loads `DataKey::ExchangeRate(from, to)` and returns the converted value in `to_token`.
- **Errors**:
  - `ExchangeRateNotFound` – missing FX rate for `(from, to)`
  - `ExchangeRateInvalid` – non-positive rate or division error
  - `ExchangeRateOverflow` – multiplication overflow

#### `claim_payroll_in_token`

```rust
pub fn claim_payroll_in_token(
    env: Env,
    caller: Address,
    agreement_id: u128,
    employee_index: u32,
    payout_token: Address,
) -> Result<(), PayrollError>
```

**Semantics**

- Agreement is **denominated in a base token** (`Agreement.token`).
- Employee calls this function to receive salary in `payout_token`.
- Accounting (`total_amount`, `paid_amount`, per-period logic) remains in base units.
- Actual token transfer uses `payout_token` and the converted amount.

**Flow (high level)**

1. **Validation & state checks**
   - Agreement exists and is in `Payroll` mode.
   - Agreement status is `Active` or `Cancelled` with an active grace period.
   - `caller` matches the employee at `employee_index`.
2. **Period math**
   - Uses `DataKey::AgreementActivationTime`, `AgreementPeriodDuration`,
     `EmployeeClaimedPeriods`, and `EmployeeSalary` as in `claim_payroll`.
   - Computes `amount_base = salary_per_period * periods_to_pay`.
3. **FX conversion**
   - If `payout_token == base_token`, defers to standard `claim_payroll`.
   - Else calls internal `convert_amount(env, &base_token, &payout_token, amount_base)`.
4. **Escrow checks & transfer**
   - Reads `DataKey::AgreementEscrowBalance(agreement_id, payout_token)`.
   - Requires balance ≥ `amount_payout`.
   - Transfers `amount_payout` from the contract to the employee via the payout token’s `transfer` entrypoint, using `authorize_as_current_contract` for auth.
5. **State updates**
   - Decrements escrow balance for `payout_token`.
   - Increments `EmployeeClaimedPeriods` for the employee.
   - Increments `AgreementPaidAmount` **by `amount_base`** (base currency).
6. **Events**
   - `PayrollClaimedEvent.amount` – recorded in **base** units.
   - `PaymentSentEvent` / `PaymentReceivedEvent` – reflect:
     - `token = payout_token`
     - `amount = amount_payout` (converted amount actually transferred)

---

### Typical Workflow Example

1. **Owner initializes contract** and configures an FX oracle/admin:

```rust
client.initialize(&owner);
client.set_exchange_rate_admin(&owner, &oracle_addr);
```

2. **Oracle (or owner) publishes rates**:

```rust
// 1 base = 2 payout
let rate: i128 = 2_000_000;
client.set_exchange_rate(&oracle_addr, &base_token, &payout_token, &rate);
```

3. **Employer creates and funds a payroll agreement** (base token):

- Uses `create_payroll_agreement`, `add_employee_to_agreement`, `activate_agreement`.
- Escrow for payouts is funded in `payout_token` and recorded via
  `DataKey::AgreementEscrowBalance(agreement_id, payout_token)`.

4. **Employee claims salary in payout_token**:

```rust
client.claim_payroll_in_token(&employee, &agreement_id, &0u32, &payout_token);
```

5. **On-chain effects**:

- Employee receives converted salary in `payout_token`.
- Agreement’s `paid_amount` moves in base units.
- Escrow for `payout_token` is debited by the converted payout amount.

---

### Security & Safety Notes

- **Access control**:
  - Only the **owner** can set the FX admin.
  - Only the **owner or FX admin** can update FX rates.
- **Overflow protection**:
  - All FX math uses `checked_mul` / `checked_div` and returns
    `ExchangeRateOverflow` / `ExchangeRateInvalid` on failure.
- **Escrow isolation**:
  - Escrow balances are per `(agreement_id, token)` via
    `DataKey::AgreementEscrowBalance`.
  - Multi-currency payouts never touch escrow balances for other tokens.
- **Deterministic behaviour**:
  - `claim_payroll_in_token` shares the same eligibility and period-counting
    logic as `claim_payroll`, ensuring consistent invariants across currencies.

