#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, Env, String,
};
use stello_pay_contract::PayrollContractClient;

// ============================================================================
// Errors
// ============================================================================

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum OracleError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
    InvalidSource = 4,
    PairNotConfigured = 5,
    RateOutOfBounds = 6,
    RateStale = 7,
    FxUpdateFailed = 8,
}

// ============================================================================
// Types
// ============================================================================

/// Configuration for a `(base, quote)` price pair.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairConfig {
    pub min_rate: i128,
    pub max_rate: i128,
    pub max_staleness_seconds: u64,
    pub enabled: bool,
}

/// Last accepted rate for a `(base, quote)` pair.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairState {
    pub rate: i128,
    pub last_updated_ts: u64,
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
}

#[contract]
pub struct PriceOracleContract;

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

#[contractimpl]
impl PriceOracleContract {
    /// @notice Initializes the price oracle contract.
    /// @dev Must be called exactly once by the protocol owner.
    /// @param owner Administrative owner address.
    /// @param payroll_contract Address of the core payroll contract that will consume FX rates.
    pub fn initialize(env: Env, owner: Address, payroll_contract: Address) -> Result<(), OracleError> {
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
        env.storage()
            .instance()
            .set(&DataKey::Initialized, &true);
        Ok(())
    }

    /// @notice Adds an authorized oracle source.
    /// @param caller Owner address authorizing the source.
    /// @param source Oracle source address (e.g. signer or feeder).
    pub fn add_source(env: Env, caller: Address, source: Address) -> Result<(), OracleError> {
        require_admin(&env, &caller)?;
        env.storage()
            .instance()
            .set(&DataKey::OracleSource(source), &true);
        Ok(())
    }

    /// @notice Removes an authorized oracle source.
    /// @param caller Owner address.
    /// @param source Oracle source to revoke.
    pub fn remove_source(env: Env, caller: Address, source: Address) -> Result<(), OracleError> {
        require_admin(&env, &caller)?;
        env.storage()
            .instance()
            .remove(&DataKey::OracleSource(source));
        Ok(())
    }

    /// @notice Configures bounds and freshness requirements for a `(base, quote)` pair.
    /// @param caller Owner address.
    /// @param base Base token address.
    /// @param quote Quote token address.
    /// @param min_rate Minimum allowed scaled rate (inclusive).
    /// @param max_rate Maximum allowed scaled rate (inclusive).
    /// @param max_staleness_seconds Maximum allowed age of a rate update.
    pub fn configure_pair(
        env: Env,
        caller: Address,
        base: Address,
        quote: Address,
        min_rate: i128,
        max_rate: i128,
        max_staleness_seconds: u64,
    ) -> Result<(), OracleError> {
        require_admin(&env, &caller)?;

        if base == quote || min_rate <= 0 || max_rate <= 0 || min_rate > max_rate {
            return Err(OracleError::RateOutOfBounds);
        }
        if max_staleness_seconds == 0 {
            return Err(OracleError::RateStale);
        }

        let cfg = PairConfig {
            min_rate,
            max_rate,
            max_staleness_seconds,
            enabled: true,
        };

        env.storage()
            .instance()
            .set(&DataKey::PairConfig(base, quote), &cfg);

        Ok(())
    }

    /// @notice Returns the configuration for a `(base, quote)` pair, if any.
    pub fn get_pair_config(env: Env, base: Address, quote: Address) -> Option<PairConfig> {
        env.storage()
            .instance()
            .get(&DataKey::PairConfig(base, quote))
    }

    /// @notice Returns the last accepted state for a `(base, quote)` pair, if any.
    pub fn get_pair_state(env: Env, base: Address, quote: Address) -> Option<PairState> {
        env.storage()
            .instance()
            .get(&DataKey::PairState(base, quote))
    }

    /// @notice Pushes a new price for a `(base, quote)` pair from an authorized source.
    /// @dev On success, this function:
    ///      - validates the rate against configured bounds and staleness,
    ///      - records the last accepted rate and source,
    ///      - calls `set_exchange_rate` on the payroll contract using the oracle
    ///        contract as the FX admin.
    /// @param env Contract environment.
    /// @param source Oracle source address (must be pre-authorized).
    /// @param base Base token address.
    /// @param quote Quote token address.
    /// @param rate Scaled exchange rate (quote_per_base * FX_SCALE).
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

        let mut cfg: PairConfig = env
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
        if source_timestamp > now {
            return Err(OracleError::RateStale);
        }

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

        // Persist new state.
        let new_state = PairState {
            rate,
            last_updated_ts: source_timestamp,
            last_source: source.clone(),
        };
        env.storage()
            .instance()
            .set(&DataKey::PairState(base.clone(), quote.clone()), &new_state);

        // Push FX rate into the payroll contract via its client.
        let payroll_addr = read_payroll_contract(&env);
        let payroll_client = PayrollContractClient::new(&env, &payroll_addr);

        // As FX admin, this contract's address must be registered in the payroll contract
        // via `set_exchange_rate_admin`. The call below uses the oracle's contract address
        // as the authenticated caller.
        let oracle_addr = env.current_contract_address();
        let fx_result = payroll_client.try_set_exchange_rate(&oracle_addr, &base, &quote, &rate);

        match fx_result {
            Ok(Ok(())) => Ok(()),
            _ => Err(OracleError::FxUpdateFailed),
        }
    }

    /// @notice Returns the configured owner.
    pub fn get_owner(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Owner)
    }

    /// @notice Returns whether an address is an authorized oracle source.
    pub fn is_source_address(env: Env, addr: Address) -> bool {
        is_source(&env, &addr)
    }
}

