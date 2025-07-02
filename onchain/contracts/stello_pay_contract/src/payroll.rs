use soroban_sdk::{
    contract, contracterror, contractimpl, symbol_short, token::Client as TokenClient, Address,
    Env, Symbol, Vec,
};

use crate::storage::{DataKey, Payroll};
use crate::events::{
    PAUSED_EVENT, UNPAUSED_EVENT, DEPOSIT_EVENT, emit_disburse,
};

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
    /// Recurrence frequency is invalid (must be > 0)
    InvalidRecurrenceFrequency = 8,
    /// Next payout time has not been reached
    NextPayoutTimeNotReached = 9,
    /// No eligible employees for recurring disbursement
    NoEligibleEmployees = 10,
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



/// Event emitted when recurring disbursements are processed
pub const RECUR_EVENT: Symbol = symbol_short!("recur");

/// Event emitted when payroll is created or updated with recurrence
pub const UPDATED_EVENT: Symbol = symbol_short!("updated");

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

    pub fn add_supported_token(env: Env, token: Address) -> Result<(), PayrollError> {
        let storage = env.storage().persistent();
        let owner = storage.get::<DataKey, Address>(&DataKey::Owner).unwrap();
        owner.require_auth();

        let key = DataKey::SupportedToken(token.clone());
        storage.set(&key, &true);

        let token_client = TokenClient::new(&env, &token);
        let decimals = token_client.decimals();
        let metadata_key = DataKey::TokenMetadata(token.clone());
        storage.set(&metadata_key, &decimals);

        Ok(())
    }

    /// Remove a supported token - only callable by owner
    pub fn remove_supported_token(env: Env, token: Address) -> Result<(), PayrollError> {
        let storage = env.storage().persistent();
        let owner = storage.get::<DataKey, Address>(&DataKey::Owner).unwrap();
        owner.require_auth();

        let key = DataKey::SupportedToken(token.clone());
        storage.set(&key, &false);

        let metadata_key = DataKey::TokenMetadata(token.clone());
        storage.remove(&metadata_key);

        Ok(())
    }

    /// Check if a token is supported
    pub fn is_token_supported(env: Env, token: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::SupportedToken(token))
            .unwrap_or(false)
    }

    /// Get token metadata like decimals
    pub fn get_token_metadata(env: Env, token: Address) -> Option<u32> {
        env.storage()
            .persistent()
            .get(&DataKey::TokenMetadata(token))
    }

    /// Creates or updates a payroll escrow for production scenarios.
    ///
    /// Requirements:
    /// - Contract must not be paused
    /// - Only the employer can call this method (if updating an existing record).
    /// - Must provide valid interval (> 0).
    /// - Must provide valid recurrence frequency (> 0).
    /// - Sets `last_payment_time` to current block timestamp when created.
    /// - Sets `next_payout_timestamp` to current time + recurrence frequency when created.
    pub fn create_or_update_escrow(
        env: Env,
        employer: Address,
        employee: Address,
        token: Address,
        amount: i128,
        interval: u64,
        recurrence_frequency: u64,
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

        if interval == 0 || amount <= 0 || recurrence_frequency == 0 {
            return Err(PayrollError::InvalidData);
        }

        let current_time = env.ledger().timestamp();
        let last_payment_time = if let Some(ref existing) = existing_payroll {
            // If updating, preserve last payment time
            existing.last_payment_time
        } else {
            // If creating, set to current time
            current_time
        };

        let next_payout_timestamp = if let Some(ref existing) = existing_payroll {
            // If updating, always recalculate next payout time based on new recurrence frequency
            current_time + recurrence_frequency
        } else {
            // If creating, set to current time + recurrence frequency
            current_time + recurrence_frequency
        };

        storage.set(&DataKey::PayrollEmployer(employee.clone()), &employer);
        storage.set(&DataKey::PayrollToken(employee.clone()), &token);
        storage.set(&DataKey::PayrollAmount(employee.clone()), &amount);
        storage.set(&DataKey::PayrollInterval(employee.clone()), &interval);
        storage.set(
            &DataKey::PayrollLastPayment(employee.clone()),
            &last_payment_time,
        );
        storage.set(
            &DataKey::PayrollRecurrenceFrequency(employee.clone()),
            &recurrence_frequency,
        );
        storage.set(
            &DataKey::PayrollNextPayoutTimestamp(employee.clone()),
            &next_payout_timestamp,
        );

        // Automatically add token as supported if it's not already
        if !Self::is_token_supported(env.clone(), token.clone()) {
            let key = DataKey::SupportedToken(token.clone());
            storage.set(&key, &true);

            // Set default decimals (7 for Stellar assets)
            let metadata_key = DataKey::TokenMetadata(token.clone());
            storage.set(&metadata_key, &7u32);
        }

        let payroll = Payroll {
            employer,
            token,
            amount,
            interval,
            last_payment_time,
            recurrence_frequency,
            next_payout_timestamp,
        };

        // Emit payroll updated event
        env.events().publish(
            (UPDATED_EVENT,),
            (
                payroll.employer.clone(),
                employee.clone(),
                payroll.recurrence_frequency,
            ),
        );

        Ok(payroll)
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
    /// - Next payout timestamp must be reached
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

        // Ensure the token is supported
        if !Self::is_token_supported(env.clone(), payroll.token.clone()) {
            return Err(PayrollError::InvalidData);
        }

        // Check if next payout time has been reached
        let current_time = env.ledger().timestamp();
        if current_time < payroll.next_payout_timestamp {
            return Err(PayrollError::NextPayoutTimeNotReached);
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

        // Update the last payment time and next payout timestamp
        storage.set(
            &DataKey::PayrollLastPayment(employee.clone()),
            &current_time,
        );
        storage.set(
            &DataKey::PayrollNextPayoutTimestamp(employee.clone()),
            &(current_time + payroll.recurrence_frequency),
        );

        // Emit disburse event
        emit_disburse(
            env,
            payroll.employer,
            employee,
            payroll.token,
            payroll.amount,
            current_time,
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
            recurrence_frequency: storage
                .get(&DataKey::PayrollRecurrenceFrequency(employee.clone()))
                .unwrap(),
            next_payout_timestamp: storage
                .get(&DataKey::PayrollNextPayoutTimestamp(employee.clone()))
                .unwrap(),
        })
    }

    /// Check if an employee is eligible for recurring disbursement
    pub fn is_eligible_for_disbursement(env: Env, employee: Address) -> bool {
        if let Some(payroll) = Self::_get_payroll(&env, &employee) {
            let current_time = env.ledger().timestamp();
            current_time >= payroll.next_payout_timestamp
        } else {
            false
        }
    }

    /// Process recurring disbursements for all eligible employees
    /// This function can be called by admin or off-chain bot
    pub fn process_recurring_disbursements(
        env: Env,
        caller: Address,
        employees: Vec<Address>,
    ) -> Vec<Address> {
        // Check if contract is paused
        Self::require_not_paused(&env).unwrap();

        caller.require_auth();

        let storage = env.storage().persistent();
        let owner = storage.get::<DataKey, Address>(&DataKey::Owner).unwrap();

        // Only owner can process recurring disbursements
        if caller != owner {
            panic!("Unauthorized");
        }

        let mut processed_employees = Vec::new(&env);
        let current_time = env.ledger().timestamp();

        for employee in employees.iter() {
            if let Some(payroll) = Self::_get_payroll(&env, &employee) {
                // Check if employee is eligible for disbursement
                if current_time >= payroll.next_payout_timestamp {
                    // Check if employer has sufficient balance
                    let balance_key =
                        DataKey::Balance(payroll.employer.clone(), payroll.token.clone());
                    let current_balance: i128 = storage.get(&balance_key).unwrap_or(0);

                    if current_balance >= payroll.amount {
                        // Deduct from employer's balance
                        storage.set(&balance_key, &(current_balance - payroll.amount));

                        // Transfer tokens to employee
                        let token_client = TokenClient::new(&env, &payroll.token);
                        let contract_address = env.current_contract_address();
                        token_client.transfer(&contract_address, &employee, &payroll.amount);

                        // Update timestamps
                        storage.set(
                            &DataKey::PayrollLastPayment(employee.clone()),
                            &current_time,
                        );
                        storage.set(
                            &DataKey::PayrollNextPayoutTimestamp(employee.clone()),
                            &(current_time + payroll.recurrence_frequency),
                        );

                        // Add to processed list
                        processed_employees.push_back(employee.clone());

                        // Emit individual disbursement event
                        emit_disburse(
                            env.clone(),
                            payroll.employer.clone(),
                            employee.clone(),
                            payroll.token.clone(),
                            payroll.amount,
                            current_time,
                        );
                        // env.events().publish(
                        //     (DISBURSE_EVENT,),
                        //     (payroll.employer, employee, payroll.token, payroll.amount),
                        // );
                    }
                }
            }
        }

        // Emit recurring disbursement event
        env.events()
            .publish((RECUR_EVENT,), (caller, processed_employees.len() as u32));

        processed_employees
    }

    /// Get next payout timestamp for an employee
    pub fn get_next_payout_timestamp(env: Env, employee: Address) -> Option<u64> {
        Self::_get_payroll(&env, &employee).map(|payroll| payroll.next_payout_timestamp)
    }

    /// Get recurrence frequency for an employee
    pub fn get_recurrence_frequency(env: Env, employee: Address) -> Option<u64> {
        Self::_get_payroll(&env, &employee).map(|payroll| payroll.recurrence_frequency)
    }
}
