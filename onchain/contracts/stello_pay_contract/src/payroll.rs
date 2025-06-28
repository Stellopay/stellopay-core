use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short,
    token::Client as TokenClient, Address, Env, Symbol,
};
use soroban_sdk::events;

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

/// Key used to store payroll info in contract storage.
#[contracttype]
pub struct PayrollKey(pub Address);

/// Storage keys using symbols instead of unit structs
const PAUSE_KEY: Symbol = symbol_short!("PAUSED");
const OWNER_KEY: Symbol = symbol_short!("OWNER");


#[contracttype]
pub struct SalaryDisbursed {
    pub employer: Address,
    pub employee: Address,
    pub token: Address,
    pub amount: i128,
    pub timestamp: u64,
}

//-----------------------------------------------------------------------------
// Contract Struct
//-----------------------------------------------------------------------------
#[contract]
pub struct PayrollContract;

/// Key used to store employer's token balance in contract storage.
#[contracttype]
#[derive(Clone)]
pub struct EmployerBalanceKey {
    pub employer: Address,
    pub token: Address,
}

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
        storage.set(&PAUSE_KEY, &true);

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
        storage.set(&PAUSE_KEY, &false);

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
        let is_paused = storage.get(&PAUSE_KEY).unwrap_or(false);

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
        storage.set(&DataKey::PayrollLastPayment(employee.clone()), &last_payment_time);

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
        let balance_key = EmployerBalanceKey {
            employer: employer.clone(),
            token: token.clone(),
        };

        // Get current balance or default to 0
        let current_balance: i128 = storage.get(&balance_key).unwrap_or(0);
        let new_balance = current_balance + amount;

        // Update balance in storage
        storage.set(&balance_key, &new_balance);

        // Emit deposit event
        env.events().publish(
            (DEPOSIT_EVENT, employer.clone(), token.clone()), // topics
            (amount, new_balance),                            // data
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
        let balance_key = EmployerBalanceKey {
            employer: employer.clone(),
            token: token.clone(),
        };

        let current_balance: i128 = storage.get(&balance_key).unwrap_or(0);

        if current_balance < amount {
            return Err(PayrollError::InsufficientBalance);
        }

        let new_balance = balance - amount;
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

        let storage = env.storage().persistent();
        let key = PayrollKey(employee.clone());

        if let Some(mut payroll_data) = storage.get::<PayrollKey, Payroll>(&key) {
            if caller != payroll_data.employer {
                return Err(PayrollError::Unauthorized);
            }

            // Require authorization from both employer and employee
            caller.require_auth();

            // Check if enough time has passed since the last payment
            let current_time = env.ledger().timestamp();
            if current_time < payroll_data.last_payment_time + payroll_data.interval {
                return Err(PayrollError::IntervalNotReached);
            }

            // Deduct from employer's salary pool
            Self::deduct_from_balance(
                &env,
                &payroll_data.employer,
                &payroll_data.token,
                payroll_data.amount,
            )?;

            // Handle dispatch transfer from contract to employee
            let token_client = TokenClient::new(&env, &payroll_data.token);
            let contract_address = env.current_contract_address();
            let initial_balance = token_client.balance(&payroll_data.employee);

            // Transfer the amount to the employee
            token_client.transfer(
                &contract_address,
                &payroll_data.employee,
                &payroll_data.amount,
            );

            // Handle transfer failure
            let employee_balance = token_client.balance(&payroll_data.employee);
            if employee_balance != initial_balance + payroll_data.amount {
                return Err(PayrollError::TransferFailed);
            }

            // Update the last payment time
            payroll_data.last_payment_time = current_time;
           
            storage.set(&key, &payroll_data);

            // Emit disbursement event
            env.events().publish(
                ("salary_disbursed",), // topic
                SalaryDisbursed {
                    employer: payroll_data.employer,
                    employee: employee.clone(),
                    token: payroll_data.token,
                    amount: payroll_data.amount,
                    timestamp: current_time,
                },
            );

            Ok(())
        } else {
            Err(PayrollError::PayrollNotFound)
        }
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

        let storage = env.storage().persistent();
        let key = PayrollKey(employee.clone());
        let existing_payroll = storage
            .get::<PayrollKey, Payroll>(&key)
            .ok_or(PayrollError::PayrollNotFound)?;

        // Invoke disburse_salary internally
        Self::disburse_salary(
            env.clone(),
            existing_payroll.employer.clone(),
            employee.clone(),
        )
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
        new_owner.require_auth();

        let storage = env.storage().persistent();

        // Check if caller is the current owner
        if let Some(current_owner) = storage.get::<Symbol, Address>(&OWNER_KEY) {
            if caller != current_owner {
                return Err(PayrollError::Unauthorized);
            }
        } else {
            // Should not happen if initialized
            return Err(PayrollError::Unauthorized);
        }

        // Set new owner
        storage.set(&OWNER_KEY, &new_owner);

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
            token: storage.get(&DataKey::PayrollToken(employee.clone())).unwrap(),
            amount: storage.get(&DataKey::PayrollAmount(employee.clone())).unwrap(),
            interval: storage.get(&DataKey::PayrollInterval(employee.clone())).unwrap(),
            last_payment_time: storage.get(&DataKey::PayrollLastPayment(employee.clone())).unwrap(),
        })
    }
}
