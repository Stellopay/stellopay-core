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
//!
//! # Outcome and exhaustion
//! [`RateLimiter::check_and_consume`] and
//! [`RateLimiter::check_and_consume_for_contract`] return
//! `Result<ConsumptionOutcome, RateLimitError>`. A successful call reports the
//! subject's remaining allowance and the whole seconds until its bucket next
//! holds a token. An exhausted bucket is reported as
//! [`RateLimitError::RateLimitExceeded`] rather than as an in-band `0` token
//! balance, so callers can tell "served" from "rejected" without reading the
//! implementation.

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env};

/// Typed errors reported at the rate limiter boundary.
///
/// Exhaustion is always reported through this type: a successful call returns
/// [`ConsumptionOutcome`], so a zero token balance can never be mistaken for a
/// rejection (and vice versa).
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum RateLimitError {
    /// At least one enforced bucket (global, per-contract, or per-address) held
    /// no tokens, so the call was rejected. No bucket is debited when this is
    /// returned.
    RateLimitExceeded = 1,
}

#[contracttype]
#[derive(Clone)]
enum StorageKey {
    Admin,
    PendingAdmin,
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

/// Result of a successful [`RateLimiter::check_and_consume`] call.
///
/// Carries the two facts a caller needs after a served request: how much
/// allowance is left, and when the bucket will next admit a request.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsumptionOutcome {
    /// Subject's token balance after this call consumed one token.
    ///
    /// `u32::MAX` signals the admin bypass (the subject is exempt and its
    /// allowance is effectively unlimited).
    pub remaining: u32,
    /// Whole seconds until the subject's bucket next holds at least one token.
    ///
    /// `Some(0)` means a token is available now, `Some(1)` means the bucket is
    /// empty and the next whole-second tick refills it, and `None` means the
    /// bucket is empty and the configured refill rate is zero, so it will never
    /// refill on its own.
    pub refill_in_seconds: Option<u64>,
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
    ///         A bypassed subject is never debited and receives
    ///         `remaining == u32::MAX` (unlimited).
    /// @notice Resolves the subject-specific limit as:
    ///         `set_limit_for(subject, ...)` override first, otherwise the
    ///         default values established during `initialize(...)`.
    /// @dev Refill uses whole ledger seconds only: `elapsed_seconds * refill_rate`.
    ///      Multiple calls in the same ledger second share the same balance and
    ///      do not accumulate fractional refill credit.
    /// @param subject Address to check and consume quota for (must authenticate).
    /// @return `Ok(ConsumptionOutcome)` when the request was served. The outcome
    ///         reports the subject's remaining allowance and the whole seconds
    ///         until its bucket next holds a token.
    /// @return `Err(RateLimitError::RateLimitExceeded)` when the subject's (or an
    ///         enabled global) bucket is empty. **Exhaustion is never signalled
    ///         by a zero return value**, and no bucket is debited on rejection.
    pub fn check_and_consume(
        env: Env,
        subject: Address,
    ) -> Result<ConsumptionOutcome, RateLimitError> {
        Self::check_and_consume_inner(&env, subject, None)
    }

    /// Checks and consumes subject and (when configured) contract budgets.
    ///
    /// @notice Enforces the per-address budget and, if
    ///         [`Self::set_limit_for_contract`] was called for `contract`, the
    ///         shared per-contract budget. Either bucket being exhausted rejects
    ///         the call, so rotating subject addresses within the same contract
    ///         cannot exceed the contract-scoped cap.
    /// @dev All enforced buckets are checked before any is debited, so a
    ///      rejection never partially drains an unrelated budget.
    /// @param subject Address whose per-address quota is consumed.
    /// @param contract Integrating contract whose shared quota is consumed when set.
    /// @return `Ok(ConsumptionOutcome)` when the request was served. `remaining`
    ///         is the **subject's** post-debit balance (not the contract bucket's),
    ///         alongside the subject's `refill_in_seconds`.
    /// @return `Err(RateLimitError::RateLimitExceeded)` when the subject, the
    ///         global, or the configured contract bucket is empty. No bucket is
    ///         debited on rejection.
    pub fn check_and_consume_for_contract(
        env: Env,
        subject: Address,
        contract: Address,
    ) -> Result<ConsumptionOutcome, RateLimitError> {
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

    /// Proposes transferring admin rights to a new address.
    ///
    /// @dev Only callable by current admin. The new admin has no privileges
    ///      until they accept the transfer.
    pub fn transfer_admin(env: Env, new_admin: Address) {
        Self::require_admin_auth(&env);
        env.storage()
            .persistent()
            .set(&StorageKey::PendingAdmin, &new_admin);
    }

    /// Accepts a pending admin transfer.
    ///
    /// @dev Only the pending admin may call this entry point.
    pub fn accept_admin(env: Env, caller: Address) {
        caller.require_auth();
        let pending: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::PendingAdmin)
            .expect("no pending admin");
        assert!(caller == pending, "caller is not pending admin");

        env.storage().persistent().set(&StorageKey::Admin, &caller);
        env.storage().persistent().remove(&StorageKey::PendingAdmin);
    }

    /// Cancels a pending admin transfer.
    ///
    /// @dev Only callable by current admin. Safe no-op when no transfer is pending.
    pub fn cancel_admin_transfer(env: Env) {
        Self::require_admin_auth(&env);
        env.storage().persistent().remove(&StorageKey::PendingAdmin);
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

    fn check_and_consume_inner(
        env: &Env,
        subject: Address,
        contract: Option<Address>,
    ) -> Result<ConsumptionOutcome, RateLimitError> {
        Self::require_initialized(env);

        let admin: Address = env.storage().persistent().get(&StorageKey::Admin).unwrap();
        let bypass: bool = env
            .storage()
            .persistent()
            .get(&StorageKey::AdminBypass)
            .unwrap_or(true);

        // Security assumption: Admin bypass prevents permanent lockout of governance controllers.
        if bypass && subject == admin {
            return Ok(ConsumptionOutcome {
                remaining: u32::MAX,
                refill_in_seconds: Some(0),
            });
        }

        let addr_limit = Self::get_limit_config(env, &subject);
        let addr_key = StorageKey::Usage(subject);

        // Resolve the global bucket, if enabled.
        let global_bucket = if env
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
            Some((
                StorageKey::GlobalUsage,
                Self::bucket_after_refill(env, &StorageKey::GlobalUsage, g_burst, g_refill),
            ))
        } else {
            None
        };

        // Resolve the per-contract bucket when the caller opted in and one is set.
        let contract_bucket = match contract {
            Some(contract_addr) => env
                .storage()
                .persistent()
                .get::<_, LimitConfig>(&StorageKey::ContractLimit(contract_addr.clone()))
                .map(|c_limit| {
                    let c_key = StorageKey::ContractUsage(contract_addr);
                    let c_usage =
                        Self::bucket_after_refill(env, &c_key, c_limit.burst, c_limit.refill_rate);
                    (c_key, c_usage)
                }),
            None => None,
        };

        let addr_usage =
            Self::bucket_after_refill(env, &addr_key, addr_limit.burst, addr_limit.refill_rate);

        // Every enforced bucket is checked before any is debited, so a rejection
        // on one cannot silently drain another (e.g. address rotation vs contract cap).
        let exhausted = global_bucket
            .as_ref()
            .is_some_and(|(_, usage)| usage.tokens < 1)
            || contract_bucket
                .as_ref()
                .is_some_and(|(_, usage)| usage.tokens < 1)
            || addr_usage.tokens < 1;
        if exhausted {
            return Err(RateLimitError::RateLimitExceeded);
        }

        if let Some((key, usage)) = global_bucket {
            Self::debit_bucket(env, &key, usage);
        }
        if let Some((key, usage)) = contract_bucket {
            Self::debit_bucket(env, &key, usage);
        }
        let remaining = Self::debit_bucket(env, &addr_key, addr_usage);

        Ok(ConsumptionOutcome {
            remaining,
            refill_in_seconds: Self::refill_in_seconds(remaining, addr_limit.refill_rate),
        })
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

    /// Debits one token from a bucket already known to hold at least one token
    /// and returns the balance after the debit.
    fn debit_bucket(env: &Env, key: &StorageKey, mut usage: Usage) -> u32 {
        usage.tokens -= 1;
        env.storage().persistent().set(key, &usage);
        usage.tokens
    }

    /// Whole seconds until a bucket with `remaining` tokens next holds at least
    /// one token. A non-empty bucket is usable immediately. An empty bucket
    /// refills on the next whole-second tick when the rate is non-zero, and
    /// never refills (`None`) when the rate is zero. Because refill is credited
    /// per whole second, any `refill_rate >= 1` yields a token within one second.
    fn refill_in_seconds(remaining: u32, refill_rate: u32) -> Option<u64> {
        if remaining > 0 {
            Some(0)
        } else if refill_rate == 0 {
            None
        } else {
            Some(1)
        }
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
