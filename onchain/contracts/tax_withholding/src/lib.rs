#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, token, Address, Env, Symbol, Vec};
use stellar_contract_utils::upgradeable::UpgradeableInternal;
use stellar_macros::Upgradeable;

/// Error codes for tax withholding operations.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum TaxError {
    /// Caller is not authorized to configure tax settings.
    Unauthorized = 1,
    /// Tax rate parameters are invalid (e.g. > 100%).
    InvalidRate = 2,
    /// No tax configuration found for the requested entity.
    NotConfigured = 3,
    /// Arithmetic overflow/underflow during tax calculation.
    ArithmeticError = 4,
    /// No treasury address configured for this jurisdiction.
    TreasuryNotSet = 5,
    /// Accrued balance is zero; nothing to remit.
    NothingToRemit = 6,
}

/// Storage keys for the tax withholding contract.
#[contracttype]
#[derive(Clone)]
pub enum StorageKey {
    /// Contract owner (global admin).
    Owner,
    /// Global jurisdiction tax rate in basis points (0–10_000).
    JurisdictionRate(Symbol),
    /// Employee-specific jurisdictions (subset of global jurisdictions).
    EmployeeJurisdictions(Address),
    /// Fixed treasury address per jurisdiction for remittance.
    /// Only the owner may set this — prevents redirection to arbitrary addresses.
    JurisdictionTreasury(Symbol),
    /// Accumulated unremitted withholding balance per jurisdiction.
    AccruedWithholding(Symbol),
}

/// Per-jurisdiction tax share in a computation.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaxShare {
    /// Jurisdiction identifier (e.g. "US_CA", "EU_DE").
    pub jurisdiction: Symbol,
    /// Withheld amount for this jurisdiction.
    pub amount: i128,
}

/// Result of a tax withholding calculation.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaxComputation {
    /// Gross amount before withholding.
    pub gross_amount: i128,
    /// Total tax withheld across all jurisdictions.
    pub total_tax: i128,
    /// Net amount after withholding (employee take-home pay).
    pub net_amount: i128,
    /// Per-jurisdiction breakdown.
    pub shares: Vec<TaxShare>,
}

/// Emitted when withholding is accrued for a pay period.
///
/// Consumers (indexers, UI, other contracts) use this event to track the
/// employer's growing tax liability without inspecting storage directly.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithholdingAccruedEvent {
    /// Employee whose gross pay triggered the accrual.
    pub employee: Address,
    /// Gross pay amount for this pay period.
    pub gross_amount: i128,
    /// Total amount withheld across all jurisdictions.
    pub total_tax: i128,
    /// Net pay (gross minus total_tax) — employee's take-home amount.
    pub net_amount: i128,
}

/// Emitted when accrued withholding is remitted to the treasury.
///
/// Rounding note: all withholding amounts are floored toward the treasury
/// (`floor(gross × rate_bps / 10_000)`). Any sub-unit remainder is retained
/// by the employee in their net pay, protecting employees from over-withholding.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithholdingRemittedEvent {
    /// Jurisdiction whose accrued balance was remitted.
    pub jurisdiction: Symbol,
    /// Amount transferred to the treasury.
    pub amount: i128,
    /// Treasury address that received the funds.
    pub treasury: Address,
}

/// Tax Withholding Contract
///
/// Provides configurable per-jurisdiction tax rates, accrual tracking, and
/// remittance hooks. Withheld liabilities are clearly separated from employee
/// net pay.
///
/// # Security Model
///
/// * Only the **owner** can configure rates, treasury addresses, employee
///   jurisdictions, and trigger remittances.
/// * Treasury addresses are owner-controlled; no other caller can redirect
///   withheld funds to an arbitrary address.
/// * Accrued state is updated **before** token transfers (state-before-interaction).
/// * All arithmetic uses `checked_*` operations to prevent overflow/underflow.
///
/// # Rounding Policy
///
/// Withholding is computed as `floor(gross × rate_bps / 10_000)`. Fractional
/// units are always retained in the employee's net pay, never rounded up to
/// the treasury. This protects employees from over-withholding.
#[derive(Upgradeable)]
#[contract]
pub struct TaxWithholdingContract;

impl UpgradeableInternal for TaxWithholdingContract {
    fn _require_auth(e: &Env, _operator: &Address) {
        let owner: Address = e.storage().persistent().get(&StorageKey::Owner).unwrap();
        owner.require_auth();
    }
}

/// Private helpers — not exported as contract entry points.
impl TaxWithholdingContract {
    fn require_owner(env: &Env, caller: &Address) -> Result<(), TaxError> {
        let owner: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::Owner)
            .ok_or(TaxError::Unauthorized)?;
        if *caller != owner {
            return Err(TaxError::Unauthorized);
        }
        Ok(())
    }

    /// Core withholding calculation shared by `calculate_withholding` and
    /// `accrue_withholding`.
    fn compute_withholding(
        env: &Env,
        employee: Address,
        gross_amount: i128,
    ) -> Result<TaxComputation, TaxError> {
        if gross_amount <= 0 {
            return Err(TaxError::ArithmeticError);
        }

        let jurisdictions: Vec<Symbol> = env
            .storage()
            .persistent()
            .get(&StorageKey::EmployeeJurisdictions(employee))
            .unwrap_or(Vec::new(env));

        if jurisdictions.is_empty() {
            return Err(TaxError::NotConfigured);
        }

        let mut total_tax: i128 = 0;
        let mut shares: Vec<TaxShare> = Vec::new(env);

        for j in jurisdictions.iter() {
            let rate_bps: u32 = env
                .storage()
                .persistent()
                .get(&StorageKey::JurisdictionRate(j.clone()))
                .ok_or(TaxError::NotConfigured)?;

            // Rounding: floor toward treasury (any remainder stays with employee).
            let part = gross_amount
                .checked_mul(rate_bps as i128)
                .ok_or(TaxError::ArithmeticError)?
                .checked_div(10_000)
                .ok_or(TaxError::ArithmeticError)?;

            if part < 0 {
                return Err(TaxError::ArithmeticError);
            }

            total_tax = total_tax
                .checked_add(part)
                .ok_or(TaxError::ArithmeticError)?;

            shares.push_back(TaxShare {
                jurisdiction: j.clone(),
                amount: part,
            });
        }

        if total_tax > gross_amount {
            return Err(TaxError::ArithmeticError);
        }

        let net_amount = gross_amount
            .checked_sub(total_tax)
            .ok_or(TaxError::ArithmeticError)?;

        Ok(TaxComputation {
            gross_amount,
            total_tax,
            net_amount,
            shares,
        })
    }
}

#[contractimpl]
impl TaxWithholdingContract {
    /// Initializes the tax withholding contract.
    ///
    /// # Arguments
    /// * `owner` - Address with permission to configure global tax rates.
    ///
    /// # Access Control
    /// Requires owner authentication.
    pub fn initialize(env: Env, owner: Address) {
        owner.require_auth();
        env.storage().persistent().set(&StorageKey::Owner, &owner);
    }

    /// Configures a global tax rate for a jurisdiction, expressed in basis
    /// points (1/10,000). For example, 1500 = 15%.
    ///
    /// # Access Control
    /// Caller must be the contract owner.
    ///
    /// # Errors
    /// * `Unauthorized` — caller is not the owner.
    /// * `InvalidRate` — `rate_bps > 10_000`.
    pub fn set_jurisdiction_rate(
        env: Env,
        caller: Address,
        jurisdiction: Symbol,
        rate_bps: u32,
    ) -> Result<(), TaxError> {
        caller.require_auth();
        Self::require_owner(&env, &caller)?;

        if rate_bps > 10_000 {
            return Err(TaxError::InvalidRate);
        }

        env.storage()
            .persistent()
            .set(&StorageKey::JurisdictionRate(jurisdiction), &rate_bps);
        Ok(())
    }

    /// Returns the configured tax rate in basis points for a jurisdiction,
    /// or `None` if not configured.
    pub fn get_jurisdiction_rate(env: Env, jurisdiction: Symbol) -> Option<u32> {
        env.storage()
            .persistent()
            .get(&StorageKey::JurisdictionRate(jurisdiction))
    }

    /// Sets the treasury address for a jurisdiction.
    ///
    /// Accrued withholding for this jurisdiction is remitted to this address.
    ///
    /// # Security
    /// Only the owner can set this. Callers cannot redirect withheld funds to
    /// arbitrary addresses.
    ///
    /// # Access Control
    /// Caller must be the contract owner.
    ///
    /// # Errors
    /// * `Unauthorized` — caller is not the owner.
    pub fn set_jurisdiction_treasury(
        env: Env,
        caller: Address,
        jurisdiction: Symbol,
        treasury: Address,
    ) -> Result<(), TaxError> {
        caller.require_auth();
        Self::require_owner(&env, &caller)?;

        env.storage()
            .persistent()
            .set(&StorageKey::JurisdictionTreasury(jurisdiction), &treasury);
        Ok(())
    }

    /// Returns the treasury address configured for a jurisdiction, or `None`.
    pub fn get_jurisdiction_treasury(env: Env, jurisdiction: Symbol) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&StorageKey::JurisdictionTreasury(jurisdiction))
    }

    /// Assigns the set of applicable jurisdictions for a given employee.
    ///
    /// # Access Control
    /// Caller must be the contract owner.
    ///
    /// # Errors
    /// * `Unauthorized` — caller is not the owner.
    pub fn set_employee_jurisdictions(
        env: Env,
        caller: Address,
        employee: Address,
        jurisdictions: Vec<Symbol>,
    ) -> Result<(), TaxError> {
        caller.require_auth();
        Self::require_owner(&env, &caller)?;

        env.storage()
            .persistent()
            .set(&StorageKey::EmployeeJurisdictions(employee), &jurisdictions);
        Ok(())
    }

    /// Returns the jurisdictions currently configured for an employee.
    pub fn get_employee_jurisdictions(env: Env, employee: Address) -> Vec<Symbol> {
        env.storage()
            .persistent()
            .get(&StorageKey::EmployeeJurisdictions(employee))
            .unwrap_or(Vec::new(&env))
    }

    /// Computes tax withholding for an employee given a gross amount (pure read).
    ///
    /// Does not modify state. Use `accrue_withholding` to record an actual
    /// pay period and update the running liability balances.
    ///
    /// # Arguments
    /// * `employee`     — Employee address whose jurisdictions are used.
    /// * `gross_amount` — Gross payment amount before withholding.
    ///
    /// # Errors
    /// * `ArithmeticError` — `gross_amount <= 0` or overflow.
    /// * `NotConfigured`   — employee has no jurisdictions, or a jurisdiction
    ///                        has no rate set.
    pub fn calculate_withholding(
        env: Env,
        employee: Address,
        gross_amount: i128,
    ) -> Result<TaxComputation, TaxError> {
        Self::compute_withholding(&env, employee, gross_amount)
    }

    /// Accrual hook: records withholding for a completed pay period.
    ///
    /// Adds each jurisdiction's withheld share to the running
    /// `AccruedWithholding` balance for that jurisdiction. Call this once per
    /// pay cycle after gross pay is determined.
    ///
    /// # Arguments
    /// * `caller`       — Must be the contract owner (e.g. payroll processor).
    /// * `employee`     — Employee whose jurisdictions determine the split.
    /// * `gross_amount` — Gross pay for this period.
    ///
    /// # Returns
    /// `TaxComputation` with the per-jurisdiction breakdown and net amount.
    ///
    /// # Events
    /// Emits `("withholding_accrued",)` with a [`WithholdingAccruedEvent`].
    ///
    /// # Access Control
    /// Caller must be the contract owner.
    ///
    /// # Errors
    /// * `Unauthorized`    — caller is not the owner.
    /// * `ArithmeticError` — overflow or non-positive gross amount.
    /// * `NotConfigured`   — employee or jurisdiction not set up.
    pub fn accrue_withholding(
        env: Env,
        caller: Address,
        employee: Address,
        gross_amount: i128,
    ) -> Result<TaxComputation, TaxError> {
        caller.require_auth();
        Self::require_owner(&env, &caller)?;

        let computation = Self::compute_withholding(&env, employee.clone(), gross_amount)?;

        for share in computation.shares.iter() {
            let key = StorageKey::AccruedWithholding(share.jurisdiction.clone());
            let prev: i128 = env.storage().persistent().get(&key).unwrap_or(0);
            let new_balance = prev
                .checked_add(share.amount)
                .ok_or(TaxError::ArithmeticError)?;
            env.storage().persistent().set(&key, &new_balance);
        }

        env.events().publish(
            ("withholding_accrued",),
            WithholdingAccruedEvent {
                employee,
                gross_amount: computation.gross_amount,
                total_tax: computation.total_tax,
                net_amount: computation.net_amount,
            },
        );

        Ok(computation)
    }

    /// Returns the current accrued (unremitted) withholding balance for a
    /// jurisdiction.
    pub fn get_accrued_balance(env: Env, jurisdiction: Symbol) -> i128 {
        env.storage()
            .persistent()
            .get(&StorageKey::AccruedWithholding(jurisdiction))
            .unwrap_or(0)
    }

    /// Remittance hook: transfers the full accrued balance for a jurisdiction
    /// to its configured treasury.
    ///
    /// State is updated **before** the token transfer (state-before-interaction
    /// pattern) to prevent re-entrancy.
    ///
    /// # Arguments
    /// * `caller`       — Must be the contract owner. Tokens are transferred
    ///                    **from** this address, so the caller must hold the
    ///                    accrued amount in `token`.
    /// * `jurisdiction` — Jurisdiction whose balance is being remitted.
    /// * `token`        — Token contract address for the transfer.
    ///
    /// # Returns
    /// Amount remitted.
    ///
    /// # Events
    /// Emits `("withholding_remitted",)` with a [`WithholdingRemittedEvent`].
    ///
    /// # Security
    /// The destination (`treasury`) is read from owner-controlled storage.
    /// No caller can redirect withheld funds to an arbitrary address.
    ///
    /// # Access Control
    /// Caller must be the contract owner.
    ///
    /// # Errors
    /// * `Unauthorized`   — caller is not the owner.
    /// * `TreasuryNotSet` — no treasury configured for the jurisdiction.
    /// * `NothingToRemit` — accrued balance is zero.
    pub fn remit_withholding(
        env: Env,
        caller: Address,
        jurisdiction: Symbol,
        token: Address,
    ) -> Result<i128, TaxError> {
        caller.require_auth();
        Self::require_owner(&env, &caller)?;

        let treasury: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::JurisdictionTreasury(jurisdiction.clone()))
            .ok_or(TaxError::TreasuryNotSet)?;

        let key = StorageKey::AccruedWithholding(jurisdiction.clone());
        let amount: i128 = env.storage().persistent().get(&key).unwrap_or(0);

        if amount == 0 {
            return Err(TaxError::NothingToRemit);
        }

        // Update state BEFORE external token transfer (state-before-interaction).
        env.storage().persistent().set(&key, &0i128);

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&caller, &treasury, &amount);

        env.events().publish(
            ("withholding_remitted",),
            WithholdingRemittedEvent {
                jurisdiction,
                amount,
                treasury,
            },
        );

        Ok(amount)
    }
}
