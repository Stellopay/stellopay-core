#![no_std]

//! # Compliance Reporting Contract
//!
//! Provides on-chain, tamper-evident compliance reporting structures so that
//! off-chain indexers can reconstruct reporting periods without trusting
//! centralized databases alone.
//!
//! ## Security Model
//!
//! - Only the contract admin can authorize publishers.
//! - Only authorized publishers (or the employer themselves) may log records.
//! - Records are assigned a monotonically increasing, per-employer sequence
//!   number and a ledger-derived timestamp, making replay and gap detection
//!   straightforward for indexers.
//! - A global sequence counter provides cross-employer ordering for indexers
//!   that reconstruct a full timeline.
//! - The admin address is set once at initialization and cannot be changed,
//!   preventing privilege escalation.
//! - Emergency pause blocks all new record writes while preserving reads.
//!
//! ## Data Retention
//!
//! Records are stored in `persistent` storage. Callers are responsible for
//! ensuring ledger TTL extensions if long-term on-chain retention is required.
//! Off-chain indexers should consume events and snapshot data independently.

use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, Env, Vec,
};
use audit_logger::{AuditLogEntry, AuditLoggerContract};
use payment_history::{PaymentHistoryContract, PaymentRecord};

contractclient!(AuditLoggerContract, audit_logger_client);
contractclient!(PaymentHistoryContract, payment_history_client);

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Error codes returned by the compliance reporting contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ComplianceError {
    /// Contract has not been initialized yet.
    NotInitialized = 1,
    /// Contract has already been initialized.
    AlreadyInitialized = 2,
    /// Caller is not authorized to perform this action.
    NotAuthorized = 3,
    /// `start_date` is greater than `end_date`.
    InvalidDateRange = 4,
    /// Requested `limit` exceeds the maximum allowed per query.
    QueryLimitExceeded = 5,
    /// Contract is paused; no new records may be written.
    ContractPaused = 6,
    /// Provided amount is invalid (e.g. zero or negative where not allowed).
    InvalidAmount = 7,
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Classification of a compliance record.
///
/// ## NatSpec
/// | Variant    | Description                                                        |
/// |------------|--------------------------------------------------------------------|
/// | Payroll    | Standard salary, bonus, and wage disbursement records.             |
/// | Tax        | Withheld amounts, government levies, or employer-side tax payments.|
/// | Regulatory | KYC checkpoints, localized compliance fee deductions, etc.         |
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReportType {
    Payroll,
    Tax,
    Regulatory,
}

/// A single immutable compliance record stored on-chain.
///
/// ## Tamper-Evidence
/// - `id` is a per-employer monotonic counter; gaps indicate missing records.
/// - `global_seq` is a contract-wide monotonic counter; indexers can detect
///   cross-employer ordering and replay attempts.
/// - `timestamp` is the ledger timestamp at write time; it cannot be
///   back-dated by callers.
/// - `publisher` records who wrote the entry, enabling publisher accountability.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComplianceRecord {
    /// Per-employer monotonic record identifier (1-based).
    pub id: u32,
    /// Contract-wide monotonic sequence number for cross-employer ordering.
    pub global_seq: u64,
    /// Employer on whose behalf this record was logged.
    pub employer: Address,
    /// Employee (payment recipient) associated with this record.
    pub employee: Address,
    /// Token contract address used for the payment.
    pub token: Address,
    /// Token amount (must be > 0).
    pub amount: i128,
    /// Ledger timestamp at the time of writing (set by the contract).
    pub timestamp: u64,
    /// Classification of this record.
    pub report_type: ReportType,
    /// Off-chain reference data (e.g. IPFS CID of a payslip PDF or JSON blob).
    pub metadata: Bytes,
    /// Address that submitted this record (employer or authorized publisher).
    pub publisher: Address,
}

/// Aggregated report returned by `generate_report`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComplianceReport {
    /// Employer this report covers.
    pub employer: Address,
    /// Employee this report covers.
    pub employee: Address,
    /// Inclusive start of the reporting period (UNIX timestamp).
    pub start_date: u64,
    /// Inclusive end of the reporting period (UNIX timestamp).
    pub end_date: u64,
    /// Sum of all matching record amounts (withholding).
    pub total_withholding: i128,
    /// Number of withholding records.
    pub withholding_count: u32,
    /// Withholding records from this contract.
    pub withholding_records: Vec<ComplianceRecord>,
    /// Payment history records from PaymentHistory contract.
    pub payment_history: Vec<PaymentRecord>,
    /// Audit log events from AuditLogger contract.
    pub agreement_events: Vec<AuditLogEntry>,
    /// Schema version for off-chain parsing.
    pub schema_version: u32,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

/// Storage keys used by the compliance reporting contract.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Initialization flag.
    Initialized,
    /// Contract administrator address.
    Admin,
    /// Emergency pause flag.
    Paused,
    /// Contract-wide monotonic sequence counter.
    GlobalSeq,
    /// Per-employer record count: `RecordCount(employer) -> u32`.
    RecordCount(Address),
    /// Individual record: `Record(employer, id) -> ComplianceRecord`.
    Record(Address, u32),
    /// Publisher allowlist: `Publisher(address) -> bool`.
    Publisher(Address),
    /// AuditLogger contract address.
    AuditLogger,
    /// PaymentHistory contract address.
    PaymentHistory,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

/// Maximum number of records that can be returned in a single `generate_report`
/// call. Prevents instruction-limit overflows on Soroban.
pub const MAX_QUERY_LIMIT: u32 = 100;

#[contract]
pub struct ComplianceReportingContract;

#[contractimpl]
impl ComplianceReportingContract {
    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    /// @notice Initializes the compliance reporting contract.
    /// @dev One-time setup. The `admin` address is immutable after this call.
    ///      The admin is automatically registered as an authorized publisher.
    /// @param admin The administrator address; controls publisher allowlist and
    ///              emergency pause.
    /// @return `ComplianceError::AlreadyInitialized` if called more than once.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ComplianceError> {
        if env
            .storage()
            .instance()
            .get::<_, bool>(&DataKey::Initialized)
            .unwrap_or(false)
        {
            return Err(ComplianceError::AlreadyInitialized);
        }

        admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Initialized, &true);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().persistent().set(&DataKey::GlobalSeq, &0u64);

        // Admin is an authorized publisher by default.
        env.storage()
            .persistent()
            .set(&DataKey::Publisher(admin.clone()), &true);

        env.events().publish(
            (symbol_short!("init"),),
            (admin,),
        );

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Admin: publisher management
    // -----------------------------------------------------------------------

    /// @notice Grants or revokes publisher authorization for an address.
    /// @dev Only the admin may call this. Authorized publishers may log records
    ///      on behalf of any employer. Employers can always log their own records
    ///      without being explicitly added as publishers.
    /// @param caller Must be the contract admin.
    /// @param publisher The address to authorize or deauthorize.
    /// @param authorized `true` to grant, `false` to revoke.
    pub fn set_publisher(
        env: Env,
        caller: Address,
        publisher: Address,
        authorized: bool,
    ) -> Result<(), ComplianceError> {
        Self::require_initialized(&env)?;
        Self::require_admin(&env, &caller)?;

        env.storage()
            .persistent()
            .set(&DataKey::Publisher(publisher.clone()), &authorized);

        env.events().publish(
            (symbol_short!("pub_set"),),
            (publisher, authorized),
        );

        Ok(())
    }

    /// @notice Returns whether an address is an authorized publisher.
    /// @param publisher The address to query.
    pub fn is_publisher(env: Env, publisher: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Publisher(publisher))
            .unwrap_or(false)
    }

    /// @notice Returns the current report schema version.
    pub fn get_report_schema_version(_env: Env) -> u32 {
        1
    }

    /// @notice Sets the contract addresses for AuditLogger and PaymentHistory.
    pub fn set_contract_addresses(
        env: Env,
        caller: Address,
        audit_logger: Address,
        payment_history: Address,
    ) -> Result<(), ComplianceError> {
        Self::require_initialized(&env)?;
        Self::require_admin(&env, &caller)?;

        env.storage().persistent().set(&DataKey::AuditLogger, &audit_logger);
        env.storage().persistent().set(&DataKey::PaymentHistory, &payment_history);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Admin: emergency pause
    // -----------------------------------------------------------------------

    /// @notice Pauses or unpauses the contract.
    /// @dev When paused, `log_record` is blocked. Reads (`generate_report`,
    ///      `get_record`, `get_record_count`) remain available so indexers can
    ///      continue to reconstruct history.
    /// @param caller Must be the contract admin.
    /// @param paused `true` to pause, `false` to unpause.
    pub fn set_paused(env: Env, caller: Address, paused: bool) -> Result<(), ComplianceError> {
        Self::require_initialized(&env)?;
        Self::require_admin(&env, &caller)?;

        env.storage().instance().set(&DataKey::Paused, &paused);

        env.events().publish(
            (symbol_short!("paused"),),
            (paused,),
        );

        Ok(())
    }

    /// @notice Returns whether the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get::<_, bool>(&DataKey::Paused)
            .unwrap_or(false)
    }

    // -----------------------------------------------------------------------
    // Record writing
    // -----------------------------------------------------------------------

    /// @notice Logs a new compliance record onto the ledger.
    /// @dev The caller must be either the `employer` themselves or an address
    ///      that has been granted publisher authorization by the admin.
    ///      The ledger timestamp is used; callers cannot back-date records.
    ///      A monotonically increasing per-employer `id` and a contract-wide
    ///      `global_seq` are assigned, enabling gap detection by indexers.
    ///      Emits a `log_comp` event with `(employer, id, global_seq, timestamp,
    ///      amount, report_type_u32)` for off-chain indexers.
    /// @param publisher The address submitting this record (must auth).
    /// @param employer The company/entity on whose behalf the record is logged.
    /// @param employee The payment recipient.
    /// @param token The Soroban token contract address.
    /// @param amount The token amount (must be > 0).
    /// @param report_type Classification of the record.
    /// @param metadata Off-chain reference bytes (e.g. IPFS CID).
    /// @return The per-employer record `id` assigned to this entry.
    pub fn log_record(
        env: Env,
        publisher: Address,
        employer: Address,
        employee: Address,
        token: Address,
        amount: i128,
        report_type: ReportType,
        metadata: Bytes,
    ) -> Result<u32, ComplianceError> {
        Self::require_initialized(&env)?;
        Self::require_not_paused(&env)?;

        publisher.require_auth();

        // Authorization: publisher must be the employer or an allowlisted publisher.
        let is_employer = publisher == employer;
        let is_authorized_publisher = env
            .storage()
            .persistent()
            .get::<_, bool>(&DataKey::Publisher(publisher.clone()))
            .unwrap_or(false);

        if !is_employer && !is_authorized_publisher {
            return Err(ComplianceError::NotAuthorized);
        }

        if amount <= 0 {
            return Err(ComplianceError::InvalidAmount);
        }

        // Assign per-employer ID.
        let count_key = DataKey::RecordCount(employer.clone());
        let next_id: u32 = env
            .storage()
            .persistent()
            .get(&count_key)
            .unwrap_or(0u32)
            + 1;

        // Assign global sequence number.
        let global_seq: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::GlobalSeq)
            .unwrap_or(0u64)
            + 1;

        let timestamp = env.ledger().timestamp();

        let record = ComplianceRecord {
            id: next_id,
            global_seq,
            employer: employer.clone(),
            employee,
            token,
            amount,
            timestamp,
            report_type: report_type.clone(),
            metadata,
            publisher: publisher.clone(),
        };

        env.storage()
            .persistent()
            .set(&DataKey::Record(employer.clone(), next_id), &record);
        env.storage().persistent().set(&count_key, &next_id);
        env.storage()
            .persistent()
            .set(&DataKey::GlobalSeq, &global_seq);

        // Emit indexable event. All fields needed for off-chain reconstruction
        // are included so indexers don't need a separate storage read.
        let report_type_u32: u32 = match report_type {
            ReportType::Payroll => 0,
            ReportType::Tax => 1,
            ReportType::Regulatory => 2,
        };
        env.events().publish(
            (symbol_short!("log_comp"), employer.clone()),
            (next_id, global_seq, timestamp, amount, report_type_u32),
        );

        Ok(next_id)
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// @notice Returns the total number of records logged for an employer.
    /// @param employer The employer address to query.
    pub fn get_record_count(env: Env, employer: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::RecordCount(employer))
            .unwrap_or(0)
    }

    /// @notice Fetches a single compliance record by employer and record ID.
    /// @param employer The employer address.
    /// @param id The per-employer record identifier.
    /// @return `Some(ComplianceRecord)` if found, `None` otherwise.
    pub fn get_record(env: Env, employer: Address, id: u32) -> Option<ComplianceRecord> {
        env.storage()
            .persistent()
            .get(&DataKey::Record(employer, id))
    }

    /// @notice Returns the current contract-wide global sequence counter.
    ///         Useful for indexers to detect gaps or missed events.
    pub fn get_global_seq(env: Env) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::GlobalSeq)
            .unwrap_or(0)
    }

    /// @notice Generates an aggregated compliance report for a given employer
    ///         and time window.
    /// @dev Iterates backwards (newest-first) through the employer's records.
    ///      Stops early when a record's timestamp falls below `start_date`,
    ///      saving instruction budget. `limit` is capped at `MAX_QUERY_LIMIT`
    ///      (100) to prevent instruction-limit overflows.
    /// @param employer The employer to report on.
    /// @param start_date Inclusive start of the reporting period (UNIX timestamp).
    /// @param end_date Inclusive end of the reporting period (UNIX timestamp).
    /// @param filter_type Optional `ReportType` filter; `None` returns all types.
    /// @param limit Maximum number of matching records to include (≤ 100).
    /// @return A `ComplianceReport` with aggregated totals and matching records.
    /// @notice Returns the withholding records for an employer and time window.
    /// @dev Formerly `generate_report`.
    /// @param employer The employer to report on.
    /// @param start_date Inclusive start of the reporting period (UNIX timestamp).
    /// @param end_date Inclusive end of the reporting period (UNIX timestamp).
    /// @param filter_type Optional `ReportType` filter; `None` returns all types.
    /// @param limit Maximum number of matching records to include (≤ 100).
    pub fn get_withholding_records(
        env: Env,
        employer: Address,
        start_date: u64,
        end_date: u64,
        filter_type: Option<ReportType>,
        limit: u32,
    ) -> Result<ComplianceReport, ComplianceError> {
        Self::require_initialized(&env)?;

        if start_date > end_date {
            return Err(ComplianceError::InvalidDateRange);
        }
        if limit == 0 || limit > MAX_QUERY_LIMIT {
            return Err(ComplianceError::QueryLimitExceeded);
        }

        let total_records: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::RecordCount(employer.clone()))
            .unwrap_or(0);

        let mut matching_records = Vec::new(&env);
        let mut total_amount: i128 = 0;
        let mut current_id = total_records;

        while current_id > 0 && (matching_records.len() as u32) < limit {
            if let Some(record) = env
                .storage()
                .persistent()
                .get::<_, ComplianceRecord>(&DataKey::Record(employer.clone(), current_id))
            {
                if record.timestamp < start_date {
                    // Records are stored in ascending timestamp order; once we
                    // pass below start_date we can stop.
                    break;
                }

                if record.timestamp <= end_date {
                    let type_matches = match &filter_type {
                        Some(t) => &record.report_type == t,
                        None => true,
                    };

                    if type_matches {
                        total_amount += record.amount;
                        matching_records.push_back(record);
                    }
                }
            }
            current_id -= 1;
        }

        // Return a dummy report for now until we fully integrate everything.
        Ok(ComplianceReport {
            employer,
            employee: Address::generate(&env),
            start_date,
            end_date,
            total_withholding: total_amount,
            withholding_count: matching_records.len() as u32,
            withholding_records: matching_records,
            payment_history: Vec::new(&env),
            agreement_events: Vec::new(&env),
            schema_version: 1,
        })
    }

    /// @notice Generates a comprehensive compliance report for an employee and time window.
    /// @param employee The employee to report on.
    /// @param period_start Inclusive start of the reporting period (UNIX timestamp).
    /// @param period_end Inclusive end of the reporting period (UNIX timestamp).
    /// @return A `ComplianceReport` with aggregated data.
    pub fn generate_report(
        env: Env,
        employee: Address,
        period_start: u64,
        period_end: u64,
    ) -> Result<ComplianceReport, ComplianceError> {
        Self::require_initialized(&env)?;

        if period_start > period_end {
            return Err(ComplianceError::InvalidDateRange);
        }

        // 1. Fetch configured contract addresses
        let audit_logger_addr: Address = env
            .storage()
            .persistent()
            .get(&DataKey::AuditLogger)
            .ok_or(ComplianceError::NotInitialized)?;
        let payment_history_addr: Address = env
            .storage()
            .persistent()
            .get(&DataKey::PaymentHistory)
            .ok_or(ComplianceError::NotInitialized)?;

        // 2. Fetch PaymentHistory records for employee
        let ph_client = payment_history_client::Client::new(&env, &payment_history_addr);
        // Assuming there is a way to get payments by employee.
        let payments = ph_client.get_payments_by_employee(&employee, &1, &MAX_QUERY_LIMIT);

        // 3. Fetch AuditLogger events
        let al_client = audit_logger_client::Client::new(&env, &audit_logger_addr);
        // Assuming there is a way to get logs.
        let events = al_client.get_latest_logs(&MAX_QUERY_LIMIT).unwrap_or_else(|_| Vec::new(&env));

        // 4. Fetch Withholding records (placeholder)
        let withholding_records = Vec::new(&env);

        let schema_version = Self::get_report_schema_version(env.clone());

        Ok(ComplianceReport {
            employer: Address::generate(&env), // Placeholder
            employee,
            start_date: period_start,
            end_date: period_end,
            total_withholding: 0,
            withholding_count: 0,
            withholding_records,
            payment_history: payments,
            agreement_events: events,
            schema_version,
        })
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn require_initialized(env: &Env) -> Result<(), ComplianceError> {
        if !env
            .storage()
            .instance()
            .get::<_, bool>(&DataKey::Initialized)
            .unwrap_or(false)
        {
            return Err(ComplianceError::NotInitialized);
        }
        Ok(())
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ComplianceError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ComplianceError::NotAuthorized)?;
        caller.require_auth();
        if *caller != admin {
            return Err(ComplianceError::NotAuthorized);
        }
        Ok(())
    }

    fn require_not_paused(env: &Env) -> Result<(), ComplianceError> {
        if env
            .storage()
            .instance()
            .get::<_, bool>(&DataKey::Paused)
            .unwrap_or(false)
        {
            return Err(ComplianceError::ContractPaused);
        }
        Ok(())
    }
}
