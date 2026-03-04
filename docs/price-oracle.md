## Price Oracle Integration Contract

The **PriceOracleContract** integrates external price feeds with the core
`PayrollContract` FX system. It provides a thin, auditable layer that:

- Accepts prices from **multiple oracle sources**.
- Enforces **rate bounds** and **freshness windows** per `(base, quote)` pair.
- Updates the payroll contract’s FX table via `set_exchange_rate`, acting as a
  configured FX admin.

---

### Core Roles

- **Owner**
  - Initializes the oracle with a reference to the `PayrollContract`.
  - Manages the set of authorized oracle sources.
  - Configures bounds and freshness rules for each `(base, quote)` pair.

- **Oracle sources**
  - Addresses allowed to call `push_price`.
  - Can represent individual feeders, off-chain services, or other contracts.

- **Payroll contract**
  - Receives validated FX rates via `set_exchange_rate`.
  - Must be configured once with `set_exchange_rate_admin(oracle_address)` so
    the oracle contract is allowed to update rates.

---

### Data Model

On-chain storage (see `price_oracle/src/lib.rs`):

- `Owner: Address`
- `PayrollContract: Address`
- `OracleSource(Address) -> bool`
- `PairConfig(base: Address, quote: Address) -> PairConfig`
- `PairState(base: Address, quote: Address) -> PairState`

Where:

- `PairConfig`:
  - `min_rate` / `max_rate`: inclusive bounds for scaled rate
    (`quote_per_base * FX_SCALE` as used by `PayrollContract`).
  - `max_staleness_seconds`: maximum allowed age of an update based on
    `source_timestamp` vs `Env::ledger().timestamp()`.
  - `enabled`: allows pausing a pair without deleting configuration.

- `PairState`:
  - `rate`: last accepted rate.
  - `last_updated_ts`: timestamp associated with the accepted update.
  - `last_source`: oracle source address that supplied the update.

---

### Update Flow (`push_price`)

1. **Authorization**
   - `source` must be present in `OracleSource(Address)`.
2. **Configuration lookup**
   - `PairConfig(base, quote)` must exist and be `enabled`.
3. **Bounds & freshness**
   - `min_rate <= rate <= max_rate`
   - Let `now = Env::ledger().timestamp()` and `age = now - source_timestamp`:
     - `age <= max_staleness_seconds`
4. **Monotonicity**
   - If `PairState` already exists and `source_timestamp <= last_updated_ts`,
     the update is ignored (treated as a no-op).
5. **State + FX write**
   - Persist `PairState` with `(rate, last_updated_ts, last_source)`.
   - Call `PayrollContract.set_exchange_rate` using the oracle contract’s own
     address as caller. This succeeds only if:
     - The oracle address is the configured FX admin, or
     - The oracle address is the payroll owner (unusual in practice).

On failure at any step, an `OracleError` is returned and no state in either
contract is mutated.

---

### Integration with Existing FX Flow

The existing `PayrollContract` exposes:

- `set_exchange_rate_admin(caller, admin)`
- `set_exchange_rate(caller, base, quote, rate)`
- `convert_currency(from_token, to_token, amount)`

Integration steps:

1. **Deploy and initialize the oracle**
   - Call `PriceOracleContract.initialize(owner, payroll_address)`.
2. **Authorize oracle as FX admin**
   - From the payroll owner:
     - `PayrollContract.set_exchange_rate_admin(owner, oracle_address)`.
3. **Configure sources and pairs**
   - Oracle owner calls:
     - `add_source(source_address)`
     - `configure_pair(base, quote, min_rate, max_rate, max_staleness_seconds)`
4. **Feed prices**
   - Authorized sources call `push_price(source, base, quote, rate, source_ts)`.
   - On success, the payroll FX table is updated and any subsequent
     `convert_currency` / `claim_payroll_in_token` operations use the fresh
     rate.

This design keeps **validation and multiplexing of price sources** in a single
contract, while reusing the existing FX accounting and conversion logic in the
core payroll contract.  

