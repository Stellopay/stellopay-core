#![no_std]

pub mod storage;
mod payroll;

use soroban_sdk::{contract, contractimpl, Address, Env, Error};
use payroll::{claim_payroll, get_employee_claimed_periods};

/// Payroll Contract for managing payroll agreements with employee claiming functionality.
///
/// This contract supports:
/// - Multiple employees per agreement with individual salary tracking
/// - Per-employee period tracking for claimed salaries
/// - Employee-initiated payroll claiming based on elapsed time periods
/// - Secure escrow fund release
/// - Grace period support for claims
#[contract]
pub struct PayrollContract;

#[contractimpl]
impl PayrollContract {
    /// One-time initialization hook.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment
    /// * `owner` - The contract owner address
    pub fn initialize(env: Env, owner: Address) {
        owner.require_auth();
        // Placeholder: store `owner` in persistent storage when implementing access control.
        let _ = env;
    }

    /// Claim payroll for an employee in a payroll agreement.
    ///
    /// Allows an employee to claim their salary based on elapsed time periods since
    /// the agreement was activated. Each employee has individual period tracking,
    /// allowing independent claiming.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment
    /// * `agreement_id` - The unique identifier for the payroll agreement
    /// * `employee_index` - The index of the employee in the agreement (0-based)
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or a `PayrollError` on failure.
    ///
    /// # Errors
    ///
    /// * `PayrollError::Unauthorized` - If the caller is not the employee at the given index
    /// * `PayrollError::InvalidEmployeeIndex` - If the employee index is out of bounds
    /// * `PayrollError::AgreementNotFound` - If the agreement doesn't exist or isn't activated
    /// * `PayrollError::InsufficientEscrowBalance` - If there aren't enough funds in escrow
    /// * `PayrollError::NoPeriodsToClaim` - If there are no periods available to claim
    /// * `PayrollError::TransferFailed` - If the token transfer fails
    ///
    /// # Events
    ///
    /// Emits `PayrollClaimed`, `PaymentSent`, and `PaymentReceived` events on success.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Employee at index 0 claims their payroll
    /// let employee = Address::from_str("...");
    /// employee.require_auth();
    /// contract.claim_payroll(&env, employee, 1u128, 0u32)?;
    /// ```
    pub fn claim_payroll(env: Env, caller: Address, agreement_id: u128, employee_index: u32) -> Result<(), Error> {
        caller.require_auth();
        claim_payroll(&env, &caller, agreement_id, employee_index).map_err(Into::into)
    }

    /// Get the number of periods already claimed by an employee.
    ///
    /// This function returns the individual claimed period count for a specific employee
    /// within an agreement, enabling independent tracking per employee.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment
    /// * `agreement_id` - The unique identifier for the payroll agreement
    /// * `employee_index` - The index of the employee in the agreement (0-based)
    ///
    /// # Returns
    ///
    /// Returns the number of claimed periods (0 if none have been claimed).
    ///
    /// # Example
    ///
    /// ```ignore
    /// let claimed = contract.get_employee_claimed_periods(&env, 1u128, 0u32);
    /// println!("Employee has claimed {} periods", claimed);
    /// ```
    pub fn get_employee_claimed_periods(env: Env, agreement_id: u128, employee_index: u32) -> u32 {
        get_employee_claimed_periods(&env, agreement_id, employee_index)
    }
}
