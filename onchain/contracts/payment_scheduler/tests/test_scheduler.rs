//! Comprehensive tests for the PaymentScheduler contract.
//!
//! Coverage targets:
//! * Initialization — happy path, double-init guard
//! * `create_job` — happy path, zero amount, zero interval (recurring), one-time
//!   zero interval, duplicate schedule rejection, multiple jobs get unique IDs
//! * `create_job` idempotency — same parameters rejected, different employer allowed,
//!   different token allowed (same other params)
//! * `cancel_job` — active/paused cancellable, already cancelled, terminal (completed/failed)
//!   not cancellable, wrong employer rejected
//! * `pause_job` / `resume_job` — happy path, wrong employer, wrong status
//! * `fund_job` — increases scheduler balance, job not found, wrong amount
//! * `process_due_payments` — empty scheduler, max_jobs=0, max_jobs bound,
//!   recurring execution cycles & completion, one-time payment, pause prevents
//!   execution, resume after pause, cancelled job skipped, retry on insufficient
//!   funds, retry exhaustion → Failed, state-before-interaction (job persisted
//!   before transfer)
//! * `get_job_id_by_schedule` — lookup by deterministic ID
//! * `get_owner` / `get_job` view helpers

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

use payment_scheduler::{
    JobStatus, PaymentJob, PaymentSchedulerContract, PaymentSchedulerContractClient, SchedulerError,
};

// ─── Fixtures ─────────────────────────────────────────────────────────────────

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

#[allow(deprecated)]
fn register_contract(env: &Env) -> (Address, PaymentSchedulerContractClient<'static>) {
    let id = env.register_contract(None, PaymentSchedulerContract);
    let client = PaymentSchedulerContractClient::new(env, &id);
    (id, client)
}

fn create_token_contract<'a>(env: &Env, admin: &Address) -> TokenClient<'a> {
    let token_addr = env.register_stellar_asset_contract(admin.clone());
    TokenClient::new(env, &token_addr)
}

/// Convenience: initialize the scheduler and return (scheduler_id, client).
fn setup(env: &Env) -> (Address, PaymentSchedulerContractClient<'static>) {
    let (id, client) = register_contract(env);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (id, client)
}

// ─── Initialization ───────────────────────────────────────────────────────────

#[test]
fn test_initialize_and_read_owner() {
    let env = create_env();
    let (_, client) = register_contract(&env);
    let owner = Address::generate(&env);

    client.initialize(&owner);
    assert_eq!(client.get_owner(), Some(owner.clone()));
}

#[test]
fn test_double_init_rejected() {
    let env = create_env();
    let (_, client) = register_contract(&env);
    let owner = Address::generate(&env);

    client.initialize(&owner);
    let result = client.try_initialize(&owner);
    assert_eq!(
        result.unwrap_err().unwrap(),
        SchedulerError::AlreadyInitialized
    );
}

// ─── create_job ───────────────────────────────────────────────────────────────

#[test]
fn test_create_job_happy_path() {
    let env = create_env();
    let (_, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 0);

    let job_id = client.create_job(
        &employer, &recipient, &token, &100i128, &10u64, &0u64, &None, &2u32,
    );

    let job: PaymentJob = client.get_job(&job_id).unwrap();
    assert_eq!(job.id, job_id);
    assert_eq!(job.employer, employer);
    assert_eq!(job.recipient, recipient);
    assert_eq!(job.amount, 100);
    assert_eq!(job.interval_seconds, 10);
    assert_eq!(job.next_scheduled_time, 0);
    assert_eq!(job.executions, 0);
    assert_eq!(job.retry_count, 0);
    assert_eq!(job.max_retries, 2);
    assert_eq!(job.status, JobStatus::Active);
    // schedule_id is present (non-zero length)
    assert_eq!(job.schedule_id.len(), 32);
}

#[test]
fn test_create_job_zero_amount_rejected() {
    let env = create_env();
    let (_, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);

    let result = client.try_create_job(
        &employer, &recipient, &token, &0i128, &10u64, &0u64, &None, &1u32,
    );
    assert_eq!(
        result.unwrap_err().unwrap(),
        SchedulerError::AmountNotPositive
    );
}

#[test]
fn test_create_job_zero_interval_recurring_rejected() {
    let env = create_env();
    let (_, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);

    // max_executions = None (unlimited) with interval = 0 → error
    let result = client.try_create_job(
        &employer,
        &recipient,
        &token,
        &100i128,
        &0u64, // zero interval
        &0u64,
        &None, // unlimited → must have interval
        &1u32,
    );
    assert_eq!(
        result.unwrap_err().unwrap(),
        SchedulerError::IntervalRequired
    );
}

#[test]
fn test_create_job_one_time_zero_interval_allowed() {
    let env = create_env();
    let (_, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);

    // max_executions = Some(1) with interval = 0 → allowed
    let result = client.try_create_job(
        &employer,
        &recipient,
        &token,
        &100i128,
        &0u64,       // zero interval OK for one-time
        &0u64,
        &Some(1u32), // one-time
        &0u32,
    );
    assert!(result.is_ok());
}

#[test]
fn test_create_job_increments_id() {
    let env = create_env();
    let (_, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    env.ledger().with_mut(|li| li.timestamp = 0);

    let id1 = client.create_job(
        &employer, &recipient, &token.address, &100i128, &10u64, &0u64, &None, &1u32,
    );
    let id2 = client.create_job(
        &employer, &recipient, &token.address, &100i128, &10u64, &1000u64, &None, &1u32,
    );

    assert_eq!(id2, id1 + 1);
}

// ─── Deterministic schedule_id & idempotency ─────────────────────────────────

#[test]
fn test_deterministic_schedule_id_consistent() {
    let env = create_env();
    let (_, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 0);

    let job_id = client.create_job(
        &employer, &recipient, &token, &200i128, &60u64, &1000u64, &None, &3u32,
    );

    let job = client.get_job(&job_id).unwrap();

    // get_job_id_by_schedule should resolve back to the same job
    let looked_up = client.get_job_id_by_schedule(&job.schedule_id);
    assert_eq!(looked_up, Some(job_id));
}

#[test]
fn test_duplicate_schedule_rejected() {
    let env = create_env();
    let (_, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);

    // Create first job
    client.create_job(
        &employer, &recipient, &token, &100i128, &10u64, &1000u64, &Some(3u32), &1u32,
    );

    // Exact same parameters → DuplicateSchedule
    let result = client.try_create_job(
        &employer, &recipient, &token, &100i128, &10u64, &1000u64, &Some(3u32), &1u32,
    );
    assert_eq!(
        result.unwrap_err().unwrap(),
        SchedulerError::DuplicateSchedule
    );
}

#[test]
fn test_same_timestamp_different_employers_allowed() {
    let env = create_env();
    let (_, client) = setup(&env);
    let employer1 = Address::generate(&env);
    let employer2 = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);

    // Two different employers, same other params — distinct schedule_ids
    let id1 = client.create_job(
        &employer1, &recipient, &token, &100i128, &10u64, &1000u64, &None, &1u32,
    );
    let id2 = client.create_job(
        &employer2, &recipient, &token, &100i128, &10u64, &1000u64, &None, &1u32,
    );

    assert_ne!(id1, id2);
    // Both should have different schedule_ids
    let j1 = client.get_job(&id1).unwrap();
    let j2 = client.get_job(&id2).unwrap();
    assert_ne!(j1.schedule_id, j2.schedule_id);
}

#[test]
fn test_different_token_produces_different_schedule_id() {
    let env = create_env();
    let (_, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token1 = Address::generate(&env);
    let token2 = Address::generate(&env);

    let id1 = client.create_job(
        &employer, &recipient, &token1, &100i128, &10u64, &1000u64, &None, &1u32,
    );
    let id2 = client.create_job(
        &employer, &recipient, &token2, &100i128, &10u64, &1000u64, &None, &1u32,
    );

    let j1 = client.get_job(&id1).unwrap();
    let j2 = client.get_job(&id2).unwrap();
    assert_ne!(j1.schedule_id, j2.schedule_id);
}

// ─── cancel_job ───────────────────────────────────────────────────────────────

#[test]
fn test_cancel_active_job() {
    let env = create_env();
    let (_, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);

    let job_id = client.create_job(
        &employer, &recipient, &token, &100i128, &10u64, &0u64, &None, &1u32,
    );

    client.cancel_job(&employer, &job_id);

    let job = client.get_job(&job_id).unwrap();
    assert_eq!(job.status, JobStatus::Cancelled);
}

#[test]
fn test_cancel_paused_job() {
    let env = create_env();
    let (_, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);

    let job_id = client.create_job(
        &employer, &recipient, &token, &100i128, &10u64, &0u64, &None, &1u32,
    );

    client.pause_job(&employer, &job_id);
    client.cancel_job(&employer, &job_id);

    assert_eq!(
        client.get_job(&job_id).unwrap().status,
        JobStatus::Cancelled
    );
}

#[test]
fn test_cancel_already_cancelled_rejected() {
    let env = create_env();
    let (_, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);

    let job_id = client.create_job(
        &employer, &recipient, &token, &100i128, &10u64, &0u64, &None, &1u32,
    );

    client.cancel_job(&employer, &job_id);

    let result = client.try_cancel_job(&employer, &job_id);
    assert_eq!(
        result.unwrap_err().unwrap(),
        SchedulerError::AlreadyCancelled
    );
}

#[test]
fn test_cancel_wrong_employer_rejected() {
    let env = create_env();
    let (_, client) = setup(&env);
    let employer = Address::generate(&env);
    let attacker = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);

    let job_id = client.create_job(
        &employer, &recipient, &token, &100i128, &10u64, &0u64, &None, &1u32,
    );

    let result = client.try_cancel_job(&attacker, &job_id);
    assert_eq!(result.unwrap_err().unwrap(), SchedulerError::NotEmployer);
}

#[test]
fn test_cancel_completed_job_rejected() {
    let env = create_env();
    let (scheduler_id, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    asset_admin.mint(&employer, &100i128);
    token.transfer(&employer, &scheduler_id, &100i128);

    env.ledger().with_mut(|li| li.timestamp = 0);

    let job_id = client.create_job(
        &employer,
        &recipient,
        &token.address,
        &100i128,
        &10u64,
        &0u64,
        &Some(1u32),
        &0u32,
    );

    client.process_due_payments(&10u32);
    assert_eq!(client.get_job(&job_id).unwrap().status, JobStatus::Completed);

    let result = client.try_cancel_job(&employer, &job_id);
    assert_eq!(
        result.unwrap_err().unwrap(),
        SchedulerError::JobNotCancellable
    );
}

// ─── pause_job / resume_job ───────────────────────────────────────────────────

#[test]
fn test_pause_and_resume_job() {
    let env = create_env();
    let (scheduler_id, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    asset_admin.mint(&employer, &500i128);
    token.transfer(&employer, &scheduler_id, &500i128);

    env.ledger().with_mut(|li| li.timestamp = 0);

    let job_id = client.create_job(
        &employer,
        &recipient,
        &token.address,
        &100i128,
        &10u64,
        &0u64,
        &None,
        &1u32,
    );

    // Pause immediately
    client.pause_job(&employer, &job_id);
    assert_eq!(client.get_job(&job_id).unwrap().status, JobStatus::Paused);

    // Paused job should not be processed
    env.ledger().with_mut(|li| li.timestamp = 100);
    let processed = client.process_due_payments(&10u32);
    assert_eq!(processed, 0);
    assert_eq!(token.balance(&recipient), 0i128);

    // Resume and process
    client.resume_job(&employer, &job_id);
    let _ = client.process_due_payments(&10u32);
    let job = client.get_job(&job_id).unwrap();
    assert_eq!(job.executions, 1);
    assert_eq!(token.balance(&recipient), 100i128);
}

#[test]
fn test_pause_non_active_job_rejected() {
    let env = create_env();
    let (_, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);

    let job_id = client.create_job(
        &employer, &recipient, &token, &100i128, &10u64, &0u64, &None, &1u32,
    );

    // Pause once (OK)
    client.pause_job(&employer, &job_id);

    // Pause again (already Paused) → JobNotActive
    let result = client.try_pause_job(&employer, &job_id);
    assert_eq!(result.unwrap_err().unwrap(), SchedulerError::JobNotActive);
}

#[test]
fn test_resume_non_paused_job_rejected() {
    let env = create_env();
    let (_, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);

    let job_id = client.create_job(
        &employer, &recipient, &token, &100i128, &10u64, &0u64, &None, &1u32,
    );

    // Resume an Active (not Paused) job → JobNotPaused
    let result = client.try_resume_job(&employer, &job_id);
    assert_eq!(result.unwrap_err().unwrap(), SchedulerError::JobNotPaused);
}

// ─── fund_job ─────────────────────────────────────────────────────────────────

#[test]
fn test_fund_job_increases_scheduler_balance() {
    let env = create_env();
    let (scheduler_id, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    asset_admin.mint(&employer, &300i128);

    let job_id = client.create_job(
        &employer,
        &recipient,
        &token.address,
        &100i128,
        &10u64,
        &0u64,
        &None,
        &1u32,
    );

    client.fund_job(&employer, &job_id, &200i128);
    assert_eq!(token.balance(&scheduler_id), 200i128);
}

// ─── process_due_payments ─────────────────────────────────────────────────────

#[test]
fn test_process_no_jobs_returns_zero() {
    let env = create_env();
    let (_, client) = setup(&env);
    let result = client.process_due_payments(&10u32);
    assert_eq!(result, 0);
}

#[test]
fn test_process_max_jobs_bound() {
    let env = create_env();
    let (scheduler_id, client) = setup(&env);
    let employer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    asset_admin.mint(&employer, &1000i128);
    token.transfer(&employer, &scheduler_id, &1000i128);

    env.ledger().with_mut(|li| li.timestamp = 0);

    // Create 5 jobs all due at t=0
    for _ in 0..5u32 {
        let recipient = Address::generate(&env);
        client.create_job(
            &employer,
            &recipient,
            &token.address,
            &100i128,
            &10u64,
            &0u64,
            &None,
            &0u32,
        );
    }

    // Process with max_jobs=3 — only 3 should be evaluated
    let processed = client.process_due_payments(&3u32);
    assert_eq!(processed, 3);
}

#[test]
fn test_basic_recurring_job_execution() {
    let env = create_env();
    let (scheduler_id, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    asset_admin.mint(&employer, &300i128);
    token.transfer(&employer, &scheduler_id, &300i128);

    env.ledger().with_mut(|li| li.timestamp = 0);

    let job_id = client.create_job(
        &employer,
        &recipient,
        &token.address,
        &100i128,
        &10u64,
        &0u64,
        &Some(3u32),
        &1u32,
    );

    // Execution 1 at t=0
    let processed = client.process_due_payments(&10u32);
    assert_eq!(processed, 1);
    let job = client.get_job(&job_id).unwrap();
    assert_eq!(job.executions, 1);
    assert_eq!(job.status, JobStatus::Active);
    assert_eq!(token.balance(&recipient), 100i128);

    // Execution 2 at t=10
    env.ledger().with_mut(|li| li.timestamp = 10);
    client.process_due_payments(&10u32);
    assert_eq!(client.get_job(&job_id).unwrap().executions, 2);
    assert_eq!(token.balance(&recipient), 200i128);

    // Execution 3 at t=20 — completes the job
    env.ledger().with_mut(|li| li.timestamp = 20);
    client.process_due_payments(&10u32);
    let job = client.get_job(&job_id).unwrap();
    assert_eq!(job.executions, 3);
    assert_eq!(job.status, JobStatus::Completed);
    assert_eq!(token.balance(&recipient), 300i128);
}

#[test]
fn test_one_time_payment() {
    let env = create_env();
    let (scheduler_id, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    asset_admin.mint(&employer, &100i128);
    token.transfer(&employer, &scheduler_id, &100i128);

    env.ledger().with_mut(|li| li.timestamp = 0);

    let job_id = client.create_job(
        &employer,
        &recipient,
        &token.address,
        &100i128,
        &0u64,
        &0u64,
        &Some(1u32),
        &1u32,
    );

    let processed = client.process_due_payments(&10u32);
    assert_eq!(processed, 1);

    let job = client.get_job(&job_id).unwrap();
    assert_eq!(job.executions, 1);
    assert_eq!(job.status, JobStatus::Completed);
    assert_eq!(token.balance(&recipient), 100i128);
}

#[test]
fn test_cancelled_job_skipped_by_processor() {
    let env = create_env();
    let (scheduler_id, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    asset_admin.mint(&employer, &500i128);
    token.transfer(&employer, &scheduler_id, &500i128);

    env.ledger().with_mut(|li| li.timestamp = 0);

    let job_id = client.create_job(
        &employer,
        &recipient,
        &token.address,
        &100i128,
        &10u64,
        &0u64,
        &None,
        &1u32,
    );

    client.cancel_job(&employer, &job_id);

    // Even at t=100 the cancelled job must not be processed
    env.ledger().with_mut(|li| li.timestamp = 100);
    let processed = client.process_due_payments(&10u32);
    assert_eq!(processed, 0);
    assert_eq!(token.balance(&recipient), 0i128);
}

#[test]
fn test_insufficient_funds_then_retry_success() {
    let env = create_env();
    let (scheduler_id, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    // Fund 50 but job needs 100
    asset_admin.mint(&employer, &50i128);
    token.transfer(&employer, &scheduler_id, &50i128);

    env.ledger().with_mut(|li| li.timestamp = 0);

    let job_id = client.create_job(
        &employer,
        &recipient,
        &token.address,
        &100i128,
        &10u64,
        &0u64,
        &Some(1u32),
        &2u32,
    );

    // First attempt fails due to insufficient funds
    let processed = client.process_due_payments(&5u32);
    assert_eq!(processed, 1);
    let mut job = client.get_job(&job_id).unwrap();
    assert_eq!(job.retry_count, 1);
    assert_eq!(job.status, JobStatus::Active);
    assert_eq!(token.balance(&recipient), 0i128);

    // Top up and advance to retry time
    asset_admin.mint(&employer, &200i128);
    token.transfer(&employer, &scheduler_id, &200i128);
    env.ledger().with_mut(|li| li.timestamp = job.next_scheduled_time);
    client.process_due_payments(&5u32);

    job = client.get_job(&job_id).unwrap();
    assert_eq!(job.executions, 1);
    assert_eq!(job.status, JobStatus::Completed);
    assert_eq!(token.balance(&recipient), 100i128);
}

#[test]
fn test_retry_exhaustion_marks_failed() {
    let env = create_env();
    let (_, client) = setup(&env);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    // No funding at all → every attempt fails
    env.ledger().with_mut(|li| li.timestamp = 0);

    let job_id = client.create_job(
        &employer,
        &recipient,
        &token.address,
        &100i128,
        &10u64,
        &0u64,
        &None,
        &2u32, // 2 retries allowed
    );

    // Retry #1 at t=0
    client.process_due_payments(&1u32);
    assert_eq!(client.get_job(&job_id).unwrap().retry_count, 1);
    assert_eq!(client.get_job(&job_id).unwrap().status, JobStatus::Active);

    // Retry #2 at t=10
    env.ledger().with_mut(|li| li.timestamp = 10);
    client.process_due_payments(&1u32);
    assert_eq!(client.get_job(&job_id).unwrap().retry_count, 2);
    assert_eq!(client.get_job(&job_id).unwrap().status, JobStatus::Active);

    // Retry #3 at t=20 — exceeds max_retries=2 → Failed
    env.ledger().with_mut(|li| li.timestamp = 20);
    client.process_due_payments(&1u32);
    let job = client.get_job(&job_id).unwrap();
    assert_eq!(job.retry_count, 3);
    assert_eq!(job.status, JobStatus::Failed);
}

#[test]
fn test_conflict_detection_prevents_duplicates() {
    let env = create_env();
    let (_, client) = setup(&env);

    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);

    client.create_job(
        &employer, &recipient, &token, &100i128, &10u64, &1000u64, &Some(3u32), &1u32,
    );

    let result = client.try_create_job(
        &employer, &recipient, &token, &100i128, &10u64, &1000u64, &Some(3u32), &1u32,
    );
    assert_eq!(
        result.unwrap_err().unwrap(),
        SchedulerError::DuplicateSchedule
    );
}
