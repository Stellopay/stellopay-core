#![no_std]

//! Per-address, per-contract, and global rate limiting using Token Bucket.
//!
//! Provides burst-friendly rate limiting with automatic token refills,
//! global throttling, per-contract throughput caps, and admin bypass to
//! ensure security and fairness.
//!
//! # Per-contract vs per-address budgets
//! [`RateLimiter::set_limit_for`] configures a bucket for one subject address.
//! [`RateLimiter::set_limit_for_contract`] configures a separate bucket for an
//! integrating (calling) contract. When consuming via
//! [`RateLimiter::check_and_consume_for_contract`], **both** buckets are
//! checked: exhausting either one rejects the call. Address rotation inside
//! the same contract therefore cannot bypass the contract-scoped budget.
//!
//! # Fractional Refill Policy
//! Soroban ledger timestamps are whole seconds and bucket balances are whole
//! `u32` tokens. Refill is therefore calculated as
//! `elapsed_seconds * refill_rate` using integer arithmetic. Calls made inside
//! the same ledger second receive no partial or fractional refill credit.

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
    /// Per-contract override: calling contract -> LimitConfig
    ContractLimit(Address),
    /// Per-contract usage: calling contract -> Usage
    ContractUsage(Address),
}

/// Usage state for a token bucket.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Usage {
    /// Last whole-second ledger timestamp when tokens were refilled.
    pub last_update: u64,
    /// Current whole-token balance in the bucket.
    pub tokens: u32,
}

/// Configuration for a specific rate limit.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LimitConfig {
    /// Maximum tokens the bucket can hold (burst capacity)
    pub burst: u32,
    /// Whole tokens added to the bucket per whole ledger second.
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
        env.storage()
            .persistent()
            .set(&StorageKey::DefaultBurst, &default_burst);
        env.storage()
            .persistent()
            .set(&StorageKey::DefaultRefillRate, &default_refill_rate);
        env.storage()
            .persistent()
            .set(&StorageKey::AdminBypass, &admin_bypass);
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);
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
        env.storage()
            .persistent()
            .set(&StorageKey::GlobalLimitEnabled, &enabled);
        env.storage()
            .persistent()
            .set(&StorageKey::GlobalBurst, &burst);
        env.storage()
            .persistent()
            .set(&StorageKey::GlobalRefillRate, &refill_rate);
    }

    /// Sets a per-address limit override.
    ///
    /// @notice Per-address overrides take precedence over the initialized
    ///         default limit for the target address.
    /// @dev Only callable by admin.
    /// @param addr Subject address.
    /// @param burst Max burst capacity for this address.
    /// @param refill_rate Tokens added per second for this address.
    pub fn set_limit_for(env: Env, addr: Address, burst: u32, refill_rate: u32) {
        Self::require_admin_auth(&env);
        env.storage().persistent().set(
            &StorageKey::Limit(addr),
            &LimitConfig { burst, refill_rate },
        );
    }

    /// Removes a per-address limit override.
    ///
    /// @dev Only callable by admin.
    pub fn clear_limit_for(env: Env, addr: Address) {
        Self::require_admin_auth(&env);
        env.storage().persistent().remove(&StorageKey::Limit(addr));
    }

    /// Sets a per-contract throughput budget.
    ///
    /// @notice Caps total consumption across all subject addresses that call
    ///         through this integrating contract. Distinct from
    ///         [`Self::set_limit_for`]: both budgets are enforced together by
    ///         [`Self::check_and_consume_for_contract`].
    /// @dev Only callable by admin.
    /// @param contract Calling / integrating contract address.
    /// @param burst Max burst capacity shared by all subjects via this contract.
    /// @param refill_rate Tokens added per second to the contract bucket.
    pub fn set_limit_for_contract(env: Env, contract: Address, burst: u32, refill_rate: u32) {
        Self::require_admin_auth(&env);
        env.storage().persistent().set(
            &StorageKey::ContractLimit(contract),
            &LimitConfig { burst, refill_rate },
        );
    }

    /// Removes a per-contract throughput budget.
    ///
    /// @notice Does not reset contract usage; call [`Self::reset_contract_usage`]
    ///         if a fresh bucket is needed.
    /// @dev Only callable by admin. Safe no-op when no budget was configured.
    pub fn clear_limit_for_contract(env: Env, contract: Address) {
        Self::require_admin_auth(&env);
        env.storage()
            .persistent()
            .remove(&StorageKey::ContractLimit(contract));
    }

    /// Checks and consumes one whole token from the subject's rate limit.
    ///
    /// @notice Implements Token Bucket algorithm for burst handling.
    /// @notice Validates security by allowing admins to bypass if configured.
    /// @notice Resolves the subject-specific limit as:
    ///         `set_limit_for(subject, ...)` override first, otherwise the
    ///         default values established during `initialize(...)`.
    /// @dev Refill uses whole ledger seconds only: `elapsed_seconds * refill_rate`.
    ///      Multiple calls in the same ledger second share the same balance and
    ///      do not accumulate fractional refill credit.
    /// @param subject Address to check and consume quota for (must authenticate).
    /// @return tokens_remaining User's tokens remaining after consumption.
    pub fn check_and_consume(env: Env, subject: Address) -> u32 {
        Self::check_and_consume_inner(&env, subject, None)
    }

    /// Checks and consumes subject and (when configured) contract budgets.
    ///
    /// @notice Enforces the per-address budget and, if
    ///         [`Self::set_limit_for_contract`] was called for `contract`, the
    ///         shared per-contract budget. Either bucket being exhausted rejects
    ///         the call, so rotating subject addresses within the same contract
    ///         cannot exceed the contract-scoped cap.
    /// @param subject Address whose per-address quota is consumed.
    /// @param contract Integrating contract whose shared quota is consumed when set.
    /// @return tokens_remaining Subject's tokens remaining after consumption.
    pub fn check_and_consume_for_contract(env: Env, subject: Address, contract: Address) -> u32 {
        Self::check_and_consume_inner(&env, subject, Some(contract))
    }

    /// Explicitly resets usage for an address.
    ///
    /// @dev Only callable by admin.
    pub fn reset_usage(env: Env, addr: Address) {
        Self::require_admin_auth(&env);
        env.storage().persistent().remove(&StorageKey::Usage(addr));
    }

    /// Explicitly resets usage for a contract-scoped bucket.
    ///
    /// @dev Only callable by admin.
    pub fn reset_contract_usage(env: Env, contract: Address) {
        Self::require_admin_auth(&env);
        env.storage()
            .persistent()
            .remove(&StorageKey::ContractUsage(contract));
    }

    /// Transfers admin rights to a new address.
    ///
    /// @dev Only callable by current admin.
    pub fn transfer_admin(env: Env, new_admin: Address) {
        Self::require_admin_auth(&env);
        env.storage()
            .persistent()
            .set(&StorageKey::Admin, &new_admin);
    }

    /// Gets current config for an address.
    pub fn get_limit_for(env: Env, addr: Address) -> LimitConfig {
        Self::get_limit_config(&env, &addr)
    }

    /// Gets the configured per-contract budget, if any.
    ///
    /// @return `None` when no contract-scoped budget has been set (contract
    ///         bucket is not enforced until [`Self::set_limit_for_contract`]).
    pub fn get_limit_for_contract(env: Env, contract: Address) -> Option<LimitConfig> {
        env.storage()
            .persistent()
            .get(&StorageKey::ContractLimit(contract))
    }

    /// Returns the current usage state for an address without consuming tokens.
    ///
    /// # Read-Only Semantics
    /// This is a purely observational query. It computes the token refill based on
    /// elapsed time since the last update but does **not** mutate any state.
    /// No authentication is required.
    ///
    /// # Returns
    /// - `Some(Usage)` — the current token count and last-update timestamp, with refill applied up
    ///   to the current ledger time.
    /// - `None` — if no usage has ever been recorded for this address (the bucket is effectively
    ///   full at the configured burst capacity).
    pub fn get_usage(env: Env, addr: Address) -> Option<Usage> {
        env.storage()
            .persistent()
            .get(&StorageKey::Usage(addr.clone()))
            .map(|usage: Usage| {
                let now = env.ledger().timestamp();
                let config = Self::get_limit_config(&env, &addr);
                Self::preview_refill(usage, now, config.burst, config.refill_rate)
            })
    }

    /// Returns the current contract-scoped usage without consuming tokens.
    ///
    /// @return `None` if no contract usage has been recorded yet, or if no
    ///         contract budget is configured (nothing to preview against).
    pub fn get_contract_usage(env: Env, contract: Address) -> Option<Usage> {
        let config: LimitConfig = env
            .storage()
            .persistent()
            .get(&StorageKey::ContractLimit(contract.clone()))?;
        env.storage()
            .persistent()
            .get(&StorageKey::ContractUsage(contract))
            .map(|usage: Usage| {
                let now = env.ledger().timestamp();
                Self::preview_refill(usage, now, config.burst, config.refill_rate)
            })
    }

    /// Gets effective admin address.
    pub fn get_admin(env: Env) -> Option<Address> {
        env.storage().persistent().get(&StorageKey::Admin)
    }

    // Internal helpers

    fn check_and_consume_inner(env: &Env, subject: Address, contract: Option<Address>) -> u32 {
        Self::require_initialized(env);

        let admin: Address = env.storage().persistent().get(&StorageKey::Admin).unwrap();
        let bypass: bool = env
            .storage()
            .persistent()
            .get(&StorageKey::AdminBypass)
            .unwrap_or(true);

        // Security assumption: Admin bypass prevents permanent lockout of governance controllers.
        if bypass && subject == admin {
            return u32::MAX;
        }

        // 1. Check Global Limit (if enabled)
        if env
            .storage()
            .persistent()
            .get(&StorageKey::GlobalLimitEnabled)
            .unwrap_or(false)
        {
            let g_burst = env
                .storage()
                .persistent()
                .get(&StorageKey::GlobalBurst)
                .unwrap_or(0);
            let g_refill = env
                .storage()
                .persistent()
                .get(&StorageKey::GlobalRefillRate)
                .unwrap_or(0);
            Self::consume_bucket(env, StorageKey::GlobalUsage, g_burst, g_refill);
        }

        // 2–3. Per-contract (when configured) and per-address budgets.
        // Both are checked before either is debited so a rejection on one
        // cannot silently drain the other (e.g. address rotation vs contract cap).
        let addr_limit = Self::get_limit_config(env, &subject);
        let addr_key = StorageKey::Usage(subject);

        if let Some(contract_addr) = contract {
            let c_limit: Option<LimitConfig> = env
                .storage()
                .persistent()
                .get(&StorageKey::ContractLimit(contract_addr.clone()));
            if let Some(c_limit) = c_limit {
                let c_key = StorageKey::ContractUsage(contract_addr);
                let c_usage = Self::bucket_after_refill(env, &c_key, c_limit.burst, c_limit.refill_rate);
                let a_usage =
                    Self::bucket_after_refill(env, &addr_key, addr_limit.burst, addr_limit.refill_rate);
                assert!(c_usage.tokens >= 1, "rate limit exceeded");
                assert!(a_usage.tokens >= 1, "rate limit exceeded");
                Self::debit_bucket(env, &c_key, c_usage);
                return Self::debit_bucket(env, &addr_key, a_usage);
            }
        }

        Self::consume_bucket(env, addr_key, addr_limit.burst, addr_limit.refill_rate)
    }

    fn preview_refill(usage: Usage, now: u64, burst: u32, refill_rate: u32) -> Usage {
        let elapsed = now.saturating_sub(usage.last_update);
        if elapsed > 0 {
            let new_tokens = (elapsed as u32).saturating_mul(refill_rate);
            let tokens = usage.tokens.saturating_add(new_tokens);
            Usage {
                last_update: now,
                tokens: if tokens > burst { burst } else { tokens },
            }
        } else {
            usage
        }
    }

    fn bucket_after_refill(env: &Env, key: &StorageKey, burst: u32, refill_rate: u32) -> Usage {
        let now = env.ledger().timestamp();
        let usage: Usage = env.storage().persistent().get(key).unwrap_or(Usage {
            last_update: now,
            tokens: burst,
        });
        Self::preview_refill(usage, now, burst, refill_rate)
    }

    fn debit_bucket(env: &Env, key: &StorageKey, mut usage: Usage) -> u32 {
        assert!(usage.tokens >= 1, "rate limit exceeded");
        usage.tokens -= 1;
        env.storage().persistent().set(key, &usage);
        usage.tokens
    }

    fn consume_bucket(env: &Env, key: StorageKey, burst: u32, refill_rate: u32) -> u32 {
        let usage = Self::bucket_after_refill(env, &key, burst, refill_rate);
        Self::debit_bucket(env, &key, usage)
    }

    fn get_limit_config(env: &Env, addr: &Address) -> LimitConfig {
        env.storage()
            .persistent()
            .get(&StorageKey::Limit(addr.clone()))
            .unwrap_or_else(|| LimitConfig {
                burst: env
                    .storage()
                    .persistent()
                    .get(&StorageKey::DefaultBurst)
                    .unwrap_or(0),
                refill_rate: env
                    .storage()
                    .persistent()
                    .get(&StorageKey::DefaultRefillRate)
                    .unwrap_or(0),
            })
    }

    fn is_initialized(env: &Env) -> bool {
        env.storage()
            .persistent()
            .get(&StorageKey::Initialized)
            .unwrap_or(false)
    }

    fn require_initialized(env: &Env) {
        assert!(Self::is_initialized(env), "not initialized");
    }

    fn require_admin_auth(env: &Env) {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::Admin)
            .expect("admin not set");
        admin.require_auth();
    }
}
