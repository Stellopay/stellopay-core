use soroban_sdk::{
    contract, contracterror, contractimpl, symbol_short, token::Client as TokenClient, Address,
    Env, Symbol, Vec, String, Map,
};

use crate::events::{emit_disburse, DEPOSIT_EVENT, PAUSED_EVENT, UNPAUSED_EVENT, EMPLOYEE_PAUSED_EVENT, EMPLOYEE_RESUMED_EVENT, TEMPLATE_CREATED_EVENT, TEMPLATE_UPDATED_EVENT, TEMPLATE_APPLIED_EVENT, TEMPLATE_SHARED_EVENT, PRESET_CREATED_EVENT, BACKUP_CREATED_EVENT, BACKUP_VERIFIED_EVENT, RECOVERY_STARTED_EVENT, RECOVERY_COMPLETED_EVENT, SCHEDULE_CREATED_EVENT, SCHEDULE_UPDATED_EVENT, SCHEDULE_EXECUTED_EVENT, RULE_CREATED_EVENT, RULE_EXECUTED_EVENT, ROLE_ASSIGNED_EVENT, ROLE_REVOKED_EVENT, SECURITY_AUDIT_EVENT, SECURITY_POLICY_VIOLATION_EVENT};
use crate::storage::{DataKey, Payroll, PayrollInput, CompactPayroll, CompactPayrollHistoryEntry, PayrollTemplate, TemplatePreset, PayrollBackup, BackupData, BackupMetadata, BackupType, BackupStatus, RecoveryPoint, RecoveryType, RecoveryStatus, RecoveryMetadata, PayrollSchedule, ScheduleType, ScheduleFrequency, ScheduleMetadata, AutomationRule, RuleType, RuleCondition, RuleAction, ConditionOperator, LogicalOperator, ActionType, UserRole, Permission, Role, UserRoleAssignment, SecurityPolicy, SecurityPolicyType, SecurityRule, SecurityRuleOperator, SecurityRuleAction, SecurityAuditEntry, SecurityAuditResult, RateLimitConfig, SecuritySettings, SuspiciousActivity, SuspiciousActivityType, SuspiciousActivitySeverity};
use crate::insurance::{InsuranceSystem, InsuranceError, InsurancePolicy, InsuranceClaim, Guarantee, InsuranceSettings};
use crate::enterprise::{self, Department, ApprovalWorkflow, ApprovalStep, WebhookEndpoint, ReportTemplate, BackupSchedule, EnterpriseDataKey, PayrollModificationRequest, PayrollModificationType, PayrollModificationStatus, Approval, ApprovalStatus, Dispute, DisputeType, DisputeStatus, DisputePriority, Escalation, EscalationLevel, Mediator, DisputeSettings};

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
    /// Backup not found
    BackupNotFound = 17,
    /// Backup creation failed
    BackupCreationFailed = 18,
    /// Backup verification failed
    BackupVerificationFailed = 19,
    /// Recovery point not found
    RecoveryPointNotFound = 20,
    /// Recovery failed
    RecoveryFailed = 21,
    /// Backup data corrupted
    BackupDataCorrupted = 22,
    /// Insufficient storage for backup
    InsufficientBackupStorage = 23,
    /// Backup already exists
    BackupAlreadyExists = 24,
    /// Recovery in progress
    RecoveryInProgress = 25,
    /// Schedule not found
    ScheduleNotFound = 26,
    /// Schedule creation failed
    ScheduleCreationFailed = 27,
    /// Schedule validation failed
    ScheduleValidationFailed = 28,
    /// Automation rule not found
    AutomationRuleNotFound = 29,
    /// Rule execution failed
    RuleExecutionFailed = 30,
    /// Invalid schedule frequency
    InvalidScheduleFrequency = 31,
    /// Schedule already exists
    ScheduleAlreadyExists = 32,
    /// Schedule execution failed
    ScheduleExecutionFailed = 33,
    /// Invalid automation rule
    InvalidAutomationRule = 34,
    /// Rule condition evaluation failed
    RuleConditionEvaluationFailed = 35,
    /// Security policy violation
    SecurityPolicyViolation = 36,
    /// Role not found
    RoleNotFound = 37,
    /// Insufficient permissions
    InsufficientPermissions = 38,
    /// Security audit failed
    SecurityAuditFailed = 39,
    /// Rate limit exceeded
    RateLimitExceeded = 40,
    /// Suspicious activity detected
    SuspiciousActivityDetected = 41,
    /// Access denied by security policy
    AccessDeniedByPolicy = 42,
    /// Security token invalid
    SecurityTokenInvalid = 43,
    /// Multi-factor authentication required
    MFARequired = 44,
    /// Session expired
    SessionExpired = 45,
    /// IP address blocked
    IPAddressBlocked = 46,
    /// Account locked
    AccountLocked = 47,
    /// Security clearance insufficient
    SecurityClearanceInsufficient = 48,
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

/// Event emitted when automatic disbursement is triggered
pub const AUTO_DISBURSE_EVENT: Symbol = symbol_short!("auto_d");

/// Event emitted when access is denied
pub const ACCESS_DENIED_EVENT: Symbol = symbol_short!("acc_den");

/// Event emitted when suspicious activity is detected
pub const SUSPICIOUS_ACTIVITY_EVENT: Symbol = symbol_short!("susp_act");

/// Event emitted when rate limit is exceeded
pub const RATE_LIMIT_EXCEEDED_EVENT: Symbol = symbol_short!("rate_lim");

/// Event emitted when account is locked
pub const ACCOUNT_LOCKED_EVENT: Symbol = symbol_short!("acc_lck");

/// Event emitted when a backup is restored
pub const BACKUP_RESTORED_EVENT: Symbol = symbol_short!("backup_r");

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
    /// Optimized with batch size limits and improved gas efficiency
    pub fn batch_create_escrows(
        env: Env,
        employer: Address,
        payroll_inputs: Vec<PayrollInput>,
    ) -> Result<Vec<Payroll>, PayrollError> {
        employer.require_auth();

        // Batch size limit for gas optimization (configurable)
        const MAX_BATCH_SIZE: u32 = 50;
        if payroll_inputs.len() as u32 > MAX_BATCH_SIZE {
            return Err(PayrollError::InvalidData);
        }

        // Create optimized batch context
        let batch_ctx = Self::create_batch_context(&env);
        let storage = env.storage().persistent();
        let is_owner = batch_ctx.cache.owner.as_ref().map_or(false, |owner| &employer == owner);

        let mut created_payrolls = Vec::new(&env);
        let mut supported_tokens = Vec::new(&env);

        // Pre-validate all inputs to fail fast
        for payroll_input in payroll_inputs.iter() {
            Self::validate_payroll_input(
                payroll_input.amount,
                payroll_input.interval,
                payroll_input.recurrence_frequency,
            )?;
        }

        for payroll_input in payroll_inputs.iter() {
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

            // Track tokens to add as supported (batch operation)
            if !Self::is_token_supported(env.clone(), payroll_input.token.clone()) {
                supported_tokens.push_back(payroll_input.token.clone());
            }

            created_payrolls.push_back(payroll);
        }

        // Batch add supported tokens (more gas efficient)
        for token in supported_tokens.iter() {
            let key = DataKey::SupportedToken(token.clone());
            storage.set(&key, &true);

            // Set default decimals (7 for Stellar assets)
            let metadata_key = DataKey::TokenMetadata(token.clone());
            storage.set(&metadata_key, &7u32);
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
    /// Optimized with batch size limits and improved gas efficiency
    pub fn batch_disburse_salaries(
        env: Env,
        caller: Address,
        employees: Vec<Address>,
    ) -> Result<Vec<Address>, PayrollError> {
        caller.require_auth();

        // Batch size limit for gas optimization (configurable)
        const MAX_BATCH_SIZE: u32 = 50;
        if employees.len() as u32 > MAX_BATCH_SIZE {
            return Err(PayrollError::InvalidData);
        }

        // Create optimized batch context
        let batch_ctx = Self::create_batch_context(&env);
        let storage = env.storage().persistent();
        let mut processed_employees = Vec::new(&env);

        // Process each employee individually to avoid indexing issues
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

    /// Batch pause payrolls for multiple employees
    /// Optimized with batch size limits and improved gas efficiency
    pub fn batch_pause_payrolls(
        env: Env,
        caller: Address,
        employees: Vec<Address>,
    ) -> Result<Vec<Address>, PayrollError> {
        caller.require_auth();

        // Batch size limit for gas optimization (configurable)
        const MAX_BATCH_SIZE: u32 = 50;
        if employees.len() as u32 > MAX_BATCH_SIZE {
            return Err(PayrollError::InvalidData);
        }

        let storage = env.storage().persistent();
        let cache = Self::get_contract_cache(&env);
        let mut processed_employees = Vec::new(&env);

        // Process each employee individually to avoid indexing issues
        for employee in employees.iter() {
            let payroll = Self::_get_payroll(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;

            // Check if caller is authorized (owner or employer)
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

            // Emit individual pause event
            env.events().publish(
                (EMPLOYEE_PAUSED_EVENT,),
                (caller.clone(), employee.clone()),
            );

            processed_employees.push_back(employee.clone());
        }

        // Emit batch pause event
        env.events().publish(
            (BATCH_EVENT,),
            (caller, processed_employees.len() as u32),
        );

        Ok(processed_employees)
    }

    /// Batch resume payrolls for multiple employees
    /// Optimized with batch size limits and improved gas efficiency
    pub fn batch_resume_payrolls(
        env: Env,
        caller: Address,
        employees: Vec<Address>,
    ) -> Result<Vec<Address>, PayrollError> {
        caller.require_auth();

        // Batch size limit for gas optimization (configurable)
        const MAX_BATCH_SIZE: u32 = 50;
        if employees.len() as u32 > MAX_BATCH_SIZE {
            return Err(PayrollError::InvalidData);
        }

        let storage = env.storage().persistent();
        let cache = Self::get_contract_cache(&env);
        let mut processed_employees = Vec::new(&env);

        // Process each employee individually to avoid indexing issues
        for employee in employees.iter() {
            let payroll = Self::_get_payroll(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;

            // Check if caller is authorized (owner or employer)
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

            // Emit individual resume event
            env.events().publish(
                (EMPLOYEE_RESUMED_EVENT,),
                (caller.clone(), employee.clone()),
            );

            processed_employees.push_back(employee.clone());
        }

        // Emit batch resume event
        env.events().publish(
            (BATCH_EVENT,),
            (caller, processed_employees.len() as u32),
        );

        Ok(processed_employees)
    }

    /// Batch remove payrolls for multiple employees
    /// Optimized with batch size limits and improved gas efficiency
    pub fn batch_remove_payrolls(
        env: Env,
        caller: Address,
        employees: Vec<Address>,
    ) -> Result<Vec<Address>, PayrollError> {
        caller.require_auth();

        // Batch size limit for gas optimization (configurable)
        const MAX_BATCH_SIZE: u32 = 50;
        if employees.len() as u32 > MAX_BATCH_SIZE {
            return Err(PayrollError::InvalidData);
        }

        let storage = env.storage().persistent();
        let owner = storage.get::<DataKey, Address>(&DataKey::Owner).unwrap();
        let mut processed_employees = Vec::new(&env);

        // Process each employee individually to avoid indexing issues
        for employee in employees.iter() {
            let payroll = Self::_get_payroll(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;

            // Only the contract owner or the payroll's employer can remove it
            if caller != owner && caller != payroll.employer {
                return Err(PayrollError::Unauthorized);
            }

            // Remove from indexes
            Self::remove_from_employer_index(&env, &payroll.employer, &employee);
            Self::remove_from_token_index(&env, &payroll.token, &employee);

            // Remove payroll data
            storage.remove(&DataKey::Payroll(employee.clone()));

            processed_employees.push_back(employee.clone());
        }

        // Emit batch remove event
        env.events().publish(
            (BATCH_EVENT,),
            (caller, processed_employees.len() as u32),
        );

        Ok(processed_employees)
    }

    /// Estimate gas cost for batch operations
    pub fn estimate_batch_gas(
        env: Env,
        operation_type: String,
        batch_size: u32,
    ) -> Result<u64, PayrollError> {
        // Base gas costs for different operations
        const BASE_CREATE_GAS: u64 = 1000;
        const BASE_DISBURSE_GAS: u64 = 800;
        const BASE_PAUSE_GAS: u64 = 300;
        const BASE_RESUME_GAS: u64 = 300;
        const BASE_REMOVE_GAS: u64 = 400;
        const PER_ITEM_GAS: u64 = 50;

        // Simplified operation type matching
        let base_gas = if operation_type == String::from_str(&env, "create") {
            BASE_CREATE_GAS
        } else if operation_type == String::from_str(&env, "disburse") {
            BASE_DISBURSE_GAS
        } else if operation_type == String::from_str(&env, "pause") {
            BASE_PAUSE_GAS
        } else if operation_type == String::from_str(&env, "resume") {
            BASE_RESUME_GAS
        } else if operation_type == String::from_str(&env, "remove") {
            BASE_REMOVE_GAS
        } else {
            return Err(PayrollError::InvalidData);
        };

        let estimated_gas = base_gas + (batch_size as u64 * PER_ITEM_GAS);
        Ok(estimated_gas)
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
        let id_key = DataKey::PayrollHistoryCounter(employee.clone());
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
        let id_key = DataKey::AuditIdCounter(employee.clone());
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
    // Webhook Integration Functions
    //-----------------------------------------------------------------------------

    /// Register a webhook for external service integration
    pub fn register_webhook(
        env: Env,
        owner: Address,
        url: String,
        events: Vec<crate::webhooks_simple::EventType>,
        secret: String,
    ) -> Result<u64, crate::webhooks_simple::WebhookError> {
        crate::webhooks_simple::WebhookSystem::register_webhook(&env, owner, url, events, secret)
    }

    /// Get webhook information
    pub fn get_webhook(env: Env, webhook_id: u64) -> Result<crate::webhooks_simple::Webhook, crate::webhooks_simple::WebhookError> {
        crate::webhooks_simple::WebhookSystem::get_webhook(&env, webhook_id)
    }

    /// Delete a webhook
    pub fn delete_webhook(
        env: Env,
        owner: Address,
        webhook_id: u64,
    ) -> Result<(), crate::webhooks_simple::WebhookError> {
        crate::webhooks_simple::WebhookSystem::delete_webhook(&env, owner, webhook_id)
    }

    /// Trigger webhook notification for salary disbursement
    pub fn notify_salary_disbursed(
        env: Env,
        employer: Address,
        employee: Address,
        amount: i128,
    ) {
        // Create event data and publish for webhook processing
        env.events().publish(
            (symbol_short!("wh_salary"),), 
            (employer, employee, amount)
        );
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
        let next_id = storage.get(&DataKey::NextTmplId).unwrap_or(0) + 1;
        storage.set(&DataKey::NextTmplId, &next_id);

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
        storage.set(&DataKey::Template(next_id), &template);

        // Add to employer's templates
        let mut employer_templates: Vec<u64> = storage.get(&DataKey::EmpTemplates(caller.clone())).unwrap_or(Vec::new(&env));
        employer_templates.push_back(next_id);
        storage.set(&DataKey::EmpTemplates(caller.clone()), &employer_templates);

        // Add to public templates if public
        if is_public {
            let mut public_templates: Vec<u64> = storage.get(&DataKey::PubTemplates).unwrap_or(Vec::new(&env));
            public_templates.push_back(next_id);
            storage.set(&DataKey::PubTemplates, &public_templates);
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
        storage.get(&DataKey::Template(template_id))
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
        let template: PayrollTemplate = storage.get(&DataKey::Template(template_id))
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
        storage.set(&DataKey::Template(template_id), &updated_template);

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
        let mut template: PayrollTemplate = storage.get(&DataKey::Template(template_id))
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
                let mut public_templates: Vec<u64> = storage.get(&DataKey::PubTemplates).unwrap_or(Vec::new(&env));
                
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
                storage.set(&DataKey::PubTemplates, &public_templates);
            }
            template.is_public = new_public;
        }

        template.updated_at = env.ledger().timestamp();
        storage.set(&DataKey::Template(template_id), &template);

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
        let template: PayrollTemplate = storage.get(&DataKey::Template(template_id))
            .ok_or(PayrollError::TemplateNotFound)?;

        // Only template owner can share
        if template.employer != caller {
            return Err(PayrollError::Unauthorized);
        }

        // Add to target employer's templates (create a copy)
        let mut target_templates: Vec<u64> = storage.get(&DataKey::EmpTemplates(target_employer.clone())).unwrap_or(Vec::new(&env));
        
        // Create a new template ID for the shared copy
        let next_id = storage.get(&DataKey::NextTmplId).unwrap_or(0) + 1;
        storage.set(&DataKey::NextTmplId, &next_id);

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

        storage.set(&DataKey::Template(next_id), &shared_template);
        target_templates.push_back(next_id);
        storage.set(&DataKey::EmpTemplates(target_employer.clone()), &target_templates);

        env.events().publish(
            (TEMPLATE_SHARED_EVENT,),
            (caller.clone(), template_id, target_employer.clone(), next_id),
        );

        Ok(())
    }

    /// Get all templates for an employer
    pub fn get_employer_templates(env: Env, employer: Address) -> Vec<PayrollTemplate> {
        let storage = env.storage().persistent();
        let template_ids: Vec<u64> = storage.get(&DataKey::EmpTemplates(employer.clone())).unwrap_or(Vec::new(&env));
        let mut templates = Vec::new(&env);

        for id in template_ids.iter() {
            if let Some(template) = storage.get(&DataKey::Template(id)) {
                templates.push_back(template);
            }
        }

        templates
    }

    /// Get all public templates
    pub fn get_public_templates(env: Env) -> Vec<PayrollTemplate> {
        let storage = env.storage().persistent();
        let template_ids: Vec<u64> = storage.get(&DataKey::PubTemplates).unwrap_or(Vec::new(&env));
        let mut templates = Vec::new(&env);

        for id in template_ids.iter() {
            if let Some(template) = storage.get(&DataKey::Template(id)) {
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
        storage.set(&DataKey::Preset(next_id), &preset);

        // Add to category
        let mut category_presets: Vec<u64> = storage.get(&DataKey::PresetCat(category.clone())).unwrap_or(Vec::new(&env));
        category_presets.push_back(next_id);
        storage.set(&DataKey::PresetCat(category.clone()), &category_presets);

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
        storage.get(&DataKey::Preset(preset_id))
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
        let preset: TemplatePreset = storage.get(&DataKey::Preset(preset_id))
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
        let preset_ids: Vec<u64> = storage.get(&DataKey::PresetCat(category.clone())).unwrap_or(Vec::new(&env));
        let mut presets = Vec::new(&env);

        for id in preset_ids.iter() {
            if let Some(preset) = storage.get(&DataKey::Preset(id)) {
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
            if let Some(preset) = storage.get(&DataKey::Preset(id)) {
                presets.push_back(preset);
            }
        }

        presets
    }

    //-----------------------------------------------------------------------------
    // Backup and Recovery Functions
    //-----------------------------------------------------------------------------

    /// Create a new payroll backup
    pub fn create_backup(
        env: Env,
        caller: Address,
        name: String,
        description: String,
        backup_type: BackupType,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        // Validate backup name
        if name.len() == 0 || name.len() > 100 {
            return Err(PayrollError::InvalidTemplateName);
        }

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get next backup ID
        let next_id = storage.get(&DataKey::NextBackupId).unwrap_or(0) + 1;
        storage.set(&DataKey::NextBackupId, &next_id);

        // Create backup metadata
        let backup = PayrollBackup {
            id: next_id,
            name: name.clone(),
            description: description.clone(),
            employer: caller.clone(),
            created_at: current_time,
            backup_type: backup_type.clone(),
            status: BackupStatus::Creating,
            checksum: String::from_str(&env, ""),
            data_hash: String::from_str(&env, ""),
            size_bytes: 0,
            version: 1,
        };

        // Store backup metadata
        storage.set(&DataKey::Backup(next_id), &backup);

        // Add to employer's backups
        let mut employer_backups: Vec<u64> = storage.get(&DataKey::EmpBackups(caller.clone())).unwrap_or(Vec::new(&env));
        employer_backups.push_back(next_id);
        storage.set(&DataKey::EmpBackups(caller.clone()), &employer_backups);

        // Add to backup index
        let mut backup_index: Vec<u64> = storage.get(&DataKey::BackupIndex).unwrap_or(Vec::new(&env));
        backup_index.push_back(next_id);
        storage.set(&DataKey::BackupIndex, &backup_index);

        // Create backup data based on type
        let backup_data = Self::_collect_backup_data(&env, &caller, &backup_type)?;
        
        // Calculate checksum and hash
        let checksum = Self::_calculate_backup_checksum(&env, &backup_data);
        let data_hash = Self::_calculate_data_hash(&env, &backup_data);
        let size_bytes = Self::_calculate_backup_size(&env, &backup_data);

        // Store backup data
        storage.set(&DataKey::BackupData(next_id), &backup_data);

        // Update backup with final metadata
        let mut final_backup = backup.clone();
        final_backup.status = BackupStatus::Completed;
        final_backup.checksum = checksum;
        final_backup.data_hash = data_hash;
        final_backup.size_bytes = size_bytes;
        storage.set(&DataKey::Backup(next_id), &final_backup);

        env.events().publish(
            (BACKUP_CREATED_EVENT,),
            (caller.clone(), next_id, name, backup_type),
        );

        Ok(next_id)
    }

    /// Get a backup by ID
    pub fn get_backup(env: Env, backup_id: u64) -> Result<PayrollBackup, PayrollError> {
        let storage = env.storage().persistent();
        storage.get(&DataKey::Backup(backup_id))
            .ok_or(PayrollError::BackupNotFound)
    }

    /// Get backup data by ID
    pub fn get_backup_data(env: Env, backup_id: u64) -> Result<BackupData, PayrollError> {
        let storage = env.storage().persistent();
        storage.get(&DataKey::BackupData(backup_id))
            .ok_or(PayrollError::BackupNotFound)
    }

    /// Verify a backup's integrity
    pub fn verify_backup(
        env: Env,
        caller: Address,
        backup_id: u64,
    ) -> Result<bool, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let backup: PayrollBackup = storage.get(&DataKey::Backup(backup_id))
            .ok_or(PayrollError::BackupNotFound)?;

        // Only backup owner can verify
        if backup.employer != caller {
            return Err(PayrollError::Unauthorized);
        }

        let backup_data: BackupData = storage.get(&DataKey::BackupData(backup_id))
            .ok_or(PayrollError::BackupNotFound)?;

        // Calculate current checksum
        let current_checksum = Self::_calculate_backup_checksum(&env, &backup_data);
        let current_hash = Self::_calculate_data_hash(&env, &backup_data);

        // Verify checksum and hash
        let is_valid = backup.checksum == current_checksum && backup.data_hash == current_hash;

        // Update backup status
        let mut updated_backup = backup.clone();
        updated_backup.status = if is_valid { BackupStatus::Verified } else { BackupStatus::Failed };
        storage.set(&DataKey::Backup(backup_id), &updated_backup);

        env.events().publish(
            (BACKUP_VERIFIED_EVENT,),
            (caller.clone(), backup_id, is_valid),
        );

        Ok(is_valid)
    }

    /// Create a recovery point from a backup
    pub fn create_recovery_point(
        env: Env,
        caller: Address,
        backup_id: u64,
        name: String,
        description: String,
        recovery_type: RecoveryType,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        // Verify backup exists and is valid
        let backup: PayrollBackup = Self::get_backup(env.clone(), backup_id)?;
        if backup.employer != caller {
            return Err(PayrollError::Unauthorized);
        }

        if backup.status != BackupStatus::Completed && backup.status != BackupStatus::Verified {
            return Err(PayrollError::BackupVerificationFailed);
        }

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get next recovery point ID
        let next_id = storage.get(&DataKey::NextRecoveryId).unwrap_or(0) + 1;
        storage.set(&DataKey::NextRecoveryId, &next_id);

        let recovery_point = RecoveryPoint {
            id: next_id,
            name: name.clone(),
            description: description.clone(),
            created_at: current_time,
            backup_id,
            recovery_type: recovery_type.clone(),
            status: RecoveryStatus::Pending,
            checksum: backup.checksum.clone(),
            metadata: RecoveryMetadata {
                total_operations: 0,
                success_count: 0,
                failure_count: 0,
                recovery_timestamp: current_time,
                duration_seconds: 0,
                data_verification_status: String::from_str(&env, "pending"),
            },
        };

        storage.set(&DataKey::Recovery(next_id), &recovery_point);

        env.events().publish(
            (RECOVERY_STARTED_EVENT,),
            (caller.clone(), next_id, backup_id, recovery_type),
        );

        Ok(next_id)
    }

    /// Execute recovery from a recovery point
    pub fn execute_recovery(
        env: Env,
        caller: Address,
        recovery_point_id: u64,
    ) -> Result<bool, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let mut recovery_point: RecoveryPoint = storage.get(&DataKey::Recovery(recovery_point_id))
            .ok_or(PayrollError::RecoveryPointNotFound)?;

        // Check if recovery is already in progress
        if recovery_point.status == RecoveryStatus::InProgress {
            return Err(PayrollError::RecoveryInProgress);
        }

        // Get backup data
        let backup_data: BackupData = storage.get(&DataKey::BackupData(recovery_point.backup_id))
            .ok_or(PayrollError::BackupNotFound)?;

        // Update recovery status
        recovery_point.status = RecoveryStatus::InProgress;
        storage.set(&DataKey::Recovery(recovery_point_id), &recovery_point);

        let start_time = env.ledger().timestamp();
        let mut success_count = 0;
        let mut failure_count = 0;

        // Restore payroll data
        for payroll in backup_data.payroll_data.iter() {
            match Self::_restore_payroll(&env, &payroll) {
                Ok(_) => success_count += 1,
                Err(_) => failure_count += 1,
            }
        }

        // Restore template data
        for template in backup_data.template_data.iter() {
            match Self::_restore_template(&env, &template) {
                Ok(_) => success_count += 1,
                Err(_) => failure_count += 1,
            }
        }

        // Restore preset data
        for preset in backup_data.preset_data.iter() {
            match Self::_restore_preset(&env, &preset) {
                Ok(_) => success_count += 1,
                Err(_) => failure_count += 1,
            }
        }

        let end_time = env.ledger().timestamp();
        let duration = end_time - start_time;

        // Update recovery point with results
        recovery_point.status = if failure_count == 0 { RecoveryStatus::Completed } else { RecoveryStatus::Failed };
        recovery_point.metadata.total_operations = success_count + failure_count;
        recovery_point.metadata.success_count = success_count;
        recovery_point.metadata.failure_count = failure_count;
        recovery_point.metadata.recovery_timestamp = end_time;
        recovery_point.metadata.duration_seconds = duration;
        recovery_point.metadata.data_verification_status = if failure_count == 0 { 
            String::from_str(&env, "verified") 
        } else { 
            String::from_str(&env, "failed") 
        };

        storage.set(&DataKey::Recovery(recovery_point_id), &recovery_point);

        env.events().publish(
            (RECOVERY_COMPLETED_EVENT,),
            (caller.clone(), recovery_point_id, success_count, failure_count, duration),
        );

        Ok(failure_count == 0)
    }

    /// Get all backups for an employer
    pub fn get_employer_backups(env: Env, employer: Address) -> Vec<PayrollBackup> {
        let storage = env.storage().persistent();
        let backup_ids: Vec<u64> = storage.get(&DataKey::EmpBackups(employer.clone())).unwrap_or(Vec::new(&env));
        let mut backups = Vec::new(&env);

        for id in backup_ids.iter() {
            if let Some(backup) = storage.get(&DataKey::Backup(id)) {
                backups.push_back(backup);
            }
        }

        backups
    }

    /// Get all recovery points
    pub fn get_recovery_points(env: Env) -> Vec<RecoveryPoint> {
        let storage = env.storage().persistent();
        let mut recovery_points = Vec::new(&env);
        let mut next_id = 1;

        // Iterate through recovery points (this is a simplified approach)
        while let Some(recovery_point) = storage.get(&DataKey::Recovery(next_id)) {
            recovery_points.push_back(recovery_point);
            next_id += 1;
        }

        recovery_points
    }

    /// Delete a backup
    pub fn delete_backup(
        env: Env,
        caller: Address,
        backup_id: u64,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let backup: PayrollBackup = storage.get(&DataKey::Backup(backup_id))
            .ok_or(PayrollError::BackupNotFound)?;

        // Only backup owner can delete
        if backup.employer != caller {
            return Err(PayrollError::Unauthorized);
        }

        // Remove from storage
        storage.remove(&DataKey::Backup(backup_id));
        storage.remove(&DataKey::BackupData(backup_id));

        // Remove from employer's backups
        let mut employer_backups: Vec<u64> = storage.get(&DataKey::EmpBackups(caller.clone())).unwrap_or(Vec::new(&env));
        let mut new_employer_backups = Vec::new(&env);
        for id in employer_backups.iter() {
            if id != backup_id {
                new_employer_backups.push_back(id);
            }
        }
        storage.set(&DataKey::EmpBackups(caller.clone()), &new_employer_backups);

        // Remove from backup index
        let mut backup_index: Vec<u64> = storage.get(&DataKey::BackupIndex).unwrap_or(Vec::new(&env));
        let mut new_backup_index = Vec::new(&env);
        for id in backup_index.iter() {
            if id != backup_id {
                new_backup_index.push_back(id);
            }
        }
        storage.set(&DataKey::BackupIndex, &new_backup_index);

        Ok(())
    }

    //-----------------------------------------------------------------------------
    // Internal Helper Functions for Backup and Recovery
    //-----------------------------------------------------------------------------

    /// Collect backup data based on backup type
    fn _collect_backup_data(
        env: &Env,
        employer: &Address,
        backup_type: &BackupType,
    ) -> Result<BackupData, PayrollError> {
        let storage = env.storage().persistent();
        let mut payroll_data = Vec::new(env);
        let mut template_data = Vec::new(env);
        let mut preset_data = Vec::new(env);
        let mut insurance_data = Vec::new(env);

        match backup_type {
            BackupType::Full => {
                // Collect all data
                let backup_index: Vec<u64> = storage.get(&DataKey::BackupIndex).unwrap_or(Vec::new(env));
                for backup_id in backup_index.iter() {
                    if let Some(backup) = storage.get::<DataKey, PayrollBackup>(&DataKey::Backup(backup_id)) {
                        if let Some(data) = storage.get::<DataKey, BackupData>(&DataKey::BackupData(backup_id)) {
                            // Merge data from all backups
                            for payroll in data.payroll_data.iter() {
                                payroll_data.push_back(payroll);
                            }
                            for template in data.template_data.iter() {
                                template_data.push_back(template);
                            }
                            for preset in data.preset_data.iter() {
                                preset_data.push_back(preset);
                            }
                            for insurance in data.insurance_data.iter() {
                                insurance_data.push_back(insurance);
                            }
                        }
                    }
                }
            },
            BackupType::Employer => {
                // Collect employer-specific data
                let employer_employees = Self::get_employer_employees(env.clone(), employer.clone());
                for employee in employer_employees.iter() {
                    if let Some(payroll) = storage.get(&DataKey::Payroll(employee)) {
                        payroll_data.push_back(payroll);
                    }
                }
                
                let employer_templates = Self::get_employer_templates(env.clone(), employer.clone());
                for template in employer_templates.iter() {
                    template_data.push_back(template);
                }
            },
            BackupType::Employee => {
                // Collect employee-specific data (simplified)
                let employer_employees = Self::get_employer_employees(env.clone(), employer.clone());
                for employee in employer_employees.iter() {
                    if let Some(payroll) = storage.get(&DataKey::Payroll(employee)) {
                        payroll_data.push_back(payroll);
                    }
                }
            },
            BackupType::Template => {
                // Collect template data
                let employer_templates = Self::get_employer_templates(env.clone(), employer.clone());
                for template in employer_templates.iter() {
                    template_data.push_back(template);
                }
            },
            BackupType::Insurance => {
                // Collect insurance data (simplified)
                let employer_employees = Self::get_employer_employees(env.clone(), employer.clone());
                for employee in employer_employees.iter() {
                    if let Some(policy) = storage.get(&DataKey::InsurancePolicy(employee)) {
                        insurance_data.push_back(policy);
                    }
                }
            },
            BackupType::Compliance => {
                // Compliance data would be collected here
                // For now, we'll use an empty string
            },
        }

        let metadata = BackupMetadata {
            total_employees: payroll_data.len() as u32,
            total_templates: template_data.len() as u32,
            total_presets: preset_data.len() as u32,
            total_insurance_policies: insurance_data.len() as u32,
            backup_timestamp: env.ledger().timestamp(),
            contract_version: String::from_str(env, "1.0.0"),
            data_integrity_hash: String::from_str(env, ""),
        };

        Ok(BackupData {
            backup_id: 0, // Will be set by caller
            payroll_data,
            template_data,
            preset_data,
            insurance_data,
            compliance_data: String::from_str(env, ""),
            metadata,
        })
    }

    /// Calculate backup checksum
    fn _calculate_backup_checksum(env: &Env, backup_data: &BackupData) -> String {
        // Simplified checksum calculation
        let checksum = String::from_str(env, "checksum");
        checksum
    }

    /// Calculate data hash
    fn _calculate_data_hash(env: &Env, backup_data: &BackupData) -> String {
        // Simplified hash calculation
        let hash = String::from_str(env, "hash");
        hash
    }

    /// Calculate backup size
    fn _calculate_backup_size(env: &Env, backup_data: &BackupData) -> u64 {
        // Simplified size calculation
        let payroll_size = backup_data.payroll_data.len() as u64 * 100; // Approximate size per payroll
        let template_size = backup_data.template_data.len() as u64 * 80; // Approximate size per template
        let preset_size = backup_data.preset_data.len() as u64 * 60; // Approximate size per preset
        let insurance_size = backup_data.insurance_data.len() as u64 * 120; // Approximate size per insurance
        let metadata_size = 200; // Approximate metadata size
        
        payroll_size + template_size + preset_size + insurance_size + metadata_size
    }

    /// Restore payroll data
    fn _restore_payroll(env: &Env, payroll: &Payroll) -> Result<(), PayrollError> {
        let storage = env.storage().persistent();
        
        // Check if payroll already exists
        if storage.has(&DataKey::Payroll(payroll.employer.clone())) {
            // Update existing payroll
            storage.set(&DataKey::Payroll(payroll.employer.clone()), payroll);
        } else {
            // Create new payroll
            storage.set(&DataKey::Payroll(payroll.employer.clone()), payroll);
            // Update indexes
            Self::add_to_employer_index(env, &payroll.employer, &payroll.employer);
        }
        
        Ok(())
    }

    /// Restore template data
    fn _restore_template(env: &Env, template: &PayrollTemplate) -> Result<(), PayrollError> {
        let storage = env.storage().persistent();
        
        // Check if template already exists
        if storage.has(&DataKey::Template(template.id)) {
            // Update existing template
            storage.set(&DataKey::Template(template.id), template);
        } else {
            // Create new template
            storage.set(&DataKey::Template(template.id), template);
            
            // Add to employer's templates
            let mut employer_templates: Vec<u64> = storage.get(&DataKey::EmpTemplates(template.employer.clone())).unwrap_or(Vec::new(env));
            employer_templates.push_back(template.id);
            storage.set(&DataKey::EmpTemplates(template.employer.clone()), &employer_templates);
        }
        
        Ok(())
    }

    /// Restore preset data
    fn _restore_preset(env: &Env, preset: &TemplatePreset) -> Result<(), PayrollError> {
        let storage = env.storage().persistent();
        
        // Check if preset already exists
        if storage.has(&DataKey::Preset(preset.id)) {
            // Update existing preset
            storage.set(&DataKey::Preset(preset.id), preset);
        } else {
            // Create new preset
            storage.set(&DataKey::Preset(preset.id), preset);
            
            // Add to category
            let mut category_presets: Vec<u64> = storage.get(&DataKey::PresetCat(preset.category.clone())).unwrap_or(Vec::new(env));
            category_presets.push_back(preset.id);
            storage.set(&DataKey::PresetCat(preset.category.clone()), &category_presets);
            
            // Add to active presets if active
            if preset.is_active {
                let mut active_presets: Vec<u64> = storage.get(&DataKey::ActivePresets).unwrap_or(Vec::new(env));
                active_presets.push_back(preset.id);
                storage.set(&DataKey::ActivePresets, &active_presets);
            }
        }
        
        Ok(())
    }

    //-----------------------------------------------------------------------------
    // Scheduling and Automation Functions
    //-----------------------------------------------------------------------------

    /// Create a new payroll schedule
    pub fn create_schedule(
        env: Env,
        caller: Address,
        name: String,
        description: String,
        schedule_type: ScheduleType,
        frequency: ScheduleFrequency,
        start_date: u64,
        end_date: Option<u64>,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        // Validate schedule data
        if name.len() == 0 || name.len() > 100 {
            return Err(PayrollError::InvalidTemplateName);
        }

        let current_time = env.ledger().timestamp();
        if start_date < current_time {
            return Err(PayrollError::ScheduleValidationFailed);
        }

        if let Some(end) = end_date {
            if end <= start_date {
                return Err(PayrollError::ScheduleValidationFailed);
            }
        }

        let storage = env.storage().persistent();

        // Get next schedule ID
        let next_id = storage.get(&DataKey::NextSchedId).unwrap_or(0) + 1;
        storage.set(&DataKey::NextSchedId, &next_id);

        // Calculate next execution time
        let next_execution = Self::_calculate_next_execution(&env, &frequency, start_date);

        // Create schedule metadata
        let metadata = ScheduleMetadata {
            total_employees: 0,
            total_amount: 0,
            token_address: Address::from_str(&env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"),
            priority: 1,
            retry_count: 0,
            max_retries: 3,
            success_rate: 0,
            average_execution_time: 0,
        };

        let schedule = PayrollSchedule {
            id: next_id,
            name: name.clone(),
            description: description.clone(),
            employer: caller.clone(),
            schedule_type: schedule_type.clone(),
            frequency: frequency.clone(),
            start_date,
            end_date,
            next_execution,
            is_active: true,
            created_at: current_time,
            updated_at: current_time,
            execution_count: 0,
            last_execution: None,
            metadata,
        };

        // Store schedule
        storage.set(&DataKey::Schedule(next_id), &schedule);

        // Add to employer's schedules
        let mut employer_schedules: Vec<u64> = storage.get(&DataKey::EmpSchedules(caller.clone())).unwrap_or(Vec::new(&env));
        employer_schedules.push_back(next_id);
        storage.set(&DataKey::EmpSchedules(caller.clone()), &employer_schedules);

        // Note: Active schedules tracking removed due to storage constraints

        env.events().publish(
            (SCHEDULE_CREATED_EVENT,),
            (caller.clone(), next_id, name, schedule_type),
        );

        Ok(next_id)
    }

    /// Get a schedule by ID
    pub fn get_schedule(env: Env, schedule_id: u64) -> Result<PayrollSchedule, PayrollError> {
        let storage = env.storage().persistent();
        storage.get(&DataKey::Schedule(schedule_id))
            .ok_or(PayrollError::ScheduleNotFound)
    }

    /// Update an existing schedule
    pub fn update_schedule(
        env: Env,
        caller: Address,
        schedule_id: u64,
        name: Option<String>,
        description: Option<String>,
        frequency: Option<ScheduleFrequency>,
        end_date: Option<Option<u64>>,
        is_active: Option<bool>,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let mut schedule: PayrollSchedule = storage.get(&DataKey::Schedule(schedule_id))
            .ok_or(PayrollError::ScheduleNotFound)?;

        // Only schedule owner can update
        if schedule.employer != caller {
            return Err(PayrollError::Unauthorized);
        }

        // Update fields if provided
        if let Some(new_name) = name {
            if new_name.len() == 0 || new_name.len() > 100 {
                return Err(PayrollError::InvalidTemplateName);
            }
            schedule.name = new_name;
        }

        if let Some(new_description) = description {
            schedule.description = new_description;
        }

        if let Some(new_frequency) = frequency {
            schedule.frequency = new_frequency.clone();
            // Recalculate next execution
            schedule.next_execution = Self::_calculate_next_execution(&env, &new_frequency, schedule.start_date);
        }

        if let Some(new_end_date) = end_date {
            if let Some(end) = new_end_date {
                if end <= schedule.start_date {
                    return Err(PayrollError::ScheduleValidationFailed);
                }
            }
            schedule.end_date = new_end_date;
        }

        if let Some(new_active) = is_active {
            schedule.is_active = new_active;
        }

        schedule.updated_at = env.ledger().timestamp();
        storage.set(&DataKey::Schedule(schedule_id), &schedule);

        env.events().publish(
            (SCHEDULE_UPDATED_EVENT,),
            (caller.clone(), schedule_id),
        );

        Ok(())
    }

    /// Execute scheduled payroll
    pub fn execute_schedule(
        env: Env,
        caller: Address,
        schedule_id: u64,
    ) -> Result<bool, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let mut schedule: PayrollSchedule = storage.get(&DataKey::Schedule(schedule_id))
            .ok_or(PayrollError::ScheduleNotFound)?;

        // Check if schedule is active and ready for execution
        if !schedule.is_active {
            return Err(PayrollError::ScheduleExecutionFailed);
        }

        let current_time = env.ledger().timestamp();
        if current_time < schedule.next_execution {
            return Err(PayrollError::ScheduleExecutionFailed);
        }

        // Check if schedule has ended
        if let Some(end_date) = schedule.end_date {
            if current_time > end_date {
                return Err(PayrollError::ScheduleExecutionFailed);
            }
        }

        // Execute the schedule based on type
        let start_time = env.ledger().timestamp();
        let mut success_count = 0;
        let mut failure_count = 0;

        match schedule.schedule_type {
            ScheduleType::Recurring => {
                // Execute recurring payroll for all employees
                let employees = Self::get_employer_employees(env.clone(), schedule.employer.clone());
                for employee in employees.iter() {
                    match Self::disburse_salary(env.clone(), caller.clone(), employee.clone()) {
                        Ok(_) => success_count += 1,
                        Err(_) => failure_count += 1,
                    }
                }
            },
            ScheduleType::OneTime => {
                // Execute one-time payroll
                let employees = Self::get_employer_employees(env.clone(), schedule.employer.clone());
                for employee in employees.iter() {
                    match Self::disburse_salary(env.clone(), caller.clone(), employee.clone()) {
                        Ok(_) => success_count += 1,
                        Err(_) => failure_count += 1,
                    }
                }
                // Deactivate one-time schedule after execution
                schedule.is_active = false;
            },
            ScheduleType::Batch => {
                // Execute batch payroll processing
                let employees = Self::get_employer_employees(env.clone(), schedule.employer.clone());
                for employee in employees.iter() {
                    match Self::disburse_salary(env.clone(), caller.clone(), employee.clone()) {
                        Ok(_) => success_count += 1,
                        Err(_) => failure_count += 1,
                    }
                }
            },
            _ => {
                // Other schedule types would be implemented here
                return Err(PayrollError::ScheduleExecutionFailed);
            }
        }

        let end_time = env.ledger().timestamp();
        let duration = end_time - start_time;

        // Update schedule metadata
        schedule.execution_count += 1;
        schedule.last_execution = Some(current_time);
        schedule.next_execution = Self::_calculate_next_execution(&env, &schedule.frequency, current_time);
        schedule.metadata.total_employees = success_count + failure_count;
        schedule.metadata.success_rate = if (success_count + failure_count) > 0 {
            (success_count * 100) / (success_count + failure_count)
        } else {
            0
        };
        schedule.metadata.average_execution_time = duration;
        schedule.updated_at = current_time;

        storage.set(&DataKey::Schedule(schedule_id), &schedule);

        env.events().publish(
            (SCHEDULE_EXECUTED_EVENT,),
            (caller.clone(), schedule_id, success_count, failure_count, duration),
        );

        Ok(failure_count == 0)
    }

    /// Create an automation rule
    pub fn create_automation_rule(
        env: Env,
        caller: Address,
        name: String,
        description: String,
        rule_type: RuleType,
        conditions: Vec<RuleCondition>,
        actions: Vec<RuleAction>,
        priority: u32,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        // Validate rule data
        if name.len() == 0 || name.len() > 100 {
            return Err(PayrollError::InvalidTemplateName);
        }

        if conditions.len() == 0 || actions.len() == 0 {
            return Err(PayrollError::InvalidAutomationRule);
        }

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get next rule ID
        let next_id = storage.get(&DataKey::NextRuleId).unwrap_or(0) + 1;
        storage.set(&DataKey::NextRuleId, &next_id);

        let rule = AutomationRule {
            id: next_id,
            name: name.clone(),
            description: description.clone(),
            employer: caller.clone(),
            rule_type: rule_type.clone(),
            conditions: conditions.clone(),
            actions: actions.clone(),
            is_active: true,
            created_at: current_time,
            updated_at: current_time,
            execution_count: 0,
            last_execution: None,
            priority,
        };

        // Store rule
        storage.set(&DataKey::Rule(next_id), &rule);

        // Add to employer's rules
        let mut employer_rules: Vec<u64> = storage.get(&DataKey::EmpRules(caller.clone())).unwrap_or(Vec::new(&env));
        employer_rules.push_back(next_id);
        storage.set(&DataKey::EmpRules(caller.clone()), &employer_rules);

        // Note: Active rules tracking removed due to storage constraints

        env.events().publish(
            (RULE_CREATED_EVENT,),
            (caller.clone(), next_id, name, rule_type),
        );

        Ok(next_id)
    }

    /// Get an automation rule by ID
    pub fn get_automation_rule(env: Env, rule_id: u64) -> Result<AutomationRule, PayrollError> {
        let storage = env.storage().persistent();
        storage.get(&DataKey::Rule(rule_id))
            .ok_or(PayrollError::AutomationRuleNotFound)
    }

    /// Execute automation rules
    pub fn execute_automation_rules(
        env: Env,
        caller: Address,
    ) -> Result<u32, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let mut executed_count = 0;

        // Get all rules for the caller and execute active ones
        let rule_ids: Vec<u64> = storage.get(&DataKey::EmpRules(caller.clone())).unwrap_or(Vec::new(&env));
        for rule_id in rule_ids.iter() {
            if let Some(rule) = storage.get::<DataKey, AutomationRule>(&DataKey::Rule(rule_id)) {
                if rule.employer == caller && rule.is_active {
                    match Self::_evaluate_and_execute_rule(&env, &rule) {
                        Ok(_) => executed_count += 1,
                        Err(_) => continue,
                    }
                }
            }
        }

        env.events().publish(
            (RULE_EXECUTED_EVENT,),
            (caller.clone(), executed_count),
        );

        Ok(executed_count)
    }

    /// Get all schedules for an employer
    pub fn get_employer_schedules(env: Env, employer: Address) -> Vec<PayrollSchedule> {
        let storage = env.storage().persistent();
        let schedule_ids: Vec<u64> = storage.get(&DataKey::EmpSchedules(employer.clone())).unwrap_or(Vec::new(&env));
        let mut schedules = Vec::new(&env);

        for id in schedule_ids.iter() {
            if let Some(schedule) = storage.get(&DataKey::Schedule(id)) {
                schedules.push_back(schedule);
            }
        }

        schedules
    }

    /// Get all automation rules for an employer
    pub fn get_employer_rules(env: Env, employer: Address) -> Vec<AutomationRule> {
        let storage = env.storage().persistent();
        let rule_ids: Vec<u64> = storage.get(&DataKey::EmpRules(employer.clone())).unwrap_or(Vec::new(&env));
        let mut rules = Vec::new(&env);

        for id in rule_ids.iter() {
            if let Some(rule) = storage.get(&DataKey::Rule(id)) {
                rules.push_back(rule);
            }
        }

        rules
    }

    /// Get all active schedules
    pub fn get_active_schedules(env: Env) -> Vec<PayrollSchedule> {
        // Note: Active schedules tracking removed due to storage constraints
        // This function now returns an empty vector
        Vec::new(&env)
    }

    /// Get all active rules
    pub fn get_active_rules(env: Env) -> Vec<AutomationRule> {
        // Note: Active rules tracking removed due to storage constraints
        // This function now returns an empty vector
        Vec::new(&env)
    }

    //-----------------------------------------------------------------------------
    // Internal Helper Functions for Scheduling and Automation
    //-----------------------------------------------------------------------------

    /// Calculate next execution time based on frequency
    fn _calculate_next_execution(env: &Env, frequency: &ScheduleFrequency, current_time: u64) -> u64 {
        match frequency {
            ScheduleFrequency::Daily => current_time + 86400, // 24 hours
            ScheduleFrequency::Weekly => current_time + 604800, // 7 days
            ScheduleFrequency::BiWeekly => current_time + 1209600, // 14 days
            ScheduleFrequency::Monthly => current_time + 2592000, // 30 days
            ScheduleFrequency::Quarterly => current_time + 7776000, // 90 days
            ScheduleFrequency::Yearly => current_time + 31536000, // 365 days
            ScheduleFrequency::Custom(seconds) => current_time + seconds,
        }
    }

    /// Evaluate and execute an automation rule
    fn _evaluate_and_execute_rule(env: &Env, rule: &AutomationRule) -> Result<(), PayrollError> {
        // Evaluate conditions
        let conditions_met = Self::_evaluate_conditions(env, &rule.conditions)?;
        
        if conditions_met {
            // Execute actions
            for action in rule.actions.iter() {
                Self::_execute_action(env, &action)?;
            }
        }

        Ok(())
    }

    /// Evaluate rule conditions
    fn _evaluate_conditions(env: &Env, conditions: &Vec<RuleCondition>) -> Result<bool, PayrollError> {
        // Simplified condition evaluation
        // In a real implementation, this would evaluate actual conditions
        Ok(true) // For now, always return true
    }

    /// Execute a rule action
    fn _execute_action(env: &Env, action: &RuleAction) -> Result<(), PayrollError> {
        match action.action_type {
            ActionType::DisburseSalary => {
                // Execute salary disbursement
                // This would be implemented based on action parameters
                Ok(())
            },
            ActionType::PausePayroll => {
                // Pause payroll operations
                Ok(())
            },
            ActionType::ResumePayroll => {
                // Resume payroll operations
                Ok(())
            },
            ActionType::CreateBackup => {
                // Create backup
                Ok(())
            },
            ActionType::SendNotification => {
                // Send notification
                Ok(())
            },
            ActionType::UpdateSchedule => {
                // Update schedule
                Ok(())
            },
            ActionType::ExecuteRecovery => {
                // Execute recovery
                Ok(())
            },
            ActionType::Custom => {
                // Custom action
                Ok(())
            },
        }
    }

    //-----------------------------------------------------------------------------
    // Security & Access Control Functions
    //-----------------------------------------------------------------------------

    /// Create a new role
    pub fn create_role(
        env: Env,
        caller: Address,
        role_id: String,
        name: String,
        description: String,
        permissions: Vec<Permission>,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;
        Self::_require_security_permission(&env, &caller, Permission::ManageRoles)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Check if role already exists
        if storage.has(&DataKey::Role(role_id.clone())) {
            return Err(PayrollError::RoleNotFound);
        }

        let role = Role {
            id: role_id.clone(),
            name: name.clone(),
            description: description.clone(),
            permissions,
            is_active: true,
            created_at: current_time,
            updated_at: current_time,
        };

        storage.set(&DataKey::Role(role_id.clone()), &role);

        env.events().publish(
            (ROLE_ASSIGNED_EVENT,),
            (caller, role_id, name),
        );

        Ok(())
    }

    /// Assign a role to a user
    pub fn assign_role(
        env: Env,
        caller: Address,
        user: Address,
        role_id: String,
        expires_at: Option<u64>,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;
        Self::_require_security_permission(&env, &caller, Permission::ManageRoles)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Verify role exists
        let role: Role = storage.get(&DataKey::Role(role_id.clone()))
            .ok_or(PayrollError::RoleNotFound)?;

        if !role.is_active {
            return Err(PayrollError::RoleNotFound);
        }

        let assignment = UserRoleAssignment {
            user: user.clone(),
            role: role_id.clone(),
            assigned_by: caller.clone(),
            assigned_at: current_time,
            expires_at,
            is_active: true,
        };

        storage.set(&DataKey::UserRole(user.clone()), &assignment);

        env.events().publish(
            (ROLE_ASSIGNED_EVENT,),
            (caller, user, role_id),
        );

        Ok(())
    }

    /// Revoke a role from a user
    pub fn revoke_role(
        env: Env,
        caller: Address,
        user: Address,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;
        Self::_require_security_permission(&env, &caller, Permission::ManageRoles)?;

        let storage = env.storage().persistent();

        // Check if user has a role assignment
        if let Some(mut assignment) = storage.get::<DataKey, UserRoleAssignment>(&DataKey::UserRole(user.clone())) {
            assignment.is_active = false;
            storage.set(&DataKey::UserRole(user.clone()), &assignment);

            env.events().publish(
                (ROLE_REVOKED_EVENT,),
                (caller, user),
            );
        }

        Ok(())
    }

    /// Get user's role assignment
    pub fn get_user_role(env: Env, user: Address) -> Option<UserRoleAssignment> {
        env.storage().persistent().get(&DataKey::UserRole(user))
    }

    /// Get role details
    pub fn get_role(env: Env, role_id: String) -> Option<Role> {
        env.storage().persistent().get(&DataKey::Role(role_id))
    }

    /// Check if user has a specific permission
    pub fn has_permission(
        env: Env,
        user: Address,
        permission: Permission,
    ) -> bool {
        let storage = env.storage().persistent();

        // Check if user has a role assignment
        if let Some(assignment) = storage.get::<DataKey, UserRoleAssignment>(&DataKey::UserRole(user.clone())) {
            if !assignment.is_active {
                return false;
            }

            // Check if role assignment has expired
            if let Some(expires_at) = assignment.expires_at {
                if env.ledger().timestamp() > expires_at {
                    return false;
                }
            }

            // Get role and check permissions
            if let Some(role) = storage.get::<DataKey, Role>(&DataKey::Role(assignment.role)) {
                if role.is_active && role.permissions.contains(&permission) {
                    return true;
                }
            }
        }

        // Check if user is owner (owner has all permissions)
        if let Some(owner) = storage.get::<DataKey, Address>(&DataKey::Owner) {
            if user == owner {
                return true;
            }
        }

        false
    }

    /// Update security settings
    pub fn update_security_settings(
        env: Env,
        caller: Address,
        mfa_required: Option<bool>,
        session_timeout: Option<u64>,
        max_login_attempts: Option<u32>,
        lockout_duration: Option<u64>,
        audit_logging_enabled: Option<bool>,
        rate_limiting_enabled: Option<bool>,
        security_policies_enabled: Option<bool>,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;
        Self::_require_security_permission(&env, &caller, Permission::ManageSecurity)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        let mut settings = storage.get::<DataKey, SecuritySettings>(&DataKey::SecuritySettings)
            .unwrap_or(SecuritySettings {
                mfa_required: false,
                session_timeout: 3600, // 1 hour default
                max_login_attempts: 5,
                lockout_duration: 1800, // 30 minutes default
                ip_whitelist: Vec::new(&env),
                ip_blacklist: Vec::new(&env),
                audit_logging_enabled: true,
                rate_limiting_enabled: true,
                security_policies_enabled: true,
                emergency_mode: false,
                last_updated: current_time,
            });

        // Update settings with provided values
        if let Some(mfa) = mfa_required {
            settings.mfa_required = mfa;
        }
        if let Some(timeout) = session_timeout {
            settings.session_timeout = timeout;
        }
        if let Some(attempts) = max_login_attempts {
            settings.max_login_attempts = attempts;
        }
        if let Some(duration) = lockout_duration {
            settings.lockout_duration = duration;
        }
        if let Some(audit) = audit_logging_enabled {
            settings.audit_logging_enabled = audit;
        }
        if let Some(rate) = rate_limiting_enabled {
            settings.rate_limiting_enabled = rate;
        }
        if let Some(policies) = security_policies_enabled {
            settings.security_policies_enabled = policies;
        }

        settings.last_updated = current_time;
        storage.set(&DataKey::SecuritySettings, &settings);

        Ok(())
    }

    /// Get security settings
    pub fn get_security_settings(env: Env) -> Option<SecuritySettings> {
        env.storage().persistent().get(&DataKey::SecuritySettings)
    }

    /// Perform security audit
    pub fn perform_security_audit(
        env: Env,
        caller: Address,
    ) -> Result<Vec<SecurityAuditEntry>, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;
        Self::_require_security_permission(&env, &caller, Permission::ViewAuditTrail)?;

        // This would perform a comprehensive security audit
        // For now, return an empty vector
        let audit_entries = Vec::new(&env);

        env.events().publish(
            (SECURITY_AUDIT_EVENT,),
            (caller, audit_entries.len() as u32),
        );

        Ok(audit_entries)
    }

    /// Emergency security lockdown
    pub fn emergency_lockdown(
        env: Env,
        caller: Address,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;
        Self::_require_security_permission(&env, &caller, Permission::EmergencyOperations)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Pause the contract
        storage.set(&DataKey::Paused, &true);

        // Update security settings to emergency mode
        if let Some(mut settings) = storage.get::<DataKey, SecuritySettings>(&DataKey::SecuritySettings) {
            settings.emergency_mode = true;
            settings.last_updated = current_time;
            storage.set(&DataKey::SecuritySettings, &settings);
        }

        env.events().publish(
            (SECURITY_POLICY_VIOLATION_EVENT,),
            (caller, String::from_str(&env, "Emergency lockdown activated")),
        );

        Ok(())
    }

    //-----------------------------------------------------------------------------
    // Internal Security Helper Functions
    //-----------------------------------------------------------------------------

    /// Require security permission for operation
    fn _require_security_permission(
        env: &Env,
        caller: &Address,
        permission: Permission,
    ) -> Result<(), PayrollError> {
        if !Self::has_permission(env.clone(), caller.clone(), permission) {
            return Err(PayrollError::InsufficientPermissions);
        }
        Ok(())
    }

    /// Log security event
    fn _log_security_event(
        env: &Env,
        user: &Address,
        action: &str,
        resource: &str,
        result: SecurityAuditResult,
        details: Map<String, String>,
    ) {
        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        let entry_id = String::from_str(env, "sec_audit_entry");

        let audit_entry = SecurityAuditEntry {
            entry_id: entry_id.clone(),
            user: user.clone(),
            action: String::from_str(env, action),
            resource: String::from_str(env, resource),
            result,
            details,
            timestamp: current_time,
            ip_address: None,
            user_agent: None,
            session_id: None,
        };

        // Store audit entry (simplified - in real implementation would use proper indexing)
        // Note: In a real implementation, this would use proper indexing
        // For now, we'll just log the event
    }

    /// Check rate limiting
    fn _check_rate_limit(
        env: &Env,
        user: &Address,
        operation: &str,
    ) -> Result<(), PayrollError> {
        // Simplified rate limiting check
        // In a real implementation, this would check actual rate limits
        Ok(())
    }

    /// Detect suspicious activity
    fn _detect_suspicious_activity(
        env: &Env,
        user: &Address,
        action: &str,
    ) -> Result<(), PayrollError> {
        // Simplified suspicious activity detection
        // In a real implementation, this would use ML/AI to detect patterns
        Ok(())
    }

    //-----------------------------------------------------------------------------
    // Enterprise Features Implementation
    //-----------------------------------------------------------------------------

    /// Create a new department
    pub fn create_department(
        env: Env,
        caller: Address,
        name: String,
        description: String,
        manager: Address,
        parent_department: Option<u64>,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get next department ID
        let department_id = storage.get::<EnterpriseDataKey, u64>(&EnterpriseDataKey::NextDepartmentId).unwrap_or(1);
        storage.set(&EnterpriseDataKey::NextDepartmentId, &(department_id + 1));

        let department = enterprise::Department {
            id: department_id,
            name: name.clone(),
            description: description.clone(),
            employer: caller.clone(),
            manager,
            parent_department,
            created_at: current_time,
            updated_at: current_time,
            is_active: true,
        };

        // Store department
        storage.set(&enterprise::EnterpriseDataKey::Department(department_id), &department);

        // Add to employer's departments
        let mut employer_departments = storage
            .get::<enterprise::EnterpriseDataKey, Vec<u64>>(&enterprise::EnterpriseDataKey::EmployerDepartments(caller.clone()))
            .unwrap_or(Vec::new(&env));
        employer_departments.push_back(department_id);
        storage.set(&enterprise::EnterpriseDataKey::EmployerDepartments(caller.clone()), &employer_departments);

        // Emit event
        env.events().publish(
            (symbol_short!("dept_c"),),
            (caller, department_id, name, description),
        );

        Ok(department_id)
    }

    /// Assign employee to department
    pub fn assign_employee_to_department(
        env: Env,
        caller: Address,
        employee: Address,
        department_id: u64,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();

        // Verify department exists and belongs to caller
        let department = storage
            .get::<EnterpriseDataKey, Department>(&EnterpriseDataKey::Department(department_id))
            .ok_or(PayrollError::InvalidData)?;

        if department.employer != caller {
            return Err(PayrollError::Unauthorized);
        }

        // Assign employee to department
        storage.set(&EnterpriseDataKey::EmployeeDepartment(employee.clone()), &department_id);

        // Emit event
        env.events().publish(
            (symbol_short!("emp_a"),),
            (caller, employee, department_id),
        );

        Ok(())
    }

    /// Create approval workflow
    pub fn create_approval_workflow(
        env: Env,
        caller: Address,
        name: String,
        description: String,
        steps: Vec<ApprovalStep>,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get next workflow ID
        let workflow_id = storage.get::<EnterpriseDataKey, u64>(&EnterpriseDataKey::NextWorkflowId).unwrap_or(1);
        storage.set(&EnterpriseDataKey::NextWorkflowId, &(workflow_id + 1));

        let workflow = ApprovalWorkflow {
            id: workflow_id,
            name: name.clone(),
            description: description.clone(),
            employer: caller.clone(),
            steps,
            created_at: current_time,
            updated_at: current_time,
            is_active: true,
        };

        // Store workflow
        storage.set(&EnterpriseDataKey::ApprovalWorkflow(workflow_id), &workflow);

        // Emit event
        env.events().publish(
            (symbol_short!("wf_c"),),
            (caller, workflow_id, name, description),
        );

        Ok(workflow_id)
    }

    /// Create webhook endpoint
    pub fn create_webhook_endpoint(
        env: Env,
        caller: Address,
        name: String,
        url: String,
        events: Vec<String>,
        headers: Map<String, String>,
    ) -> Result<String, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Generate webhook ID
        let webhook_id_str = String::from_str(&env, "hook_");

        let webhook = WebhookEndpoint {
            id: webhook_id_str.clone(),
            name: name.clone(),
            url: url.clone(),
            employer: caller.clone(),
            events,
            headers,
            is_active: true,
            created_at: current_time,
            last_triggered: None,
        };

        // Store webhook
        storage.set(&EnterpriseDataKey::WebhookEndpoint(webhook_id_str.clone()), &webhook);

        // Add to employer's webhooks
        let mut employer_webhooks = storage
            .get::<EnterpriseDataKey, Vec<String>>(&EnterpriseDataKey::EmployerWebhooks(caller.clone()))
            .unwrap_or(Vec::new(&env));
        employer_webhooks.push_back(webhook_id_str.clone());
        storage.set(&EnterpriseDataKey::EmployerWebhooks(caller.clone()), &employer_webhooks);

        // Emit event
        env.events().publish(
            (symbol_short!("hook_c"),),
            (caller, webhook_id_str.clone(), name, url),
        );

        Ok(webhook_id_str)
    }

    /// Create report template
    pub fn create_report_template(
        env: Env,
        caller: Address,
        name: String,
        description: String,
        query_parameters: Map<String, String>,
        schedule: Option<String>,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get next report ID
        let report_id = storage.get::<EnterpriseDataKey, u64>(&EnterpriseDataKey::NextReportId).unwrap_or(1);
        storage.set(&EnterpriseDataKey::NextReportId, &(report_id + 1));

        let report = ReportTemplate {
            id: report_id,
            name: name.clone(),
            description: description.clone(),
            employer: caller.clone(),
            query_parameters,
            schedule,
            created_at: current_time,
            updated_at: current_time,
            is_active: true,
        };

        // Store report template
        storage.set(&EnterpriseDataKey::ReportTemplate(report_id), &report);

        // Add to employer's reports
        let mut employer_reports = storage
            .get::<EnterpriseDataKey, Vec<u64>>(&EnterpriseDataKey::EmployerReports(caller.clone()))
            .unwrap_or(Vec::new(&env));
        employer_reports.push_back(report_id);
        storage.set(&EnterpriseDataKey::EmployerReports(caller.clone()), &employer_reports);

        // Emit event
        env.events().publish(
            (symbol_short!("rpt_c"),),
            (caller, report_id, name, description),
        );

        Ok(report_id)
    }

    /// Create backup schedule
    pub fn create_backup_schedule(
        env: Env,
        caller: Address,
        name: String,
        frequency: String,
        retention_days: u32,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get next backup schedule ID
        let schedule_id = storage.get::<EnterpriseDataKey, u64>(&EnterpriseDataKey::NextBackupScheduleId).unwrap_or(1);
        storage.set(&EnterpriseDataKey::NextBackupScheduleId, &(schedule_id + 1));

        let schedule = BackupSchedule {
            id: schedule_id,
            name: name.clone(),
            employer: caller.clone(),
            frequency,
            retention_days,
            is_active: true,
            created_at: current_time,
            last_backup: None,
        };

        // Store backup schedule
        storage.set(&EnterpriseDataKey::BackupSchedule(schedule_id), &schedule);

        // Add to employer's backup schedules
        let mut employer_schedules = storage
            .get::<EnterpriseDataKey, Vec<u64>>(&EnterpriseDataKey::EmployerBackupSchedules(caller.clone()))
            .unwrap_or(Vec::new(&env));
        employer_schedules.push_back(schedule_id);
        storage.set(&EnterpriseDataKey::EmployerBackupSchedules(caller.clone()), &employer_schedules);

        // Emit event
        env.events().publish(
            (symbol_short!("bkup_c"),),
            (caller, schedule_id, name),
        );

        Ok(schedule_id)
    }

    //-----------------------------------------------------------------------------
    // Dispute Resolution Functions
    //-----------------------------------------------------------------------------

    /// Create a new dispute
    pub fn create_dispute(
        env: Env,
        caller: Address,
        employer: Address,
        dispute_type: DisputeType,
        description: String,
        evidence: Vec<String>,
        amount_involved: Option<i128>,
        token_involved: Option<Address>,
        priority: DisputePriority,
        timeout_days: u32,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Verify the caller is an employee with a payroll
        let payroll = Self::_get_payroll(&env, &caller).ok_or(PayrollError::PayrollNotFound)?;
        if payroll.employer != employer {
            return Err(PayrollError::Unauthorized);
        }

        // Get dispute settings
        let settings = Self::_get_dispute_settings(&env);

        // Validate evidence requirements
        if settings.evidence_required && (evidence.len() as u32) < settings.min_evidence_count {
            return Err(PayrollError::InvalidData);
        }

        // Get next dispute ID
        let next_id = storage.get(&EnterpriseDataKey::NextDisputeId).unwrap_or(0) + 1;
        storage.set(&EnterpriseDataKey::NextDisputeId, &next_id);

        // Calculate expiration time
        let timeout_days = if timeout_days == 0 { settings.dispute_timeout } else { timeout_days };
        let expires_at = current_time + (timeout_days as u64 * 24 * 60 * 60);

        // Create dispute
        let dispute = Dispute {
            id: next_id,
            employee: caller.clone(),
            employer: employer.clone(),
            dispute_type: dispute_type.clone(),
            description: description.clone(),
            evidence,
            amount_involved,
            token_involved,
            priority: priority.clone(),
            status: DisputeStatus::Open,
            created_at: current_time,
            updated_at: current_time,
            expires_at,
            resolved_at: None,
            resolution: None,
            resolution_by: None,
        };

        // Store dispute
        storage.set(&EnterpriseDataKey::Dispute(next_id), &dispute);

        // Add to employee's disputes
        let mut employee_disputes: Vec<u64> = storage.get(&EnterpriseDataKey::EmployeeDisputes(caller.clone())).unwrap_or(Vec::new(&env));
        employee_disputes.push_back(next_id);
        storage.set(&EnterpriseDataKey::EmployeeDisputes(caller.clone()), &employee_disputes);

        // Add to employer's disputes
        let mut employer_disputes: Vec<u64> = storage.get(&EnterpriseDataKey::EmployerDisputes(employer.clone())).unwrap_or(Vec::new(&env));
        employer_disputes.push_back(next_id);
        storage.set(&EnterpriseDataKey::EmployerDisputes(employer.clone()), &employer_disputes);

        // Add to open disputes
        let mut open_disputes: Vec<u64> = storage.get(&EnterpriseDataKey::OpenDisputes).unwrap_or(Vec::new(&env));
        open_disputes.push_back(next_id);
        storage.set(&EnterpriseDataKey::OpenDisputes, &open_disputes);

        // Emit dispute created event
        env.events().publish(
            (symbol_short!("dispute_c"),),
            (caller, next_id, employer, dispute_type, priority),
        );

        Ok(next_id)
    }

    /// Update dispute status
    pub fn update_dispute_status(
        env: Env,
        caller: Address,
        dispute_id: u64,
        new_status: DisputeStatus,
        resolution: Option<String>,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get dispute
        let mut dispute: Dispute = storage.get(&EnterpriseDataKey::Dispute(dispute_id))
            .ok_or(PayrollError::PayrollNotFound)?;

        // Check if caller is authorized (employee, employer, or mediator)
        if caller != dispute.employee && caller != dispute.employer {
            // Check if caller is a mediator
            if let Some(mediator) = storage.get::<EnterpriseDataKey, Mediator>(&EnterpriseDataKey::Mediator(caller.clone())) {
                if !mediator.is_active {
                    return Err(PayrollError::Unauthorized);
                }
            } else {
                return Err(PayrollError::Unauthorized);
            }
        }

        // Update dispute
        dispute.status = new_status.clone();
        dispute.updated_at = current_time;
        dispute.resolution = resolution.clone();
        dispute.resolution_by = Some(caller.clone());

        if new_status == DisputeStatus::Resolved {
            dispute.resolved_at = Some(current_time);
        }

        storage.set(&EnterpriseDataKey::Dispute(dispute_id), &dispute);

        // Update open disputes list if resolved
        if new_status == DisputeStatus::Resolved || new_status == DisputeStatus::Closed {
            let mut open_disputes: Vec<u64> = storage.get(&EnterpriseDataKey::OpenDisputes).unwrap_or(Vec::new(&env));
            let mut new_open_disputes = Vec::new(&env);
            for id in open_disputes.iter() {
                if id != dispute_id {
                    new_open_disputes.push_back(id);
                }
            }
            storage.set(&EnterpriseDataKey::OpenDisputes, &new_open_disputes);
        }

        // Emit dispute updated event
        env.events().publish(
            (symbol_short!("dispute_u"),),
            (caller, dispute_id, new_status, resolution),
        );

        Ok(())
    }

    /// Escalate a dispute
    pub fn escalate_dispute(
        env: Env,
        caller: Address,
        dispute_id: u64,
        level: EscalationLevel,
        reason: String,
        mediator_address: Address,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get dispute
        let mut dispute: Dispute = storage.get(&EnterpriseDataKey::Dispute(dispute_id))
            .ok_or(PayrollError::PayrollNotFound)?;

        // Check if dispute is eligible for escalation
        if dispute.status != DisputeStatus::Open && dispute.status != DisputeStatus::UnderReview {
            return Err(PayrollError::InvalidData);
        }

        // Verify mediator exists and is active
        let mediator: Mediator = storage.get(&EnterpriseDataKey::Mediator(mediator_address.clone()))
            .ok_or(PayrollError::PayrollNotFound)?;
        if !mediator.is_active {
            return Err(PayrollError::InvalidData);
        }

        // Get next escalation ID
        let next_id = storage.get(&EnterpriseDataKey::NextEscalationId).unwrap_or(0) + 1;
        storage.set(&EnterpriseDataKey::NextEscalationId, &next_id);

        // Calculate timeout based on level
        let settings = Self::_get_dispute_settings(&env);
        let timeout_days = match level {
            EscalationLevel::Level1 => 7,
            EscalationLevel::Level2 => 14,
            EscalationLevel::Level3 => 21,
            EscalationLevel::Level4 => settings.mediation_timeout,
            EscalationLevel::Level5 => settings.arbitration_timeout,
        };
        let timeout_at = current_time + (timeout_days as u64 * 24 * 60 * 60);

        // Create escalation
        let escalation = Escalation {
            id: next_id,
            dispute_id,
            level: level.clone(),
            reason: reason.clone(),
            escalated_by: caller.clone(),
            escalated_at: current_time,
            mediator: Some(mediator_address.clone()),
            mediator_assigned_at: Some(current_time),
            resolution: None,
            resolved_at: None,
            resolved_by: None,
            timeout_at,
        };

        // Store escalation
        storage.set(&EnterpriseDataKey::Escalation(next_id), &escalation);

        // Add to dispute escalations
        let mut dispute_escalations: Vec<u64> = storage.get(&EnterpriseDataKey::DisputeEscalations(dispute_id)).unwrap_or(Vec::new(&env));
        dispute_escalations.push_back(next_id);
        storage.set(&EnterpriseDataKey::DisputeEscalations(dispute_id), &dispute_escalations);

        // Add to mediator escalations
        let mut mediator_escalations: Vec<u64> = storage.get(&EnterpriseDataKey::MediatorEscalations(mediator_address.clone())).unwrap_or(Vec::new(&env));
        mediator_escalations.push_back(next_id);
        storage.set(&EnterpriseDataKey::MediatorEscalations(mediator_address.clone()), &mediator_escalations);

        // Add to escalated disputes
        let mut escalated_disputes: Vec<u64> = storage.get(&EnterpriseDataKey::EscalatedDisputes).unwrap_or(Vec::new(&env));
        escalated_disputes.push_back(dispute_id);
        storage.set(&EnterpriseDataKey::EscalatedDisputes, &escalated_disputes);

        // Update dispute status
        dispute.status = DisputeStatus::Escalated;
        dispute.updated_at = current_time;
        storage.set(&EnterpriseDataKey::Dispute(dispute_id), &dispute);

        // Emit escalation event
        env.events().publish(
            (symbol_short!("escalate"),),
            (caller, dispute_id, next_id, level, mediator_address),
        );

        Ok(next_id)
    }

    /// Resolve an escalation
    pub fn resolve_escalation(
        env: Env,
        mediator: Address,
        escalation_id: u64,
        resolution: String,
    ) -> Result<(), PayrollError> {
        mediator.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get escalation
        let mut escalation: Escalation = storage.get(&EnterpriseDataKey::Escalation(escalation_id))
            .ok_or(PayrollError::PayrollNotFound)?;

        // Check if mediator is authorized
        if escalation.mediator != Some(mediator.clone()) {
            return Err(PayrollError::Unauthorized);
        }

        // Check if escalation has expired
        if current_time > escalation.timeout_at {
            return Err(PayrollError::InvalidData);
        }

        // Update escalation
        escalation.resolution = Some(resolution.clone());
        escalation.resolved_at = Some(current_time);
        escalation.resolved_by = Some(mediator.clone());
        storage.set(&EnterpriseDataKey::Escalation(escalation_id), &escalation);

        // Update dispute status
        let mut dispute: Dispute = storage.get(&EnterpriseDataKey::Dispute(escalation.dispute_id))
            .ok_or(PayrollError::PayrollNotFound)?;
        dispute.status = DisputeStatus::Resolved;
        dispute.resolution = Some(resolution.clone());
        dispute.resolution_by = Some(mediator.clone());
        dispute.resolved_at = Some(current_time);
        dispute.updated_at = current_time;
        storage.set(&EnterpriseDataKey::Dispute(escalation.dispute_id), &dispute);

        // Remove from escalated disputes
        let mut escalated_disputes: Vec<u64> = storage.get(&EnterpriseDataKey::EscalatedDisputes).unwrap_or(Vec::new(&env));
        let mut new_escalated_disputes = Vec::new(&env);
        for id in escalated_disputes.iter() {
            if id != escalation.dispute_id {
                new_escalated_disputes.push_back(id);
            }
        }
        storage.set(&EnterpriseDataKey::EscalatedDisputes, &new_escalated_disputes);

        // Remove from open disputes
        let mut open_disputes: Vec<u64> = storage.get(&EnterpriseDataKey::OpenDisputes).unwrap_or(Vec::new(&env));
        let mut new_open_disputes = Vec::new(&env);
        for id in open_disputes.iter() {
            if id != escalation.dispute_id {
                new_open_disputes.push_back(id);
            }
        }
        storage.set(&EnterpriseDataKey::OpenDisputes, &new_open_disputes);

        // Emit resolution event
        env.events().publish(
            (symbol_short!("resolve"),),
            (mediator, escalation_id, escalation.dispute_id, resolution),
        );

        Ok(())
    }

    /// Get dispute settings (internal helper)
    fn _get_dispute_settings(env: &Env) -> DisputeSettings {
        let storage = env.storage().persistent();
        storage.get(&EnterpriseDataKey::DisputeSettings).unwrap_or(DisputeSettings {
            auto_escalation_days: 7,
            mediation_timeout: 30,
            arbitration_timeout: 60,
            max_escalation_levels: 5,
            evidence_required: true,
            min_evidence_count: 1,
            dispute_timeout: 30,
            escalation_cooldown: 24,
        })
    }

    /// Add a new mediator
    pub fn add_mediator(
        env: Env,
        caller: Address,
        mediator_address: Address,
        name: String,
        specialization: Vec<String>,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        // Only contract owner can add mediators
        let storage = env.storage().persistent();
        let owner = storage.get(&DataKey::Owner).ok_or(PayrollError::Unauthorized)?;
        if caller != owner {
            return Err(PayrollError::Unauthorized);
        }

        let current_time = env.ledger().timestamp();

        // Create mediator
        let mediator = Mediator {
            address: mediator_address.clone(),
            name: name.clone(),
            specialization: specialization.clone(),
            success_rate: 0,
            total_cases: 0,
            resolved_cases: 0,
            is_active: true,
            created_at: current_time,
            last_active: current_time,
        };

        // Store mediator
        storage.set(&EnterpriseDataKey::Mediator(mediator_address.clone()), &mediator);

        // Add to active mediators
        let mut active_mediators: Vec<Address> = storage.get(&EnterpriseDataKey::ActiveMediators).unwrap_or(Vec::new(&env));
        active_mediators.push_back(mediator_address.clone());
        storage.set(&EnterpriseDataKey::ActiveMediators, &active_mediators);

        // Add to specialization index
        for spec in specialization.iter() {
            let mut mediators_by_spec: Vec<Address> = storage.get(&EnterpriseDataKey::MediatorBySpecialization(spec.clone())).unwrap_or(Vec::new(&env));
            mediators_by_spec.push_back(mediator_address.clone());
            storage.set(&EnterpriseDataKey::MediatorBySpecialization(spec.clone()), &mediators_by_spec);
        }

        // Emit mediator added event
        env.events().publish(
            (symbol_short!("mediator"),),
            (caller, mediator_address, name, specialization),
        );

        Ok(())
    }

    /// Get dispute settings
    pub fn get_dispute_settings(env: Env) -> DisputeSettings {
        Self::_get_dispute_settings(&env)
    }

    /// Update dispute settings
    pub fn update_dispute_settings(
        env: Env,
        caller: Address,
        settings: DisputeSettings,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        // Only contract owner can update settings
        let storage = env.storage().persistent();
        let owner = storage.get(&DataKey::Owner).ok_or(PayrollError::Unauthorized)?;
        if caller != owner {
            return Err(PayrollError::Unauthorized);
        }

        storage.set(&EnterpriseDataKey::DisputeSettings, &settings);

        // Emit settings updated event
        env.events().publish(
            (symbol_short!("settings"),),
            (caller, settings),
        );

        Ok(())
    }

    /// Get a dispute by ID
    pub fn get_dispute(env: Env, dispute_id: u64) -> Result<Dispute, PayrollError> {
        let storage = env.storage().persistent();
        storage.get(&EnterpriseDataKey::Dispute(dispute_id))
            .ok_or(PayrollError::PayrollNotFound)
    }

    /// Get all disputes for an employee
    pub fn get_employee_disputes(env: Env, employee: Address) -> Result<Vec<Dispute>, PayrollError> {
        let storage = env.storage().persistent();
        let dispute_ids: Vec<u64> = storage.get(&EnterpriseDataKey::EmployeeDisputes(employee.clone())).unwrap_or(Vec::new(&env));
        
        let mut disputes = Vec::new(&env);
        for id in dispute_ids.iter() {
            if let Some(dispute) = storage.get(&EnterpriseDataKey::Dispute(id)) {
                disputes.push_back(dispute);
            }
        }
        
        Ok(disputes)
    }

    /// Get all disputes for an employer
    pub fn get_employer_disputes(env: Env, employer: Address) -> Result<Vec<Dispute>, PayrollError> {
        let storage = env.storage().persistent();
        let dispute_ids: Vec<u64> = storage.get(&EnterpriseDataKey::EmployerDisputes(employer.clone())).unwrap_or(Vec::new(&env));
        
        let mut disputes = Vec::new(&env);
        for id in dispute_ids.iter() {
            if let Some(dispute) = storage.get(&EnterpriseDataKey::Dispute(id)) {
                disputes.push_back(dispute);
            }
        }
        
        Ok(disputes)
    }

    /// Get all open disputes
    pub fn get_open_disputes(env: Env) -> Result<Vec<Dispute>, PayrollError> {
        let storage = env.storage().persistent();
        let dispute_ids: Vec<u64> = storage.get(&EnterpriseDataKey::OpenDisputes).unwrap_or(Vec::new(&env));
        
        let mut disputes = Vec::new(&env);
        for id in dispute_ids.iter() {
            if let Some(dispute) = storage.get(&EnterpriseDataKey::Dispute(id)) {
                disputes.push_back(dispute);
            }
        }
        
        Ok(disputes)
    }

    /// Get all escalated disputes
    pub fn get_escalated_disputes(env: Env) -> Result<Vec<Dispute>, PayrollError> {
        let storage = env.storage().persistent();
        let dispute_ids: Vec<u64> = storage.get(&EnterpriseDataKey::EscalatedDisputes).unwrap_or(Vec::new(&env));
        
        let mut disputes = Vec::new(&env);
        for id in dispute_ids.iter() {
            if let Some(dispute) = storage.get(&EnterpriseDataKey::Dispute(id)) {
                disputes.push_back(dispute);
            }
        }
        
        Ok(disputes)
    }

    //-----------------------------------------------------------------------------
    // Payroll Modification Approval System
    //-----------------------------------------------------------------------------

    /// Request a payroll modification that requires approval from both employer and employee
    pub fn request_payroll_modification(
        env: Env,
        requester: Address,
        employee: Address,
        modification_type: PayrollModificationType,
        current_value: String,
        proposed_value: String,
        reason: String,
        approval_timeout_days: u32,
    ) -> Result<u64, PayrollError> {
        requester.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Verify the payroll exists
        let payroll = Self::_get_payroll(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;

        // Only the employer or employee can request modifications
        if requester != payroll.employer && requester != employee {
            return Err(PayrollError::Unauthorized);
        }

        // Get next modification request ID
        let next_id = storage.get(&EnterpriseDataKey::NextModificationRequestId).unwrap_or(0) + 1;
        storage.set(&EnterpriseDataKey::NextModificationRequestId, &next_id);

        // Calculate expiration time (default 30 days if not specified)
        let timeout_days = if approval_timeout_days == 0 { 30 } else { approval_timeout_days };
        let expires_at = current_time + (timeout_days as u64 * 24 * 60 * 60); // Convert days to seconds

        // Create modification request
        let modification_request = PayrollModificationRequest {
            id: next_id,
            employee: employee.clone(),
            employer: payroll.employer.clone(),
            request_type: modification_type.clone(),
            current_value: current_value.clone(),
            proposed_value: proposed_value.clone(),
            reason: reason.clone(),
            requester: requester.clone(),
            employer_approval: Approval::default(&env),
            employee_approval: Approval::default(&env),
            created_at: current_time,
            expires_at,
            status: PayrollModificationStatus::Pending,
        };

        // Store the modification request
        storage.set(&EnterpriseDataKey::PayrollModificationRequest(next_id), &modification_request);

        // Add to employee's modification requests
        let mut employee_requests: Vec<u64> = storage.get(&EnterpriseDataKey::EmployeeModificationRequests(employee.clone())).unwrap_or(Vec::new(&env));
        employee_requests.push_back(next_id);
        storage.set(&EnterpriseDataKey::EmployeeModificationRequests(employee.clone()), &employee_requests);

        // Add to employer's modification requests
        let mut employer_requests: Vec<u64> = storage.get(&EnterpriseDataKey::EmployerModificationRequests(payroll.employer.clone())).unwrap_or(Vec::new(&env));
        employer_requests.push_back(next_id);
        storage.set(&EnterpriseDataKey::EmployerModificationRequests(payroll.employer.clone()), &employer_requests);

        // Add to pending modification requests
        let mut pending_requests: Vec<u64> = storage.get(&EnterpriseDataKey::PendingModificationRequests).unwrap_or(Vec::new(&env));
        pending_requests.push_back(next_id);
        storage.set(&EnterpriseDataKey::PendingModificationRequests, &pending_requests);

        // Emit modification request event
        env.events().publish(
            (symbol_short!("mod_req"),),
            (requester, next_id, employee, modification_type, current_value, proposed_value),
        );

        Ok(next_id)
    }

    /// Approve a payroll modification request
    pub fn approve_payroll_modification(
        env: Env,
        approver: Address,
        request_id: u64,
    ) -> Result<(), PayrollError> {
        approver.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get the modification request
        let mut modification_request: PayrollModificationRequest = storage.get(&EnterpriseDataKey::PayrollModificationRequest(request_id))
            .ok_or(PayrollError::PayrollNotFound)?;

        // Check if request has expired
        if current_time > modification_request.expires_at {
            modification_request.status = PayrollModificationStatus::Expired;
            storage.set(&EnterpriseDataKey::PayrollModificationRequest(request_id), &modification_request);
            return Err(PayrollError::InvalidData);
        }

        // Check if request is already completed
        if modification_request.status == PayrollModificationStatus::BothApproved || 
           modification_request.status == PayrollModificationStatus::Rejected ||
           modification_request.status == PayrollModificationStatus::Cancelled {
            return Err(PayrollError::InvalidData);
        }

        // Determine if approver is employer or employee
        let is_employer = approver == modification_request.employer;
        let is_employee = approver == modification_request.employee;

        if !is_employer && !is_employee {
            return Err(PayrollError::Unauthorized);
        }

        // Update approval status
        if is_employer {
            modification_request.employer_approval.approver = approver.clone();
            modification_request.employer_approval.approved = true;
            modification_request.employer_approval.timestamp = current_time;
        } else {
            modification_request.employee_approval.approver = approver.clone();
            modification_request.employee_approval.approved = true;
            modification_request.employee_approval.timestamp = current_time;
        }

        // Check if both parties have approved
        if modification_request.employer_approval.approved && 
           modification_request.employee_approval.approved {
            modification_request.status = PayrollModificationStatus::BothApproved;
            
            // Apply the modification to the payroll
            Self::_apply_payroll_modification(&env, &modification_request)?;
        } else if modification_request.employer_approval.approved {
            modification_request.status = PayrollModificationStatus::EmployerApproved;
        } else if modification_request.employee_approval.approved {
            modification_request.status = PayrollModificationStatus::EmployeeApproved;
        }

        // Store updated modification request
        storage.set(&EnterpriseDataKey::PayrollModificationRequest(request_id), &modification_request);

        // Remove from pending requests if completed
        if modification_request.status == PayrollModificationStatus::BothApproved || 
           modification_request.status == PayrollModificationStatus::Rejected {
            let mut pending_requests: Vec<u64> = storage.get(&EnterpriseDataKey::PendingModificationRequests).unwrap_or(Vec::new(&env));
            let mut new_pending_requests = Vec::new(&env);
            for id in pending_requests.iter() {
                if id != request_id {
                    new_pending_requests.push_back(id);
                }
            }
            storage.set(&EnterpriseDataKey::PendingModificationRequests, &new_pending_requests);
        }

        // Emit approval event
        env.events().publish(
            (symbol_short!("mod_app"),),
            (approver, request_id, modification_request.status),
        );

        Ok(())
    }

    /// Reject a payroll modification request
    pub fn reject_payroll_modification(
        env: Env,
        rejector: Address,
        request_id: u64,
        rejection_reason: String,
    ) -> Result<(), PayrollError> {
        rejector.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get the modification request
        let mut modification_request: PayrollModificationRequest = storage.get(&EnterpriseDataKey::PayrollModificationRequest(request_id))
            .ok_or(PayrollError::PayrollNotFound)?;

        // Check if request has expired
        if current_time > modification_request.expires_at {
            modification_request.status = PayrollModificationStatus::Expired;
            storage.set(&EnterpriseDataKey::PayrollModificationRequest(request_id), &modification_request);
            return Err(PayrollError::InvalidData);
        }

        // Check if request is already completed
        if modification_request.status == PayrollModificationStatus::BothApproved || 
           modification_request.status == PayrollModificationStatus::Rejected ||
           modification_request.status == PayrollModificationStatus::Cancelled {
            return Err(PayrollError::InvalidData);
        }

        // Determine if rejector is employer or employee
        let is_employer = rejector == modification_request.employer;
        let is_employee = rejector == modification_request.employee;

        if !is_employer && !is_employee {
            return Err(PayrollError::Unauthorized);
        }

        // Update approval status to rejected
        if is_employer {
            modification_request.employer_approval.approver = rejector.clone();
            modification_request.employer_approval.approved = false;
            modification_request.employer_approval.timestamp = current_time;
            modification_request.employer_approval.comment = rejection_reason.clone();
        } else {
            modification_request.employee_approval.approver = rejector.clone();
            modification_request.employee_approval.approved = false;
            modification_request.employee_approval.timestamp = current_time;
            modification_request.employee_approval.comment = rejection_reason.clone();
        }

        // Set status to rejected
        modification_request.status = PayrollModificationStatus::Rejected;

        // Store updated modification request
        storage.set(&EnterpriseDataKey::PayrollModificationRequest(request_id), &modification_request);

        // Remove from pending requests
        let mut pending_requests: Vec<u64> = storage.get(&EnterpriseDataKey::PendingModificationRequests).unwrap_or(Vec::new(&env));
        let mut new_pending_requests = Vec::new(&env);
        for id in pending_requests.iter() {
            if id != request_id {
                new_pending_requests.push_back(id);
            }
        }
        storage.set(&EnterpriseDataKey::PendingModificationRequests, &new_pending_requests);

        // Emit rejection event
        env.events().publish(
            (symbol_short!("mod_rej"),),
            (rejector, request_id, rejection_reason),
        );

        Ok(())
    }

    /// Get a payroll modification request
    pub fn get_payroll_modification_request(
        env: Env,
        request_id: u64,
    ) -> Result<PayrollModificationRequest, PayrollError> {
        let storage = env.storage().persistent();
        storage.get(&EnterpriseDataKey::PayrollModificationRequest(request_id))
            .ok_or(PayrollError::PayrollNotFound)
    }

    /// Get all modification requests for an employee
    pub fn get_employee_mod_requests(
        env: Env,
        employee: Address,
    ) -> Result<Vec<PayrollModificationRequest>, PayrollError> {
        let storage = env.storage().persistent();
        let request_ids: Vec<u64> = storage.get(&EnterpriseDataKey::EmployeeModificationRequests(employee.clone())).unwrap_or(Vec::new(&env));
        
        let mut requests = Vec::new(&env);
        for id in request_ids.iter() {
            if let Some(request) = storage.get(&EnterpriseDataKey::PayrollModificationRequest(id)) {
                requests.push_back(request);
            }
        }
        
        Ok(requests)
    }

    /// Get all modification requests for an employer
    pub fn get_employer_mod_requests(
        env: Env,
        employer: Address,
    ) -> Result<Vec<PayrollModificationRequest>, PayrollError> {
        let storage = env.storage().persistent();
        let request_ids: Vec<u64> = storage.get(&EnterpriseDataKey::EmployerModificationRequests(employer.clone())).unwrap_or(Vec::new(&env));
        
        let mut requests = Vec::new(&env);
        for id in request_ids.iter() {
            if let Some(request) = storage.get(&EnterpriseDataKey::PayrollModificationRequest(id)) {
                requests.push_back(request);
            }
        }
        
        Ok(requests)
    }

    /// Get all pending modification requests
    pub fn get_pending_mod_requests(
        env: Env,
    ) -> Result<Vec<PayrollModificationRequest>, PayrollError> {
        let storage = env.storage().persistent();
        let request_ids: Vec<u64> = storage.get(&EnterpriseDataKey::PendingModificationRequests).unwrap_or(Vec::new(&env));
        
        let mut requests = Vec::new(&env);
        for id in request_ids.iter() {
            if let Some(request) = storage.get(&EnterpriseDataKey::PayrollModificationRequest(id)) {
                requests.push_back(request);
            }
        }
        
        Ok(requests)
    }

    /// Apply a payroll modification to the actual payroll
    fn _apply_payroll_modification(
        env: &Env,
        modification_request: &PayrollModificationRequest,
    ) -> Result<(), PayrollError> {
        let storage = env.storage().persistent();
        
        // Get the current payroll
        let mut payroll = Self::_get_payroll(env, &modification_request.employee)
            .ok_or(PayrollError::PayrollNotFound)?;

        // Apply the modification based on type
        match modification_request.request_type {
            PayrollModificationType::Salary => {
                // Parse the proposed salary value (simplified parsing)
                let new_salary = Self::_parse_i128(&modification_request.proposed_value)?;
                payroll.amount = new_salary;
            },
            PayrollModificationType::Interval => {
                // Parse the proposed interval value (simplified parsing)
                let new_interval = Self::_parse_u64(&modification_request.proposed_value)?;
                payroll.interval = new_interval;
            },
            PayrollModificationType::RecurrenceFrequency => {
                // Parse the proposed recurrence frequency value (simplified parsing)
                let new_frequency = Self::_parse_u64(&modification_request.proposed_value)?;
                payroll.recurrence_frequency = new_frequency;
            },
            PayrollModificationType::Token => {
                // Parse the proposed token address (simplified)
                // In a real implementation, this would properly parse the address
                // For now, we'll use a default address
                payroll.token = Address::from_str(&env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF");
            },
            PayrollModificationType::Custom(_) => {
                // For custom modifications, the implementation would depend on the specific use case
                // This is a placeholder for future custom modification types
                return Err(PayrollError::InvalidData);
            }
        }

        // Update the payroll
        let compact_payroll = Self::to_compact_payroll(&payroll);
        storage.set(&DataKey::Payroll(modification_request.employee.clone()), &compact_payroll);

        // Emit modification applied event
        env.events().publish(
            (symbol_short!("mod_appl"),),
            (modification_request.employee.clone(), modification_request.request_type.clone(), modification_request.proposed_value.clone()),
        );

        Ok(())
    }

    //-----------------------------------------------------------------------------
    // Multi-Signature Support Functions
    //-----------------------------------------------------------------------------
    //-----------------------------------------------------------------------------
    // Multi-Signature Support Functions
    //-----------------------------------------------------------------------------

    /// Enhanced transfer ownership with multi-signature support
    pub fn transfer_ownership_with_multisig(
        env: Env,
        caller: Address,
        new_owner: Address,
    ) -> Result<(), PayrollError> {
        caller.require_auth();

        // Simple multi-signature: require both caller and new_owner approval
        // In a production environment, this would be more sophisticated
        let storage = env.storage().persistent();
        
        // Check if this is a pending transfer request
        let pending_key = DataKey::Role(String::from_str(&env, "pending_ownership_transfer"));
        if let Some(pending_transfer) = storage.get::<DataKey, Address>(&pending_key) {
            // If there's a pending transfer, check if the new owner is confirming
            if caller == new_owner {
                // New owner is confirming the transfer
                storage.remove(&pending_key);
                storage.set(&DataKey::Owner, &new_owner);
                
                // Emit event
                env.events().publish(
                    (symbol_short!("owner_tr"),),
                    (caller, new_owner),
                );
                return Ok(());
            } else {
                return Err(PayrollError::Unauthorized);
            }
        }

        // Create a pending transfer request
        storage.set(&pending_key, &new_owner);
        
        // Emit event for pending transfer
        env.events().publish(
            (symbol_short!("pend_tr"),),
            (caller, new_owner),
        );

        Ok(())
    }

    /// Enhanced pause contract with multi-signature support
    pub fn pause_contract_with_multisig(
        env: Env,
        caller: Address,
    ) -> Result<(), PayrollError> {
        caller.require_auth();

        // Simple multi-signature: require both owner and caller approval
        let storage = env.storage().persistent();
        let owner = storage.get(&DataKey::Owner).ok_or(PayrollError::Unauthorized)?;
        
        if caller == owner {
            // Owner can pause directly
            Self::pause(env, caller)
        } else {
            // Non-owner needs to create a pending pause request
            let pending_key = DataKey::Role(String::from_str(&env, "pending_pause_request"));
            storage.set(&pending_key, &caller);
            
            // Emit event for pending pause
            env.events().publish(
                (symbol_short!("pending_p"),),
                caller,
            );
            
            Ok(())
        }
    }

    /// Enhanced unpause contract with multi-signature support
    pub fn unpause_contract_with_multisig(
        env: Env,
        caller: Address,
    ) -> Result<(), PayrollError> {
        caller.require_auth();

        // Simple multi-signature: require both owner and caller approval
        let storage = env.storage().persistent();
        let owner = storage.get(&DataKey::Owner).ok_or(PayrollError::Unauthorized)?;
        
        if caller == owner {
            // Owner can unpause directly
            Self::unpause(env, caller)
        } else {
            // Non-owner needs to create a pending unpause request
            let pending_key = DataKey::Role(String::from_str(&env, "pending_unpause_request"));
            storage.set(&pending_key, &caller);
            
            // Emit event for pending unpause
            env.events().publish(
                (symbol_short!("pending_u"),),
                caller,
            );
            
            Ok(())
        }
    }

    /// Confirm pending multi-signature operations
    pub fn confirm_multisig_operation(
        env: Env,
        caller: Address,
        operation_type: String,
    ) -> Result<(), PayrollError> {
        caller.require_auth();

        let storage = env.storage().persistent();
        let owner = storage.get(&DataKey::Owner).ok_or(PayrollError::Unauthorized)?;

        // Only owner can confirm operations
        if caller != owner {
            return Err(PayrollError::Unauthorized);
        }

        if operation_type == String::from_str(&env, "pause") {
            let pending_key = DataKey::Role(String::from_str(&env, "pending_pause_request"));
            if let Some(requester) = storage.get::<DataKey, Address>(&pending_key) {
                storage.remove(&pending_key);
                storage.set(&DataKey::Paused, &true);
                
                env.events().publish(
                    (PAUSED_EVENT,),
                    (requester, caller),
                );
            }
        } else if operation_type == String::from_str(&env, "unpause") {
            let pending_key = DataKey::Role(String::from_str(&env, "pending_unpause_request"));
            if let Some(requester) = storage.get::<DataKey, Address>(&pending_key) {
                storage.remove(&pending_key);
                storage.set(&DataKey::Paused, &false);
                
                env.events().publish(
                    (UNPAUSED_EVENT,),
                    (requester, caller),
                );
            }
        } else {
            return Err(PayrollError::InvalidData);
        }

        Ok(())
    }

    /// Get pending multi-signature operations
    pub fn get_pending_multisig_operations(
        env: Env,
        caller: Address,
    ) -> Result<Vec<String>, PayrollError> {
        caller.require_auth();

        let storage = env.storage().persistent();
        let owner = storage.get(&DataKey::Owner).ok_or(PayrollError::Unauthorized)?;

        // Only owner can view pending operations
        if caller != owner {
            return Err(PayrollError::Unauthorized);
        }

        let mut pending_operations = Vec::new(&env);

        // Check for pending pause request
        let pause_key = DataKey::Role(String::from_str(&env, "pending_pause_request"));
        if storage.has(&pause_key) {
            pending_operations.push_back(String::from_str(&env, "pause"));
        }

        // Check for pending unpause request
        let unpause_key = DataKey::Role(String::from_str(&env, "pending_unpause_request"));
        if storage.has(&unpause_key) {
            pending_operations.push_back(String::from_str(&env, "unpause"));
        }

        // Check for pending ownership transfer
        let transfer_key = DataKey::Role(String::from_str(&env, "pending_ownership_transfer"));
        if storage.has(&transfer_key) {
            pending_operations.push_back(String::from_str(&env, "ownership_transfer"));
        }

        Ok(pending_operations)
    }

    /// Cancel pending multi-signature operations
    pub fn cancel_multisig_operation(
        env: Env,
        caller: Address,
        operation_type: String,
    ) -> Result<(), PayrollError> {
        caller.require_auth();

        let storage = env.storage().persistent();
        let owner = storage.get(&DataKey::Owner).ok_or(PayrollError::Unauthorized)?;

        // Only owner can cancel operations
        if caller != owner {
            return Err(PayrollError::Unauthorized);
        }

        if operation_type == String::from_str(&env, "pause") {
            let pending_key = DataKey::Role(String::from_str(&env, "pending_pause_request"));
            storage.remove(&pending_key);
        } else if operation_type == String::from_str(&env, "unpause") {
            let pending_key = DataKey::Role(String::from_str(&env, "pending_unpause_request"));
            storage.remove(&pending_key);
        } else if operation_type == String::from_str(&env, "ownership_transfer") {
            let pending_key = DataKey::Role(String::from_str(&env, "pending_ownership_transfer"));
            storage.remove(&pending_key);
        } else {
            return Err(PayrollError::InvalidData);
        }

        env.events().publish(
            (symbol_short!("cancel_op"),),
            (caller, operation_type),
        );

        Ok(())
    }

    /// Parse i128 from string (simplified implementation)
    fn _parse_i128(value: &String) -> Result<i128, PayrollError> {
        // Simplified parsing - in a real implementation, this would be more robust
        // For now, we'll return a default value
        Ok(1000) // Default value
    }

    /// Parse u64 from string (simplified implementation)
    fn _parse_u64(value: &String) -> Result<u64, PayrollError> {
        // Simplified parsing - in a real implementation, this would be more robust
        // For now, we'll return a default value
        Ok(86400) // Default value (1 day in seconds)
    }
}
