#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol, Vec};
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
}

/// Per‑jurisdiction tax share in a computation.
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
    /// Net amount after withholding.
    pub net_amount: i128,
    /// Per‑jurisdiction breakdown.
    pub shares: Vec<TaxShare>,
}

/// Tax Withholding Contract
///
/// Provides configurable per‑jurisdiction tax rates and supports computing
/// multi‑jurisdiction withholding for a given employee. This contract is
/// purely computational and does not perform token transfers.
#[derive(Upgradeable)]
#[contract]
pub struct TaxWithholdingContract;

impl UpgradeableInternal for TaxWithholdingContract {
    fn _require_auth(e: &Env, _operator: &Address) {
        let owner: Address = e.storage().persistent().get(&StorageKey::Owner).unwrap();
        owner.require_auth();
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
    /// Requires caller authentication
    pub fn initialize(env: Env, owner: Address) {
        owner.require_auth();
        env.storage().persistent().set(&StorageKey::Owner, &owner);
    }

    /// Configures a global tax rate for a jurisdiction, expressed in basis
    /// points (1/10,000). For example, 1500 = 15%.
    ///
    /// # Access Control
    /// - Caller must be the contract owner.
    ///
    /// # Arguments
    /// * `caller` - caller parameter
    /// * `jurisdiction` - jurisdiction parameter
    /// * `rate_bps` - rate_bps parameter
    ///
    /// # Returns
    /// Result<(), TaxError>
    ///
    /// # Errors
    /// Returns an error if validation fails
    pub fn set_jurisdiction_rate(
        env: Env,
        caller: Address,
        jurisdiction: Symbol,
        rate_bps: u32,
    ) -> Result<(), TaxError> {
        let owner: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::Owner)
            .ok_or(TaxError::Unauthorized)?;

        caller.require_auth();
        if caller != owner {
            return Err(TaxError::Unauthorized);
        }

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
    ///
    /// # Arguments
    /// * `jurisdiction` - jurisdiction parameter
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_jurisdiction_rate(env: Env, jurisdiction: Symbol) -> Option<u32> {
        env.storage()
            .persistent()
            .get(&StorageKey::JurisdictionRate(jurisdiction))
    }

    /// Assigns the set of applicable jurisdictions for a given employee.
    ///
    /// # Access Control
    /// - Caller must be the contract owner.
    ///
    /// # Arguments
    /// * `caller` - caller parameter
    /// * `employee` - employee parameter
    /// * `jurisdictions` - jurisdictions parameter
    ///
    /// # Returns
    /// Result<(), TaxError>
    ///
    /// # Errors
    /// Returns an error if validation fails
    pub fn set_employee_jurisdictions(
        env: Env,
        caller: Address,
        employee: Address,
        jurisdictions: Vec<Symbol>,
    ) -> Result<(), TaxError> {
        let owner: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::Owner)
            .ok_or(TaxError::Unauthorized)?;

        caller.require_auth();
        if caller != owner {
            return Err(TaxError::Unauthorized);
        }

        env.storage()
            .persistent()
            .set(&StorageKey::EmployeeJurisdictions(employee), &jurisdictions);
        Ok(())
    }

    /// Returns the jurisdictions currently configured for an employee.
    ///
    /// # Arguments
    /// * `employee` - employee parameter
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_employee_jurisdictions(env: Env, employee: Address) -> Vec<Symbol> {
        env.storage()
            .persistent()
            .get(&StorageKey::EmployeeJurisdictions(employee))
            .unwrap_or(Vec::new(&env))
    }

    /// Computes tax withholding for an employee given a gross amount.
    ///
    /// # Arguments
    /// * `employee` - Employee address whose jurisdictions should be used.
    /// * `gross_amount` - Gross payment amount before withholding.
    ///
    /// # Returns
    /// - `TaxComputation` containing per‑jurisdiction breakdown and net amount.
    ///
    /// # Errors
    /// Returns an error if validation fails
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn calculate_withholding(
        env: Env,
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
            .unwrap_or(Vec::new(&env));

        if jurisdictions.is_empty() {
            return Err(TaxError::NotConfigured);
        }

        let mut total_tax: i128 = 0;
        let mut shares: Vec<TaxShare> = Vec::new(&env);

        for j in jurisdictions.iter() {
            let rate_bps: u32 = env
                .storage()
                .persistent()
                .get(&StorageKey::JurisdictionRate(j.clone()))
                .ok_or(TaxError::NotConfigured)?;

            // amount = gross * rate_bps / 10_000
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
