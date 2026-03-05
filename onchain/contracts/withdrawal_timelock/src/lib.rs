#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Vec};

/// Errors for the withdrawal timelock contract.
#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TimelockError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    QueueTooSmall = 4,
    InvalidDelay = 5,
    OperationNotFound = 6,
    NotReady = 7,
    AlreadyExecutedOrCancelled = 8,
}

/// Types of queued timelocked operations.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationKind {
    /// Generic withdrawal from an external treasury or escrow.
    /// Tuple layout: (token, to, amount)
    Withdrawal(Address, Address, i128),
    /// Administrative change on an external contract, represented generically
    /// as an opaque payload.
    /// Tuple layout: (target_contract, payload_hash)
    AdminChange(Address, soroban_sdk::BytesN<32>),
}

/// Status of a queued operation.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationStatus {
    Queued,
    Executed,
    Cancelled,
}

/// A queued timelocked operation.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimelockedOperation {
    pub id: u128,
    pub kind: OperationKind,
    pub creator: Address,
    pub eta: u64,
    pub created_at: u64,
    pub executed_at: Option<u64>,
    pub status: OperationStatus,
}

/// Storage keys for the timelock contract.
#[contracttype]
#[derive(Clone)]
pub enum StorageKey {
    Initialized,
    Admin,
    MinDelaySeconds,
    NextOpId,
    Operation(u128),
    OperationsFor(Address),
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

fn next_op_id(env: &Env) -> u128 {
    let current: u128 = env
        .storage()
        .persistent()
        .get(&StorageKey::NextOpId)
        .unwrap_or(0);
    let next = current.checked_add(1).expect("op id overflow");
    env.storage()
        .persistent()
        .set(&StorageKey::NextOpId, &next);
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

fn push_operation_for(env: &Env, owner: &Address, id: u128) {
    let mut ids: Vec<u128> = env
        .storage()
        .persistent()
        .get(&StorageKey::OperationsFor(owner.clone()))
        .unwrap_or(Vec::new(env));
    ids.push_back(id);
    env.storage()
        .persistent()
        .set(&StorageKey::OperationsFor(owner.clone()), &ids);
}

#[contract]
pub struct WithdrawalTimelock;

#[contractimpl]
impl WithdrawalTimelock {
    /// @notice Initializes the withdrawal timelock contract.
    /// @dev Sets the admin and the global minimum delay for queued operations.
    /// @param admin Address allowed to queue and execute operations.
    /// @param min_delay_seconds Minimum delay between queue time and executable time.
    pub fn initialize(env: Env, admin: Address, min_delay_seconds: u64) -> Result<(), TimelockError> {
        if env
            .storage()
            .persistent()
            .get::<_, bool>(&StorageKey::Initialized)
            .unwrap_or(false)
        {
            return Err(TimelockError::AlreadyInitialized);
        }
        if min_delay_seconds == 0 {
            return Err(TimelockError::InvalidDelay);
        }
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

    /// @notice Queues a new timelocked operation.
    /// @dev Only admin may queue. The `eta` is automatically computed as
    ///      `now + min_delay_seconds` and must be reached before execution.
    /// @param caller Admin queuing the operation; must authenticate.
    /// @param kind Encoded operation kind (withdrawal/admin change).
    /// @return op_id Newly queued operation identifier.
    pub fn queue(
        env: Env,
        caller: Address,
        kind: OperationKind,
    ) -> Result<u128, TimelockError> {
        require_initialized(&env)?;
        require_admin(&env, &caller)?;

        let min_delay = read_min_delay(&env);
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
            kind,
            creator: caller.clone(),
            eta,
            created_at: now,
            executed_at: None,
            status: OperationStatus::Queued,
        };

        write_operation(&env, &op);
        push_operation_for(&env, &caller, id);

        Ok(id)
    }

    /// @notice Executes a ready timelocked operation.
    /// @dev Only admin may execute, and only after `eta` has been reached.
    ///      This contract records execution metadata and emits events; actual
    ///      token transfers or admin changes are expected to be performed by
    ///      external orchestrators using the recorded intent.
    /// @param caller Admin executing the operation; must authenticate.
    /// @param op_id Operation identifier.
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

        env.events()
            .publish(("timelock_executed", op_id), op.kind.clone());

        Ok(())
    }

    /// @notice Cancels a queued operation.
    /// @dev Only admin may cancel a queued operation; executed operations
    ///      cannot be cancelled.
    /// @param caller Admin requesting cancellation; must authenticate.
    /// @param op_id Operation identifier.
    pub fn cancel(env: Env, caller: Address, op_id: u128) -> Result<(), TimelockError> {
        require_initialized(&env)?;
        require_admin(&env, &caller)?;

        let mut op = read_operation(&env, op_id)?;
        if op.status != OperationStatus::Queued {
            return Err(TimelockError::AlreadyExecutedOrCancelled);
        }

        op.status = OperationStatus::Cancelled;
        write_operation(&env, &op);

        env.events()
            .publish(("timelock_cancelled", op_id), ());

        Ok(())
    }

    /// @notice Returns the current timelock configuration.
    pub fn get_config(env: Env) -> Result<(Address, u64), TimelockError> {
        require_initialized(&env)?;
        let admin = read_admin(&env)?;
        let delay = read_min_delay(&env);
        Ok((admin, delay))
    }

    /// @notice Returns an operation by id, if any.
    pub fn get_operation(env: Env, op_id: u128) -> Option<TimelockedOperation> {
        env.storage()
            .persistent()
            .get(&StorageKey::Operation(op_id))
    }

    /// @notice Returns queued operation ids created by the given admin address.
    pub fn get_operations_for(env: Env, owner: Address) -> Vec<u128> {
        env.storage()
            .persistent()
            .get(&StorageKey::OperationsFor(owner))
            .unwrap_or(Vec::new(&env))
    }
}

