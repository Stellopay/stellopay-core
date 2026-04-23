#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, BytesN, Env,
};

use payment_scheduler::{PaymentSchedulerContract, PaymentSchedulerContractClient};
use payment_retry::{PaymentRetryContract, PaymentRetryContractClient, RetryState};

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

fn deploy_scheduler(env: &Env, owner: &Address, retry_addr: &Address) -> (Address, PaymentSchedulerContractClient<'_>) {
    let id = env.register_contract(None, PaymentSchedulerContract);
    let client = PaymentSchedulerContractClient::new(env, &id);
    client.initialize(owner, retry_addr);
    (id, client)
}

fn deploy_retry(env: &Env, owner: &Address) -> (Address, PaymentRetryContractClient<'_>) {
    let id = env.register_contract(None, PaymentRetryContract);
    let client = PaymentRetryContractClient::new(env, &id);
    client.initialize(owner);
    (id, client)
}

fn mint(env: &Env, tok: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, tok).mint(to, &amount);
}

fn advance(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| li.timestamp += seconds);
}

#[test]
fn test_retry_orchestration_e2e() {
    let env = env();
    let owner = addr(&env);
    let employer = addr(&env);
    let recipient = addr(&env);
    let tok_addr = token(&env);

    let (retry_id, retry_client) = deploy_retry(&env, &owner);
    let (_sched_id, sched_client) = deploy_scheduler(&env, &owner, &retry_id);

    let amount = 1000i128;
    let start_time = env.ledger().timestamp();
    
    // 1. Create Job
    let _job_id = sched_client.create_job(
        &employer,
        &recipient,
        &tok_addr,
        &amount,
        &3600,
        &start_time,
        &Some(1),
        &3,
    );

    // 2. Process - Fails (Scheduler has 0 balance)
    sched_client.process_due_payments(&1);

    // The payment_id is hash(employer, recipient, amount, start_time)
    let payment_id = {
        use soroban_sdk::xdr::ToXdr;
        let mut buf = soroban_sdk::Bytes::new(&env);
        buf.append(&employer.to_xdr(&env));
        buf.append(&recipient.to_xdr(&env));
        let amount_bytes = amount.to_le_bytes();
        for b in amount_bytes.iter() { buf.push_back(*b); }
        let time_bytes = start_time.to_le_bytes();
        for b in time_bytes.iter() { buf.push_back(*b); }
        env.crypto().sha256(&buf).into()
    };

    // 3. Verify in Retry Contract
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.state, RetryState::Scheduled);

    // 4. Try process_retry - still fails (Retry contract has 0 balance)
    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.state, RetryState::Retrying);
    assert_eq!(payment.retry_count, 1);

    // 5. Add funds to Retry contract and advance time
    mint(&env, &tok_addr, &retry_id, amount);
    advance(&env, 120); // past first retry interval

    // 6. Retry succeeds
    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.state, RetryState::Success);
    
    // 7. Verify recipient balance
    let bal = TokenClient::new(&env, &tok_addr).balance(&recipient);
    assert_eq!(bal, amount);

    // 8. Idempotency test: call again
    retry_client.process_retry(&payment_id);
    let bal = TokenClient::new(&env, &tok_addr).balance(&recipient);
    assert_eq!(bal, amount); // Still the same
}

#[test]
fn test_retry_orchestration_max_retries() {
    let env = env();
    let owner = addr(&env);
    let employer = addr(&env);
    let recipient = addr(&env);
    let tok_addr = token(&env);

    let (retry_id, retry_client) = deploy_retry(&env, &owner);
    let (_sched_id, sched_client) = deploy_scheduler(&env, &owner, &retry_id);

    let amount = 1000i128;
    let start_time = env.ledger().timestamp();
    
    sched_client.create_job(&employer, &recipient, &tok_addr, &amount, &3600, &start_time, &Some(1), &1); // max_retries = 1

    sched_client.process_due_payments(&1);

    let payment_id = {
        use soroban_sdk::xdr::ToXdr;
        let mut buf = soroban_sdk::Bytes::new(&env);
        buf.append(&employer.to_xdr(&env));
        buf.append(&recipient.to_xdr(&env));
        let amount_bytes = amount.to_le_bytes();
        for b in amount_bytes.iter() { buf.push_back(*b); }
        let time_bytes = start_time.to_le_bytes();
        for b in time_bytes.iter() { buf.push_back(*b); }
        env.crypto().sha256(&buf).into()
    };

    // Attempt 1
    retry_client.process_retry(&payment_id);
    assert_eq!(retry_client.get_payment(&payment_id).unwrap().state, RetryState::Retrying);

    // Attempt 2 -> Exceeds max_retries (1)
    advance(&env, 120);
    retry_client.process_retry(&payment_id);
    assert_eq!(retry_client.get_payment(&payment_id).unwrap().state, RetryState::Failed);
}
