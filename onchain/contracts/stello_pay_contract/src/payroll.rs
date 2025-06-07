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

/// Stores basic payroll information.
#[contracttype]
#[derive(Clone)]
pub struct Payroll {
    // Address of the employer (who pays).
    pub employer: Address,
    // Address of the employee (who receives salary).
    pub employee: Address,
    // Amount to be paid (this is an example stub, real logic to do token
    // transfers would reference a token contract).
    pub amount: i64,
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
        amount: i64,
        interval: u64,
    ) -> Result<Payroll, PayrollError> {
        // Check if contract is paused
        Self::require_not_paused(&env)?;
        
        employer.require_auth();
        if interval == 0 {
            return Err(PayrollError::InvalidData);
        }

        let key = PayrollKey(employee.clone());
        let storage = env.storage().persistent();

        if let Some(existing_payroll) = storage.get::<PayrollKey, Payroll>(&key) {
            if existing_payroll.employer != employer {
                return Err(PayrollError::Unauthorized);
            }

            let updated_payroll = Payroll {
                employer: existing_payroll.employer,
                employee: existing_payroll.employee,
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
            amount,
            interval,
            last_payment_time: current_time,
        };

        storage.set(&key, &new_payroll);
        Ok(new_payroll)
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

            // Handle dispatch transfer
            // TODO: Implement actual token transfer logic here

            // Update the last payment time
            let updated_payroll = Payroll {
                employer: payroll_data.employer,
                employee: payroll_data.employee,
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
                    last_payment_time: current_time,
                    ..existing_payroll
                };
                storage.set(&key, &updated_payroll);

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