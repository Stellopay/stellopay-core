# Rate Limiting Contract

## Overview

The rate limiter enforces per-address quotas over a configurable time window to prevent abuse and ensure fair usage. It provides:
- Configurable default and per-address limits
- Automatic window reset based on ledger timestamp
- Admin-controlled explicit resets and configuration updates
- NatSpec-style documentation in the contract

Location: `onchain/contracts/rate_limiter`

## Data Model

- Admin: Address authorized to modify configuration.
- DefaultLimit (u32): Global limit per window.
- WindowSeconds (u64): Window duration.
- Limit(Address → u32): Optional per-address override.
- Usage(Address → { count: u32, window_start: u64 }): Current usage within the active window.

## API

- initialize(admin, default_limit, window_seconds)
- get_admin() → Option<Address>
- get_default_limit() → u32
- set_default_limit(limit)
- get_window_seconds() → u64
- set_window_seconds(seconds)
- set_limit_for(addr, limit)
- clear_limit_for(addr)
- get_limit_for(addr) → u32
- get_usage(addr) → Usage
- check_and_consume(subject) → u32
- reset_usage(addr)

## Security Model

- Only the stored admin can modify configuration or reset usage; admin must authenticate.
- Subjects must authenticate to consume quota, preventing arbitrary penalization by others.
- Window reset uses ledger timestamp; tests mock time via `Env::ledger()`.
- All counters use saturating arithmetic and bounds checks to avoid overflow.

## Testing

Comprehensive tests live in `onchain/contracts/rate_limiter/tests/test_rate_limit.rs` and cover:
- Initialization and configuration updates
- Per-address overrides and fallback to default
- Consumption within limit and rejection beyond limit
- Automatic window reset at boundary
- Admin reset and security assumptions
- Edge cases (e.g., zero default limit)

## Usage Notes

- Set sensible defaults and overrides for high-traffic addresses.
- Consider composing this contract in other contracts by calling `check_and_consume` at entry points that need throttling.
- For auditability, emit events if you need operational telemetry. (Current minimal version omits events.) 

