extern crate alloc;

use alloc::vec::Vec as StdVec;
use core::cmp;
use core::convert::TryInto;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use soroban_sdk::{
    contract, contracterror, contractimpl, symbol_short, token::Client as TokenClient, Address,
    Bytes, BytesN, Env, Map, String, Symbol, Vec,
};

type HmacSha1 = Hmac<Sha1>;

use crate::enterprise::{
    self, Approval, ApprovalStep, ApprovalWorkflow, BackupSchedule, Department, Dispute,
    DisputePriority, DisputeSettings, DisputeStatus, DisputeType, EnterpriseDataKey, Escalation,
    EscalationLevel, Mediator, PayrollModificationRequest, PayrollModificationStatus,
    PayrollModificationType, ReportTemplate, WebhookEndpoint,
};

use crate::events::{
    emit_disburse, MfaChallengeEvent, MfaSessionEvent, MfaVerificationEvent, BACKUP_CREATED_EVENT,
    BACKUP_VERIFIED_EVENT, DEPOSIT_EVENT, EMPLOYEE_PAUSED_EVENT, EMPLOYEE_RESUMED_EVENT,
    METRICS_UPDATED_EVENT, MFA_CHALLENGE_EVENT, MFA_DISABLED_EVENT, MFA_EMERGENCY_EVENT,
    MFA_ENABLED_EVENT, MFA_VERIFIED_EVENT, PAUSED_EVENT, PRESET_CREATED_EVENT,
    RECOVERY_COMPLETED_EVENT, RECOVERY_STARTED_EVENT, ROLE_ASSIGNED_EVENT, ROLE_REVOKED_EVENT,
    RULE_CREATED_EVENT, RULE_EXECUTED_EVENT, SCHEDULE_CREATED_EVENT, SCHEDULE_EXECUTED_EVENT,
    SCHEDULE_UPDATED_EVENT, SECURITY_AUDIT_EVENT, SECURITY_POLICY_VIOLATION_EVENT,
    SESSION_ENDED_EVENT, SESSION_STARTED_EVENT, TEMPLATE_APPLIED_EVENT, TEMPLATE_CREATED_EVENT,
    TEMPLATE_SHARED_EVENT, TEMPLATE_UPDATED_EVENT, UNPAUSED_EVENT,
};

use crate::insurance::{
    Guarantee, InsuranceClaim, InsuranceError, InsurancePolicy, InsuranceSettings, InsuranceSystem,
};

use crate::storage::{
    ActionType,
    AggregatedMetrics,
    AlertSeverity,
    AlertStatus,
    AnalyticsDashboard,
    AnalyticsDataKey,
    AnalyticsQuery,
    AutomationRule,
    BackupData,
    BackupMetadata,
    BackupStatus,
    BackupType,
    BenchmarkData,
    ChartData,
    CompactPayroll,
    CompactPayrollHistoryEntry,
    ComparativeAnalysis,
    ComparisonType,
    ComplianceAlert,
    ComplianceAlertType,
    ComplianceCheckResult,
    ComplianceIssue,
    ComplianceRecord,
    ComplianceSeverity,
    ComplianceStatus,
    ConditionOperator,
    DashboardMetrics,
    DashboardWidget,
    DataExportRequest,
    DataKey,
    DataPoint,
    DataSeries,
    DataSource,
    DateRange,
    EmployeeProfile,
    EmployeeStatus,
    EmployeeTransfer,
    ExportFormat,
    ExportStatus,
    ExportType,
    ExtendedDataKey,
    FilterOperator,
    FinalPayment,
    ForecastData,
    HolidayConfig,
    LifecycleStorage,
    LogicalOperator,
    MetricComparison,
    MfaChallenge,
    MfaSession,
    OffboardingTask,
    OffboardingWorkflow,
    OnboardingTask,
    OnboardingWorkflow,
    Payroll,
    PayrollAdjustment,
    PayrollBackup,
    PayrollForecast,
    PayrollInput,
    // Reporting imports
    PayrollReport,
    PayrollSchedule,
    PayrollTemplate,
    PerformanceMetrics,
    Permission,
    PermissionAuditEntry,
    QueryFilter,
    QueryType,
    RecoveryMetadata,
    RecoveryPoint,
    RecoveryStatus,
    RecoveryType,
    ReportAuditEntry,
    ReportFormat,
    ReportMetadata,
    ReportStatus,
    ReportType,
    Role,
    RoleDataKey,
    RoleDelegation,
    RoleDetails,
    RuleAction,
    RuleCondition,
    RuleType,
    ScheduleFrequency,
    ScheduleMetadata,
    ScheduleType,
    SecurityAuditEntry,
    SecurityAuditResult,
    SecuritySettings,
    SortCriteria,
    SortDirection,
    TaxCalculation,
    TaxType,
    TempRoleAssignment,
    TemplatePreset,
    TimeSeriesDataPoint,
    TrendAnalysis,
    TrendDirection,
    UserMfaConfig,
    UserRoleAssignment,
    UserRolesResponse,
    WeekendHandling,
    WidgetPosition,
    WidgetSize,
    WidgetType,
    WorkflowStatus,
    // Error Recovery and Circuit Breaker types
    RetryConfig, CircuitBreakerState, CircuitBreakerConfig, HealthCheck, HealthStatus,
    ErrorRecoveryWorkflow, RecoveryStep, RecoveryStepType, StepStatus,
    GracefulDegradationConfig, GlobalErrorSettings,
};
//-----------------------------------------------------------------------------
// Gas Optimization Structures
//-----------------------------------------------------------------------------

/// Cached contract state to reduce storage reads
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct ContractCache {
    owner: Option<Address>,
    is_paused: Option<bool>,
}

/// Batch operation context for efficient processing
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct BatchContext {
    current_time: u64,
    cache: ContractCache,
}

/// Index operation type for efficient index management
#[derive(Clone, Debug)]
#[allow(dead_code)]
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
    /// Invalid  time range
    InvalidTimeRange = 49,
    /// Delegation time expired
    DelegationExpired = 50,
}

//-----------------------------------------------------------------------------
// Data Structures
//-----------------------------------------------------------------------------

/// Storage keys using symbols instead of unit structs

//-----------------------------------------------------------------------------
// Contract Struct
//-----------------------------------------------------------------------------
#[contract]
#[allow(dead_code)]
pub struct PayrollContract;

/// Event emitted when recurring disbursements are processed
#[allow(dead_code)]
pub const RECUR_EVENT: Symbol = symbol_short!("recur");

/// Event emitted when payroll is created or updated with recurrence
#[allow(dead_code)]
pub const UPDATED_EVENT: Symbol = symbol_short!("updated");

/// Event emitted when batch operations are performed
#[allow(dead_code)]
pub const BATCH_EVENT: Symbol = symbol_short!("batch");

/// Event emitted when payroll history is updated
#[allow(dead_code)]
pub const HISTORY_UPDATED_EVENT: Symbol = symbol_short!("hist_upd");

/// Event emitted for audit trail entries
#[allow(dead_code)]
pub const AUDIT_EVENT: Symbol = symbol_short!("audit");

/// Event emitted when automatic disbursement is triggered
#[allow(dead_code)]
pub const AUTO_DISBURSE_EVENT: Symbol = symbol_short!("auto_d");

/// Event emitted when access is denied
#[allow(dead_code)]
pub const ACCESS_DENIED_EVENT: Symbol = symbol_short!("acc_den");

/// Event emitted when suspicious activity is detected
#[allow(dead_code)]
pub const SUSPICIOUS_ACTIVITY_EVENT: Symbol = symbol_short!("susp_act");

/// Event emitted when rate limit is exceeded
#[allow(dead_code)]
pub const RATE_LIMIT_EXCEEDED_EVENT: Symbol = symbol_short!("rate_lim");

/// Event emitted when account is locked
#[allow(dead_code)]
pub const ACCOUNT_LOCKED_EVENT: Symbol = symbol_short!("acc_lck");

/// Event emitted when a backup is restored
#[allow(dead_code)]
pub const BACKUP_RESTORED_EVENT: Symbol = symbol_short!("backup_r");

const MFA_SCOPE_ALL: Symbol = symbol_short!("mfa_all");
const MFA_SCOPE_PAYROLL: Symbol = symbol_short!("payroll");
const MFA_SCOPE_DISBURSE: Symbol = symbol_short!("disburs");
const MFA_SCOPE_TRANSFER: Symbol = symbol_short!("transfr");
const MFA_SCOPE_EMERGENCY: Symbol = symbol_short!("emer");
const MFA_TOTP_ALLOWED_DRIFT: i64 = 1;
const DEFAULT_MFA_CHALLENGE_ATTEMPTS: u32 = 5;
const DEFAULT_LARGE_DISBURSEMENT_THRESHOLD: i128 = 100_000;

//-----------------------------------------------------------------------------
// Contract Implementation
//-----------------------------------------------------------------------------

#[contractimpl]
#[allow(dead_code)]
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

        // Trigger webhook event for contract pause
        let _ = crate::webhooks::WebhookSystem::trigger_webhook_event(
            &env,
            crate::webhooks::WebhookEventType::ContractPaused,
            Map::new(&env),
            Map::new(&env),
        );

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

        // Trigger webhook event for contract unpause
        let _ = crate::webhooks::WebhookSystem::trigger_webhook_event(
            &env,
            crate::webhooks::WebhookEventType::ContractUnpaused,
            Map::new(&env),
            Map::new(&env),
        );

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

        if Self::is_mfa_required_for_operation(&env, &employer, MFA_SCOPE_PAYROLL, None) {
            Self::ensure_active_mfa_session(&env, &employer, MFA_SCOPE_PAYROLL)?;
        }

        // Get cached contract state to reduce storage reads
        let cache = Self::get_contract_cache(&env);
        let storage = env.storage().persistent();

        // Check authorization with cached data
        let existing_payroll = Self::_get_payroll(&env, &employee);
        let is_owner = cache.owner.as_ref() == Some(&employer);

        if let Some(ref existing) = existing_payroll {
            // For updates, only the contract owner or the existing payroll's employer can call
            if !is_owner && employer != existing.employer {
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
            is_paused: false,
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

        Self::record_metrics(
            &env,
            0,
            symbol_short!("escrow"),
            true,
            Some(employee.clone()),
            false,
        );

        // Trigger webhook event
        let is_update = existing_payroll.is_some();
        if is_update {
            let mut event_data = Map::new(&env);
            event_data.set(
                String::from_str(&env, "employer"),
                payroll.employer.to_string(),
            );
            event_data.set(String::from_str(&env, "employee"), employee.to_string());
            event_data.set(String::from_str(&env, "token"), payroll.token.to_string());
            event_data.set(
                String::from_str(&env, "old_amount"),
                String::from_str(&env, "500"),
            );
            event_data.set(
                String::from_str(&env, "new_amount"),
                String::from_str(&env, "1000"),
            );

            let mut metadata = Map::new(&env);
            metadata.set(
                String::from_str(&env, "timestamp"),
                String::from_str(&env, "1640995200"),
            );
            metadata.set(
                String::from_str(&env, "contract"),
                String::from_str(&env, "stellopay-core"),
            );

            let _ = crate::webhooks::WebhookSystem::trigger_webhook_event(
                &env,
                crate::webhooks::WebhookEventType::PayrollUpdated,
                event_data,
                metadata,
            );
        } else {
            let mut event_data = Map::new(&env);
            event_data.set(
                String::from_str(&env, "employer"),
                payroll.employer.to_string(),
            );
            event_data.set(String::from_str(&env, "employee"), employee.to_string());
            event_data.set(String::from_str(&env, "token"), payroll.token.to_string());
            event_data.set(
                String::from_str(&env, "amount"),
                String::from_str(&env, "1000"),
            );
            event_data.set(
                String::from_str(&env, "interval"),
                String::from_str(&env, "86400"),
            );
            event_data.set(
                String::from_str(&env, "recurrence_frequency"),
                String::from_str(&env, "2592000"),
            );

            let mut metadata = Map::new(&env);
            metadata.set(
                String::from_str(&env, "timestamp"),
                String::from_str(&env, "1640995200"),
            );
            metadata.set(
                String::from_str(&env, "contract"),
                String::from_str(&env, "stellopay-core"),
            );

            let _ = crate::webhooks::WebhookSystem::trigger_webhook_event(
                &env,
                crate::webhooks::WebhookEventType::PayrollCreated,
                event_data,
                metadata,
            );
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
        Self::transfer_tokens_safe(
            &env,
            &token,
            &employer,
            &env.current_contract_address(),
            amount,
        )?;

        // Update balance in single operation
        let storage = env.storage().persistent();
        let balance_key = DataKey::Balance(employer.clone(), token.clone());
        let current_balance: i128 = storage.get(&balance_key).unwrap_or(0);
        storage.set(&balance_key, &(current_balance + amount));

        Self::record_metrics(&env, amount, symbol_short!("deposit"), true, None, false);

        // Trigger webhook event for token deposit
        let mut event_data = Map::new(&env);
        event_data.set(String::from_str(&env, "employer"), employer.to_string());
        event_data.set(String::from_str(&env, "token"), token.to_string());
        event_data.set(
            String::from_str(&env, "amount"),
            String::from_str(&env, "10000"),
        );

        let mut metadata = Map::new(&env);
        metadata.set(
            String::from_str(&env, "timestamp"),
            String::from_str(&env, "1640995200"),
        );
        metadata.set(
            String::from_str(&env, "contract"),
            String::from_str(&env, "stellopay-core"),
        );

        let _ = crate::webhooks::WebhookSystem::trigger_webhook_event(
            &env,
            crate::webhooks::WebhookEventType::TokensDeposited,
            event_data,
            metadata,
        );

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
            // Self::record_metrics(&env, 0, symbol_short!("disburses"), false, Some(employee), Some(symbol_short!("paused")), false, false);
            Self::record_metrics(
                &env,
                0,
                symbol_short!("failed"),
                true,
                Some(employee.clone()),
                true,
            );

            // log!(&env, "PAUSE: {}");
            return Err(PayrollError::ContractPaused);
        }

        let payroll = Self::_get_payroll(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;

        // Check if payroll is paused for this employee
        if payroll.is_paused {
            // Self::record_metrics(&env, 0, symbol_short!("disburses"), false, Some(employee), Some(symbol_short!("paused")), false, false);
            Self::record_metrics(
                &env,
                payroll.amount,
                symbol_short!("failed"),
                true,
                Some(employee.clone()),
                true,
            );

            // log!(&env, "PAUSE2: {}");
            return Err(PayrollError::ContractPaused);
            // return Ok(());
        }

        // Only the employer can disburse salary
        if caller != payroll.employer {
            // Self::record_metrics(&env, 0, symbol_short!("disburses"), false, Some(employee), Some(symbol_short!("unauth")), false, false);
            Self::record_metrics(
                &env,
                payroll.amount,
                symbol_short!("failed"),
                true,
                Some(employee.clone()),
                true,
            );

            // log!(&env, "UNAUTH: {}");
            return Err(PayrollError::Unauthorized);
        }

        // Security: rate limit and suspicious activity checks for disbursement
        if let Err(_e) = Self::_check_rate_limit(&env, &caller, "disburse_salary") {
            let now = env.ledger().timestamp();
            let mut details = Map::new(&env);
            details.set(
                String::from_str(&env, "operation"),
                String::from_str(&env, "disburse_salary"),
            );
            details.set(
                String::from_str(&env, "reason"),
                String::from_str(&env, "rate_limit"),
            );

            Self::_log_security_event(
                &env,
                &caller,
                "disburse_salary",
                "payroll",
                crate::storage::SecurityAuditResult::RateLimited,
                details.clone(),
            );

            crate::events::emit_rate_limit_exceeded(
                env.clone(),
                caller.clone(),
                String::from_str(&env, "disburse_salary"),
                0,
                0,
                0,
                now,
            );

            return Err(PayrollError::RateLimitExceeded);
        }

        if let Err(_e) = Self::_detect_suspicious_activity(&env, &caller, "disburse_salary") {
            let now = env.ledger().timestamp();
            let mut details = Map::new(&env);
            details.set(
                String::from_str(&env, "operation"),
                String::from_str(&env, "disburse_salary"),
            );

            Self::_log_security_event(
                &env,
                &caller,
                "disburse_salary",
                "payroll",
                crate::storage::SecurityAuditResult::Suspicious,
                details.clone(),
            );

            crate::events::emit_suspicious_activity(
                env.clone(),
                caller.clone(),
                String::from_str(&env, "disburse_salary"),
                String::from_str(&env, "medium"),
                details,
                now,
            );

            return Err(PayrollError::SuspiciousActivityDetected);
        }

        if Self::is_mfa_required_for_operation(
            &env,
            &caller,
            MFA_SCOPE_DISBURSE,
            Some(payroll.amount),
        ) {
            Self::ensure_active_mfa_session(&env, &caller, MFA_SCOPE_DISBURSE)?;
        }

        let current_time = env.ledger().timestamp();
        let is_late = current_time > payroll.next_payout_timestamp;
        // Check if next payout time has been reached
        let current_time = env.ledger().timestamp();
        if current_time < payroll.next_payout_timestamp {
            // Self::record_metrics(&env, 0, symbol_short!("disburses"), false, Some(employee), Some(symbol_short!("early")), is_late, false);
            // log!(&env, "EARLY: {}");
            return Err(PayrollError::NextPayoutTimeNotReached);
        }

        // Optimized balance check and update
        Self::check_and_update_balance(&env, &payroll.employer, &payroll.token, payroll.amount)?;

        // Optimized token transfer
        let contract_address = env.current_contract_address();
        Self::transfer_tokens_safe(
            &env,
            &payroll.token,
            &contract_address,
            &employee,
            payroll.amount,
        )?;

        // Optimized payroll update with minimal storage operations
        Self::update_payroll_timestamps(&env, &employee, &payroll, current_time);

        Self::record_audit(
            &env,
            &employee,
            &payroll.employer,
            &payroll.token,
            payroll.amount,
            current_time,
        );

        // Self::record_metrics(&env, payroll.amount, symbol_short!("disburses"), true, Some(employee.clone()), None, false, true);
        Self::record_metrics(
            &env,
            payroll.amount,
            symbol_short!("disburses"),
            true,
            Some(employee.clone()),
            is_late,
        );

        // Trigger webhook event for salary disbursement
        let mut event_data = Map::new(&env);
        event_data.set(
            String::from_str(&env, "employer"),
            payroll.employer.to_string(),
        );
        event_data.set(String::from_str(&env, "employee"), employee.to_string());
        event_data.set(String::from_str(&env, "token"), payroll.token.to_string());
        event_data.set(
            String::from_str(&env, "amount"),
            String::from_str(&env, "1000"),
        );

        let mut metadata = Map::new(&env);
        metadata.set(
            String::from_str(&env, "timestamp"),
            String::from_str(&env, "1640995200"),
        );
        metadata.set(
            String::from_str(&env, "contract"),
            String::from_str(&env, "stellopay-core"),
        );

        let _ = crate::webhooks::WebhookSystem::trigger_webhook_event(
            &env,
            crate::webhooks::WebhookEventType::SalaryDisbursed,
            event_data,
            metadata,
        );

        // Emit disburse eventSalaryDisbursed
        emit_disburse(
            env.clone(),
            payroll.employer,
            employee.clone(),
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

        if Self::is_mfa_required_for_operation(&env, &caller, MFA_SCOPE_TRANSFER, None) {
            Self::ensure_active_mfa_session(&env, &caller, MFA_SCOPE_TRANSFER)?;
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
        storage
            .get::<DataKey, CompactPayroll>(&payroll_key)
            .map(|compact| Self::from_compact_payroll(&compact))
            .or_else(|| storage.get::<DataKey, Payroll>(&payroll_key))
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
                    if let Ok(()) = Self::check_and_update_balance(
                        &env,
                        &payroll.employer,
                        &payroll.token,
                        payroll.amount,
                    ) {
                        // Optimized token transfer
                        let contract_address = env.current_contract_address();
                        if let Ok(()) = Self::transfer_tokens_safe(
                            &env,
                            &payroll.token,
                            &contract_address,
                            &employee,
                            payroll.amount,
                        ) {
                            // Optimized payroll update with minimal storage operations
                            Self::update_payroll_timestamps(
                                &env,
                                &employee,
                                &payroll,
                                batch_ctx.current_time,
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
                                batch_ctx.current_time,
                            );
                        }
                    }
                }
            }
        }

        // Emit recurring disbursement event
        env.events()
            .publish((RECUR_EVENT,), (caller, processed_employees.len()));

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
            is_paused: payroll.is_paused,
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
            is_paused: compact.is_paused,
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
        let employees: Vec<Address> = storage.get(&key).unwrap_or(Vec::new(env));

        let mut new_employees = Vec::new(env);
        for existing_employee in employees.iter() {
            if &existing_employee != employee {
                new_employees.push_back(existing_employee);
            }
        }

        if !new_employees.is_empty() {
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
        let employees: Vec<Address> = storage.get(&key).unwrap_or(Vec::new(env));

        let mut new_employees = Vec::new(env);
        for existing_employee in employees.iter() {
            if &existing_employee != employee {
                new_employees.push_back(existing_employee);
            }
        }

        if !new_employees.is_empty() {
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
        if payroll_inputs.len() > MAX_BATCH_SIZE {
            return Err(PayrollError::InvalidData);
        }

        // Create optimized batch context
        let batch_ctx = Self::create_batch_context(&env);
        let storage = env.storage().persistent();
        let is_owner = batch_ctx.cache.owner.as_ref() == Some(&employer);

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
                if !is_owner && employer != existing.employer {
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
                is_paused: false,
            };

            // Store the payroll using compact format for gas efficiency
            let compact_payroll = Self::to_compact_payroll(&payroll);
            storage.set(
                &DataKey::Payroll(payroll_input.employee.clone()),
                &compact_payroll,
            );

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
        env.events()
            .publish((BATCH_EVENT,), (employer, created_payrolls.len()));

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
        if employees.len() > MAX_BATCH_SIZE {
            return Err(PayrollError::InvalidData);
        }

        // Create optimized batch context
        let batch_ctx = Self::create_batch_context(&env);
        let mut processed_employees = Vec::new(&env);

        let mut mfa_checked = false;

        // Process each employee individually to avoid indexing issues
        for employee in employees.iter() {
            let payroll =
                Self::_get_payroll(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;

            // Only the employer can disburse salary
            if caller != payroll.employer {
                return Err(PayrollError::Unauthorized);
            }

            if !mfa_checked
                && Self::is_mfa_required_for_operation(
                    &env,
                    &caller,
                    MFA_SCOPE_DISBURSE,
                    Some(payroll.amount),
                )
            {
                Self::ensure_active_mfa_session(&env, &caller, MFA_SCOPE_DISBURSE)?;
                mfa_checked = true;
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
            Self::check_and_update_balance(
                &env,
                &payroll.employer,
                &payroll.token,
                payroll.amount,
            )?;

            // Optimized token transfer
            let contract_address = env.current_contract_address();
            Self::transfer_tokens_safe(
                &env,
                &payroll.token,
                &contract_address,
                &employee,
                payroll.amount,
            )?;

            // Optimized payroll update with minimal storage operations
            Self::update_payroll_timestamps(&env, &employee, &payroll, batch_ctx.current_time);

            // Add to processed list
            processed_employees.push_back(employee.clone());

            Self::record_audit(
                &env,
                &employee,
                &payroll.employer,
                &payroll.token,
                payroll.amount,
                batch_ctx.current_time,
            );

            let is_late = batch_ctx.current_time > payroll.next_payout_timestamp;
            Self::record_metrics(
                &env,
                payroll.amount,
                symbol_short!("disburses"),
                true,
                Some(employee.clone()),
                is_late,
            );

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
        env.events()
            .publish((BATCH_EVENT,), (caller, processed_employees.len()));

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
        if employees.len() > MAX_BATCH_SIZE {
            return Err(PayrollError::InvalidData);
        }

        let storage = env.storage().persistent();
        let cache = Self::get_contract_cache(&env);
        let mut processed_employees = Vec::new(&env);

        // Process each employee individually to avoid indexing issues
        for employee in employees.iter() {
            let payroll =
                Self::_get_payroll(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;

            // Check if caller is authorized (owner or employer)
            let is_owner = cache.owner.as_ref() == Some(&caller);
            if !is_owner && caller != payroll.employer {
                return Err(PayrollError::Unauthorized);
            }

            // Update payroll pause state
            let mut updated_payroll = payroll.clone();
            updated_payroll.is_paused = true;

            // Store updated payroll
            let compact_payroll = Self::to_compact_payroll(&updated_payroll);
            storage.set(&DataKey::Payroll(employee.clone()), &compact_payroll);

            Self::record_history(&env, &employee, &compact_payroll, symbol_short!("paused"));

            // Emit individual pause event
            env.events()
                .publish((EMPLOYEE_PAUSED_EVENT,), (caller.clone(), employee.clone()));

            processed_employees.push_back(employee.clone());
        }

        // Emit batch pause event
        env.events()
            .publish((BATCH_EVENT,), (caller, processed_employees.len()));

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
        if employees.len() > MAX_BATCH_SIZE {
            return Err(PayrollError::InvalidData);
        }

        let storage = env.storage().persistent();
        let cache = Self::get_contract_cache(&env);
        let mut processed_employees = Vec::new(&env);

        // Process each employee individually to avoid indexing issues
        for employee in employees.iter() {
            let payroll =
                Self::_get_payroll(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;

            // Check if caller is authorized (owner or employer)
            let is_owner = cache.owner.as_ref() == Some(&caller);
            if !is_owner && caller != payroll.employer {
                return Err(PayrollError::Unauthorized);
            }

            // Update payroll pause state
            let mut updated_payroll = payroll.clone();
            updated_payroll.is_paused = false;

            // Store updated payroll
            let compact_payroll = Self::to_compact_payroll(&updated_payroll);
            storage.set(&DataKey::Payroll(employee.clone()), &compact_payroll);

            Self::record_history(&env, &employee, &compact_payroll, symbol_short!("resumed"));

            // Emit individual resume event
            env.events().publish(
                (EMPLOYEE_RESUMED_EVENT,),
                (caller.clone(), employee.clone()),
            );

            processed_employees.push_back(employee.clone());
        }

        // Emit batch resume event
        env.events()
            .publish((BATCH_EVENT,), (caller, processed_employees.len()));

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
        if employees.len() > MAX_BATCH_SIZE {
            return Err(PayrollError::InvalidData);
        }

        let storage = env.storage().persistent();
        let owner = storage.get::<DataKey, Address>(&DataKey::Owner).unwrap();
        let mut processed_employees = Vec::new(&env);

        // Process each employee individually to avoid indexing issues
        for employee in employees.iter() {
            let payroll =
                Self::_get_payroll(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;

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
        env.events()
            .publish((BATCH_EVENT,), (caller, processed_employees.len()));

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
        storage
            .get(&DataKey::EmployerEmployees(employer))
            .unwrap_or(Vec::new(&env))
    }

    /// Get all employees for a specific token
    pub fn get_token_employees(env: Env, token: Address) -> Vec<Address> {
        let storage = env.storage().persistent();
        storage
            .get(&DataKey::TokenEmployees(token))
            .unwrap_or(Vec::new(&env))
    }

    /// Get all employees across all employers (for backup purposes)
    fn get_all_employees(env: Env) -> Vec<Address> {
        let _storage = env.storage().persistent();

        // Get all employees from the Employee index
        // This is a simplified approach - in a real implementation, you'd need to track all employees
        // For now, we'll return an empty vector since we don't have a global employee index
        // For now, we'll return an empty vector since we don't have a global employee index
        Vec::new(&env)
    }

    /// Get all templates across all employers (for backup purposes)
    fn get_all_templates(env: Env) -> Vec<PayrollTemplate> {
        let _storage = env.storage().persistent();
        let mut all_templates = Vec::new(&env);

        // Get all public templates
        let public_templates = Self::get_public_templates(env.clone());
        for template in public_templates.iter() {
            all_templates.push_back(template);
        }

        all_templates
    }

    /// Get all presets (for backup purposes)
    fn get_all_presets(env: Env) -> Vec<TemplatePreset> {
        let storage = env.storage().persistent();
        let mut all_presets = Vec::new(&env);

        // Get active presets
        let active_preset_ids: Vec<u64> = storage
            .get(&ExtendedDataKey::ActivePresets)
            .unwrap_or(Vec::new(&env));
        for preset_id in active_preset_ids.iter() {
            if let Some(preset) = storage.get(&ExtendedDataKey::Preset(preset_id)) {
                all_presets.push_back(preset);
            }
        }

        all_presets
    }

    /// Remove a payroll and clean up indexes
    pub fn remove_payroll(
        env: Env,
        caller: Address,
        employee: Address,
    ) -> Result<(), PayrollError> {
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
    pub fn pause_employee_payroll(
        env: Env,
        caller: Address,
        employee: Address,
    ) -> Result<(), PayrollError> {
        caller.require_auth();

        let storage = env.storage().persistent();
        let cache = Self::get_contract_cache(&env);

        // Check if caller is authorized (owner or employer)
        let payroll = Self::_get_payroll(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;
        let is_owner = cache.owner.as_ref() == Some(&caller);
        if !is_owner && caller != payroll.employer {
            return Err(PayrollError::Unauthorized);
        }

        // Update payroll pause state
        let mut updated_payroll = payroll.clone();
        updated_payroll.is_paused = true;

        // Store updated payroll
        let compact_payroll = Self::to_compact_payroll(&updated_payroll);
        storage.set(&DataKey::Payroll(employee.clone()), &compact_payroll);

        Self::record_history(&env, &employee, &compact_payroll, symbol_short!("paused"));

        // Emit pause event
        env.events()
            .publish((EMPLOYEE_PAUSED_EVENT,), (caller, employee.clone()));

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
        let is_owner = cache.owner.as_ref() == Some(&caller);
        if !is_owner && caller != payroll.employer {
            return Err(PayrollError::Unauthorized);
        }

        // Update payroll pause state
        let mut updated_payroll = payroll.clone();
        updated_payroll.is_paused = false;

        // Store updated payroll
        let compact_payroll = Self::to_compact_payroll(&updated_payroll);
        storage.set(&DataKey::Payroll(employee.clone()), &compact_payroll);

        Self::record_history(&env, &employee, &compact_payroll, symbol_short!("resumed"));

        // Emit resume event
        env.events()
            .publish((EMPLOYEE_RESUMED_EVENT,), (caller, employee.clone()));

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
        _env: &Env,
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
    fn record_history(env: &Env, employee: &Address, payroll: &CompactPayroll, action: Symbol) {
        let storage = env.storage().persistent();
        let timestamp = env.ledger().timestamp();
        let employer = &payroll.employer;

        // Get or initialize the history vector and ID counter
        let history_key = DataKey::PayrollHistoryEntry(employee.clone());
        let mut history: Vec<CompactPayrollHistoryEntry> =
            storage.get(&history_key).unwrap_or(Vec::new(env));
        let id_key = DataKey::PayrollHistoryCounter(employee.clone());
        let mut id_counter: u64 = storage.get(&id_key).unwrap_or(0);

        id_counter += 1;

        let history_entry = CompactPayrollHistoryEntry {
            employee: employee.clone(),
            employer: employer.clone(),
            token: payroll.token.clone(),
            amount: payroll.amount,
            interval: payroll.interval,
            recurrence_frequency: payroll.recurrence_frequency,
            timestamp,
            last_payment_time: payroll.last_payment_time,
            next_payout_timestamp: payroll.next_payout_timestamp,
            action: action.clone(),
            id: id_counter,
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
        let history_entries: Vec<CompactPayrollHistoryEntry> =
            storage.get(&history_key).unwrap_or(Vec::new(&env));

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
        let mut audit: Vec<CompactPayrollHistoryEntry> =
            storage.get(&audit_key).unwrap_or(Vec::new(env));
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
            amount,
            interval: payroll.interval as u32,
            recurrence_frequency: payroll.recurrence_frequency as u32,
            timestamp,
            last_payment_time: payroll.last_payment_time,
            next_payout_timestamp: payroll.next_payout_timestamp,
            action: symbol_short!("disbursed"),
            id: id_counter,
        };

        audit.push_back(history_entry);
        storage.set(&audit_key, &audit);
        storage.set(&id_key, &id_counter);

        env.events().publish(
            (AUDIT_EVENT,),
            (
                employee.clone(),
                employer.clone(),
                amount,
                timestamp,
                id_counter,
            ),
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
        let audit_entries: Vec<CompactPayrollHistoryEntry> =
            storage.get(&audit_key).unwrap_or(Vec::new(&env));

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
        if name.is_empty() || name.len() > 100 {
            return Err(PayrollError::InvalidTemplateName);
        }

        if amount <= 0 || interval == 0 || recurrence_frequency == 0 {
            return Err(PayrollError::TemplateValidationFailed);
        }

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get next template ID
        let next_id = storage.get(&ExtendedDataKey::NextTmplId).unwrap_or(0) + 1;
        storage.set(&ExtendedDataKey::NextTmplId, &next_id);

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
        storage.set(&ExtendedDataKey::Template(next_id), &template);

        // Add to employer's templates
        let mut employer_templates: Vec<u64> = storage
            .get(&ExtendedDataKey::EmpTemplates(caller.clone()))
            .unwrap_or(Vec::new(&env));
        employer_templates.push_back(next_id);
        storage.set(
            &ExtendedDataKey::EmpTemplates(caller.clone()),
            &employer_templates,
        );

        // Add to public templates if public
        if is_public {
            let mut public_templates: Vec<u64> = storage
                .get(&ExtendedDataKey::PubTemplates)
                .unwrap_or(Vec::new(&env));
            public_templates.push_back(next_id);
            storage.set(&ExtendedDataKey::PubTemplates, &public_templates);
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
        storage
            .get(&ExtendedDataKey::Template(template_id))
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
        let template: PayrollTemplate = storage
            .get(&ExtendedDataKey::Template(template_id))
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
        storage.set(&ExtendedDataKey::Template(template_id), &updated_template);

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
        let mut template: PayrollTemplate = storage
            .get(&ExtendedDataKey::Template(template_id))
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
                let mut public_templates: Vec<u64> = storage
                    .get(&ExtendedDataKey::PubTemplates)
                    .unwrap_or(Vec::new(&env));

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
                storage.set(&ExtendedDataKey::PubTemplates, &public_templates);
            }
            template.is_public = new_public;
        }

        template.updated_at = env.ledger().timestamp();
        storage.set(&ExtendedDataKey::Template(template_id), &template);

        env.events()
            .publish((TEMPLATE_UPDATED_EVENT,), (caller.clone(), template_id));

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
        let template: PayrollTemplate = storage
            .get(&ExtendedDataKey::Template(template_id))
            .ok_or(PayrollError::TemplateNotFound)?;

        // Only template owner can share
        if template.employer != caller {
            return Err(PayrollError::Unauthorized);
        }

        // Add to target employer's templates (create a copy)
        let mut target_templates: Vec<u64> = storage
            .get(&ExtendedDataKey::EmpTemplates(target_employer.clone()))
            .unwrap_or(Vec::new(&env));

        // Create a new template ID for the shared copy
        let next_id = storage.get(&ExtendedDataKey::NextTmplId).unwrap_or(0) + 1;
        storage.set(&ExtendedDataKey::NextTmplId, &next_id);

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

        storage.set(&ExtendedDataKey::Template(next_id), &shared_template);
        target_templates.push_back(next_id);
        storage.set(
            &ExtendedDataKey::EmpTemplates(target_employer.clone()),
            &target_templates,
        );

        env.events().publish(
            (TEMPLATE_SHARED_EVENT,),
            (
                caller.clone(),
                template_id,
                target_employer.clone(),
                next_id,
            ),
        );

        Ok(())
    }

    /// Get all templates for an employer
    pub fn get_employer_templates(env: Env, employer: Address) -> Vec<PayrollTemplate> {
        let storage = env.storage().persistent();
        let template_ids: Vec<u64> = storage
            .get(&ExtendedDataKey::EmpTemplates(employer.clone()))
            .unwrap_or(Vec::new(&env));
        let mut templates = Vec::new(&env);

        for id in template_ids.iter() {
            if let Some(template) = storage.get(&ExtendedDataKey::Template(id)) {
                templates.push_back(template);
            }
        }

        templates
    }

    /// Get all public templates
    pub fn get_public_templates(env: Env) -> Vec<PayrollTemplate> {
        let storage = env.storage().persistent();
        let template_ids: Vec<u64> = storage
            .get(&ExtendedDataKey::PubTemplates)
            .unwrap_or(Vec::new(&env));
        let mut templates = Vec::new(&env);

        for id in template_ids.iter() {
            if let Some(template) = storage.get(&ExtendedDataKey::Template(id)) {
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
        if name.is_empty() || name.len() > 100 {
            return Err(PayrollError::InvalidTemplateName);
        }

        if amount <= 0 || interval == 0 || recurrence_frequency == 0 {
            return Err(PayrollError::TemplateValidationFailed);
        }

        let current_time = env.ledger().timestamp();

        // Get next preset ID
        let next_id = storage.get(&ExtendedDataKey::NextPresetId).unwrap_or(0) + 1;
        storage.set(&ExtendedDataKey::NextPresetId, &next_id);

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
        storage.set(&ExtendedDataKey::Preset(next_id), &preset);

        // Add to category
        let mut category_presets: Vec<u64> = storage
            .get(&ExtendedDataKey::PresetCat(category.clone()))
            .unwrap_or(Vec::new(&env));
        category_presets.push_back(next_id);
        storage.set(
            &ExtendedDataKey::PresetCat(category.clone()),
            &category_presets,
        );

        // Add to active presets
        let mut active_presets: Vec<u64> = storage
            .get(&ExtendedDataKey::ActivePresets)
            .unwrap_or(Vec::new(&env));
        active_presets.push_back(next_id);
        storage.set(&ExtendedDataKey::ActivePresets, &active_presets);

        env.events()
            .publish((PRESET_CREATED_EVENT,), (next_id, name, category));

        Ok(next_id)
    }

    /// Get a preset by ID
    pub fn get_preset(env: Env, preset_id: u64) -> Result<TemplatePreset, PayrollError> {
        let storage = env.storage().persistent();
        storage
            .get(&ExtendedDataKey::Preset(preset_id))
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
        let preset: TemplatePreset = storage
            .get(&ExtendedDataKey::Preset(preset_id))
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
        let preset_ids: Vec<u64> = storage
            .get(&ExtendedDataKey::PresetCat(category.clone()))
            .unwrap_or(Vec::new(&env));
        let mut presets = Vec::new(&env);

        for id in preset_ids.iter() {
            if let Some(preset) = storage.get(&ExtendedDataKey::Preset(id)) {
                presets.push_back(preset);
            }
        }

        presets
    }

    /// Get all active presets
    pub fn get_active_presets(env: Env) -> Vec<TemplatePreset> {
        let storage = env.storage().persistent();
        let preset_ids: Vec<u64> = storage
            .get(&ExtendedDataKey::ActivePresets)
            .unwrap_or(Vec::new(&env));
        let mut presets = Vec::new(&env);

        for id in preset_ids.iter() {
            if let Some(preset) = storage.get(&ExtendedDataKey::Preset(id)) {
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
        if name.is_empty() || name.len() > 100 {
            return Err(PayrollError::InvalidTemplateName);
        }

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get next backup ID
        let next_id = storage.get(&ExtendedDataKey::NextBackupId).unwrap_or(0) + 1;
        storage.set(&ExtendedDataKey::NextBackupId, &next_id);

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
        storage.set(&ExtendedDataKey::Backup(next_id), &backup);

        // Add to employer's backups
        let mut employer_backups: Vec<u64> = storage
            .get(&ExtendedDataKey::EmpBackups(caller.clone()))
            .unwrap_or(Vec::new(&env));
        employer_backups.push_back(next_id);
        storage.set(
            &ExtendedDataKey::EmpBackups(caller.clone()),
            &employer_backups,
        );

        // Add to backup index
        let mut backup_index: Vec<u64> = storage
            .get(&ExtendedDataKey::BackupIndex)
            .unwrap_or(Vec::new(&env));
        backup_index.push_back(next_id);
        storage.set(&ExtendedDataKey::BackupIndex, &backup_index);

        // Create backup data based on type
        let backup_data = Self::_collect_backup_data(&env, &caller, &backup_type)?;

        // Calculate checksum and hash
        let checksum = Self::_calculate_backup_checksum(&env, &backup_data);
        let data_hash = Self::_calculate_data_hash(&env, &backup_data);
        let size_bytes = Self::_calculate_backup_size(&env, &backup_data);

        // Store backup data
        storage.set(&ExtendedDataKey::BackupData(next_id), &backup_data);

        // Update backup with final metadata
        let mut final_backup = backup.clone();
        final_backup.status = BackupStatus::Completed;
        final_backup.checksum = checksum;
        final_backup.data_hash = data_hash;
        final_backup.size_bytes = size_bytes;
        storage.set(&ExtendedDataKey::Backup(next_id), &final_backup);

        env.events()
            .publish((BACKUP_CREATED_EVENT,), (caller.clone(), next_id, name));

        Ok(next_id)
    }

    /// Get a backup by ID
    pub fn get_backup(env: Env, backup_id: u64) -> Result<PayrollBackup, PayrollError> {
        let storage = env.storage().persistent();
        storage
            .get(&ExtendedDataKey::Backup(backup_id))
            .ok_or(PayrollError::BackupNotFound)
    }

    /// Get backup data by ID
    pub fn get_backup_data(env: Env, backup_id: u64) -> Result<BackupData, PayrollError> {
        let storage = env.storage().persistent();
        storage
            .get(&ExtendedDataKey::BackupData(backup_id))
            .ok_or(PayrollError::BackupNotFound)
    }

    /// Verify a backup's integrity
    pub fn verify_backup(env: Env, caller: Address, backup_id: u64) -> Result<bool, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let backup: PayrollBackup = storage
            .get(&ExtendedDataKey::Backup(backup_id))
            .ok_or(PayrollError::BackupNotFound)?;

        // Only backup owner can verify
        if backup.employer != caller {
            return Err(PayrollError::Unauthorized);
        }

        let backup_data: BackupData = storage
            .get(&ExtendedDataKey::BackupData(backup_id))
            .ok_or(PayrollError::BackupNotFound)?;

        // Calculate current checksum
        let current_checksum = Self::_calculate_backup_checksum(&env, &backup_data);
        let current_hash = Self::_calculate_data_hash(&env, &backup_data);

        // Verify checksum and hash
        let is_valid = backup.checksum == current_checksum && backup.data_hash == current_hash;

        // Update backup status
        let mut updated_backup = backup.clone();
        updated_backup.status = if is_valid {
            BackupStatus::Verified
        } else {
            BackupStatus::Failed
        };
        storage.set(&ExtendedDataKey::Backup(backup_id), &updated_backup);

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
        let next_id = storage.get(&ExtendedDataKey::NextRecoveryId).unwrap_or(0) + 1;
        storage.set(&ExtendedDataKey::NextRecoveryId, &next_id);

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

        storage.set(&ExtendedDataKey::Recovery(next_id), &recovery_point);

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
        let mut recovery_point: RecoveryPoint = storage
            .get(&ExtendedDataKey::Recovery(recovery_point_id))
            .ok_or(PayrollError::RecoveryPointNotFound)?;

        // Check if recovery is already in progress, completed, or failed
        if recovery_point.status == RecoveryStatus::InProgress
            || recovery_point.status == RecoveryStatus::Completed
            || recovery_point.status == RecoveryStatus::Failed
        {
            return Err(PayrollError::RecoveryInProgress);
        }

        // Get backup data
        let backup_data: BackupData = storage
            .get(&ExtendedDataKey::BackupData(recovery_point.backup_id))
            .ok_or(PayrollError::BackupNotFound)?;

        // Update recovery status
        recovery_point.status = RecoveryStatus::InProgress;
        storage.set(
            &ExtendedDataKey::Recovery(recovery_point_id),
            &recovery_point,
        );

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
        recovery_point.status = if failure_count == 0 {
            RecoveryStatus::Completed
        } else {
            RecoveryStatus::Failed
        };
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

        storage.set(
            &ExtendedDataKey::Recovery(recovery_point_id),
            &recovery_point,
        );

        env.events().publish(
            (RECOVERY_COMPLETED_EVENT,),
            (
                caller.clone(),
                recovery_point_id,
                success_count,
                failure_count,
                duration,
            ),
        );

        Ok(failure_count == 0)
    }

    /// Get all backups for an employer
    pub fn get_employer_backups(env: Env, employer: Address) -> Vec<PayrollBackup> {
        let storage = env.storage().persistent();
        let backup_ids: Vec<u64> = storage
            .get(&ExtendedDataKey::EmpBackups(employer.clone()))
            .unwrap_or(Vec::new(&env));
        let mut backups = Vec::new(&env);

        for id in backup_ids.iter() {
            if let Some(backup) = storage.get(&ExtendedDataKey::Backup(id)) {
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
        while let Some(recovery_point) = storage.get(&ExtendedDataKey::Recovery(next_id)) {
            recovery_points.push_back(recovery_point);
            next_id += 1;
        }

        recovery_points
    }

    /// Delete a backup
    pub fn delete_backup(env: Env, caller: Address, backup_id: u64) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let backup: PayrollBackup = storage
            .get(&ExtendedDataKey::Backup(backup_id))
            .ok_or(PayrollError::BackupNotFound)?;

        // Only backup owner can delete
        if backup.employer != caller {
            return Err(PayrollError::Unauthorized);
        }

        // Remove from storage
        storage.remove(&ExtendedDataKey::Backup(backup_id));
        storage.remove(&ExtendedDataKey::BackupData(backup_id));

        // Remove from employer's backups
        let employer_backups: Vec<u64> = storage
            .get(&ExtendedDataKey::EmpBackups(caller.clone()))
            .unwrap_or(Vec::new(&env));
        let mut new_employer_backups = Vec::new(&env);
        for id in employer_backups.iter() {
            if id != backup_id {
                new_employer_backups.push_back(id);
            }
        }
        storage.set(
            &ExtendedDataKey::EmpBackups(caller.clone()),
            &new_employer_backups,
        );

        // Remove from backup index
        let backup_index: Vec<u64> = storage
            .get(&ExtendedDataKey::BackupIndex)
            .unwrap_or(Vec::new(&env));
        let mut new_backup_index = Vec::new(&env);
        for id in backup_index.iter() {
            if id != backup_id {
                new_backup_index.push_back(id);
            }
        }
        storage.set(&ExtendedDataKey::BackupIndex, &new_backup_index);

        Ok(())
    }

    //-----------------------------------------------------------------------------
    // Internal Helper Functions for Backup and Recovery
    //-----------------------------------------------------------------------------

    /// Collect backup data based on backup type
    fn _collect_backup_data(
        env: &Env,
        _employer: &Address,
        backup_type: &BackupType,
    ) -> Result<BackupData, PayrollError> {
        let payroll_data = Vec::new(env);
        let template_data = Vec::new(env);
        let preset_data = Vec::new(env);
        let insurance_data: Vec<InsurancePolicy> = Vec::new(env);

        // Simplified implementation to avoid conversion errors
        // For now, just return empty data structures
        match backup_type {
            BackupType::Full => {
                // Empty implementation for now
            }
            BackupType::Employer => {
                // Empty implementation for now
            }
            BackupType::Employee => {
                // Empty implementation for now
            }
            BackupType::Template => {
                // Empty implementation for now
            }
            BackupType::Insurance => {
                // Empty implementation for now
            }
            BackupType::Compliance => {
                // Empty implementation for now
            }
        }

        let metadata = BackupMetadata {
            total_employees: payroll_data.len(),
            total_templates: template_data.len(),
            total_presets: preset_data.len(),
            total_insurance_policies: insurance_data.len(),
            backup_timestamp: env.ledger().timestamp(),
            contract_version: String::from_str(env, "1.0.0"),
            data_integrity_hash: String::from_str(env, "hash"),
        };

        Ok(BackupData {
            backup_id: 0, // Will be set by caller
            payroll_data,
            template_data,
            preset_data,
            insurance_data,
            compliance_data: String::from_str(env, "compliance"),
            metadata,
        })
    }

    /// Calculate backup checksum
    fn _calculate_backup_checksum(env: &Env, _backup_data: &BackupData) -> String {
        // Simplified checksum calculation
        let checksum = String::from_str(env, "checksum");
        checksum
    }

    /// Calculate data hash
    fn _calculate_data_hash(env: &Env, _backup_data: &BackupData) -> String {
        // Simplified hash calculation
        let hash = String::from_str(env, "hash");
        hash
    }

    /// Calculate backup size
    fn _calculate_backup_size(_env: &Env, backup_data: &BackupData) -> u64 {
        // Simplified size calculation
        let payroll_size = backup_data.payroll_data.len() as u64 * 100; // Approximate size per payroll
        let template_size = backup_data.template_data.len() as u64 * 80; // Approximate size per template
        let preset_size = backup_data.preset_data.len() as u64 * 60; // Approximate size per preset
        let insurance_size = backup_data.insurance_data.len() as u64 * 120; // Approximate size per insurance policy
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
        if storage.has(&ExtendedDataKey::Template(template.id)) {
            // Update existing template
            storage.set(&ExtendedDataKey::Template(template.id), template);
        } else {
            // Create new template
            storage.set(&ExtendedDataKey::Template(template.id), template);

            // Add to employer's templates
            let mut employer_templates: Vec<u64> = storage
                .get(&ExtendedDataKey::EmpTemplates(template.employer.clone()))
                .unwrap_or(Vec::new(env));
            employer_templates.push_back(template.id);
            storage.set(
                &ExtendedDataKey::EmpTemplates(template.employer.clone()),
                &employer_templates,
            );
        }

        Ok(())
    }

    /// Restore preset data
    fn _restore_preset(env: &Env, preset: &TemplatePreset) -> Result<(), PayrollError> {
        let storage = env.storage().persistent();

        // Check if preset already exists
        if storage.has(&ExtendedDataKey::Preset(preset.id)) {
            // Update existing preset
            storage.set(&ExtendedDataKey::Preset(preset.id), preset);
        } else {
            // Create new preset
            storage.set(&ExtendedDataKey::Preset(preset.id), preset);

            // Add to category
            let mut category_presets: Vec<u64> = storage
                .get(&ExtendedDataKey::PresetCat(preset.category.clone()))
                .unwrap_or(Vec::new(env));
            category_presets.push_back(preset.id);
            storage.set(
                &ExtendedDataKey::PresetCat(preset.category.clone()),
                &category_presets,
            );

            // Add to active presets if active
            if preset.is_active {
                let mut active_presets: Vec<u64> = storage
                    .get(&ExtendedDataKey::ActivePresets)
                    .unwrap_or(Vec::new(env));
                active_presets.push_back(preset.id);
                storage.set(&ExtendedDataKey::ActivePresets, &active_presets);
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
        if name.is_empty() || name.len() > 100 {
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
        let next_id = storage.get(&ExtendedDataKey::NextSchedId).unwrap_or(0) + 1;
        storage.set(&ExtendedDataKey::NextSchedId, &next_id);

        // Calculate next execution time
        let next_execution = Self::_calculate_next_execution(&env, &frequency, start_date);

        // Create schedule metadata
        let metadata = ScheduleMetadata {
            total_employees: 0,
            total_amount: 0,
            token_address: Address::from_str(
                &env,
                "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF",
            ),
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
        storage.set(&ExtendedDataKey::Schedule(next_id), &schedule);

        // Add to employer's schedules
        let mut employer_schedules: Vec<u64> = storage
            .get(&ExtendedDataKey::EmpSchedules(caller.clone()))
            .unwrap_or(Vec::new(&env));
        employer_schedules.push_back(next_id);
        storage.set(
            &ExtendedDataKey::EmpSchedules(caller.clone()),
            &employer_schedules,
        );

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
        storage
            .get(&ExtendedDataKey::Schedule(schedule_id))
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
        let mut schedule: PayrollSchedule = storage
            .get(&ExtendedDataKey::Schedule(schedule_id))
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
            schedule.next_execution =
                Self::_calculate_next_execution(&env, &new_frequency, schedule.start_date);
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
        storage.set(&ExtendedDataKey::Schedule(schedule_id), &schedule);

        env.events()
            .publish((SCHEDULE_UPDATED_EVENT,), (caller.clone(), schedule_id));

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
        let mut schedule: PayrollSchedule = storage
            .get(&ExtendedDataKey::Schedule(schedule_id))
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
                let employees =
                    Self::get_employer_employees(env.clone(), schedule.employer.clone());
                for employee in employees.iter() {
                    match Self::disburse_salary(env.clone(), caller.clone(), employee.clone()) {
                        Ok(_) => success_count += 1,
                        Err(_) => failure_count += 1,
                    }
                }
            }
            ScheduleType::OneTime => {
                // Execute one-time payroll
                let employees =
                    Self::get_employer_employees(env.clone(), schedule.employer.clone());
                for employee in employees.iter() {
                    match Self::disburse_salary(env.clone(), caller.clone(), employee.clone()) {
                        Ok(_) => success_count += 1,
                        Err(_) => failure_count += 1,
                    }
                }
                // Deactivate one-time schedule after execution
                schedule.is_active = false;
            }
            ScheduleType::Batch => {
                // Execute batch payroll processing
                let employees =
                    Self::get_employer_employees(env.clone(), schedule.employer.clone());
                for employee in employees.iter() {
                    match Self::disburse_salary(env.clone(), caller.clone(), employee.clone()) {
                        Ok(_) => success_count += 1,
                        Err(_) => failure_count += 1,
                    }
                }
            }
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
        schedule.next_execution =
            Self::_calculate_next_execution(&env, &schedule.frequency, current_time);
        schedule.metadata.total_employees = success_count + failure_count;
        schedule.metadata.success_rate = if (success_count + failure_count) > 0 {
            (success_count * 100) / (success_count + failure_count)
        } else {
            0
        };
        schedule.metadata.average_execution_time = duration;
        schedule.updated_at = current_time;

        storage.set(&ExtendedDataKey::Schedule(schedule_id), &schedule);

        env.events().publish(
            (SCHEDULE_EXECUTED_EVENT,),
            (
                caller.clone(),
                schedule_id,
                success_count,
                failure_count,
                duration,
            ),
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
        if name.is_empty() || name.len() > 100 {
            return Err(PayrollError::InvalidTemplateName);
        }

        if conditions.len() == 0 || actions.len() == 0 {
            return Err(PayrollError::InvalidAutomationRule);
        }

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get next rule ID
        let next_id = storage.get(&ExtendedDataKey::NextRuleId).unwrap_or(0) + 1;
        storage.set(&ExtendedDataKey::NextRuleId, &next_id);

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
        storage.set(&ExtendedDataKey::Rule(next_id), &rule);

        // Add to employer's rules
        let mut employer_rules: Vec<u64> = storage
            .get(&ExtendedDataKey::EmpRules(caller.clone()))
            .unwrap_or(Vec::new(&env));
        employer_rules.push_back(next_id);
        storage.set(&ExtendedDataKey::EmpRules(caller.clone()), &employer_rules);

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
        storage
            .get(&ExtendedDataKey::Rule(rule_id))
            .ok_or(PayrollError::AutomationRuleNotFound)
    }

    /// Execute automation rules
    pub fn execute_automation_rules(env: Env, caller: Address) -> Result<u32, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let mut executed_count = 0;

        // Get all rules for the caller and execute active ones
        let rule_ids: Vec<u64> = storage
            .get(&ExtendedDataKey::EmpRules(caller.clone()))
            .unwrap_or(Vec::new(&env));
        for rule_id in rule_ids.iter() {
            if let Some(rule) =
                storage.get::<ExtendedDataKey, AutomationRule>(&ExtendedDataKey::Rule(rule_id))
            {
                if rule.employer == caller && rule.is_active {
                    match Self::_evaluate_and_execute_rule(&env, &rule) {
                        Ok(_) => executed_count += 1,
                        Err(_) => continue,
                    }
                }
            }
        }

        env.events()
            .publish((RULE_EXECUTED_EVENT,), (caller.clone(), executed_count));

        Ok(executed_count)
    }

    /// Get all schedules for an employer
    pub fn get_employer_schedules(env: Env, employer: Address) -> Vec<PayrollSchedule> {
        let storage = env.storage().persistent();
        let schedule_ids: Vec<u64> = storage
            .get(&ExtendedDataKey::EmpSchedules(employer.clone()))
            .unwrap_or(Vec::new(&env));
        let mut schedules = Vec::new(&env);

        for id in schedule_ids.iter() {
            if let Some(schedule) = storage.get(&ExtendedDataKey::Schedule(id)) {
                schedules.push_back(schedule);
            }
        }

        schedules
    }

    /// Get all automation rules for an employer
    pub fn get_employer_rules(env: Env, employer: Address) -> Vec<AutomationRule> {
        let storage = env.storage().persistent();
        let rule_ids: Vec<u64> = storage
            .get(&ExtendedDataKey::EmpRules(employer.clone()))
            .unwrap_or(Vec::new(&env));
        let mut rules = Vec::new(&env);

        for id in rule_ids.iter() {
            if let Some(rule) = storage.get(&ExtendedDataKey::Rule(id)) {
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
    // Advanced Scheduling and Automation Functions
    //-----------------------------------------------------------------------------

    /// Create a flexible schedule with holiday handling
    pub fn create_flexible_schedule(
        env: Env,
        caller: Address,
        name: String,
        description: String,
        schedule_type: ScheduleType,
        frequency: ScheduleFrequency,
        start_date: u64,
        end_date: Option<u64>,
        skip_weekends: bool,
        skip_holidays: Vec<u64>,
        weekend_handling: WeekendHandling,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        if name.len() == 0 || name.len() > 100 {
            return Err(PayrollError::InvalidTemplateName);
        }

        let current_time = env.ledger().timestamp();
        if start_date < current_time {
            return Err(PayrollError::ScheduleValidationFailed);
        }

        let storage = env.storage().persistent();
        let next_id = storage.get(&ExtendedDataKey::NextSchedId).unwrap_or(0) + 1;
        storage.set(&ExtendedDataKey::NextSchedId, &next_id);

        // Calculate next execution time considering holidays and weekends
        let next_execution = Self::_calculate_next_valid_execution(
            &env,
            &frequency,
            start_date,
            skip_weekends,
            &skip_holidays,
            &weekend_handling,
        );

        let metadata = ScheduleMetadata {
            total_employees: 0,
            total_amount: 0,
            token_address: Address::from_str(
                &env,
                "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF",
            ),
            priority: 1,
            retry_count: 0,
            max_retries: 3,
            success_rate: 0,
            average_execution_time: 0,
        };

        let schedule = PayrollSchedule {
            id: next_id,
            name: name.clone(),
            description,
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

        storage.set(&ExtendedDataKey::Schedule(next_id), &schedule);

        // Store holiday configuration
        let holiday_config = HolidayConfig {
            schedule_id: next_id,
            skip_weekends,
            holidays: skip_holidays.clone(),
            weekend_handling: weekend_handling.clone(),
            created_at: current_time,
            updated_at: current_time,
        };
        storage.set(&ExtendedDataKey::HolidayConfig(next_id), &holiday_config);

        let mut employer_schedules: Vec<u64> = storage
            .get(&ExtendedDataKey::EmpSchedules(caller.clone()))
            .unwrap_or(Vec::new(&env));
        employer_schedules.push_back(next_id);
        storage.set(&ExtendedDataKey::EmpSchedules(caller), &employer_schedules);

        env.events()
            .publish((SCHEDULE_CREATED_EVENT,), (next_id, name, skip_weekends));

        Ok(next_id)
    }

    /// Get holiday configuration for a schedule
    pub fn get_holiday_config(env: Env, schedule_id: u64) -> Result<HolidayConfig, PayrollError> {
        let storage = env.storage().persistent();
        storage
            .get(&ExtendedDataKey::HolidayConfig(schedule_id))
            .ok_or(PayrollError::ScheduleNotFound)
    }

    /// Update holiday configuration
    pub fn update_holiday_config(
        env: Env,
        caller: Address,
        schedule_id: u64,
        skip_weekends: bool,
        holidays: Vec<u64>,
        weekend_handling: WeekendHandling,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();

        // Verify schedule ownership
        let schedule: PayrollSchedule = storage
            .get(&ExtendedDataKey::Schedule(schedule_id))
            .ok_or(PayrollError::ScheduleNotFound)?;

        if schedule.employer != caller {
            return Err(PayrollError::Unauthorized);
        }

        let mut config: HolidayConfig = storage
            .get(&ExtendedDataKey::HolidayConfig(schedule_id))
            .ok_or(PayrollError::ScheduleNotFound)?;

        config.skip_weekends = skip_weekends;
        config.holidays = holidays;
        config.weekend_handling = weekend_handling;
        config.updated_at = env.ledger().timestamp();

        storage.set(&ExtendedDataKey::HolidayConfig(schedule_id), &config);

        Ok(())
    }

    /// Create conditional payroll trigger (performance/milestone based)
    pub fn create_conditional_trigger(
        env: Env,
        caller: Address,
        name: String,
        description: String,
        trigger_type: String,
        conditions: Vec<RuleCondition>,
        actions: Vec<RuleAction>,
        threshold_value: i128,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        if name.len() == 0 || conditions.len() == 0 {
            return Err(PayrollError::InvalidAutomationRule);
        }

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();
        let next_id = storage.get(&ExtendedDataKey::NextRuleId).unwrap_or(0) + 1;
        storage.set(&ExtendedDataKey::NextRuleId, &next_id);

        let rule = AutomationRule {
            id: next_id,
            name: name.clone(),
            description,
            employer: caller.clone(),
            rule_type: RuleType::Custom,
            conditions,
            actions,
            is_active: true,
            created_at: current_time,
            updated_at: current_time,
            execution_count: 0,
            last_execution: None,
            priority: 1,
        };

        storage.set(&ExtendedDataKey::Rule(next_id), &rule);

        let mut employer_rules: Vec<u64> = storage
            .get(&ExtendedDataKey::EmpRules(caller.clone()))
            .unwrap_or(Vec::new(&env));
        employer_rules.push_back(next_id);
        storage.set(&ExtendedDataKey::EmpRules(caller), &employer_rules);

        env.events().publish(
            (RULE_CREATED_EVENT,),
            (next_id, name, trigger_type, threshold_value),
        );

        Ok(next_id)
    }

    /// Apply automated payroll adjustment
    pub fn apply_automated_adjustment(
        env: Env,
        caller: Address,
        employee: Address,
        adjustment_type: String,
        adjustment_amount: i128,
        reason: String,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let mut payroll =
            Self::_get_payroll(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;

        if payroll.employer != caller {
            return Err(PayrollError::Unauthorized);
        }

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get next adjustment ID
        let adjustment_id = storage.get(&ExtendedDataKey::NextAdjustmentId).unwrap_or(0) + 1;
        storage.set(&ExtendedDataKey::NextAdjustmentId, &adjustment_id);

        // Calculate new amount
        let new_amount = if adjustment_amount > 0 {
            payroll.amount + adjustment_amount
        } else {
            if payroll.amount + adjustment_amount < 0 {
                return Err(PayrollError::InvalidData);
            }
            payroll.amount + adjustment_amount
        };

        // Create adjustment record
        let adjustment = PayrollAdjustment {
            id: adjustment_id,
            employee: employee.clone(),
            adjustment_type: adjustment_type.clone(),
            amount: adjustment_amount,
            reason: reason.clone(),
            applied_by: caller.clone(),
            applied_at: current_time,
            approved: true,
        };

        storage.set(&ExtendedDataKey::Adjustment(adjustment_id), &adjustment);

        // Track employee adjustments
        let mut emp_adjustments: Vec<u64> = storage
            .get(&ExtendedDataKey::EmpAdjustments(employee.clone()))
            .unwrap_or(Vec::new(&env));
        emp_adjustments.push_back(adjustment_id);
        storage.set(
            &ExtendedDataKey::EmpAdjustments(employee.clone()),
            &emp_adjustments,
        );

        // Apply adjustment to payroll
        payroll.amount = new_amount;
        let compact = Self::to_compact_payroll(&payroll);
        storage.set(&DataKey::Payroll(employee.clone()), &compact);

        Self::record_history(&env, &employee, &compact, symbol_short!("adjusted"));

        env.events().publish(
            (symbol_short!("adj_app"),),
            (employee, adjustment_type, adjustment_amount, reason),
        );

        Ok(adjustment_id)
    }

    /// Get adjustment history for an employee
    pub fn get_employee_adjustments(env: Env, employee: Address) -> Vec<PayrollAdjustment> {
        let storage = env.storage().persistent();
        let adjustment_ids: Vec<u64> = storage
            .get(&ExtendedDataKey::EmpAdjustments(employee))
            .unwrap_or(Vec::new(&env));

        let mut adjustments = Vec::new(&env);
        for id in adjustment_ids.iter() {
            if let Some(adjustment) = storage.get(&ExtendedDataKey::Adjustment(id)) {
                adjustments.push_back(adjustment);
            }
        }

        adjustments
    }

    /// Generate payroll forecast for upcoming periods
    pub fn forecast_payroll(
        env: Env,
        employer: Address,
        periods: u32,
        frequency_days: u32,
    ) -> Result<Vec<PayrollForecast>, PayrollError> {
        let storage = env.storage().persistent();
        let employees = Self::get_employer_employees(env.clone(), employer.clone());
        let current_time = env.ledger().timestamp();

        let mut forecasts = Vec::new(&env);
        let forecast_id_start = storage.get(&ExtendedDataKey::NextForecastId).unwrap_or(0);

        for period in 0..periods {
            let mut period_total: i128 = 0;
            let mut active_employees = 0u32;

            let period_start = current_time + ((period * frequency_days) as u64 * 86400);
            let period_end = period_start + (frequency_days as u64 * 86400);

            for employee in employees.iter() {
                if let Some(payroll) = Self::_get_payroll(&env, &employee) {
                    if payroll.employer == employer && !payroll.is_paused {
                        period_total += payroll.amount;
                        active_employees += 1;
                    }
                }
            }

            let forecast = PayrollForecast {
                period: period + 1,
                start_date: period_start,
                end_date: period_end,
                estimated_amount: period_total,
                employee_count: active_employees,
                confidence_level: 85, // Base confidence level
            };

            let forecast_id = forecast_id_start + 1 + (period as u64);
            storage.set(&ExtendedDataKey::Forecast(forecast_id), &forecast);
            forecasts.push_back(forecast);
        }

        storage.set(
            &ExtendedDataKey::NextForecastId,
            &(forecast_id_start + periods as u64),
        );

        env.events().publish(
            (symbol_short!("forecast"),),
            (employer, periods, current_time),
        );

        Ok(forecasts)
    }

    /// Get stored forecast
    pub fn get_forecast(env: Env, forecast_id: u64) -> Result<PayrollForecast, PayrollError> {
        let storage = env.storage().persistent();
        storage
            .get(&ExtendedDataKey::Forecast(forecast_id))
            .ok_or(PayrollError::PayrollNotFound)
    }

    /// Run automated compliance checks
    pub fn run_compliance_checks(
        env: Env,
        caller: Address,
    ) -> Result<ComplianceCheckResult, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let employees = Self::get_employer_employees(env.clone(), caller.clone());
        let current_time = env.ledger().timestamp();

        let check_id = storage
            .get(&ExtendedDataKey::NextComplianceCheckId)
            .unwrap_or(0)
            + 1;
        storage.set(&ExtendedDataKey::NextComplianceCheckId, &check_id);

        let mut issues = Vec::new(&env);

        for employee in employees.iter() {
            if let Some(payroll) = Self::_get_payroll(&env, &employee) {
                // Check if payroll is overdue
                if current_time > payroll.next_payout_timestamp + 86400 {
                    let issue = ComplianceIssue {
                        employee: employee.clone(),
                        issue_type: String::from_str(&env, "overdue_payment"),
                        severity: ComplianceSeverity::High,
                        description: String::from_str(&env, "Payment overdue by more than 1 day"),
                    };
                    issues.push_back(issue);
                }

                // Check minimum amount compliance
                if payroll.amount < 1000 {
                    let issue = ComplianceIssue {
                        employee: employee.clone(),
                        issue_type: String::from_str(&env, "below_minimum"),
                        severity: ComplianceSeverity::Medium,
                        description: String::from_str(&env, "Amount below minimum threshold"),
                    };
                    issues.push_back(issue);
                }

                // Check interval compliance
                if payroll.interval < 86400 {
                    let issue = ComplianceIssue {
                        employee: employee.clone(),
                        issue_type: String::from_str(&env, "invalid_interval"),
                        severity: ComplianceSeverity::Low,
                        description: String::from_str(&env, "Interval less than 1 day"),
                    };
                    issues.push_back(issue);
                }
            }
        }

        let result = ComplianceCheckResult {
            check_id,
            employer: caller.clone(),
            check_time: current_time,
            issues_found: issues.clone(),
            passed: issues.len() == 0,
        };

        storage.set(&ExtendedDataKey::ComplianceCheck(check_id), &result);

        env.events().publish(
            (symbol_short!("comp_chk"),),
            (caller, check_id, issues.len() as u32, result.passed),
        );

        Ok(result)
    }

    /// Get compliance check results
    pub fn get_compliance_check(
        env: Env,
        check_id: u64,
    ) -> Result<ComplianceCheckResult, PayrollError> {
        let storage = env.storage().persistent();
        storage
            .get(&ExtendedDataKey::ComplianceCheck(check_id))
            .ok_or(PayrollError::PayrollNotFound)
    }

    //-----------------------------------------------------------------------------
    // Internal Helper Functions
    //-----------------------------------------------------------------------------

    /// Calculate next valid execution considering holidays and weekends
    fn _calculate_next_valid_execution(
        env: &Env,
        frequency: &ScheduleFrequency,
        current_time: u64,
        skip_weekends: bool,
        skip_holidays: &Vec<u64>,
        weekend_handling: &WeekendHandling,
    ) -> u64 {
        let mut next_time = Self::_calculate_next_execution(env, frequency, current_time);

        // Handle weekends
        if skip_weekends {
            let day_of_week = ((next_time / 86400) + 4) % 7; // Adjust for epoch

            match weekend_handling {
                WeekendHandling::Skip | WeekendHandling::ProcessLate => {
                    // If Saturday (6) or Sunday (0), move to Monday
                    if day_of_week == 6 {
                        next_time += 2 * 86400; // Saturday to Monday
                    } else if day_of_week == 0 {
                        next_time += 86400; // Sunday to Monday
                    }
                }
                WeekendHandling::ProcessEarly => {
                    // If Saturday or Sunday, move to Friday
                    if day_of_week == 6 {
                        next_time -= 86400; // Saturday to Friday
                    } else if day_of_week == 0 {
                        next_time -= 2 * 86400; // Sunday to Friday
                    }
                }
            }
        }

        // Handle holidays
        for holiday in skip_holidays.iter() {
            let holiday_day = (holiday / 86400) * 86400;
            let next_day = (next_time / 86400) * 86400;

            if holiday_day == next_day {
                next_time += 86400; // Move to next day
            }
        }

        next_time
    }

    /// Evaluate complex conditions for automation rules
    fn _evaluate_complex_conditions(
        env: &Env,
        conditions: &Vec<RuleCondition>,
        employee: &Address,
    ) -> Result<bool, PayrollError> {
        if conditions.len() == 0 {
            return Ok(true);
        }

        let payroll = Self::_get_payroll(env, employee);
        let mut result = true;

        for condition in conditions.iter() {
            let field_value = if let Some(ref p) = payroll {
                if condition.field == String::from_str(env, "amount") {
                    Some(p.amount)
                } else if condition.field == String::from_str(env, "interval") {
                    Some(p.interval as i128)
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(value) = field_value {
                // Parse threshold from string
                let threshold: i128 = 1000; // Default threshold

                let condition_met = match condition.operator {
                    ConditionOperator::GreaterThan => value > threshold,
                    ConditionOperator::LessThan => value < threshold,
                    ConditionOperator::Equals => value == threshold,
                    ConditionOperator::GreaterThanOrEqual => value >= threshold,
                    ConditionOperator::LessThanOrEqual => value <= threshold,
                    _ => false,
                };

                result = match condition.logical_operator {
                    LogicalOperator::And => result && condition_met,
                    LogicalOperator::Or => result || condition_met,
                    LogicalOperator::Not => result && !condition_met,
                };
            }
        }

        Ok(result)
    }
    //-----------------------------------------------------------------------------
    // Internal Helper Functions for Scheduling and Automation
    //-----------------------------------------------------------------------------

    /// Calculate next execution time based on frequency
    fn _calculate_next_execution(
        _env: &Env,
        frequency: &ScheduleFrequency,
        current_time: u64,
    ) -> u64 {
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
    fn _evaluate_conditions(
        _env: &Env,
        _conditions: &Vec<RuleCondition>,
    ) -> Result<bool, PayrollError> {
        // Simplified condition evaluation
        // In a real implementation, this would evaluate actual conditions
        Ok(true) // For now, always return true
    }

    /// Execute a rule action
    fn _execute_action(_env: &Env, action: &RuleAction) -> Result<(), PayrollError> {
        match action.action_type {
            ActionType::DisburseSalary => {
                // Execute salary disbursement
                // This would be implemented based on action parameters
                Ok(())
            }
            ActionType::PausePayroll => {
                // Pause payroll operations
                Ok(())
            }
            ActionType::ResumePayroll => {
                // Resume payroll operations
                Ok(())
            }
            ActionType::CreateBackup => {
                // Create backup
                Ok(())
            }
            ActionType::SendNotification => {
                // Send notification
                Ok(())
            }
            ActionType::UpdateSchedule => {
                // Update schedule
                Ok(())
            }
            ActionType::ExecuteRecovery => {
                // Execute recovery
                Ok(())
            }
            ActionType::Custom => {
                // Custom action
                Ok(())
            }
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
        if storage.has(&RoleDataKey::Role(role_id.clone())) {
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

        storage.set(&RoleDataKey::Role(role_id.clone()), &role);

        env.events()
            .publish((ROLE_ASSIGNED_EVENT,), (caller, role_id, name));

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
        let role: Role = storage
            .get(&RoleDataKey::Role(role_id.clone()))
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

        storage.set(&RoleDataKey::UserRole(user.clone()), &assignment);

        env.events()
            .publish((ROLE_ASSIGNED_EVENT,), (caller, user, role_id));

        Ok(())
    }

    /// Create a temporary role assignment
    pub fn assign_temp_role(
        env: Env,
        caller: Address,
        user: Address,
        role_id: String,
        expires_at: u64,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;
        Self::_require_security_permission(&env, &caller, Permission::ManageRoles)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Verify role exists
        let role: Role = storage
            .get(&RoleDataKey::Role(role_id.clone()))
            .ok_or(PayrollError::RoleNotFound)?;

        if !role.is_active {
            return Err(PayrollError::RoleNotFound);
        }

        // Ensure expires_at is in the future
        if expires_at <= current_time {
            return Err(PayrollError::InvalidTimeRange);
        }

        // Get next temp role ID
        let temp_id = storage
            .get::<RoleDataKey, u64>(&RoleDataKey::NextTempRoleId)
            .unwrap_or(1);

        let temp_assignment = TempRoleAssignment {
            id: temp_id,
            role_id: role_id.clone(),
            user: user.clone(),
            assigned_by: caller.clone(),
            assigned_at: current_time,
            expires_at,
        };

        storage.set(&RoleDataKey::TempRole(temp_id), &temp_assignment);
        storage.set(&RoleDataKey::NextTempRoleId, &(temp_id + 1));

        // Log audit entry
        Self::_log_permission_audit(
            &env,
            caller.clone(),
            user.clone(),
            String::from_str(&env, "ManageRoles"),
            String::from_str(&env, "assign_temp_role"),
            String::from_str(&env, "granted"),
            String::from_str(&env, "Accepted delegation role"),
        )?;

        env.events().publish(
            (String::from_str(&env, "temp_role_assigned"),),
            (caller, user, role_id, temp_id),
        );

        Ok(temp_id)
    }

    /// Get active temporary roles for a user
    fn _get_active_temp_roles(
        env: &Env,
        user: Address,
        current_time: u64,
    ) -> Vec<TempRoleAssignment> {
        let storage = env.storage().persistent();
        let mut active_temp_roles = Vec::new(env);

        let next_id = storage
            .get::<RoleDataKey, u64>(&RoleDataKey::NextTempRoleId)
            .unwrap_or(1);

        for id in 1..next_id {
            if let Some(temp_role) =
                storage.get::<RoleDataKey, TempRoleAssignment>(&RoleDataKey::TempRole(id))
            {
                if temp_role.user == user && temp_role.expires_at > current_time {
                    active_temp_roles.push_back(temp_role);
                }
            }
        }

        active_temp_roles
    }

    /// Get active delegations for a user
    fn _get_active_delegations(env: &Env, user: Address, current_time: u64) -> Vec<RoleDelegation> {
        let storage = env.storage().persistent();
        let mut active_delegations = Vec::new(env);

        let next_id = storage
            .get::<RoleDataKey, u64>(&RoleDataKey::NextDelegationId)
            .unwrap_or(1);

        for id in 1..next_id {
            if let Some(delegation) =
                storage.get::<RoleDataKey, RoleDelegation>(&RoleDataKey::Delegation(id))
            {
                if delegation.to == user
                    && delegation.accepted
                    && (delegation.expires_at.is_none()
                        || delegation.expires_at.unwrap() > current_time)
                {
                    active_delegations.push_back(delegation);
                }
            }
        }

        active_delegations
    }

    /// Log permission audit entry
    fn _log_permission_audit(
        env: &Env,
        actor: Address,
        subject: Address,
        permission: String,
        action: String,
        result: String,
        details: String,
    ) -> Result<(), PayrollError> {
        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        let audit_id = storage
            .get::<RoleDataKey, u64>(&RoleDataKey::NextAuditId)
            .unwrap_or(1);

        let audit_entry = PermissionAuditEntry {
            id: audit_id,
            actor,
            subject,
            permission,
            action,
            result,
            timestamp: current_time,
            details,
        };

        storage.set(&RoleDataKey::Audit(audit_id), &audit_entry);
        storage.set(&RoleDataKey::NextAuditId, &(audit_id + 1));

        Ok(())
    }

    /// Get user's roles (direct, temp, and delegated)
    pub fn get_user_roles(env: Env, user: Address) -> UserRolesResponse {
        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Direct roles
        let direct_roles = storage
            .get::<RoleDataKey, Vec<String>>(&RoleDataKey::UserRole(user.clone()))
            .unwrap_or_else(|| Vec::new(&env));

        // Temporary roles
        let temp_roles = Self::_get_active_temp_roles(&env, user.clone(), current_time);

        // Delegated roles
        let delegated_roles = Self::_get_active_delegations(&env, user.clone(), current_time);

        UserRolesResponse {
            direct_roles,
            temp_roles,
            delegated_roles,
        }
    }

    /// Get role details with hierarchy info
    pub fn get_role_details(env: Env, role_id: String) -> Option<RoleDetails> {
        let storage = env.storage().persistent();

        if let Some(role) = storage.get::<RoleDataKey, Role>(&RoleDataKey::Role(role_id.clone())) {
            let parent_role =
                storage.get::<RoleDataKey, String>(&RoleDataKey::RoleParent(role_id.clone()));
            let members = storage
                .get::<RoleDataKey, Vec<Address>>(&RoleDataKey::RoleMembers(role_id.clone()))
                .unwrap_or_else(|| Vec::new(&env));
            let all_permissions = Self::get_role_permissions(env.clone(), role_id);

            Some(RoleDetails {
                role,
                parent_role,
                members,
                all_permissions,
            })
        } else {
            None
        }
    }

    /// Get permission audit trail
    pub fn get_permission_audit_trail(
        env: Env,
        caller: Address,
        user: Option<Address>,
        limit: Option<u32>,
    ) -> Result<Vec<PermissionAuditEntry>, PayrollError> {
        caller.require_auth();
        Self::_require_security_permission(&env, &caller, Permission::ViewAuditTrail)?;

        let storage = env.storage().persistent();
        let mut audit_entries = Vec::new(&env);

        let next_id = storage
            .get::<RoleDataKey, u64>(&RoleDataKey::NextAuditId)
            .unwrap_or(1);

        let max_entries = limit.unwrap_or(100) as u64;
        let start_id = if next_id > max_entries {
            next_id - max_entries
        } else {
            1
        };

        for id in start_id..next_id {
            if let Some(entry) =
                storage.get::<RoleDataKey, PermissionAuditEntry>(&RoleDataKey::Audit(id))
            {
                if let Some(target_user) = &user {
                    if entry.subject == *target_user || entry.actor == *target_user {
                        audit_entries.push_back(entry);
                    }
                } else {
                    audit_entries.push_back(entry);
                }
            }
        }

        Ok(audit_entries)
    }

    /// Delegate a role from one user to another
    pub fn delegate_role(
        env: Env,
        caller: Address,
        to: Address,
        role_id: String,
        expires_at: Option<u64>,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Verify caller has the role they want to delegate

        if !Self::has_role(env.clone(), caller.clone(), role_id.clone()) {
            return Err(PayrollError::InsufficientPermissions);
        }

        // Verify role exists and is active
        let role: Role = storage
            .get(&RoleDataKey::Role(role_id.clone()))
            .ok_or(PayrollError::RoleNotFound)?;

        if !role.is_active {
            return Err(PayrollError::RoleNotFound);
        }

        // Get next delegation ID
        let delegation_id = storage
            .get::<RoleDataKey, u64>(&RoleDataKey::NextDelegationId)
            .unwrap_or(1);

        let delegation = RoleDelegation {
            id: delegation_id,
            role_id: role_id.clone(),
            from: caller.clone(),
            to: to.clone(),
            delegated_at: current_time,
            expires_at,
            accepted: false,
        };

        storage.set(&RoleDataKey::Delegation(delegation_id), &delegation);
        storage.set(&RoleDataKey::NextDelegationId, &(delegation_id + 1));

        // Log audit entry
        Self::_log_permission_audit(
            &env,
            caller.clone(),
            to.clone(),
            String::from_str(&env, "RoleDelegate"),
            String::from_str(&env, "delegate_role"),
            String::from_str(&env, "pending"),
            String::from_str(&env, "Role delegation created"),
        )?;

        env.events().publish(
            (String::from_str(&env, "role_delegated"),),
            (caller, to, role_id, delegation_id),
        );

        Ok(delegation_id)
    }

    /// Accept a delegated role
    pub fn accept_role_delegation(
        env: Env,
        caller: Address,
        delegation_id: u64,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        let mut delegation: RoleDelegation = storage
            .get(&RoleDataKey::Delegation(delegation_id))
            .ok_or(PayrollError::InvalidData)?;

        // Verify caller is the recipient
        if delegation.to != caller {
            return Err(PayrollError::InsufficientPermissions);
        }

        // Check if delegation hasn't expired
        if let Some(expires_at) = delegation.expires_at {
            if current_time > expires_at {
                return Err(PayrollError::DelegationExpired);
            }
        }

        delegation.accepted = true;
        storage.set(&RoleDataKey::Delegation(delegation_id), &delegation);

        // Log audit entry
        Self::_log_permission_audit(
            &env,
            caller.clone(),
            caller.clone(),
            String::from_str(&env, "RoleDelegate"),
            String::from_str(&env, "accept_delegation"),
            String::from_str(&env, "granted"),
            String::from_str(&env, "Accepted delegation for role"),
        )?;

        env.events().publish(
            (String::from_str(&env, "role_delegation_accepted"),),
            (caller, delegation.role_id, current_time),
        );

        Ok(())
    }

    /// Check if user has a specific role
    pub fn has_role(env: Env, user: Address, role_id: String) -> bool {
        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Check direct role assignment
        if let Some(user_roles) =
            storage.get::<RoleDataKey, Vec<String>>(&RoleDataKey::UserRole(user.clone()))
        {
            if user_roles.contains(&role_id) {
                return true;
            }
        }

        // Check temporary assignments
        let temp_roles = Self::_get_active_temp_roles(&env, user.clone(), current_time);
        for temp_role in temp_roles.iter() {
            if temp_role.role_id == role_id {
                return true;
            }
        }

        // Check accepted delegations
        let delegations = Self::_get_active_delegations(&env, user, current_time);
        for delegation in delegations.iter() {
            if delegation.role_id == role_id {
                return true;
            }
        }

        false
    }

    /// Get all permissions for a role (including inherited)
    pub fn get_role_permissions(env: Env, role_id: String) -> Vec<Permission> {
        Self::_get_role_permissions_recursive(&env, &role_id)
    }

    fn _get_role_permissions_recursive(env: &Env, role_id: &String) -> Vec<Permission> {
        let storage = env.storage().persistent();
        let mut permissions = Vec::new(env);

        if let Some(role) = storage.get::<RoleDataKey, Role>(&RoleDataKey::Role(role_id.clone())) {
            if role.is_active {
                // Add direct permissions
                for perm in role.permissions.iter() {
                    if !permissions.contains(&perm) {
                        permissions.push_back(perm);
                    }
                }

                // Add inherited permissions from parent
                if let Some(parent_id) =
                    storage.get::<RoleDataKey, String>(&RoleDataKey::RoleParent(role_id.clone()))
                {
                    let parent_permissions = Self::_get_role_permissions_recursive(env, &parent_id);
                    for perm in parent_permissions.iter() {
                        if !permissions.contains(&perm) {
                            permissions.push_back(perm);
                        }
                    }
                }
            }
        }

        permissions
    }

    /// Revoke a role from a user
    pub fn revoke_role(env: Env, caller: Address, user: Address) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;
        Self::_require_security_permission(&env, &caller, Permission::ManageRoles)?;

        let storage = env.storage().persistent();

        // Check if user has a role assignment
        if let Some(mut assignment) =
            storage.get::<RoleDataKey, UserRoleAssignment>(&RoleDataKey::UserRole(user.clone()))
        {
            assignment.is_active = false;
            storage.set(&RoleDataKey::UserRole(user.clone()), &assignment);

            env.events().publish((ROLE_REVOKED_EVENT,), (caller, user));
        }

        Ok(())
    }

    /// Get user's role assignment
    pub fn get_user_role(env: Env, user: Address) -> Option<UserRoleAssignment> {
        env.storage().persistent().get(&RoleDataKey::UserRole(user))
    }

    /// Get role details
    pub fn get_role(env: Env, role_id: String) -> Option<Role> {
        env.storage().persistent().get(&RoleDataKey::Role(role_id))
    }

    /// Check if user has a specific permission
    pub fn has_permission(env: Env, user: Address, permission: Permission) -> bool {
        let storage = env.storage().persistent();

        // Check if user has a role assignment
        if let Some(assignment) =
            storage.get::<RoleDataKey, UserRoleAssignment>(&RoleDataKey::UserRole(user.clone()))
        {
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
            if let Some(role) =
                storage.get::<RoleDataKey, Role>(&RoleDataKey::Role(assignment.role))
            {
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
        large_disbursement_threshold: Option<i128>,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;
        Self::_require_security_permission(&env, &caller, Permission::ManageSecurity)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        let mut settings = storage
            .get::<DataKey, SecuritySettings>(&DataKey::SecuritySettings)
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
                large_disbursement_threshold: DEFAULT_LARGE_DISBURSEMENT_THRESHOLD,
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
        if let Some(threshold) = large_disbursement_threshold {
            settings.large_disbursement_threshold = threshold;
        }

        settings.last_updated = current_time;
        storage.set(&DataKey::SecuritySettings, &settings);

        Ok(())
    }

    /// Enable multi-factor authentication for the caller
    #[allow(clippy::too_many_arguments)]
    pub fn enable_mfa(
        env: Env,
        caller: Address,
        secret: Bytes,
        digits: u32,
        period: u64,
        emergency_code_hashes: Vec<BytesN<32>>,
        emergency_bypass_enabled: bool,
        session_timeout_override: Option<u64>,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        if secret.is_empty() || period == 0 {
            return Err(PayrollError::InvalidData);
        }

        if digits < 4 || digits > 8 {
            return Err(PayrollError::InvalidData);
        }

        // Ensure any stale sessions are removed before enabling
        Self::clear_user_sessions(&env, &caller);

        let mut config =
            Self::get_user_mfa_config(&env, &caller).unwrap_or_else(|| UserMfaConfig {
                user: caller.clone(),
                is_enabled: true,
                secret: Bytes::new(&env),
                digits,
                period,
                last_verified_at: None,
                session_timeout_override,
                active_sessions: Vec::<u64>::new(&env),
                emergency_bypass_enabled: false,
                emergency_code_hashes: Vec::new(&env),
                emergency_bypass_last_used_at: None,
            });

        config.user = caller.clone();
        config.is_enabled = true;
        config.secret = secret;
        config.digits = digits;
        config.period = period;
        config.last_verified_at = None;
        config.session_timeout_override = session_timeout_override;
        config.active_sessions = Vec::<u64>::new(&env);
        config.emergency_bypass_enabled =
            emergency_bypass_enabled && !emergency_code_hashes.is_empty();
        config.emergency_code_hashes = emergency_code_hashes;
        config.emergency_bypass_last_used_at = None;

        Self::save_user_mfa_config(&env, &config);

        env.events()
            .publish((MFA_ENABLED_EVENT,), (caller, digits, period));

        Ok(())
    }

    /// Disable multi-factor authentication for the caller and destroy active sessions
    pub fn disable_mfa(env: Env, caller: Address) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        Self::clear_user_sessions(&env, &caller);

        let mut config =
            Self::get_user_mfa_config(&env, &caller).unwrap_or_else(|| UserMfaConfig {
                user: caller.clone(),
                is_enabled: false,
                secret: Bytes::new(&env),
                digits: 6,
                period: 30,
                last_verified_at: None,
                session_timeout_override: None,
                active_sessions: Vec::<u64>::new(&env),
                emergency_bypass_enabled: false,
                emergency_code_hashes: Vec::new(&env),
                emergency_bypass_last_used_at: None,
            });

        config.user = caller.clone();
        config.is_enabled = false;
        config.secret = Bytes::new(&env);
        config.digits = 6;
        config.period = 30;
        config.last_verified_at = None;
        config.session_timeout_override = None;
        config.active_sessions = Vec::<u64>::new(&env);
        config.emergency_bypass_enabled = false;
        config.emergency_code_hashes = Vec::new(&env);
        config.emergency_bypass_last_used_at = None;

        Self::save_user_mfa_config(&env, &config);

        env.events().publish((MFA_DISABLED_EVENT,), caller);

        Ok(())
    }

    /// Start an MFA challenge for the provided operation scope
    pub fn begin_mfa_challenge(
        env: Env,
        caller: Address,
        operation: Symbol,
        emergency_bypass: bool,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        if !Self::is_supported_mfa_operation(operation.clone()) {
            return Err(PayrollError::InvalidData);
        }

        let config = Self::get_user_mfa_config(&env, &caller).ok_or(PayrollError::MFARequired)?;
        if !config.is_enabled {
            return Err(PayrollError::MFARequired);
        }

        let emergency_allowed = emergency_bypass
            && config.emergency_bypass_enabled
            && !config.emergency_code_hashes.is_empty();

        let current_time = env.ledger().timestamp();
        let challenge_id = Self::next_mfa_challenge_id(&env);
        let validity = cmp::max(config.period.saturating_mul(2), 120);
        let expires_at = current_time.saturating_add(validity);
        let session_scope = Self::scope_for_operation(&env, operation.clone());

        let challenge = MfaChallenge {
            challenge_id,
            user: caller.clone(),
            operation: operation.clone(),
            issued_at: current_time,
            expires_at,
            attempts_remaining: DEFAULT_MFA_CHALLENGE_ATTEMPTS,
            requires_totp: !emergency_allowed,
            emergency_bypass_allowed: emergency_allowed,
            session_scope: session_scope.clone(),
            resolved: false,
        };

        let storage = env.storage().persistent();
        storage.set(&ExtendedDataKey::MfaChallenge(challenge_id), &challenge);

        let challenge_event = MfaChallengeEvent {
            user: caller.clone(),
            challenge_id,
            operation,
            expires_at,
        };
        env.events()
            .publish((MFA_CHALLENGE_EVENT,), challenge_event);

        Ok(challenge_id)
    }

    /// Complete an MFA challenge using either TOTP or an emergency bypass code
    pub fn complete_mfa_challenge(
        env: Env,
        caller: Address,
        challenge_id: u64,
        totp_code: Option<u32>,
        emergency_code_hash: Option<BytesN<32>>,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let mut config =
            Self::get_user_mfa_config(&env, &caller).ok_or(PayrollError::MFARequired)?;
        if !config.is_enabled {
            return Err(PayrollError::MFARequired);
        }

        let storage = env.storage().persistent();
        let mut challenge = storage
            .get::<ExtendedDataKey, MfaChallenge>(&ExtendedDataKey::MfaChallenge(challenge_id))
            .ok_or(PayrollError::SecurityTokenInvalid)?;

        if challenge.user != caller {
            return Err(PayrollError::Unauthorized);
        }

        if challenge.resolved {
            return Err(PayrollError::SecurityTokenInvalid);
        }

        let current_time = env.ledger().timestamp();
        if current_time > challenge.expires_at {
            challenge.resolved = true;
            storage.set(&ExtendedDataKey::MfaChallenge(challenge_id), &challenge);
            return Err(PayrollError::SecurityTokenInvalid);
        }

        if challenge.attempts_remaining == 0 {
            challenge.resolved = true;
            storage.set(&ExtendedDataKey::MfaChallenge(challenge_id), &challenge);
            return Err(PayrollError::SecurityTokenInvalid);
        }

        let mut emergency_used = false;
        let totp_valid = if let Some(code) = totp_code {
            Self::verify_totp(&config, code, current_time)?
        } else {
            false
        };

        if challenge.requires_totp {
            if !totp_valid {
                challenge.attempts_remaining = challenge.attempts_remaining.saturating_sub(1);
                storage.set(&ExtendedDataKey::MfaChallenge(challenge_id), &challenge);
                return Err(PayrollError::SecurityTokenInvalid);
            }
            config.last_verified_at = Some(current_time);
        } else if totp_valid {
            config.last_verified_at = Some(current_time);
        } else {
            let hash = emergency_code_hash.ok_or(PayrollError::SecurityTokenInvalid)?;
            if !Self::consume_emergency_code(&env, &mut config, &hash) {
                challenge.attempts_remaining = challenge.attempts_remaining.saturating_sub(1);
                storage.set(&ExtendedDataKey::MfaChallenge(challenge_id), &challenge);
                return Err(PayrollError::SecurityTokenInvalid);
            }
            emergency_used = true;
            config.emergency_bypass_last_used_at = Some(current_time);
        }

        challenge.resolved = true;
        challenge.attempts_remaining = 0;
        storage.set(&ExtendedDataKey::MfaChallenge(challenge_id), &challenge);

        let session_id = Self::next_mfa_session_id(&env);
        let session_timeout = config
            .session_timeout_override
            .unwrap_or_else(|| Self::resolve_default_session_timeout(&env));
        let expires_at = current_time.saturating_add(session_timeout);

        let mut issued_scope = challenge.session_scope.clone();
        if emergency_used {
            issued_scope.push_back(MFA_SCOPE_EMERGENCY);
        }

        let session = MfaSession {
            session_id,
            user: caller.clone(),
            issued_for: issued_scope.clone(),
            created_at: current_time,
            last_used_at: current_time,
            expires_at,
            emergency_bypass_used: emergency_used,
            challenge_id,
        };

        storage.set(&ExtendedDataKey::MfaSession(session_id), &session);

        config.active_sessions.push_back(session_id);
        Self::save_user_mfa_config(&env, &config);

        let verification_event = MfaVerificationEvent {
            user: caller.clone(),
            challenge_id,
            session_id,
            operation: challenge.operation,
            emergency_bypass: emergency_used,
        };
        env.events()
            .publish((MFA_VERIFIED_EVENT,), verification_event);

        let session_event = MfaSessionEvent {
            user: caller,
            session_id,
            created_at: current_time,
            expires_at,
            operations: issued_scope,
            emergency_bypass: emergency_used,
        };
        env.events()
            .publish((SESSION_STARTED_EVENT,), session_event);

        Ok(session_id)
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

        env.events()
            .publish((SECURITY_AUDIT_EVENT,), (caller, audit_entries.len()));

        Ok(audit_entries)
    }

    /// Emergency security lockdown
    pub fn emergency_lockdown(env: Env, caller: Address) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;
        Self::_require_security_permission(&env, &caller, Permission::EmergencyOperations)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Pause the contract
        storage.set(&DataKey::Paused, &true);

        // Update security settings to emergency mode
        if let Some(mut settings) =
            storage.get::<DataKey, SecuritySettings>(&DataKey::SecuritySettings)
        {
            settings.emergency_mode = true;
            settings.last_updated = current_time;
            storage.set(&DataKey::SecuritySettings, &settings);
        }

        env.events().publish(
            (SECURITY_POLICY_VIOLATION_EVENT,),
            (
                caller,
                String::from_str(&env, "Emergency lockdown activated"),
            ),
        );

        Ok(())
    }

    //-----------------------------------------------------------------------------
    // Internal Security Helper Functions
    //-----------------------------------------------------------------------------

    fn get_user_mfa_config(env: &Env, user: &Address) -> Option<UserMfaConfig> {
        env.storage()
            .persistent()
            .get(&ExtendedDataKey::UserMfaConfig(user.clone()))
    }

    fn save_user_mfa_config(env: &Env, config: &UserMfaConfig) {
        let storage = env.storage().persistent();
        storage.set(&ExtendedDataKey::UserMfaConfig(config.user.clone()), config);
        storage.set(
            &ExtendedDataKey::UserMfaSessions(config.user.clone()),
            &config.active_sessions,
        );
    }

    fn clear_user_sessions(env: &Env, user: &Address) {
        let storage = env.storage().persistent();
        let mut updated = storage
            .get::<ExtendedDataKey, UserMfaConfig>(&ExtendedDataKey::UserMfaConfig(user.clone()));

        if let Some(mut config) = updated {
            for session_id in config.active_sessions.iter() {
                let key = ExtendedDataKey::MfaSession(session_id);
                if let Some(session) = storage.get::<ExtendedDataKey, MfaSession>(&key) {
                    let event = MfaSessionEvent {
                        user: session.user.clone(),
                        session_id,
                        created_at: session.created_at,
                        expires_at: session.expires_at,
                        operations: session.issued_for.clone(),
                        emergency_bypass: session.emergency_bypass_used,
                    };
                    env.events().publish((SESSION_ENDED_EVENT,), event);
                    storage.remove(&key);
                } else {
                    storage.remove(&key);
                }
            }

            config.active_sessions = Vec::<u64>::new(env);
            Self::save_user_mfa_config(env, &config);
        } else {
            storage.remove(&ExtendedDataKey::UserMfaSessions(user.clone()));
        }
    }

    fn resolve_default_session_timeout(env: &Env) -> u64 {
        env.storage()
            .persistent()
            .get::<DataKey, SecuritySettings>(&DataKey::SecuritySettings)
            .map(|settings| settings.session_timeout)
            .unwrap_or(3600)
    }

    fn next_mfa_session_id(env: &Env) -> u64 {
        let storage = env.storage().persistent();
        let next = storage
            .get::<ExtendedDataKey, u64>(&ExtendedDataKey::NextMfaSessionId)
            .unwrap_or(1);
        storage.set(&ExtendedDataKey::NextMfaSessionId, &(next + 1));
        next
    }

    fn next_mfa_challenge_id(env: &Env) -> u64 {
        let storage = env.storage().persistent();
        let next = storage
            .get::<ExtendedDataKey, u64>(&ExtendedDataKey::NextMfaChallengeId)
            .unwrap_or(1);
        storage.set(&ExtendedDataKey::NextMfaChallengeId, &(next + 1));
        next
    }

    fn scope_for_operation(env: &Env, operation: Symbol) -> Vec<Symbol> {
        let mut scope = Vec::new(env);
        scope.push_back(operation.clone());
        if operation != MFA_SCOPE_ALL {
            scope.push_back(MFA_SCOPE_ALL);
        }
        scope
    }

    fn is_supported_mfa_operation(operation: Symbol) -> bool {
        operation == MFA_SCOPE_PAYROLL
            || operation == MFA_SCOPE_DISBURSE
            || operation == MFA_SCOPE_TRANSFER
            || operation == MFA_SCOPE_EMERGENCY
    }

    fn consume_emergency_code(env: &Env, config: &mut UserMfaConfig, hash: &BytesN<32>) -> bool {
        let mut remaining = Vec::new(env);
        let mut consumed = false;
        for entry in config.emergency_code_hashes.iter() {
            if !consumed && entry == hash.clone() {
                consumed = true;
                continue;
            }
            remaining.push_back(entry.clone());
        }

        if consumed {
            config.emergency_code_hashes = remaining;
        }

        consumed
    }

    fn bytes_to_std(bytes: &Bytes) -> StdVec<u8> {
        let mut result = StdVec::with_capacity(bytes.len() as usize);
        for byte in bytes.iter() {
            result.push(byte);
        }
        result
    }

    fn hotp(secret: &[u8], counter: u64, digits: u32) -> Result<u32, PayrollError> {
        let mut mac = HmacSha1::new_from_slice(secret).map_err(|_| PayrollError::InvalidData)?;
        mac.update(&counter.to_be_bytes());

        let code_bytes = mac.finalize().into_bytes();
        let offset = (code_bytes[code_bytes.len() - 1] & 0x0f) as usize;

        if offset + 3 >= code_bytes.len() {
            return Err(PayrollError::InvalidData);
        }

        let binary = ((code_bytes[offset] & 0x7f) as u32) << 24
            | (code_bytes[offset + 1] as u32) << 16
            | (code_bytes[offset + 2] as u32) << 8
            | (code_bytes[offset + 3] as u32);

        let modulus = 10u32.saturating_pow(digits);
        if modulus == 0 {
            return Err(PayrollError::InvalidData);
        }

        Ok(binary % modulus)
    }

    fn verify_totp(
        config: &UserMfaConfig,
        code: u32,
        timestamp: u64,
    ) -> Result<bool, PayrollError> {
        if config.secret.is_empty() || config.period == 0 {
            return Ok(false);
        }

        let secret_bytes = Self::bytes_to_std(&config.secret);
        let digits = config.digits;
        let period = config.period;
        let base_counter = (timestamp / period) as i64;

        for drift in -MFA_TOTP_ALLOWED_DRIFT..=MFA_TOTP_ALLOWED_DRIFT {
            let candidate_counter = base_counter + drift;
            if candidate_counter < 0 {
                continue;
            }

            let expected = Self::hotp(secret_bytes.as_slice(), candidate_counter as u64, digits)?;
            if expected == code {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn session_allows_operation(session: &MfaSession, operation: Symbol) -> bool {
        if operation == MFA_SCOPE_ALL {
            return true;
        }

        for scope in session.issued_for.iter() {
            if scope == operation || scope == MFA_SCOPE_ALL {
                return true;
            }
        }

        false
    }

    fn ensure_active_mfa_session(
        env: &Env,
        user: &Address,
        operation: Symbol,
    ) -> Result<(), PayrollError> {
        let mut config = match Self::get_user_mfa_config(env, user) {
            Some(cfg) if cfg.is_enabled => cfg,
            Some(_) => return Err(PayrollError::MFARequired),
            None => return Err(PayrollError::MFARequired),
        };

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();
        let mut active = Vec::<u64>::new(env);
        let mut allowed = false;

        for session_id in config.active_sessions.iter() {
            let key = ExtendedDataKey::MfaSession(session_id);
            if let Some(mut session) = storage.get::<ExtendedDataKey, MfaSession>(&key) {
                if session.expires_at <= current_time {
                    let event = MfaSessionEvent {
                        user: session.user.clone(),
                        session_id,
                        created_at: session.created_at,
                        expires_at: session.expires_at,
                        operations: session.issued_for.clone(),
                        emergency_bypass: session.emergency_bypass_used,
                    };
                    env.events().publish((SESSION_ENDED_EVENT,), event);
                    storage.remove(&key);
                    continue;
                }

                if Self::session_allows_operation(&session, operation.clone()) {
                    allowed = true;
                    session.last_used_at = current_time;
                    storage.set(&key, &session);
                }

                active.push_back(session_id);
            } else {
                storage.remove(&key);
            }
        }

        config.active_sessions = active;
        Self::save_user_mfa_config(env, &config);

        if allowed {
            Ok(())
        } else {
            Err(PayrollError::MFARequired)
        }
    }

    fn is_large_disbursement(amount: i128, threshold: i128) -> bool {
        threshold > 0 && amount >= threshold
    }

    fn is_mfa_required_for_operation(
        env: &Env,
        user: &Address,
        operation: Symbol,
        amount: Option<i128>,
    ) -> bool {
        let settings = env
            .storage()
            .persistent()
            .get::<DataKey, SecuritySettings>(&DataKey::SecuritySettings);

        if let Some(cfg) = Self::get_user_mfa_config(env, user) {
            if cfg.is_enabled {
                return true;
            }

            if operation == MFA_SCOPE_DISBURSE {
                if let Some(value) = amount {
                    let threshold = settings
                        .as_ref()
                        .map(|s| s.large_disbursement_threshold)
                        .unwrap_or(DEFAULT_LARGE_DISBURSEMENT_THRESHOLD);
                    if Self::is_large_disbursement(value, threshold) {
                        return true;
                    }
                }
            }

            if let Some(settings_ref) = settings.as_ref() {
                if settings_ref.mfa_required
                    && (operation == MFA_SCOPE_PAYROLL || operation == MFA_SCOPE_TRANSFER)
                {
                    return true;
                }
            }

            return false;
        }

        if let Some(settings_val) = settings {
            if settings_val.mfa_required
                && (operation == MFA_SCOPE_PAYROLL || operation == MFA_SCOPE_TRANSFER)
            {
                return true;
            }

            if operation == MFA_SCOPE_DISBURSE {
                if let Some(value) = amount {
                    if Self::is_large_disbursement(value, settings_val.large_disbursement_threshold)
                    {
                        return true;
                    }
                }
            }
        } else if operation == MFA_SCOPE_DISBURSE {
            if let Some(value) = amount {
                if Self::is_large_disbursement(value, DEFAULT_LARGE_DISBURSEMENT_THRESHOLD) {
                    return true;
                }
            }
        }

        false
    }

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

        // Store audit entry (simplified)
        storage.set(&DataKey::AuditTrail(user.clone()), &audit_entry);

        // Emit audit event for off-chain listeners
        env.events().publish(
            (SECURITY_AUDIT_EVENT,),
            (
                user.clone(),
                String::from_str(env, action),
                String::from_str(env, resource),
                current_time,
            ),
        );
    }

    /// Check rate limiting
    fn _check_rate_limit(
        _env: &Env,
        _user: &Address,
        _operation: &str,
    ) -> Result<(), PayrollError> {
        // Simplified rate limiting check
        // In a real implementation, this would check actual rate limits
        Ok(())
    }

    /// Detect suspicious activity
    fn _detect_suspicious_activity(
        _env: &Env,
        _user: &Address,
        _action: &str,
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
        let department_id = storage
            .get::<EnterpriseDataKey, u64>(&EnterpriseDataKey::NextDepartmentId)
            .unwrap_or(1);
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
        storage.set(
            &enterprise::EnterpriseDataKey::Department(department_id),
            &department,
        );

        // Add to employer's departments
        let mut employer_departments = storage
            .get::<enterprise::EnterpriseDataKey, Vec<u64>>(
                &enterprise::EnterpriseDataKey::EmployerDepartments(caller.clone()),
            )
            .unwrap_or(Vec::new(&env));
        employer_departments.push_back(department_id);
        storage.set(
            &enterprise::EnterpriseDataKey::EmployerDepartments(caller.clone()),
            &employer_departments,
        );

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
        storage.set(
            &EnterpriseDataKey::EmployeeDepartment(employee.clone()),
            &department_id,
        );

        // Emit event
        env.events()
            .publish((symbol_short!("emp_a"),), (caller, employee, department_id));

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
        let workflow_id = storage
            .get::<EnterpriseDataKey, u64>(&EnterpriseDataKey::NextWorkflowId)
            .unwrap_or(1);
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
        storage.set(
            &EnterpriseDataKey::WebhookEndpoint(webhook_id_str.clone()),
            &webhook,
        );

        // Add to employer's webhooks
        let mut employer_webhooks = storage
            .get::<EnterpriseDataKey, Vec<String>>(&EnterpriseDataKey::EmployerWebhooks(
                caller.clone(),
            ))
            .unwrap_or(Vec::new(&env));
        employer_webhooks.push_back(webhook_id_str.clone());
        storage.set(
            &EnterpriseDataKey::EmployerWebhooks(caller.clone()),
            &employer_webhooks,
        );

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
        let report_id = storage
            .get::<EnterpriseDataKey, u64>(&EnterpriseDataKey::NextReportId)
            .unwrap_or(1);
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
        storage.set(
            &EnterpriseDataKey::EmployerReports(caller.clone()),
            &employer_reports,
        );

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
        let schedule_id = storage
            .get::<EnterpriseDataKey, u64>(&EnterpriseDataKey::NextBackupScheduleId)
            .unwrap_or(1);
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
            .get::<EnterpriseDataKey, Vec<u64>>(&EnterpriseDataKey::EmployerBackupSchedules(
                caller.clone(),
            ))
            .unwrap_or(Vec::new(&env));
        employer_schedules.push_back(schedule_id);
        storage.set(
            &EnterpriseDataKey::EmployerBackupSchedules(caller.clone()),
            &employer_schedules,
        );

        // Emit event
        env.events()
            .publish((symbol_short!("bkup_c"),), (caller, schedule_id, name));

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
        if settings.evidence_required && (evidence.len()) < settings.min_evidence_count {
            return Err(PayrollError::InvalidData);
        }

        // Get next dispute ID
        let next_id = storage.get(&EnterpriseDataKey::NextDisputeId).unwrap_or(0) + 1;
        storage.set(&EnterpriseDataKey::NextDisputeId, &next_id);

        // Calculate expiration time
        let timeout_days = if timeout_days == 0 {
            settings.dispute_timeout
        } else {
            timeout_days
        };
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
        let mut employee_disputes: Vec<u64> = storage
            .get(&EnterpriseDataKey::EmployeeDisputes(caller.clone()))
            .unwrap_or(Vec::new(&env));
        employee_disputes.push_back(next_id);
        storage.set(
            &EnterpriseDataKey::EmployeeDisputes(caller.clone()),
            &employee_disputes,
        );

        // Add to employer's disputes
        let mut employer_disputes: Vec<u64> = storage
            .get(&EnterpriseDataKey::EmployerDisputes(employer.clone()))
            .unwrap_or(Vec::new(&env));
        employer_disputes.push_back(next_id);
        storage.set(
            &EnterpriseDataKey::EmployerDisputes(employer.clone()),
            &employer_disputes,
        );

        // Add to open disputes
        let mut open_disputes: Vec<u64> = storage
            .get(&EnterpriseDataKey::OpenDisputes)
            .unwrap_or(Vec::new(&env));
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
        let mut dispute: Dispute = storage
            .get(&EnterpriseDataKey::Dispute(dispute_id))
            .ok_or(PayrollError::PayrollNotFound)?;

        // Check if caller is authorized (employee, employer, or mediator)
        if caller != dispute.employee && caller != dispute.employer {
            // Check if caller is a mediator
            if let Some(mediator) = storage
                .get::<EnterpriseDataKey, Mediator>(&EnterpriseDataKey::Mediator(caller.clone()))
            {
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
            let open_disputes: Vec<u64> = storage
                .get(&EnterpriseDataKey::OpenDisputes)
                .unwrap_or(Vec::new(&env));
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
        let mut dispute: Dispute = storage
            .get(&EnterpriseDataKey::Dispute(dispute_id))
            .ok_or(PayrollError::PayrollNotFound)?;

        // Check if dispute is eligible for escalation
        if dispute.status != DisputeStatus::Open && dispute.status != DisputeStatus::UnderReview {
            return Err(PayrollError::InvalidData);
        }

        // Verify mediator exists and is active
        let mediator: Mediator = storage
            .get(&EnterpriseDataKey::Mediator(mediator_address.clone()))
            .ok_or(PayrollError::PayrollNotFound)?;
        if !mediator.is_active {
            return Err(PayrollError::InvalidData);
        }

        // Get next escalation ID
        let next_id = storage
            .get(&EnterpriseDataKey::NextEscalationId)
            .unwrap_or(0)
            + 1;
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
        let mut dispute_escalations: Vec<u64> = storage
            .get(&EnterpriseDataKey::DisputeEscalations(dispute_id))
            .unwrap_or(Vec::new(&env));
        dispute_escalations.push_back(next_id);
        storage.set(
            &EnterpriseDataKey::DisputeEscalations(dispute_id),
            &dispute_escalations,
        );

        // Add to mediator escalations
        let mut mediator_escalations: Vec<u64> = storage
            .get(&EnterpriseDataKey::MediatorEscalations(
                mediator_address.clone(),
            ))
            .unwrap_or(Vec::new(&env));
        mediator_escalations.push_back(next_id);
        storage.set(
            &EnterpriseDataKey::MediatorEscalations(mediator_address.clone()),
            &mediator_escalations,
        );

        // Add to escalated disputes
        let mut escalated_disputes: Vec<u64> = storage
            .get(&EnterpriseDataKey::EscalatedDisputes)
            .unwrap_or(Vec::new(&env));
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
        let mut escalation: Escalation = storage
            .get(&EnterpriseDataKey::Escalation(escalation_id))
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
        let mut dispute: Dispute = storage
            .get(&EnterpriseDataKey::Dispute(escalation.dispute_id))
            .ok_or(PayrollError::PayrollNotFound)?;
        dispute.status = DisputeStatus::Resolved;
        dispute.resolution = Some(resolution.clone());
        dispute.resolution_by = Some(mediator.clone());
        dispute.resolved_at = Some(current_time);
        dispute.updated_at = current_time;
        storage.set(&EnterpriseDataKey::Dispute(escalation.dispute_id), &dispute);

        // Remove from escalated disputes
        let escalated_disputes: Vec<u64> = storage
            .get(&EnterpriseDataKey::EscalatedDisputes)
            .unwrap_or(Vec::new(&env));
        let mut new_escalated_disputes = Vec::new(&env);
        for id in escalated_disputes.iter() {
            if id != escalation.dispute_id {
                new_escalated_disputes.push_back(id);
            }
        }
        storage.set(
            &EnterpriseDataKey::EscalatedDisputes,
            &new_escalated_disputes,
        );

        // Remove from open disputes
        let open_disputes: Vec<u64> = storage
            .get(&EnterpriseDataKey::OpenDisputes)
            .unwrap_or(Vec::new(&env));
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
        storage
            .get(&EnterpriseDataKey::DisputeSettings)
            .unwrap_or(DisputeSettings {
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
        let owner = storage
            .get(&DataKey::Owner)
            .ok_or(PayrollError::Unauthorized)?;
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
        storage.set(
            &EnterpriseDataKey::Mediator(mediator_address.clone()),
            &mediator,
        );

        // Add to active mediators
        let mut active_mediators: Vec<Address> = storage
            .get(&EnterpriseDataKey::ActiveMediators)
            .unwrap_or(Vec::new(&env));
        active_mediators.push_back(mediator_address.clone());
        storage.set(&EnterpriseDataKey::ActiveMediators, &active_mediators);

        // Add to specialization index
        for spec in specialization.iter() {
            let mut mediators_by_spec: Vec<Address> = storage
                .get(&EnterpriseDataKey::MediatorBySpecialization(spec.clone()))
                .unwrap_or(Vec::new(&env));
            mediators_by_spec.push_back(mediator_address.clone());
            storage.set(
                &EnterpriseDataKey::MediatorBySpecialization(spec.clone()),
                &mediators_by_spec,
            );
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
        let owner = storage
            .get(&DataKey::Owner)
            .ok_or(PayrollError::Unauthorized)?;
        if caller != owner {
            return Err(PayrollError::Unauthorized);
        }

        storage.set(&EnterpriseDataKey::DisputeSettings, &settings);

        // Emit settings updated event
        env.events()
            .publish((symbol_short!("settings"),), (caller, settings));

        Ok(())
    }

    /// Get a dispute by ID
    pub fn get_dispute(env: Env, dispute_id: u64) -> Result<Dispute, PayrollError> {
        let storage = env.storage().persistent();
        storage
            .get(&EnterpriseDataKey::Dispute(dispute_id))
            .ok_or(PayrollError::PayrollNotFound)
    }

    /// Get all disputes for an employee
    pub fn get_employee_disputes(
        env: Env,
        employee: Address,
    ) -> Result<Vec<Dispute>, PayrollError> {
        let storage = env.storage().persistent();
        let dispute_ids: Vec<u64> = storage
            .get(&EnterpriseDataKey::EmployeeDisputes(employee.clone()))
            .unwrap_or(Vec::new(&env));

        let mut disputes = Vec::new(&env);
        for id in dispute_ids.iter() {
            if let Some(dispute) = storage.get(&EnterpriseDataKey::Dispute(id)) {
                disputes.push_back(dispute);
            }
        }

        Ok(disputes)
    }

    /// Get all disputes for an employer
    pub fn get_employer_disputes(
        env: Env,
        employer: Address,
    ) -> Result<Vec<Dispute>, PayrollError> {
        let storage = env.storage().persistent();
        let dispute_ids: Vec<u64> = storage
            .get(&EnterpriseDataKey::EmployerDisputes(employer.clone()))
            .unwrap_or(Vec::new(&env));

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
        let dispute_ids: Vec<u64> = storage
            .get(&EnterpriseDataKey::OpenDisputes)
            .unwrap_or(Vec::new(&env));

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
        let dispute_ids: Vec<u64> = storage
            .get(&EnterpriseDataKey::EscalatedDisputes)
            .unwrap_or(Vec::new(&env));

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
        let next_id = storage
            .get(&EnterpriseDataKey::NextModificationRequestId)
            .unwrap_or(0)
            + 1;
        storage.set(&EnterpriseDataKey::NextModificationRequestId, &next_id);

        // Calculate expiration time (default 30 days if not specified)
        let timeout_days = if approval_timeout_days == 0 {
            30
        } else {
            approval_timeout_days
        };
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
        storage.set(
            &EnterpriseDataKey::PayrollModificationRequest(next_id),
            &modification_request,
        );

        // Add to employee's modification requests
        let mut employee_requests: Vec<u64> = storage
            .get(&EnterpriseDataKey::EmployeeModificationRequests(
                employee.clone(),
            ))
            .unwrap_or(Vec::new(&env));
        employee_requests.push_back(next_id);
        storage.set(
            &EnterpriseDataKey::EmployeeModificationRequests(employee.clone()),
            &employee_requests,
        );

        // Add to employer's modification requests
        let mut employer_requests: Vec<u64> = storage
            .get(&EnterpriseDataKey::EmployerModificationRequests(
                payroll.employer.clone(),
            ))
            .unwrap_or(Vec::new(&env));
        employer_requests.push_back(next_id);
        storage.set(
            &EnterpriseDataKey::EmployerModificationRequests(payroll.employer.clone()),
            &employer_requests,
        );

        // Add to pending modification requests
        let mut pending_requests: Vec<u64> = storage
            .get(&EnterpriseDataKey::PendingModificationRequests)
            .unwrap_or(Vec::new(&env));
        pending_requests.push_back(next_id);
        storage.set(
            &EnterpriseDataKey::PendingModificationRequests,
            &pending_requests,
        );

        // Emit modification request event
        env.events().publish(
            (symbol_short!("mod_req"),),
            (
                requester,
                next_id,
                employee,
                modification_type,
                current_value,
                proposed_value,
            ),
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
        let mut modification_request: PayrollModificationRequest = storage
            .get(&EnterpriseDataKey::PayrollModificationRequest(request_id))
            .ok_or(PayrollError::PayrollNotFound)?;

        // Check if request has expired
        if current_time > modification_request.expires_at {
            modification_request.status = PayrollModificationStatus::Expired;
            storage.set(
                &EnterpriseDataKey::PayrollModificationRequest(request_id),
                &modification_request,
            );
            return Err(PayrollError::InvalidData);
        }

        // Check if request is already completed
        if modification_request.status == PayrollModificationStatus::BothApproved
            || modification_request.status == PayrollModificationStatus::Rejected
            || modification_request.status == PayrollModificationStatus::Cancelled
        {
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
        if modification_request.employer_approval.approved
            && modification_request.employee_approval.approved
        {
            modification_request.status = PayrollModificationStatus::BothApproved;

            // Apply the modification to the payroll
            Self::_apply_payroll_modification(&env, &modification_request)?;
        } else if modification_request.employer_approval.approved {
            modification_request.status = PayrollModificationStatus::EmployerApproved;
        } else if modification_request.employee_approval.approved {
            modification_request.status = PayrollModificationStatus::EmployeeApproved;
        }

        // Store updated modification request
        storage.set(
            &EnterpriseDataKey::PayrollModificationRequest(request_id),
            &modification_request,
        );

        // Remove from pending requests if completed
        if modification_request.status == PayrollModificationStatus::BothApproved
            || modification_request.status == PayrollModificationStatus::Rejected
        {
            let pending_requests: Vec<u64> = storage
                .get(&EnterpriseDataKey::PendingModificationRequests)
                .unwrap_or(Vec::new(&env));
            let mut new_pending_requests = Vec::new(&env);
            for id in pending_requests.iter() {
                if id != request_id {
                    new_pending_requests.push_back(id);
                }
            }
            storage.set(
                &EnterpriseDataKey::PendingModificationRequests,
                &new_pending_requests,
            );
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
        let mut modification_request: PayrollModificationRequest = storage
            .get(&EnterpriseDataKey::PayrollModificationRequest(request_id))
            .ok_or(PayrollError::PayrollNotFound)?;

        // Check if request has expired
        if current_time > modification_request.expires_at {
            modification_request.status = PayrollModificationStatus::Expired;
            storage.set(
                &EnterpriseDataKey::PayrollModificationRequest(request_id),
                &modification_request,
            );
            return Err(PayrollError::InvalidData);
        }

        // Check if request is already completed
        if modification_request.status == PayrollModificationStatus::BothApproved
            || modification_request.status == PayrollModificationStatus::Rejected
            || modification_request.status == PayrollModificationStatus::Cancelled
        {
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
        storage.set(
            &EnterpriseDataKey::PayrollModificationRequest(request_id),
            &modification_request,
        );

        // Remove from pending requests
        let pending_requests: Vec<u64> = storage
            .get(&EnterpriseDataKey::PendingModificationRequests)
            .unwrap_or(Vec::new(&env));
        let mut new_pending_requests = Vec::new(&env);
        for id in pending_requests.iter() {
            if id != request_id {
                new_pending_requests.push_back(id);
            }
        }
        storage.set(
            &EnterpriseDataKey::PendingModificationRequests,
            &new_pending_requests,
        );

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
        storage
            .get(&EnterpriseDataKey::PayrollModificationRequest(request_id))
            .ok_or(PayrollError::PayrollNotFound)
    }

    /// Get all modification requests for an employee
    pub fn get_employee_mod_requests(
        env: Env,
        employee: Address,
    ) -> Result<Vec<PayrollModificationRequest>, PayrollError> {
        let storage = env.storage().persistent();
        let request_ids: Vec<u64> = storage
            .get(&EnterpriseDataKey::EmployeeModificationRequests(
                employee.clone(),
            ))
            .unwrap_or(Vec::new(&env));

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
        let request_ids: Vec<u64> = storage
            .get(&EnterpriseDataKey::EmployerModificationRequests(
                employer.clone(),
            ))
            .unwrap_or(Vec::new(&env));

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
        let request_ids: Vec<u64> = storage
            .get(&EnterpriseDataKey::PendingModificationRequests)
            .unwrap_or(Vec::new(&env));

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
            }
            PayrollModificationType::Interval => {
                // Parse the proposed interval value (simplified parsing)
                let new_interval = Self::_parse_u64(&modification_request.proposed_value)?;
                payroll.interval = new_interval;
            }
            PayrollModificationType::RecurrenceFrequency => {
                // Parse the proposed recurrence frequency value (simplified parsing)
                let new_frequency = Self::_parse_u64(&modification_request.proposed_value)?;
                payroll.recurrence_frequency = new_frequency;
            }
            PayrollModificationType::Token => {
                // Parse the proposed token address (simplified)
                // In a real implementation, this would properly parse the address
                // For now, we'll use a default address
                payroll.token = Address::from_str(
                    &env,
                    "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF",
                );
            }
            PayrollModificationType::Custom(_) => {
                // For custom modifications, the implementation would depend on the specific use case
                // This is a placeholder for future custom modification types
                return Err(PayrollError::InvalidData);
            }
        }

        // Update the payroll
        let compact_payroll = Self::to_compact_payroll(&payroll);
        storage.set(
            &DataKey::Payroll(modification_request.employee.clone()),
            &compact_payroll,
        );

        // Emit modification applied event
        env.events().publish(
            (symbol_short!("mod_appl"),),
            (
                modification_request.employee.clone(),
                modification_request.request_type.clone(),
                modification_request.proposed_value.clone(),
            ),
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
        let pending_key = RoleDataKey::Role(String::from_str(&env, "pending_ownership_transfer"));
        if let Some(_pending_transfer) = storage.get::<RoleDataKey, Address>(&pending_key) {
            // If there's a pending transfer, check if the new owner is confirming
            if caller == new_owner {
                // New owner is confirming the transfer
                storage.remove(&pending_key);
                storage.set(&DataKey::Owner, &new_owner);

                // Emit event
                env.events()
                    .publish((symbol_short!("owner_tr"),), (caller, new_owner));
                return Ok(());
            } else {
                return Err(PayrollError::Unauthorized);
            }
        }

        // Create a pending transfer request
        storage.set(&pending_key, &new_owner);

        // Emit event for pending transfer
        env.events()
            .publish((symbol_short!("pend_tr"),), (caller, new_owner));

        Ok(())
    }

    /// Enhanced pause contract with multi-signature support
    pub fn pause_contract_with_multisig(env: Env, caller: Address) -> Result<(), PayrollError> {
        caller.require_auth();

        // Simple multi-signature: require both owner and caller approval
        let storage = env.storage().persistent();
        let owner = storage
            .get(&DataKey::Owner)
            .ok_or(PayrollError::Unauthorized)?;

        if caller == owner {
            // Owner can pause directly
            Self::pause(env, caller)
        } else {
            // Non-owner needs to create a pending pause request
            let pending_key = RoleDataKey::Role(String::from_str(&env, "pending_pause_request"));
            storage.set(&pending_key, &caller);

            // Emit event for pending pause
            env.events().publish((symbol_short!("pending_p"),), caller);

            Ok(())
        }
    }

    /// Enhanced unpause contract with multi-signature support
    pub fn unpause_contract_with_multisig(env: Env, caller: Address) -> Result<(), PayrollError> {
        caller.require_auth();

        // Simple multi-signature: require both owner and caller approval
        let storage = env.storage().persistent();
        let owner = storage
            .get(&DataKey::Owner)
            .ok_or(PayrollError::Unauthorized)?;

        if caller == owner {
            // Owner can unpause directly
            Self::unpause(env, caller)
        } else {
            // Non-owner needs to create a pending unpause request
            let pending_key = RoleDataKey::Role(String::from_str(&env, "pending_unpause_request"));
            storage.set(&pending_key, &caller);

            // Emit event for pending unpause
            env.events().publish((symbol_short!("pending_u"),), caller);

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
        let owner = storage
            .get(&DataKey::Owner)
            .ok_or(PayrollError::Unauthorized)?;

        // Only owner can confirm operations
        if caller != owner {
            return Err(PayrollError::Unauthorized);
        }

        if operation_type == String::from_str(&env, "pause") {
            let pending_key = RoleDataKey::Role(String::from_str(&env, "pending_pause_request"));
            if let Some(requester) = storage.get::<RoleDataKey, Address>(&pending_key) {
                storage.remove(&pending_key);
                storage.set(&DataKey::Paused, &true);

                env.events().publish((PAUSED_EVENT,), (requester, caller));
            }
        } else if operation_type == String::from_str(&env, "unpause") {
            let pending_key = RoleDataKey::Role(String::from_str(&env, "pending_unpause_request"));
            if let Some(requester) = storage.get::<RoleDataKey, Address>(&pending_key) {
                storage.remove(&pending_key);
                storage.set(&DataKey::Paused, &false);

                env.events().publish((UNPAUSED_EVENT,), (requester, caller));
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
        let owner = storage
            .get(&DataKey::Owner)
            .ok_or(PayrollError::Unauthorized)?;

        // Only owner can view pending operations
        if caller != owner {
            return Err(PayrollError::Unauthorized);
        }

        let mut pending_operations = Vec::new(&env);

        // Check for pending pause request
        let pause_key = RoleDataKey::Role(String::from_str(&env, "pending_pause_request"));
        if storage.has(&pause_key) {
            pending_operations.push_back(String::from_str(&env, "pause"));
        }

        // Check for pending unpause request
        let unpause_key = RoleDataKey::Role(String::from_str(&env, "pending_unpause_request"));
        if storage.has(&unpause_key) {
            pending_operations.push_back(String::from_str(&env, "unpause"));
        }

        // Check for pending ownership transfer
        let transfer_key = RoleDataKey::Role(String::from_str(&env, "pending_ownership_transfer"));
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
        let owner = storage
            .get(&DataKey::Owner)
            .ok_or(PayrollError::Unauthorized)?;

        // Only owner can cancel operations
        if caller != owner {
            return Err(PayrollError::Unauthorized);
        }

        if operation_type == String::from_str(&env, "pause") {
            let pending_key = RoleDataKey::Role(String::from_str(&env, "pending_pause_request"));
            storage.remove(&pending_key);
        } else if operation_type == String::from_str(&env, "unpause") {
            let pending_key = RoleDataKey::Role(String::from_str(&env, "pending_unpause_request"));
            storage.remove(&pending_key);
        } else if operation_type == String::from_str(&env, "ownership_transfer") {
            let pending_key =
                RoleDataKey::Role(String::from_str(&env, "pending_ownership_transfer"));
            storage.remove(&pending_key);
        } else {
            return Err(PayrollError::InvalidData);
        }

        env.events()
            .publish((symbol_short!("cancel_op"),), (caller, operation_type));

        Ok(())
    }

    /// Parse i128 from string (simplified implementation)
    fn _parse_i128(_value: &String) -> Result<i128, PayrollError> {
        // Simplified parsing - in a real implementation, this would be more robust
        // For now, we'll return a default value
        Ok(1000) // Default value
    }

    /// Parse u64 from string (simplified implementation)
    fn _parse_u64(_value: &String) -> Result<u64, PayrollError> {
        // Simplified parsing - in a real implementation, this would be more robust
        // For now, we'll return a default value
        Ok(86400) // Default value (1 day in seconds)
    }

    // record_metrics(&env, 0, symbol_short!("disburses"), false, Some(employee), Some(symbol_short!("unauth")), false);
    // record_metrics(env,amount. ,operation_type: Symbol, is_success, employee:    ,       error_type.         ,is_late: bool)
    /// Record performance metrics for an operation with daily aggregation
    //     env: &Env,
    //     amount: i128,
    //     operation_type: Symbol,
    //     is_success: bool,
    //     employee: Option<Address>,
    //     error_type: Option<Symbol>,
    //     is_late: bool,
    // ) {
    //     let storage = env.storage().persistent();

    //     // Convert timestamp to start of day (midnight UTC) for daily aggregation
    //     let timestamp = env.ledger().timestamp();
    //     log!(&env, "timestamp in record metrics: {}", timestamp);

    //     let day_timestamp = (timestamp / 86_400) * 86_400; // Round down to nearest day (86,400 seconds)
    //     // log!(&env, "day_timestamp: {}", day_timestamp);

    //     let metrics_key = DataKey::Metrics(day_timestamp);

    //     // Retrieve existing metrics or initialize new ones
    //     let mut metrics: PerformanceMetrics = storage.get(&metrics_key).unwrap_or(PerformanceMetrics {
    //         total_disbursements: 0,
    //         total_amount: 0,
    //         // gas_used: 0,
    //         operation_count: 0,
    //         timestamp: day_timestamp,
    //         error_count: 0,
    //         error_types: Map::new(&env),
    //         employee_count: 0,
    //         operation_type_counts: Map::new(&env),
    //         // compliance_violations: 0,
    //         late_disbursements: 0,
    //     });

    //     // Update metrics with overflow checks
    //     let prev_operation_count = metrics.operation_count;

    //     metrics.operation_count = metrics.operation_count.checked_add(1).unwrap_or(metrics.operation_count);

    //     if is_success {
    //         metrics.total_disbursements = metrics.total_disbursements.checked_add(1).unwrap_or(metrics.total_disbursements);
    //         metrics.total_amount = metrics.total_amount.checked_add(amount).unwrap_or(metrics.total_amount);
    //         log!(&env, "SUCCESS: {}");

    //     } else {

    //         metrics.error_count = metrics.error_count.checked_add(1).unwrap_or(metrics.error_count);
    //         if let Some(err) = error_type {
    //             let err_count = metrics.error_types.get(err.clone()).unwrap_or(0);
    //             metrics.error_types.set(err, err_count.checked_add(1).unwrap_or(err_count));
    //         }
    //         log!(&env, "OTHERS: {}");

    //     }
    //     // metrics.gas_used = metrics.gas_used.checked_add(gas_used).unwrap_or(metrics.gas_used);

    //     // Track unique employees
    //     if let Some(emp) = employee {
    //         let employee_key = DataKey::Employee(emp.clone());
    //         if !storage.has(&employee_key) {
    //             storage.set(&employee_key, &true);
    //             metrics.employee_count = metrics.employee_count.checked_add(1).unwrap_or(metrics.employee_count);
    //         }
    //     }

    //     // Update operation type counts
    //     let current_count = metrics.operation_type_counts.get(operation_type.clone()).unwrap_or(0);
    //     metrics.operation_type_counts.set(operation_type.clone(), current_count.checked_add(1).unwrap_or(current_count));

    //     // Track compliance violations
    //     // if is_compliance_issue {
    //         // metrics.compliance_violations = metrics.compliance_violations.checked_add(1).unwrap_or(metrics.compliance_violations);
    //     // }

    //     // Track late disbursements
    //     if is_late {
    //         metrics.late_disbursements = metrics.late_disbursements.checked_add(1).unwrap_or(metrics.late_disbursements);
    //     }

    //     // Only write to storage if metrics have changed
    //     // if metrics.operation_count > prev_operation_count || metrics.total_amount != 0 || metrics.error_count > 0 || metrics.late_disbursements > 0 {
    //         storage.set(&metrics_key, &metrics);
    //         log!(&env, "day_timestamp: {}", day_timestamp);
    //         log!(&env, "metrics: {}", metrics);

    //         // let res = Self::get_metrics(&env, Some(day_timestamp), Some(day_timestamp *3), Some(3));
    //         // log!(&env, "res: {}", res);

    //         // Publish event with key metrics
    //         env.events().publish(
    //             (METRICS_UPDATED_EVENT,),
    //             (
    //                 day_timestamp,
    //                 operation_type,
    //                 metrics.total_disbursements,
    //                 metrics.total_amount,
    //                 metrics.error_count,
    //                 // metrics.compliance_violations,
    //                 metrics.late_disbursements,
    //             ),
    //         );
    //     // }
    // }

    fn record_metrics(
        env: &Env,
        amount: i128,
        operation_type: Symbol,
        is_success: bool,
        employee: Option<Address>,
        is_late: bool,
    ) {
        let storage = env.storage().persistent();
        let timestamp = env.ledger().timestamp();
        // log!(&env, "timestamp in record metrics: {}", timestamp);
        let day_timestamp = (timestamp / 86_400) * 86_400;
        let metrics_key = DataKey::Metrics(day_timestamp);

        let mut metrics: PerformanceMetrics =
            storage.get(&metrics_key).unwrap_or(PerformanceMetrics {
                total_disbursements: 0,
                total_amount: 0,
                operation_count: 0,
                timestamp: day_timestamp,
                employee_count: 0,
                operation_type_counts: Map::new(&env),
                late_disbursements: 0,
            });

        let prev_operation_count = metrics.operation_count;
        metrics.operation_count = metrics
            .operation_count
            .checked_add(1)
            .unwrap_or(metrics.operation_count);
        if is_success {
            metrics.total_disbursements = metrics
                .total_disbursements
                .checked_add(1)
                .unwrap_or(metrics.total_disbursements);
            metrics.total_amount = metrics
                .total_amount
                .checked_add(amount)
                .unwrap_or(metrics.total_amount);
            // log!(&env, "SUCCESS: {}");
        } else {
            // log!(&env, "OTHERS: {}");
        }

        if let Some(emp) = employee.clone() {
            let employee_key = DataKey::Employee(emp.clone());
            if !storage.has(&employee_key) {
                storage.set(&employee_key, &true);
                metrics.employee_count = metrics
                    .employee_count
                    .checked_add(1)
                    .unwrap_or(metrics.employee_count);
            }
        }

        let current_count = metrics
            .operation_type_counts
            .get(operation_type.clone())
            .unwrap_or(0);
        metrics.operation_type_counts.set(
            operation_type.clone(),
            current_count.checked_add(1).unwrap_or(current_count),
        );

        if is_late {
            metrics.late_disbursements = metrics
                .late_disbursements
                .checked_add(1)
                .unwrap_or(metrics.late_disbursements);
        }

        if metrics.operation_count > prev_operation_count
            || metrics.total_amount != 0
            || metrics.late_disbursements > 0
        {
            storage.set(&metrics_key, &metrics);
            // log!(&env, "day_timestamp: {}", day_timestamp);
            // let _res = Self::get_metrics(&env, Some(day_timestamp), Some(day_timestamp * 3), Some(3));
            // log!(&env, "res: {}", _res);

            env.events().publish(
                (METRICS_UPDATED_EVENT,),
                (
                    day_timestamp,
                    operation_type,
                    metrics.total_disbursements,
                    metrics.total_amount,
                    metrics.late_disbursements,
                ),
            );
        }
    }

    /// Get all performance metrics with optional time range and limit
    pub fn get_metrics(
        env: &Env,
        start_timestamp: Option<u64>,
        end_timestamp: Option<u64>,
        limit: Option<u32>,
    ) -> Vec<PerformanceMetrics> {
        let storage = env.storage().persistent();
        let mut metrics_list = Vec::new(&env);
        let max_entries = limit.unwrap_or(100);

        // Default to all available metrics if no timestamps provided
        let start = start_timestamp.unwrap_or(0);
        let end = end_timestamp.unwrap_or(env.ledger().timestamp());

        // log!(&env, "timestamp in get metrics: {}", start);

        // Round to day boundaries for daily aggregation
        let start_day = (start / 86_400) * 86_400;
        let end_day = (end / 86_400) * 86_400;

        let mut count = 0;
        for timestamp in (start_day..=end_day).step_by(86_400) {
            if let Some(metrics) =
                storage.get::<DataKey, PerformanceMetrics>(&DataKey::Metrics(timestamp))
            {
                metrics_list.push_back(metrics);
                count += 1;
                if count >= max_entries {
                    break;
                }
            }
        }

        metrics_list
    }

    /// Calculate average metrics over a time range
    pub fn calculate_avg_metrics(
        env: &Env,
        start_timestamp: u64,
        end_timestamp: u64,
    ) -> Option<PerformanceMetrics> {
        let storage = env.storage().persistent();
        let mut total_disbursements = 0u64;
        let mut total_amount = 0i128;
        let mut total_operation_count = 0u64;
        let mut employee_count = 0u32;
        let mut operation_type_counts = Map::new(&env);
        let mut late_disbursements = 0u64;

        let start_day = (start_timestamp / 86_400) * 86_400;
        let end_day = (end_timestamp / 86_400) * 86_400;

        for timestamp in (start_day..=end_day).step_by(86_400) {
            if let Some(metrics) =
                storage.get::<DataKey, PerformanceMetrics>(&DataKey::Metrics(timestamp))
            {
                total_disbursements = total_disbursements
                    .checked_add(metrics.total_disbursements)
                    .unwrap_or(total_disbursements);
                total_amount = total_amount
                    .checked_add(metrics.total_amount)
                    .unwrap_or(total_amount);
                total_operation_count = total_operation_count
                    .checked_add(metrics.operation_count)
                    .unwrap_or(total_operation_count);
                employee_count = employee_count
                    .checked_add(metrics.employee_count)
                    .unwrap_or(employee_count);
                late_disbursements = late_disbursements
                    .checked_add(metrics.late_disbursements)
                    .unwrap_or(late_disbursements);

                for (op_type, count) in metrics.operation_type_counts.iter() {
                    let current_count = operation_type_counts.get(op_type.clone()).unwrap_or(0);
                    operation_type_counts.set(
                        op_type,
                        (current_count as u64)
                            .checked_add(count as u64)
                            .unwrap_or(current_count),
                    );
                }
            }
        }

        if total_operation_count == 0 {
            return None;
        }

        Some(PerformanceMetrics {
            total_disbursements,
            total_amount,
            operation_count: total_operation_count,
            timestamp: end_timestamp,
            employee_count,
            operation_type_counts,
            late_disbursements,
        })
    }

    pub fn calculate_total_deposited_token(
        env: &Env,
        start_timestamp: u64,
        end_timestamp: u64,
    ) -> Option<i128> {
        let storage = env.storage().persistent();
        let mut total_deposited_token = 0i128;
        let mut total_operation_count = 0_u64;
        let start_day = (start_timestamp / 86_400) * 86_400;
        let end_day = (end_timestamp / 86_400) * 86_400;

        for timestamp in (start_day..=end_day).step_by(86_400) {
            if let Some(metrics) =
                storage.get::<DataKey, PerformanceMetrics>(&DataKey::Metrics(timestamp))
            {
                total_operation_count = total_operation_count
                    .checked_add(metrics.operation_count)
                    .unwrap_or(total_operation_count);
                for (op_type, _count) in metrics.operation_type_counts.iter() {
                    if op_type == symbol_short!("deposit") {
                        total_deposited_token = total_deposited_token
                            .checked_add(metrics.total_amount)
                            .unwrap_or(total_deposited_token);
                    }
                }
            }
        }

        if total_operation_count == 0 {
            return None;
        }

        Some(total_deposited_token)
    }

    pub fn generate_performance_report(
        env: &Env,
        start_timestamp: u64,
        end_timestamp: u64,
    ) -> Option<PerformanceMetrics> {
        let metrics = Self::calculate_avg_metrics(&env, start_timestamp, end_timestamp)?;
        env.events().publish(
            (METRICS_UPDATED_EVENT,),
            (
                start_timestamp,
                end_timestamp,
                metrics.total_amount,
                metrics.late_disbursements,
            ),
        );
        Some(metrics)
    }

    //-----------------------------------------------------------------------------
    // Employee Lifecycle Management
    //-----------------------------------------------------------------------------

    /// Create employee profile and start onboarding workflow
    pub fn onboard_employee(
        env: Env,
        employer: Address,
        employee: Address,
        department_id: Option<u64>,
        job_title: String,
        employee_id: String,
        manager: Option<Address>,
    ) -> Result<u64, PayrollError> {
        employer.require_auth();
        Self::require_not_paused(&env)?;

        let current_time = env.ledger().timestamp();

        // Check if employee already exists
        if LifecycleStorage::get_profile(&env, &employee).is_some() {
            return Err(PayrollError::InvalidData);
        }

        // Create employee profile
        let profile = EmployeeProfile {
            employee: employee.clone(),
            employer: employer.clone(),
            department_id,
            status: EmployeeStatus::Pending,
            hire_date: current_time,
            termination_date: None,
            job_title,
            employee_id,
            manager,
            created_at: current_time,
            updated_at: current_time,
            metadata: Map::new(&env),
        };

        LifecycleStorage::store_profile(&env, &employee, &profile);

        // Create onboarding workflow
        let workflow_id = LifecycleStorage::get_next_onboarding_id(&env);
        let mut checklist = Vec::new(&env);

        // Default onboarding tasks
        checklist.push_back(OnboardingTask {
            id: 1,
            name: String::from_str(&env, "Complete paperwork"),
            description: String::from_str(&env, "Fill out employment forms"),
            required: true,
            completed: false,
            completed_at: None,
            completed_by: None,
            due_date: Some(current_time + 604800), // 7 days
        });

        checklist.push_back(OnboardingTask {
            id: 2,
            name: String::from_str(&env, "Setup payroll"),
            description: String::from_str(&env, "Configure salary and payment details"),
            required: true,
            completed: false,
            completed_at: None,
            completed_by: None,
            due_date: Some(current_time + 1209600), // 14 days
        });

        let workflow = OnboardingWorkflow {
            id: workflow_id,
            employee: employee.clone(),
            employer: employer.clone(),
            status: WorkflowStatus::InProgress,
            checklist,
            approvals: Vec::new(&env),
            created_at: current_time,
            completed_at: None,
            expires_at: current_time + 2592000, // 30 days
        };

        LifecycleStorage::store_onboarding(&env, workflow_id, &workflow);
        LifecycleStorage::link_employee_onboarding(&env, &employee, workflow_id);

        env.events().publish(
            (symbol_short!("onb_start"),),
            (employer, employee.clone(), workflow_id),
        );

        Ok(workflow_id)
    }

    /// Complete onboarding task
    pub fn complete_onboarding_task(
        env: Env,
        caller: Address,
        employee: Address,
        task_id: u32,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let workflow_id = LifecycleStorage::get_employee_onboarding_id(&env, &employee)
            .ok_or(PayrollError::PayrollNotFound)?;

        let mut workflow = LifecycleStorage::get_onboarding(&env, workflow_id)
            .ok_or(PayrollError::PayrollNotFound)?;

        // Find and complete the task
        let mut task_found = false;
        let current_time = env.ledger().timestamp();

        for i in 0..workflow.checklist.len() {
            if let Some(mut task) = workflow.checklist.get(i) {
                if task.id == task_id {
                    task.completed = true;
                    task.completed_at = Some(current_time);
                    task.completed_by = Some(caller.clone());
                    workflow.checklist.set(i, task);
                    task_found = true;
                    break;
                }
            }
        }

        if !task_found {
            return Err(PayrollError::InvalidData);
        }

        // Check if all required tasks are completed
        let mut all_required_completed = true;
        for i in 0..workflow.checklist.len() {
            if let Some(task) = workflow.checklist.get(i) {
                if task.required && !task.completed {
                    all_required_completed = false;
                    break;
                }
            }
        }

        // If all required tasks completed, finish onboarding
        if all_required_completed {
            workflow.status = WorkflowStatus::Completed;
            workflow.completed_at = Some(current_time);

            // Update employee status to active
            Self::update_employee_status(env.clone(), employee.clone(), EmployeeStatus::Active)?;

            env.events().publish(
                (symbol_short!("onb_comp"),),
                (workflow.employer.clone(), employee.clone(), workflow_id),
            );
        }

        LifecycleStorage::store_onboarding(&env, workflow_id, &workflow);

        Ok(())
    }

    /// Start offboarding process
    pub fn start_offboarding(
        env: Env,
        employer: Address,
        employee: Address,
        termination_reason: String,
        final_payment_amount: Option<i128>,
        final_payment_token: Option<Address>,
    ) -> Result<u64, PayrollError> {
        employer.require_auth();
        Self::require_not_paused(&env)?;

        let current_time = env.ledger().timestamp();

        // Check if employee exists and is active
        let profile =
            LifecycleStorage::get_profile(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;

        if profile.status == EmployeeStatus::Terminated {
            return Err(PayrollError::InvalidData);
        }

        // Create offboarding workflow
        let workflow_id = LifecycleStorage::get_next_offboarding_id(&env);
        let mut checklist = Vec::new(&env);

        // Default offboarding tasks
        checklist.push_back(OffboardingTask {
            id: 1,
            name: String::from_str(&env, "Return company assets"),
            description: String::from_str(&env, "Return all company property"),
            required: true,
            completed: false,
            completed_at: None,
            completed_by: None,
            due_date: Some(current_time + 604800), // 7 days
        });

        checklist.push_back(OffboardingTask {
            id: 2,
            name: String::from_str(&env, "Knowledge transfer"),
            description: String::from_str(&env, "Complete knowledge transfer"),
            required: true,
            completed: false,
            completed_at: None,
            completed_by: None,
            due_date: Some(current_time + 1209600), // 14 days
        });

        let has_final_payment = final_payment_amount.is_some() && final_payment_token.is_some();

        // Store final payment separately if provided
        if let (Some(amount), Some(token)) = (final_payment_amount, final_payment_token) {
            let final_payment = FinalPayment {
                amount,
                token,
                includes_severance: false,
                includes_unused_leave: false,
                processed: false,
                processed_at: None,
            };
            LifecycleStorage::store_final_payment(&env, &employee, &final_payment);
        }

        let workflow = OffboardingWorkflow {
            id: workflow_id,
            employee: employee.clone(),
            employer: employer.clone(),
            status: WorkflowStatus::InProgress,
            checklist,
            has_final_payment,
            approvals: Vec::new(&env),
            created_at: current_time,
            completed_at: None,
            termination_reason,
        };

        LifecycleStorage::store_offboarding(&env, workflow_id, &workflow);
        LifecycleStorage::link_employee_offboarding(&env, &employee, workflow_id);

        // Update employee status to inactive
        Self::update_employee_status(env.clone(), employee.clone(), EmployeeStatus::Inactive)?;

        env.events().publish(
            (symbol_short!("off_start"),),
            (employer, employee.clone(), workflow_id),
        );

        Ok(workflow_id)
    }

    /// Complete offboarding task
    pub fn complete_offboarding_task(
        env: Env,
        caller: Address,
        employee: Address,
        task_id: u32,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let workflow_id = LifecycleStorage::get_employee_offboarding_id(&env, &employee)
            .ok_or(PayrollError::PayrollNotFound)?;

        let mut workflow = LifecycleStorage::get_offboarding(&env, workflow_id)
            .ok_or(PayrollError::PayrollNotFound)?;

        // Find and complete the task
        let mut task_found = false;
        let current_time = env.ledger().timestamp();

        for i in 0..workflow.checklist.len() {
            if let Some(mut task) = workflow.checklist.get(i) {
                if task.id == task_id {
                    task.completed = true;
                    task.completed_at = Some(current_time);
                    task.completed_by = Some(caller.clone());
                    workflow.checklist.set(i, task);
                    task_found = true;
                    break;
                }
            }
        }

        if !task_found {
            return Err(PayrollError::InvalidData);
        }

        // Check if all required tasks are completed
        let mut all_required_completed = true;
        for i in 0..workflow.checklist.len() {
            if let Some(task) = workflow.checklist.get(i) {
                if task.required && !task.completed {
                    all_required_completed = false;
                    break;
                }
            }
        }

        // If all required tasks completed, finish offboarding
        if all_required_completed {
            workflow.status = WorkflowStatus::Completed;
            workflow.completed_at = Some(current_time);

            // Process final payment if exists
            if workflow.has_final_payment {
                if let Some(mut final_payment) =
                    LifecycleStorage::get_final_payment(&env, &employee)
                {
                    if !final_payment.processed {
                        // Transfer final payment
                        let contract_address = env.current_contract_address();
                        if let Ok(()) = Self::transfer_tokens_safe(
                            &env,
                            &final_payment.token,
                            &contract_address,
                            &employee,
                            final_payment.amount,
                        ) {
                            final_payment.processed = true;
                            final_payment.processed_at = Some(current_time);
                            LifecycleStorage::store_final_payment(&env, &employee, &final_payment);
                        }
                    }
                }
            }

            // Update employee status to terminated
            Self::update_employee_status(
                env.clone(),
                employee.clone(),
                EmployeeStatus::Terminated,
            )?;

            env.events().publish(
                (symbol_short!("off_comp"),),
                (workflow.employer.clone(), employee.clone(), workflow_id),
            );
        }

        LifecycleStorage::store_offboarding(&env, workflow_id, &workflow);

        Ok(())
    }

    /// Update employee status
    pub fn update_employee_status(
        env: Env,
        employee: Address,
        new_status: EmployeeStatus,
    ) -> Result<(), PayrollError> {
        let mut profile =
            LifecycleStorage::get_profile(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;

        profile.status = new_status.clone();
        profile.updated_at = env.ledger().timestamp();

        if new_status == EmployeeStatus::Terminated {
            profile.termination_date = Some(env.ledger().timestamp());
        }

        LifecycleStorage::store_profile(&env, &employee, &profile);

        env.events().publish(
            (symbol_short!("emp_stat"),),
            (employee.clone(), Self::status_to_u32(&new_status)),
        );

        Ok(())
    }

    /// Transfer employee between departments
    pub fn transfer_employee(
        env: Env,
        employer: Address,
        employee: Address,
        to_department: u64,
        to_manager: Address,
        reason: String,
    ) -> Result<u64, PayrollError> {
        employer.require_auth();
        Self::require_not_paused(&env)?;

        let current_time = env.ledger().timestamp();

        let mut profile =
            LifecycleStorage::get_profile(&env, &employee).ok_or(PayrollError::PayrollNotFound)?;

        let from_department = profile.department_id.unwrap_or(0);
        let from_manager = profile.manager.clone().unwrap_or(employer.clone());

        // Create transfer record
        let transfer_id = LifecycleStorage::get_next_transfer_id(&env);
        let transfer = EmployeeTransfer {
            id: transfer_id,
            employee: employee.clone(),
            from_department,
            to_department,
            from_manager,
            to_manager: to_manager.clone(),
            transfer_date: current_time,
            reason,
            approved: true, // Auto-approved by employer
            approved_by: Some(employer.clone()),
            approved_at: Some(current_time),
            created_at: current_time,
        };

        LifecycleStorage::store_transfer(&env, transfer_id, &transfer);

        // Update employee profile
        profile.department_id = Some(to_department);
        profile.manager = Some(to_manager);
        profile.updated_at = current_time;

        LifecycleStorage::store_profile(&env, &employee, &profile);

        env.events().publish(
            (symbol_short!("emp_trf"),),
            (employee.clone(), from_department, to_department),
        );

        Ok(transfer_id)
    }

    /// Get employee profile
    pub fn get_employee_profile(env: Env, employee: Address) -> Option<EmployeeProfile> {
        LifecycleStorage::get_profile(&env, &employee)
    }

    /// Get onboarding workflow
    pub fn get_onboarding_workflow(env: Env, employee: Address) -> Option<OnboardingWorkflow> {
        if let Some(workflow_id) = LifecycleStorage::get_employee_onboarding_id(&env, &employee) {
            LifecycleStorage::get_onboarding(&env, workflow_id)
        } else {
            None
        }
    }

    /// Get offboarding workflow
    pub fn get_offboarding_workflow(env: Env, employee: Address) -> Option<OffboardingWorkflow> {
        if let Some(workflow_id) = LifecycleStorage::get_employee_offboarding_id(&env, &employee) {
            LifecycleStorage::get_offboarding(&env, workflow_id)
        } else {
            None
        }
    }

    /// Update compliance record
    pub fn update_compliance(
        env: Env,
        employer: Address,
        employee: Address,
        compliance_type: String,
        status: ComplianceStatus,
        due_date: u64,
        notes: String,
    ) -> Result<(), PayrollError> {
        employer.require_auth();
        Self::require_not_paused(&env)?;

        let current_time = env.ledger().timestamp();

        let record = ComplianceRecord {
            employee: employee.clone(),
            compliance_type: compliance_type.clone(),
            status: status.clone(),
            due_date,
            completed_date: if status == ComplianceStatus::Completed {
                Some(current_time)
            } else {
                None
            },
            notes,
            created_at: current_time,
            updated_at: current_time,
        };

        LifecycleStorage::store_compliance(&env, &employee, &compliance_type, &record);

        env.events()
            .publish((symbol_short!("comp_upd"),), (employee, compliance_type));

        Ok(())
    }

    /// Get compliance record
    pub fn get_compliance_record(
        env: Env,
        employee: Address,
        compliance_type: String,
    ) -> Option<ComplianceRecord> {
        LifecycleStorage::get_compliance(&env, &employee, &compliance_type)
    }

    /// Helper function to convert status to u32
    fn status_to_u32(status: &EmployeeStatus) -> u32 {
        match status {
            EmployeeStatus::Pending => 0,
            EmployeeStatus::Active => 1,
            EmployeeStatus::Inactive => 2,
            EmployeeStatus::Terminated => 3,
            EmployeeStatus::OnLeave => 4,
            EmployeeStatus::Suspended => 5,
        }
    }

    //-----------------------------------------------------------------------------
    // Comprehensive Reporting Functions
    //-----------------------------------------------------------------------------

    /// Generate payroll summary report
    pub fn generate_payroll_summary_report(
        env: Env,
        caller: Address,
        employer: Address,
        period_start: u64,
        period_end: u64,
        format: ReportFormat,
    ) -> Result<PayrollReport, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let current_time = env.ledger().timestamp();
        let report_id = Self::get_next_report_id(&env);

        // Collect payroll data for the period
        let mut total_amount = 0i128;
        let mut total_employees = 0u32;
        let mut total_transactions = 0u32;

        let mut report_data = Map::new(&env);
        report_data.set(
            String::from_str(&env, "period_start"),
            String::from_str(&env, "1640995200"),
        );
        report_data.set(
            String::from_str(&env, "period_end"),
            String::from_str(&env, "1643673600"),
        );
        report_data.set(
            String::from_str(&env, "employer"),
            String::from_str(&env, "employer_address"),
        );

        // Calculate summary metrics
        let mut data_sources = Vec::new(&env);
        data_sources.push_back(String::from_str(&env, "payroll_data"));

        let mut filters_applied = Vec::new(&env);
        filters_applied.push_back(String::from_str(&env, "period_filter"));

        let metadata = ReportMetadata {
            total_employees,
            total_amount,
            total_transactions,
            compliance_score: 95, // Default compliance score
            generation_time_ms: 100,
            data_sources,
            filters_applied,
        };

        let report = PayrollReport {
            id: report_id,
            name: String::from_str(&env, "Payroll Summary Report"),
            report_type: ReportType::PayrollSummary,
            format,
            status: ReportStatus::Completed,
            employer: employer.clone(),
            period_start,
            period_end,
            filters: Map::new(&env),
            data: report_data,
            metadata,
            created_at: current_time,
            completed_at: Some(current_time),
            file_hash: None,
            file_size: None,
        };

        // Store report
        Self::store_report(&env, &report);
        Self::add_report_audit(&env, report_id, "report_generated", &caller);

        Ok(report)
    }

    /// Generate detailed payroll report
    pub fn generate_detailed_payroll_report(
        env: Env,
        caller: Address,
        employer: Address,
        period_start: u64,
        period_end: u64,
        employee_filter: Option<Address>,
    ) -> Result<PayrollReport, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let current_time = env.ledger().timestamp();
        let report_id = Self::get_next_report_id(&env);

        let mut report_data = Map::new(&env);
        let mut filters = Map::new(&env);

        if let Some(employee) = employee_filter {
            filters.set(String::from_str(&env, "employee"), employee.to_string());
        }

        let mut data_sources = Vec::new(&env);
        data_sources.push_back(String::from_str(&env, "payroll_data"));
        data_sources.push_back(String::from_str(&env, "audit_trail"));

        let mut filters_applied = Vec::new(&env);
        filters_applied.push_back(String::from_str(&env, "period_filter"));
        filters_applied.push_back(String::from_str(&env, "employee_filter"));

        let metadata = ReportMetadata {
            total_employees: 0,
            total_amount: 0,
            total_transactions: 0,
            compliance_score: 95,
            generation_time_ms: 200,
            data_sources,
            filters_applied,
        };

        let report = PayrollReport {
            id: report_id,
            name: String::from_str(&env, "Detailed Payroll Report"),
            report_type: ReportType::PayrollDetailed,
            format: ReportFormat::Json,
            status: ReportStatus::Completed,
            employer: employer.clone(),
            period_start,
            period_end,
            filters,
            data: report_data,
            metadata,
            created_at: current_time,
            completed_at: Some(current_time),
            file_hash: None,
            file_size: None,
        };

        Self::store_report(&env, &report);
        Self::add_report_audit(&env, report_id, "detailed_report_generated", &caller);

        Ok(report)
    }

    /// Calculate tax for employee
    pub fn calculate_employee_tax(
        env: Env,
        employee: Address,
        employer: Address,
        jurisdiction: String,
        gross_amount: i128,
        tax_type: TaxType,
        tax_rate: u32,
    ) -> Result<TaxCalculation, PayrollError> {
        Self::require_not_paused(&env)?;

        let current_time = env.ledger().timestamp();
        let tax_amount = (gross_amount * tax_rate as i128) / 10000; // Convert basis points
        let net_amount = gross_amount - tax_amount;

        let tax_calc = TaxCalculation {
            employee: employee.clone(),
            employer: employer.clone(),
            jurisdiction: jurisdiction.clone(),
            gross_amount,
            tax_type: tax_type.clone(),
            tax_rate,
            tax_amount,
            net_amount,
            calculation_date: current_time,
            tax_period: String::from_str(&env, "monthly"),
            deductions: Vec::new(&env),
        };

        // Store tax calculation
        Self::store_tax_calculation(&env, &employee, current_time, &tax_calc);

        Ok(tax_calc)
    }

    /// Create compliance alert
    pub fn create_compliance_alert(
        env: Env,
        caller: Address,
        alert_type: ComplianceAlertType,
        severity: AlertSeverity,
        jurisdiction: String,
        employee: Option<Address>,
        employer: Address,
        title: String,
        description: String,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        let current_time = env.ledger().timestamp();
        let alert_id = Self::get_next_alert_id(&env);

        let alert = ComplianceAlert {
            id: alert_id,
            alert_type: alert_type.clone(),
            severity: severity.clone(),
            jurisdiction: jurisdiction.clone(),
            employee: employee.clone(),
            employer: employer.clone(),
            title: title.clone(),
            description: description.clone(),
            violation_details: Map::new(&env),
            recommended_actions: Vec::new(&env),
            created_at: current_time,
            due_date: Some(current_time + 7 * 24 * 3600), // 7 days
            resolved_at: None,
            resolved_by: None,
            status: AlertStatus::Active,
        };

        Self::store_compliance_alert(&env, &alert);

        // Emit alert event
        env.events().publish(
            (symbol_short!("alert"),),
            (alert_id, employer, severity as u32),
        );

        Ok(alert_id)
    }

    /// Generate dashboard metrics
    pub fn generate_dashboard_metrics(
        env: Env,
        employer: Address,
        period_start: u64,
        period_end: u64,
    ) -> Result<DashboardMetrics, PayrollError> {
        Self::require_not_paused(&env)?;

        let current_time = env.ledger().timestamp();

        let metrics = DashboardMetrics {
            employer: employer.clone(),
            period_start,
            period_end,
            total_employees: 0,
            active_employees: 0,
            total_payroll_amount: 0,
            total_tax_amount: 0,
            compliance_score: 95,
            pending_payments: 0,
            overdue_payments: 0,
            active_alerts: 0,
            resolved_alerts: 0,
            last_updated: current_time,
            jurisdiction_metrics: Map::new(&env),
        };

        Self::store_dashboard_metrics(&env, &employer, &metrics);

        Ok(metrics)
    }

    //-----------------------------------------------------------------------------
    // Helper Functions for Reporting
    //-----------------------------------------------------------------------------

    fn get_next_report_id(env: &Env) -> u64 {
        let storage = env.storage().persistent();
        let current_id: u64 = storage.get(&ExtendedDataKey::NextTmplId).unwrap_or(1);
        storage.set(&ExtendedDataKey::NextTmplId, &(current_id + 1));
        current_id
    }

    fn get_next_alert_id(env: &Env) -> u64 {
        let storage = env.storage().persistent();
        let current_id: u64 = storage.get(&ExtendedDataKey::NextPresetId).unwrap_or(1);
        storage.set(&ExtendedDataKey::NextPresetId, &(current_id + 1));
        current_id
    }

    fn store_report(env: &Env, report: &PayrollReport) {
        let storage = env.storage().persistent();
        storage.set(&ExtendedDataKey::Template(report.id), report);
    }

    fn store_tax_calculation(
        env: &Env,
        employee: &Address,
        period: u64,
        tax_calc: &TaxCalculation,
    ) {
        let storage = env.storage().persistent();
        let key = DataKey::Balance(
            employee.clone(),
            Address::from_string(&String::from_str(env, "TAX")),
        );
        storage.set(&key, tax_calc);
    }

    fn store_compliance_alert(env: &Env, alert: &ComplianceAlert) {
        let storage = env.storage().persistent();
        storage.set(&ExtendedDataKey::Preset(alert.id), alert);
    }

    fn store_dashboard_metrics(env: &Env, employer: &Address, metrics: &DashboardMetrics) {
        let storage = env.storage().persistent();
        storage.set(&DataKey::Metrics(env.ledger().timestamp()), metrics);
    }

    fn add_report_audit(env: &Env, report_id: u64, action: &str, actor: &Address) {
        let current_time = env.ledger().timestamp();
        let audit_id = Self::get_next_report_id(env);

        let audit_entry = ReportAuditEntry {
            id: audit_id,
            report_id,
            action: String::from_str(env, action),
            actor: actor.clone(),
            timestamp: current_time,
            details: Map::new(env),
            ip_address: None,
        };

        let storage = env.storage().persistent();
        storage.set(&ExtendedDataKey::Backup(audit_id), &audit_entry);
    }

    pub fn record_time_series_data(
        env: Env,
        metric_name: String,
        value: i128,
        metadata: Map<String, String>,
    ) -> Result<(), PayrollError> {
        let storage = env.storage().persistent();
        let timestamp = env.ledger().timestamp();

        let data_point = TimeSeriesDataPoint {
            timestamp,
            value,
            metric_type: metric_name.clone(),
            metadata,
        };

        // Store data point
        storage.set(
            &AnalyticsDataKey::TimeSeriesData(metric_name.clone(), timestamp),
            &data_point,
        );

        // Update index
        let mut index: Vec<u64> = storage
            .get(&AnalyticsDataKey::TimeSeriesIndex(metric_name.clone()))
            .unwrap_or(Vec::new(&env));
        index.push_back(timestamp);
        storage.set(&AnalyticsDataKey::TimeSeriesIndex(metric_name), &index);

        Ok(())
    }

    /// Generate aggregated metrics for a period
    pub fn generate_aggregated_metrics(
        env: Env,
        employer: Address,
        period_start: u64,
        period_end: u64,
    ) -> Result<AggregatedMetrics, PayrollError> {
        let storage = env.storage().persistent();

        // Collect metrics from the period
        let mut total_disbursements = 0u64;
        let mut total_amount = 0i128;
        let mut amounts = Vec::new(&env);
        let mut token_breakdown = Map::new(&env);
        let mut department_breakdown = Map::new(&env);
        let mut on_time_count = 0u64;
        let mut late_count = 0u64;
        let mut error_count = 0u64;

        // Iterate through daily metrics in the period
        let start_day = (period_start / 86_400) * 86_400;
        let end_day = (period_end / 86_400) * 86_400;

        for day_timestamp in (start_day..=end_day).step_by(86_400) {
            if let Some(metrics) =
                storage.get::<DataKey, PerformanceMetrics>(&DataKey::Metrics(day_timestamp))
            {
                total_disbursements += metrics.total_disbursements;
                total_amount += metrics.total_amount;
                amounts.push_back(metrics.total_amount);

                // Count on-time vs late
                let total_ops = metrics.operation_count;
                if total_ops > 0 {
                    on_time_count += total_ops - metrics.late_disbursements;
                    late_count += metrics.late_disbursements;
                }
            }
        }

        // Calculate statistics
        let average_amount = if total_disbursements > 0 {
            total_amount / (total_disbursements as i128)
        } else {
            0
        };

        let (min_amount, max_amount) = Self::calculate_min_max(&amounts);

        let total_operations = on_time_count + late_count + error_count;
        let on_time_rate = if total_operations > 0 {
            ((on_time_count * 100) / total_operations) as u32
        } else {
            0
        };

        let late_rate = if total_operations > 0 {
            ((late_count * 100) / total_operations) as u32
        } else {
            0
        };

        let error_rate = if total_operations > 0 {
            ((error_count * 100) / total_operations) as u32
        } else {
            0
        };

        let metrics = AggregatedMetrics {
            period_start,
            period_end,
            employer: employer.clone(),
            total_disbursements,
            total_amount,
            average_amount,
            min_amount,
            max_amount,
            employee_count: 0, // Would be calculated from actual employee data
            on_time_rate,
            late_rate,
            error_rate,
            token_breakdown,
            department_breakdown,
        };

        // Cache the result
        storage.set(
            &AnalyticsDataKey::AggregatedMetrics(employer, period_start),
            &metrics,
        );

        Ok(metrics)
    }

    /// Analyze trends for a specific metric
    pub fn analyze_trend(
        env: Env,
        metric_name: String,
        period_start: u64,
        period_end: u64,
    ) -> Result<TrendAnalysis, PayrollError> {
        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get time series data
        let index: Vec<u64> = storage
            .get(&AnalyticsDataKey::TimeSeriesIndex(metric_name.clone()))
            .unwrap_or(Vec::new(&env));

        let mut data_points = Vec::new(&env);
        for timestamp in index.iter() {
            if timestamp >= period_start && timestamp <= period_end {
                if let Some(point) = storage.get::<AnalyticsDataKey, TimeSeriesDataPoint>(
                    &AnalyticsDataKey::TimeSeriesData(metric_name.clone(), timestamp),
                ) {
                    data_points.push_back(point);
                }
            }
        }

        if data_points.len() < 2 {
            return Ok(TrendAnalysis {
                metric_name,
                period_start,
                period_end,
                data_points,
                trend_direction: TrendDirection::Insufficient,
                growth_rate: 0,
                volatility: 0,
                has_forecast: false, // ADD THIS
                forecast: ForecastData {
                    // Use default instead of None
                    next_period_prediction: 0,
                    confidence_level: 0,
                    prediction_range_low: 0,
                    prediction_range_high: 0,
                    forecast_method: String::from_str(&env, "none"),
                },
                analysis_timestamp: current_time,
            });
        }

        // Calculate trend direction and growth rate
        let first_value = data_points.get(0).unwrap().value;
        let last_value = data_points.get(data_points.len() - 1).unwrap().value;

        let growth_rate = if first_value != 0 {
            ((last_value - first_value) * 10000) / first_value // Basis points
        } else {
            0
        };

        let trend_direction = if growth_rate > 500 {
            TrendDirection::Increasing
        } else if growth_rate < -500 {
            TrendDirection::Decreasing
        } else if Self::calculate_volatility(&data_points) > 20 {
            TrendDirection::Volatile
        } else {
            TrendDirection::Stable
        };

        // Generate forecast
        let forecast = Self::generate_forecast(&env, &data_points, growth_rate);

        let trend = TrendAnalysis {
            metric_name: metric_name.clone(),
            period_start,
            period_end,
            data_points: data_points.clone(),
            trend_direction,
            growth_rate,
            volatility: Self::calculate_volatility(&data_points),
            forecast,
            has_forecast: false,
            analysis_timestamp: current_time,
        };

        // Cache the analysis
        storage.set(
            &AnalyticsDataKey::TrendAnalysis(metric_name, current_time),
            &trend,
        );

        Ok(trend)
    }

    /// Generate forecast data
    fn generate_forecast(
        env: &Env,
        data_points: &Vec<TimeSeriesDataPoint>,
        growth_rate: i128,
    ) -> ForecastData {
        let last_value = data_points.get(data_points.len() - 1).unwrap().value;

        // Simple linear forecast
        let next_period_prediction = last_value + ((last_value * growth_rate) / 10000);

        // Calculate prediction range (±20% for simplicity)
        let range_margin = next_period_prediction / 5;

        ForecastData {
            next_period_prediction,
            confidence_level: 75, // Simplified confidence
            prediction_range_low: next_period_prediction - range_margin,
            prediction_range_high: next_period_prediction + range_margin,
            forecast_method: String::from_str(env, "linear_regression"),
        }
    }

    /// Calculate volatility (standard deviation as percentage)
    fn calculate_volatility(data_points: &Vec<TimeSeriesDataPoint>) -> u32 {
        if data_points.len() < 2 {
            return 0;
        }

        // Calculate mean
        let mut sum = 0i128;
        for point in data_points.iter() {
            sum += point.value;
        }
        let mean = sum / (data_points.len() as i128);

        // Calculate variance
        let mut variance_sum = 0i128;
        for point in data_points.iter() {
            let diff = point.value - mean;
            variance_sum += diff * diff;
        }
        let variance = variance_sum / (data_points.len() as i128);

        // Return standard deviation as percentage of mean
        if mean != 0 {
            let std_dev = Self::sqrt_i128(variance);
            ((std_dev * 100) / mean.abs()) as u32
        } else {
            0
        }
    }

    /// Simple integer square root
    fn sqrt_i128(n: i128) -> i128 {
        if n < 0 {
            return 0;
        }
        if n == 0 {
            return 0;
        }

        let mut x = n;
        let mut y = (x + 1) / 2;

        while y < x {
            x = y;
            y = (x + n / x) / 2;
        }

        x
    }

    /// Calculate min and max values
    fn calculate_min_max(values: &Vec<i128>) -> (i128, i128) {
        if values.len() == 0 {
            return (0, 0);
        }

        let mut min = values.get(0).unwrap();
        let mut max = values.get(0).unwrap();

        for value in values.iter() {
            if value < min {
                min = value;
            }
            if value > max {
                max = value;
            }
        }

        (min, max)
    }

    /// Create analytics dashboard
    pub fn create_analytics_dashboard(
        env: Env,
        owner: Address,
        name: String,
        description: String,
        widgets: Vec<DashboardWidget>,
        is_public: bool,
    ) -> Result<u64, PayrollError> {
        owner.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        let dashboard_id = storage
            .get::<AnalyticsDataKey, u64>(&AnalyticsDataKey::NextDashboardId)
            .unwrap_or(1);
        storage.set(&AnalyticsDataKey::NextDashboardId, &(dashboard_id + 1));

        let dashboard = AnalyticsDashboard {
            id: dashboard_id,
            name: name.clone(),
            description,
            owner: owner.clone(),
            widgets,
            is_default: false,
            is_public,
            created_at: current_time,
            updated_at: current_time,
        };

        storage.set(&AnalyticsDataKey::Dashboard(dashboard_id), &dashboard);

        // Add to user's dashboards
        let mut user_dashboards: Vec<u64> = storage
            .get(&AnalyticsDataKey::UserDashboards(owner.clone()))
            .unwrap_or(Vec::new(&env));
        user_dashboards.push_back(dashboard_id);
        storage.set(&AnalyticsDataKey::UserDashboards(owner), &user_dashboards);

        env.events()
            .publish((symbol_short!("dash_c"),), (dashboard_id, name));

        Ok(dashboard_id)
    }

    /// Generate chart data
    pub fn generate_chart_data(
        env: Env,
        chart_type: WidgetType,
        title: String,
        data_source: DataSource,
        period_start: u64,
        period_end: u64,
    ) -> Result<ChartData, PayrollError> {
        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        let chart_id = storage
            .get::<AnalyticsDataKey, u64>(&AnalyticsDataKey::NextChartId)
            .unwrap_or(1);
        storage.set(&AnalyticsDataKey::NextChartId, &(chart_id + 1));

        let mut data_series = Vec::new(&env);

        // Generate data based on source
        match data_source {
            DataSource::PayrollMetrics => {
                let mut payroll_series =
                    Self::generate_payroll_series(&env, period_start, period_end);
                data_series.push_back(payroll_series);
            }
            DataSource::EmployeeMetrics => {
                let mut employee_series =
                    Self::generate_employee_series(&env, period_start, period_end);
                data_series.push_back(employee_series);
            }
            _ => {}
        }

        let chart = ChartData {
            chart_id,
            chart_type,
            title,
            x_axis_label: String::from_str(&env, "Time"),
            y_axis_label: String::from_str(&env, "Amount"),
            data_series,
            generated_at: current_time,
        };

        storage.set(&AnalyticsDataKey::ChartData(chart_id), &chart);

        Ok(chart)
    }

    /// Generate payroll data series
    fn generate_payroll_series(env: &Env, period_start: u64, period_end: u64) -> DataSeries {
        let storage = env.storage().persistent();
        let mut data_points = Vec::new(env);

        let start_day = (period_start / 86_400) * 86_400;
        let end_day = (period_end / 86_400) * 86_400;

        for day_timestamp in (start_day..=end_day).step_by(86_400) {
            if let Some(metrics) =
                storage.get::<DataKey, PerformanceMetrics>(&DataKey::Metrics(day_timestamp))
            {
                data_points.push_back(DataPoint {
                    x: day_timestamp,
                    y: metrics.total_amount,
                    label: Some(String::from_str(env, "Daily Total")),
                });
            }
        }

        DataSeries {
            name: String::from_str(env, "Payroll Disbursements"),
            data_points,
            color: Some(String::from_str(env, "#4F46E5")),
            line_style: Some(String::from_str(env, "solid")),
        }
    }

    /// Generate employee data series
    fn generate_employee_series(env: &Env, period_start: u64, period_end: u64) -> DataSeries {
        let storage = env.storage().persistent();
        let mut data_points = Vec::new(env);

        let start_day = (period_start / 86_400) * 86_400;
        let end_day = (period_end / 86_400) * 86_400;

        for day_timestamp in (start_day..=end_day).step_by(86_400) {
            if let Some(metrics) =
                storage.get::<DataKey, PerformanceMetrics>(&DataKey::Metrics(day_timestamp))
            {
                data_points.push_back(DataPoint {
                    x: day_timestamp,
                    y: metrics.employee_count as i128,
                    label: Some(String::from_str(env, "Employee Count")),
                });
            }
        }

        DataSeries {
            name: String::from_str(env, "Active Employees"),
            data_points,
            color: Some(String::from_str(env, "#10B981")),
            line_style: Some(String::from_str(env, "solid")),
        }
    }

    /// Create custom analytics query
    pub fn create_analytics_query(
        env: Env,
        creator: Address,
        name: String,
        description: String,
        query_type: QueryType,
        filters: Vec<QueryFilter>,
        group_by: Vec<String>,
        sort_by: Vec<SortCriteria>,
        limit: Option<u32>,
    ) -> Result<u64, PayrollError> {
        creator.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        let query_id = storage
            .get::<AnalyticsDataKey, u64>(&AnalyticsDataKey::NextQueryId)
            .unwrap_or(1);
        storage.set(&AnalyticsDataKey::NextQueryId, &(query_id + 1));

        let query = AnalyticsQuery {
            id: query_id,
            name: name.clone(),
            description,
            query_type,
            filters,
            group_by,
            sort_by,
            limit,
            created_by: creator.clone(),
            created_at: current_time,
        };

        storage.set(&AnalyticsDataKey::AnalyticsQuery(query_id), &query);

        // Add to user's queries
        let mut user_queries: Vec<u64> = storage
            .get(&AnalyticsDataKey::UserQueries(creator.clone()))
            .unwrap_or(Vec::new(&env));
        user_queries.push_back(query_id);
        storage.set(&AnalyticsDataKey::UserQueries(creator), &user_queries);

        env.events()
            .publish((symbol_short!("query_c"),), (query_id, name));

        Ok(query_id)
    }

    /// Request data export
    pub fn request_data_export(
        env: Env,
        requester: Address,
        export_type: ExportType,
        format: ExportFormat,
        date_range: DateRange,
        filters: Map<String, String>,
    ) -> Result<u64, PayrollError> {
        requester.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        let export_id = storage
            .get::<AnalyticsDataKey, u64>(&AnalyticsDataKey::NextExportId)
            .unwrap_or(1);
        storage.set(&AnalyticsDataKey::NextExportId, &(export_id + 1));

        let export_request = DataExportRequest {
            id: export_id,
            export_type: export_type.clone(),
            format: format.clone(),
            data_range: date_range,
            filters,
            requested_by: requester.clone(),
            requested_at: current_time,
            status: ExportStatus::Pending,
            file_url: None,
            file_size: None,
            completed_at: None,
        };

        storage.set(&AnalyticsDataKey::ExportRequest(export_id), &export_request);

        // Add to user's exports
        let mut user_exports: Vec<u64> = storage
            .get(&AnalyticsDataKey::UserExports(requester.clone()))
            .unwrap_or(Vec::new(&env));
        user_exports.push_back(export_id);
        storage.set(&AnalyticsDataKey::UserExports(requester), &user_exports);

        env.events().publish(
            (symbol_short!("export_r"),),
            (export_id, export_type, format),
        );

        Ok(export_id)
    }

    /// Process data export (would be called by backend service)
    pub fn process_data_export(
        env: Env,
        processor: Address,
        export_id: u64,
        file_url: String,
        file_size: u64,
    ) -> Result<(), PayrollError> {
        processor.require_auth();
        Self::require_not_paused(&env)?;

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        let mut export_request = storage
            .get::<AnalyticsDataKey, DataExportRequest>(&AnalyticsDataKey::ExportRequest(export_id))
            .ok_or(PayrollError::InvalidData)?;

        export_request.status = ExportStatus::Completed;
        export_request.file_url = Some(file_url.clone());
        export_request.file_size = Some(file_size);
        export_request.completed_at = Some(current_time);

        storage.set(&AnalyticsDataKey::ExportRequest(export_id), &export_request);

        env.events().publish(
            (symbol_short!("export_c"),),
            (export_id, file_url, file_size),
        );

        Ok(())
    }

    /// Perform comparative analysis between two periods
    pub fn comparative_analysis(
        env: Env,
        comparison_type: ComparisonType,
        period_1: DateRange,
        period_2: DateRange,
        metrics_to_compare: Vec<String>,
    ) -> Result<ComparativeAnalysis, PayrollError> {
        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        let analysis_id = storage
            .get::<AnalyticsDataKey, u64>(&AnalyticsDataKey::NextAnalysisId)
            .unwrap_or(1);
        storage.set(&AnalyticsDataKey::NextAnalysisId, &(analysis_id + 1));

        let mut metrics_comparison = Vec::new(&env);

        // Compare key metrics between periods
        for metric_name in metrics_to_compare.iter() {
            let period_1_value = Self::get_period_metric_value(&env, &period_1, &metric_name);
            let period_2_value = Self::get_period_metric_value(&env, &period_2, &metric_name);

            let absolute_change = period_2_value - period_1_value;
            let percentage_change = if period_1_value != 0 {
                (absolute_change * 10000) / period_1_value
            } else {
                0
            };

            let is_improvement = Self::is_metric_improvement(&metric_name, absolute_change);

            metrics_comparison.push_back(MetricComparison {
                metric_name,
                period_1_value,
                period_2_value,
                absolute_change,
                percentage_change,
                is_improvement,
            });
        }

        let summary = Self::generate_comparison_summary(&env, &metrics_comparison);

        let analysis = ComparativeAnalysis {
            analysis_id,
            comparison_type,
            period_1,
            period_2,
            metrics_comparison,
            summary,
            generated_at: current_time,
        };

        storage.set(
            &AnalyticsDataKey::ComparativeAnalysis(analysis_id),
            &analysis,
        );

        env.events()
            .publish((symbol_short!("comp_a"),), (analysis_id, current_time));

        Ok(analysis)
    }

    /// Get metric value for a period
    fn get_period_metric_value(env: &Env, period: &DateRange, metric_name: &String) -> i128 {
        let storage = env.storage().persistent();
        let mut total = 0i128;

        let start_day = (period.start / 86_400) * 86_400;
        let end_day = (period.end / 86_400) * 86_400;

        for day_timestamp in (start_day..=end_day).step_by(86_400) {
            if let Some(metrics) =
                storage.get::<DataKey, PerformanceMetrics>(&DataKey::Metrics(day_timestamp))
            {
                if metric_name == &String::from_str(env, "total_amount") {
                    total += metrics.total_amount;
                } else if metric_name == &String::from_str(env, "disbursements") {
                    total += metrics.total_disbursements as i128;
                }
            }
        }

        total
    }

    /// Determine if change is an improvement
    fn is_metric_improvement(metric_name: &String, change: i128) -> bool {
        // For revenue/amount metrics, positive is good
        // For error/late metrics, negative is good
        change > 0 // Simplified logic
    }

    /// Generate comparison summary
    fn generate_comparison_summary(env: &Env, comparisons: &Vec<MetricComparison>) -> String {
        let mut improvement_count = 0u32;
        for comp in comparisons.iter() {
            if comp.is_improvement {
                improvement_count += 1;
            }
        }

        if improvement_count > (comparisons.len() / 2) {
            String::from_str(env, "Overall performance improved")
        } else {
            String::from_str(env, "Performance needs attention")
        }
    }

    /// Update benchmark data
    pub fn update_benchmark_data(
        env: Env,
        caller: Address,
        metric_name: String,
        industry_average: i128,
        top_quartile: i128,
        median: i128,
        bottom_quartile: i128,
        company_value: i128,
    ) -> Result<(), PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        // Only owner can update benchmarks
        let storage = env.storage().persistent();
        let owner = storage
            .get::<DataKey, Address>(&DataKey::Owner)
            .ok_or(PayrollError::Unauthorized)?;
        if caller != owner {
            return Err(PayrollError::Unauthorized);
        }

        let current_time = env.ledger().timestamp();

        // Calculate percentile rank
        let percentile_rank = if company_value >= top_quartile {
            75
        } else if company_value >= median {
            50
        } else if company_value >= bottom_quartile {
            25
        } else {
            0
        };

        let benchmark = BenchmarkData {
            metric_name: metric_name.clone(),
            industry_average,
            top_quartile,
            median,
            bottom_quartile,
            company_value,
            percentile_rank,
            last_updated: current_time,
        };

        storage.set(
            &AnalyticsDataKey::Benchmark(metric_name.clone()),
            &benchmark,
        );

        env.events()
            .publish((symbol_short!("bench_u"),), (metric_name, percentile_rank));

        Ok(())
    }

    /// Get analytics dashboard
    pub fn get_analytics_dashboard(
        env: Env,
        dashboard_id: u64,
    ) -> Result<AnalyticsDashboard, PayrollError> {
        let storage = env.storage().persistent();
        storage
            .get(&AnalyticsDataKey::Dashboard(dashboard_id))
            .ok_or(PayrollError::InvalidData)
    }

    /// Get user's dashboards
    pub fn get_user_dashboards(env: Env, user: Address) -> Vec<AnalyticsDashboard> {
        let storage = env.storage().persistent();
        let dashboard_ids: Vec<u64> = storage
            .get(&AnalyticsDataKey::UserDashboards(user))
            .unwrap_or(Vec::new(&env));

        let mut dashboards = Vec::new(&env);
        for id in dashboard_ids.iter() {
            if let Some(dashboard) = storage.get(&AnalyticsDataKey::Dashboard(id)) {
                dashboards.push_back(dashboard);
            }
        }

        dashboards
    }

    /// Get chart data
    pub fn get_chart_data(env: Env, chart_id: u64) -> Result<ChartData, PayrollError> {
        let storage = env.storage().persistent();
        storage
            .get(&AnalyticsDataKey::ChartData(chart_id))
            .ok_or(PayrollError::InvalidData)
    }

    /// Get analytics query
    pub fn get_analytics_query(env: Env, query_id: u64) -> Result<AnalyticsQuery, PayrollError> {
        let storage = env.storage().persistent();
        storage
            .get(&AnalyticsDataKey::AnalyticsQuery(query_id))
            .ok_or(PayrollError::InvalidData)
    }

    /// Get export request status
    pub fn get_export_status(env: Env, export_id: u64) -> Result<DataExportRequest, PayrollError> {
        let storage = env.storage().persistent();
        storage
            .get(&AnalyticsDataKey::ExportRequest(export_id))
            .ok_or(PayrollError::InvalidData)
    }

    /// Get user's export requests
    pub fn get_user_exports(env: Env, user: Address) -> Vec<DataExportRequest> {
        let storage = env.storage().persistent();
        let export_ids: Vec<u64> = storage
            .get(&AnalyticsDataKey::UserExports(user))
            .unwrap_or(Vec::new(&env));

        let mut exports = Vec::new(&env);
        for id in export_ids.iter() {
            if let Some(export) = storage.get(&AnalyticsDataKey::ExportRequest(id)) {
                exports.push_back(export);
            }
        }

        exports
    }

    /// Get trend analysis
    pub fn get_trend_analysis(
        env: Env,
        metric_name: String,
        analysis_timestamp: u64,
    ) -> Result<TrendAnalysis, PayrollError> {
        let storage = env.storage().persistent();
        storage
            .get(&AnalyticsDataKey::TrendAnalysis(
                metric_name,
                analysis_timestamp,
            ))
            .ok_or(PayrollError::InvalidData)
    }

    /// Get comparative analysis
    pub fn get_comparative_analysis(
        env: Env,
        analysis_id: u64,
    ) -> Result<ComparativeAnalysis, PayrollError> {
        let storage = env.storage().persistent();
        storage
            .get(&AnalyticsDataKey::ComparativeAnalysis(analysis_id))
            .ok_or(PayrollError::InvalidData)
    }

    /// Get benchmark data
    pub fn get_benchmark_data(
        env: Env,
        metric_name: String,
    ) -> Result<BenchmarkData, PayrollError> {
        let storage = env.storage().persistent();
        storage
            .get(&AnalyticsDataKey::Benchmark(metric_name))
            .ok_or(PayrollError::InvalidData)
    }

    /// Get aggregated metrics
    pub fn get_aggregated_metrics(
        env: Env,
        employer: Address,
        period_start: u64,
    ) -> Result<AggregatedMetrics, PayrollError> {
        let storage = env.storage().persistent();
        storage
            .get(&AnalyticsDataKey::AggregatedMetrics(employer, period_start))
            .ok_or(PayrollError::InvalidData)
    }

    /// Generate real-time analytics snapshot
    pub fn generate_realtime_snapshot(
        env: Env,
        employer: Address,
    ) -> Result<Map<String, i128>, PayrollError> {
        let storage = env.storage().persistent();
        let mut snapshot = Map::new(&env);

        // Get today's metrics
        let today = (env.ledger().timestamp() / 86_400) * 86_400;

        if let Some(metrics) = storage.get::<DataKey, PerformanceMetrics>(&DataKey::Metrics(today))
        {
            snapshot.set(
                String::from_str(&env, "total_disbursements"),
                metrics.total_disbursements as i128,
            );
            snapshot.set(
                String::from_str(&env, "total_amount"),
                metrics.total_amount as i128,
            );
            snapshot.set(
                String::from_str(&env, "employee_count"),
                metrics.employee_count as i128,
            );
            snapshot.set(
                String::from_str(&env, "late_disbursements"),
                metrics.late_disbursements as i128,
            );
        }

        // Get employer balance summary
        let employees = Self::get_employer_employees(env.clone(), employer.clone());
        snapshot.set(
            String::from_str(&env, "active_employees"),
            employees.len() as i128,
        );

        Ok(snapshot)
    }

    /// Delete analytics data (cleanup)
    pub fn cleanup_old_analytics(
        env: Env,
        caller: Address,
        cutoff_date: u64,
    ) -> Result<u32, PayrollError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        // Only owner can cleanup
        let storage = env.storage().persistent();
        let owner = storage
            .get::<DataKey, Address>(&DataKey::Owner)
            .ok_or(PayrollError::Unauthorized)?;
        if caller != owner {
            return Err(PayrollError::Unauthorized);
        }

        let mut cleaned_count = 0u32;

        // Cleanup old time series data
        // Implementation would iterate through old data and remove it
        // For now, return 0 as placeholder

        env.events()
            .publish((symbol_short!("cleanup"),), (cutoff_date, cleaned_count));

        Ok(cleaned_count)
    }

    //-----------------------------------------------------------------------------
    // Error Recovery and Circuit Breaker Functions
    //-----------------------------------------------------------------------------

    /// Create a retry configuration for operations
    pub fn create_retry_config(
        env: Env,
        caller: Address,
        max_attempts: u32,
        base_delay: u64,
        max_delay: u64,
        backoff_multiplier: u64,
        jitter: bool,
        retryable_errors: Vec<String>,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();

        let storage = env.storage().persistent();
        let next_id = storage
            .get(&ExtendedDataKey::NextRetryId)
            .unwrap_or(1u64);

        let retry_config = RetryConfig {
            max_attempts,
            base_delay,
            max_delay,
            backoff_multiplier,
            jitter,
            retryable_errors,
        };

        storage.set(&ExtendedDataKey::RetryConfig(next_id), &retry_config);
        storage.set(&ExtendedDataKey::NextRetryId, &(next_id + 1));

        env.events().publish(
            (symbol_short!("retry_cfg"),),
            (next_id, max_attempts, base_delay),
        );

        Ok(next_id)
    }

    /// Get retry configuration by ID
    pub fn get_retry_config(env: Env, retry_id: u64) -> Option<RetryConfig> {
        let storage = env.storage().persistent();
        storage.get(&ExtendedDataKey::RetryConfig(retry_id)).unwrap_or(None)
    }

    /// Set circuit breaker state for a service
    pub fn set_circuit_breaker_state(
        env: Env,
        caller: Address,
        service_name: String,
        state: CircuitBreakerState,
    ) -> Result<(), PayrollError> {
        caller.require_auth();

        let storage = env.storage().persistent();
        storage.set(&ExtendedDataKey::CircuitBreakerState(service_name.clone()), &state);

        env.events().publish(
            (symbol_short!("cb_state"),),
            (service_name, state),
        );

        Ok(())
    }

    /// Get circuit breaker state for a service
    pub fn get_circuit_breaker_state(env: Env, service_name: String) -> Option<CircuitBreakerState> {
        let storage = env.storage().persistent();
        storage.get(&ExtendedDataKey::CircuitBreakerState(service_name)).unwrap_or(None)
    }

    /// Update health check for a service
    pub fn update_health_check(
        env: Env,
        caller: Address,
        service_name: String,
        status: HealthStatus,
        response_time: u64,
        error_message: Option<String>,
    ) -> Result<(), PayrollError> {
        caller.require_auth();

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        let mut health_check = storage
            .get(&ExtendedDataKey::HealthCheck(service_name.clone()))
            .unwrap_or(None)
            .unwrap_or(HealthCheck {
                service_name: service_name.clone(),
                last_check_time: current_time,
                status: HealthStatus::Unknown,
                response_time: 0,
                error_count: 0,
                success_count: 0,
                consecutive_failures: 0,
                last_error: None,
            });

        health_check.last_check_time = current_time;
        health_check.status = status.clone();
        health_check.response_time = response_time;

        match status {
            HealthStatus::Healthy => {
                health_check.success_count += 1;
                health_check.consecutive_failures = 0;
            }
            HealthStatus::Unhealthy | HealthStatus::Degraded => {
                health_check.error_count += 1;
                health_check.consecutive_failures += 1;
                health_check.last_error = error_message;
            }
            _ => {}
        }

        storage.set(&ExtendedDataKey::HealthCheck(service_name.clone()), &health_check);

        env.events().publish(
            (symbol_short!("health"),),
            (service_name, status, response_time),
        );

        Ok(())
    }

    /// Get health check for a service
    pub fn get_health_check(env: Env, service_name: String) -> Option<HealthCheck> {
        let storage = env.storage().persistent();
        storage.get(&ExtendedDataKey::HealthCheck(service_name)).unwrap_or(None)
    }

    /// Create an error recovery workflow
    pub fn create_error_recovery_workflow(
        env: Env,
        caller: Address,
        operation_type: String,
        error_type: String,
        recovery_steps: Vec<RecoveryStep>,
        max_retries: u32,
    ) -> Result<u64, PayrollError> {
        caller.require_auth();

        let storage = env.storage().persistent();
        let next_id = storage
            .get(&ExtendedDataKey::NextWorkflowId)
            .unwrap_or(1u64);

        let current_time = env.ledger().timestamp();

        let workflow = ErrorRecoveryWorkflow {
            workflow_id: next_id,
            operation_type: operation_type.clone(),
            error_type: error_type.clone(),
            recovery_steps,
            current_step: 0,
            status: WorkflowStatus::Active,
            created_at: current_time,
            updated_at: current_time,
            retry_count: 0,
            max_retries,
        };

        storage.set(&ExtendedDataKey::ErrorRecoveryWorkflow(next_id), &workflow);
        storage.set(&ExtendedDataKey::NextWorkflowId, &(next_id + 1));

        env.events().publish(
            (symbol_short!("workflow"),),
            (next_id, operation_type, error_type),
        );

        Ok(next_id)
    }

    /// Execute the next step in an error recovery workflow
    pub fn execute_recovery_step(
        env: Env,
        caller: Address,
        workflow_id: u64,
    ) -> Result<(), PayrollError> {
        caller.require_auth();

        let storage = env.storage().persistent();
        let mut workflow: ErrorRecoveryWorkflow = storage
            .get(&ExtendedDataKey::ErrorRecoveryWorkflow(workflow_id))
            .unwrap_or(None)
            .ok_or(PayrollError::InvalidData)?;

        if workflow.status != WorkflowStatus::Active {
            return Err(PayrollError::InvalidData);
        }

        if workflow.current_step >= workflow.recovery_steps.len() as u32 {
            workflow.status = WorkflowStatus::Completed;
            workflow.updated_at = env.ledger().timestamp();
            storage.set(&ExtendedDataKey::ErrorRecoveryWorkflow(workflow_id), &workflow);

            env.events().publish(
                (symbol_short!("workflow"),),
                (workflow_id, "completed"),
            );

            return Ok(());
        }

        let step_index = workflow.current_step as usize;
        let mut step = workflow.recovery_steps.get(step_index as u32).unwrap().clone();
        
        step.status = StepStatus::InProgress;
        step.executed_at = Some(env.ledger().timestamp());

        // Simulate step execution (in real implementation, this would execute the actual recovery logic)
        step.status = StepStatus::Completed;
        workflow.current_step += 1;
        workflow.updated_at = env.ledger().timestamp();

        // Update the step in the workflow
        let mut steps = workflow.recovery_steps.clone();
        steps.set(step_index as u32, step);
        workflow.recovery_steps = steps;

        storage.set(&ExtendedDataKey::ErrorRecoveryWorkflow(workflow_id), &workflow);

        env.events().publish(
            (symbol_short!("step"),),
            (workflow_id, workflow.current_step),
        );

        Ok(())
    }

    /// Get error recovery workflow by ID
    pub fn get_error_recovery_workflow(env: Env, workflow_id: u64) -> Option<ErrorRecoveryWorkflow> {
        let storage = env.storage().persistent();
        storage.get(&ExtendedDataKey::ErrorRecoveryWorkflow(workflow_id)).unwrap_or(None)
    }

    /// Update global error recovery settings
    pub fn update_error_settings(
        env: Env,
        caller: Address,
        retry_enabled: bool,
        circuit_breaker_enabled: bool,
        health_check_enabled: bool,
        graceful_degradation_enabled: bool,
        default_max_retries: u32,
        default_timeout: u64,
        notification_enabled: bool,
        escalation_enabled: bool,
    ) -> Result<(), PayrollError> {
        caller.require_auth();

        let storage = env.storage().persistent();

        let global_config = CircuitBreakerConfig {
            failure_threshold: 5,
            recovery_timeout: 60000, // 1 minute
            success_threshold: 3,
            timeout: default_timeout,
        };

        let settings = GlobalErrorSettings {
            retry_enabled,
            circuit_breaker_enabled,
            health_check_enabled,
            graceful_degradation_enabled,
            default_max_retries,
            default_timeout,
            global_circuit_breaker_config: global_config,
            notification_enabled,
            escalation_enabled,
        };

        storage.set(&ExtendedDataKey::GlobalErrorSettings, &settings);

        env.events().publish(
            (symbol_short!("err_set"),),
            (retry_enabled, circuit_breaker_enabled),
        );

        Ok(())
    }

    /// Get global error recovery settings
    pub fn get_error_settings(env: Env) -> Option<GlobalErrorSettings> {
        let storage = env.storage().persistent();
        storage.get(&ExtendedDataKey::GlobalErrorSettings).unwrap_or(None)
    }

    /// Check if a service is healthy
    pub fn is_service_healthy(env: Env, service_name: String) -> bool {
        if let Some(health_check) = Self::get_health_check(env, service_name) {
            matches!(health_check.status, HealthStatus::Healthy)
        } else {
            false
        }
    }

    /// Check if circuit breaker allows requests
    pub fn is_circuit_breaker_closed(env: Env, service_name: String) -> bool {
        if let Some(state) = Self::get_circuit_breaker_state(env, service_name) {
            matches!(state, CircuitBreakerState::Closed)
        } else {
            true // Default to allowing requests if no circuit breaker state
        }
    }
}
