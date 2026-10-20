use soroban_sdk::{address, contract, contractimpl, contractype, panic_with_error, symbol_short, Address, Env, Val};

/// Error types for the price oracle contract.
#[contracterror]
#[repr(u32)]
pub enum Error {
    /// The contract has already been initialized.
    AlreadyInitialized = 1,
    /// The contract has not been initialized yet.
    NotInitialized = 2,
    /// The caller is not authorized to perform this action.
    Unauthorized = 3,
    /// The price feed has not been set.
    PriceNotSet = 4,
    /// The provided price is invalid (e.g. negative).
    InvalidPrice = 5,
}

/// Event emitted when the price is updated.
#[contracttpe]
#[derive(Clone, Debug, Eq)]
pub struct PriceUpdated {
    pub asset: Symbol,
    pub price: i128,
    pub timestamp: u64,
}

/// Event emitted when the price is queried via get_price_checked.
#[contracttpe]
#[derive(Clone, Debug, Eq)]
pub struct PriceQueried {
    pub asset: Symbol,
    pub price: i128,
    pub timestamp: u64,
}

/// Event emitted when a query fee is charged.
#[contracttype]
#[derive(Clone, Debug, Eq)]
pub struct FeeCharged {
    pub from: Address,
    pub amount: i128,
}

/// Storage keys.
#[contracttype]
#[derive(Clone, Debug, Eq)]
pub enum DataKey {
    Admin,
    Asset,
    Price,
    Timestamp,
    QueryFee,
    FeeRecipient,
    Balance(Address),
}

const PRICE_SCALE: i128 = 10_7;

const FALLBACK_PRICE: i128 = 1_000_000;

#[contract]
pub struct PriceOracle;

/// Returns the admin address.
fn read_admin(env: &Env) -> Result<Address, Error> {
    env.storage().instance().get(&DataKey::Admin).ok_or(Error::NotInitialized)
}

/// Returns the current asset symbol.
fn read_asset(env: &Env) -> Result<Symbol, Error> {
    env.storage().instance().get(&DataKey::Asset).ok_or(Error::PriceNotSet)
}

/// Returns the current price.
fn read_price(env: &Env) -> Result<i128, Error> {
    env.storage().instance().get(&DataKey::Price).ok_or(Error::PriceNotSet)
}

/// Returns the current price timestamp.
fn read_timestamp(env: &Env) -> Result<u64, Error> {
    env.storage().instance().get(&DataKey::Timestamp).ok_or(Error::PriceNotSet)
}

/// Returns the query fee.
fn read_query_fee(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::QueryFee)
        .unwrap_or_default()
}

/// Returns the fee recipient.
fn read_fee_recipient(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::FeeRecipient)
}

/// Returns the token balance of `address`.
fn read_balance(env: &Env, address: &Address) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::Balance(address.clone()))
        .unwrap_or_default()
}

/// Sets the token balance of `address`.
fn write_balance(env: &Env, address: &Address, amount: i128) {
    env.storage()
        .instance()
        .set(&DataKey::Balance(address.clone()), &amount);
}

/// Moves tokens from `from` to `to`. Returns a typed error if the funds are insufficient.
fn transfer_internal(env: &Env, from: &Address, to: &Address, amount: i128) -> Result<(), Error> {
    if amount < 0 {
        return Err(Error::InvalidPrice);
    }
    let from_balance = read_balance(env, from);
    if from_balance < amount {
        return Err(Error::Unauthorized);
    }
    write_balance(env, from, from_balance - amount);
    let to_balance = read_balance(env, to);
    write_balance(env, to, to_balance + amount);
    Ok()
}

#[contractimpl]
impl PriceOracle {
    /// Initializes the contract with an admin, asset symbol, and initial price.
    ///
    /// **Repeat semantics:** calling `initialize` twice returns `Error::AlreadyInitialized`.
    pub fn initialize(
        env: Env,
        admin: Address,
        asset: Symbol,
        price: i128,
        timestamp: u64,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        if price < 0 {
            return Err(Error::InvalidPrice);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Asset, &asset);
        env.storage().instance().set(&DataKey::Price, &price);
        env.storage().instance().set(&DataKey::Timestamp, &timestamp);
        Ok(()
    }

    /// Sets the query fee and fee recipient. Only the admin may call this.
    pub fn set_query_fee(env: Env, fee: i128, recipient: Address) -> Result<(), Error> {
        let admin = read_admin(&env)?;
        admin.require_auth();
        if fee < 0 {
            return Err(Error::InvalidPrice);
        }
        env.storage().instance().set(&DataKey::QueryFee, &fee);
        env.storage()
            .instance()
            .set(&DataKey::FeeRecipient, &recipient);
        Ok(()
    }

    /// Mints tokens to `to`. Only the admin may call this.
    pub fn mint(env: Env, to: Address, amount: i128) -> Result<(), Error> {
        let admin = read_admin(&env)?;
        admin.require_auth();
        if amount < 0 {
            return Err(Error::InvalidPrice);
        }
        let balance = read_balance(&env, &to);
        write_balance(&env, &to, balance + amount);
        Ok(())
    }

    /// Returns the token balance of `address`.
    pub fn balance(env: Env, address: Address) -> i128 {
        read_balance(&env, &address)
    }

    /// Updates the price. Only the admin may call this.
    pub fn set_price(env: Env, price: i128, timestamp: u64) -> Result<(), Error> {
        let admin = read_admin(&env)?;
        admin.require_auth();
        if price < 0 {
            return Err(Error::InvalidPrice);
        }
        let asset = read_asset(&env)?;
        env.storage().instance().set(&DataKey::Price, &price);
        env.storage().instance().set(&DataKey::Timestamp, &timestamp);
        env.events().publish((&&symbol_short("price_up"), ), PriceUpdated { asset, price, timestamp });
        Ok(())
    }

    /// Returns the current price and timestamp without charging a fee.
    pub fn get_price(env: Env) -> Result<(i128, u64), Error> {
        let price = read_price(&env)?;
        let timestamp = read_timestamp(&env)?;
        Ok((price, timestamp))
    }

    /// Returns the current price and timestamp, charging the configured query fee.
    ///
    /// **Repeat semantics:** this function is not idempotent with respect to fees
    /// because each call is a separate query and charges the fee again. However, it
    /// is safe to call twice: the price state is not mutated, and the second call
    /// succeeds if and only if the caller can pay the fee again. If the caller lacks
    /// funds, the second call is refused with `Error::Unauthorized` and no state is
    /// changed. This is the behaviour asserted by the `calling_twice_charges_fee
    /// test.
    pub fn get_price_checked(env: Env, payer: Address) -> Result<(i128, u64), Error> {
        payer.require_auth();
        let price = read_price(&env)?;
        let timestamp = read_timestamp(&env)?;
        let asset = read_asset(&env)?;
        let fee = read_query_fee(&env);
        if fee > 0 {
            let recipient = read_fee_recipient(&env).ok_or(Error::NotInitialized)?;
            transfer_internal(&env, &payer, &recipient, fee)?;
            env.events().publish((&&symbol_short("fee_charg"),), FeeCharged { from: payer.clone(), amount: fee });
        }
        env.events().publish(
            (&&symbol_short("price_qry"),),
            PriceQueried { asset, price, timestamp },
        );
        Ok((price, timestamp))
    }

    /// Returns the admin address.
    pub fn admin(env: Env) -> Result<Address, Error> {
        read_admin(&env)
    }

    /// Returns the configured query fee.
    pub fn query_fee(env: Env) -> i128 {
        read_query_fee(&env)
    }

    /// Returns the configured fee recipient, if any.
    pub fn fee_recipient(env: Env) -> Option<Address> {
        read_fee_recipient(&env)
    }
}

/// Test-only fallback price helper kept for deterministic construction in tests.
pub fn fallback_price() -> i128 {
    FALLBACK_PRICE
}

/// Test-only scale constant exposed for assertions.
pub fn price_scale() -> i128 {
    PRICE_SCALE
}

#[allow(dead_code)]
fn __address_helper(env: &Env, seed: u8) -> Address {
    let mut bytes = [0u8; 32];
    bytes[31] = seed;
    Address::from_array(env, &bytes)
}

#[allow(dead_code)]
fn __panic_helper(env: &Env, msg: &str) -> ! {
    panic_with_error(env, msg)
}
