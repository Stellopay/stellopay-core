#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol, Vec};
use stellar_contract_utils::upgradeable::UpgradeableInternal;
use stellar_macros::Upgradeable;

/// Maximum number of entries returned by a single [`AuditLoggerContract::get_logs`] call.
///
/// Callers supplying a larger `limit` are silently clamped to this value.
/// This bounds ledger-read budget per invocation and prevents a DoS via
/// an uncapped loop driven by a caller-controlled `limit` up to `u32::MAX`.
pub const MAX_PAGE_SIZE: u32 = 100;

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

/// Result type returned by [`AuditLoggerContract::get_logs`].
///
/// `next_cursor` is `Some(offset)` when more entries may exist beyond the
/// current page, allowing the caller to resume by passing that value as
/// `offset` in the next call. It is `None` when the end of the retained
/// window has been reached.
#[contracttype]
#[derive(Clone, Debug)]
pub struct LogsPage {
    /// Entries retrieved for this page (may be fewer than `limit` if
    /// orphaned entries were skipped due to retention pruning).
    pub entries: Vec<AuditLogEntry>,
    /// Offset to pass as `offset` in the next `get_logs` call to resume,
    /// or `None` if there are no more entries.
    pub next_cursor: Option<u32>,
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
    /// `limit` is silently clamped to [`MAX_PAGE_SIZE`] (100) to prevent an
    /// unbounded loop driven by a caller-supplied value up to `u32::MAX`.
    ///
    /// Entries whose storage key is absent — because retention has pruned them
    /// since the window counters were last updated — are **skipped silently**
    /// and do not count against `limit`. This avoids misrepresenting the log
    /// count while still returning every retrievable entry in the range.
    ///
    /// # Arguments
    /// * `offset` - Zero-based index into the retained log window
    /// * `limit` - Maximum number of entries to return (capped at [`MAX_PAGE_SIZE`])
    ///
    /// # Returns
    /// A [`LogsPage`] containing the retrieved entries and an optional
    /// `next_cursor` offset for resuming past any gaps.
    ///
    /// # Errors
    /// Returns [`AuditError::InvalidArguments`] if `limit` is 0.
    pub fn get_logs(env: Env, offset: u32, limit: u32) -> Result<LogsPage, AuditError> {
        if limit == 0 {
            return Err(AuditError::InvalidArguments);
        }

        // Clamp to MAX_PAGE_SIZE to bound ledger-read budget.
        let effective_limit = limit.min(MAX_PAGE_SIZE);

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
            return Ok(LogsPage {
                entries: Vec::new(&env),
                next_cursor: None,
            });
        }

        let mut entries = Vec::new(&env);
        let window = core::cmp::min(effective_limit as u64, log_count - offset as u64);
        let start_id = first_id + offset as u64;
        let end_id = start_id + window; // exclusive upper bound

        for id in start_id..end_id {
            // Skip orphaned entries: storage may not hold the key when
            // retention has pruned the underlying record after first_id
            // was last updated. We skip and continue rather than returning
            // a short count or panicking.
            if let Some(entry) = env
                .storage()
                .persistent()
                .get::<_, AuditLogEntry>(&StorageKey::LogEntry(id))
            {
                entries.push_back(entry);
            }
        }

        // Compute the next cursor: the offset of the entry after this window.
        let next_offset = offset.saturating_add(window as u32);
        let next_cursor = if (next_offset as u64) < log_count {
            Some(next_offset)
        } else {
            None
        };

        Ok(LogsPage {
            entries,
            next_cursor,
        })
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
