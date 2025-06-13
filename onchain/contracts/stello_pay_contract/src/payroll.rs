use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol, symbol_short};

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
    /// Contract is paused
    ContractPaused = 6,
    /// Insufficient Balance
    InsufficientBalance = 7,
}

//-----------------------------------------------------------------------------
// Data Structures
//-----------------------------------------------------------------------------

/// Key used to store payroll info in contract storage.
#[contracttype]
pub struct PayrollKey(pub Address);

/// Key used to store employer balance in contract storage.
#[contracttype]
pub struct EmployerBalanceKey {
    pub employer: Address,
    pub token: Address,
}

/// Storage keys using symbols instead of unit structs
const PAUSE_KEY: Symbol = symbol_short!("PAUSED");
const OWNER_KEY: Symbol = symbol_short!("OWNER");
const DEPOSIT_EVENT: Symbol = symbol_short!("deposit");
const DISBURSE_EVENT: Symbol = symbol_short!("disburse");

/// Stores basic payroll information.
#[contracttype]
#[derive(Clone)]
pub struct Payroll {
    // Address of the employer (who pays).
    pub employer: Address,
    // Address of the employee (who receives salary).
    pub employee: Address,
    // Token contract address for payments
    pub token: Address,
    // Amount to be paid per interval
    pub amount: i128,
    // Payment interval in seconds (e.g., weekly, monthly).
    pub interval: u64,
    // Last payment timestamp.
    pub last_payment_time: u64,
}

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
        if storage.has(&OWNER_KEY) {
            panic!("Contract already initialized");
        }
        
        storage.set(&OWNER_KEY, &owner);
        // Contract starts unpaused by default
        storage.set(&PAUSE_KEY, &false);
    }

    /// Pause the contract - only callable by owner
    pub fn pause(env: Env, caller: Address) -> Result<(), PayrollError> {
        caller.require_auth();
        
        let storage = env.storage().persistent();
        
        // Check if caller is the owner
        if let Some(owner) = storage.get::<Symbol, Address>(&OWNER_KEY) {
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
        if let Some(owner) = storage.get::<Symbol, Address>(&OWNER_KEY) {
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
        storage.get(&PAUSE_KEY).unwrap_or(false)
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

        let key = PayrollKey(employee.clone());
        let storage = env.storage().persistent();
        // let owner = storage.get::<Symbol, Address>(&OWNER_KEY).ok_or(PayrollError::Unauthorized)?;
        let owner = storage.get::<Symbol, Address>(&OWNER_KEY).unwrap();
        
        if let Some(existing_payroll) = storage.get::<PayrollKey, Payroll>(&key) {
            // For updates, only the contract owner or the existing payroll's employer can call
            if employer != owner && employer != existing_payroll.employer {
                return Err(PayrollError::Unauthorized);
            }
        } else if employer != owner {
            // For creation, only the contract owner can call
            return Err(PayrollError::Unauthorized);
        }

        if interval == 0 || amount <= 0 {
            return Err(PayrollError::InvalidData);
        }

        if let Some(existing_payroll) = storage.get::<PayrollKey, Payroll>(&key) {
            if existing_payroll.employer != employer {
                return Err(PayrollError::Unauthorized);
            }

            let updated_payroll = Payroll {
                employer: existing_payroll.employer,
                employee: existing_payroll.employee,
                token,
                amount,
                interval,
                last_payment_time: existing_payroll.last_payment_time,
            };

            storage.set(&key, &updated_payroll);
            return Ok(updated_payroll);
        }

        let current_time = env.ledger().timestamp();

        let new_payroll = Payroll {
            employer,
            employee,
            token,
            amount,
            interval,
            last_payment_time: current_time,
        };

        storage.set(&key, &new_payroll);
        Ok(new_payroll)
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
        // Check if contract is paused
        Self::require_not_paused(&env)?;
        
        employer.require_auth();
        
        if amount <= 0 {
            return Err(PayrollError::InvalidData);
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

        // TODO: In production, transfer tokens from employer to contract
        // token_client.transfer(&employer, &env.current_contract_address(), &amount);

        // Emit deposit event
        env.events().publish(
            (DEPOSIT_EVENT, employer.clone(), token.clone()),
            (amount, new_balance)
        );

        Ok(())
    }

    /// Get employer's token balance in the contract
    pub fn get_employer_balance(
        env: Env,
        employer: Address,
        token: Address,
    ) -> i128 {
        let storage = env.storage().persistent();
        let balance_key = EmployerBalanceKey { employer, token };
        storage.get(&balance_key).unwrap_or(0)
    }

    /// Internal function to deduct from employer balance
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

        let new_balance = current_balance - amount;
        storage.set(&balance_key, &new_balance);

        Ok(())
    }

    /// Disburses salary if enough time has elapsed since the last payment.
    /// In a production-level contract, this would involve:
    /// - Checking the token contract for transferring funds.
    /// - Verifying that the caller is the employer or an automated process with the right credentials.
    /// - Updating the `last_payment_time` on success.
    pub fn disburse_salary(
        env: Env,
        caller: Address,
        employee: Address,
    ) -> Result<(), PayrollError> {
        // Check if contract is paused
        Self::require_not_paused(&env)?;

        caller.require_auth();
        
        let storage = env.storage().persistent();
        let key = PayrollKey(employee.clone());
        
        if let Some(payroll_data) = storage.get::<PayrollKey, Payroll>(&key) {
            if caller != payroll_data.employer {
                return Err(PayrollError::Unauthorized);
            }
            
            let current_time = env.ledger().timestamp();

            if current_time < payroll_data.last_payment_time + payroll_data.interval {
                return Err(PayrollError::IntervalNotReached);
            }

            Self::deduct_from_balance(&env, &payroll_data.employer, &payroll_data.token, payroll_data.amount)?;

            // Handle dispatch transfer
            // TODO: Implement actual token transfer logic here

            // Update the last payment time
            let updated_payroll = Payroll {
                employer: payroll_data.employer,
                employee: payroll_data.employee,
                token: payroll_data.token,
                amount: payroll_data.amount,
                interval: payroll_data.interval,
                last_payment_time: current_time,
            };

            let storage = env.storage().persistent();
            storage.set(&key, &updated_payroll);
            Ok(())
        } else {
            Err(PayrollError::PayrollNotFound)
        }
    }

    /// Gets the payroll details for a given employee.
    /// This can be used by UIs or dashboards to display status.
    /// Note: This function is not blocked when paused as it's read-only
    pub fn get_payroll(env: Env, employee: Address) -> Option<Payroll> {
        employee.require_auth();
        let key = PayrollKey(employee.clone());
        let storage = env.storage().persistent();

        storage.get::<PayrollKey, Payroll>(&key)
    }

    /// Example function to allow employees to pull payment themselves.
    /// In some real-world setups, employees might want to call the contract
    /// to initiate the disbursement if they are due for a payout.
    ///
    /// For security, you'd still require that the employer's account
    /// or an automated bot is used to actually sign transactions
    /// or have an on-chain logic that automatically triggers disburse.
    pub fn employee_withdraw(env: Env, employee: Address) -> Result<(), PayrollError> {
        // Check if contract is paused
        Self::require_not_paused(&env)?;
        
        employee.require_auth();
        let key = PayrollKey(employee.clone());
        let storage = env.storage().persistent();

        if let Some(existing_payroll) = storage.get::<PayrollKey, Payroll>(&key) {
            // Check if the interval has passed since the last payment time
            let current_time = env.ledger().timestamp();
            let last_payment_time = existing_payroll.last_payment_time;
            if current_time - last_payment_time >= existing_payroll.interval {
                // Process the disbursement of payment
                Self::disburse_salary(env.clone(), existing_payroll.employer.clone(), employee.clone())?;

                // Update last_payment_time of the payroll
                let updated_payroll = Payroll {
                    employer: existing_payroll.employer.clone(),
                    employee: existing_payroll.employee.clone(),
                    token: existing_payroll.token.clone(),
                    amount: existing_payroll.amount,
                    interval: existing_payroll.interval,
                    last_payment_time: current_time,
                };
                storage.set(&key, &updated_payroll);

                // Emit disbursement event
                env.events().publish(
                    (DISBURSE_EVENT, existing_payroll.employer, existing_payroll.employee),
                    (existing_payroll.token, existing_payroll.amount, current_time)
                );

                Ok(())
            } else {
                // Interval has not completed since the last payment time
                Err(PayrollError::IntervalNotReached)
            }
        } else {
            // No existing payroll record found for the employee
            Err(PayrollError::PayrollNotFound)
        }
    }

    /// Get the contract owner address
    pub fn get_owner(env: Env) -> Option<Address> {
        let storage = env.storage().persistent();
        storage.get(&OWNER_KEY)
    }

    /// Transfer ownership to a new address - only callable by current owner
    pub fn transfer_ownership(env: Env, caller: Address, new_owner: Address) -> Result<(), PayrollError> {
        caller.require_auth();
        new_owner.require_auth();
        
        let storage = env.storage().persistent();
        
        // Check if caller is the current owner
        if let Some(current_owner) = storage.get::<Symbol, Address>(&OWNER_KEY) {
            if caller != current_owner {
                return Err(PayrollError::Unauthorized);
            }
        } else {
            return Err(PayrollError::Unauthorized);
        }
        
        // Set new owner
        storage.set(&OWNER_KEY, &new_owner);
        
        Ok(())
    }
}