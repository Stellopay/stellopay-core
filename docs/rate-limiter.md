# Rate Limiter Contract (Token Bucket)

## Overview

The Rate Limiter contract provides per-address and global throttling using the **Token Bucket** algorithm. This approach is superior to fixed-window limiting as it allows for bursts of traffic while maintaining a steady long-term rate, making it ideal for abuse-prone operations like spam proposals or rapid policy toggles.

### Key Features
- **Token Bucket Algorithm**: Smoothly handles bursts and steady-state traffic.
- **Global Throttling**: Optional global limit that applies across all users.
- **Per-Address Overrides**: Fine-grained control for specific high-trust or high-risk addresses.
- **Admin Bypass**: Prevents permanent lockout of governance controllers by exempting admins from limits.
- **Soroban Optimized**: Efficient storage usage using persistent data.

## Mechanism: Token Bucket

A "bucket" is initialized with a **Burst Capacity** (maximum tokens). Every second, a **Refill Rate** number of tokens are added to the bucket, up to the burst capacity. Each operation consumes one token. If no tokens are available, the operation is rejected.

### Example Configuration
- `Burst: 5`, `Refill Rate: 1`: Allows a user to perform 5 operations immediately, then 1 operation per second thereafter.

## API Reference

### Initialization
- `initialize(admin, default_burst, default_refill_rate, admin_bypass)`
  - Sets up the contract. `admin_bypass` ensures the admin (e.g., a DAO or multisig) cannot be locked out by its own rate limits.

### Configuration
- `set_global_limit(enabled, burst, refill_rate)`: Enables or disables the global rate limit.
- `set_limit_for(addr, burst, refill_rate)`: Sets an override for a specific address.
- `clear_limit_for(addr)`: Reverts an address to the default limit.
- `transfer_admin(new_admin)`: Changes the contract administrator.

### Consumption
- `check_and_consume(subject) -> u32`: The main entry point. Increments usage and returns remaining tokens. Throws an error (trap) if the limit is exceeded.

### Maintenance
- `reset_usage(addr)`: Allows the admin to manually clear a user's rate limit state (e.g., after an appeal).

## Security Assumptions

1. **Admin Trust**: The admin is trusted to set reasonable limits and not maliciously throttle users.
2. **Lockout Prevention**: The `admin_bypass` flag is critical. It should be set to `true` for contracts controlled by governance to ensure that even in high-load scenarios, administrative actions (like changing limits) can still proceed.
3. **Clock Accuracy**: The contract relies on `env.ledger().timestamp()`. Minor clock skew between validators is handled by the Stellar protocol.

## Integration

Other contracts can integrate the rate limiter by storing its contract ID and calling `check_and_consume(caller)` at the start of protected functions.

```rust
let rate_limiter = RateLimiterClient::new(&env, &rate_limiter_id);
rate_limiter.check_and_consume(&env.invoker());
```
