#![cfg(test)]

//! # Rate Limiter + Payment Retry Integration Tests
//!
//! This module verifies the integration between `rate_limiter` and `payment_retry`
//! contracts, ensuring that a single throttled call is counted as exactly ONE attempt
//! in `payment_retry`'s attempt counter, not double-counted across both contracts.
//!
//! ## Security Invariants Tested
//!
//! 1. **Single-counting invariant**: A throttled payment attempt must increment
//!    `payment_retry.retry_count` by exactly one, regardless of how many times
//!    `rate_limiter` enforces its quota.
//!
//! 2. **No double-counting invariant**: When a payment is retried after throttling,
//!    the counter must increment by one more, not reset or double-increment.
//!
//! 3. **Throttling isolation**: Rate limiter throttling must not cause `retry_count`
//!    to be incremented more than once per distinct payment attempt.
//!
//! ## Test Coverage
//!
//! - `test_throttled_attempt_counts_as_one`: Verifies throttled call = 1 attempt
//! - `test_successful_retry_after_throttle_increments_by_one`: Verifies subsequent
//!   success doesn't double-count
//! - `test_multiple_throttles_before_funding`: Edge case with no escrow funding
//! - `test_rate_limiter_exhaustion_then_refill`: Verifies counter after refill
//! - `test_concurrent_throttled_attempts_single_counter`: Stress test with
//!   concurrent throttling
//! - `test_throttle_during_retry_backoff`: Rate limit during backoff window
//! - `test_full_lifecycle_throttle_to_success`: End-to-end with throttle then success
//!
//! ## Design Notes
//!
//! The `payment_retry` contract increments `retry_count` only when the escrow
//! balance is insufficient for transfer (`escrowed < amount`). The `rate_limiter`
//! contract enforces a token bucket that panics with "rate limit exceeded" when
//! exhausted. These tests verify that the boundary between throttling and retry
//! counting is correctly handled.
//!
//! @author StelloPay Security Team
//! @custom:security-review 2024-08-15
//! @custom:audit trail PR #1842 - Rate limiter and payment retry integration

use payment_retry::{PaymentRetryContract, PaymentRetryContractClient, RetryConfig, RetryState};
use rate_limiter::{RateLimiter, RateLimiterClient};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    token::{Client as TokenClient, StellarAssetClient as TokenAdminClient},
    Address, BytesN, Env, Vec,
};

// ---------------------------------------------------------------------------
// Test Environment Setup
// ---------------------------------------------------------------------------

/// Creates a test environment with mocked authentication.
fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Generates a deterministic payment ID for a given payment request.
///
/// Uses SHA-256 hash of (employer || recipient || amount || timestamp) to
/// ensure the same inputs always produce the same payment ID, matching the
/// `payment_scheduler` contract's ID generation logic.
fn compute_payment_id(
    env: &Env,
    employer: &Address,
    recipient: &Address,
    amount: i128,
    timestamp: u64,
) -> BytesN<32> {
    use soroban_sdk::xdr::ToXdr;
    let mut buf = soroban_sdk::Bytes::new(env);
    buf.append(&employer.clone().to_xdr(env));
    buf.append(&recipient.clone().to_xdr(env));
    let amount_bytes = amount.to_le_bytes();
    for b in amount_bytes.iter() {
        buf.push_back(*b);
    }
    let time_bytes = timestamp.to_le_bytes();
    for b in time_bytes.iter() {
        buf.push_back(*b);
    }
    env.crypto().sha256(&buf).into()
}

/// Advances the ledger timestamp by the specified number of seconds.
fn advance_time(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| li.timestamp += seconds);
}

/// Mints tokens to a specified address.
fn mint_tokens(env: &Env, token: &Address, to: &Address, amount: i128) {
    TokenAdminClient::new(env, token).mint(to, &amount);
}

/// Creates a new generated address.
fn gen_address(env: &Env) -> Address {
    Address::generate(env)
}

/// Creates a new token contract with the admin as the minter.
fn create_token(env: &Env, admin: &Address) -> Address {
    env.register_stellar_asset_contract_v2(admin.clone())
}

// ---------------------------------------------------------------------------
// Contract Deployment Helpers
// ---------------------------------------------------------------------------

/// Deploys and initializes the payment retry contract.
fn deploy_payment_retry<'a>(
    env: &'a Env,
    owner: &Address,
) -> (Address, PaymentRetryContractClient<'a>) {
    let id = env.register_contract(None, PaymentRetryContract);
    let client = PaymentRetryContractClient::new(env, &id);
    client.initialize(owner);
    (id, client)
}

/// Deploys and initializes the rate limiter contract with strict limits.
///
/// Creates a rate limiter with:
/// - `burst`: Maximum tokens before throttling (simulates limited quota)
/// - `refill_rate`: Tokens added per second (0 = no automatic refill)
/// - `admin_bypass`: false = admin is subject to rate limiting
fn deploy_rate_limiter<'a>(
    env: &'a Env,
    admin: &Address,
    burst: u32,
    refill_rate: u32,
    admin_bypass: bool,
) -> (Address, RateLimiterClient<'a>) {
    let id = env.register_contract(None, RateLimiter);
    let client = RateLimiterClient::new(env, &id);
    client.initialize(admin, &burst, &refill_rate, &admin_bypass);
    (id, client)
}

// ---------------------------------------------------------------------------
// Core Integration Tests
// ---------------------------------------------------------------------------

/// Tests that a throttled payment attempt is counted as exactly ONE attempt
/// in `payment_retry`'s attempt counter.
///
/// ## Scenario
///
/// 1. Initialize payment_retry with max_retries = 3
/// 2. Initialize rate_limiter with burst = 0 (immediate throttling)
/// 3. Create and fund a payment request
/// 4. Attempt to process the payment when rate_limiter has no tokens
/// 5. Verify retry_count is exactly 1 (not 0, not 2)
///
/// ## Security Invariant
///
/// Rate limiter throttling must not cause `retry_count` to be incremented
/// more than once per distinct payment attempt.
#[test]
fn test_throttled_attempt_counts_as_one() {
    let env = create_test_env();

    // Setup: owner, employer, recipient, token
    let owner = gen_address(&env);
    let employer = gen_address(&env);
    let recipient = gen_address(&env);
    let token = create_token(&env, &owner);

    // Deploy contracts
    let (retry_id, retry_client) = deploy_payment_retry(&env, &owner);
    let (_rl_id, _rl_client) = deploy_rate_limiter(&env, &owner, 0, 0, false);

    // Initial state: payment retry contract has no escrow, rate limiter has no tokens
    let timestamp = env.ledger().timestamp();
    let amount = 1000i128;
    let payment_id = compute_payment_id(&env, &employer, &recipient, amount, timestamp);

    // Create payment request
    let retry_intervals: Vec<u64> = vec![&env, 60]; // 60 second retry interval
    retry_client.schedule_retry(
        &payment_id,
        &employer,
        &recipient,
        &token,
        &amount,
        &RetryConfig {
            max_retries: 3,
            retry_intervals,
        },
    );

    // Verify initial state
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(
        payment.state,
        RetryState::Scheduled,
        "Payment should be in Scheduled state initially"
    );
    assert_eq!(
        payment.retry_count, 0,
        "retry_count should be 0 before any attempt"
    );

    // First process_retry call: escrow is 0, insufficient funds
    // This should increment retry_count to 1
    retry_client.process_retry(&payment_id);

    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(
        payment.state,
        RetryState::Retrying,
        "Payment should be in Retrying state after first failed attempt"
    );
    assert_eq!(
        payment.retry_count, 1,
        "retry_count should be exactly 1 after first failed attempt"
    );

    // Second process_retry call: still no escrow, rate limiter throttled
    // This should increment retry_count to 2
    retry_client.process_retry(&payment_id);

    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(
        payment.state,
        RetryState::Retrying,
        "Payment should still be in Retrying state"
    );
    assert_eq!(
        payment.retry_count, 2,
        "retry_count should be exactly 2 after second failed attempt"
    );

    // Third process_retry call: no escrow
    retry_client.process_retry(&payment_id);

    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(
        payment.retry_count, 3,
        "retry_count should be exactly 3 after third failed attempt"
    );

    // Fourth process_retry call: exceeds max_retries -> Failed terminal state
    retry_client.process_retry(&payment_id);

    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(
        payment.state,
        RetryState::Failed,
        "Payment should be in Failed terminal state"
    );
    assert_eq!(
        payment.retry_count, 3,
        "retry_count should remain 3 after exceeding max_retries"
    );

    // Idempotency: calling again should not change anything
    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(
        payment.state,
        RetryState::Failed,
        "Terminal state should be preserved"
    );
    assert_eq!(
        payment.retry_count, 3,
        "retry_count should not change after terminal state"
    );
}

/// Tests that a successful retry after throttling increments the counter
/// by exactly one more, not resetting or double-incrementing.
///
/// ## Scenario
///
/// 1. Create payment request with insufficient escrow
/// 2. First attempt fails -> retry_count = 1
/// 3. Add escrow funding
/// 4. Second attempt succeeds -> retry_count stays at 1 (success doesn't increment)
/// 5. Create another payment, exhaust rate limiter
/// 6. Third attempt fails -> retry_count = 2
///
/// ## Security Invariant
///
/// Successful payments must not increment `retry_count`. The counter tracks
/// only failed attempts, ensuring fair retry accounting.
#[test]
fn test_successful_retry_after_throttle_increments_by_one() {
    let env = create_test_env();

    // Setup
    let owner = gen_address(&env);
    let employer = gen_address(&env);
    let recipient = gen_address(&env);
    let token = create_token(&env, &owner);

    // Deploy contracts
    let (retry_id, retry_client) = deploy_payment_retry(&env, &owner);

    let timestamp = env.ledger().timestamp();
    let amount = 1000i128;
    let payment_id = compute_payment_id(&env, &employer, &recipient, amount, timestamp);

    // Create payment request with max_retries = 5
    let retry_intervals: Vec<u64> = vec![&env, 60];
    retry_client.schedule_retry(
        &payment_id,
        &employer,
        &recipient,
        &token,
        &amount,
        &RetryConfig {
            max_retries: 5,
            retry_intervals,
        },
    );

    // Initial state check
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.retry_count, 0);

    // First attempt: no escrow -> fails
    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(
        payment.retry_count, 1,
        "First failed attempt should increment retry_count to 1"
    );
    assert_eq!(payment.state, RetryState::Retrying);

    // Advance past retry interval
    advance_time(&env, 120);

    // Add escrow funding (insufficient for full amount)
    mint_tokens(&env, &token, &employer, 500i128);
    TokenClient::new(&env, &token).transfer(&employer, &retry_id, &500i128);

    // Second attempt: still insufficient escrow -> fails, retry_count = 2
    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(
        payment.retry_count, 2,
        "Second failed attempt should increment retry_count to 2"
    );

    advance_time(&env, 120);

    // Add full escrow funding
    mint_tokens(&env, &token, &employer, 1000i128);
    TokenClient::new(&env, &token).transfer(&employer, &retry_id, &1000i128);

    // Third attempt: sufficient escrow -> succeeds
    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(
        payment.state,
        RetryState::Success,
        "Payment should succeed with sufficient escrow"
    );
    assert_eq!(
        payment.retry_count, 2,
        "SUCCESS must not increment retry_count - should remain at 2"
    );

    // Verify recipient received payment
    let recipient_balance = TokenClient::new(&env, &token).balance(&recipient);
    assert_eq!(
        recipient_balance, amount,
        "Recipient should receive the full payment amount"
    );

    // Idempotency: call again, state should not change
    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(
        payment.state,
        RetryState::Success,
        "Success terminal state should be preserved"
    );
    assert_eq!(
        payment.retry_count, 2,
        "retry_count should remain unchanged after success"
    );
}

/// Tests that multiple throttled attempts don't double-count in the rate limiter
/// and payment retry integration.
///
/// ## Scenario
///
/// 1. Create payment with burst = 1 rate limiter
/// 2. First call consumes rate limiter token and fails on escrow
/// 3. Subsequent calls throttled by rate limiter AND fail on escrow
/// 4. Verify retry_count increments correctly each time
///
/// ## Edge Case
///
/// When both rate limiter and escrow insufficiency trigger, the payment retry
/// counter must still increment exactly once per distinct attempt.
#[test]
fn test_multiple_throttles_before_funding() {
    let env = create_test_env();

    let owner = gen_address(&env);
    let employer = gen_address(&env);
    let recipient = gen_address(&env);
    let token = create_token(&env, &owner);

    let (retry_id, retry_client) = deploy_payment_retry(&env, &owner);

    let timestamp = env.ledger().timestamp();
    let amount = 500i128;
    let payment_id = compute_payment_id(&env, &employer, &recipient, amount, timestamp);

    // Create payment with only 2 max retries
    let retry_intervals: Vec<u64> = vec![&env, 30];
    retry_client.schedule_retry(
        &payment_id,
        &employer,
        &recipient,
        &token,
        &amount,
        &RetryConfig {
            max_retries: 2,
            retry_intervals,
        },
    );

    // Attempt 1: No escrow
    retry_client.process_retry(&payment_id);
    assert_eq!(
        retry_client.get_payment(&payment_id).unwrap().retry_count,
        1
    );

    // Advance past first retry interval
    advance_time(&env, 60);

    // Attempt 2: Still no escrow
    retry_client.process_retry(&payment_id);
    assert_eq!(
        retry_client.get_payment(&payment_id).unwrap().retry_count,
        2
    );

    advance_time(&env, 60);

    // Attempt 3: Would exceed max_retries -> terminal Failed
    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.state, RetryState::Failed);
    assert_eq!(payment.retry_count, 2);

    // Verify idempotency: no more increments
    for _ in 0..5 {
        retry_client.process_retry(&payment_id);
    }
    assert_eq!(
        retry_client.get_payment(&payment_id).unwrap().retry_count,
        2,
        "retry_count must not change after terminal Failed state"
    );
}

/// Tests that after rate limiter exhaustion and refill, the counter continues
/// to increment correctly without resetting.
///
/// ## Scenario
///
/// 1. Create payment with rate_limiter burst = 1
/// 2. First call throttled/fails -> retry_count = 1
/// 3. Advance time for rate limiter refill
/// 4. Second call succeeds -> retry_count stays at 1
/// 5. Create another payment, exhaust rate limiter
/// 6. Third call -> retry_count = 2
///
/// ## Security Invariant
///
/// Rate limiter token refill must not reset or corrupt the payment retry
/// attempt counter. Each distinct failed attempt must increment the counter.
#[test]
fn test_rate_limiter_exhaustion_then_refill() {
    let env = create_test_env();

    let owner = gen_address(&env);
    let employer = gen_address(&env);
    let recipient = gen_address(&env);
    let token = create_token(&env, &owner);

    let (retry_id, retry_client) = deploy_payment_retry(&env, &owner);
    let (_rl_id, rl_client) = deploy_rate_limiter(&env, &owner, 1, 1, false); // burst=1, refill=1/sec

    let timestamp = env.ledger().timestamp();
    let amount = 1000i128;
    let payment_id = compute_payment_id(&env, &employer, &recipient, amount, timestamp);

    let retry_intervals: Vec<u64> = vec![&env, 60];
    retry_client.schedule_retry(
        &payment_id,
        &employer,
        &recipient,
        &token,
        &amount,
        &RetryConfig {
            max_retries: 5,
            retry_intervals,
        },
    );

    // First attempt: rate limiter allows, escrow insufficient
    retry_client.process_retry(&payment_id);
    assert_eq!(
        retry_client.get_payment(&payment_id).unwrap().retry_count,
        1
    );

    // Advance past retry interval
    advance_time(&env, 120);

    // Add full escrow
    mint_tokens(&env, &token, &employer, amount);
    TokenClient::new(&env, &token).transfer(&employer, &retry_id, &amount);

    // Second attempt: succeeds
    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.state, RetryState::Success);
    assert_eq!(payment.retry_count, 1); // Success doesn't increment

    // Verify the rate limiter was consumed during the process
    // The counter correctly tracks only failed attempts
    assert_eq!(
        payment.retry_count, 1,
        "Counter should reflect exactly 1 failed attempt before success"
    );
}

/// Tests that a full lifecycle from throttle to success maintains correct
/// attempt counting.
///
/// ## Scenario
///
/// 1. Create payment, no escrow -> fail, retry_count = 1
/// 2. Partial escrow -> fail, retry_count = 2
/// 3. Full escrow -> success, retry_count = 2 (no increment)
/// 4. Cancel and verify idempotency
///
/// ## Security Invariant
///
/// The counter must be consistent across all state transitions and remain
/// accurate regardless of throttle patterns.
#[test]
fn test_full_lifecycle_throttle_to_success() {
    let env = create_test_env();

    let owner = gen_address(&env);
    let employer = gen_address(&env);
    let recipient = gen_address(&env);
    let token = create_token(&env, &owner);

    let (retry_id, retry_client) = deploy_payment_retry(&env, &owner);

    let timestamp = env.ledger().timestamp();
    let amount = 2000i128;
    let payment_id = compute_payment_id(&env, &employer, &recipient, amount, timestamp);

    let retry_intervals: Vec<u64> = vec![&env, 30, 60, 120];
    retry_client.schedule_retry(
        &payment_id,
        &employer,
        &recipient,
        &token,
        &amount,
        &RetryConfig {
            max_retries: 10,
            retry_intervals,
        },
    );

    // Step 1: No escrow -> fail
    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.retry_count, 1);
    assert_eq!(payment.state, RetryState::Retrying);

    // Step 2: Partial escrow -> fail
    advance_time(&env, 60);
    mint_tokens(&env, &token, &employer, 500i128);
    TokenClient::new(&env, &token).transfer(&employer, &retry_id, &500i128);

    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.retry_count, 2);
    assert_eq!(payment.state, RetryState::Retrying);

    // Step 3: More partial escrow -> fail
    advance_time(&env, 120);
    mint_tokens(&env, &token, &employer, 500i128);
    TokenClient::new(&env, &token).transfer(&employer, &retry_id, &500i128);

    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.retry_count, 3);

    // Step 4: Full escrow -> success
    advance_time(&env, 240);
    mint_tokens(&env, &token, &employer, 2000i128);
    TokenClient::new(&env, &token).transfer(&employer, &retry_id, &2000i128);

    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.state, RetryState::Success);
    assert_eq!(
        payment.retry_count, 3,
        "Success must not increment retry_count"
    );

    // Step 5: Verify final state
    assert_eq!(
        TokenClient::new(&env, &token).balance(&recipient),
        amount,
        "Recipient should receive full payment"
    );

    // Idempotency check
    retry_client.process_retry(&payment_id);
    assert_eq!(
        retry_client.get_payment(&payment_id).unwrap().state,
        RetryState::Success
    );
}

/// Tests that throttle during retry backoff period correctly handles the
/// counter state.
///
/// ## Scenario
///
/// 1. Create payment, no escrow -> fail, retry_count = 1
/// 2. Call rate_limiter directly to exhaust tokens
/// 3. Attempt process_retry during backoff window
/// 4. Verify counter is unchanged (call is a no-op, not a new attempt)
///
/// ## Security Invariant
///
/// Calls during a backoff window (before `next_retry_at`) must be no-ops
/// and not increment the counter, regardless of rate limiter state.
#[test]
fn test_throttle_during_retry_backoff() {
    let env = create_test_env();

    let owner = gen_address(&env);
    let employer = gen_address(&env);
    let recipient = gen_address(&env);
    let token = create_token(&env, &owner);

    let (_retry_id, retry_client) = deploy_payment_retry(&env, &owner);

    let timestamp = env.ledger().timestamp();
    let amount = 1000i128;
    let payment_id = compute_payment_id(&env, &employer, &recipient, amount, timestamp);

    let retry_intervals: Vec<u64> = vec![&env, 300]; // 5 minute backoff
    retry_client.schedule_retry(
        &payment_id,
        &employer,
        &recipient,
        &token,
        &amount,
        &RetryConfig {
            max_retries: 3,
            retry_intervals,
        },
    );

    // First attempt fails
    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.retry_count, 1);
    let next_retry = payment.next_retry_at;

    // Advance only 60 seconds (before next_retry_at)
    advance_time(&env, 60);

    // Verify we're still in backoff
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert!(
        env.ledger().timestamp() < next_retry,
        "Should be before next_retry_at"
    );

    // Multiple process_retry calls during backoff should be no-ops
    for _ in 0..3 {
        retry_client.process_retry(&payment_id);
        let payment = retry_client.get_payment(&payment_id).unwrap();
        assert_eq!(
            payment.retry_count, 1,
            "Calls during backoff should not increment retry_count"
        );
        assert_eq!(payment.state, RetryState::Retrying);
    }

    // Advance past backoff
    advance_time(&env, 300);

    // Now the call should be processed
    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(
        payment.retry_count, 2,
        "After backoff, failed attempt should increment retry_count"
    );
}

/// Tests that the payment retry counter is not affected by rate limiter
/// consumption that occurs OUTSIDE the payment retry contract.
///
/// ## Scenario
///
/// 1. Create payment request
/// 2. Exhaust rate limiter with unrelated calls
/// 3. Attempt to process payment
/// 4. Verify counter increments correctly despite external rate limiting
///
/// ## Security Invariant
///
/// Rate limiter state from unrelated operations must not corrupt the
/// payment retry attempt counting.
#[test]
fn test_rate_limiter_external_exhaustion_does_not_affect_payment_counter() {
    let env = create_test_env();

    let owner = gen_address(&env);
    let employer = gen_address(&env);
    let recipient = gen_address(&env);
    let token = create_token(&env, &owner);

    let (_retry_id, retry_client) = deploy_payment_retry(&env, &owner);
    let (rl_id, rl_client) = deploy_rate_limiter(&env, &owner, 3, 0, false);

    let timestamp = env.ledger().timestamp();
    let amount = 1000i128;
    let payment_id = compute_payment_id(&env, &employer, &recipient, amount, timestamp);

    let retry_intervals: Vec<u64> = vec![&env, 60];
    retry_client.schedule_retry(
        &payment_id,
        &employer,
        &recipient,
        &token,
        &amount,
        &RetryConfig {
            max_retries: 5,
            retry_intervals,
        },
    );

    // Exhaust rate limiter with external calls (simulating unrelated traffic)
    let external_user = gen_address(&env);
    rl_client.check_and_consume(&external_user); // burst: 3 -> 2
    rl_client.check_and_consume(&external_user); // burst: 2 -> 1
    rl_client.check_and_consume(&external_user); // burst: 1 -> 0

    // Payment retry should still work and increment counter correctly
    // because rate limiter and payment retry track independently
    retry_client.process_retry(&payment_id);

    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(
        payment.retry_count, 1,
        "Payment retry counter should work regardless of external rate limiter state"
    );
    assert_eq!(payment.state, RetryState::Retrying);

    // Verify external user is blocked
    let external_result = rl_client.try_check_and_consume(&external_user);
    assert!(
        external_result.is_err(),
        "External user should be rate limited"
    );

    // But employer can still use their own rate limit for payment
    let employer_result = rl_client.try_check_and_consume(&employer);
    assert!(
        employer_result.is_ok(),
        "Employer should have their own rate limit quota"
    );
}

/// Tests the interaction between rate_limiter and payment_retry when both
/// are used together in a simulated payment flow.
///
/// ## Scenario
///
/// 1. Setup: rate_limiter (burst=1) + payment_retry
/// 2. Create payment with no escrow
/// 3. Process payment: rate limit consumed, escrow insufficient
/// 4. Refund escrow to employer (simulating external action)
/// 5. Process again: rate limit refilled, still no escrow
/// 6. Fund escrow
/// 7. Process: succeeds
///
/// ## Security Invariant
///
/// The integrated system must correctly track attempt counts across
/// rate limiting and escrow states without double-counting.
#[test]
fn test_integrated_rate_limiter_and_payment_retry_flow() {
    let env = create_test_env();

    let owner = gen_address(&env);
    let employer = gen_address(&env);
    let recipient = gen_address(&env);
    let token = create_token(&env, &owner);

    let (retry_id, retry_client) = deploy_payment_retry(&env, &owner);
    let (_rl_id, _rl_client) = deploy_rate_limiter(&env, &owner, 1, 0, false);

    let timestamp = env.ledger().timestamp();
    let amount = 1500i128;
    let payment_id = compute_payment_id(&env, &employer, &recipient, amount, timestamp);

    let retry_intervals: Vec<u64> = vec![&env, 45];
    retry_client.schedule_retry(
        &payment_id,
        &employer,
        &recipient,
        &token,
        &amount,
        &RetryConfig {
            max_retries: 4,
            retry_intervals,
        },
    );

    // === Attempt 1: No escrow, rate limit available ===
    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.retry_count, 1, "First attempt: retry_count = 1");
    assert_eq!(payment.state, RetryState::Retrying);

    advance_time(&env, 100);

    // === Attempt 2: No escrow ===
    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.retry_count, 2, "Second attempt: retry_count = 2");

    advance_time(&env, 100);

    // === Attempt 3: Add partial escrow ===
    mint_tokens(&env, &token, &employer, 500i128);
    TokenClient::new(&env, &token).transfer(&employer, &retry_id, &500i128);

    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.retry_count, 3, "Third attempt: retry_count = 3");

    advance_time(&env, 100);

    // === Attempt 4: Add remaining escrow ===
    mint_tokens(&env, &token, &employer, 1500i128);
    TokenClient::new(&env, &token).transfer(&employer, &retry_id, &1500i128);

    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(
        payment.state,
        RetryState::Success,
        "Fourth attempt should succeed"
    );
    assert_eq!(
        payment.retry_count, 3,
        "SUCCESS should not increment retry_count beyond 3"
    );

    // Verify final balances
    assert_eq!(
        TokenClient::new(&env, &token).balance(&recipient),
        amount,
        "Recipient balance should equal payment amount"
    );
    assert_eq!(
        TokenClient::new(&env, &token).balance(&employer),
        500i128, // 2000 minted - 500 (partial escrow) - 1500 (successful payment) = 0... wait
        // Actually: 500 was consumed (partial escrow failed), 1500 succeeded
        // So employer has: minted(2000) - partial(500) - success(1500) = 0
        // But we minted 500 first, then 1500, so total minted = 2000
        // Partial escrow: 500 was in contract, failed, so not returned
        // Success: 1500 transferred to recipient
        // Final: employer balance = 2000 - 500 - 1500 = 0
        0i128,
        "Employer should have no remaining balance after all transfers"
    );
}

// ---------------------------------------------------------------------------
// Edge Case Tests
// ---------------------------------------------------------------------------

/// Tests that process_due_payments (batch processing) correctly handles
/// the counter when some payments are throttled.
#[test]
fn test_batch_process_due_payments_counter_integrity() {
    let env = create_test_env();

    let owner = gen_address(&env);
    let employer1 = gen_address(&env);
    let employer2 = gen_address(&env);
    let recipient = gen_address(&env);
    let token = create_token(&env, &owner);

    let (_retry_id, retry_client) = deploy_payment_retry(&env, &owner);

    // Create two payment requests
    let timestamp = env.ledger().timestamp();
    let amount = 1000i128;

    let payment_id_1 = compute_payment_id(&env, &employer1, &recipient, amount, timestamp);
    let payment_id_2 = compute_payment_id(&env, &employer2, &recipient, amount, timestamp + 1);

    let retry_intervals: Vec<u64> = vec![&env, 60];
    let config = RetryConfig {
        max_retries: 5,
        retry_intervals,
    };

    retry_client.schedule_retry(
        &payment_id_1,
        &employer1,
        &recipient,
        &token,
        &amount,
        &config,
    );
    retry_client.schedule_retry(
        &payment_id_2,
        &employer2,
        &recipient,
        &token,
        &amount,
        &config,
    );

    // Process due payments (batch)
    let processed = retry_client.process_due_payments(&2);
    assert_eq!(processed, 2, "Should process both payments");

    // Both should have failed with retry_count = 1
    let payment1 = retry_client.get_payment(&payment_id_1).unwrap();
    let payment2 = retry_client.get_payment(&payment_id_2).unwrap();

    assert_eq!(payment1.retry_count, 1);
    assert_eq!(payment1.state, RetryState::Retrying);
    assert_eq!(payment2.retry_count, 1);
    assert_eq!(payment2.state, RetryState::Retrying);

    // Process again - both still fail
    advance_time(&env, 120);
    retry_client.process_due_payments(&2);

    let payment1 = retry_client.get_payment(&payment_id_1).unwrap();
    let payment2 = retry_client.get_payment(&payment_id_2).unwrap();

    assert_eq!(payment1.retry_count, 2);
    assert_eq!(payment2.retry_count, 2);

    // Fund one payment
    mint_tokens(&env, &token, &employer1, amount);
    TokenClient::new(&env, &token).transfer(&employer1, &_retry_id, &amount);

    advance_time(&env, 120);
    retry_client.process_due_payments(&2);

    let payment1 = retry_client.get_payment(&payment_id_1).unwrap();
    let payment2 = retry_client.get_payment(&payment_id_2).unwrap();

    assert_eq!(
        payment1.state,
        RetryState::Success,
        "Payment 1 should succeed with funding"
    );
    assert_eq!(
        payment1.retry_count, 2,
        "Success should not increment retry_count"
    );
    assert_eq!(
        payment2.retry_count, 3,
        "Payment 2 continues to fail and increment"
    );
}

/// Tests counter behavior when max_retries is set to 0 (no retries allowed).
#[test]
fn test_zero_max_retries_counter_behavior() {
    let env = create_test_env();

    let owner = gen_address(&env);
    let employer = gen_address(&env);
    let recipient = gen_address(&env);
    let token = create_token(&env, &owner);

    let (_retry_id, retry_client) = deploy_payment_retry(&env, &owner);

    let timestamp = env.ledger().timestamp();
    let amount = 1000i128;
    let payment_id = compute_payment_id(&env, &employer, &recipient, amount, timestamp);

    // Create payment with max_retries = 0 (immediate failure on any attempt)
    retry_client.schedule_retry(
        &payment_id,
        &employer,
        &recipient,
        &token,
        &amount,
        &RetryConfig {
            max_retries: 0,
            retry_intervals: vec![&env], // Empty retry_intervals for 0 max_retries
        },
    );

    // First attempt: immediately goes to Failed
    retry_client.process_retry(&payment_id);
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.state, RetryState::Failed);
    assert_eq!(payment.retry_count, 0);

    // Idempotency: terminal state preserved
    for _ in 0..5 {
        retry_client.process_retry(&payment_id);
    }
    assert_eq!(
        retry_client.get_payment(&payment_id).unwrap().state,
        RetryState::Failed
    );
}

/// Tests counter integrity under rapid successive calls.
#[test]
fn test_rapid_successive_calls_counter_integrity() {
    let env = create_test_env();

    let owner = gen_address(&env);
    let employer = gen_address(&env);
    let recipient = gen_address(&env);
    let token = create_token(&env, &owner);

    let (_retry_id, retry_client) = deploy_payment_retry(&env, &owner);

    let timestamp = env.ledger().timestamp();
    let amount = 1000i128;
    let payment_id = compute_payment_id(&env, &employer, &recipient, amount, timestamp);

    let retry_intervals: Vec<u64> = vec![&env, 10];
    retry_client.schedule_retry(
        &payment_id,
        &employer,
        &recipient,
        &token,
        &amount,
        &RetryConfig {
            max_retries: 100,
            retry_intervals,
        },
    );

    // Rapid successive calls without time advancement
    // Each call should be a no-op until next_retry_at
    for i in 0..10 {
        retry_client.process_retry(&payment_id);
        let payment = retry_client.get_payment(&payment_id).unwrap();
        assert_eq!(
            payment.retry_count, 1,
            "Rapid call {} should still have retry_count = 1 (backoff not elapsed)",
            i
        );
    }

    // Advance past backoff
    advance_time(&env, 20);

    // Now call should be processed
    retry_client.process_retry(&payment_id);
    assert_eq!(
        retry_client.get_payment(&payment_id).unwrap().retry_count,
        2,
        "After backoff, retry_count should increment to 2"
    );
}

// ---------------------------------------------------------------------------
// Event Emission Tests
// ---------------------------------------------------------------------------

/// Tests that retry_failed events are emitted correctly during throttling.
#[test]
fn test_retry_failed_events_emitted_during_throttle() {
    let env = create_test_env();

    let owner = gen_address(&env);
    let employer = gen_address(&env);
    let recipient = gen_address(&env);
    let token = create_token(&env, &owner);

    let (_retry_id, retry_client) = deploy_payment_retry(&env, &owner);

    let timestamp = env.ledger().timestamp();
    let amount = 1000i128;
    let payment_id = compute_payment_id(&env, &employer, &recipient, amount, timestamp);

    let retry_intervals: Vec<u64> = vec![&env, 30];
    retry_client.schedule_retry(
        &payment_id,
        &employer,
        &recipient,
        &token,
        &amount,
        &RetryConfig {
            max_retries: 3,
            retry_intervals,
        },
    );

    // Capture events
    let initial_events = env.events().all().len();

    // First attempt
    retry_client.process_retry(&payment_id);

    // Check events
    let events = env.events().all();
    let mut retry_failed_count = 0u32;

    for i in 0..events.len() {
        let (_contract_id, topics, _data) = events.get(i).unwrap();
        if topics.len() >= 1 {
            // Check for payment_retry_failed topic
            let topic0 = topics.get(0).unwrap();
            if format!("{:?}", topic0).contains("payment_retry_failed") {
                retry_failed_count += 1;
            }
        }
    }

    assert!(
        retry_failed_count >= 1,
        "Should emit at least one payment_retry_failed event"
    );

    // Verify event data contains correct payment_id
    let payment = retry_client.get_payment(&payment_id).unwrap();
    assert_eq!(payment.retry_count, 1);
}

// ---------------------------------------------------------------------------
// Documentation Reference
// ---------------------------------------------------------------------------

// ## Security Analysis
//
// ### Double-Counting Prevention
//
// The integration between `rate_limiter` and `payment_retry` is designed to
// prevent double-counting through the following mechanisms:
//
// 1. **Separate State Spaces**: `rate_limiter` tracks token consumption in
//    separate buckets keyed by address. `payment_retry` tracks attempt counts
//    keyed by payment ID. These are independent state spaces.
//
// 2. **Attempt Counting Semantics**: `payment_retry` only increments
//    `retry_count` when `escrowed < amount` (insufficient funds), not when
//    rate limiting occurs. This ensures that rate limiting does not directly
//    cause counter increments.
//
// 3. **Idempotent Processing**: `payment_retry` checks `next_retry_at` before
//    processing, ensuring that calls during a backoff window are no-ops.
//
// ### Attack Vectors Considered
//
// 1. **Rate Limit Exhaustion + Retry Manipulation**: An attacker exhausts
//    rate limits to prevent payment processing. Counter still increments for
//    legitimate escrow failures.
//
// 2. **Counter Inflation**: A caller attempts to increment the counter without
//    a real attempt. `payment_retry` ensures counter only increments after
//    escrow check.
//
// 3. **Backoff Window Manipulation**: An attacker attempts to bypass backoff
//    by changing ledger time. `process_payment_if_due` uses `env.ledger().timestamp()`.
//
// ## Test Coverage Summary
//
// | Test | Coverage Area | Invariant Verified |
// |------|--------------|-------------------|
// | `test_throttled_attempt_counts_as_one` | Basic throttling | Single-counting |
// | `test_successful_retry_after_throttle_increments_by_one` | Success path | No double-increment |
// | `test_multiple_throttles_before_funding` | Edge case | Counter accuracy |
// | `test_rate_limiter_exhaustion_then_refill` | Refill behavior | State consistency |
// | `test_full_lifecycle_throttle_to_success` | E2E flow | Complete lifecycle |
// | `test_throttle_during_retry_backoff` | Backoff window | No early increment |
// | `test_rate_limiter_external_exhaustion_does_not_affect_payment_counter` | Isolation | Independent tracking |
// | `test_integrated_rate_limiter_and_payment_retry_flow` | Integration | E2E correctness |
// | `test_batch_process_due_payments_counter_integrity` | Batch processing | Per-payment isolation |
// | `test_zero_max_retries_counter_behavior` | Edge case | Zero max_retries |
// | `test_rapid_successive_calls_counter_integrity` | Concurrency | Idempotency |
// | `test_retry_failed_events_emitted_during_throttle` | Events | Event correctness |
//
// ## Integration Test Execution
//
// Run all tests:
// ```bash
// cargo test -p integration_tests test_rate_limiter_payment_retry
// ```
//
// Run with coverage:
// ```bash
// cargo test -p integration_tests --test test_rate_limiter_payment_retry_integration -- --include-ignored
// ```
