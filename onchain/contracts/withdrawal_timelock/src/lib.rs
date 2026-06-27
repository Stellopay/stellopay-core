#![no_std]
#![allow(deprecated)] // env.events().publish() — codebase-wide pattern; contractevent migration is a separate concern

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Vec};

// ─── Constants ────────────────────────────────────────────────────────────────

/// Maximum configurable delay: 30 days.
///
/// Prevents an admin from setting `min_delay_seconds` to an arbitrarily large
/// value that would permanently lock every queued operation. Any
/// `initialize` or `update_delay` call with a value exceeding this constant
/// returns `Err(TimelockError::DelayTooLarge)`.
pub const MAX_DELAY_SECONDS: u64 = 30 * 24 * 3600; // 2_592_000

/// Maximum number of operations returned in a single paginated query.
pub const MAX_PAGE_SIZE: u32 = 100;

// ─── Error Types ──────────────────────────────────────────────────────────────

/// Errors returned by the withdrawal timelock contract.
#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TimelockError {
    /// Contract has not been initialized.
    NotInitialized = 1,
    /// `initialize` was called more than once.
    AlreadyInitialized = 2,
    /// Caller is not the configured admin.
    NotAdmin = 3,
    /// Delay exceeds `MAX_DELAY_SECONDS` (30 days).
    DelayTooLarge = 4,
    /// Delay is zero (not allowed).
    InvalidDelay = 5,
    /// No operation exists with the given id.
    OperationNotFound = 6,
    /// Operation `eta` has not yet been reached.
    NotReady = 7,
    /// Operation is no longer in `Queued` status.
    AlreadyExecutedOrCancelled = 8,
}

// ─── Domain Types ─────────────────────────────────────────────────────────────

/// Types of timelocked operations that can be queued.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationKind {
    /// A large outbound payment intent.
    ///
    /// Layout: `(token, to, amount)`.
    /// Actual token transfer is performed by an external orchestrator after
    /// the timelock is satisfied; this contract records the intent only.
    Withdrawal(Address, Address, i128),

    /// A generic administrative change on an external contract.
    ///
    /// Layout: `(target_contract, payload_hash)`.
    /// The `payload_hash` is an opaque 32-byte commitment to the change
    /// payload; off-chain tooling must verify this hash before applying.
    AdminChange(Address, soroban_sdk::BytesN<32>),
}

/// Lifecycle status of a queued operation.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationStatus {
    /// Waiting for `eta` to pass.
    Queued,
    /// Successfully executed.
    Executed,
    /// Cancelled before execution.
    Cancelled,
}

/// A queued timelocked operation.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimelockedOperation {
    /// Monotone, auto-incremented identifier.
    pub id: u128,
    /// Encoded operation kind and its parameters.
    pub kind: OperationKind,
    /// Address that queued this operation.
    pub creator: Address,
    /// Earliest timestamp at which this operation may be executed.
    pub eta: u64,
    /// Ledger timestamp at which the operation was queued.
    pub created_at: u64,
    /// Ledger timestamp at which the operation was executed; `None` otherwise.
    pub executed_at: Option<u64>,
    /// Ledger timestamp at which the operation was cancelled; `None` otherwise.
    pub cancelled_at: Option<u64>,
    /// Current lifecycle status.
    pub status: OperationStatus,
}

/// Paginated result containing a list of operations and an optional cursor.
#[contracttype]
#[derive(Clone, Debug)]
pub struct OperationPage {
    /// List of operations in the current page.
    pub operations: Vec<TimelockedOperation>,
    /// Index to use as `start` in the next query to continue.
    pub next_cursor: Option<u32>,
}

// ─── Storage Keys ─────────────────────────────────────────────────────────────

/// Persistent storage keys for the timelock contract.
#[contracttype]
#[derive(Clone)]
pub enum StorageKey {
    /// One-time initialization flag (`bool`).
    Initialized,
    /// Admin address (`Address`).
    Admin,
    /// Current minimum delay in seconds (`u64`).
    MinDelaySeconds,
    /// Auto-incremented next operation id (`u128`).
    NextOpId,
    /// Count of currently active (Queued) operations (`u32`).
    ///
    /// Incremented by `queue`; decremented by `execute` and `cancel`.
    QueuedCount,
    /// Full operation data keyed by id (`TimelockedOperation`).
    Operation(u128),
    /// Count of operations created by an address (`u32`).
    OperationsCount(Address),
    /// Operation id at a specific position for an address (`u32`).
    ///
    /// Layout: `(owner, position) -> op_id`.
    OperationAt(Address, u32),
}

// ─── Private Helpers ──────────────────────────────────────────────────────────

/// Validates a delay value: must be > 0 and ≤ `MAX_DELAY_SECONDS`.
///
/// Centralises the two-sided delay guard so both `initialize` and
/// `update_delay` share identical validation logic.
fn validate_delay(d: u64) -> Result<(), TimelockError> {
    if d == 0 {
        return Err(TimelockError::InvalidDelay);
    }
    if d > MAX_DELAY_SECONDS {
        return Err(TimelockError::DelayTooLarge);
    }
    Ok(())
}

fn require_initialized(env: &Env) -> Result<(), TimelockError> {
    let initialized: bool = env
        .storage()
        .persistent()
        .get(&StorageKey::Initialized)
        .unwrap_or(false);
    if !initialized {
        return Err(TimelockError::NotInitialized);
    }
    Ok(())
}

fn read_admin(env: &Env) -> Result<Address, TimelockError> {
    env.storage()
        .persistent()
        .get(&StorageKey::Admin)
        .ok_or(TimelockError::NotInitialized)
}

/// Authenticates the caller and verifies it matches the stored admin address.
fn require_admin(env: &Env, caller: &Address) -> Result<(), TimelockError> {
    caller.require_auth();
    let admin = read_admin(env)?;
    if *caller != admin {
        return Err(TimelockError::NotAdmin);
    }
    Ok(())
}

fn read_min_delay(env: &Env) -> u64 {
    env.storage()
        .persistent()
        .get(&StorageKey::MinDelaySeconds)
        .unwrap_or(0)
}

/// Reads `QueuedCount`, defaulting to 0 if absent.
fn read_queued_count(env: &Env) -> u32 {
    env.storage()
        .persistent()
        .get(&StorageKey::QueuedCount)
        .unwrap_or(0u32)
}

/// Writes `QueuedCount` to persistent storage.
fn write_queued_count(env: &Env, count: u32) {
    env.storage()
        .persistent()
        .set(&StorageKey::QueuedCount, &count);
}

/// Returns the next operation id and advances the counter atomically.
fn next_op_id(env: &Env) -> u128 {
    let current: u128 = env
        .storage()
        .persistent()
        .get(&StorageKey::NextOpId)
        .unwrap_or(0);
    let next = current.checked_add(1).expect("op id overflow");
    env.storage().persistent().set(&StorageKey::NextOpId, &next);
    next
}

fn read_operation(env: &Env, id: u128) -> Result<TimelockedOperation, TimelockError> {
    env.storage()
        .persistent()
        .get(&StorageKey::Operation(id))
        .ok_or(TimelockError::OperationNotFound)
}

fn write_operation(env: &Env, op: &TimelockedOperation) {
    env.storage()
        .persistent()
        .set(&StorageKey::Operation(op.id), op);
}

/// Appends `id` to the indexed operation list for `owner`.
fn push_operation_for(env: &Env, owner: &Address, id: u128) {
    let count: u32 = env
        .storage()
        .persistent()
        .get(&StorageKey::OperationsCount(owner.clone()))
        .unwrap_or(0);

    let new_count = count.checked_add(1).expect("owner op count overflow");

    env.storage()
        .persistent()
        .set(&StorageKey::OperationAt(owner.clone(), new_count), &id);

    env.storage()
        .persistent()
        .set(&StorageKey::OperationsCount(owner.clone()), &new_count);
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct WithdrawalTimelock;

#[contractimpl]
impl WithdrawalTimelock {
    // ── Initialization ────────────────────────────────────────────────────────

    /// @notice Initializes the withdrawal timelock contract.
    /// @dev One-time call. Stores the admin address and global minimum delay.
    ///      Subsequent calls return `Err(AlreadyInitialized)`.
    ///      The delay must satisfy `0 < min_delay_seconds <= MAX_DELAY_SECONDS`
    ///      (i.e. between 1 second and 30 days inclusive).
    /// @param admin Address authorized to queue, execute, cancel, and update
    ///              the delay. Must authenticate this call.
    /// @param min_delay_seconds Seconds between queue time and earliest
    ///                          execution time.
    /// @return Ok(()) on success.
    pub fn initialize(
        env: Env,
        admin: Address,
        min_delay_seconds: u64,
    ) -> Result<(), TimelockError> {
        if env
            .storage()
            .persistent()
            .get::<_, bool>(&StorageKey::Initialized)
            .unwrap_or(false)
        {
            return Err(TimelockError::AlreadyInitialized);
        }

        validate_delay(min_delay_seconds)?;

        admin.require_auth();

        env.storage().persistent().set(&StorageKey::Admin, &admin);
        env.storage()
            .persistent()
            .set(&StorageKey::MinDelaySeconds, &min_delay_seconds);
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);

        Ok(())
    }

    // ── Timelock Workflow ─────────────────────────────────────────────────────

    /// @notice Queues a new timelocked operation.
    /// @dev Admin-only. The `eta` is computed as `now + min_delay_seconds`.
    ///      The operation cannot be executed before `eta` has been reached.
    ///      Multiple distinct operations may be queued simultaneously; each
    ///      receives a unique monotone id. Emits:
    ///        `("timelock_queued", op_id) → kind`
    ///      which off-chain monitors should subscribe to for alerting.
    /// @param caller Admin address queuing the operation; must authenticate.
    /// @param kind   `Withdrawal(token, to, amount)` or
    ///               `AdminChange(target_contract, payload_hash)`.
    /// @return op_id The newly assigned operation identifier.
    pub fn queue(env: Env, caller: Address, kind: OperationKind) -> Result<u128, TimelockError> {
        require_initialized(&env)?;
        require_admin(&env, &caller)?;

        let min_delay = read_min_delay(&env);
        // Defensive belt-and-suspenders: min_delay can only be 0 if the
        // storage slot is missing, which require_initialized already guards.
        if min_delay == 0 {
            return Err(TimelockError::InvalidDelay);
        }

        let now = env.ledger().timestamp();
        let eta = now
            .checked_add(min_delay)
            .ok_or(TimelockError::InvalidDelay)?;

        let id = next_op_id(&env);
        let op = TimelockedOperation {
            id,
            kind: kind.clone(),
            creator: caller.clone(),
            eta,
            created_at: now,
            executed_at: None,
            cancelled_at: None,
            status: OperationStatus::Queued,
        };

        write_operation(&env, &op);
        push_operation_for(&env, &caller, id);
        write_queued_count(&env, read_queued_count(&env) + 1);

        env.events().publish(("timelock_queued", id), kind);

        Ok(id)
    }

    /// @notice Executes a ready timelocked operation.
    /// @dev Admin-only. Execution requires `env.ledger().timestamp() >= op.eta`.
    ///      This contract records the execution intent (status, timestamp, event);
    ///      actual token transfers or admin changes are performed by an external
    ///      orchestrator using the recorded `op.kind` data.
    ///      Emits: `("timelock_executed", op_id) → kind`.
    /// @param caller Admin address executing the operation; must authenticate.
    /// @param op_id  Operation identifier returned by `queue`.
    /// @return Ok(()) on success.
    pub fn execute(env: Env, caller: Address, op_id: u128) -> Result<(), TimelockError> {
        require_initialized(&env)?;
        require_admin(&env, &caller)?;

        let mut op = read_operation(&env, op_id)?;
        if op.status != OperationStatus::Queued {
            return Err(TimelockError::AlreadyExecutedOrCancelled);
        }

        let now = env.ledger().timestamp();
        if now < op.eta {
            return Err(TimelockError::NotReady);
        }

        op.status = OperationStatus::Executed;
        op.executed_at = Some(now);
        write_operation(&env, &op);

        write_queued_count(&env, read_queued_count(&env).saturating_sub(1));

        env.events()
            .publish(("timelock_executed", op_id), op.kind.clone());

        Ok(())
    }

    /// @notice Cancels a queued operation before it is executed.
    /// @dev Admin-only. Sets `cancelled_at` for the audit trail.
    ///      Already-executed operations cannot be cancelled.
    ///      Emits: `("timelock_cancelled", op_id) → ()`.
    /// @param caller Admin address cancelling the operation; must authenticate.
    /// @param op_id  Operation identifier returned by `queue`.
    /// @return Ok(()) on success.
    pub fn cancel(env: Env, caller: Address, op_id: u128) -> Result<(), TimelockError> {
        require_initialized(&env)?;
        require_admin(&env, &caller)?;

        let mut op = read_operation(&env, op_id)?;
        if op.status != OperationStatus::Queued {
            return Err(TimelockError::AlreadyExecutedOrCancelled);
        }

        let now = env.ledger().timestamp();
        op.status = OperationStatus::Cancelled;
        op.cancelled_at = Some(now);
        write_operation(&env, &op);

        write_queued_count(&env, read_queued_count(&env).saturating_sub(1));

        env.events().publish(("timelock_cancelled", op_id), ());

        Ok(())
    }

    /// @notice Updates the global minimum delay for future queued operations.
    /// @dev Admin-only. The new delay must satisfy the same constraints as at
    ///      initialization (`0 < new_delay <= MAX_DELAY_SECONDS`).
    ///
    ///      IMPORTANT: This change does NOT retroactively alter the `eta` of
    ///      any already-queued operation. Operations queued before this call
    ///      keep their original `eta`, which was frozen at queue time. Only
    ///      operations queued *after* this call use the new delay.
    ///
    ///      Emits: `("timelock_delay_updated", old_delay) → new_delay`.
    /// @param caller     Admin address; must authenticate.
    /// @param new_delay  New minimum delay in seconds.
    ///                   Must be in range `(0, MAX_DELAY_SECONDS]`.
    /// @return Ok(()) on success.
    pub fn update_delay(env: Env, caller: Address, new_delay: u64) -> Result<(), TimelockError> {
        require_initialized(&env)?;
        require_admin(&env, &caller)?;
        validate_delay(new_delay)?;

        let old_delay = read_min_delay(&env);
        env.storage()
            .persistent()
            .set(&StorageKey::MinDelaySeconds, &new_delay);

        env.events()
            .publish(("timelock_delay_updated", old_delay), new_delay);

        Ok(())
    }

    // ── Read Helpers ──────────────────────────────────────────────────────────

    /// @notice Returns the current timelock configuration.
    /// @dev Returns `Err(NotInitialized)` if called before `initialize`.
    /// @return Tuple `(admin_address, min_delay_seconds)`.
    pub fn get_config(env: Env) -> Result<(Address, u64), TimelockError> {
        require_initialized(&env)?;
        let admin = read_admin(&env)?;
        let delay = read_min_delay(&env);
        Ok((admin, delay))
    }

    /// @notice Returns a stored operation by id.
    /// @param op_id The operation identifier.
    /// @return `Some(TimelockedOperation)` if found, `None` otherwise.
    pub fn get_operation(env: Env, op_id: u128) -> Option<TimelockedOperation> {
        env.storage()
            .persistent()
            .get(&StorageKey::Operation(op_id))
    }

    /// @notice Returns a paginated and optionally filtered list of operations
    ///         created by the given address.
    /// @dev `limit` is silently capped to [`MAX_PAGE_SIZE`] (100).
    ///      Returns a next cursor for resumable iteration.
    /// @param owner  The creator address to list operations for.
    /// @param status Optional status filter (e.g., only `Queued` operations).
    /// @param start  1-based start position in the owner's history (inclusive).
    /// @param limit  Maximum number of operations to return.
    /// @return `OperationPage` containing operations and the next cursor.
    pub fn get_operations_for(
        env: Env,
        owner: Address,
        status: Option<OperationStatus>,
        start: Option<u32>,
        limit: Option<u32>,
    ) -> OperationPage {
        let total_count: u32 = env
            .storage()
            .persistent()
            .get(&StorageKey::OperationsCount(owner.clone()))
            .unwrap_or(0);

        let mut operations = Vec::new(&env);
        let start_pos = start.unwrap_or(1).max(1);

        if start_pos > total_count {
            return OperationPage {
                operations,
                next_cursor: None,
            };
        }

        let effective_limit = limit.unwrap_or(MAX_PAGE_SIZE).min(MAX_PAGE_SIZE);
        let mut next_cursor = None;
        let mut found_count = 0;

        for i in start_pos..=total_count {
            if found_count >= effective_limit {
                next_cursor = Some(i);
                break;
            }

            let op_id: u128 = env
                .storage()
                .persistent()
                .get(&StorageKey::OperationAt(owner.clone(), i))
                .unwrap();

            let op = read_operation(&env, op_id).unwrap();

            if let Some(ref s) = status {
                if op.status == *s {
                    operations.push_back(op);
                    found_count += 1;
                }
            } else {
                operations.push_back(op);
                found_count += 1;
            }
        }

        OperationPage {
            operations,
            next_cursor,
        }
    }

    /// @notice Returns the number of currently active (`Queued`) operations.
    /// @dev O(1) — maintained as a persistent counter. Incremented by `queue`;
    ///      decremented by `execute` and `cancel`. Returns 0 before any queues.
    /// @return `u32` count of pending operations.
    pub fn get_queued_count(env: Env) -> u32 {
        read_queued_count(&env)
    }
}
