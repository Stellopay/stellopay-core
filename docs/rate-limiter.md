# Rate Limiter Contract (Token Bucket)

## Overview

The Rate Limiter contract provides per-address, per-contract, and global throttling using the **Token Bucket** algorithm. This approach is superior to fixed-window limiting as it allows for bursts of traffic while maintaining a steady long-term rate, making it ideal for abuse-prone operations like spam proposals or rapid policy toggles.

### Key Features
- **Token Bucket Algorithm**: Smoothly handles bursts and steady-state traffic.
- **Global Throttling**: Optional global limit that applies across all users.
- **Per-Address Overrides**: Fine-grained control for specific high-trust or high-risk addresses.
- **Per-Contract Budgets**: Caps total throughput for an integrating contract across all subject addresses, so address rotation cannot bypass the cap.
- **Admin Bypass**: Prevents permanent lockout of governance controllers by exempting admins from limits.
- **Soroban Optimized**: Efficient storage usage using persistent data.

## Mechanism: Token Bucket

A "bucket" is initialized with a **Burst Capacity** (maximum tokens). Every second, a **Refill Rate** number of tokens are added to the bucket, up to the burst capacity. Each operation consumes one token. If no tokens are available, the operation is rejected.

### Fractional Refill and Rounding Policy

The contract uses Soroban ledger timestamps, which are whole seconds, and stores
bucket balances as whole `u32` tokens. Refill is calculated as:

```text
new_tokens = elapsed_whole_seconds * refill_rate
```

There is no fractional-token accumulator. Multiple calls made within the same
ledger second share the same bucket balance and receive no partial refill
credit. This intentionally rounds sub-second refill down to zero, preventing an
attacker from splitting activity into many tiny calls to farm rounding dust or
exceed `burst + elapsed_whole_seconds * refill_rate`.

### Example Configuration
- `Burst: 5`, `Refill Rate: 1`: Allows a user to perform 5 operations immediately, then 1 operation per second thereafter.

## API Reference

### Initialization
- `initialize(admin, default_burst, default_refill_rate, admin_bypass)`
  - Sets up the contract. `admin_bypass` ensures the admin (e.g., a DAO or multisig) cannot be locked out by its own rate limits.

### Configuration
- `set_global_limit(enabled, burst, refill_rate)`: Enables or disables the global rate limit.
- `set_limit_for(addr, burst, refill_rate)`: Sets an override for a specific address.
- `clear_limit_for(addr)`: Removes the per-address override and causes subsequent
  `check_and_consume` calls for that address to fall back to the default (global)
  limit.  Safe to call even when the address has no override — the call is a
  no-op in that case.  See [Limit Resolution Precedence](#limit-resolution-precedence)
  for the full fallback chain.
- `set_limit_for_contract(contract, burst, refill_rate)`: Sets a shared throughput
  budget for an integrating (calling) contract. Distinct from `set_limit_for`.
- `clear_limit_for_contract(contract)`: Removes the per-contract budget (does not
  reset usage). Safe no-op when unset.
- `get_limit_for_contract(contract) -> Option<LimitConfig>`: Returns the configured
  contract budget, or `None` if unset.
- `transfer_admin(new_admin)`: Changes the contract administrator.

### Consumption
- `check_and_consume(subject) -> Result<ConsumptionOutcome, RateLimitError>`: Address
  (+ optional global) path. Increments usage and reports the subject's remaining
  allowance and time until refill. Returns
  `Err(RateLimitError::RateLimitExceeded)` when the bucket is empty.
- `check_and_consume_for_contract(subject, contract) -> Result<ConsumptionOutcome, RateLimitError>`:
  Enforces the subject address budget **and**, when configured, the shared contract
  budget. Either bucket being empty rejects the call.

Neither entrypoint signals exhaustion with an in-band value: a served call is `Ok`,
and only a rejected call is `Err`. See [Outcome and Exhaustion](#outcome-and-exhaustion).

### Outcome and Exhaustion

The return type names what it carries, so callers no longer have to read the
implementation to learn whether a number is the remaining allowance, the consumed
amount, or a wait time:

```rust
pub struct ConsumptionOutcome {
    /// Subject's allowance after this call consumed one token.
    /// `u32::MAX` means the admin bypass is active (effectively unlimited).
    pub remaining: u32,
    /// Whole seconds until the subject's bucket next holds a token.
    pub refill_in_seconds: Option<u64>,
}

pub enum RateLimitError {
    /// A global, per-contract, or per-address bucket held no tokens.
    RateLimitExceeded = 1,
}
```

`refill_in_seconds` values:

| Value | Meaning |
| --- | --- |
| `Some(0)` | A token is available now. |
| `Some(1)` | The bucket was emptied by this call; the next whole-second tick refills it. |
| `None` | The bucket is empty and the refill rate is `0`, so it never refills on its own. |

Exhaustion behaviour, guaranteed by both the implementation and the contract's
documentation:

- An empty bucket is reported as `Err(RateLimitError::RateLimitExceeded)`. It is
  **never** reported as a `0` balance, and a served call that happens to leave `0`
  tokens is still `Ok` — this is the whole point of the typed outcome.
- A rejected call debits no bucket. All enforced buckets (global, per-contract,
  per-address) are checked before any is debited, so a rejection cannot partially
  drain an unrelated budget.
- `try_check_and_consume` / `try_check_and_consume_for_contract` return the typed
  error; the non-`try` accessors trap on rejection.

The four boundary cases are pinned by tests in `test_rate_limit.rs`: consumption
below the burst, exactly at the burst (which is `Ok` with `remaining == 0`), past the
burst (typed error), and after a refill interval.

### Maintenance
- `reset_usage(addr)`: Allows the admin to manually clear a user's rate limit state (e.g., after an appeal).
- `reset_contract_usage(contract)`: Clears the contract-scoped usage bucket.

## Limit Resolution Precedence

Every call to `check_and_consume` must resolve a `LimitConfig` (burst capacity
and refill rate) for the subject address.  The contract uses the following
priority order, highest first:

```
1. Per-address override  — set via set_limit_for(addr, burst, refill_rate)
2. Default (global) limit — set via initialize(…, default_burst, default_refill_rate, …)
```

There is no third tier: if no per-address override exists, the contract always
falls back to the default values stored during `initialize`.

### Precedence examples

The intended behavior is exclusive precedence, not additive merging:

- If the default limit is `burst = 5` and a caller-specific override sets
  `burst = 2`, that caller gets only 2 immediate successful calls. The default
  burst of 5 is ignored for that caller.
- If the default limit is `burst = 2` and a caller-specific override sets
  `burst = 4`, that caller gets 4 immediate successful calls. The stricter
  default is ignored for that caller.
- If an address has no override at all, it continues to use the default
  `default_burst` and `default_refill_rate` from `initialize`.

The test suite covers all three cases so reviewers can verify the precedence
rule directly in `test_rate_limit.rs`.

### What `clear_limit_for` does

`clear_limit_for(addr)` removes the `StorageKey::Limit(addr)` entry from
persistent storage.  After the call:

- `get_limit_for(addr)` returns the default `LimitConfig`.
- `check_and_consume(addr)` draws from the address's *usage* bucket but now
  sizes and caps that bucket against the **default** burst and refill rate, not
  the old override.
- If the address never had a per-address override, calling `clear_limit_for` is
  a safe no-op: the Soroban storage layer silently ignores removal of a
  non-existent key, and the default limit remains intact.

### Interaction with usage state

`clear_limit_for` removes the *config* entry only — it does **not** reset the
address's *usage* (token count and last-update timestamp).  In most cases this
is the right behavior: you are changing the cap, not forgiving past consumption.
If you also want to give the address a fresh bucket at the new (default) cap,
call `reset_usage(addr)` immediately after `clear_limit_for(addr)`.

### Visualized lookup chain

```
check_and_consume(addr)
        │
        ▼
StorageKey::Limit(addr)  ──exists?──► use override LimitConfig
        │
       no
        │
        ▼
StorageKey::DefaultBurst + StorageKey::DefaultRefillRate  ──► use default LimitConfig
```

## Per-contract budgets

Integrators that call the rate limiter on behalf of many users should use
`check_and_consume_for_contract(subject, contract)` and pass their own contract
address (typically `env.current_contract_address()`). When the admin has called
`set_limit_for_contract` for that address:

1. Global bucket (if enabled)
2. Contract-scoped bucket (shared by all subjects through that contract)
3. Per-address bucket (override or default)

Exhausting **either** the contract or address bucket rejects the call. Rotating
subject addresses therefore cannot exceed the contract-scoped cap. If no contract
budget is configured, step 2 is skipped and behavior matches address-only limiting.

`check_and_consume(subject)` remains unchanged and does **not** debit contract
buckets — use it only when contract-scoped capping is not required.

## Security Assumptions

1. **Admin Trust**: The admin is trusted to set reasonable limits and not maliciously throttle users.
2. **Lockout Prevention**: The `admin_bypass` flag is critical. It should be set to `true` for contracts controlled by governance to ensure that even in high-load scenarios, administrative actions (like changing limits) can still proceed.
3. **Clock Accuracy**: The contract relies on `env.ledger().timestamp()`. Minor clock skew between validators is handled by the Stellar protocol.
4. **No Fractional Drift**: Because refill uses whole-second integer arithmetic and caps balances at burst capacity, repeated sub-second calls cannot accumulate fractional rounding credit beyond the theoretical token-bucket allowance.
5. **Override Isolation**: A per-address override changes only that caller's
   effective limit configuration. It does not mutate the initialized default
   values, and callers without overrides remain governed by the default bucket.
6. **Burst Capacity Capping**: After any idle gap (even extremely long ones), the bucket refills to exactly the configured `burst` capacity. `preview_refill` caps token accumulation at `burst`, preventing attackers from "farming" tokens by waiting extended periods between calls. This is verified by the `test_long_idle_gap_refill_is_capped_at_burst_capacity` test.
7. **Contract Cap vs Address Rotation**: The contract-scoped bucket is shared across subjects. Clever rotation of per-address identities within one integrating contract cannot bypass a configured `set_limit_for_contract` budget when consumption goes through `check_and_consume_for_contract`.

## Integration

Other contracts can integrate the rate limiter by storing its contract ID and calling
`check_and_consume(caller)` (address-only) or `check_and_consume_for_contract(caller, env.current_contract_address())`
(address + contract budgets) at the start of protected functions.

For example, the `stello_pay_contract` integrates the Rate Limiter by optionally storing its address via `set_rate_limiter_contract(owner, addr)`. When configured, the `claim_payroll`, `claim_payroll_in_token`, and `batch_claim_payroll` entrypoints will invoke `try_check_and_consume(&caller)` to throttle spam. If the user exceeds their token bucket quota, these entrypoints reject the request with `PayrollError::RateLimited` (error code 34). To also apply a per-contract budget, switch that call to `try_check_and_consume_for_contract` and configure `set_limit_for_contract`.

```rust
#[contractclient(name = "RateLimiterClient")]
trait RateLimiterInterface {
    fn check_and_consume(
        env: Env,
        subject: Address,
    ) -> Result<ConsumptionOutcome, RateLimitError>;
    fn check_and_consume_for_contract(
        env: Env,
        subject: Address,
        contract: Address,
    ) -> Result<ConsumptionOutcome, RateLimitError>;
}

// Address + contract budgets (recommended for multi-user integrators)
let client = RateLimiterClient::new(&env, &rate_limiter_id);
match client.try_check_and_consume_for_contract(&caller, &env.current_contract_address()) {
    Ok(Ok(outcome)) => {
        // Served. `outcome.remaining` is the subject's allowance left and
        // `outcome.refill_in_seconds` says when the next token arrives.
    }
    Ok(Err(_rate_limit_error)) | Err(_) => return Err(PayrollError::RateLimited),
}
```
