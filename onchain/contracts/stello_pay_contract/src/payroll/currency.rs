//! Currency conversion domain: FX rate administration and amount conversion.

use crate::events::{emit_exchange_rate_updated, ExchangeRateUpdatedEvent};
use crate::storage::{DataKey, PayrollError, StorageKey};
use soroban_sdk::{Address, Env};

/// Fixed-point scaling factor for FX rates: 1e6 precision.
const FX_SCALE: i128 = 1_000_000;

/// Minimum converted amount (in quote-token base units) below which the
/// conversion is treated as pure dust and rejected.
///
/// Rounding policy: `convert_amount` uses **floor division** (truncation toward
/// zero). Any remainder is discarded. If truncation reduces the converted amount
/// to zero the call returns `ExchangeRateInvalid` so callers are not silently
/// credited nothing. Callers that need to claim very small amounts should
/// accumulate multiple periods before claiming.
const DUST_THRESHOLD: i128 = 1;

/// Sets the global FX rate admin address that is allowed to update exchange
/// rates in addition to the contract owner (e.g. an oracle contract).
pub fn set_exchange_rate_admin(
    env: &Env,
    caller: Address,
    admin: Address,
) -> Result<(), PayrollError> {
    let owner: Address = env
        .storage()
        .persistent()
        .get(&StorageKey::Owner)
        .ok_or(PayrollError::Unauthorized)?;

    caller.require_auth();

    if caller != owner {
        return Err(PayrollError::Unauthorized);
    }

    env.storage()
        .persistent()
        .set(&StorageKey::ExchangeRateAdmin, &admin);

    Ok(())
}

/// Configures the FX rate for a `(base, quote)` token pair.
///
/// Access control:
/// - Contract owner OR
/// - FX admin set via `set_exchange_rate_admin`
pub fn set_exchange_rate(
    env: &Env,
    caller: Address,
    base: Address,
    quote: Address,
    rate: i128,
) -> Result<(), PayrollError> {
    if rate <= 0 || base == quote {
        return Err(PayrollError::ExchangeRateInvalid);
    }

    caller.require_auth();

    let owner: Option<Address> = env.storage().persistent().get(&StorageKey::Owner);
    let fx_admin: Option<Address> = env
        .storage()
        .persistent()
        .get(&StorageKey::ExchangeRateAdmin);

    let is_authorized = match (owner, fx_admin) {
        (Some(o), _) if caller == o => true,
        (_, Some(a)) if caller == a => true,
        _ => false,
    };

    if !is_authorized {
        return Err(PayrollError::Unauthorized);
    }

    // Enforce absolute sanity bound if configured.
    if let Some(max_rate) = DataKey::get_exchange_rate_max_rate_sanity_bound(env) {
        if rate > max_rate {
            return Err(PayrollError::ExchangeRateInvalid);
        }
    }

    // Enforce max-deviation if configured: compare with previous rate.
    if let Some(max_dev_bps) = DataKey::get_exchange_rate_max_deviation_bps(env) {
        if let Some(prev) = DataKey::get_exchange_rate(env, &base, &quote) {
            // compute allowed delta = prev.rate * max_dev_bps / 10000
            let prev_rate = prev.rate;
            // Avoid negative or zero prev_rate (shouldn't happen)
            if prev_rate > 0 {
                let allowed_delta = (prev_rate
                    .checked_mul(max_dev_bps as i128)
                    .unwrap_or(i128::MAX))
                .checked_div(10_000i128)
                .unwrap_or(i128::MAX);
                let diff = if rate > prev_rate {
                    rate - prev_rate
                } else {
                    prev_rate - rate
                };
                if diff > allowed_delta {
                    return Err(PayrollError::ExchangeRateInvalid);
                }
            }
        }
    }

    let prev_rate = DataKey::get_exchange_rate(env, &base, &quote)
        .map(|r| r.rate)
        .unwrap_or(0);

    DataKey::set_exchange_rate(env, &base, &quote, rate);

    emit_exchange_rate_updated(
        env,
        ExchangeRateUpdatedEvent {
            base,
            quote,
            new_rate: rate,
            prev_rate,
            updater: caller,
            updated_at: env.ledger().timestamp(),
        },
    );

    Ok(())
}

/// Default conservative maximum acceptable age for exchange rates in seconds (1 hour = 3600s)
/// used when the caller does not specify `max_rate_age_seconds` and no contract-wide max age is set.
pub const DEFAULT_MAX_RATE_AGE_SECONDS: u64 = 3600;

/// Pure conversion helper exposed as a contract entry point so off-chain
/// clients can query expected converted amounts without performing a transfer.
///
/// Caller can supply a `max_rate_age_seconds` bound and acceptable min/max output amounts.
/// If `max_rate_age_seconds` is omitted (`None`), it falls back to the contract-wide
/// max age setting or `DEFAULT_MAX_RATE_AGE_SECONDS`.
pub fn convert_currency(
    env: &Env,
    from_token: Address,
    to_token: Address,
    amount: i128,
    max_rate_age_seconds: Option<u64>,
    min_output_amount: Option<i128>,
    max_output_amount: Option<i128>,
) -> Result<i128, PayrollError> {
    if amount == 0 || from_token == to_token {
        if matches!(min_output_amount, Some(min_out) if amount < min_out) {
            return Err(PayrollError::ExchangeRateInvalid);
        }
        if matches!(max_output_amount, Some(max_out) if amount > max_out) {
            return Err(PayrollError::ExchangeRateInvalid);
        }
        return Ok(amount);
    }

    let info = DataKey::get_exchange_rate(env, &from_token, &to_token)
        .ok_or(PayrollError::ExchangeRateNotFound)?;

    let max_age = max_rate_age_seconds
        .or_else(|| DataKey::get_exchange_rate_max_age_seconds(env))
        .unwrap_or(DEFAULT_MAX_RATE_AGE_SECONDS);

    let now = env.ledger().timestamp();
    if now < info.updated_at {
        return Err(PayrollError::ExchangeRateInvalid);
    }
    if now - info.updated_at > max_age {
        return Err(PayrollError::ExchangeRateInvalid);
    }

    let converted = convert_amount(env, &from_token, &to_token, amount)?;

    if matches!(min_output_amount, Some(min_out) if converted < min_out) {
        return Err(PayrollError::ExchangeRateInvalid);
    }

    if matches!(max_output_amount, Some(max_out) if converted > max_out) {
        return Err(PayrollError::ExchangeRateInvalid);
    }

    Ok(converted)
}

/// Internal helper: convert `amount` from `from_token` into `to_token` using
/// Convert `amount` from `from_token` units into `to_token` units using
/// the configured FX rate stored in `DataKey::ExchangeRate`.
///
/// # Rate encoding
///
/// The rate is a fixed-point integer interpreted as
/// `quote_per_base * FX_SCALE` (where `FX_SCALE = 1_000_000`).  Examples:
/// - `rate = 1_000_000` → 1 base = 1 quote (1:1 parity)
/// - `rate = 2_000_000` → 1 base = 2 quote
/// - `rate =   500_000` → 1 base = 0.5 quote
///
/// # Rounding convention — floor (truncation toward zero)
///
/// The conversion uses **integer floor division**:
/// ```text
/// converted = (amount * rate) / FX_SCALE   (truncated, not rounded)
/// ```
/// Any fractional remainder is silently discarded.  Callers that require
/// exact amounts should ensure `amount * rate` is divisible by `FX_SCALE`,
/// or accumulate multiple periods before claiming so the truncated dust
/// remains negligible relative to the total payout.
///
/// # Dust guard — conversion-to-zero is rejected
///
/// If the floor-division result is less than `DUST_THRESHOLD` (= 1), the
/// function returns `Err(PayrollError::ExchangeRateInvalid)` instead of
/// crediting zero tokens.  This prevents a scenario where:
/// 1. An employee claims a period.
/// 2. The claimed period is marked as paid (`claimed_periods` increments).
/// 3. The employee receives **zero** tokens — effectively burning the salary.
///
/// Concretely: a `salary_per_period` of 1 with a sub-parity rate
/// (e.g. `rate = 999_999`) gives `(1 * 999_999) / 1_000_000 = 0`, which
/// triggers the dust guard.  The employee must either accumulate more periods
/// or use a larger salary so the conversion yields ≥ 1 quote unit.
///
/// # Errors
///
/// | Error | Condition |
/// |---|---|
/// | `ExchangeRateNotFound` | No rate configured for the pair, or rate is stale |
/// | `ExchangeRateInvalid` | Rate ≤ 0, timestamp inconsistency, or result < `DUST_THRESHOLD` |
/// | `ExchangeRateOverflow` | `amount * rate` overflows `i128` |
pub(super) fn convert_amount(
    env: &Env,
    from_token: &Address,
    to_token: &Address,
    amount: i128,
) -> Result<i128, PayrollError> {
    if amount == 0 || from_token == to_token {
        return Ok(amount);
    }

    let info = DataKey::get_exchange_rate(env, from_token, to_token)
        .ok_or(PayrollError::ExchangeRateNotFound)?;

    let rate = info.rate;
    if rate <= 0 {
        return Err(PayrollError::ExchangeRateInvalid);
    }

    // Enforce staleness (max-age) if configured
    if let Some(max_age) = DataKey::get_exchange_rate_max_age_seconds(env) {
        let now = env.ledger().timestamp();
        // Protect against underflow
        if now < info.updated_at {
            return Err(PayrollError::ExchangeRateInvalid);
        }
        if now - info.updated_at > max_age {
            return Err(PayrollError::ExchangeRateNotFound);
        }
    }

    let scaled = amount
        .checked_mul(rate)
        .ok_or(PayrollError::ExchangeRateOverflow)?;

    let converted = scaled
        .checked_div(FX_SCALE)
        .ok_or(PayrollError::ExchangeRateInvalid)?;

    // Dust guard: reject conversions that floor-round to zero to prevent
    // callers from being silently credited nothing for a non-zero input.
    if converted < DUST_THRESHOLD {
        return Err(PayrollError::ExchangeRateInvalid);
    }

    Ok(converted)
}
