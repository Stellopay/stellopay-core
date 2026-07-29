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
    assert_eq!(client.get_payment(&id).unwrap().state, RetryState::Failed);
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

// ═══════════════════════════════════════════════════════════════════════════════
// Maximum-attempt ceiling tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_max_attempt_ceiling_via_process_retry() {
    // Forces repeated failures past the maximum attempt count using
    // `process_retry` and asserts the ceiling is enforced:
    //   - The ceiling stops retries after `max_retry_attempts` is exceeded.
    //   - Subsequent calls to `process_retry` are no-ops.
    //   - The final state is `Failed` (distinguishable from `Retrying`/`Success`).
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
            id_seed: 13,
            payer: &payer,
            recipient: &recipient,
            token: &token.address,
            amount: 100,
            max_retries: 2, // ceiling → fail after 3rd failure
            intervals: &[30],
        },
    );

    // --- Attempt 1 (retry_count: 0 → 1) ---
    env.ledger().with_mut(|li| li.timestamp = 10);
    client.process_retry(&id);
    // Capture events BEFORE any other contract call (which would clear them).
    let events_1 = env.events().all().len();
    let pay = client.get_payment(&id).unwrap();
    assert_eq!(pay.retry_count, 1, "retry_count should be 1");
    assert_eq!(pay.state, RetryState::Retrying, "should still be Retrying");
    assert!(events_1 > 0, "events should be emitted on retry");

    // --- Attempt 2 (retry_count: 1 → 2) ---
    env.ledger().with_mut(|li| li.timestamp = 40);
    client.process_retry(&id);
    let pay = client.get_payment(&id).unwrap();
    assert_eq!(pay.retry_count, 2, "retry_count should be 2");
    assert_eq!(pay.state, RetryState::Retrying, "should still be Retrying");

    // --- Attempt 3 (retry_count: 2 → 3 => exhausted) ---
    env.ledger().with_mut(|li| li.timestamp = 70);
    client.process_retry(&id);
    let pay = client.get_payment(&id).unwrap();
    assert_eq!(pay.retry_count, 3, "retry_count should be 3");
    assert_eq!(pay.state, RetryState::Failed, "must be Failed after exhausting retries");

    // --- Attempt 4 — ceiling already enforced; must be no-op ---
    env.ledger().with_mut(|li| li.timestamp = 100);
    client.process_retry(&id);
    let events_4 = env.events().all().len();
    let pay = client.get_payment(&id).unwrap();
    assert_eq!(pay.state, RetryState::Failed, "state must remain Failed");
    assert_eq!(pay.retry_count, 3, "retry_count must not change");
    assert_eq!(events_4, 0, "no events on no-op retry after ceiling");
}

#[test]
fn test_max_attempt_ceiling_via_process_due_payments() {
    // Forces repeated failures using `process_due_payments` (the batch entry
    // point) and asserts the ceiling is honoured identically to the single-record
    // path.
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
            id_seed: 14,
            payer: &payer,
            recipient: &recipient,
            token: &token.address,
            amount: 100,
            max_retries: 1, // ceiling → fail after 2nd failure
            intervals: &[30],
        },
    );

    // --- Attempt 1 (retry_count: 0 → 1) ---
    env.ledger().with_mut(|li| li.timestamp = 10);
    client.process_due_payments(&10);
    let events_1 = env.events().all().len();
    let pay = client.get_payment(&id).unwrap();
    assert_eq!(pay.retry_count, 1, "retry_count should be 1");
    assert_eq!(pay.state, RetryState::Retrying, "should still be Retrying");
    assert!(events_1 > 0, "events emitted on retry");

    // --- Attempt 2 (retry_count: 1 → 2 => exhausted) ---
    env.ledger().with_mut(|li| li.timestamp = 40);
    client.process_due_payments(&10);
    let events_2 = env.events().all().len();
    let pay = client.get_payment(&id).unwrap();
    assert_eq!(pay.retry_count, 2, "retry_count should be 2");
    assert_eq!(pay.state, RetryState::Failed, "must be Failed after exhausting retries");
    assert!(events_2 > 0, "events emitted on terminal failure");

    // --- process_due_payments again — must be no-op ---
    env.ledger().with_mut(|li| li.timestamp = 70);
    let processed = client.process_due_payments(&10);
    let events_3 = env.events().all().len();
    assert_eq!(processed, 0, "process_due_payments must return 0 for exhausted payment");
    let pay = client.get_payment(&id).unwrap();
    assert_eq!(pay.state, RetryState::Failed, "state must remain Failed");
    assert_eq!(pay.retry_count, 2, "retry_count must not change");
    assert_eq!(events_3, 0, "no events on no-op after ceiling");
}

#[test]
fn test_max_attempt_ceiling_state_is_distinguishable() {
    // Asserts that after exhausting retries the payment's state (`Failed`) is
    // distinguishable from every other lifecycle state. This ensures off-chain
    // indexers and payroll-completion logic can reliably detect a permanently-
    // failed payment.
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
            id_seed: 15,
            payer: &payer,
            recipient: &recipient,
            token: &token.address,
            amount: 100,
            max_retries: 2,
            intervals: &[30],
        },
    );

    // Exhaust retries
    env.ledger().with_mut(|li| li.timestamp = 10);
    client.process_retry(&id);
    assert_eq!(
        client.get_payment(&id).unwrap().state,
        RetryState::Retrying,
        "after first failure → Retrying"
    );
    env.ledger().with_mut(|li| li.timestamp = 40);
    client.process_retry(&id);
    assert_eq!(
        client.get_payment(&id).unwrap().state,
        RetryState::Retrying,
        "after second failure → Retrying"
    );
    env.ledger().with_mut(|li| li.timestamp = 70);
    client.process_retry(&id);

    let final_state = client.get_payment(&id).unwrap().state;

    // Failed must not equal any other variant
    assert_eq!(final_state, RetryState::Failed, "final state must be Failed");
    assert_ne!(final_state, RetryState::Pending, "Failed ≠ Pending");
    assert_ne!(final_state, RetryState::Scheduled, "Failed ≠ Scheduled");
    assert_ne!(final_state, RetryState::Retrying, "Failed ≠ Retrying");
    assert_ne!(final_state, RetryState::Success, "Failed ≠ Success");
}

#[test]
fn test_max_attempt_ceiling_zero_max_retries_is_terminal_first_failure() {
    // With `max_retries = 0`, the very first failure should transition directly
    // to `Failed` without any retry scheduling.
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    client.initialize(&owner);
    env.ledger().with_mut(|li| li.timestamp = 0);

    // `max_retries: 0` with non-empty intervals is valid per the validation
    // logic (the intervals are simply unused).
    let id = schedule_payment(
        &env,
        &client,
        PaymentInput {
            id_seed: 16,
            payer: &payer,
            recipient: &recipient,
            token: &token.address,
            amount: 100,
            max_retries: 0,
            intervals: &[1],
        },
    );

    client.process_retry(&id);
    let events_1 = env.events().all().len();
    let pay = client.get_payment(&id).unwrap();
    assert_eq!(pay.state, RetryState::Failed, "with max_retries=0, first failure → Failed");
    assert_eq!(pay.retry_count, 1, "retry_count should be 1 after first failure");
    assert!(events_1 > 0, "events emitted on terminal failure");

    // Subsequent calls are no-ops
    env.ledger().with_mut(|li| li.timestamp = 10);
    assert_eq!(client.process_due_payments(&10), 0);
    let events_2 = env.events().all().len();
    let pay = client.get_payment(&id).unwrap();
    assert_eq!(pay.state, RetryState::Failed);
    assert_eq!(events_2, 0, "no events on no-op after ceiling");
}

#[test]
fn test_max_attempt_ceiling_does_not_affect_success_path() {
    // Ensure that the ceiling logic does not interfere with a payment that
    // eventually succeeds before exhausting retries.
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
            id_seed: 17,
            payer: &payer,
            recipient: &recipient,
            token: &token.address,
            amount: 100,
            max_retries: 5,
            intervals: &[30],
        },
    );

    // First two attempts fail
    env.ledger().with_mut(|li| li.timestamp = 10);
    client.process_retry(&id);
    env.ledger().with_mut(|li| li.timestamp = 40);
    client.process_retry(&id);
    assert_eq!(client.get_payment(&id).unwrap().retry_count, 2);
    assert_eq!(client.get_payment(&id).unwrap().state, RetryState::Retrying);

    // Now fund and succeed on the third attempt
    asset_admin.mint(&payer, &100);
    client.fund_payment(&payer, &id, &100);
    env.ledger().with_mut(|li| li.timestamp = 70);
    client.process_retry(&id);

    let pay = client.get_payment(&id).unwrap();
    assert_eq!(pay.state, RetryState::Success, "should succeed despite 2 prior failures");
    assert_eq!(pay.retry_count, 2, "retry_count must not be incremented on success");
    assert_eq!(token.balance(&recipient), 100, "recipient should receive funds");
    assert_eq!(token.balance(&contract_id), 0, "escrow should be debited");
}
