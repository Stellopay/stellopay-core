#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

use payment_scheduler::{
    JobStatus, PaymentJob, PaymentSchedulerContract, PaymentSchedulerContractClient,
};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn register_contract(env: &Env) -> (Address, PaymentSchedulerContractClient<'static>) {
    #[allow(deprecated)]
    let id = env.register_contract(None, PaymentSchedulerContract);
    let client = PaymentSchedulerContractClient::new(env, &id);
    (id, client)
}

fn create_token_contract<'a>(env: &Env, admin: &Address) -> TokenClient<'a> {
    let token_addr = env.register_stellar_asset_contract(admin.clone());
    TokenClient::new(env, &token_addr)
}

#[test]
fn initialize_and_owner() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let owner = Address::generate(&env);

    client.initialize(&owner);
    assert_eq!(client.get_owner(), Some(owner.clone()));

    let res = client.try_initialize(&owner);
    assert!(res.is_err());
}

#[test]
fn basic_recurring_job_execution() {
    let env = create_env();
    let (scheduler_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    // employer pre-funds scheduler with enough balance for 3 executions
    asset_admin.mint(&employer, &300i128);
    token.transfer(&employer, &scheduler_id, &300i128);

    env.ledger().with_mut(|li| {
        li.timestamp = 0;
    });

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

    // First execution at t=0
    let processed = client.process_due_payments(&10u32);
    assert_eq!(processed, 1);

    let job = client.get_job(&job_id).unwrap();
    assert_eq!(job.executions, 1);
    assert_eq!(job.status, JobStatus::Active);
    assert_eq!(token.balance(&recipient), 100i128);

    // Second execution at t=10
    env.ledger().with_mut(|li| {
        li.timestamp = 10;
    });
    let _ = client.process_due_payments(&10u32);
    let job = client.get_job(&job_id).unwrap();
    assert_eq!(job.executions, 2);
    assert_eq!(token.balance(&recipient), 200i128);

    // Third execution at t=20 completes the job
    env.ledger().with_mut(|li| {
        li.timestamp = 20;
    });
    let _ = client.process_due_payments(&10u32);
    let job = client.get_job(&job_id).unwrap();
    assert_eq!(job.executions, 3);
    assert_eq!(job.status, JobStatus::Completed);
    assert_eq!(token.balance(&recipient), 300i128);
}

#[test]
fn insufficient_funds_and_retries() {
    let env = create_env();
    let (scheduler_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    // Only fund 50 but job requires 100
    asset_admin.mint(&employer, &50i128);
    token.transfer(&employer, &scheduler_id, &50i128);

    env.ledger().with_mut(|li| {
        li.timestamp = 0;
    });

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

    // First attempt fails due to insufficient funds, schedules retry
    let processed = client.process_due_payments(&5u32);
    assert_eq!(processed, 1);
    let mut job = client.get_job(&job_id).unwrap();
    assert_eq!(job.retry_count, 1);
    assert_eq!(job.status, JobStatus::Active);
    assert_eq!(token.balance(&recipient), 0i128);

    // Top up funds before retry
    asset_admin.mint(&employer, &200i128);
    token.transfer(&employer, &scheduler_id, &200i128);

    // Move time forward to next scheduled time and process again
    env.ledger().with_mut(|li| {
        li.timestamp = job.next_scheduled_time;
    });
    let _ = client.process_due_payments(&5u32);

    job = client.get_job(&job_id).unwrap();
    assert_eq!(job.executions, 1);
    assert_eq!(job.status, JobStatus::Completed);
    assert_eq!(token.balance(&recipient), 100i128);
}

#[test]
fn pause_and_resume_job() {
    let env = create_env();
    let (scheduler_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    asset_admin.mint(&employer, &500i128);
    token.transfer(&employer, &scheduler_id, &500i128);

    env.ledger().with_mut(|li| {
        li.timestamp = 0;
    });

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
    let job = client.get_job(&job_id).unwrap();
    assert_eq!(job.status, JobStatus::Paused);

    // Even if time passes, paused job is not processed
    env.ledger().with_mut(|li| {
        li.timestamp = 100;
    });
    let processed = client.process_due_payments(&10u32);
    assert_eq!(processed, 0);
    assert_eq!(token.balance(&recipient), 0i128);

    // Resume and process
    client.resume_job(&employer, &job_id);
    let _ = client.process_due_payments(&10u32);
    let job = client.get_job(&job_id).unwrap();
    assert_eq!(job.executions, 1);
    assert_eq!(job.status, JobStatus::Active);
    assert_eq!(token.balance(&recipient), 100i128);
}

#[test]
fn test_one_time_payment() {
    let env = create_env();
    let (scheduler_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    asset_admin.mint(&employer, &100i128);
    token.transfer(&employer, &scheduler_id, &100i128);

    env.ledger().with_mut(|li| li.timestamp = 0);

    // Create a strict ONE-TIME job (interval = 0, max_executions = Some(1))
    let job_id = client.create_job(
        &employer,
        &recipient,
        &token.address,
        &100i128,
        &0u64, // 0 interval allowed for one-time
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

    // Same timestamp second processing must not execute again.
    let second = client.process_due_payments(&10u32);
    assert_eq!(second, 0);
    assert_eq!(token.balance(&recipient), 100i128);
}

#[test]
#[should_panic(expected = "Conflict: identical job scheduled for this time")]
fn test_conflict_detection_prevents_duplicates() {
    let env = create_env();
    let (_, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);

    // Create first job
    client.create_job(
        &employer,
        &recipient,
        &token,
        &100i128,
        &10u64,
        &1000u64, // Start time
        &Some(3u32),
        &1u32,
    );

    // Attempting to create the exact same job at the same time should panic
    client.create_job(
        &employer,
        &recipient,
        &token,
        &100i128,
        &10u64,
        &1000u64, // Exact same start time triggers conflict
        &Some(3u32),
        &1u32,
    );
}
