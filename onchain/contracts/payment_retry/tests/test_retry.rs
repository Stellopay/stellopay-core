#![cfg(test)]

use payment_retry::{PaymentRetryContract, PaymentRetryContractClient, PaymentStatus};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    vec, Address, Env,
};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn register_contract(env: &Env) -> (Address, PaymentRetryContractClient<'static>) {
    #[allow(deprecated)]
    let id = env.register_contract(None, PaymentRetryContract);
    let client = PaymentRetryContractClient::new(env, &id);
    (id, client)
}

fn create_token_contract<'a>(env: &Env, admin: &Address) -> TokenClient<'a> {
    let token_addr = env.register_stellar_asset_contract(admin.clone());
    TokenClient::new(env, &token_addr)
}

#[test]
fn initialize_once_and_read_owner() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let owner = Address::generate(&env);

    client.initialize(&owner);
    assert_eq!(client.get_owner(), Some(owner.clone()));

    let second_init = client.try_initialize(&owner);
    assert!(second_init.is_err());
}

#[test]
fn create_fund_and_process_successfully() {
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
    );

    client.fund_payment(&payer, &payment_id, &100i128);
    assert_eq!(token.balance(&contract_id), 100i128);

    let processed = client.process_due_payments(&10u32);
    assert_eq!(processed, 1);

    let payment = client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.status, PaymentStatus::Completed);
    assert_eq!(payment.retry_count, 0);
    assert_eq!(token.balance(&recipient), 100i128);
}

#[test]
fn retries_then_succeeds_after_refund() {
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
    );

    // No funding yet, first attempt fails and schedules retry at t=10.
    let processed = client.process_due_payments(&1u32);
    assert_eq!(processed, 1);

    let mut payment = client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.status, PaymentStatus::Pending);
    assert_eq!(payment.retry_count, 1);
    assert_eq!(payment.next_retry_at, 10);

    // Top up before next retry.
    client.fund_payment(&payer, &payment_id, &100i128);
    env.ledger().with_mut(|li| li.timestamp = 10);

    let processed = client.process_due_payments(&1u32);
    assert_eq!(processed, 1);

    payment = client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.status, PaymentStatus::Completed);
    assert_eq!(payment.retry_count, 1);
    assert_eq!(token.balance(&recipient), 100i128);
}

#[test]
fn fails_after_max_retries_and_emits_notification_event() {
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
        &2u32,
        &vec![&env, 5u64],
        &notifier,
    );

    // Retry #1
    let _ = client.process_due_payments(&10u32);
    let mut payment = client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.status, PaymentStatus::Pending);
    assert_eq!(payment.retry_count, 1);
    assert_eq!(payment.next_retry_at, 6);

    // Retry #2
    env.ledger().with_mut(|li| li.timestamp = 6);
    let _ = client.process_due_payments(&10u32);
    payment = client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.status, PaymentStatus::Pending);
    assert_eq!(payment.retry_count, 2);
    assert_eq!(payment.next_retry_at, 11);

    // Retry #3 exceeds max retry attempts and fails terminally.
    env.ledger().with_mut(|li| li.timestamp = 11);
    let _ = client.process_due_payments(&10u32);
    payment = client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.status, PaymentStatus::Failed);
    assert_eq!(payment.retry_count, 3);
    assert_eq!(payment.failure_notifier, notifier);
}

#[test]
fn reuses_last_interval_when_retry_count_exceeds_interval_list() {
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
        &vec![&env, 7u64],
        &notifier,
    );

    // First retry: next at 7
    let _ = client.process_due_payments(&1u32);
    let mut payment = client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.next_retry_at, 7);

    // Second retry: still +7 (reuse last interval)
    env.ledger().with_mut(|li| li.timestamp = 7);
    let _ = client.process_due_payments(&1u32);
    payment = client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.next_retry_at, 14);

    // Third retry: still +7
    env.ledger().with_mut(|li| li.timestamp = 14);
    let _ = client.process_due_payments(&1u32);
    payment = client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.next_retry_at, 21);
}

#[test]
fn cancel_prevents_further_processing() {
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
    );

    client.cancel_payment(&payer, &payment_id);
    let payment = client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.status, PaymentStatus::Cancelled);

    let processed = client.process_due_payments(&10u32);
    assert_eq!(processed, 0);
}

#[test]
fn process_limit_is_enforced() {
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

    let payment_a = client.create_payment_request(
        &payer,
        &recipient_a,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env, 5u64],
        &notifier,
    );
    let payment_b = client.create_payment_request(
        &payer,
        &recipient_b,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env, 5u64],
        &notifier,
    );

    client.fund_payment(&payer, &payment_a, &100i128);
    client.fund_payment(&payer, &payment_b, &100i128);
    assert_eq!(token.balance(&contract_id), 200i128);

    let processed = client.process_due_payments(&1u32);
    assert_eq!(processed, 1);

    let p1 = client.get_payment(&payment_a).unwrap();
    let p2 = client.get_payment(&payment_b).unwrap();
    assert_eq!(p1.status, PaymentStatus::Completed);
    assert_eq!(p2.status, PaymentStatus::Pending);

    let processed = client.process_due_payments(&2u32);
    assert_eq!(processed, 1);
    let p2 = client.get_payment(&payment_b).unwrap();
    assert_eq!(p2.status, PaymentStatus::Completed);
}

#[test]
fn validation_and_access_control_checks() {
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    client.initialize(&owner);

    let payer = Address::generate(&env);
    let another_payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let notifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let invalid_amount = client.try_create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &0i128,
        &1u32,
        &vec![&env, 5u64],
        &notifier,
    );
    assert!(invalid_amount.is_err());

    let missing_intervals = client.try_create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env],
        &notifier,
    );
    assert!(missing_intervals.is_err());

    let payment_id = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &1u32,
        &vec![&env, 5u64],
        &notifier,
    );

    let fund_by_wrong_payer = client.try_fund_payment(&another_payer, &payment_id, &100i128);
    assert!(fund_by_wrong_payer.is_err());

    let cancel_by_wrong_payer = client.try_cancel_payment(&another_payer, &payment_id);
    assert!(cancel_by_wrong_payer.is_err());

    let zero_max_with_empty_intervals = client.create_payment_request(
        &payer,
        &recipient,
        &token.address,
        &100i128,
        &0u32,
        &vec![&env],
        &notifier,
    );

    // Zero retries means first failed attempt becomes terminal.
    let _ = client.process_due_payments(&10u32);
    let payment = client.get_payment(&zero_max_with_empty_intervals).unwrap();
    assert_eq!(payment.status, PaymentStatus::Failed);
    assert_eq!(payment.retry_count, 1);
}
