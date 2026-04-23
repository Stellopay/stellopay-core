#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env,
};
use stello_pay_contract::PayrollContractClient;

// ============================================================================
// Errors
// ============================================================================

/// @notice Exhaustive error codes emitted by the price oracle.
/// @dev Each variant maps to a unique u32 for off-chain indexing.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum OracleError {
    /// Contract has not been initialized yet.
    NotInitialized = 1,
    /// `initialize` was called more than once.
    AlreadyInitialized = 2,
    /// Caller lacks the required permission (not owner).
    NotAuthorized = 3,
    /// Caller is not a registered oracle source.
    InvalidSource = 4,
    /// The `(base, quote)` pair is not configured or is disabled.
    PairNotConfigured = 5,
    /// Submitted rate falls outside configured `[min_rate, max_rate]`.
    RateOutOfBounds = 6,
    /// Rate is stale (too old) or has a future timestamp.
    RateStale = 7,
    /// The downstream `set_exchange_rate` call on the payroll contract failed.
    FxUpdateFailed = 8,
    /// A zero or negative rate was submitted.
    ZeroRate = 9,
    /// Pair configuration parameters are invalid (e.g. min > max, zero staleness).
    InvalidPairConfig = 10,
}

// ============================================================================
// Types
// ============================================================================

/// Configuration for a `(base, quote)` price pair.
///
/// @dev All rates use a fixed-point representation with 6 decimal places
///      (i.e. `1_000_000` = 1.0). This is referred to as `FX_SCALE` throughout.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairConfig {
    /// Minimum allowed scaled rate (inclusive). Must be > 0.
    pub min_rate: i128,
    /// Maximum allowed scaled rate (inclusive). Must be >= min_rate.
    pub max_rate: i128,
    /// Maximum allowed age (in seconds) of a source timestamp relative to
    /// the ledger timestamp. Updates older than this are rejected.
    pub max_staleness_seconds: u64,
    /// Whether the pair accepts new price updates.
    pub enabled: bool,
    /// Minimum number of unique authorized sources required to accept a price.
    pub quorum: u32,
}

/// Last accepted rate for a `(base, quote)` pair.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairState {
    /// The most recent accepted rate (scaled by FX_SCALE).
    pub rate: i128,
    /// Timestamp (seconds) of the accepted update.
    pub last_updated_ts: u64,
    /// Address of the oracle source that submitted the accepted rate.
    pub last_source: Address,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Initialized,
    Owner,
    /// Address of the core payroll contract to which FX rates are pushed.
    PayrollContract,
    /// Authorized oracle sources (address -> bool).
    OracleSource(Address),
    /// Static configuration for a `(base, quote)` pair.
    PairConfig(Address, Address),
    /// Last accepted state for a `(base, quote)` pair.
    PairState(Address, Address),
    /// Tracking individual source votes for quorum: (base, quote, timestamp, rate, source).
    QuorumVote(Address, Address, u64, i128, Address),
    /// Tracking aggregate vote counts for quorum: (base, quote, timestamp, rate).
    QuorumCount(Address, Address, u64, i128),
}

#[contract]
pub struct PriceOracleContract;

// ============================================================================
// Internal helpers
// ============================================================================

fn require_initialized(env: &Env) -> Result<(), OracleError> {
    let init = env
        .storage()
        .instance()
        .get::<_, bool>(&DataKey::Initialized)
        .unwrap_or(false);
    if !init {
        return Err(OracleError::NotInitialized);
    }
    Ok(())
}

fn read_owner(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Owner)
        .expect("owner not set")
}

fn read_payroll_contract(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::PayrollContract)
        .expect("payroll not set")
}

fn is_source(env: &Env, addr: &Address) -> bool {
    env.storage()
        .instance()
        .get::<_, bool>(&DataKey::OracleSource(addr.clone()))
        .unwrap_or(false)
}

/// @dev Requires caller to be the contract owner. Returns error otherwise.
fn require_admin(env: &Env, caller: &Address) -> Result<(), OracleError> {
    require_initialized(env)?;
    caller.require_auth();
    let owner = read_owner(env);
    if &owner == caller {
        Ok(())
    } else {
        Err(OracleError::NotAuthorized)
    }
}

// ============================================================================
// Contract implementation
// ============================================================================

#[contractimpl]
impl PriceOracleContract {
    // ------------------------------------------------------------------------
    // Initialization
    // ------------------------------------------------------------------------

    /// @notice Initializes the price oracle contract.
    /// @dev Must be called exactly once by the protocol owner.
    ///      Emits event `("oracle", "init")` with the owner address.
    /// @param owner            Administrative owner address.
    /// @param payroll_contract Address of the core payroll contract that
    ///                         will consume FX rates.
    /// @return Result<(), OracleError>
    pub fn initialize(
        env: Env,
        owner: Address,
        payroll_contract: Address,
    ) -> Result<(), OracleError> {
        if env
            .storage()
            .instance()
            .get::<_, bool>(&DataKey::Initialized)
            .unwrap_or(false)
        {
            return Err(OracleError::AlreadyInitialized);
        }

        owner.require_auth();
        env.storage().instance().set(&DataKey::Owner, &owner);
        env.storage()
            .instance()
            .set(&DataKey::PayrollContract, &payroll_contract);
        env.storage().instance().set(&DataKey::Initialized, &true);

        env.events()
            .publish((symbol_short!("oracle"), symbol_short!("init")), &owner);

        Ok(())
    }

    // ------------------------------------------------------------------------
    // Source management
    // ------------------------------------------------------------------------

    /// @notice Adds an authorized oracle source.
    /// @dev Only the contract owner may call this.
    ///      Emits event `("oracle", "addsrc")` with the source address.
    /// @param caller Owner address authorizing the source.
    /// @param source Oracle source address (e.g. off-chain signer or feeder).
    pub fn add_source(env: Env, caller: Address, source: Address) -> Result<(), OracleError> {
        require_admin(&env, &caller)?;
        env.storage()
            .instance()
            .set(&DataKey::OracleSource(source.clone()), &true);

        env.events()
            .publish((symbol_short!("oracle"), symbol_short!("addsrc")), &source);

        Ok(())
    }

    /// @notice Removes an authorized oracle source.
    /// @dev Only the contract owner may call this. A removed source can no
    ///      longer push prices. Existing rates published by this source remain.
    ///      Emits event `("oracle", "rmsrc")` with the source address.
    /// @param caller Owner address.
    /// @param source Oracle source to revoke.
    pub fn remove_source(env: Env, caller: Address, source: Address) -> Result<(), OracleError> {
        require_admin(&env, &caller)?;
        env.storage()
            .instance()
            .remove(&DataKey::OracleSource(source.clone()));

        env.events()
            .publish((symbol_short!("oracle"), symbol_short!("rmsrc")), &source);

        Ok(())
    }

    // ------------------------------------------------------------------------
    // Pair management
    // ------------------------------------------------------------------------

    /// @notice Configures bounds and freshness requirements for a `(base, quote)` pair.
    /// @dev Validates that:
    ///      - `base != quote`
    ///      - `min_rate > 0` and `max_rate > 0`
    ///      - `min_rate <= max_rate`
    ///      - `max_staleness_seconds > 0`
    ///      Emits event `("oracle", "cfgpair")` with `(base, quote)`.
    /// @param caller               Owner address.
    /// @param base                 Base token address.
    /// @param quote                Quote token address.
    /// @param min_rate             Minimum allowed scaled rate (inclusive).
    /// @param max_rate             Maximum allowed scaled rate (inclusive).
    /// @param max_staleness_seconds Maximum allowed age of a rate update.
    pub fn configure_pair(
        env: Env,
        caller: Address,
        base: Address,
        quote: Address,
        min_rate: i128,
        max_rate: i128,
        max_staleness_seconds: u64,
        quorum: u32,
    ) -> Result<(), OracleError> {
        require_admin(&env, &caller)?;

        if base == quote || min_rate <= 0 || max_rate <= 0 || min_rate > max_rate {
            return Err(OracleError::InvalidPairConfig);
        }
        if max_staleness_seconds == 0 || quorum == 0 {
            return Err(OracleError::InvalidPairConfig);
        }

        let cfg = PairConfig {
            min_rate,
            max_rate,
            max_staleness_seconds,
            enabled: true,
            quorum,
        };

        env.storage()
            .instance()
            .set(&DataKey::PairConfig(base.clone(), quote.clone()), &cfg);

        env.events().publish(
            (symbol_short!("oracle"), symbol_short!("cfgpair")),
            (&base, &quote),
        );

        Ok(())
    }

    /// @notice Disables a `(base, quote)` pair so it no longer accepts updates.
    /// @dev The configuration is preserved but `enabled` is set to false.
    ///      Emits event `("oracle", "disable")` with `(base, quote)`.
    /// @param caller Owner address.
    /// @param base   Base token address.
    /// @param quote  Quote token address.
    pub fn disable_pair(
        env: Env,
        caller: Address,
        base: Address,
        quote: Address,
    ) -> Result<(), OracleError> {
        require_admin(&env, &caller)?;

        let mut cfg: PairConfig = env
            .storage()
            .instance()
            .get(&DataKey::PairConfig(base.clone(), quote.clone()))
            .ok_or(OracleError::PairNotConfigured)?;

        cfg.enabled = false;
        env.storage()
            .instance()
            .set(&DataKey::PairConfig(base.clone(), quote.clone()), &cfg);

        env.events().publish(
            (symbol_short!("oracle"), symbol_short!("disable")),
            (&base, &quote),
        );

        Ok(())
    }

    /// @notice Re-enables a previously disabled `(base, quote)` pair.
    /// @dev Emits event `("oracle", "enable")` with `(base, quote)`.
    /// @param caller Owner address.
    /// @param base   Base token address.
    /// @param quote  Quote token address.
    pub fn enable_pair(
        env: Env,
        caller: Address,
        base: Address,
        quote: Address,
    ) -> Result<(), OracleError> {
        require_admin(&env, &caller)?;

        let mut cfg: PairConfig = env
            .storage()
            .instance()
            .get(&DataKey::PairConfig(base.clone(), quote.clone()))
            .ok_or(OracleError::PairNotConfigured)?;

        cfg.enabled = true;
        env.storage()
            .instance()
            .set(&DataKey::PairConfig(base.clone(), quote.clone()), &cfg);

        env.events().publish(
            (symbol_short!("oracle"), symbol_short!("enable")),
            (&base, &quote),
        );

        Ok(())
    }

    // ------------------------------------------------------------------------
    // Price submission
    // ------------------------------------------------------------------------

    /// @notice Pushes a new price for a `(base, quote)` pair from an authorized source.
    /// @dev On success this function:
    ///      1. Validates the source is registered.
    ///      2. Validates the pair is configured and enabled.
    ///      3. Rejects zero or negative rates.
    ///      4. Validates the rate against configured `[min_rate, max_rate]`.
    ///      5. Rejects future timestamps (`source_timestamp > ledger.timestamp`).
    ///      6. Rejects stale timestamps (age > `max_staleness_seconds`).
    ///      7. Ignores updates older than or equal to the last accepted timestamp
    ///         (monotonic ordering).
    ///      8. Persists the new `PairState`.
    ///      9. Calls `set_exchange_rate` on the downstream payroll contract.
    ///      Emits event `("oracle", "price")` with `(base, quote, rate)`.
    /// @param source           Oracle source address (must be pre-authorized).
    /// @param base             Base token address.
    /// @param quote            Quote token address.
    /// @param rate             Scaled exchange rate (quote_per_base * FX_SCALE).
    /// @param source_timestamp Timestamp associated with the external price (seconds).
    pub fn push_price(
        env: Env,
        source: Address,
        base: Address,
        quote: Address,
        rate: i128,
        source_timestamp: u64,
    ) -> Result<(), OracleError> {
        require_initialized(&env)?;

        if !is_source(&env, &source) {
            return Err(OracleError::InvalidSource);
        }

        // Reject zero or negative rates.
        if rate <= 0 {
            return Err(OracleError::ZeroRate);
        }

        let cfg: PairConfig = env
            .storage()
            .instance()
            .get(&DataKey::PairConfig(base.clone(), quote.clone()))
            .ok_or(OracleError::PairNotConfigured)?;

        if !cfg.enabled {
            return Err(OracleError::PairNotConfigured);
        }

        // Bounds checks.
        if rate < cfg.min_rate || rate > cfg.max_rate {
            return Err(OracleError::RateOutOfBounds);
        }

        let now = env.ledger().timestamp();

        // Reject future timestamps.
        if source_timestamp > now {
            return Err(OracleError::RateStale);
        }

        // Reject stale timestamps.
        let age = now.saturating_sub(source_timestamp);
        if age > cfg.max_staleness_seconds {
            return Err(OracleError::RateStale);
        }

        // Monotonic update: ignore strictly older timestamps than the last accepted one.
        if let Some(state) = env
            .storage()
            .instance()
            .get::<_, PairState>(&DataKey::PairState(base.clone(), quote.clone()))
        {
            if source_timestamp <= state.last_updated_ts {
                // Older or equal update; treat as no-op.
                return Ok(());
            }
        }

        // Quorum check if enabled (> 1).
        if cfg.quorum > 1 {
            let vote_key = DataKey::QuorumVote(
                base.clone(),
                quote.clone(),
                source_timestamp,
                rate,
                source.clone(),
            );
            if env.storage().temporary().has(&vote_key) {
                // Source already submitted this exact rate/timestamp; ignore.
                return Ok(());
            }

            env.storage().temporary().set(&vote_key, &true);

            let count_key =
                DataKey::QuorumCount(base.clone(), quote.clone(), source_timestamp, rate);
            let current_count: u32 = env.storage().temporary().get(&count_key).unwrap_or(0);
            let new_count = current_count + 1;
            env.storage().temporary().set(&count_key, &new_count);

            if new_count < cfg.quorum {
                // Quorum not yet reached.
                return Ok(());
            }
        }

        // Persist new state.
        let new_state = PairState {
            rate,
            last_updated_ts: source_timestamp,
            last_source: source.clone(),
        };
        env.storage()
            .instance()
            .set(&DataKey::PairState(base.clone(), quote.clone()), &new_state);

        env.events().publish(
            (symbol_short!("oracle"), symbol_short!("price")),
            (&base, &quote, rate),
        );

        // Push FX rate into the payroll contract via its client.
        let payroll_addr = read_payroll_contract(&env);
        let payroll_client = PayrollContractClient::new(&env, &payroll_addr);

        let oracle_addr = env.current_contract_address();
        let fx_result = payroll_client.try_set_exchange_rate(&oracle_addr, &base, &quote, &rate);

        match fx_result {
            Ok(Ok(())) => Ok(()),
            _ => Err(OracleError::FxUpdateFailed),
        }
    }

    // ------------------------------------------------------------------------
    // Queries
    // ------------------------------------------------------------------------

    /// @notice Returns the configuration for a `(base, quote)` pair, if any.
    /// @param base Base token address.
    /// @param quote Quote token address.
    pub fn get_pair_config(env: Env, base: Address, quote: Address) -> Option<PairConfig> {
        env.storage()
            .instance()
            .get(&DataKey::PairConfig(base, quote))
    }

    /// @notice Returns the last accepted state for a `(base, quote)` pair, if any.
    /// @param base Base token address.
    /// @param quote Quote token address.
    pub fn get_pair_state(env: Env, base: Address, quote: Address) -> Option<PairState> {
        env.storage()
            .instance()
            .get(&DataKey::PairState(base, quote))
    }

    /// @notice Returns the configured owner.
    pub fn get_owner(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Owner)
    }

    /// @notice Returns whether an address is an authorized oracle source.
    /// @param addr Address to check.
    pub fn is_source_address(env: Env, addr: Address) -> bool {
        is_source(&env, &addr)
    }

    // ------------------------------------------------------------------------
    // Admin transfer
    // ------------------------------------------------------------------------

    /// @notice Transfers contract ownership to a new address.
    /// @dev Only the current owner may call this. The new owner immediately
    ///      takes effect (single-step for simplicity; the price oracle is
    ///      a lighter-weight contract than the RBAC core).
    ///      Emits event `("oracle", "owner")` with the new owner address.
    /// @param caller    Current owner; must authenticate.
    /// @param new_owner New owner address.
    pub fn transfer_ownership(
        env: Env,
        caller: Address,
        new_owner: Address,
    ) -> Result<(), OracleError> {
        require_admin(&env, &caller)?;
        env.storage().instance().set(&DataKey::Owner, &new_owner);

        env.events().publish(
            (symbol_short!("oracle"), symbol_short!("owner")),
            &new_owner,
        );

        Ok(())
    }
}
