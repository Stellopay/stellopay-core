#![no_std]

//! Per-address and global rate limiting contract using Token Bucket algorithm.
//!
//! Provides burst-friendly rate limiting with automatic token refills, 
//! global throttling, and admin bypass to ensure security and fairness.

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

#[contracttype]
#[derive(Clone)]
enum StorageKey {
    Admin,
    Initialized,
    /// Default burst capacity for all addresses
    DefaultBurst,
    /// Default refill rate (tokens per second) for all addresses
    DefaultRefillRate,
    /// Global limit active
    GlobalLimitEnabled,
    /// Global burst capacity
    GlobalBurst,
    /// Global refill rate
    GlobalRefillRate,
    /// Global usage state
    GlobalUsage,
    /// Admin bypass enabled
    AdminBypass,
    /// Per-address override: address -> LimitConfig
    Limit(Address),
    /// Per-address usage: address -> Usage
    Usage(Address),
}

/// Usage state for a token bucket
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Usage {
    /// Last timestamp when tokens were refilled
    pub last_update: u64,
    /// Current number of tokens in the bucket
    pub tokens: u32,
}

/// Configuration for a specific rate limit
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LimitConfig {
    /// Maximum tokens the bucket can hold (burst capacity)
    pub burst: u32,
    /// Tokens added to the bucket per second (refill rate)
    pub refill_rate: u32,
}

#[contract]
pub struct RateLimiter;

#[contractimpl]
impl RateLimiter {
    /// Initializes the Rate Limiter contract.
    ///
    /// @notice Sets initial configuration. Only callable once.
    /// @param admin Admin address that controls configuration (must authenticate).
    /// @param default_burst Max burst capacity for addresses without overrides.
    /// @param default_refill_rate Tokens added per second for addresses without overrides.
    /// @param admin_bypass If true, the admin address is exempt from all rate limits.
    pub fn initialize(
        env: Env,
        admin: Address,
        default_burst: u32,
        default_refill_rate: u32,
        admin_bypass: bool,
    ) {
        admin.require_auth();
        assert!(!Self::is_initialized(&env), "already initialized");

        env.storage().persistent().set(&StorageKey::Admin, &admin);
        env.storage().persistent().set(&StorageKey::DefaultBurst, &default_burst);
        env.storage().persistent().set(&StorageKey::DefaultRefillRate, &default_refill_rate);
        env.storage().persistent().set(&StorageKey::AdminBypass, &admin_bypass);
        env.storage().persistent().set(&StorageKey::Initialized, &true);
    }

    /// Configures the global rate limit.
    ///
    /// @notice Applies to the entire contract across all users if enabled.
    /// @dev Only callable by admin.
    /// @param enabled Whether to enforce the global limit.
    /// @param burst Global maximum burst capacity.
    /// @param refill_rate Global tokens added per second.
    pub fn set_global_limit(env: Env, enabled: bool, burst: u32, refill_rate: u32) {
        Self::require_admin_auth(&env);
        env.storage().persistent().set(&StorageKey::GlobalLimitEnabled, &enabled);
        env.storage().persistent().set(&StorageKey::GlobalBurst, &burst);
        env.storage().persistent().set(&StorageKey::GlobalRefillRate, &refill_rate);
    }

    /// Sets a per-address limit override.
    ///
    /// @dev Only callable by admin.
    /// @param addr Subject address.
    /// @param burst Max burst capacity for this address.
    /// @param refill_rate Tokens added per second for this address.
    pub fn set_limit_for(env: Env, addr: Address, burst: u32, refill_rate: u32) {
        Self::require_admin_auth(&env);
        env.storage().persistent().set(&StorageKey::Limit(addr), &LimitConfig { burst, refill_rate });
    }

    /// Removes a per-address limit override.
    ///
    /// @dev Only callable by admin.
    pub fn clear_limit_for(env: Env, addr: Address) {
        Self::require_admin_auth(&env);
        env.storage().persistent().remove(&StorageKey::Limit(addr));
    }

    /// Checks and consumes one unit from the subject's rate limit.
    ///
    /// @notice Implements Token Bucket algorithm for burst handling.
    /// @notice Validates security by allowing admins to bypass if configured.
    /// @param subject Address to check and consume quota for (must authenticate).
    /// @return tokens_remaining User's tokens remaining after consumption.
    pub fn check_and_consume(env: Env, subject: Address) -> u32 {
        subject.require_auth();
        Self::require_initialized(&env);

        let admin: Address = env.storage().persistent().get(&StorageKey::Admin).unwrap();
        let bypass: bool = env.storage().persistent().get(&StorageKey::AdminBypass).unwrap_or(true);

        // Security assumption: Admin bypass prevents permanent lockout of governance controllers.
        if bypass && subject == admin {
            return u32::MAX;
        }

        // 1. Check Global Limit (if enabled)
        if env.storage().persistent().get(&StorageKey::GlobalLimitEnabled).unwrap_or(false) {
            let g_burst = env.storage().persistent().get(&StorageKey::GlobalBurst).unwrap_or(0);
            let g_refill = env.storage().persistent().get(&StorageKey::GlobalRefillRate).unwrap_or(0);
            Self::consume_bucket(&env, StorageKey::GlobalUsage, g_burst, g_refill);
        }

        // 2. Check Per-Address Limit
        let limit = Self::get_limit_config(&env, &subject);
        Self::consume_bucket(&env, StorageKey::Usage(subject.clone()), limit.burst, limit.refill_rate)
    }

    /// Explicitly resets usage for an address.
    ///
    /// @dev Only callable by admin.
    pub fn reset_usage(env: Env, addr: Address) {
        Self::require_admin_auth(&env);
        env.storage().persistent().remove(&StorageKey::Usage(addr));
    }

    /// Transfers admin rights to a new address.
    ///
    /// @dev Only callable by current admin.
    pub fn transfer_admin(env: Env, new_admin: Address) {
        Self::require_admin_auth(&env);
        env.storage().persistent().set(&StorageKey::Admin, &new_admin);
    }

    /// Gets current config for an address.
    pub fn get_limit_for(env: Env, addr: Address) -> LimitConfig {
        Self::get_limit_config(&env, &addr)
    }

    /// Gets effective admin address.
    pub fn get_admin(env: Env) -> Option<Address> {
        env.storage().persistent().get(&StorageKey::Admin)
    }

    // Internal helpers

    fn consume_bucket(env: &Env, key: StorageKey, burst: u32, refill_rate: u32) -> u32 {
        let now = env.ledger().timestamp();
        let mut usage: Usage = env.storage().persistent().get(&key).unwrap_or(Usage {
            last_update: now,
            tokens: burst,
        });

        // Refill tokens based on time elapsed since last update
        let elapsed = now.saturating_sub(usage.last_update);
        if elapsed > 0 {
            let new_tokens = (elapsed as u32).saturating_mul(refill_rate);
            usage.tokens = usage.tokens.saturating_add(new_tokens);
            if usage.tokens > burst {
                usage.tokens = burst;
            }
            usage.last_update = now;
        }

        assert!(usage.tokens >= 1, "rate limit exceeded");
        usage.tokens -= 1;

        env.storage().persistent().set(&key, &usage);
        usage.tokens
    }

    fn get_limit_config(env: &Env, addr: &Address) -> LimitConfig {
        env.storage().persistent().get(&StorageKey::Limit(addr.clone())).unwrap_or_else(|| {
            LimitConfig {
                burst: env.storage().persistent().get(&StorageKey::DefaultBurst).unwrap_or(0),
                refill_rate: env.storage().persistent().get(&StorageKey::DefaultRefillRate).unwrap_or(0),
            }
        })
    }

    fn is_initialized(env: &Env) -> bool {
        env.storage().persistent().get(&StorageKey::Initialized).unwrap_or(false)
    }

    fn require_initialized(env: &Env) {
        assert!(Self::is_initialized(env), "not initialized");
    }

    fn require_admin_auth(env: &Env) {
        let admin: Address = env.storage().persistent().get(&StorageKey::Admin).expect("admin not set");
        admin.require_auth();
    }
}

