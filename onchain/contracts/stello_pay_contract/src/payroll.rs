use soroban_sdk::{
    contract, contracterror, contractimpl, symbol_short, token::Client as TokenClient, Address,
    Env, Symbol,
};

use crate::storage::{DataKey, Payroll};

//-----------------------------------------------------------------------------
// Errors
//-----------------------------------------------------------------------------

/// Possible errors for the payroll contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PayrollError {
    /// Raised when a non-employer attempts to call a restricted function.
    Unauthorized = 1,
    /// Raised when the current time has not reached the required interval.
    IntervalNotReached = 2,
    /// Raised when attempting to initialize or disburse with invalid data.
    InvalidData = 3,
    /// Payroll Not Found
    PayrollNotFound = 4,
    /// Transfer Failed
    TransferFailed = 5,
    /// Insufficient Balance
    InsufficientBalance = 6,
    /// Contract is paused
    ContractPaused = 7,
}

//-----------------------------------------------------------------------------
// Data Structures
//-----------------------------------------------------------------------------

/// Storage keys using symbols instead of unit structs

//-----------------------------------------------------------------------------
// Contract Struct
//-----------------------------------------------------------------------------
#[contract]
pub struct PayrollContract;

//-----------------------------------------------------------------------------
// Events
//-----------------------------------------------------------------------------

/// Event emitted when contract is paused
pub const PAUSED_EVENT: Symbol = symbol_short!("paused");

/// Event emitted when contract is unpaused
pub const UNPAUSED_EVENT: Symbol = symbol_short!("unpaused");

/// Event emitted when salary is disbursed
pub const DISBURSE_EVENT: Symbol = symbol_short!("disburse");

/// Event emitted when tokens are deposited to employer's salary pool
pub const DEPOSIT_EVENT: Symbol = symbol_short!("deposit");

//-----------------------------------------------------------------------------
// Contract Implementation
//-----------------------------------------------------------------------------

#[contractimpl]
impl PayrollContract {
    /// Initialize the contract with an owner/admin address
    /// This should be called once when deploying the contract
    pub fn initialize(env: Env, owner: Address) {
        owner.require_auth();

        let storage = env.storage().persistent();

        // Only allow initialization if no owner is set
        if storage.has(&DataKey::Owner) {
            panic!("Contract already initialized");
        }

        storage.set(&DataKey::Owner, &owner);

        // Contract starts unpaused by default
        storage.set(&DataKey::Paused, &false);
    }

    /// Pause the contract - only callable by owner
    pub fn pause(env: Env, caller: Address) -> Result<(), PayrollError> {
        caller.require_auth();

        let storage = env.storage().persistent();

        // Check if caller is the owner
        if let Some(owner) = storage.get::<DataKey, Address>(&DataKey::Owner) {
            if caller != owner {
                return Err(PayrollError::Unauthorized);
            }
        } else {
            return Err(PayrollError::Unauthorized);
        }

        // Set paused state to true
        storage.set(&DataKey::Paused, &true);

        // Emit paused event
        env.events().publish((PAUSED_EVENT,), caller);

        Ok(())
    }

    /// Unpause the contract - only callable by owner
    pub fn unpause(env: Env, caller: Address) -> Result<(), PayrollError> {
        caller.require_auth();

        let storage = env.storage().persistent();

        // Check if caller is the owner
        if let Some(owner) = storage.get::<DataKey, Address>(&DataKey::Owner) {
            if caller != owner {
                return Err(PayrollError::Unauthorized);
            }
        } else {
            return Err(PayrollError::Unauthorized);
        }

        // Set paused state to false
        storage.set(&DataKey::Paused, &false);

        // Emit unpaused event
        env.events().publish((UNPAUSED_EVENT,), caller);

        Ok(())
    }

    /// Check if the contract is paused
    pub fn is_paused(env: Env) -> bool {
        let storage = env.storage().persistent();
        storage.get(&DataKey::Paused).unwrap_or(false)
    }

    /// Internal function to check pause state and panic if paused
    fn require_not_paused(env: &Env) -> Result<(), PayrollError> {
        let storage = env.storage().persistent();
        let is_paused = storage.get(&DataKey::Paused).unwrap_or(false);

        if is_paused {
            return Err(PayrollError::ContractPaused);
        }

        Ok(())
    }

    /// Creates or updates a payroll escrow for production scenarios.
    ///
    /// Requirements:
    /// - Contract must not be paused
    /// - Only the employer can call this method (if updating an existing record).
    /// - Must provide valid interval (> 0).
    /// - Sets `last_payment_time` to current block timestamp when created.
    pub fn create_or_update_escrow(
        env: Env,
        employer: Address,
        employee: Address,
        token: Address,
        amount: i128,
        interval: u64,
    ) -> Result<Payroll, PayrollError> {
        // Check if contract is paused
        Self::require_not_paused(&env)?;

        employer.require_auth();

        let storage = env.storage().persistent();
        let owner = storage.get::<DataKey, Address>(&DataKey::Owner).unwrap();

        let existing_payroll = Self::_get_payroll(&env, &employee);

        if let Some(ref existing) = existing_payroll {
            // For updates, only the contract owner or the existing payroll's employer can call
            if employer != owner && employer != existing.employer {
                return Err(PayrollError::Unauthorized);
            }
        } else if employer != owner {
            // For creation, only the contract owner can call
            return Err(PayrollError::Unauthorized);
        }

        if interval == 0 || amount <= 0 {
            return Err(PayrollError::InvalidData);
        }

        let last_payment_time = if let Some(ref existing) = existing_payroll {
            // If updating, preserve last payment time
            existing.last_payment_time
        } else {
            // If creating, set to current time
            env.ledger().timestamp()
        };

        storage.set(&DataKey::PayrollEmployer(employee.clone()), &employer);
        storage.set(&DataKey::PayrollToken(employee.clone()), &token);
        storage.set(&DataKey::PayrollAmount(employee.clone()), &amount);
        storage.set(&DataKey::PayrollInterval(employee.clone()), &interval);
        storage.set(
            &DataKey::PayrollLastPayment(employee.clone()),
            &last_payment_time,
        );

        Ok(Payroll {
            employer,
            token,
            amount,
            interval,
            last_payment_time,
        })
    }

    /// Deposit tokens to employer's salary pool
    ///
    /// Requirements:
    /// - Contract must not be paused
    /// - Only the employer can deposit to their own pool
    /// - Amount must be positive
    pub fn deposit_tokens(
        env: Env,
        employer: Address,
        token: Address,
        amount: i128,
    ) -> Result<(), PayrollError> {
        Self::require_not_paused(&env)?;
        employer.require_auth();

        if amount <= 0 {
            return Err(PayrollError::InvalidData);
        }

        // Transfer tokens from employer to contract
        let token_client = TokenClient::new(&env, &token);
        let initial_balance = token_client.balance(&env.current_contract_address());
        token_client.transfer(&employer, &env.current_contract_address(), &amount);

        // Handle transfer failure
        if token_client.balance(&env.current_contract_address()) < initial_balance + amount {
            return Err(PayrollError::TransferFailed);
        }

        let storage = env.storage().persistent();
        let balance_key = DataKey::Balance(employer.clone(), token.clone());

        // Get current balance or default to 0
        let current_balance: i128 = storage.get(&balance_key).unwrap_or(0);
        let new_balance = current_balance + amount;

        // Update balance in storage
        storage.set(&balance_key, &new_balance);

        env.events().publish(
            (DEPOSIT_EVENT, employer, token), // topics
            amount,                           // data
        );

        Ok(())
    }

    /// Get employer's token balance in the contract
    pub fn get_employer_balance(env: Env, employer: Address, token: Address) -> i128 {
        let storage = env.storage().persistent();
        storage.get(&DataKey::Balance(employer, token)).unwrap_or(0)
    }

    /// Internal function to deduct from employer's balance
    fn deduct_from_balance(
        env: &Env,
        employer: &Address,
        token: &Address,
        amount: i128,
    ) -> Result<(), PayrollError> {
        let storage = env.storage().persistent();
        let balance_key = DataKey::Balance(employer.clone(), token.clone());

        let current_balance: i128 = storage.get(&balance_key).unwrap_or(0);

        if current_balance < amount {
            return Err(PayrollError::InsufficientBalance);
        }

        let new_balance = current_balance - amount;
        storage.set(&balance_key, &new_balance);

        Ok(())
    }

    /// Disburse salary to an employee.
    ///
    /// Requirements:
    /// - Contract must not be paused
    /// - Can be called by anyone
    /// - Payroll must exist for the employee
    /// - Time since last payment must be >= interval
    pub fn disburse_salary(
        env: Env,
        caller: Address,
        employee: Address,
    ) -> Result<(), PayrollError> {
        // Check if contract is paused
        Self::require_not_paused(&env)?;

        caller.require_auth();

        let storage = env.storage().persistent();
        let payroll = Self::_get_payroll(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;

        // Only the employer can disburse salary
        if caller != payroll.employer {
            return Err(PayrollError::Unauthorized);
        }

        // Check if payment interval has elapsed
        let current_time = env.ledger().timestamp();
        if current_time < payroll.last_payment_time + payroll.interval {
            return Err(PayrollError::IntervalNotReached);
        }

        // Deduct from employer's salary pool
        Self::deduct_from_balance(&env, &payroll.employer, &payroll.token, payroll.amount)?;

        // Handle dispatch transfer from contract to employee
        let token_client = TokenClient::new(&env, &payroll.token);
        let contract_address = env.current_contract_address();
        let initial_balance = token_client.balance(&employee);

        // Transfer the amount to the employee
        token_client.transfer(&contract_address, &employee, &payroll.amount);

        // Handle transfer failure
        let employee_balance = token_client.balance(&employee);
        if employee_balance != initial_balance + payroll.amount {
            return Err(PayrollError::TransferFailed);
        }

        // Update the last payment time
        storage.set(
            &DataKey::PayrollLastPayment(employee.clone()),
            &current_time,
        );

        // Emit disbursement event
        env.events().publish(
            (DISBURSE_EVENT,),
            (payroll.employer, employee, payroll.token, payroll.amount),
        );

        Ok(())
    }

    /// Get payroll information for an employee.
    pub fn get_payroll(env: Env, employee: Address) -> Option<Payroll> {
        Self::_get_payroll(&env, &employee)
    }

    /// Allows an employee to withdraw their salary.
    /// This is an alternative to `disburse_salary` where the employee initiates the transaction.
    pub fn employee_withdraw(env: Env, employee: Address) -> Result<(), PayrollError> {
        // Check if contract is paused
        Self::require_not_paused(&env)?;

        employee.require_auth();

        let payroll = Self::_get_payroll(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;

        // Invoke disburse_salary internally
        Self::disburse_salary(env.clone(), payroll.employer.clone(), employee.clone())
    }

    /// Get the owner of the contract
    pub fn get_owner(env: Env) -> Option<Address> {
        env.storage().persistent().get(&DataKey::Owner)
    }

    /// Transfer ownership to a new address - only callable by current owner
    pub fn transfer_ownership(
        env: Env,
        caller: Address,
        new_owner: Address,
    ) -> Result<(), PayrollError> {
        caller.require_auth();

        let storage = env.storage().persistent();

        // Check if caller is the current owner
        if let Some(owner) = storage.get::<DataKey, Address>(&DataKey::Owner) {
            if caller != owner {
                return Err(PayrollError::Unauthorized);
            }
        } else {
            // Should not happen if initialized
            return Err(PayrollError::Unauthorized);
        }

        // Set new owner
        storage.set(&DataKey::Owner, &new_owner);

        Ok(())
    }

    fn _get_payroll(env: &Env, employee: &Address) -> Option<Payroll> {
        let storage = env.storage().persistent();
        let employer_key = DataKey::PayrollEmployer(employee.clone());

        if !storage.has(&employer_key) {
            return None;
        }

        Some(Payroll {
            employer: storage.get(&employer_key).unwrap(),
            token: storage
                .get(&DataKey::PayrollToken(employee.clone()))
                .unwrap(),
            amount: storage
                .get(&DataKey::PayrollAmount(employee.clone()))
                .unwrap(),
            interval: storage
                .get(&DataKey::PayrollInterval(employee.clone()))
                .unwrap(),
            last_payment_time: storage
                .get(&DataKey::PayrollLastPayment(employee.clone()))
                .unwrap(),
        })
    }
}
