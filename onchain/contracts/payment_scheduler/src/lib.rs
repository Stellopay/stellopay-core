#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env};

#[contract]
pub struct PaymentSchedulerContract;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobStatus {
    Active,
    Paused,
    Failed,
    Completed,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentJob {
    pub id: u128,
    pub employer: Address,
    pub recipient: Address,
    pub token: Address,
    pub amount: i128,
    pub interval_seconds: u64,
    pub next_scheduled_time: u64,
    pub max_executions: Option<u32>,
    pub executions: u32,
    pub max_retries: u32,
    pub retry_count: u32,
    pub status: JobStatus,
}

#[contracttype]
#[derive(Clone)]
enum StorageKey {
    Initialized,
    Owner,
    NextJobId,
    Job(u128),
    JobConflict(Address, Address, u64),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobCreatedEvent {
    pub job_id: u128,
    pub employer: Address,
    pub recipient: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobExecutedEvent {
    pub job_id: u128,
    pub execution_index: u32,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobFailedEvent {
    pub job_id: u128,
    pub retry_count: u32,
    pub max_retries: u32,
}

fn require_initialized(env: &Env) {
    let initialized = env
        .storage()
        .persistent()
        .get::<_, bool>(&StorageKey::Initialized)
        .unwrap_or(false);
    assert!(initialized, "Contract not initialized");
}

fn read_owner(env: &Env) -> Address {
    env.storage()
        .persistent()
        .get::<_, Address>(&StorageKey::Owner)
        .expect("Owner not set")
}

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

fn read_job(env: &Env, id: u128) -> PaymentJob {
    env.storage()
        .persistent()
        .get::<_, PaymentJob>(&StorageKey::Job(id))
        .expect("Job not found")
}

fn write_job(env: &Env, job: &PaymentJob) {
    env.storage()
        .persistent()
        .set(&StorageKey::Job(job.id), job);
}

#[contractimpl]
impl PaymentSchedulerContract {
    /// @notice Initializes the payment scheduler.
    /// @dev Must be called once by the admin/owner.
    /// @param owner Address allowed to perform admin operations.
    pub fn initialize(env: Env, owner: Address) {
        owner.require_auth();

        let initialized = env
            .storage()
            .persistent()
            .get::<_, bool>(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(!initialized, "Contract already initialized");

        env.storage().persistent().set(&StorageKey::Owner, &owner);
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);
    }

    /// @notice Creates a new recurring or one-time payment job.
    /// @dev Employer optionally pre-funds the contract; jobs are executed
    ///      by calling `process_due_payments`.
    /// @param employer Employer funding the job; must authenticate.
    /// @param recipient Payment recipient.
    /// @param token Token contract used for payments.
    /// @param amount Amount per execution.
    /// @param interval_seconds Time between executions (can be 0 for one-time payments).
    /// @param start_time First execution timestamp.
    /// @param max_executions Optional maximum number of executions (None = unlimited).
    /// @param max_retries Maximum retry attempts for insufficient funds.
    /// @return u128
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
    ) -> u128 {
        require_initialized(&env);
        employer.require_auth();

        assert!(amount > 0, "Amount must be positive");

        // SUPPORT ONE-TIME PAYMENTS: Allow 0 interval only if max_executions is 1
        if max_executions != Some(1) {
            assert!(
                interval_seconds > 0,
                "Interval must be positive for recurring jobs"
            );
        }

        // CONFLICT DETECTION: Prevent identical jobs for the same recipient at the exact same start time
        let conflict_key = StorageKey::JobConflict(employer.clone(), recipient.clone(), start_time);
        assert!(
            !env.storage().persistent().has(&conflict_key),
            "Conflict: identical job scheduled for this time"
        );

        let id = next_job_id(&env);
        let job = PaymentJob {
            id,
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

        // Record the conflict footprint to prevent duplicates
        env.storage().persistent().set(&conflict_key, &id);

        env.events().publish(
            ("job_created", id),
            JobCreatedEvent {
                job_id: id,
                employer,
                recipient: job.recipient.clone(),
            },
        );

        id
    }

    /// @notice Processes due payments across all jobs.
    /// @dev Anyone can call this; it acts like a cron trigger.
    /// @param max_jobs Maximum number of jobs to process in this call.
    /// @return processed Number of jobs that executed or attempted retries.
    pub fn process_due_payments(env: Env, max_jobs: u32) -> u32 {
        require_initialized(&env);

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

                    if balance < job_mut.amount {
                        // Insufficient funds: schedule retry or mark failed.
                        job_mut.retry_count = job_mut.retry_count.saturating_add(1);
                        if job_mut.retry_count > job_mut.max_retries {
                            job_mut.status = JobStatus::Failed;
                        } else {
                            job_mut.next_scheduled_time =
                                now.saturating_add(job_mut.interval_seconds);
                        }
                        env.events().publish(
                            ("job_failed", job_mut.id),
                            JobFailedEvent {
                                job_id: job_mut.id,
                                retry_count: job_mut.retry_count,
                                max_retries: job_mut.max_retries,
                            },
                        );
                        write_job(&env, &job_mut);
                        processed = processed.saturating_add(1);
                    } else {
                        // Execute payment
                        token_client.transfer(
                            &env.current_contract_address(),
                            &job_mut.recipient,
                            &job_mut.amount,
                        );
                        job_mut.executions = job_mut.executions.saturating_add(1);
                        job_mut.retry_count = 0;
                        job_mut.next_scheduled_time = now.saturating_add(job_mut.interval_seconds);

                        if let Some(max_exec) = job_mut.max_executions {
                            if job_mut.executions >= max_exec {
                                job_mut.status = JobStatus::Completed;
                            }
                        }

                        env.events().publish(
                            ("job_executed", job_mut.id),
                            JobExecutedEvent {
                                job_id: job_mut.id,
                                execution_index: job_mut.executions,
                                amount: job_mut.amount,
                            },
                        );
                        write_job(&env, &job_mut);
                        processed = processed.saturating_add(1);
                    }
                }
            }

            job_id = job_id.saturating_add(1);
        }

        processed
    }

    /// @notice Pauses a job, preventing automatic execution.
    /// @param employer Employer that created the job; must authenticate.
    /// @param job_id Job identifier.
    pub fn pause_job(env: Env, employer: Address, job_id: u128) {
        require_initialized(&env);
        employer.require_auth();

        let mut job = read_job(&env, job_id);
        assert!(job.employer == employer, "Only employer can pause");
        assert!(job.status == JobStatus::Active, "Job not active");

        job.status = JobStatus::Paused;
        write_job(&env, &job);
    }

    /// @notice Resumes a previously paused job.
    /// @param employer Employer that created the job; must authenticate.
    /// @param job_id Job identifier.
    pub fn resume_job(env: Env, employer: Address, job_id: u128) {
        require_initialized(&env);
        employer.require_auth();

        let mut job = read_job(&env, job_id);
        assert!(job.employer == employer, "Only employer can resume");
        assert!(job.status == JobStatus::Paused, "Job not paused");

        job.status = JobStatus::Active;
        write_job(&env, &job);
    }

    /// @notice Funds a job by transferring tokens into the scheduler contract.
    /// @param from Funding address; must authenticate.
    /// @param job_id Job identifier.
    /// @param amount Amount to transfer.
    pub fn fund_job(env: Env, from: Address, job_id: u128, amount: i128) {
        require_initialized(&env);
        from.require_auth();
        assert!(amount > 0, "Amount must be positive");

        let job = read_job(&env, job_id);
        let token_client = token::Client::new(&env, &job.token);
        token_client.transfer(&from, &env.current_contract_address(), &amount);
    }

    /// @notice Reads a payment job by id.
    /// @param job_id job_id parameter
    /// @return `Option<PaymentJob>`
    /// @dev Requires caller authentication
    pub fn get_job(env: Env, job_id: u128) -> Option<PaymentJob> {
        env.storage().persistent().get(&StorageKey::Job(job_id))
    }

    /// @notice Returns the contract owner/admin.
    /// @dev Requires caller authentication
    pub fn get_owner(env: Env) -> Option<Address> {
        env.storage().persistent().get(&StorageKey::Owner)
    }
}
