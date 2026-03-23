//! Comprehensive tests for the PaymentRetry contract.
//!
//! Coverage targets:
//! * Initialization (happy path, double-init guard)
//! * `create_payment_request` — happy path, zero amount, missing intervals,
//!   too many retries, oversized interval, alternate payout
//! * `fund_payment` — happy path, wrong payer, terminal state guard
//! * `process_due_payments` — immediate success, retry on insufficient balance,
//!   backoff timing, last-interval reuse, terminal failure, alternate payout
//!   routing, max_payments bound, zero max_retries terminal, idempotency
//! * `cancel_payment` — cancels pending, prevents processing, wrong payer
//! * View helpers (`get_payment`, `get_owner`)
//! * Edge cases: zero max_retries, multiple payments, process limit

#![cfg(test)]

use payment_retry::{PaymentRetryContract, PaymentRetryContractClient, PaymentStatus};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    vec, Address, Env,
};

// ─── Fixtures ────────────────────────────────────────────────────────────────

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
    let token_addr = env.register_stellar_asset_contract(admin.clone());
    TokenClient::new(env, &token_addr)
}

// ─── Initialization ──────────────────────────────────────────────────────────

#[test]
fn test_initialize_and_read_owner() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let owner = Address::generate(&env);

    client.initialize(&owner);
    assert_eq!(client.get_owner(), Some(owner.clone()));
}

#[test]
fn test_initialize_double_init_rejected() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let owner = Address::generate(&env);

    client.initialize(&owner);
    let second = client.try_initialize(&owner);
    assert!(second.is_err());
}

// ─── create_payment_request ──────────────────────────────────────────────────

#[test]
fn test_create_payment_request_happy_path() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    env.ledger().with_mut(|li| li.timestamp = 100);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &500i128,
        &3u32,
        &vec![&env, 30u64, 60u64],
        &notifier,
        &None,
    );

    let record = client.get_payment(&payment_id).unwrap();
    assert_eq!(record.id, payment_id);
    assert_eq!(record.payer, payer);
    assert_eq!(record.recipient, recipient);
    assert_eq!(record.amount, 500);
    assert_eq!(record.retry_count, 0);
    assert_eq!(record.max_retry_attempts, 3);
    assert_eq!(record.status, PaymentStatus::Pending);
    assert_eq!(record.next_retry_at, 100); // immediately eligible
    assert!(record.alternate_payout.is_none());
}

#[test]
fn test_create_payment_request_with_alternate_payout() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let alternate = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env, 5u64],
        &notifier,
        &Some(alternate.clone()),
    );

    let record = client.get_payment(&payment_id).unwrap();
    assert_eq!(record.alternate_payout, Some(alternate));
}

#[test]
fn test_create_payment_request_zero_amount_rejected() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let result = client.try_create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &0i128,
        &1u32,
        &vec![&env, 5u64],
        &notifier,
        &None,
    );
    assert!(result.is_err());
}

#[test]
fn test_create_payment_request_missing_intervals_rejected() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    // max_retry_attempts > 0 but empty intervals
    let result = client.try_create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env],
        &notifier,
        &None,
    );
    assert!(result.is_err());
}

#[test]
fn test_create_payment_request_zero_retries_no_intervals_allowed() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    // zero retries with empty intervals is valid
    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &0u32,
        &vec![&env],
        &notifier,
        &None,
    );
    assert!(client.get_payment(&payment_id).is_some());
}

#[test]
fn test_create_payment_request_interval_too_large_rejected() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    // 2 years in seconds exceeds MAX_SINGLE_RETRY_INTERVAL_SECONDS
    let result = client.try_create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env, 63_072_001u64],
        &notifier,
        &None,
    );
    assert!(result.is_err());
}

#[test]
fn test_create_payment_increments_id() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let id1 = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env, 5u64],
        &notifier,
        &None,
    );
    let id2 = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env, 5u64],
        &notifier,
        &None,
    );
    assert_eq!(id2, id1 + 1);
}

// ─── fund_payment ────────────────────────────────────────────────────────────

#[test]
fn test_fund_payment_happy_path() {
    let env = create_env();
    let (contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);
    asset_admin.mint(&payer, &300i128);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env, 5u64],
        &notifier,
        &None,
    );

    client.fund_payment(&payer, &payment_id, &100i128);
    assert_eq!(token.balance(&contract_id), 100i128);
}

#[test]
fn test_fund_payment_wrong_payer_rejected() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let attacker = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env, 5u64],
        &notifier,
        &None,
    );

    let result = client.try_fund_payment(&attacker, &payment_id, &100i128);
    assert!(result.is_err());
}

#[test]
fn test_fund_payment_after_cancel_rejected() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env, 5u64],
        &notifier,
        &None,
    );

    client.cancel_payment(&payer, &payment_id);

    let result = client.try_fund_payment(&payer, &payment_id, &100i128);
    assert!(result.is_err());
}

// ─── process_due_payments — success paths ────────────────────────────────────

#[test]
fn test_process_succeeds_immediately_when_funded() {
    let env = create_env();
    let (contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);
    asset_admin.mint(&payer, &200i128);

    env.ledger().with_mut(|li| li.timestamp = 10);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &2u32,
        &vec![&env, 5u64, 10u64],
        &notifier,
        &None,
    );

    client.fund_payment(&payer, &payment_id, &100i128);
    assert_eq!(token.balance(&contract_id), 100i128);

    let processed = client.process_due_payments(&10u32);
    assert_eq!(processed, 1);

    let record = client.get_payment(&payment_id).unwrap();
    assert_eq!(record.status, PaymentStatus::Completed);
    assert_eq!(record.retry_count, 0);
    assert_eq!(token.balance(&recipient), 100i128);
    assert_eq!(token.balance(&contract_id), 0i128);
}

#[test]
fn test_process_routes_to_alternate_payout() {
    let env = create_env();
    let (contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let alternate = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);
    asset_admin.mint(&payer, &200i128);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env, 5u64],
        &notifier,
        &Some(alternate.clone()),
    );

    client.fund_payment(&payer, &payment_id, &100i128);
    client.process_due_payments(&10u32);

    let record = client.get_payment(&payment_id).unwrap();
    assert_eq!(record.status, PaymentStatus::Completed);
    // Funds went to alternate, not recipient
    assert_eq!(token.balance(&alternate), 100i128);
    assert_eq!(token.balance(&recipient), 0i128);
    assert_eq!(token.balance(&contract_id), 0i128);
}

// ─── process_due_payments — retry and backoff ────────────────────────────────

#[test]
fn test_retries_then_succeeds_after_funding() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);
    asset_admin.mint(&payer, &500i128);

    env.ledger().with_mut(|li| li.timestamp = 0);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &3u32,
        &vec![&env, 10u64, 20u64],
        &notifier,
        &None,
    );

    // No funding yet — first attempt fails, schedules retry at t=10.
    let processed = client.process_due_payments(&1u32);
    assert_eq!(processed, 1);

    let record = client.get_payment(&payment_id).unwrap();
    assert_eq!(record.status, PaymentStatus::Pending);
    assert_eq!(record.retry_count, 1);
    assert_eq!(record.next_retry_at, 10);

    // Top up before retry window opens.
    client.fund_payment(&payer, &payment_id, &100i128);
    env.ledger().with_mut(|li| li.timestamp = 10);

    let processed = client.process_due_payments(&1u32);
    assert_eq!(processed, 1);

    let record = client.get_payment(&payment_id).unwrap();
    assert_eq!(record.status, PaymentStatus::Completed);
    assert_eq!(record.retry_count, 1);
    assert_eq!(token.balance(&recipient), 100i128);
}

#[test]
fn test_retry_not_due_before_next_retry_at() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    env.ledger().with_mut(|li| li.timestamp = 0);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &2u32,
        &vec![&env, 30u64],
        &notifier,
        &None,
    );

    // First attempt fails at t=0; next_retry_at = 30.
    client.process_due_payments(&1u32);
    let record = client.get_payment(&payment_id).unwrap();
    assert_eq!(record.next_retry_at, 30);

    // Advance to t=20 (still before next_retry_at=30).
    env.ledger().with_mut(|li| li.timestamp = 20);
    let processed = client.process_due_payments(&10u32);
    // Record is still Pending but not due yet — not counted as processed.
    assert_eq!(processed, 0);

    let record = client.get_payment(&payment_id).unwrap();
    assert_eq!(record.retry_count, 1); // unchanged
}

#[test]
fn test_retry_allowed_at_exact_next_retry_at() {
    let env = create_env();
    let (_, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);
    asset_admin.mint(&payer, &200i128);

    env.ledger().with_mut(|li| li.timestamp = 0);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &2u32,
        &vec![&env, 15u64],
        &notifier,
        &None,
    );

    // First attempt fails; next_retry_at = 15.
    client.process_due_payments(&1u32);

    // Fund and advance to exactly t=15.
    client.fund_payment(&payer, &payment_id, &100i128);
    env.ledger().with_mut(|li| li.timestamp = 15);

    let processed = client.process_due_payments(&1u32);
    assert_eq!(processed, 1);
    assert_eq!(
        client.get_payment(&payment_id).unwrap().status,
        PaymentStatus::Completed
    );
}

#[test]
fn test_backoff_uses_last_interval_when_retry_count_exceeds_list() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    env.ledger().with_mut(|li| li.timestamp = 0);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &4u32,
        &vec![&env, 7u64], // single interval; all retries reuse it
        &notifier,
        &None,
    );

    // Retry 1: next_retry_at = 0 + 7 = 7
    client.process_due_payments(&1u32);
    assert_eq!(
        client.get_payment(&payment_id).unwrap().next_retry_at,
        7
    );

    // Retry 2: next_retry_at = 7 + 7 = 14
    env.ledger().with_mut(|li| li.timestamp = 7);
    client.process_due_payments(&1u32);
    assert_eq!(
        client.get_payment(&payment_id).unwrap().next_retry_at,
        14
    );

    // Retry 3: next_retry_at = 14 + 7 = 21
    env.ledger().with_mut(|li| li.timestamp = 14);
    client.process_due_payments(&1u32);
    assert_eq!(
        client.get_payment(&payment_id).unwrap().next_retry_at,
        21
    );
}

#[test]
fn test_backoff_uses_stepped_intervals() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    env.ledger().with_mut(|li| li.timestamp = 0);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &3u32,
        &vec![&env, 10u64, 30u64, 60u64],
        &notifier,
        &None,
    );

    // Retry 1 → interval[0] = 10
    client.process_due_payments(&1u32);
    assert_eq!(
        client.get_payment(&payment_id).unwrap().next_retry_at,
        10
    );

    // Retry 2 → interval[1] = 30
    env.ledger().with_mut(|li| li.timestamp = 10);
    client.process_due_payments(&1u32);
    assert_eq!(
        client.get_payment(&payment_id).unwrap().next_retry_at,
        40 // 10 + 30
    );

    // Retry 3 → interval[2] = 60
    env.ledger().with_mut(|li| li.timestamp = 40);
    client.process_due_payments(&1u32);
    assert_eq!(
        client.get_payment(&payment_id).unwrap().next_retry_at,
        100 // 40 + 60
    );
}

// ─── process_due_payments — terminal failure ─────────────────────────────────

#[test]
fn test_fails_after_max_retries_exceeded() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    env.ledger().with_mut(|li| li.timestamp = 1);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &2u32, // 2 retries allowed
        &vec![&env, 5u64],
        &notifier,
        &None,
    );

    // Retry #1 at t=1 (immediately eligible)
    client.process_due_payments(&10u32);
    let record = client.get_payment(&payment_id).unwrap();
    assert_eq!(record.status, PaymentStatus::Pending);
    assert_eq!(record.retry_count, 1);
    assert_eq!(record.next_retry_at, 6);

    // Retry #2 at t=6
    env.ledger().with_mut(|li| li.timestamp = 6);
    client.process_due_payments(&10u32);
    let record = client.get_payment(&payment_id).unwrap();
    assert_eq!(record.status, PaymentStatus::Pending);
    assert_eq!(record.retry_count, 2);
    assert_eq!(record.next_retry_at, 11);

    // Retry #3 at t=11 → exceeds max_retry_attempts=2 → terminal Failed
    env.ledger().with_mut(|li| li.timestamp = 11);
    client.process_due_payments(&10u32);
    let record = client.get_payment(&payment_id).unwrap();
    assert_eq!(record.status, PaymentStatus::Failed);
    assert_eq!(record.retry_count, 3);
    assert_eq!(record.failure_notifier, notifier);
}

#[test]
fn test_zero_max_retries_fails_on_first_missed_attempt() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    // No funding — first attempt will fail, retry_count=1 > max_retry_attempts=0 → Failed.
    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &0u32,
        &vec![&env],
        &notifier,
        &None,
    );

    client.process_due_payments(&10u32);

    let record = client.get_payment(&payment_id).unwrap();
    assert_eq!(record.status, PaymentStatus::Failed);
    assert_eq!(record.retry_count, 1);
}

#[test]
fn test_failed_record_not_reprocessed() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &0u32,
        &vec![&env],
        &notifier,
        &None,
    );

    // Drive to Failed.
    client.process_due_payments(&10u32);
    assert_eq!(
        client.get_payment(&payment_id).unwrap().status,
        PaymentStatus::Failed
    );

    // Subsequent call should not process the failed record.
    let processed = client.process_due_payments(&10u32);
    assert_eq!(processed, 0);
}

// ─── cancel_payment ──────────────────────────────────────────────────────────

#[test]
fn test_cancel_prevents_future_processing() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &50i128,
        &1u32,
        &vec![&env, 4u64],
        &notifier,
        &None,
    );

    client.cancel_payment(&payer, &payment_id);
    assert_eq!(
        client.get_payment(&payment_id).unwrap().status,
        PaymentStatus::Cancelled
    );

    let processed = client.process_due_payments(&10u32);
    assert_eq!(processed, 0);
}

#[test]
fn test_cancel_wrong_payer_rejected() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let attacker = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env, 5u64],
        &notifier,
        &None,
    );

    let result = client.try_cancel_payment(&attacker, &payment_id);
    assert!(result.is_err());
}

#[test]
fn test_cancel_already_completed_rejected() {
    let env = create_env();
    let (_, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);
    asset_admin.mint(&payer, &200i128);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env, 5u64],
        &notifier,
        &None,
    );
    client.fund_payment(&payer, &payment_id, &100i128);
    client.process_due_payments(&1u32);

    let result = client.try_cancel_payment(&payer, &payment_id);
    assert!(result.is_err());
}

// ─── process limit ────────────────────────────────────────────────────────────

#[test]
fn test_max_payments_bound_is_enforced() {
    let env = create_env();
    let (contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient_a = Address::generate(&env);
    let recipient_b = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);
    asset_admin.mint(&payer, &500i128);

    let id_a = client.create_payment_request(
        &payer,
        &recipient_a,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env, 5u64],
        &notifier,
        &None,
    );
    let id_b = client.create_payment_request(
        &payer,
        &recipient_b,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env, 5u64],
        &notifier,
        &None,
    );

    client.fund_payment(&payer, &id_a, &100i128);
    client.fund_payment(&payer, &id_b, &100i128);
    assert_eq!(token.balance(&contract_id), 200i128);

    // Only process one at a time.
    let processed = client.process_due_payments(&1u32);
    assert_eq!(processed, 1);

    let p_a = client.get_payment(&id_a).unwrap();
    let p_b = client.get_payment(&id_b).unwrap();
    assert_eq!(p_a.status, PaymentStatus::Completed);
    assert_eq!(p_b.status, PaymentStatus::Pending);

    let processed = client.process_due_payments(&10u32);
    assert_eq!(processed, 1);
    assert_eq!(
        client.get_payment(&id_b).unwrap().status,
        PaymentStatus::Completed
    );
}

#[test]
fn test_process_zero_max_payments_returns_zero() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let processed = client.process_due_payments(&0u32);
    assert_eq!(processed, 0);
}

// ─── Idempotency ─────────────────────────────────────────────────────────────

#[test]
fn test_completed_record_not_reprocessed() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let asset_admin = StellarAssetClient::new(&env, &token.address);
    asset_admin.mint(&payer, &200i128);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env, 5u64],
        &notifier,
        &None,
    );
    client.fund_payment(&payer, &payment_id, &100i128);
    client.process_due_payments(&1u32);

    assert_eq!(
        client.get_payment(&payment_id).unwrap().status,
        PaymentStatus::Completed
    );

    // Second call should not re-process the completed record.
    let processed = client.process_due_payments(&10u32);
    assert_eq!(processed, 0);
    assert_eq!(token.balance(&recipient), 100i128);
}

#[test]
fn test_cancelled_record_not_reprocessed() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &2u32,
        &vec![&env, 5u64],
        &notifier,
        &None,
    );
    client.cancel_payment(&payer, &payment_id);

    let processed = client.process_due_payments(&10u32);
    assert_eq!(processed, 0);
}

// ─── Security: cannot drain via infinite retries ─────────────────────────────

#[test]
fn test_cannot_exceed_max_retry_attempts_cap() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    // Exceeds MAX_RETRY_ATTEMPTS (100)
    let result = client.try_create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &101u32,
        &vec![&env, 1u64],
        &notifier,
        &None,
    );
    assert!(result.is_err());
}

#[test]
fn test_payment_permanently_blocked_after_exhausting_retries() {
    // Verify that once a record is Failed it cannot be re-activated or
    // drained via further process_due_payments calls.
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env, 5u64],
        &notifier,
        &None,
    );

    // Retry 1 — insufficient escrow.
    client.process_due_payments(&10u32);
    // Retry 2 — exceeds max, becomes Failed.
    env.ledger().with_mut(|li| li.timestamp = 5);
    client.process_due_payments(&10u32);

    assert_eq!(
        client.get_payment(&payment_id).unwrap().status,
        PaymentStatus::Failed
    );

    // Even after funding is attempted, the guard rejects non-Pending records.
    let asset_admin = StellarAssetClient::new(&env, &token.address);
    asset_admin.mint(&payer, &200i128);
    let result = client.try_fund_payment(&payer, &payment_id, &100i128);
    assert!(result.is_err());

    // And process_due_payments cannot revive it either.
    let processed = client.process_due_payments(&10u32);
    assert_eq!(processed, 0);
}

// ─── View functions ───────────────────────────────────────────────────────────

#[test]
fn test_get_payment_returns_none_for_missing_id() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    assert!(client.get_payment(&9999u128).is_none());
}

#[test]
fn test_get_owner_before_init_returns_none() {
    let env = create_env();
    let (_id, client) = register_contract(&env);

    assert!(client.get_owner().is_none());
}
