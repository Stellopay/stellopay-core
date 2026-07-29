//! Cross-contract integration tests: `compliance_checker`'s emergency pause
//! halting `payment_scheduler::process_due_payments` mid-batch (issue #1117).
//!
//! # What this covers
//!
//! `payment_scheduler` can optionally be wired to a `compliance_checker`
//! deployment via `set_compliance_checker`. When configured,
//! `process_due_payments` consults `compliance_checker::is_emergency_paused`
//! before evaluating any job in the call. If paused, the call is a no-op:
//! zero jobs are evaluated, no token transfer is attempted, and no job's
//! `next_scheduled_time` / `status` / `executions` / `retry_count` changes.
//!
//! A "batch" of due jobs is, in practice, drained by a keeper making a
//! sequence of `process_due_payments` calls (see the scheduler's module
//! docs: "an off-chain keeper ... invokes `process_due_payments`"). These
//! tests simulate an emergency pause occurring **mid-batch** by interleaving
//! `compliance_checker::set_emergency_pause` between two
//! `process_due_payments` calls that together would otherwise drain every
//! due job:
//!
//! 1. Process the first chunk of due jobs (they settle normally).
//! 2. Pause `compliance_checker`.
//! 3. Call `process_due_payments` again — verify it evaluates **zero**
//!    additional jobs and every remaining job is untouched (still `Active`,
//!    unpaid, `next_scheduled_time` unchanged).
//! 4. Unpause `compliance_checker`.
//! 5. Call `process_due_payments` again — verify it resumes and correctly
//!    settles exactly the remaining jobs.
//!
//! # Security notes
//!
//! - `compliance_checker::is_emergency_paused` is a permissionless, read-only
//!   view (no `require_auth`), so the scheduler can query it without needing
//!   any caller credentials of its own — this is intentional: the whole point
//!   is that *any* caller of `process_due_payments` (a permissionless
//!   entrypoint) observes the same halt behavior.
//! - Only the `compliance_checker` admin can flip the pause flag
//!   (`set_emergency_pause` requires `require_admin`), and only the
//!   `payment_scheduler` owner can point the scheduler at a given
//!   `compliance_checker` address (`set_compliance_checker` requires
//!   `require_auth` + an exact owner match) — verified below.
//! - The integration is opt-in and backward compatible: a scheduler that
//!   never calls `set_compliance_checker` behaves exactly as it did before
//!   this integration existed (also verified below).

#![cfg(test)]

use compliance_checker::{ComplianceCheckerContract, ComplianceCheckerContractClient};
use payment_scheduler::{
    JobStatus, PaymentJob, PaymentSchedulerContract, PaymentSchedulerContractClient, SchedulerError,
};
use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

fn env() -> Env {
    let e = Env::default();
    e.mock_all_auths();
    e
}

fn addr(env: &Env) -> Address {
    Address::generate(env)
}

fn token(env: &Env) -> Address {
    let admin = addr(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

fn mint(env: &Env, tok: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, tok).mint(to, &amount);
}

fn deploy_scheduler<'a>(
    env: &'a Env,
    owner: &Address,
    retry_addr: &Address,
) -> (Address, PaymentSchedulerContractClient<'a>) {
    let id = env.register_contract(None, PaymentSchedulerContract);
    let client = PaymentSchedulerContractClient::new(env, &id);
    client.initialize(owner, retry_addr);
    (id, client)
}

fn deploy_compliance_checker<'a>(
    env: &'a Env,
    admin: &Address,
) -> (Address, ComplianceCheckerContractClient<'a>) {
    let id = env.register_contract(None, ComplianceCheckerContract);
    let client = ComplianceCheckerContractClient::new(env, &id);
    client.initialize(admin);
    (id, client)
}

/// Deploys a scheduler + compliance checker, wires them together, and
/// creates `count` one-time, immediately-due jobs (distinct recipients so
/// each gets a distinct deterministic `schedule_id`), fully funded up front.
///
/// Returns `(scheduler_client, compliance_client, recipients, token, job_ids)`.
fn setup_scenario<'a>(
    env: &'a Env,
    owner: &Address,
    count: u32,
    amount: i128,
) -> (
    PaymentSchedulerContractClient<'a>,
    ComplianceCheckerContractClient<'a>,
    Vec<Address>,
    Address,
    Vec<u128>,
) {
    let employer = addr(env);
    let retry_addr = addr(env); // never invoked: scheduler is always fully funded here
    let tok_addr = token(env);

    let (sched_id, sched_client) = deploy_scheduler(env, owner, &retry_addr);
    let (compliance_id, compliance_client) = deploy_compliance_checker(env, owner);

    sched_client.set_compliance_checker(owner, &compliance_id);
    assert_eq!(sched_client.get_compliance_checker(), Some(compliance_id));

    let start_time = env.ledger().timestamp();
    let mut recipients = Vec::new();
    let mut job_ids = Vec::new();

    for _ in 0..count {
        let recipient = addr(env);
        let job_id = sched_client.create_job(
            &employer,
            &recipient,
            &tok_addr,
            &amount,
            &0, // interval_seconds: 0 is valid for one-time jobs
            &start_time,
            &Some(1), // one-time
            &3,
        );
        recipients.push(recipient);
        job_ids.push(job_id);
    }

    // Fund the scheduler's escrow up front for every job so the only reason
    // any job goes unpaid in these tests is the emergency pause gate, never
    // insufficient funds.
    mint(env, &tok_addr, &sched_id, amount * (count as i128));

    (
        sched_client,
        compliance_client,
        recipients,
        tok_addr,
        job_ids,
    )
}

#[test]
fn test_emergency_pause_halts_remaining_jobs_mid_batch() {
    let env = env();
    let owner = addr(&env);
    let amount = 1_000i128;
    let (sched_client, compliance_client, recipients, tok_addr, job_ids) =
        setup_scenario(&env, &owner, 5, amount);
    let token_client = TokenClient::new(&env, &tok_addr);

    // 1. Process the first chunk (jobs 0 and 1) — they settle normally.
    let evaluated = sched_client.process_due_payments(&2);
    assert_eq!(evaluated, 2, "first chunk should evaluate exactly 2 jobs");

    for i in 0..2usize {
        let job = sched_client.get_job(&job_ids[i]).unwrap();
        assert_eq!(
            job.status,
            JobStatus::Completed,
            "job {i} should be settled"
        );
        assert_eq!(
            token_client.balance(&recipients[i]),
            amount,
            "job {i} recipient should have been paid"
        );
    }

    // Snapshot the untouched jobs' state before pausing, to diff against
    // after the paused call below.
    let pre_pause_jobs: Vec<PaymentJob> = (2..5)
        .map(|i| sched_client.get_job(&job_ids[i]).unwrap())
        .collect();

    // 2. Pause compliance_checker mid-batch (after jobs 0-1, before 2-4).
    compliance_client.set_emergency_pause(&owner, &true);
    assert!(compliance_client.is_emergency_paused());

    // 3. Attempt to continue the batch — must evaluate zero jobs.
    let evaluated_while_paused = sched_client.process_due_payments(&3);
    assert_eq!(
        evaluated_while_paused, 0,
        "process_due_payments must evaluate no jobs while compliance_checker is paused"
    );

    // Jobs 2-4 must be byte-for-byte untouched: still Active, unpaid, and
    // with an unchanged next_scheduled_time / executions / retry_count.
    for i in 2..5usize {
        let job = sched_client.get_job(&job_ids[i]).unwrap();
        let pre = &pre_pause_jobs[i - 2];
        assert_eq!(
            job.status,
            JobStatus::Active,
            "job {i} must remain Active while paused"
        );
        assert_eq!(
            &job, pre,
            "job {i} record must be untouched by a paused call"
        );
        assert_eq!(
            token_client.balance(&recipients[i]),
            0,
            "job {i} recipient must not have been paid while paused"
        );
    }

    // 4. Lift the pause.
    compliance_client.set_emergency_pause(&owner, &false);
    assert!(!compliance_client.is_emergency_paused());

    // 5. Resume the batch — the remaining 3 jobs must now settle correctly.
    let evaluated_after_resume = sched_client.process_due_payments(&3);
    assert_eq!(
        evaluated_after_resume, 3,
        "process_due_payments must resume and evaluate exactly the remaining jobs"
    );
    for i in 2..5usize {
        let job = sched_client.get_job(&job_ids[i]).unwrap();
        assert_eq!(
            job.status,
            JobStatus::Completed,
            "job {i} should settle after resume"
        );
        assert_eq!(
            token_client.balance(&recipients[i]),
            amount,
            "job {i} recipient should have been paid after resume"
        );
    }

    // No job was ever double-paid across the pause/resume boundary.
    for i in 0..5usize {
        assert_eq!(
            token_client.balance(&recipients[i]),
            amount,
            "job {i} recipient balance must equal exactly one payment"
        );
    }
}

#[test]
fn test_pause_before_any_processing_evaluates_zero_jobs() {
    // Edge case: the pause is already active before the very first call —
    // no partial progress should be possible.
    let env = env();
    let owner = addr(&env);
    let amount = 500i128;
    let (sched_client, compliance_client, recipients, tok_addr, job_ids) =
        setup_scenario(&env, &owner, 3, amount);

    compliance_client.set_emergency_pause(&owner, &true);

    let evaluated = sched_client.process_due_payments(&10);
    assert_eq!(evaluated, 0);

    let token_client = TokenClient::new(&env, &tok_addr);
    for i in 0..3usize {
        let job = sched_client.get_job(&job_ids[i]).unwrap();
        assert_eq!(job.status, JobStatus::Active);
        assert_eq!(token_client.balance(&recipients[i]), 0);
    }
}

#[test]
fn test_repeated_pause_unpause_cycles_do_not_corrupt_state() {
    // Edge case: pausing and unpausing multiple times before any job is
    // fully drained must not lose progress or double-process a job.
    let env = env();
    let owner = addr(&env);
    let amount = 250i128;
    let (sched_client, compliance_client, recipients, tok_addr, job_ids) =
        setup_scenario(&env, &owner, 4, amount);
    let token_client = TokenClient::new(&env, &tok_addr);

    // Pause / unpause with no processing in between — must be a no-op on job state.
    compliance_client.set_emergency_pause(&owner, &true);
    compliance_client.set_emergency_pause(&owner, &false);
    compliance_client.set_emergency_pause(&owner, &true);
    assert_eq!(sched_client.process_due_payments(&10), 0);
    compliance_client.set_emergency_pause(&owner, &false);

    // Now fully unpaused: process one job, pause again, confirm halt, then
    // fully resume and drain the rest.
    assert_eq!(sched_client.process_due_payments(&1), 1);
    compliance_client.set_emergency_pause(&owner, &true);
    assert_eq!(sched_client.process_due_payments(&10), 0);
    compliance_client.set_emergency_pause(&owner, &false);
    assert_eq!(sched_client.process_due_payments(&10), 3);

    for i in 0..4usize {
        let job = sched_client.get_job(&job_ids[i]).unwrap();
        assert_eq!(job.status, JobStatus::Completed);
        assert_eq!(token_client.balance(&recipients[i]), amount);
    }
}

#[test]
fn test_scheduler_without_compliance_checker_configured_is_unaffected_by_pause() {
    // Backward compatibility: a scheduler that never calls
    // `set_compliance_checker` must process jobs normally even while an
    // unrelated compliance_checker deployment is paused.
    let env = env();
    let owner = addr(&env);
    let employer = addr(&env);
    let recipient = addr(&env);
    let retry_addr = addr(&env);
    let tok_addr = token(&env);

    let (sched_id, sched_client) = deploy_scheduler(&env, &owner, &retry_addr);
    let (_compliance_id, compliance_client) = deploy_compliance_checker(&env, &owner);
    // Note: sched_client.set_compliance_checker(...) is deliberately not called.
    assert_eq!(sched_client.get_compliance_checker(), None);

    compliance_client.set_emergency_pause(&owner, &true);

    let amount = 750i128;
    let start_time = env.ledger().timestamp();
    let job_id = sched_client.create_job(
        &employer,
        &recipient,
        &tok_addr,
        &amount,
        &0,
        &start_time,
        &Some(1),
        &3,
    );
    mint(&env, &tok_addr, &sched_id, amount);

    let evaluated = sched_client.process_due_payments(&1);
    assert_eq!(
        evaluated, 1,
        "an unconfigured scheduler must ignore an unrelated compliance_checker's pause state"
    );
    let job = sched_client.get_job(&job_id).unwrap();
    assert_eq!(job.status, JobStatus::Completed);
    assert_eq!(
        TokenClient::new(&env, &tok_addr).balance(&recipient),
        amount
    );
}

#[test]
fn test_set_compliance_checker_rejects_non_owner() {
    let env = env();
    let owner = addr(&env);
    let attacker = addr(&env);
    let retry_addr = addr(&env);
    let (_sched_id, sched_client) = deploy_scheduler(&env, &owner, &retry_addr);
    let (compliance_id, _compliance_client) = deploy_compliance_checker(&env, &owner);

    let result = sched_client.try_set_compliance_checker(&attacker, &compliance_id);
    assert_eq!(result.unwrap_err().unwrap(), SchedulerError::Unauthorized);
    assert_eq!(sched_client.get_compliance_checker(), None);
}
