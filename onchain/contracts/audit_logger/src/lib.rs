#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol, Vec};
use stellar_contract_utils::upgradeable::UpgradeableInternal;
use stellar_macros::Upgradeable;

/// Storage keys for the audit logger contract.
#[contracttype]
#[derive(Clone)]
pub enum StorageKey {
    /// Contract owner allowed to configure retention and access control.
    Owner,
    /// Next sequential log identifier.
    NextLogId,
    /// Logical count of logs currently retained.
    LogCount,
    /// Optional maximum number of logs to retain (0 = unlimited).
    RetentionLimit,
    /// Oldest retained log identifier (for paging).
    FirstLogId,
    /// Individual log entry by identifier.
    LogEntry(u64),
}

/// Single audit log entry.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditLogEntry {
    /// Monotonic identifier (1-based).
    pub id: u64,
    /// Ledger timestamp at which the event was recorded.
    pub timestamp: u64,
    /// Actor that triggered the event.
    pub actor: Address,
    /// Application-specific action label (e.g. "create_agreement").
    pub action: Symbol,
    /// Optional subject account associated with the event.
    pub subject: Option<Address>,
    /// Optional signed amount associated with the event (e.g. payment amount).
    pub amount: Option<i128>,
}

/// Error codes for the audit logger.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum AuditError {
    /// Caller is not authorized to perform the requested operation.
    Unauthorized = 1,
    /// Requested log entry is outside of the currently retained window.
    LogNotFound = 2,
    /// Pagination or limit arguments are invalid.
    InvalidArguments = 3,
}

/// Audit Logger Contract
///
/// Provides append-only audit logging for on-chain operations. Each log entry
/// is assigned a monotonically increasing identifier and timestamp, and once
/// written, cannot be modified. Retention is enforced via a configurable
/// maximum number of retained entries per contract instance.
#[derive(Upgradeable)]
#[contract]
pub struct AuditLoggerContract;

impl UpgradeableInternal for AuditLoggerContract {
    fn _require_auth(e: &Env, _operator: &Address) {
        let owner: Address = e.storage().persistent().get(&StorageKey::Owner).unwrap();
        owner.require_auth();
    }
}

#[contractimpl]
impl AuditLoggerContract {
    /// Initializes the audit logger.
    ///
    /// # Arguments
    /// * `owner` - Address that controls retention configuration
    /// * `retention_limit` - Maximum number of logs to retain (0 = unlimited)
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn initialize(env: Env, owner: Address, retention_limit: u32) {
        owner.require_auth();

        env.storage().persistent().set(&StorageKey::Owner, &owner);
        env.storage()
            .persistent()
            .set(&StorageKey::RetentionLimit, &retention_limit);
        env.storage()
            .persistent()
            .set(&StorageKey::NextLogId, &1u64);
        env.storage().persistent().set(&StorageKey::LogCount, &0u64);
        env.storage()
            .persistent()
            .set(&StorageKey::FirstLogId, &1u64);
    }

    /// Updates the log retention limit (maximum number of retained entries).
    ///
    /// # Access Control
    /// - Caller must be the contract owner.
    ///
    /// # Arguments
    /// * `caller` - caller parameter
    /// * `retention_limit` - retention_limit parameter
    ///
    /// # Returns
    /// Result<(), AuditError>
    ///
    /// # Errors
    /// Returns an error if validation fails
    pub fn set_retention_limit(
        env: Env,
        caller: Address,
        retention_limit: u32,
    ) -> Result<(), AuditError> {
        let owner: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::Owner)
            .ok_or(AuditError::Unauthorized)?;

        caller.require_auth();
        if caller != owner {
            return Err(AuditError::Unauthorized);
        }

        env.storage()
            .persistent()
            .set(&StorageKey::RetentionLimit, &retention_limit);
        Ok(())
    }

    /// Returns the current retention limit (0 = unlimited).
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_retention_limit(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get(&StorageKey::RetentionLimit)
            .unwrap_or(0u32)
    }

    /// Appends a new audit log entry.
    ///
    /// # Arguments
    /// * `actor` - Address of the caller performing the action (must auth)
    /// * `action` - Application-defined action label
    /// * `subject` - Optional subject account of the event
    /// * `amount` - Optional signed amount associated with the event
    ///
    /// # Returns
    /// * `id` - Identifier of the newly created log entry
    pub fn append_log(
        env: Env,
        actor: Address,
        action: Symbol,
        subject: Option<Address>,
        amount: Option<i128>,
    ) -> u64 {
        actor.require_auth();

        // Load counters
        let mut next_id: u64 = env
            .storage()
            .persistent()
            .get(&StorageKey::NextLogId)
            .unwrap_or(1u64);
        let mut log_count: u64 = env
            .storage()
            .persistent()
            .get(&StorageKey::LogCount)
            .unwrap_or(0u64);
        let mut first_id: u64 = env
            .storage()
            .persistent()
            .get(&StorageKey::FirstLogId)
            .unwrap_or(1u64);

        let timestamp = env.ledger().timestamp();

        let entry = AuditLogEntry {
            id: next_id,
            timestamp,
            actor,
            action,
            subject,
            amount,
        };

        env.storage()
            .persistent()
            .set(&StorageKey::LogEntry(next_id), &entry);

        next_id += 1;
        log_count += 1;

        // Apply retention policy if configured.
        let retention: u32 = env
            .storage()
            .persistent()
            .get(&StorageKey::RetentionLimit)
            .unwrap_or(0u32);

        if retention > 0 {
            let r = retention as u64;
            if log_count > r {
                // Advance the logical window; underlying storage may retain
                // older entries but they are no longer visible via queries.
                first_id += 1;
                log_count = r;
            }
        }

        env.storage()
            .persistent()
            .set(&StorageKey::NextLogId, &next_id);
        env.storage()
            .persistent()
            .set(&StorageKey::LogCount, &log_count);
        env.storage()
            .persistent()
            .set(&StorageKey::FirstLogId, &first_id);

        entry.id
    }

    /// Returns the total number of logs currently retained.
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_log_count(env: Env) -> u64 {
        env.storage()
            .persistent()
            .get(&StorageKey::LogCount)
            .unwrap_or(0u64)
    }

    /// Fetches a single log entry by identifier, if it is still retained.
    ///
    /// # Arguments
    /// * `id` - id parameter
    ///
    /// # Returns
    /// `Option<AuditLogEntry>`
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_log(env: Env, id: u64) -> Option<AuditLogEntry> {
        let first_id: u64 = env
            .storage()
            .persistent()
            .get(&StorageKey::FirstLogId)
            .unwrap_or(1u64);
        let next_id: u64 = env
            .storage()
            .persistent()
            .get(&StorageKey::NextLogId)
            .unwrap_or(first_id);

        if id < first_id || id >= next_id {
            return None;
        }

        env.storage().persistent().get(&StorageKey::LogEntry(id))
    }

    /// Returns a window of logs starting at a given offset from the first
    /// retained log.
    ///
    /// # Arguments
    /// * `offset` - Zero-based index into the retained log window
    /// * `limit` - Maximum number of entries to return
    ///
    /// # Errors
    /// Returns an error if validation fails
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_logs(env: Env, offset: u32, limit: u32) -> Result<Vec<AuditLogEntry>, AuditError> {
        if limit == 0 {
            return Err(AuditError::InvalidArguments);
        }

        let first_id: u64 = env
            .storage()
            .persistent()
            .get(&StorageKey::FirstLogId)
            .unwrap_or(1u64);
        let log_count: u64 = env
            .storage()
            .persistent()
            .get(&StorageKey::LogCount)
            .unwrap_or(0u64);

        if offset as u64 >= log_count {
            return Ok(Vec::new(&env));
        }

        let mut results = Vec::new(&env);
        let mut remaining = core::cmp::min(limit as u64, log_count - offset as u64);

        let mut current_id = first_id + offset as u64;
        while remaining > 0 {
            if let Some(entry) = env
                .storage()
                .persistent()
                .get::<_, AuditLogEntry>(&StorageKey::LogEntry(current_id))
            {
                results.push_back(entry);
            }
            current_id += 1;
            remaining -= 1;
        }

        Ok(results)
    }

    /// Returns the latest `limit` log entries (newest first).
    ///
    /// # Arguments
    /// * `limit` - limit parameter
    ///
    /// # Errors
    /// Returns an error if validation fails
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_latest_logs(env: Env, limit: u32) -> Result<Vec<AuditLogEntry>, AuditError> {
        if limit == 0 {
            return Err(AuditError::InvalidArguments);
        }

        let first_id: u64 = env
            .storage()
            .persistent()
            .get(&StorageKey::FirstLogId)
            .unwrap_or(1u64);
        let log_count: u64 = env
            .storage()
            .persistent()
            .get(&StorageKey::LogCount)
            .unwrap_or(0u64);

        if log_count == 0 {
            return Ok(Vec::new(&env));
        }

        let total = core::cmp::min(limit as u64, log_count);
        let start_id = first_id + log_count - total;

        let mut results = Vec::new(&env);
        for id in start_id..(start_id + total) {
            if let Some(entry) = env
                .storage()
                .persistent()
                .get::<_, AuditLogEntry>(&StorageKey::LogEntry(id))
            {
                results.push_back(entry);
            }
        }

        Ok(results)
    }
}
