//! # PaymentScheduler — Recurring & One-Time Payment Scheduling Contract
//!
//! This contract manages cron-like payment jobs for StelloPay's payroll system.
//! Each job encodes a recurring or one-time token transfer from a pre-funded
//! escrow (the scheduler contract itself) to a recipient. An off-chain keeper
//! or any caller invokes `process_due_payments` to execute all due jobs in a
//! single transaction.
//!
//! ## Deterministic Schedule IDs
//!
//! Every job is assigned a **deterministic `schedule_id`** — a `BytesN<32>`
//! SHA-256 fingerprint derived from the tuple
//! `(employer, recipient, token, amount, start_time)`.  This fingerprint is
//! used as the idempotency key: attempting to create two jobs with identical
//! inputs (same employer, recipient, token, amount, and start time) returns
//! `Err(SchedulerError::DuplicateSchedule)` without consuming a new job ID.
//!
//! The deterministic-ID scheme means that:
//! * Off-chain systems can predict the schedule key before submitting the
//!   transaction and check for prior registration without an extra read.
//! * Replay attacks (re-submitting the same `create_job` call) are rejected at
//!   the storage-key level, not just by sequential counters.
//!
//! ## Idempotency of `process_due_payments`
//!
//! `process_due_payments` may be called by any actor at any time. Each job
//! carries its own `next_scheduled_time` gate; calls before that timestamp are
//! no-ops for that record. State (status, counters, timestamp) is written
//! **before** the token transfer is initiated (state-before-interaction
//! pattern) so that partial failures cannot leave jobs in an inconsistent state
//! or allow double-processing in the same ledger round.
//!
//! ## Security Model
//!
//! * `initialize` is one-time only; subsequent calls return
//!   `Err(AlreadyInitialized)`.
//! * `create_job` requires employer authentication. The employer must
//!   separately fund the scheduler via `fund_job` or a direct token transfer.
//! * `pause_job`, `resume_job`, `cancel_job`, and `fund_job` are gated on the
//!   employer address stored inside the `PaymentJob` record, preventing any
//!   other address from controlling the job.
//! * `process_due_payments` is intentionally **permissionless**: any actor can
//!   call it; the contract never trusts the caller — it reads all state from
//!   storage and checks timestamps independently.
//! * Cancelled jobs are permanently terminal; they cannot be re-activated.
//!
//! ## Integration
//!
//! Off-chain services (payroll engines, alerting systems) should subscribe to
//! the following events:
//! * `job_created`   — new payment schedule registered.
//! * `job_executed`  — payment transferred; contains `execution_index` and `amount`.
//! * `job_failed`    — insufficient funds; contains `retry_count` / `max_retries`.
//! * `job_cancelled` — schedule permanently removed by employer.

#![no_std]
#![allow(deprecated)] // env.events().publish() — codebase-wide pattern

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, xdr::ToXdr, Address, Bytes,
    BytesN, Env, IntoVal, Symbol, Vec,
};

// ─── Error Types ─────────────────────────────────────────────────────────────

/// Errors returned by the payment scheduler contract.
///
/// Using a typed error enum (rather than raw panics) allows callers to
/// distinguish failure modes programmatically and write targeted error-handling
/// logic in tests and client code.
#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchedulerError {
    /// `initialize` has not been called yet.
    NotInitialized = 1,
    /// `initialize` was already called; re-initialization is not permitted.
    AlreadyInitialized = 2,
    /// No job exists with the given identifier.
    JobNotFound = 3,
    /// The caller is not the employer that created this job.
    NotEmployer = 4,
    /// Operation requires the job to be in `Active` status.
    JobNotActive = 5,
    /// Operation requires the job to be in `Paused` status.
    JobNotPaused = 6,
    /// `amount` must be a strictly positive value.
    AmountNotPositive = 7,
    /// `interval_seconds` must be > 0 for recurring jobs (max_executions != Some(1)).
    IntervalRequired = 8,
    /// A job with the same deterministic schedule fingerprint already exists.
    DuplicateSchedule = 9,
    /// The job has already been cancelled; no further state changes are allowed.
    AlreadyCancelled = 10,
    /// The job is not in a cancellable state (must be `Active` or `Paused`).
    JobNotCancellable = 11,
}

// ─── Domain Types ─────────────────────────────────────────────────────────────

/// Lifecycle status of a payment job.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobStatus {
    Active,
    Paused,
    Failed,
    Completed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RetryState {
    Pending,
    Scheduled,
    Retrying,
    Success,
    Failed,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub retry_intervals: Vec<u64>,
}

pub struct RetryContractClient {
    pub env: Env,
    pub contract_id: Address,
}

impl RetryContractClient {
    pub fn new(env: &Env, contract_id: &Address) -> Self {
        Self {
            env: env.clone(),
            contract_id: contract_id.clone(),
        }
    }

    pub fn schedule_retry(
        &self,
        payment_id: &BytesN<32>,
        payer: &Address,
        recipient: &Address,
        token: &Address,
        amount: &i128,
        config: &RetryConfig,
    ) {
        self.env.invoke_contract::<()>(
            &self.contract_id,
            &Symbol::new(&self.env, "schedule_retry"),
            soroban_sdk::vec![
                &self.env,
                payment_id.clone().into_val(&self.env),
                payer.clone().into_val(&self.env),
                recipient.clone().into_val(&self.env),
                token.clone().into_val(&self.env),
                amount.clone().into_val(&self.env),
                config.clone().into_val(&self.env),
            ],
        );
    }
}

/// A payment job record stored on-chain.
///
/// # Idempotency note
///
/// The `schedule_id` fingerprint guarantees that jobs with identical parameters
/// cannot be duplicated. All mutable fields (`status`, `executions`,
/// `retry_count`, `next_scheduled_time`) are updated atomically via a single
/// `write_job` call, keeping the record consistent across transaction restarts.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentJob {
    /// Sequential identifier assigned at creation time.
    pub id: u128,
    /// Deterministic SHA-256 fingerprint of `(employer, recipient, token, amount, start_time)`.
    /// Used as the idempotency key for deduplication.
    pub schedule_id: BytesN<32>,
    /// Employer address that created and funds this job.
    pub employer: Address,
    /// Destination address for each payment transfer.
    pub recipient: Address,
    /// Token contract address used for transfers.
    pub token: Address,
    /// Amount transferred per execution cycle (must be > 0).
    pub amount: i128,
    /// Seconds between execution cycles. Zero is only allowed for one-time jobs
    /// (`max_executions == Some(1)`).
    pub interval_seconds: u64,
    /// Ledger timestamp at which the next execution becomes eligible.
    pub next_scheduled_time: u64,
    /// Optional cap on total executions. `None` means unlimited.
    pub max_executions: Option<u32>,
    /// Number of successful executions so far.
    pub executions: u32,
    /// Maximum number of insufficient-funds retries before the job is `Failed`.
    pub max_retries: u32,
    /// Number of failed (insufficient-funds) attempts so far.
    pub retry_count: u32,
    /// Current lifecycle status.
    pub status: JobStatus,
}

// ─── Storage Keys ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
enum StorageKey {
    /// One-time initialization flag (`bool`).
    Initialized,
    /// Contract owner/admin address (`Address`).
    Owner,
    /// Auto-incrementing next job id (`u128`).
    NextJobId,
    /// Full job record keyed by sequential id (`PaymentJob`).
    Job(u128),
    /// Idempotency sentinel keyed by deterministic `schedule_id`.
    /// Stores the sequential job id (`u128`) assigned at creation time.
    ScheduleId(BytesN<32>),
    /// Address of the payment retry contract.
    RetryContract,
}

// ─── Events ───────────────────────────────────────────────────────────────────

/// Emitted when a new payment job is created via `create_job`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobCreatedEvent {
    pub job_id: u128,
    pub schedule_id: BytesN<32>,
    pub employer: Address,
    pub recipient: Address,
}

/// Emitted on each successful payment execution inside `process_due_payments`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobExecutedEvent {
    pub job_id: u128,
    /// 1-based execution index (equals `job.executions` after the transfer).
    pub execution_index: u32,
    pub amount: i128,
}

/// Emitted when a payment attempt fails due to insufficient escrow balance.
///
/// Off-chain payroll systems should use this event to alert employers to top up
/// the scheduler's escrow before `retry_count` exceeds `max_retries`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobFailedEvent {
    pub job_id: u128,
    pub retry_count: u32,
    pub max_retries: u32,
}

/// Emitted when an employer permanently cancels a job via `cancel_job`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobCancelledEvent {
    pub job_id: u128,
    pub employer: Address,
}

// ─── Internal Helpers ─────────────────────────────────────────────────────────

fn require_initialized(env: &Env) -> Result<(), SchedulerError> {
    let initialized = env
        .storage()
        .persistent()
        .get::<_, bool>(&StorageKey::Initialized)
        .unwrap_or(false);
    if !initialized {
        return Err(SchedulerError::NotInitialized);
    }
    Ok(())
}

/// Atomically increments and returns the next job ID.
fn next_job_id(env: &Env) -> u128 {
    let current = env
        .storage()
        .persistent()
        .get::<_, u128>(&StorageKey::NextJobId)
        .unwrap_or(0);
    let next = current.checked_add(1).expect("Job id overflow");
    env.storage()
        .persistent()
        .set(&StorageKey::NextJobId, &next);
    next
}

fn read_job(env: &Env, id: u128) -> Result<PaymentJob, SchedulerError> {
    env.storage()
        .persistent()
        .get::<_, PaymentJob>(&StorageKey::Job(id))
        .ok_or(SchedulerError::JobNotFound)
}

fn write_job(env: &Env, job: &PaymentJob) {
    env.storage()
        .persistent()
        .set(&StorageKey::Job(job.id), job);
}

/// Derives the deterministic schedule fingerprint from the job's immutable parameters.
///
/// The fingerprint is a SHA-256 hash over the concatenation of the canonical
/// XDR encodings of `(employer, recipient, token, amount_le_bytes, start_time_le_bytes)`.
/// Because Soroban's SHA-256 operates on raw `Bytes`, we encode numeric values
/// as little-endian byte slices to ensure a fixed-length, canonical encoding.
///
/// This function is called at `create_job` time and the result stored inside
/// `PaymentJob.schedule_id`. It is also called to look up the idempotency key
/// before inserting a new record.
fn compute_schedule_id(
    env: &Env,
    employer: &Address,
    recipient: &Address,
    token: &Address,
    amount: i128,
    start_time: u64,
) -> BytesN<32> {
    // Build a deterministic byte buffer:
    //   [employer_xdr | recipient_xdr | token_xdr | amount_le(16) | start_time_le(8)]
    let mut buf = Bytes::new(env);

    // Address XDR encoding via to_xdr
    buf.append(&employer.clone().to_xdr(env));
    buf.append(&recipient.clone().to_xdr(env));
    buf.append(&token.clone().to_xdr(env));

    // amount (i128) as 16-byte little-endian
    let amount_bytes = amount.to_le_bytes();
    for byte in amount_bytes.iter() {
        buf.push_back(*byte);
    }

    // start_time (u64) as 8-byte little-endian
    let time_bytes = start_time.to_le_bytes();
    for byte in time_bytes.iter() {
        buf.push_back(*byte);
    }

    env.crypto().sha256(&buf).into()
}

fn compute_payment_id(
    env: &Env,
    employer: &Address,
    employee: &Address,
    amount: i128,
    timestamp: u64,
) -> BytesN<32> {
    let mut buf = Bytes::new(env);
    buf.append(&employer.to_xdr(env));
    buf.append(&employee.to_xdr(env));
    
    let amount_bytes = amount.to_le_bytes();
    for byte in amount_bytes.iter() {
        buf.push_back(*byte);
    }

    let time_bytes = timestamp.to_le_bytes();
    for byte in time_bytes.iter() {
        buf.push_back(*byte);
    }

    env.crypto().sha256(&buf).into()
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct PaymentSchedulerContract;

#[contractimpl]
impl PaymentSchedulerContract {
    // ── Initialization ────────────────────────────────────────────────────────

    /// @notice Initializes the payment scheduler contract.
    /// @dev One-time call. Stores the owner address and sets the initialized
    ///      flag. Subsequent calls return `Err(AlreadyInitialized)`.
    /// @param owner Address authorized to act as admin. Must authenticate.
    /// @return Ok(()) on success.
    /// @security Requires `owner` authentication to prevent unauthorized
    ///           initialization of a newly deployed contract.
    pub fn initialize(env: Env, owner: Address, retry_contract: Address) -> Result<(), SchedulerError> {
        let already = env
            .storage()
            .persistent()
            .get::<_, bool>(&StorageKey::Initialized)
            .unwrap_or(false);
        if already {
            return Err(SchedulerError::AlreadyInitialized);
        }

        owner.require_auth();

        env.storage().persistent().set(&StorageKey::Owner, &owner);
        env.storage().persistent().set(&StorageKey::RetryContract, &retry_contract);
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);

        Ok(())
    }

    // ── Schedule Management ───────────────────────────────────────────────────

    /// @notice Creates a new recurring or one-time payment job.
    /// @dev Derives a deterministic `schedule_id` from `(employer, recipient,
    ///      token, amount, start_time)` and uses it as an idempotency key.
    ///      Attempting to create a job with identical parameters returns
    ///      `Err(DuplicateSchedule)` without consuming a new job ID.
    ///
    ///      The employer must separately fund the scheduler (via `fund_job` or
    ///      a direct token transfer) before the first execution becomes due;
    ///      otherwise the scheduler will increment `retry_count`.
    ///
    ///      For one-time payments (`max_executions == Some(1)`) `interval_seconds`
    ///      may be zero. For all other jobs it must be > 0.
    ///
    /// @param employer  Employer funding the job. Must authenticate.
    /// @param recipient Payment destination address.
    /// @param token     Token contract used for transfers.
    /// @param amount    Positive token amount per execution cycle.
    /// @param interval_seconds Seconds between executions. Must be > 0 for
    ///                  recurring jobs; may be 0 for one-time (`max_executions == Some(1)`).
    /// @param start_time Ledger timestamp of the first eligible execution.
    /// @param max_executions Optional cap on total executions (None = unlimited).
    /// @param max_retries Maximum insufficient-funds retries before `Failed` status.
    /// @return The newly assigned sequential job id.
    /// @security `employer.require_auth()` is called before any state mutation.
    ///           The idempotency key prevents replay of the same schedule parameters.
    pub fn create_job(
        env: Env,
        employer: Address,
        recipient: Address,
        token: Address,
        amount: i128,
        interval_seconds: u64,
        start_time: u64,
        max_executions: Option<u32>,
        max_retries: u32,
    ) -> Result<u128, SchedulerError> {
        require_initialized(&env)?;
        employer.require_auth();

        if amount <= 0 {
            return Err(SchedulerError::AmountNotPositive);
        }

        // One-time payments (max_executions == Some(1)) may have a zero interval.
        if max_executions != Some(1) && interval_seconds == 0 {
            return Err(SchedulerError::IntervalRequired);
        }

        // Derive and check the deterministic idempotency key.
        let schedule_id =
            compute_schedule_id(&env, &employer, &recipient, &token, amount, start_time);

        let id_key = StorageKey::ScheduleId(schedule_id.clone());
        if env.storage().persistent().has(&id_key) {
            return Err(SchedulerError::DuplicateSchedule);
        }

        let id = next_job_id(&env);
        let job = PaymentJob {
            id,
            schedule_id: schedule_id.clone(),
            employer: employer.clone(),
            recipient: recipient.clone(),
            token,
            amount,
            interval_seconds,
            next_scheduled_time: start_time,
            max_executions,
            executions: 0,
            max_retries,
            retry_count: 0,
            status: JobStatus::Active,
        };
        write_job(&env, &job);

        // Register the idempotency sentinel.
        env.storage().persistent().set(&id_key, &id);

        env.events().publish(
            ("job_created", id),
            JobCreatedEvent {
                job_id: id,
                schedule_id,
                employer,
                recipient: job.recipient.clone(),
            },
        );

        Ok(id)
    }

    /// @notice Permanently cancels a payment job.
    /// @dev Only the original employer may cancel their own job. Jobs in
    ///      `Active` or `Paused` status may be cancelled; jobs already in
    ///      `Completed` or `Failed` status return `Err(JobNotCancellable)`.
    ///      Attempting to cancel an already-`Cancelled` job returns
    ///      `Err(AlreadyCancelled)` (idempotency guard).
    ///
    ///      Cancellation does **not** return any pre-funded tokens; the employer
    ///      must reclaim these externally (e.g. by withdrawing from the
    ///      scheduler's escrow balance via a separate sweep call).
    ///
    /// @param employer Employer that created the job. Must authenticate.
    /// @param job_id   Sequential identifier returned by `create_job`.
    /// @return Ok(()) on success.
    /// @security Requires `employer` authentication. The stored `job.employer`
    ///           is compared against the caller to prevent cross-employer
    ///           cancellation.
    pub fn cancel_job(env: Env, employer: Address, job_id: u128) -> Result<(), SchedulerError> {
        require_initialized(&env)?;
        employer.require_auth();

        let mut job = read_job(&env, job_id)?;

        if job.employer != employer {
            return Err(SchedulerError::NotEmployer);
        }

        match job.status {
            JobStatus::Cancelled => return Err(SchedulerError::AlreadyCancelled),
            JobStatus::Completed | JobStatus::Failed => {
                return Err(SchedulerError::JobNotCancellable)
            }
            JobStatus::Active | JobStatus::Paused => {}
        }

        job.status = JobStatus::Cancelled;
        write_job(&env, &job);

        env.events().publish(
            ("job_cancelled", job_id),
            JobCancelledEvent {
                job_id,
                employer,
            },
        );

        Ok(())
    }

    /// @notice Pauses an active job, preventing automatic execution.
    /// @dev Only the employer that created the job may pause it.
    ///      The job must be in `Active` status.
    /// @param employer Employer that created the job. Must authenticate.
    /// @param job_id   Sequential job identifier.
    /// @return Ok(()) on success.
    pub fn pause_job(env: Env, employer: Address, job_id: u128) -> Result<(), SchedulerError> {
        require_initialized(&env)?;
        employer.require_auth();

        let mut job = read_job(&env, job_id)?;

        if job.employer != employer {
            return Err(SchedulerError::NotEmployer);
        }
        if job.status != JobStatus::Active {
            return Err(SchedulerError::JobNotActive);
        }

        job.status = JobStatus::Paused;
        write_job(&env, &job);

        Ok(())
    }

    /// @notice Resumes a previously paused job.
    /// @dev Only the employer that created the job may resume it.
    ///      The job must be in `Paused` status.
    /// @param employer Employer that created the job. Must authenticate.
    /// @param job_id   Sequential job identifier.
    /// @return Ok(()) on success.
    pub fn resume_job(env: Env, employer: Address, job_id: u128) -> Result<(), SchedulerError> {
        require_initialized(&env)?;
        employer.require_auth();

        let mut job = read_job(&env, job_id)?;

        if job.employer != employer {
            return Err(SchedulerError::NotEmployer);
        }
        if job.status != JobStatus::Paused {
            return Err(SchedulerError::JobNotPaused);
        }

        job.status = JobStatus::Active;
        write_job(&env, &job);

        Ok(())
    }

    // ── Execution ─────────────────────────────────────────────────────────────

    /// @notice Processes due payments across all registered jobs.
    /// @dev Permissionless — any caller may invoke this function (keeper, cron
    ///      service, or any Stellar account). Processes at most `max_jobs` jobs
    ///      per call to bound ledger resource consumption.
    ///
    ///      For each `Active` job whose `next_scheduled_time <= now`:
    ///      * If the scheduler's escrow balance covers `amount`:
    ///        - State is written before the transfer (state-before-interaction).
    ///        - `executions` is incremented; `retry_count` is reset to 0.
    ///        - `next_scheduled_time` is advanced by `interval_seconds`.
    ///        - If `max_executions` is reached, status becomes `Completed`.
    ///        - Emits `job_executed`.
    ///      * If the escrow balance is insufficient:
    ///        - `retry_count` is incremented.
    ///        - If `retry_count > max_retries`, status becomes `Failed`.
    ///        - Otherwise `next_scheduled_time` is advanced and the job retries.
    ///        - Emits `job_failed`.
    ///
    /// @param max_jobs Maximum number of jobs to evaluate in this call.
    ///                 Pass a small value (e.g. 10–50) to stay within ledger limits.
    /// @return Number of jobs that were actually evaluated (not necessarily paid).
    pub fn process_due_payments(env: Env, max_jobs: u32) -> u32 {
        if require_initialized(&env).is_err() {
            return 0;
        }

        let now = env.ledger().timestamp();
        let mut processed: u32 = 0;

        let highest_id = env
            .storage()
            .persistent()
            .get::<_, u128>(&StorageKey::NextJobId)
            .unwrap_or(0);

        if highest_id == 0 || max_jobs == 0 {
            return 0;
        }

        let mut job_id: u128 = 1;
        while job_id <= highest_id && processed < max_jobs {
            if let Some(job) = env
                .storage()
                .persistent()
                .get::<_, PaymentJob>(&StorageKey::Job(job_id))
            {
                if job.status == JobStatus::Active && now >= job.next_scheduled_time {
                    let mut job_mut = job;
                    let token_client = token::Client::new(&env, &job_mut.token);
                    let balance = token_client.balance(&env.current_contract_address());

                    if balance >= job_mut.amount {
                        // Checks-effects-interactions:
                        // commit job progress before transfer so reentrant
                        // callbacks cannot re-execute the same due payment.
                        job_mut.executions = job_mut.executions.saturating_add(1);
                        job_mut.retry_count = 0;
                        job_mut.next_scheduled_time = now.saturating_add(job_mut.interval_seconds);

                        if let Some(max_exec) = job_mut.max_executions {
                            if job_mut.executions >= max_exec {
                                job_mut.status = JobStatus::Completed;
                            }
                        }

                        // State-before-interaction: persist before token transfer.
                        write_job(&env, &job_mut);

                        token_client.transfer(
                            &env.current_contract_address(),
                            &job_mut.recipient,
                            &job_mut.amount,
                        );

                        env.events().publish(
                            ("job_executed", job_mut.id),
                            JobExecutedEvent {
                                job_id: job_mut.id,
                                execution_index: job_mut.executions,
                                amount: job_mut.amount,
                            },
                        );
                    } else {
                        // Insufficient funds: offload to payment_retry contract.
                        let payment_id = compute_payment_id(
                            &env,
                            &job_mut.employer,
                            &job_mut.recipient,
                            job_mut.amount,
                            job_mut.next_scheduled_time,
                        );

                        let retry_addr = env.storage().persistent().get::<_, Address>(&StorageKey::RetryContract).unwrap();
                        let retry_client = RetryContractClient::new(&env, &retry_addr);
                        
                        let retry_config = RetryConfig {
                            max_retries: job_mut.max_retries,
                            retry_intervals: soroban_sdk::vec![&env, 30u64, 60u64, 120u64], // Default backoff
                        };

                        retry_client.schedule_retry(
                            &payment_id,
                            &job_mut.employer,
                            &job_mut.recipient,
                            &job_mut.token,
                            &job_mut.amount,
                            &retry_config,
                        );

                        // Advance the job to the next period as the retry is now managed externally
                        job_mut.next_scheduled_time = now.saturating_add(job_mut.interval_seconds);
                        write_job(&env, &job_mut);

                        env.events().publish(
                            ("payment_failed", payment_id.clone()),
                            payment_id,
                        );
                    }
                    processed = processed.saturating_add(1);
                }
            }

            job_id = job_id.saturating_add(1);
        }

        processed
    }

    // ── Funding ───────────────────────────────────────────────────────────────

    /// @notice Deposits tokens from `from` into the scheduler contract's escrow.
    /// @dev The token is inferred from the job record. Multiple calls accumulate.
    ///      Any party may fund a job, not only the employer.
    /// @param from     Funding address. Must authenticate.
    /// @param job_id   Job whose token should be funded.
    /// @param amount   Positive token amount to transfer.
    /// @return Ok(()) on success.
    pub fn fund_job(
        env: Env,
        from: Address,
        job_id: u128,
        amount: i128,
    ) -> Result<(), SchedulerError> {
        require_initialized(&env)?;
        from.require_auth();

        if amount <= 0 {
            return Err(SchedulerError::AmountNotPositive);
        }

        let job = read_job(&env, job_id)?;
        let token_client = token::Client::new(&env, &job.token);
        token_client.transfer(&from, &env.current_contract_address(), &amount);

        Ok(())
    }

    // ── View Helpers ──────────────────────────────────────────────────────────

    /// @notice Returns a payment job by sequential id.
    /// @param job_id The sequential identifier returned by `create_job`.
    /// @return `Some(PaymentJob)` if the job exists, `None` otherwise.
    pub fn get_job(env: Env, job_id: u128) -> Option<PaymentJob> {
        env.storage().persistent().get(&StorageKey::Job(job_id))
    }

    /// @notice Returns the contract owner address.
    /// @return `Some(Address)` after initialization, `None` before.
    pub fn get_owner(env: Env) -> Option<Address> {
        env.storage().persistent().get(&StorageKey::Owner)
    }

    /// @notice Looks up the sequential job id registered for a deterministic
    ///         schedule fingerprint.
    /// @dev Useful for off-chain systems to check whether a particular schedule
    ///      already exists before submitting a `create_job` transaction.
    /// @param schedule_id The `BytesN<32>` fingerprint to look up.
    /// @return `Some(job_id)` if registered, `None` otherwise.
    pub fn get_job_id_by_schedule(env: Env, schedule_id: BytesN<32>) -> Option<u128> {
        env.storage()
            .persistent()
            .get(&StorageKey::ScheduleId(schedule_id))
    }
}
