# Price Oracle

FX / reference rate publication with timestamps, staleness checks, and admin/operator roles for payroll conversion flows.

## Overview

The `price_oracle` contract provides:

- **Authorized sources** – Only whitelisted addresses can publish prices.
- **Rate bounds** – Each `(base, quote)` pair has configurable `[min_rate, max_rate]` to limit oracle compromise blast radius.
- **Staleness rejection** – Updates older than `max_staleness_seconds` are rejected; future timestamps are also rejected.
- **Monotonic ordering** – Older-or-equal timestamps are silently ignored, preventing replay of stale rates.
- **Optional quorum mode** – Pairs can require `N` distinct sources to submit matching or near-matching rates inside a time window before a rate is accepted.
- **Payroll integration** – Accepted rates are automatically pushed to the core payroll contract via `set_exchange_rate`.
- **Pair enable/disable** – Admin can pause and resume individual pairs without deleting configuration.
- **Ownership transfer** – Single-step admin transfer for lightweight governance.

## Contract Location

- **Contract**: `onchain/contracts/price_oracle/src/lib.rs`
- **Tests**: `onchain/contracts/price_oracle/tests/test_oracle.rs`

## Rate representation

All rates use a fixed-point representation with 6 decimal places:

| Value | Representation |
|-------|---------------|
| 1.0   | `1_000_000`   |
| 0.5   | `500_000`     |
| 2.5   | `2_500_000`   |

This is referred to as `FX_SCALE` throughout. A rate of `2_000_000` means 1 unit of `base` equals 2.0 units of `quote`.

## Roles

| Role   | Who                   | Can do                                                        |
|--------|-----------------------|---------------------------------------------------------------|
| Owner  | Contract admin        | Add/remove sources, configure/disable/enable pairs, transfer ownership |
| Source | Whitelisted addresses | Push price updates for configured pairs                       |

Sources have no administrative privileges. They can only call `push_price` and only within the bounds the owner configured.

---

## Data Model

On-chain storage (see `price_oracle/src/lib.rs`):

- `Owner: Address`
- `PayrollContract: Address`
- `OracleSource(Address) -> bool`
- `PairConfig(base: Address, quote: Address) -> PairConfig`
- `PairState(base: Address, quote: Address) -> PairState`

Where:

- `PairConfig`:
  - `min_rate` / `max_rate`: inclusive bounds for scaled rate.
  - `max_staleness_seconds`: maximum allowed age of an update.
  - `enabled`: allows pausing a pair without deleting configuration.
  - `quorum_n`: number of distinct sources required. `1` preserves legacy single-source mode.
  - `tolerance_bps`: maximum spread allowed between quorum-supporting rates, in basis points.
  - `quorum_window_seconds`: time bucket size used to group pending quorum votes.

- `PairState`:
  - `rate`: last accepted rate.
  - `last_updated_ts`: timestamp associated with the accepted update.
  - `last_source`: oracle source address that supplied the update.

- Pending quorum state:
  - A single active temporary bucket is kept per pair.
  - Each bucket stores at most one vote per authorized source.
  - The bucket is cleared when quorum is reached, when the pair is reconfigured, or when the pair is disabled.

---

## API

### Initialization

| Function                              | Access | Description                                    |
|---------------------------------------|--------|------------------------------------------------|
| `initialize(owner, payroll_contract)` | Once   | Bootstrap the contract with owner and payroll link |

### Source management

| Function                        | Access | Description                  |
|---------------------------------|--------|------------------------------|
| `add_source(caller, source)`    | Owner  | Whitelist an oracle source   |
| `remove_source(caller, source)` | Owner  | Revoke a source              |
| `is_source_address(addr)`       | Any    | Check if address is a source |

### Pair management

| Function                                                                  | Access | Description                               |
|---------------------------------------------------------------------------|--------|-------------------------------------------|
| `configure_pair(caller, base, quote, min_rate, max_rate, max_staleness, quorum_n, tolerance_bps, quorum_window_seconds)` | Owner  | Create or update a pair's configuration   |
| `disable_pair(caller, base, quote)`                                       | Owner  | Pause updates for a pair                  |
| `enable_pair(caller, base, quote)`                                        | Owner  | Resume updates for a pair                 |
| `get_pair_config(base, quote)`                                            | Any    | Read pair configuration                   |

### Price submission

| Function                                                     | Access | Description                          |
|--------------------------------------------------------------|--------|--------------------------------------|
| `push_price(source, base, quote, rate, source_timestamp)`   | Source | Submit a new rate for a pair         |
| `get_pair_state(base, quote)`                                | Any    | Read last accepted rate and metadata |

### Admin

| Function                                   | Access | Description                   |
|--------------------------------------------|--------|-------------------------------|
| `transfer_ownership(caller, new_owner)`    | Owner  | Transfer admin to new address |
| `get_owner()`                              | Any    | Read current owner            |

---

## Validation pipeline (push_price)

Each `push_price` call passes through these checks in order:

1. **Initialized** – Contract must be initialized.
2. **Source authorized** – Caller must be a registered source.
3. **Non-zero rate** – Rate must be > 0 (rejects zero and negative).
4. **Pair configured & enabled** – The `(base, quote)` pair must exist and be enabled.
5. **Bounds check** – `min_rate <= rate <= max_rate`.
6. **No future timestamp** – `source_timestamp <= ledger.timestamp`.
7. **Staleness check** – `ledger.timestamp - source_timestamp <= max_staleness_seconds`.
8. **Single-source fast path** – If `quorum_n == 1`, the rate is accepted immediately.
9. **Quorum path** – If `quorum_n > 1`, the vote is stored in the active `(pair, bucket)` window.
10. **Duplicate rejection** – The same source cannot vote twice in the same active bucket.
11. **Tolerance check** – Quorum is satisfied only when the completing vote finds `quorum_n` distinct source votes within `tolerance_bps`.
12. **Persist & forward** – Save `PairState`, clear pending bucket state, and call `set_exchange_rate` on the payroll contract.

On failure at any step, an `OracleError` is returned and no state in either
contract is mutated.

---

## Integration with Existing FX Flow

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
     - `configure_pair(base, quote, min_rate, max_rate, max_staleness_seconds, quorum_n, tolerance_bps, quorum_window_seconds)`
4. **Feed prices**
   - Authorized sources call `push_price(source, base, quote, rate, source_ts)`.
   - On success, the payroll FX table is updated and any subsequent
     `convert_currency` / `claim_payroll_in_token` operations use the fresh
     rate.

---

## Error codes

| Code | Name              | Meaning                                        |
|------|-------------------|------------------------------------------------|
| 1    | NotInitialized    | Contract not yet initialized                   |
| 2    | AlreadyInitialized| Double initialization attempt                  |
| 3    | NotAuthorized     | Caller is not the owner                        |
| 4    | InvalidSource     | Caller is not a registered oracle source       |
| 5    | PairNotConfigured | Pair does not exist or is disabled             |
| 6    | RateOutOfBounds   | Rate falls outside `[min_rate, max_rate]`      |
| 7    | RateStale         | Timestamp is future or exceeds staleness limit |
| 8    | FxUpdateFailed    | Downstream payroll `set_exchange_rate` failed  |
| 9    | ZeroRate          | Submitted rate is zero or negative             |
| 10   | InvalidPairConfig | Invalid configuration parameters               |
| 11   | DuplicateVote     | Source already voted in the active quorum bucket |

## Quorum model

Quorum mode is optional and configured per pair:

- `quorum_n = 1` keeps the existing low-latency single-source behavior.
- `quorum_n > 1` enables quorum for that pair only.
- `tolerance_bps = 0` requires exact rate agreement.
- `quorum_window_seconds` defines the time bucket. Votes in different buckets do not combine.

Acceptance rules:

- A vote is added to the pair's active bucket based on `source_timestamp / quorum_window_seconds`.
- If a newer bucket starts, the contract drops the older pending bucket and starts collecting the new one.
- The vote that arrives last is the anchor for the tolerance check.
- Quorum is met when that anchor vote can find `quorum_n` distinct authorized-source submissions in the active bucket whose rates are all within `tolerance_bps` of the anchor.
- The accepted rate is the anchor rate, and the accepted timestamp is the maximum timestamp among the supporting quorum votes.

This model keeps the implementation small and storage bounded while still reducing single-key compromise risk.

## Threat model

| Threat                          | Mitigation                                                                      |
|---------------------------------|---------------------------------------------------------------------------------|
| Compromised oracle source       | Rate bounds limit maximum damage; source cannot modify config or transfer admin |
| Single source compromise in quorum mode | Attacker must compromise `quorum_n` distinct sources or collude within tolerance |
| Stale rate injection            | `max_staleness_seconds` + future-timestamp rejection                            |
| Replay of old rates             | Monotonic timestamp ordering (older updates are no-ops)                         |
| Duplicate voting                | One vote per source in the active quorum bucket                                 |
| Stale pending quorum state      | Pending bucket resets on rollover, reconfigure, disable, and successful acceptance |
| Admin takeover                  | Only owner can add sources, configure pairs, transfer ownership                 |
| Rate manipulation via wide bounds | Bounds are per-pair and admin-controlled; tighten as needed                   |
| Quorum drift via wide tolerance | Admin should keep `tolerance_bps` tight; accepted rate is anchored to the completing vote |
| Disabled pair bypass            | `push_price` checks `enabled` flag before accepting                             |
| Pair direction confusion        | `(A, B)` and `(B, A)` are independent pairs in storage                         |

## Trade-offs

- **Security vs latency**: `quorum_n > 1` reduces single-key risk but adds at least one extra source round-trip.
- **Tolerance vs precision**: larger `tolerance_bps` improves liveness during minor feed variance but widens the accepted spread.
- **Bounded storage vs historical visibility**: the contract keeps only one active temporary bucket per pair instead of an unbounded per-pair submission log.
- **Per-pair flexibility**: deployments can keep high-value pairs in quorum mode and leave low-latency pairs in single-source mode.

## Events

| Topic                      | Data                    | Emitted by           |
|----------------------------|-------------------------|----------------------|
| `("oracle", "init")`      | `owner`                 | `initialize`         |
| `("oracle", "addsrc")`    | `source`                | `add_source`         |
| `("oracle", "rmsrc")`     | `source`                | `remove_source`      |
| `("oracle", "cfgpair")`   | `(base, quote)`         | `configure_pair`     |
| `("oracle", "disable")`   | `(base, quote)`         | `disable_pair`       |
| `("oracle", "enable")`    | `(base, quote)`         | `enable_pair`        |
| `("oracle", "price")`     | `(base, quote, rate)`   | `push_price`         |
| `("oracle", "owner")`     | `new_owner`             | `transfer_ownership` |

## Test coverage (54 tests)

- **Initialization** (2): owner set, double-init blocked
- **Source management** (4): add/remove, non-owner blocked, removed source can't push
- **Pair configuration** (8): read config, same-token rejected, min>max rejected, zero min, negative rate, zero staleness, zero quorum window, non-owner blocked
- **Disable/enable** (4): disable blocks updates, enable resumes, unconfigured pair error, non-owner blocked
- **Push price happy path** (4): full integration, min boundary, max boundary, max staleness boundary
- **Push price forbidden** (8): unregistered source, zero rate, negative rate, below min, above max, future timestamp, stale timestamp, unconfigured pair
- **Monotonic/multi-source** (3): older ignored, equal ignored, latest-wins with backup source
- **Ownership transfer** (4): success, new owner works, old owner blocked, non-owner blocked
- **Uninitialized guards** (5): all admin/source functions revert before init
- **Security scenarios** (4): compromised source blast radius, pair isolation, reconfigure tightens bounds, pair direction matters
- **Quorum-specific edge cases** (12): quorum success, dissent without quorum, duplicate-vote rejection, tolerance-boundary acceptance, bucket rollover reset, removed-source pending vote invalidation, and invalid zero-quorum configuration
