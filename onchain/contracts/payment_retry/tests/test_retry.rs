//! Focused tests for the current PaymentRetry API.
//!
//! These tests cover `schedule_retry`, single-record `process_retry`, and the
//! keeper-facing `process_due_payments` batch entry point.

#![cfg(test)]

use payment_retry::{
    PaymentRetryContract, PaymentRetryContractClient, RetryConfig, RetryState,
};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, BytesN, Env, Vec,
};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

#[allow(deprecated)]
fn register_contract(env: &Env) -> (Address, PaymentRetryContractClient<'static>) {
    let id = env.register_contract(None, PaymentRetryContract);
    let client = PaymentRetryContractClient::new(env, &id);
    (id, client)
}

fn create_token_contract<'a>(env: &Env, admin: &Address) -> TokenClient<'a> {
    let token_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    TokenClient::new(env, &token_addr)
}

fn payment_id(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn retry_config(env: &Env, max_retries: u32, intervals: &[u64]) -> RetryConfig {
    let mut retry_intervals = Vec::new(env);
    for interval in intervals {
        retry_intervals.push_back(*interval);
    }

    RetryConfig {
        max_retries,
        retry_intervals,
    }
}

struct PaymentInput<'a> {
    id_seed: u8,
    payer: &'a Address,
    recipient: &'a Address,
    token: &'a Address,
    amount: i128,
    max_retries: u32,
    intervals: &'a [u64],
}

fn schedule_payment(
    env: &Env,
    client: &PaymentRetryContractClient<'static>,
    input: PaymentInput<'_>,
) -> BytesN<32> {
    let id = payment_id(env, input.id_seed);
    client.schedule_retry(
        &id,
        input.payer,
        input.recipient,
        input.token,
        &input.amount,
        &retry_config(env, input.max_retries, input.intervals),
    );
    id
}

#[test]
fn test_initialize_and_read_owner() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);

    client.initialize(&owner);
    assert_eq!(client.get_owner(), Some(owner));
}

#[test]
fn test_schedule_retry_stores_due_payment() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    client.initialize(&owner);
    env.ledger().with_mut(|li| li.timestamp = 100);

    let id = schedule_payment(
        &env,
        &client,
        PaymentInput {
            id_seed: 1,
            payer: &payer,
            recipient: &recipient,
            token: &token.address,
            amount: 250,
            max_retries: 2,
            intervals: &[30, 60],
        },
    );

    let payment = client.get_payment(&id).unwrap();
    assert_eq!(payment.id, id);
    assert_eq!(payment.payer, payer);
    assert_eq!(payment.recipient, recipient);
    assert_eq!(payment.amount, 250);
    assert_eq!(payment.retry_count, 0);
    assert_eq!(payment.max_retry_attempts, 2);
    assert_eq!(payment.next_retry_at, 100);
    assert_eq!(payment.state, RetryState::Scheduled);
}

#[test]
fn test_process_due_payments_succeeds_and_returns_count() {
    let env = create_env();
    let (contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    client.initialize(&owner);
    asset_admin.mint(&payer, &100);

    let id = schedule_payment(
        &env,
        &client,
        PaymentInput {
            id_seed: 2,
            payer: &payer,
            recipient: &recipient,
            token: &token.address,
            amount: 100,
            max_retries: 2,
            intervals: &[30],
        },
    );
    client.fund_payment(&payer, &id, &100);
    assert_eq!(token.balance(&contract_id), 100);

    let processed = client.process_due_payments(&10);
    assert_eq!(processed, 1);

    let payment = client.get_payment(&id).unwrap();
    assert_eq!(payment.state, RetryState::Success);
    assert_eq!(payment.retry_count, 0);
    assert_eq!(token.balance(&recipient), 100);
    assert_eq!(token.balance(&contract_id), 0);

    assert_eq!(client.process_due_payments(&10), 0);
    assert_eq!(token.balance(&recipient), 100);
}

#[test]
fn test_process_due_payments_respects_next_retry_at() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    client.initialize(&owner);
    env.ledger().with_mut(|li| li.timestamp = 0);

    let id = schedule_payment(
        &env,
        &client,
        PaymentInput {
            id_seed: 3,
            payer: &payer,
            recipient: &recipient,
            token: &token.address,
            amount: 100,
            max_retries: 2,
            intervals: &[30, 60],
        },
    );

    assert_eq!(client.process_due_payments(&10), 1);
    let payment = client.get_payment(&id).unwrap();
    assert_eq!(payment.state, RetryState::Retrying);
    assert_eq!(payment.retry_count, 1);
    assert_eq!(payment.next_retry_at, 30);

    asset_admin.mint(&payer, &100);
    client.fund_payment(&payer, &id, &100);

    env.ledger().with_mut(|li| li.timestamp = 29);
    assert_eq!(client.process_due_payments(&10), 0);
    assert_eq!(client.get_payment(&id).unwrap().state, RetryState::Retrying);

    env.ledger().with_mut(|li| li.timestamp = 30);
    assert_eq!(client.process_due_payments(&10), 1);
    assert_eq!(client.get_payment(&id).unwrap().state, RetryState::Success);
    assert_eq!(token.balance(&recipient), 100);
}

#[test]
fn test_process_due_payments_respects_max_payments() {
    let env = create_env();
    let (contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient_a = Address::generate(&env);
    let recipient_b = Address::generate(&env);
    let recipient_c = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    client.initialize(&owner);
    asset_admin.mint(&payer, &300);

    let id_a = schedule_payment(
        &env,
        &client,
        PaymentInput {
            id_seed: 4,
            payer: &payer,
            recipient: &recipient_a,
            token: &token.address,
            amount: 100,
            max_retries: 1,
            intervals: &[30],
        },
    );
    let id_b = schedule_payment(
        &env,
        &client,
        PaymentInput {
            id_seed: 5,
            payer: &payer,
            recipient: &recipient_b,
            token: &token.address,
            amount: 100,
            max_retries: 1,
            intervals: &[30],
        },
    );
    let id_c = schedule_payment(
        &env,
        &client,
        PaymentInput {
            id_seed: 6,
            payer: &payer,
            recipient: &recipient_c,
            token: &token.address,
            amount: 100,
            max_retries: 1,
            intervals: &[30],
        },
    );

    client.fund_payment(&payer, &id_a, &100);
    client.fund_payment(&payer, &id_b, &100);
    client.fund_payment(&payer, &id_c, &100);
    assert_eq!(token.balance(&contract_id), 300);

    assert_eq!(client.process_due_payments(&2), 2);
    assert_eq!(
        client.get_payment(&id_a).unwrap().state,
        RetryState::Success
    );
    assert_eq!(
        client.get_payment(&id_b).unwrap().state,
        RetryState::Success
    );
    assert_eq!(
        client.get_payment(&id_c).unwrap().state,
        RetryState::Scheduled
    );

    assert_eq!(client.process_due_payments(&10), 1);
    assert_eq!(
        client.get_payment(&id_c).unwrap().state,
        RetryState::Success
    );
}

#[test]
fn test_process_due_payments_returns_zero_for_zero_limit() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    client.initialize(&owner);
    let id = schedule_payment(
        &env,
        &client,
        PaymentInput {
            id_seed: 7,
            payer: &payer,
            recipient: &recipient,
            token: &token.address,
            amount: 100,
            max_retries: 1,
            intervals: &[30],
        },
    );

    assert_eq!(client.process_due_payments(&0), 0);
    assert_eq!(
        client.get_payment(&id).unwrap().state,
        RetryState::Scheduled
    );
}

#[test]
fn test_terminal_failure_is_not_reprocessed() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    client.initialize(&owner);
    let id = schedule_payment(
        &env,
        &client,
        PaymentInput {
            id_seed: 8,
            payer: &payer,
            recipient: &recipient,
            token: &token.address,
            amount: 100,
            max_retries: 0,
            intervals: &[],
        },
    );

    assert_eq!(client.process_due_payments(&10), 1);
    let failed = client.get_payment(&id).unwrap();
    assert_eq!(failed.state, RetryState::Failed);
    assert_eq!(failed.retry_count, 1);

    assert_eq!(client.process_due_payments(&10), 0);
}

#[test]
fn test_process_retry_removes_completed_record_from_batch_index() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    client.initialize(&owner);
    asset_admin.mint(&payer, &100);

    let id = schedule_payment(
        &env,
        &client,
        PaymentInput {
            id_seed: 9,
            payer: &payer,
            recipient: &recipient,
            token: &token.address,
            amount: 100,
            max_retries: 1,
            intervals: &[30],
        },
    );
    client.fund_payment(&payer, &id, &100);

    client.process_retry(&id);
    assert_eq!(client.get_payment(&id).unwrap().state, RetryState::Success);
    assert_eq!(client.process_due_payments(&10), 0);
}

#[test]
fn test_cancelled_record_is_skipped_by_batch_processing() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    client.initialize(&owner);
    let id = schedule_payment(
        &env,
        &client,
        PaymentInput {
            id_seed: 10,
            payer: &payer,
            recipient: &recipient,
            token: &token.address,
            amount: 100,
            max_retries: 1,
            intervals: &[30],
        },
    );

    client.cancel_payment(&payer, &id);
    assert_eq!(
        client.get_payment(&id).unwrap().state,
        RetryState::Cancelled
    );
    assert_eq!(client.process_due_payments(&10), 0);
}

#[test]
fn test_get_payment_retry_count_increments_per_attempt() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    client.initialize(&owner);
    env.ledger().with_mut(|li| li.timestamp = 0);

    let id = schedule_payment(
        &env,
        &client,
        PaymentInput {
            id_seed: 11,
            payer: &payer,
            recipient: &recipient,
            token: &token.address,
            amount: 100,
            max_retries: 3,
            intervals: &[30, 60, 120],
        },
    );
    assert_eq!(pay.next_retry_at, 40, "next_retry_at = 10 + 30");
    assert_eq!(pay.state, RetryState::Retrying);

    // --- Attempt 1 ---
    env.ledger().with_mut(|li| li.timestamp = 10);
    client.process_retry(&id);
    let pay = client.get_payment(&id).unwrap();
    assert_eq!(pay.retry_count, 1, "retry_count should be 1 after first failure");
    assert_eq!(pay.next_retry_at, 40, "next_retry_at = 10 + 30");
    assert_eq!(pay.state, RetryState::Retrying);

    // --- Attempt 2 ---
    env.ledger().with_mut(|li| li.timestamp = 40);
    client.process_retry(&id);
    let pay = client.get_payment(&id).unwrap();
    assert_eq!(pay.retry_count, 2, "retry_count should be 2 after second failure");
    assert_eq!(pay.next_retry_at, 100, "next_retry_at = 40 + 60");
    assert_eq!(pay.state, RetryState::Retrying);

    // --- Attempt 3 ---
    env.ledger().with_mut(|li| li.timestamp = 100);
    client.process_retry(&id);
    let pay = client.get_payment(&id).unwrap();
    assert_eq!(pay.retry_count, 3, "retry_count should be 3 after third failure");
    assert_eq!(pay.next_retry_at, 220, "next_retry_at = 100 + 120 (last interval reused)");
    assert_eq!(pay.state, RetryState::Retrying);

    // --- Attempt 4 (terminal) ---
    env.ledger().with_mut(|li| li.timestamp = 220);
    client.process_retry(&id);
    let pay = client.get_payment(&id).unwrap();
    assert_eq!(pay.retry_count, 4, "retry_count should be 4 after terminal failure");
    assert_eq!(pay.state, RetryState::Failed, "request should be Failed after max_retries exhausted");
}

#[test]
fn test_get_payment_successful_retry_leaves_count_unchanged() {
    let env = create_env();
    let (contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    client.initialize(&owner);
    env.ledger().with_mut(|li| li.timestamp = 0);

    let id = schedule_payment(
        &env,
        &client,
        PaymentInput {
            id_seed: 12,
            payer: &payer,
            recipient: &recipient,
            token: &token.address,
            amount: 100,
            max_retries: 2,
            intervals: &[30],
        },
    );

    // First attempt fails (no funds)
    client.process_retry(&id);
    let pay = client.get_payment(&id).unwrap();
    assert_eq!(pay.retry_count, 1, "retry_count should be 1 after first failure");
    assert_eq!(pay.state, RetryState::Retrying);

    // Fund escrow so the next attempt succeeds
    asset_admin.mint(&payer, &100);
    client.fund_payment(&payer, &id, &100);
    assert_eq!(token.balance(&contract_id), 100);

    // Second attempt succeeds — retry_count must NOT be incremented
    env.ledger().with_mut(|li| li.timestamp = 30);
    client.process_retry(&id);
    let pay = client.get_payment(&id).unwrap();
    assert_eq!(pay.state, RetryState::Success);
    assert_eq!(
        pay.retry_count, 1,
        "retry_count must remain 1 (not incremented on success)"
    );
    assert_eq!(token.balance(&recipient), 100);
    assert_eq!(token.balance(&contract_id), 0);
}

// ===========================================================================
// Cancellation permanence — "cancel beats an already-armed backoff"
// ===========================================================================
//
// These tests pin the security property that `cancel_payment` **permanently**
// stops a queued retry. The race being closed: a record fails once, the
// contract arms a backoff by writing `next_retry_at = now + interval`, the
// payer then cancels *before* that time, and a keeper's call finally lands
// after the ledger timestamp has crossed the stale `next_retry_at`. A naive
// implementation that gates only on the due timestamp would execute that
// retry and move funds. The contract must instead evaluate terminality first.

/// Full-fidelity harness for the mid-backoff cancellation race: schedules a
/// funded-capable request, burns one attempt to arm a real backoff-computed
/// `next_retry_at`, then hands control back to the test.
struct CancelRaceFixture<'a> {
    client: PaymentRetryContractClient<'static>,
    contract_id: Address,
    token: TokenClient<'a>,
    asset_admin: StellarAssetClient<'a>,
    payer: Address,
    recipient: Address,
    id: BytesN<32>,
}

fn setup_cancel_race(env: &Env, id_seed: u8, intervals: &[u64]) -> CancelRaceFixture<'static> {
    let (contract_id, client) = register_contract(env);
    let owner = Address::generate(env);
    let payer = Address::generate(env);
    let recipient = Address::generate(env);
    let token_admin = Address::generate(env);
    let token = create_token_contract(env, &token_admin);
    let asset_admin = StellarAssetClient::new(env, &token.address);

    client.initialize(&owner);
    env.ledger().with_mut(|li| li.timestamp = 1_000);

    let id = schedule_payment(
        env,
        &client,
        PaymentInput {
            id_seed,
            payer: &payer,
            recipient: &recipient,
            token: &token.address,
            amount: 500,
            max_retries: 5,
            intervals,
        },
    );

    CancelRaceFixture {
        client,
        contract_id,
        token,
        asset_admin,
        payer,
        recipient,
        id,
    }
}

/// Core regression test for the issue.
///
/// Timeline:
///   t=1000  schedule (next_retry_at = 1000)
///   t=1000  attempt #1 fails (unfunded) → retry_count=1, next_retry_at = 1000+300 = 1300
///   t=1100  payer cancels *inside* the backoff window (1100 < 1300)
///   t=1300  ledger reaches the exact armed retry time      → must NOT fire
///   t=5000  ledger far past the armed retry time           → must NOT fire
///
/// Asserted at every step: no transfer occurs, the record stays terminal, and
/// the batch processor reports zero work.
#[test]
fn test_cancel_mid_backoff_blocks_retry_past_scheduled_time() {
    let env = create_env();
    let f = setup_cancel_race(&env, 40, &[300, 600]);

    // --- Arm a real backoff by letting attempt #1 fail on an unfunded escrow.
    f.client.process_retry(&f.id);
    let armed = f.client.get_payment(&f.id).unwrap();
    assert_eq!(armed.state, RetryState::Retrying);
    assert_eq!(armed.retry_count, 1);
    assert_eq!(
        armed.next_retry_at, 1_300,
        "backoff must be computed from retry_intervals[0] (1000 + 300)"
    );

    // --- Fund escrow so that, if the cancelled retry *were* to fire, it would
    //     unambiguously succeed and transfer funds. This makes the test prove
    //     the state guard rather than an incidental lack of balance.
    f.asset_admin.mint(&f.payer, &500);
    f.client.fund_payment(&f.payer, &f.id, &500);
    assert_eq!(f.token.balance(&f.contract_id), 500);

    // --- Cancel strictly BEFORE the armed next_retry_at.
    env.ledger().with_mut(|li| li.timestamp = 1_100);
    f.client.cancel_payment(&f.payer, &f.id);

    let cancelled = f.client.get_payment(&f.id).unwrap();
    assert_eq!(
        cancelled.state,
        RetryState::Cancelled,
        "cancel_payment must record a terminal Cancelled state"
    );
    assert_eq!(
        cancelled.next_retry_at, 1_300,
        "the stale armed backoff timestamp is retained but must be inert"
    );
    // Escrow was refunded atomically on cancellation.
    assert_eq!(f.token.balance(&f.payer), 500);
    assert_eq!(f.token.balance(&f.contract_id), 0);
    assert_eq!(f.token.balance(&f.recipient), 0);

    // --- t == next_retry_at (boundary): the record is now "due" by timestamp.
    env.ledger().with_mut(|li| li.timestamp = 1_300);
    assert_eq!(
        f.client.process_due_payments(&10),
        0,
        "cancelled record must be skipped exactly at its armed next_retry_at"
    );
    f.client.process_retry(&f.id); // single-record path must also be a no-op
    let at_boundary = f.client.get_payment(&f.id).unwrap();
    assert_eq!(at_boundary.state, RetryState::Cancelled);
    assert_eq!(
        at_boundary.retry_count, 1,
        "no attempt may be consumed after cancellation"
    );
    assert_eq!(f.token.balance(&f.recipient), 0, "no funds may move");

    // --- t far beyond next_retry_at (late keeper): still inert.
    env.ledger().with_mut(|li| li.timestamp = 5_000);
    assert_eq!(f.client.process_due_payments(&10), 0);
    f.client.process_retry(&f.id);

    let after = f.client.get_payment(&f.id).unwrap();
    assert_eq!(
        after.state,
        RetryState::Cancelled,
        "state must remain terminally Cancelled forever"
    );
    assert_eq!(after.retry_count, 1);
    assert_eq!(after.next_retry_at, 1_300);
    assert_eq!(
        f.token.balance(&f.recipient),
        0,
        "a cancelled retry must never transfer funds"
    );
    assert_eq!(f.token.balance(&f.contract_id), 0);
    assert_eq!(f.token.balance(&f.payer), 500);
}

/// Cancelling from the initial `Scheduled` state (before any attempt has run)
/// must be equally permanent once the initial `next_retry_at` elapses.
#[test]
fn test_cancel_before_first_attempt_blocks_later_processing() {
    let env = create_env();
    let f = setup_cancel_race(&env, 41, &[300]);

    f.asset_admin.mint(&f.payer, &500);
    f.client.fund_payment(&f.payer, &f.id, &500);

    let before = f.client.get_payment(&f.id).unwrap();
    assert_eq!(before.state, RetryState::Scheduled);
    assert_eq!(before.retry_count, 0);

    f.client.cancel_payment(&f.payer, &f.id);
    assert_eq!(
        f.client.get_payment(&f.id).unwrap().state,
        RetryState::Cancelled
    );

    env.ledger().with_mut(|li| li.timestamp = 100_000);
    assert_eq!(f.client.process_due_payments(&50), 0);
    f.client.process_retry(&f.id);

    let after = f.client.get_payment(&f.id).unwrap();
    assert_eq!(after.state, RetryState::Cancelled);
    assert_eq!(after.retry_count, 0);
    assert_eq!(f.token.balance(&f.recipient), 0);
    assert_eq!(f.token.balance(&f.payer), 500, "escrow fully refunded");
}

/// Repeatedly hammering the keeper entry point across many ledger advances
/// (simulating a keeper that keeps waking up long after cancellation) must
/// never resurrect the record nor move funds.
#[test]
fn test_cancelled_payment_survives_repeated_late_keeper_calls() {
    let env = create_env();
    let f = setup_cancel_race(&env, 42, &[60]);

    f.client.process_retry(&f.id); // arm backoff: next_retry_at = 1060
    assert_eq!(f.client.get_payment(&f.id).unwrap().next_retry_at, 1_060);

    f.asset_admin.mint(&f.payer, &500);
    f.client.fund_payment(&f.payer, &f.id, &500);

    env.ledger().with_mut(|li| li.timestamp = 1_010);
    f.client.cancel_payment(&f.payer, &f.id);

    for step in 0..12u64 {
        env.ledger().with_mut(|li| li.timestamp = 1_060 + step * 60);
        assert_eq!(
            f.client.process_due_payments(&10),
            0,
            "keeper wake #{step} processed a cancelled record"
        );
        f.client.process_retry(&f.id);
        assert_eq!(
            f.client.get_payment(&f.id).unwrap().state,
            RetryState::Cancelled
        );
        assert_eq!(f.token.balance(&f.recipient), 0);
    }

    assert_eq!(f.token.balance(&f.contract_id), 0);
    assert_eq!(f.token.balance(&f.payer), 500);
}

/// A cancelled record must not starve or otherwise disturb a healthy sibling
/// record sharing the same batch index.
#[test]
fn test_cancelled_record_does_not_block_sibling_in_same_batch() {
    let env = create_env();
    let (contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient_a = Address::generate(&env);
    let recipient_b = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    client.initialize(&owner);
    env.ledger().with_mut(|li| li.timestamp = 1_000);

    let id_a = schedule_payment(
        &env,
        &client,
        PaymentInput {
            id_seed: 43,
            payer: &payer,
            recipient: &recipient_a,
            token: &token.address,
            amount: 100,
            max_retries: 3,
            intervals: &[300],
        },
    );
    let id_b = schedule_payment(
        &env,
        &client,
        PaymentInput {
            id_seed: 44,
            payer: &payer,
            recipient: &recipient_b,
            token: &token.address,
            amount: 100,
            max_retries: 3,
            intervals: &[300],
        },
    );

    // Arm identical backoffs on both, then fund both.
    assert_eq!(client.process_due_payments(&10), 2);
    assert_eq!(client.get_payment(&id_a).unwrap().next_retry_at, 1_300);
    assert_eq!(client.get_payment(&id_b).unwrap().next_retry_at, 1_300);

    asset_admin.mint(&payer, &200);
    client.fund_payment(&payer, &id_a, &100);
    client.fund_payment(&payer, &id_b, &100);

    // Cancel only A, mid-backoff.
    env.ledger().with_mut(|li| li.timestamp = 1_200);
    client.cancel_payment(&payer, &id_a);

    // Past the shared armed time: B settles, A stays inert.
    env.ledger().with_mut(|li| li.timestamp = 1_400);
    assert_eq!(
        client.process_due_payments(&10),
        1,
        "only the live sibling should be counted as processed"
    );

    assert_eq!(
        client.get_payment(&id_a).unwrap().state,
        RetryState::Cancelled
    );
    assert_eq!(
        client.get_payment(&id_b).unwrap().state,
        RetryState::Success
    );
    assert_eq!(token.balance(&recipient_a), 0, "cancelled leg paid out");
    assert_eq!(token.balance(&recipient_b), 100);
    assert_eq!(token.balance(&payer), 100, "A's escrow refunded on cancel");
    assert_eq!(token.balance(&contract_id), 0);
}

/// Cancelling twice must be rejected — the terminal state is not re-enterable.
#[test]
#[should_panic(expected = "Payment is already terminal")]
fn test_double_cancel_is_rejected() {
    let env = create_env();
    let f = setup_cancel_race(&env, 45, &[300]);

    f.client.cancel_payment(&f.payer, &f.id);
    env.ledger().with_mut(|li| li.timestamp = 9_999);
    f.client.cancel_payment(&f.payer, &f.id);
}

/// Re-funding a cancelled request must be rejected, so escrow cannot be used to
/// resurrect a terminal record into a payable one.
#[test]
#[should_panic(expected = "Payment is already terminal")]
fn test_funding_a_cancelled_payment_is_rejected() {
    let env = create_env();
    let f = setup_cancel_race(&env, 46, &[300]);

    f.asset_admin.mint(&f.payer, &500);
    f.client.cancel_payment(&f.payer, &f.id);

    env.ledger().with_mut(|li| li.timestamp = 9_999);
    f.client.fund_payment(&f.payer, &f.id, &500);
}

/// Only the owning payer may cancel — a third party cannot force-terminate a
/// live retry (griefing / denial-of-payment vector).
#[test]
#[should_panic(expected = "Only payer can cancel payment")]
fn test_non_payer_cannot_cancel_payment() {
    let env = create_env();
    let f = setup_cancel_race(&env, 47, &[300]);

    let attacker = Address::generate(&env);
    f.client.cancel_payment(&attacker, &f.id);
}

/// `get_payment` must expose the terminal `Cancelled` state (not `Failed`), so
/// indexers can distinguish payer opt-out from retry exhaustion, while both
/// remain equally unprocessable.
#[test]
fn test_get_payment_reports_terminal_cancelled_state() {
    let env = create_env();
    let f = setup_cancel_race(&env, 48, &[300]);

    f.client.process_retry(&f.id);
    assert_eq!(
        f.client.get_payment(&f.id).unwrap().state,
        RetryState::Retrying
    );

    f.client.cancel_payment(&f.payer, &f.id);

    let p = f.client.get_payment(&f.id).unwrap();
    assert_eq!(p.state, RetryState::Cancelled);
    assert_ne!(
        p.state,
        RetryState::Failed,
        "cancellation must be distinguishable from retry exhaustion"
    );
    assert_ne!(p.state, RetryState::Success);

    // And it is genuinely terminal for every processing entry point.
    env.ledger().with_mut(|li| li.timestamp = u32::MAX as u64);
    assert_eq!(f.client.process_due_payments(&100), 0);
    f.client.process_retry(&f.id);
    assert_eq!(
        f.client.get_payment(&f.id).unwrap().state,
        RetryState::Cancelled
    );
}

#[test]
fn test_simultaneously_due_payments_processed_in_stable_order() {
    let env = create_env();
    let (contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);

    client.initialize(&owner);
    env.ledger().with_mut(|li| li.timestamp = 100);

    // Queue 3 payments at the same time, but with different seeds so their IDs differ.
    // They will all be due immediately (next_retry_at = 100).
    // We intentionally create them in an order such that their IDs are not naturally sorted.
    let id_a = schedule_payment(
        &env,
        &client,
        PaymentInput {
            id_seed: 200,
            payer: &payer,
            recipient: &recipient,
            token: &token.address,
            amount: 100,
            max_retries: 2,
            intervals: &[30],
        },
    );
    let id_b = schedule_payment(
        &env,
        &client,
        PaymentInput {
            id_seed: 50,
            payer: &payer,
            recipient: &recipient,
            token: &token.address,
            amount: 100,
            max_retries: 2,
            intervals: &[30],
        },
    );
    let id_c = schedule_payment(
        &env,
        &client,
        PaymentInput {
            id_seed: 100,
            payer: &payer,
            recipient: &recipient,
            token: &token.address,
            amount: 100,
            max_retries: 2,
            intervals: &[30],
        },
    );

    // Ensure we have funds
    asset_admin.mint(&payer, &300);
    client.fund_payment(&payer, &id_a, &100);
    client.fund_payment(&payer, &id_b, &100);
    client.fund_payment(&payer, &id_c, &100);

    // Assert that their natural ID order is: id_b < id_c < id_a
    let mut ids = vec![id_a.clone(), id_b.clone(), id_c.clone()];
    ids.sort();

    // Process them with max_payments = 2
    let processed = client.process_due_payments(&2);
    assert_eq!(processed, 2, "Should process exactly 2 payments");

    // Because they were sorted by ID, the first two in ascending ID order should be processed (id_b and id_c)
    // The third one should still be pending.
    let pay_b = client.get_payment(&ids[0]).unwrap();
    let pay_c = client.get_payment(&ids[1]).unwrap();
    let pay_a = client.get_payment(&ids[2]).unwrap();

    assert_eq!(pay_b.state, RetryState::Success);
    assert_eq!(pay_c.state, RetryState::Success);
    assert_eq!(pay_a.state, RetryState::Scheduled);
}
