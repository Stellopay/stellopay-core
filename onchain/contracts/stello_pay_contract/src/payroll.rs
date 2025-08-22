use soroban_sdk::{
    contract, contracterror, contractimpl, symbol_short, token::Client as TokenClient, Address,
    Env, Symbol, Vec, String,
};

use crate::events::{emit_disburse, DEPOSIT_EVENT, PAUSED_EVENT, UNPAUSED_EVENT, EMPLOYEE_PAUSED_EVENT, EMPLOYEE_RESUMED_EVENT};
use crate::storage::{DataKey, Payroll, PayrollInput, CompactPayroll, CompactPayrollHistoryEntry, PayrollTemplate, TemplatePreset};
use crate::insurance::{InsuranceSystem, InsuranceError, InsurancePolicy, InsuranceClaim, Guarantee, InsuranceSettings};

//-----------------------------------------------------------------------------
// Gas Optimization Structures
//-----------------------------------------------------------------------------

/// Cached contract state to reduce storage reads
#[derive(Clone, Debug)]
struct ContractCache {
    owner: Option<Address>,
    is_paused: Option<bool>,
}

/// Batch operation context for efficient processing
#[derive(Clone, Debug)]
struct BatchContext {
    current_time: u64,
    cache: ContractCache,
}

/// Index operation type for efficient index management
#[derive(Clone, Debug)]
enum IndexOperation {
    Add,
    Remove,
}

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
    /// Template not found
    TemplateNotFound = 11,
    /// Preset not found
    PresetNotFound = 12,
    /// Template name is empty or invalid
    InvalidTemplateName = 13,
    /// Template is not public
    TemplateNotPublic = 14,
    /// Template validation failed
    TemplateValidationFailed = 15,
    /// Preset is not active
    PresetNotActive = 16,
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

/// Event emitted when batch operations are performed
pub const BATCH_EVENT: Symbol = symbol_short!("batch");

/// Event emitted when payroll history is updated
pub const HISTORY_UPDATED_EVENT: Symbol = symbol_short!("hist_upd");

/// Event emitted for audit trail entries
pub const AUDIT_EVENT: Symbol = symbol_short!("audit");

/// Event emitted when a template is created
pub const TEMPLATE_CREATED_EVENT: Symbol = symbol_short!("tmpl_crt");

/// Event emitted when a template is updated
pub const TEMPLATE_UPDATED_EVENT: Symbol = symbol_short!("tmpl_upd");

/// Event emitted when a template is applied
pub const TEMPLATE_APPLIED_EVENT: Symbol = symbol_short!("tmpl_app");

/// Event emitted when a template is shared
pub const TEMPLATE_SHARED_EVENT: Symbol = symbol_short!("tmpl_shr");

/// Event emitted when a preset is created
pub const PRESET_CREATED_EVENT: Symbol = symbol_short!("preset_c");

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
        // Optimized validation with early returns
        Self::validate_payroll_input(amount, interval, recurrence_frequency)?;

        employer.require_auth();

        // Get cached contract state to reduce storage reads
        let cache = Self::get_contract_cache(&env);
        let storage = env.storage().persistent();

        // Check authorization with cached data
        let existing_payroll = Self::_get_payroll(&env, &employee);
        let is_owner = cache.owner.as_ref().map_or(false, |owner| &employer == owner);

        if let Some(ref existing) = existing_payroll {
            // For updates, only the contract owner or the existing payroll's employer can call
            if !is_owner && &employer != &existing.employer {
                return Err(PayrollError::Unauthorized);
            }
        } else if !is_owner {
            // For creation, only the contract owner can call
            return Err(PayrollError::Unauthorized);
        }

        let current_time = env.ledger().timestamp();
        let last_payment_time = if let Some(ref existing) = existing_payroll {
            // If updating, preserve last payment time
            existing.last_payment_time
        } else {
            // If creating, set to current time
            current_time
        };

        let next_payout_timestamp = current_time + recurrence_frequency;

        let payroll = Payroll {
            employer: employer.clone(),
            token: token.clone(),
            amount,
            interval,
            last_payment_time,
            recurrence_frequency,
            next_payout_timestamp,
            is_paused: false
        };

        // Store the payroll using compact format for gas efficiency
        let compact_payroll = Self::to_compact_payroll(&payroll);
        storage.set(&DataKey::Payroll(employee.clone()), &compact_payroll);

        // Update indexing efficiently
        Self::update_indexes_efficiently(&env, &employer, &token, &employee, IndexOperation::Add);

        // Record history entry
        Self::record_history(
            &env, 
            &employee, 
            &compact_payroll,
            if existing_payroll.is_some() {
                symbol_short!("updated")
            } else {
                symbol_short!("created")
            },
        );

        // Automatically add token as supported if it's not already
        if !Self::is_token_supported(env.clone(), token.clone()) {
            let key = DataKey::SupportedToken(token.clone());
            storage.set(&key, &true);

            // Set default decimals (7 for Stellar assets)
            let metadata_key = DataKey::TokenMetadata(token.clone());
            storage.set(&metadata_key, &7u32);
        }

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
        // Early validation
        if amount <= 0 {
            return Err(PayrollError::InvalidData);
        }

        employer.require_auth();

        // Get cached contract state
        let cache = Self::get_contract_cache(&env);
        if let Some(true) = cache.is_paused {
            return Err(PayrollError::ContractPaused);
        }

        // Optimized token transfer with balance verification
        Self::transfer_tokens_safe(&env, &token, &employer, &env.current_contract_address(), amount)?;

        // Update balance in single operation
        let storage = env.storage().persistent();
        let balance_key = DataKey::Balance(employer.clone(), token.clone());
        let current_balance: i128 = storage.get(&balance_key).unwrap_or(0);
        storage.set(&balance_key, &(current_balance + amount));

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
        caller.require_auth();

        // Get cached contract state
        let cache = Self::get_contract_cache(&env);
        if let Some(true) = cache.is_paused {
            return Err(PayrollError::ContractPaused);
        }

        let payroll = Self::_get_payroll(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;

        // Check if payroll is paused for this employee
        if payroll.is_paused {
            return Err(PayrollError::ContractPaused);
        }

        // Only the employer can disburse salary
        if caller != payroll.employer {
            return Err(PayrollError::Unauthorized);
        }

        // Check if next payout time has been reached
        let current_time = env.ledger().timestamp();
        if current_time < payroll.next_payout_timestamp {
            return Err(PayrollError::NextPayoutTimeNotReached);
        }

        // Optimized balance check and update
        Self::check_and_update_balance(&env, &payroll.employer, &payroll.token, payroll.amount)?;

        // Optimized token transfer
        let contract_address = env.current_contract_address();
        Self::transfer_tokens_safe(&env, &payroll.token, &contract_address, &employee, payroll.amount)?;


        // Optimized payroll update with minimal storage operations
        Self::update_payroll_timestamps(&env, &employee, &payroll, current_time);

        Self::record_audit(&env, &employee, &payroll.employer, &payroll.token, payroll.amount, current_time);

        // Emit disburse event
        emit_disburse(
            env,
            payroll.employer,
            employee,
            payroll.token.clone(),
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
        let payroll_key = DataKey::Payroll(employee.clone());

        if !storage.has(&payroll_key) {
            return None;
        }

        // Try to get compact payroll first, fallback to regular payroll
        if let Some(compact_payroll) = storage.get::<DataKey, CompactPayroll>(&payroll_key) {
            Some(Self::from_compact_payroll(&compact_payroll))
        } else if let Some(payroll) = storage.get::<DataKey, Payroll>(&payroll_key) {
            Some(payroll)
        } else {
            None
        }
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
        caller.require_auth();

        // Create optimized batch context
        let batch_ctx = Self::create_batch_context(&env);

        // Only owner can process recurring disbursements
        if let Some(owner) = &batch_ctx.cache.owner {
            if caller != *owner {
                panic!("Unauthorized");
            }
        } else {
            panic!("Unauthorized");
        }

        let mut processed_employees = Vec::new(&env);

        for employee in employees.iter() {
            if let Some(payroll) = Self::_get_payroll(&env, &employee) {
                // Check if employee is eligible for disbursement and not paused
                if batch_ctx.current_time >= payroll.next_payout_timestamp && !payroll.is_paused {
                    // Optimized balance check and update
                    if let Ok(()) = Self::check_and_update_balance(&env, &payroll.employer, &payroll.token, payroll.amount) {
                        // Optimized token transfer
                        let contract_address = env.current_contract_address();
                        if let Ok(()) = Self::transfer_tokens_safe(&env, &payroll.token, &contract_address, &employee, payroll.amount) {
                            // Optimized payroll update with minimal storage operations
                            Self::update_payroll_timestamps(&env, &employee, &payroll, batch_ctx.current_time);

                            // Add to processed list
                            processed_employees.push_back(employee.clone());

                            // Emit individual disbursement event
                            emit_disburse(
                                env.clone(),
                                payroll.employer.clone(),
                                employee.clone(),
                                payroll.token.clone(),
                                payroll.amount,
                                batch_ctx.current_time,
                            );
                        }
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

    /// Convert Payroll to CompactPayroll for storage optimization
    fn to_compact_payroll(payroll: &Payroll) -> CompactPayroll {
        CompactPayroll {
            employer: payroll.employer.clone(),
            token: payroll.token.clone(),
            amount: payroll.amount,
            interval: payroll.interval as u32,
            last_payment_time: payroll.last_payment_time,
            recurrence_frequency: payroll.recurrence_frequency as u32,
            next_payout_timestamp: payroll.next_payout_timestamp,
            is_paused: payroll.is_paused
        }
    }

    /// Convert CompactPayroll back to Payroll
    fn from_compact_payroll(compact: &CompactPayroll) -> Payroll {
        Payroll {
            employer: compact.employer.clone(),
            token: compact.token.clone(),
            amount: compact.amount,
            interval: compact.interval as u64,
            last_payment_time: compact.last_payment_time,
            recurrence_frequency: compact.recurrence_frequency as u64,
            next_payout_timestamp: compact.next_payout_timestamp,
            is_paused: compact.is_paused
        }
    }

    /// Add employee to employer index
    fn add_to_employer_index(env: &Env, employer: &Address, employee: &Address) {
        let storage = env.storage().persistent();
        let key = DataKey::EmployerEmployees(employer.clone());
        let mut employees: Vec<Address> = storage.get(&key).unwrap_or(Vec::new(env));
        
        // Check if employee already exists to avoid duplicates
        let mut exists = false;
        for existing_employee in employees.iter() {
            if &existing_employee == employee {
                exists = true;
                break;
            }
        }
        
        if !exists {
            employees.push_back(employee.clone());
            storage.set(&key, &employees);
        }
    }

    /// Remove employee from employer index
    fn remove_from_employer_index(env: &Env, employer: &Address, employee: &Address) {
        let storage = env.storage().persistent();
        let key = DataKey::EmployerEmployees(employer.clone());
        let mut employees: Vec<Address> = storage.get(&key).unwrap_or(Vec::new(env));
        
        let mut new_employees = Vec::new(env);
        for existing_employee in employees.iter() {
            if &existing_employee != employee {
                new_employees.push_back(existing_employee);
            }
        }
        
        if new_employees.len() > 0 {
            storage.set(&key, &new_employees);
        } else {
            storage.remove(&key);
        }
    }

    /// Add employee to token index
    fn add_to_token_index(env: &Env, token: &Address, employee: &Address) {
        let storage = env.storage().persistent();
        let key = DataKey::TokenEmployees(token.clone());
        let mut employees: Vec<Address> = storage.get(&key).unwrap_or(Vec::new(env));
        
        // Check if employee already exists to avoid duplicates
        let mut exists = false;
        for existing_employee in employees.iter() {
            if &existing_employee == employee {
                exists = true;
                break;
            }
        }
        
        if !exists {
            employees.push_back(employee.clone());
            storage.set(&key, &employees);
        }
    }

    /// Remove employee from token index
    fn remove_from_token_index(env: &Env, token: &Address, employee: &Address) {
        let storage = env.storage().persistent();
        let key = DataKey::TokenEmployees(token.clone());
        let mut employees: Vec<Address> = storage.get(&key).unwrap_or(Vec::new(env));
        
        let mut new_employees = Vec::new(env);
        for existing_employee in employees.iter() {
            if &existing_employee != employee {
                new_employees.push_back(existing_employee);
            }
        }
        
        if new_employees.len() > 0 {
            storage.set(&key, &new_employees);
        } else {
            storage.remove(&key);
        }
    }

    /// Batch create or update escrows for multiple employees
    /// This is more gas efficient than calling create_or_update_escrow multiple times
    pub fn batch_create_escrows(
        env: Env,
        employer: Address,
        payroll_inputs: Vec<PayrollInput>,
    ) -> Result<Vec<Payroll>, PayrollError> {
        employer.require_auth();

        // Create optimized batch context
        let batch_ctx = Self::create_batch_context(&env);
        let storage = env.storage().persistent();
        let is_owner = batch_ctx.cache.owner.as_ref().map_or(false, |owner| &employer == owner);

        let mut created_payrolls = Vec::new(&env);

        for payroll_input in payroll_inputs.iter() {
            // Optimized validation with early returns
            Self::validate_payroll_input(
                payroll_input.amount,
                payroll_input.interval,
                payroll_input.recurrence_frequency,
            )?;

            let existing_payroll = Self::_get_payroll(&env, &payroll_input.employee);

            if let Some(ref existing) = existing_payroll {
                // For updates, only the contract owner or the existing payroll's employer can call
                if !is_owner && &employer != &existing.employer {
                    return Err(PayrollError::Unauthorized);
                }
            } else if !is_owner {
                // For creation, only the contract owner can call
                return Err(PayrollError::Unauthorized);
            }

            let last_payment_time = existing_payroll
                .as_ref()
                .map(|p| p.last_payment_time)
                .unwrap_or(batch_ctx.current_time);

            let next_payout_timestamp = batch_ctx.current_time + payroll_input.recurrence_frequency;

            let payroll = Payroll {
                employer: employer.clone(),
                token: payroll_input.token.clone(),
                amount: payroll_input.amount,
                interval: payroll_input.interval,
                last_payment_time,
                recurrence_frequency: payroll_input.recurrence_frequency,
                next_payout_timestamp,
                is_paused: false
            };

            // Store the payroll using compact format for gas efficiency
            let compact_payroll = Self::to_compact_payroll(&payroll);
            storage.set(&DataKey::Payroll(payroll_input.employee.clone()), &compact_payroll);

            // Update indexing efficiently
            Self::update_indexes_efficiently(
                &env,
                &employer,
                &payroll_input.token,
                &payroll_input.employee,
                IndexOperation::Add,
            );

            // Record history entry
            Self::record_history(
                &env, 
                &payroll_input.employee, 
                &compact_payroll,
                if existing_payroll.is_some() {
                    symbol_short!("updated")
                } else {
                    symbol_short!("created")
                },
            );

            // Automatically add token as supported if it's not already
            if !Self::is_token_supported(env.clone(), payroll_input.token.clone()) {
                let key = DataKey::SupportedToken(payroll_input.token.clone());
                storage.set(&key, &true);

                // Set default decimals (7 for Stellar assets)
                let metadata_key = DataKey::TokenMetadata(payroll_input.token.clone());
                storage.set(&metadata_key, &7u32);
            }

            created_payrolls.push_back(payroll);
        }

        // Emit batch event
        env.events().publish(
            (BATCH_EVENT,),
            (employer, created_payrolls.len() as u32),
        );

        Ok(created_payrolls)
    }

    /// Batch disburse salaries to multiple employees
    /// This is more gas efficient than calling disburse_salary multiple times
    pub fn batch_disburse_salaries(
        env: Env,
        caller: Address,
        employees: Vec<Address>,
    ) -> Result<Vec<Address>, PayrollError> {
        caller.require_auth();

        // Create optimized batch context
        let batch_ctx = Self::create_batch_context(&env);
        let storage = env.storage().persistent();
        let mut processed_employees = Vec::new(&env);

        for employee in employees.iter() {
            let payroll = Self::_get_payroll(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;

            // Only the employer can disburse salary
            if caller != payroll.employer {
                return Err(PayrollError::Unauthorized);
            }

            // Check if payroll is paused for this employee
            if payroll.is_paused {
                return Err(PayrollError::ContractPaused);
            }

            // Check if next payout time has been reached
            if batch_ctx.current_time < payroll.next_payout_timestamp {
                return Err(PayrollError::NextPayoutTimeNotReached);
            }

            // Optimized balance check and update
            Self::check_and_update_balance(&env, &payroll.employer, &payroll.token, payroll.amount)?;

            // Optimized token transfer
            let contract_address = env.current_contract_address();
            Self::transfer_tokens_safe(&env, &payroll.token, &contract_address, &employee, payroll.amount)?;

            // Optimized payroll update with minimal storage operations
            Self::update_payroll_timestamps(&env, &employee, &payroll, batch_ctx.current_time);

            // Add to processed list
            processed_employees.push_back(employee.clone());

            Self::record_audit(&env, &employee, &payroll.employer, &payroll.token, payroll.amount, batch_ctx.current_time);

            // Emit individual disbursement event
            emit_disburse(
                env.clone(),
                payroll.employer.clone(),
                employee.clone(),
                payroll.token.clone(),
                payroll.amount,
                batch_ctx.current_time,
            );
        }

        // Emit batch disbursement event
        env.events().publish(
            (BATCH_EVENT,),
            (caller, processed_employees.len() as u32),
        );

        Ok(processed_employees)
    }

    /// Get all employees for a specific employer
    pub fn get_employer_employees(env: Env, employer: Address) -> Vec<Address> {
        let storage = env.storage().persistent();
        storage.get(&DataKey::EmployerEmployees(employer)).unwrap_or(Vec::new(&env))
    }

    /// Get all employees for a specific token
    pub fn get_token_employees(env: Env, token: Address) -> Vec<Address> {
        let storage = env.storage().persistent();
        storage.get(&DataKey::TokenEmployees(token)).unwrap_or(Vec::new(&env))
    }

    /// Remove a payroll and clean up indexes
    pub fn remove_payroll(env: Env, caller: Address, employee: Address) -> Result<(), PayrollError> {
        // Check if contract is paused
        Self::require_not_paused(&env)?;

        caller.require_auth();

        let storage = env.storage().persistent();
        let owner = storage.get::<DataKey, Address>(&DataKey::Owner).unwrap();

        let payroll = Self::_get_payroll(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;

        // Only the contract owner or the payroll's employer can remove it
        if caller != owner && caller != payroll.employer {
            return Err(PayrollError::Unauthorized);
        }

        // Remove from indexes
        Self::remove_from_employer_index(&env, &payroll.employer, &employee);
        Self::remove_from_token_index(&env, &payroll.token, &employee);

        // Remove payroll data
        storage.remove(&DataKey::Payroll(employee));

        Ok(())
    }

    /// Pauses payroll for a specific employee, preventing disbursements.
    /// Only callable by contract owner or employee's employer.
    pub fn pause_employee_payroll(env: Env, caller: Address, employee: Address) -> Result<(), PayrollError> {
        caller.require_auth();

        let storage = env.storage().persistent();
        let cache = Self::get_contract_cache(&env);

        // Check if caller is authorized (owner or employer)
        let payroll = Self::_get_payroll(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;
        let is_owner = cache.owner.as_ref().map_or(false, |owner| &caller == owner);
        if !is_owner && caller != payroll.employer {
            return Err(PayrollError::Unauthorized);
        }

        // Update payroll pause state
        let mut updated_payroll = payroll.clone();
        updated_payroll.is_paused = true;
        
        // Store updated payroll
        let compact_payroll = Self::to_compact_payroll(&updated_payroll);
        storage.set(&DataKey::Payroll(employee.clone()), &compact_payroll);

        Self::record_history(
            &env, 
            &employee, 
            &compact_payroll,
            symbol_short!("paused")
        );

        // Emit pause event
        env.events().publish((EMPLOYEE_PAUSED_EVENT,), (caller, employee.clone()));

        Ok(())
    }

    /// Resumes payroll for a specific employee, allowing disbursements.
    /// Only callable by contract owner or employee's employer.
    pub fn resume_employee_payroll(
        env: Env,
        caller: Address,
        employee: Address,
    ) -> Result<(), PayrollError> {
        caller.require_auth();

        let storage = env.storage().persistent();
        let cache = Self::get_contract_cache(&env);

        // Check if caller is authorized (owner or employer)
        let payroll = Self::_get_payroll(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;
        let is_owner = cache.owner.as_ref().map_or(false, |owner| &caller == owner);
        if !is_owner && caller != payroll.employer {
            return Err(PayrollError::Unauthorized);
        }

        // Update payroll pause state
        let mut updated_payroll = payroll.clone();
        updated_payroll.is_paused = false;
        
        // Store updated payroll
        let compact_payroll = Self::to_compact_payroll(&updated_payroll);
        storage.set(&DataKey::Payroll(employee.clone()), &compact_payroll);

        Self::record_history(
            &env, 
            &employee, 
            &compact_payroll,
            symbol_short!("resumed")
        );

        // Emit resume event
        env.events().publish((EMPLOYEE_RESUMED_EVENT,), (caller, employee.clone()));

        Ok(())
    }

    //-----------------------------------------------------------------------------
    // Gas Optimization Helper Functions
    //-----------------------------------------------------------------------------

    /// Get cached contract state to reduce storage reads
    fn get_contract_cache(env: &Env) -> ContractCache {
        let storage = env.storage().persistent();
        ContractCache {
            owner: storage.get(&DataKey::Owner),
            is_paused: storage.get(&DataKey::Paused),
        }
    }

    /// Optimized validation that combines multiple checks
    fn validate_payroll_input(
        amount: i128,
        interval: u64,
        recurrence_frequency: u64,
    ) -> Result<(), PayrollError> {
        // Early return for invalid data to avoid unnecessary processing
        if amount <= 0 {
            return Err(PayrollError::InvalidData);
        }
        if interval == 0 {
            return Err(PayrollError::InvalidData);
        }
        if recurrence_frequency == 0 {
            return Err(PayrollError::InvalidRecurrenceFrequency);
        }
        Ok(())
    }

    /// Optimized authorization check with caching
    fn check_authorization(
        env: &Env,
        caller: &Address,
        cache: &ContractCache,
        required_owner: bool,
    ) -> Result<(), PayrollError> {
        // Early return if contract is paused
        if let Some(true) = cache.is_paused {
            return Err(PayrollError::ContractPaused);
        }

        if required_owner {
            if let Some(owner) = &cache.owner {
                if caller != owner {
                    return Err(PayrollError::Unauthorized);
                }
            } else {
                return Err(PayrollError::Unauthorized);
            }
        }

        Ok(())
    }

    /// Optimized balance check and update
    fn check_and_update_balance(
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

        // Update balance in single operation
        storage.set(&balance_key, &(current_balance - amount));
        Ok(())
    }

    /// Optimized token transfer with balance verification
    fn transfer_tokens_safe(
        env: &Env,
        token: &Address,
        from: &Address,
        to: &Address,
        amount: i128,
    ) -> Result<(), PayrollError> {
        let token_client = TokenClient::new(env, token);
        let initial_balance = token_client.balance(to);
        
        token_client.transfer(from, to, &amount);
        
        // Verify transfer success
        if token_client.balance(to) != initial_balance + amount {
            return Err(PayrollError::TransferFailed);
        }
        
        Ok(())
    }

    /// Optimized payroll update with minimal storage operations
    fn update_payroll_timestamps(
        env: &Env,
        employee: &Address,
        payroll: &Payroll,
        current_time: u64,
    ) {
        let storage = env.storage().persistent();
        let mut updated_payroll = payroll.clone();
        updated_payroll.last_payment_time = current_time;
        updated_payroll.next_payout_timestamp = current_time + payroll.recurrence_frequency;

        // Single storage operation for the entire update
        let compact_payroll = Self::to_compact_payroll(&updated_payroll);
        storage.set(&DataKey::Payroll(employee.clone()), &compact_payroll);
    }

    /// Optimized index management with duplicate prevention
    fn update_indexes_efficiently(
        env: &Env,
        employer: &Address,
        token: &Address,
        employee: &Address,
        operation: IndexOperation,
    ) {
        match operation {
            IndexOperation::Add => {
                Self::add_to_employer_index(env, employer, employee);
                Self::add_to_token_index(env, token, employee);
            }
            IndexOperation::Remove => {
                Self::remove_from_employer_index(env, employer, employee);
                Self::remove_from_token_index(env, token, employee);
            }
        }
    }

    /// Optimized batch context creation
    fn create_batch_context(env: &Env) -> BatchContext {
        BatchContext {
            current_time: env.ledger().timestamp(),
            cache: Self::get_contract_cache(env),
        }
    }

    //-----------------------------------------------------------------------------
    // Main Contract Functions (Optimized)
    //-----------------------------------------------------------------------------

    //-----------------------------------------------------------------------------
    // Insurance Functions
    //-----------------------------------------------------------------------------

    /// Create or update an insurance policy for an employee
    pub fn create_insurance_policy(
        env: Env,
        employer: Address,
        employee: Address,
        token: Address,
        coverage_amount: i128,
        premium_frequency: u64,
    ) -> Result<InsurancePolicy, InsuranceError> {
        employer.require_auth();
        Self::require_not_paused(&env)?;
        
        InsuranceSystem::create_or_update_insurance_policy(
            &env,
            &employer,
            &employee,
            &token,
            coverage_amount,
            premium_frequency,
        )
    }

    /// Pay insurance premium
    pub fn pay_insurance_premium(
        env: Env,
        employer: Address,
        employee: Address,
        amount: i128,
    ) -> Result<(), InsuranceError> {
        employer.require_auth();
        Self::require_not_paused(&env)?;
        
        InsuranceSystem::pay_premium(&env, &employer, &employee, amount)
    }

    /// File an insurance claim
    pub fn file_insurance_claim(
        env: Env,
        employee: Address,
        claim_amount: i128,
        claim_reason: String,
        evidence_hash: Option<String>,
    ) -> Result<u64, InsuranceError> {
        employee.require_auth();
        Self::require_not_paused(&env)?;
        
        InsuranceSystem::file_claim(&env, &employee, claim_amount, claim_reason, evidence_hash)
    }

    /// Approve an insurance claim (admin function)
    pub fn approve_insurance_claim(
        env: Env,
        approver: Address,
        claim_id: u64,
        approved_amount: i128,
    ) -> Result<(), InsuranceError> {
        approver.require_auth();
        Self::require_not_paused(&env)?;
        
        // Check if approver is owner
        let storage = env.storage().persistent();
        if let Some(owner) = storage.get::<DataKey, Address>(&DataKey::Owner) {
            if approver != owner {
                return Err(InsuranceError::ClaimNotEligible);
            }
        } else {
            return Err(InsuranceError::ClaimNotEligible);
        }
        
        InsuranceSystem::approve_claim(&env, &approver, claim_id, approved_amount)
    }

    /// Pay out an approved claim
    pub fn pay_insurance_claim(
        env: Env,
        caller: Address,
        claim_id: u64,
    ) -> Result<(), InsuranceError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;
        
        // Check if caller is owner
        let storage = env.storage().persistent();
        if let Some(owner) = storage.get::<DataKey, Address>(&DataKey::Owner) {
            if caller != owner {
                return Err(InsuranceError::ClaimNotEligible);
            }
        } else {
            return Err(InsuranceError::ClaimNotEligible);
        }
        
        InsuranceSystem::pay_claim(&env, claim_id)
    }

    /// Issue a guarantee for an employer
    pub fn issue_guarantee(
        env: Env,
        employer: Address,
        token: Address,
        guarantee_amount: i128,
        collateral_amount: i128,
        expiry_duration: u64,
    ) -> Result<u64, InsuranceError> {
        employer.require_auth();
        Self::require_not_paused(&env)?;
        
        InsuranceSystem::issue_guarantee(
            &env,
            &employer,
            &token,
            guarantee_amount,
            collateral_amount,
            expiry_duration,
        )
    }

    /// Repay a guarantee
    pub fn repay_guarantee(
        env: Env,
        employer: Address,
        guarantee_id: u64,
        repayment_amount: i128,
    ) -> Result<(), InsuranceError> {
        employer.require_auth();
        Self::require_not_paused(&env)?;
        
        InsuranceSystem::repay_guarantee(&env, &employer, guarantee_id, repayment_amount)
    }

    /// Fund the insurance pool
    pub fn fund_insurance_pool(
        env: Env,
        funder: Address,
        token: Address,
        amount: i128,
    ) -> Result<(), InsuranceError> {
        funder.require_auth();
        Self::require_not_paused(&env)?;
        
        InsuranceSystem::fund_insurance_pool(&env, &funder, &token, amount)
    }

    /// Get insurance policy for an employee
    pub fn get_insurance_policy(env: Env, employee: Address) -> Option<InsurancePolicy> {
        InsuranceSystem::get_insurance_policy(&env, &employee)
    }

    /// Get insurance claim by ID
    pub fn get_insurance_claim(env: Env, claim_id: u64) -> Option<InsuranceClaim> {
        InsuranceSystem::get_insurance_claim(&env, claim_id)
    }

    /// Get guarantee by ID
    pub fn get_guarantee(env: Env, guarantee_id: u64) -> Option<Guarantee> {
        InsuranceSystem::get_guarantee(&env, guarantee_id)
    }

    /// Get employer guarantees
    pub fn get_employer_guarantees(env: Env, employer: Address) -> Vec<u64> {
        InsuranceSystem::get_employer_guarantees(&env, &employer)
    }

    /// Get insurance settings
    pub fn get_insurance_settings(env: Env) -> InsuranceSettings {
        InsuranceSystem::get_insurance_settings(&env)
    }

    /// Set insurance settings (admin function)
    pub fn set_insurance_settings(
        env: Env,
        caller: Address,
        settings: InsuranceSettings,
    ) -> Result<(), InsuranceError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;
        
        // Check if caller is owner
        let storage = env.storage().persistent();
        if let Some(owner) = storage.get::<DataKey, Address>(&DataKey::Owner) {
            if caller != owner {
                return Err(InsuranceError::ClaimNotEligible);
            }
        } else {
            return Err(InsuranceError::ClaimNotEligible);
        }
        
        InsuranceSystem::set_insurance_settings(&env, settings)
    }

    //-----------------------------------------------------------------------------
    // Payroll History and Audit Trail
    //-----------------------------------------------------------------------------
    /// Record a payroll history entry
    fn record_history(
        env: &Env,
        employee: &Address,
        payroll: &CompactPayroll,
        action: Symbol,
    ) {
        let storage = env.storage().persistent();
        let timestamp = env.ledger().timestamp();
        let employer = &payroll.employer;

        // Get or initialize the history vector and ID counter
        let history_key = DataKey::PayrollHistoryEntry(employee.clone());
        let mut history: Vec<CompactPayrollHistoryEntry> = storage.get(&history_key).unwrap_or(Vec::new(env));
        let id_key = DataKey::PayrollHistoryIdCounter(employee.clone());
        let mut id_counter: u64 = storage.get(&id_key).unwrap_or(0);

        id_counter += 1;
        
        let history_entry = CompactPayrollHistoryEntry {
            employee: employee.clone(),
            employer: employer.clone(),
            token: payroll.token.clone(),
            amount: payroll.amount,
            interval: payroll.interval.into(),
            recurrence_frequency: payroll.recurrence_frequency,
            timestamp,
            last_payment_time: payroll.last_payment_time,
            next_payout_timestamp: payroll.next_payout_timestamp,
            action: action.clone(),
            id: id_counter
        };

         // Append to history vector
        history.push_back(history_entry);
        storage.set(&history_key, &history);
        storage.set(&id_key, &id_counter);

        env.events().publish(
            (HISTORY_UPDATED_EVENT,),
            (employee.clone(), employer.clone(), action, timestamp),
        );
       
    }

    /// Query payroll history for an employee with optional timestamp range
    pub fn get_payroll_history(
        env: Env,
        employee: Address,
        start_timestamp: Option<u64>,
        end_timestamp: Option<u64>,
        limit: Option<u32>,
    ) -> Vec<CompactPayrollHistoryEntry> {
        if limit == Some(0) {
            return Vec::new(&env);
        }
        let storage = env.storage().persistent();
        let mut history = Vec::new(&env);
        let max_entries = limit.unwrap_or(100);
        let history_key = DataKey::PayrollHistoryEntry(employee.clone());
        let history_entries: Vec<CompactPayrollHistoryEntry> = storage.get(&history_key).unwrap_or(Vec::new(&env));

        let mut count = 0;
        for entry in history_entries.iter() {
            if let Some(start) = start_timestamp {
                if entry.timestamp < start {
                    continue;
                }
            }
            if let Some(end) = end_timestamp {
                if entry.timestamp > end {
                    continue;
                }
            }

            history.push_back(entry);
            count += 1;
            if count >= max_entries {
                break;
            }
        }

        history
    }

    /// Record an audit trail entry for disbursements with sequential ID
    fn record_audit(
        env: &Env,
        employee: &Address,
        employer: &Address,
        token: &Address,
        amount: i128,
        timestamp: u64,
    ) {
        let storage = env.storage().persistent();
        
        let audit_key = DataKey::AuditTrail(employee.clone());
        let mut audit: Vec<CompactPayrollHistoryEntry> = storage.get(&audit_key).unwrap_or(Vec::new(env));
        let id_key = DataKey::AuditTrailIdCounter(employee.clone());
        let mut id_counter: u64 = storage.get(&id_key).unwrap_or(0);

        id_counter += 1;

        let payroll = Self::_get_payroll(env, employee).unwrap_or(Payroll {
            employer: employer.clone(),
            token: token.clone(),
            amount,
            interval: 0,
            recurrence_frequency: 0,
            last_payment_time: timestamp,
            next_payout_timestamp: timestamp,
            is_paused: false,
        });


        let history_entry = CompactPayrollHistoryEntry {
            employee: employee.clone(),
            employer: employer.clone(),
            token: token.clone(),
            amount: amount,
            interval: payroll.interval as u32,
            recurrence_frequency: payroll.recurrence_frequency as u32,
            timestamp,
            last_payment_time: payroll.last_payment_time,
            next_payout_timestamp: payroll.next_payout_timestamp,
            action: symbol_short!("disbursed"),
            id: id_counter
        };

        audit.push_back(history_entry);
        storage.set(&audit_key, &audit);
        storage.set(&id_key, &id_counter);

        env.events().publish(
            (AUDIT_EVENT,),
            (employee.clone(), employer.clone(), amount, timestamp, id_counter),
        );
    }

    /// Query audit trail for an employee with optional timestamp range
    pub fn get_audit_trail(
        env: Env,
        employee: Address,
        start_timestamp: Option<u64>,
        end_timestamp: Option<u64>,
        limit: Option<u32>,
    ) -> Vec<CompactPayrollHistoryEntry> {
        let storage = env.storage().persistent();
        let mut audit_trail = Vec::new(&env);
        let max_entries = limit.unwrap_or(100);

        let audit_key = DataKey::AuditTrail(employee.clone());
        let audit_entries: Vec<CompactPayrollHistoryEntry> = storage.get(&audit_key).unwrap_or(Vec::new(&env));

        let mut count = 0;
        for entry in audit_entries.iter() {
            if let Some(start) = start_timestamp {
                if entry.timestamp < start {
                    continue;
                }
            }
            if let Some(end) = end_timestamp {
                if entry.timestamp > end {
                    continue;
                }
            }

            audit_trail.push_back(CompactPayrollHistoryEntry {
                employee: entry.employee.clone(),
                employer: entry.employer.clone(),
                token: entry.token.clone(),
                amount: entry.amount,
                interval: entry.interval,
                recurrence_frequency: entry.recurrence_frequency,
                timestamp: entry.timestamp,
                last_payment_time: entry.last_payment_time,
                next_payout_timestamp: entry.next_payout_timestamp,
                action: entry.action,
                id: entry.id,
            });

            count += 1;
            if count >= max_entries {
                break;
            }
        }

        audit_trail
    }

    //-----------------------------------------------------------------------------
    // Template and Preset Functions
    //-----------------------------------------------------------------------------

    /// Create a new payroll template
    pub fn create_template(
        env: Env,
        caller: Address,
        name: String,
        description: String,
        token: Address,
        amount: i128,
        interval: u64,
        recurrence_frequency: u64,
        is_public: bool,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        // Validate template data
        if name.len() == 0 || name.len() > 100 {
            return Err(PayrollError::InvalidTemplateName);
        }

        if amount <= 0 || interval == 0 || recurrence_frequency == 0 {
            return Err(PayrollError::TemplateValidationFailed);
        }

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get next template ID
        let next_id = storage.get(&DataKey::NextTemplateId).unwrap_or(0) + 1;
        storage.set(&DataKey::NextTemplateId, &next_id);

        let template = PayrollTemplate {
            id: next_id,
            name: name.clone(),
            description: description.clone(),
            employer: caller.clone(),
            token: token.clone(),
            amount,
            interval,
            recurrence_frequency,
            is_public,
            created_at: current_time,
            updated_at: current_time,
            usage_count: 0,
        };

        // Store template
        storage.set(&DataKey::PayrollTemplate(next_id), &template);

        // Add to employer's templates
        let mut employer_templates: Vec<u64> = storage.get(&DataKey::EmployerTemplates(caller.clone())).unwrap_or(Vec::new(&env));
        employer_templates.push_back(next_id);
        storage.set(&DataKey::EmployerTemplates(caller.clone()), &employer_templates);

        // Add to public templates if public
        if is_public {
            let mut public_templates: Vec<u64> = storage.get(&DataKey::PublicTemplates).unwrap_or(Vec::new(&env));
            public_templates.push_back(next_id);
            storage.set(&DataKey::PublicTemplates, &public_templates);
        }

        env.events().publish(
            (TEMPLATE_CREATED_EVENT,),
            (caller.clone(), next_id, name, is_public),
        );

        Ok(next_id)
    }

    /// Get a template by ID
    pub fn get_template(env: Env, template_id: u64) -> Result<PayrollTemplate, PayrollError> {
        let storage = env.storage().persistent();
        storage.get(&DataKey::PayrollTemplate(template_id))
            .ok_or(PayrollError::TemplateNotFound)
    }

    /// Apply a template to create a payroll
    pub fn apply_template(
        env: Env,
        caller: Address,
        template_id: u64,
        employee: Address,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let template: PayrollTemplate = storage.get(&DataKey::PayrollTemplate(template_id))
            .ok_or(PayrollError::TemplateNotFound)?;

        // Check if template is accessible (owner or public)
        if template.employer != caller && !template.is_public {
            return Err(PayrollError::TemplateNotPublic);
        }

        // Create payroll from template
        let payroll = Payroll {
            employer: caller.clone(),
            token: template.token.clone(),
            amount: template.amount,
            interval: template.interval,
            last_payment_time: env.ledger().timestamp(),
            recurrence_frequency: template.recurrence_frequency,
            next_payout_timestamp: env.ledger().timestamp() + template.recurrence_frequency,
            is_paused: false,
        };

        // Store payroll
        storage.set(&DataKey::Payroll(employee.clone()), &payroll);

        // Update indexes
        Self::add_to_employer_index(&env, &caller, &employee);

        // Update template usage count
        let mut updated_template = template.clone();
        updated_template.usage_count += 1;
        updated_template.updated_at = env.ledger().timestamp();
        storage.set(&DataKey::PayrollTemplate(template_id), &updated_template);

        env.events().publish(
            (TEMPLATE_APPLIED_EVENT,),
            (caller.clone(), template_id, employee.clone()),
        );

        Ok(())
    }

    /// Update an existing template
    pub fn update_template(
        env: Env,
        caller: Address,
        template_id: u64,
        name: Option<String>,
        description: Option<String>,
        amount: Option<i128>,
        interval: Option<u64>,
        recurrence_frequency: Option<u64>,
        is_public: Option<bool>,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let mut template: PayrollTemplate = storage.get(&DataKey::PayrollTemplate(template_id))
            .ok_or(PayrollError::TemplateNotFound)?;

        // Only template owner can update
        if template.employer != caller {
            return Err(PayrollError::Unauthorized);
        }

        // Update fields if provided
        if let Some(new_name) = name {
            if new_name.len() == 0 || new_name.len() > 100 {
                return Err(PayrollError::InvalidTemplateName);
            }
            template.name = new_name;
        }

        if let Some(new_description) = description {
            template.description = new_description;
        }

        if let Some(new_amount) = amount {
            if new_amount <= 0 {
                return Err(PayrollError::TemplateValidationFailed);
            }
            template.amount = new_amount;
        }

        if let Some(new_interval) = interval {
            if new_interval == 0 {
                return Err(PayrollError::TemplateValidationFailed);
            }
            template.interval = new_interval;
        }

        if let Some(new_frequency) = recurrence_frequency {
            if new_frequency == 0 {
                return Err(PayrollError::TemplateValidationFailed);
            }
            template.recurrence_frequency = new_frequency;
        }

        if let Some(new_public) = is_public {
            // Handle public status change
            if template.is_public != new_public {
                let mut public_templates: Vec<u64> = storage.get(&DataKey::PublicTemplates).unwrap_or(Vec::new(&env));
                
                if new_public {
                    // Add to public templates
                    public_templates.push_back(template_id);
                } else {
                    // Remove from public templates
                    let mut new_public_templates = Vec::new(&env);
                    for id in public_templates.iter() {
                        if id != template_id {
                            new_public_templates.push_back(id);
                        }
                    }
                    public_templates = new_public_templates;
                }
                storage.set(&DataKey::PublicTemplates, &public_templates);
            }
            template.is_public = new_public;
        }

        template.updated_at = env.ledger().timestamp();
        storage.set(&DataKey::PayrollTemplate(template_id), &template);

        env.events().publish(
            (TEMPLATE_UPDATED_EVENT,),
            (caller.clone(), template_id),
        );

        Ok(())
    }

    /// Share a template with another employer
    pub fn share_template(
        env: Env,
        caller: Address,
        template_id: u64,
        target_employer: Address,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let template: PayrollTemplate = storage.get(&DataKey::PayrollTemplate(template_id))
            .ok_or(PayrollError::TemplateNotFound)?;

        // Only template owner can share
        if template.employer != caller {
            return Err(PayrollError::Unauthorized);
        }

        // Add to target employer's templates (create a copy)
        let mut target_templates: Vec<u64> = storage.get(&DataKey::EmployerTemplates(target_employer.clone())).unwrap_or(Vec::new(&env));
        
        // Create a new template ID for the shared copy
        let next_id = storage.get(&DataKey::NextTemplateId).unwrap_or(0) + 1;
        storage.set(&DataKey::NextTemplateId, &next_id);

        let shared_template = PayrollTemplate {
            id: next_id,
            name: template.name.clone(),
            description: template.description.clone(),
            employer: target_employer.clone(),
            token: template.token.clone(),
            amount: template.amount,
            interval: template.interval,
            recurrence_frequency: template.recurrence_frequency,
            is_public: false, // Shared templates are private by default
            created_at: env.ledger().timestamp(),
            updated_at: env.ledger().timestamp(),
            usage_count: 0,
        };

        storage.set(&DataKey::PayrollTemplate(next_id), &shared_template);
        target_templates.push_back(next_id);
        storage.set(&DataKey::EmployerTemplates(target_employer.clone()), &target_templates);

        env.events().publish(
            (TEMPLATE_SHARED_EVENT,),
            (caller.clone(), template_id, target_employer.clone(), next_id),
        );

        Ok(())
    }

    /// Get all templates for an employer
    pub fn get_employer_templates(env: Env, employer: Address) -> Vec<PayrollTemplate> {
        let storage = env.storage().persistent();
        let template_ids: Vec<u64> = storage.get(&DataKey::EmployerTemplates(employer.clone())).unwrap_or(Vec::new(&env));
        let mut templates = Vec::new(&env);

        for id in template_ids.iter() {
            if let Some(template) = storage.get(&DataKey::PayrollTemplate(id)) {
                templates.push_back(template);
            }
        }

        templates
    }

    /// Get all public templates
    pub fn get_public_templates(env: Env) -> Vec<PayrollTemplate> {
        let storage = env.storage().persistent();
        let template_ids: Vec<u64> = storage.get(&DataKey::PublicTemplates).unwrap_or(Vec::new(&env));
        let mut templates = Vec::new(&env);

        for id in template_ids.iter() {
            if let Some(template) = storage.get(&DataKey::PayrollTemplate(id)) {
                templates.push_back(template);
            }
        }

        templates
    }

    /// Create a template preset (admin function)
    pub fn create_preset(
        env: Env,
        caller: Address,
        name: String,
        description: String,
        token: Address,
        amount: i128,
        interval: u64,
        recurrence_frequency: u64,
        category: String,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        // Only owner can create presets
        let storage = env.storage().persistent();
        let owner = storage.get::<DataKey, Address>(&DataKey::Owner).unwrap();
        if caller != owner {
            return Err(PayrollError::Unauthorized);
        }

        // Validate preset data
        if name.len() == 0 || name.len() > 100 {
            return Err(PayrollError::InvalidTemplateName);
        }

        if amount <= 0 || interval == 0 || recurrence_frequency == 0 {
            return Err(PayrollError::TemplateValidationFailed);
        }

        let current_time = env.ledger().timestamp();

        // Get next preset ID
        let next_id = storage.get(&DataKey::NextPresetId).unwrap_or(0) + 1;
        storage.set(&DataKey::NextPresetId, &next_id);

        let preset = TemplatePreset {
            id: next_id,
            name: name.clone(),
            description: description.clone(),
            token: token.clone(),
            amount,
            interval,
            recurrence_frequency,
            category: category.clone(),
            is_active: true,
            created_at: current_time,
        };

        // Store preset
        storage.set(&DataKey::TemplatePreset(next_id), &preset);

        // Add to category
        let mut category_presets: Vec<u64> = storage.get(&DataKey::PresetCategory(category.clone())).unwrap_or(Vec::new(&env));
        category_presets.push_back(next_id);
        storage.set(&DataKey::PresetCategory(category.clone()), &category_presets);

        // Add to active presets
        let mut active_presets: Vec<u64> = storage.get(&DataKey::ActivePresets).unwrap_or(Vec::new(&env));
        active_presets.push_back(next_id);
        storage.set(&DataKey::ActivePresets, &active_presets);

        env.events().publish(
            (PRESET_CREATED_EVENT,),
            (next_id, name, category),
        );

        Ok(next_id)
    }

    /// Get a preset by ID
    pub fn get_preset(env: Env, preset_id: u64) -> Result<TemplatePreset, PayrollError> {
        let storage = env.storage().persistent();
        storage.get(&DataKey::TemplatePreset(preset_id))
            .ok_or(PayrollError::PresetNotFound)
    }

    /// Apply a preset to create a template
    pub fn apply_preset(
        env: Env,
        caller: Address,
        preset_id: u64,
        name: String,
        description: String,
        is_public: bool,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let preset: TemplatePreset = storage.get(&DataKey::TemplatePreset(preset_id))
            .ok_or(PayrollError::PresetNotFound)?;

        if !preset.is_active {
            return Err(PayrollError::PresetNotActive);
        }

        // Create template from preset
        Self::create_template(
            env,
            caller,
            name,
            description,
            preset.token.clone(),
            preset.amount,
            preset.interval,
            preset.recurrence_frequency,
            is_public,
        )
    }

    /// Get presets by category
    pub fn get_presets_by_category(env: Env, category: String) -> Vec<TemplatePreset> {
        let storage = env.storage().persistent();
        let preset_ids: Vec<u64> = storage.get(&DataKey::PresetCategory(category.clone())).unwrap_or(Vec::new(&env));
        let mut presets = Vec::new(&env);

        for id in preset_ids.iter() {
            if let Some(preset) = storage.get(&DataKey::TemplatePreset(id)) {
                presets.push_back(preset);
            }
        }

        presets
    }

    /// Get all active presets
    pub fn get_active_presets(env: Env) -> Vec<TemplatePreset> {
        let storage = env.storage().persistent();
        let preset_ids: Vec<u64> = storage.get(&DataKey::ActivePresets).unwrap_or(Vec::new(&env));
        let mut presets = Vec::new(&env);

        for id in preset_ids.iter() {
            if let Some(preset) = storage.get(&DataKey::TemplatePreset(id)) {
                presets.push_back(preset);
            }
        }

        presets
    }
}
