#![no_std]

//! Per-address rate limiting contract
//!
//! Provides configurable rate limits with automatic window resets and
//! explicit admin reset APIs. Designed for integration with other
//! Soroban contracts to prevent abuse and ensure fair usage.

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

#[contract]
pub struct RateLimiter;

/// Storage keys for the rate limiter
#[contracttype]
#[derive(Clone)]
enum StorageKey {
    Initialized,
    Admin,
    DefaultLimit,
    WindowSeconds,
    /// Per-address override: address -> limit
    Limit(Address),
    /// Per-address usage: address -> Usage
    Usage(Address),
}

/// Usage counters for a subject address within a window
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Usage {
    pub count: u32,
    pub window_start: u64,
}

#[contractimpl]
impl RateLimiter {
    /// Initializes the contract
    ///
    /// @notice Sets the admin, default limit, and window size. Callable once.
    /// @param admin Admin address that controls configuration (must authenticate)
    /// @param default_limit Default max operations per window for addresses without overrides
    /// @param window_seconds Duration of a window in seconds (must be > 0)
    pub fn initialize(env: Env, admin: Address, default_limit: u32, window_seconds: u64) {
        admin.require_auth();
        let initialized: bool = env
            .storage()
            .persistent()
            .get(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(!initialized, "Already initialized");
        assert!(window_seconds > 0, "window_seconds must be > 0");

        env.storage().persistent().set(&StorageKey::Admin, &admin);
        env.storage()
            .persistent()
            .set(&StorageKey::DefaultLimit, &default_limit);
        env.storage()
            .persistent()
            .set(&StorageKey::WindowSeconds, &window_seconds);
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);
    }

    /// Gets the admin address
    ///
    /// @return admin Optional admin address (Some if initialized)
    pub fn get_admin(env: Env) -> Option<Address> {
        env.storage().persistent().get(&StorageKey::Admin)
    }

    /// Gets the default limit
    ///
    /// @return limit Default max operations per window
    pub fn get_default_limit(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get(&StorageKey::DefaultLimit)
            .unwrap_or(0u32)
    }

    /// Gets the window duration in seconds
    ///
    /// @return seconds Window length in seconds
    pub fn get_window_seconds(env: Env) -> u64 {
        env.storage()
            .persistent()
            .get(&StorageKey::WindowSeconds)
            .unwrap_or(0u64)
    }

    /// Sets the default limit
    ///
    /// @dev Only callable by admin.
    /// @param limit New default max operations per window
    pub fn set_default_limit(env: Env, limit: u32) {
        Self::require_admin_auth(&env);
        env.storage()
            .persistent()
            .set(&StorageKey::DefaultLimit, &limit);
    }

    /// Sets the window duration
    ///
    /// @dev Only callable by admin.
    /// @param seconds New window size in seconds (must be > 0)
    pub fn set_window_seconds(env: Env, seconds: u64) {
        Self::require_admin_auth(&env);
        assert!(seconds > 0, "window_seconds must be > 0");
        env.storage()
            .persistent()
            .set(&StorageKey::WindowSeconds, &seconds);
    }

    /// Sets a per-address limit override
    ///
    /// @dev Only callable by admin.
    /// @param addr Subject address whose limit is being set
    /// @param limit Max operations per window for this address
    pub fn set_limit_for(env: Env, addr: Address, limit: u32) {
        Self::require_admin_auth(&env);
        env.storage()
            .persistent()
            .set(&StorageKey::Limit(addr), &limit);
    }

    /// Removes a per-address limit override, falling back to the default
    ///
    /// @dev Only callable by admin.
    /// @param addr Subject address whose override is removed
    pub fn clear_limit_for(env: Env, addr: Address) {
        Self::require_admin_auth(&env);
        env.storage().persistent().remove(&StorageKey::Limit(addr));
    }

    /// Gets the effective limit for an address
    ///
    /// @param addr Subject address
    /// @return limit Effective limit (override or default)
    pub fn get_limit_for(env: Env, addr: Address) -> u32 {
        let per: Option<u32> = env
            .storage()
            .persistent()
            .get(&StorageKey::Limit(addr.clone()));
        per.unwrap_or_else(|| {
            env.storage()
                .persistent()
                .get(&StorageKey::DefaultLimit)
                .unwrap_or(0u32)
        })
    }

    /// Gets the current usage for an address
    ///
    /// @param addr Subject address
    /// @return usage Current counter and window start
    pub fn get_usage(env: Env, addr: Address) -> Usage {
        let now = env.ledger().timestamp();
        let window_seconds = Self::window_seconds(&env);
        let usage: Option<Usage> = env
            .storage()
            .persistent()
            .get(&StorageKey::Usage(addr.clone()));
        match usage {
            Some(mut u) => {
                if Self::is_window_expired(u.window_start, now, window_seconds) {
                    u.count = 0;
                    u.window_start = now;
                }
                u
            }
            None => Usage {
                count: 0,
                window_start: now,
            },
        }
    }

    /// Checks and consumes one unit from the subject's rate limit
    ///
    /// @notice Requires subject authentication. Increments usage and enforces limit.
    /// @param subject Address subject to rate limiting (must authenticate)
    /// @return remaining Remaining quota after consumption
    pub fn check_and_consume(env: Env, subject: Address) -> u32 {
        subject.require_auth();
        Self::require_initialized(&env);

        let now = env.ledger().timestamp();
        let limit = Self::get_limit_for(env.clone(), subject.clone());
        let window_seconds = Self::window_seconds(&env);

        let key = StorageKey::Usage(subject.clone());
        let mut usage: Usage = env.storage().persistent().get(&key).unwrap_or(Usage {
            count: 0,
            window_start: now,
        });

        if Self::is_window_expired(usage.window_start, now, window_seconds) {
            usage.count = 0;
            usage.window_start = now;
        }

        assert!(limit > 0, "Rate limit is zero");
        assert!(usage.count < limit, "Rate limit exceeded");

        usage.count = usage.count.saturating_add(1);
        env.storage().persistent().set(&key, &usage);
        limit - usage.count
    }

    /// Explicitly resets the usage for a subject
    ///
    /// @dev Only callable by admin.
    /// @param addr Subject address to reset
    pub fn reset_usage(env: Env, addr: Address) {
        Self::require_admin_auth(&env);
        let now = env.ledger().timestamp();
        let u = Usage {
            count: 0,
            window_start: now,
        };
        env.storage()
            .persistent()
            .set(&StorageKey::Usage(addr), &u);
    }

    // Internal helpers

    fn require_initialized(env: &Env) {
        let initialized: bool = env
            .storage()
            .persistent()
            .get(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(initialized, "Not initialized");
    }

    fn require_admin_auth(env: &Env) {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::Admin)
            .expect("Admin not set");
        admin.require_auth();
    }

    fn window_seconds(env: &Env) -> u64 {
        env.storage()
            .persistent()
            .get(&StorageKey::WindowSeconds)
            .unwrap_or(0u64)
    }

    fn is_window_expired(window_start: u64, now: u64, window_seconds: u64) -> bool {
        now.saturating_sub(window_start) >= window_seconds
    }
}

