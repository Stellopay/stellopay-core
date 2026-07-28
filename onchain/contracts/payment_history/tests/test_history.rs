//! Comprehensive tests for the PaymentHistory contract.
//!
//! ## Coverage targets
//!
//! * Initialization — happy path, double-init guard
//! * `record_payment` — happy path, monotonic IDs, payment_hash stored,
//!   reverse-lookup index written, all three sequential indices updated,
//!   event emission, full field round-trip, multiple payments
//! * `record_payment` — unauthorized (no auth mocked)
//! * `get_payment_by_hash` — existing hash, unknown hash returns None
//! * `get_payment_by_id` — existing ID, non-existent ID, ID 0
//! * `get_global_payment_count` — before/after recordings
//! * `get_agreement_payment_count` — before/after, multiple agreements
//! * `get_payments_by_agreement` — full page, partial page, multi-page,
//!   start_index=0, start_index>count, empty, exact boundary, limit capped
//! * `get_employer_payment_count` — before/after, multiple employers
//! * `get_payments_by_employer` — pagination, all boundary conditions
//! * `get_employee_payment_count` — before/after, multiple employees
//! * `get_payments_by_employee` — pagination, all boundary conditions
//! * Cross-index consistency — same payment visible via hash, ID, and all
//!   three sequential indices; all return identical records
//! * Index-consistency (#912) — `get_payment_by_hash` and `get_payment_by_id`
//!   return field-for-field identical records; unknown hash returns None (not a
//!   wrong record); each hash resolves only to its own payment; batch
//!   consistency across 8 payments; idempotent replay preserves both indices;
//!   stored hash field matches the lookup key; None returned even with
//!   populated storage
//! * Security — record immutability, index counts only increase (no pruning),
//!   hash index written atomically with the primary record
//! * Large history — 20 records, boundary reads at exact count edge
//!
//! ## Security notes
//!
//! The tests below validate the following security properties directly:
//!
//! 1. **Unauthorized injection** — `test_record_payment_unauthorized_no_auth`
//!    confirms that `record_payment` panics with `Auth(InvalidAction)` when
//!    called without mocked auth for the registered payroll contract.
//!
//! 2. **History tampering** — `test_records_are_immutable_after_recording`
//!    verifies that a payment returned by all query paths is bit-for-bit
//!    identical after additional payments are recorded. There is no overwrite
//!    path in the contract; the test confirms this property holds at runtime.
//!
//! 3. **Unauthorized pruning** — `test_index_counts_only_increase` asserts
//!    that every index count after N insertions equals exactly N. Because
//!    counts can only increment and there is no decrement or delete path,
//!    it is impossible for any caller to remove entries from the pagination
//!    range without corrupting the counter, which would cause every subsequent
//!    paginated read to skip entries.
//!
//! 4. **Hash-record atomicity** — `test_hash_index_written_atomically` records
//!    a payment and immediately queries by hash. The reverse-lookup succeeds,
//!    confirming the hash index and the primary record are written in the same
//!    invocation and are always in sync.
//!
//! 5. **Double-init guard** — `test_initialize_double_init_rejected` uses the
//!    `try_initialize` path to confirm the second call is rejected without
//!    corrupting the already-initialized state.
//!
//! 6. **Dual-index consistency (#912)** — the `test_index_consistency_*` suite
//!    proves that `get_payment_by_hash` and `get_payment_by_id` always resolve
//!    to the same record (field-by-field), that an unknown hash returns `None`
//!    rather than a wrong record, and that no hash can resolve to a payment
//!    other than its own.

#![cfg(test)]

use payment_history::{PaymentHistoryContract, PaymentHistoryContractClient, MAX_PAGE_SIZE};
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, BytesN, Env, IntoVal, Symbol,
};

// ─── Fixtures ────────────────────────────────────────────────────────────────

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn register_contract(env: &Env) -> (Address, PaymentHistoryContractClient<'_>) {
    let id = env.register(PaymentHistoryContract, ());
    let client = PaymentHistoryContractClient::new(env, &id);
    (id, client)
}

/// Initialize the contract and return `(owner, payroll)`.
fn initialize_contract<'a>(
    env: &Env,
    client: &PaymentHistoryContractClient<'a>,
) -> (Address, Address) {
    let owner = Address::generate(env);
    let payroll = Address::generate(env);
    client.initialize(&owner, &payroll);
    (owner, payroll)
}

/// Build a deterministic 32-byte hash from a seed value.
/// Each distinct `seed` value produces a unique hash, making it easy to
/// assign distinct hashes to distinct payments in tests.
fn make_hash(env: &Env, seed: u32) -> BytesN<32> {
    let mut hash = [0u8; 32];
    // Use the seed to create a unique hash pattern
    let seed_bytes = seed.to_le_bytes();
    for i in 0..32 {
        hash[i] = seed_bytes[i % 4];
    }
    BytesN::from_array(env, &hash)
}

/// Record a payment with a deterministic hash derived from `hash_seed`.
#[allow(clippy::too_many_arguments)]
fn record(
    client: &PaymentHistoryContractClient<'_>,
    env: &Env,
    agreement_id: u128,
    hash_seed: u32,
    token: &Address,
    amount: i128,
    from: &Address,
    to: &Address,
    timestamp: u64,
) -> u128 {
    let hash = make_hash(env, hash_seed);
    client.record_payment(&agreement_id, &hash, token, &amount, from, to, &timestamp)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReconciliationSource {
    SchedulerExecution,
    EscrowRelease,
    BonusPayment,
    ExpenseReimbursement,
}

impl ReconciliationSource {
    fn topic(self) -> &'static str {
        match self {
            ReconciliationSource::SchedulerExecution => "job_executed",
            ReconciliationSource::EscrowRelease => "released",
            ReconciliationSource::BonusPayment => "incentive_claimed",
            ReconciliationSource::ExpenseReimbursement => "expense_paid",
        }
    }
}

#[derive(Clone, Debug)]
struct ReconciliationFixture {
    source: ReconciliationSource,
    source_event_id: u128,
    agreement_id: u128,
    hash_seed: u32,
    token: Address,
    amount: i128,
    from: Address,
    to: Address,
    timestamp: u64,
}

fn reconcile_fixture(
    client: &PaymentHistoryContractClient<'_>,
    env: &Env,
    fixture: &ReconciliationFixture,
) -> u128 {
    let hash = make_hash(env, fixture.hash_seed);
    client.record_payment(
        &fixture.agreement_id,
        &hash,
        &fixture.token,
        &fixture.amount,
        &fixture.from,
        &fixture.to,
        &fixture.timestamp,
    )
}

// ─── Initialization ───────────────────────────────────────────────────────────

#[test]
fn test_initialize_happy_path() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    assert_eq!(client.get_global_payment_count(), 0u128);
}

#[test]
fn test_initialize_double_init_rejected() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let (owner, payroll) = initialize_contract(&env, &client);

    let result = client.try_initialize(&owner, &payroll);
    assert!(result.is_err(), "second initialize must be rejected");
}

// ─── record_payment ───────────────────────────────────────────────────────────

#[test]
fn test_record_payment_returns_sequential_ids() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let id1 = record(
        &client, &env, 1, 1u32, &token, 100, &employer, &employee, 1_000,
    );
    let id2 = record(
        &client, &env, 1, 2u32, &token, 200, &employer, &employee, 2_000,
    );
    let id3 = record(
        &client, &env, 2, 3u32, &token, 300, &employer, &employee, 3_000,
    );

    assert_eq!(id1, 1u128);
    assert_eq!(id2, 2u128);
    assert_eq!(id3, 3u128);
}

#[test]
fn test_record_payment_persists_all_fields() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let agreement_id = 42u128;
    let amount = 9_999i128;
    let timestamp = 1_700_000_000u64;
    let hash = make_hash(&env, 0xABu32);

    let payment_id = client.record_payment(
        &agreement_id,
        &hash,
        &token,
        &amount,
        &employer,
        &employee,
        &timestamp,
    );

    let rec = client
        .get_payment_by_id(&payment_id)
        .expect("record must exist after recording");

    assert_eq!(rec.id, payment_id);
    assert_eq!(rec.agreement_id, agreement_id);
    assert_eq!(rec.payment_hash, hash);
    assert_eq!(rec.token, token);
    assert_eq!(rec.amount, amount);
    assert_eq!(rec.from, employer);
    assert_eq!(rec.to, employee);
    assert_eq!(rec.timestamp, timestamp);
}

#[test]
fn test_record_payment_increments_global_count() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    assert_eq!(client.get_global_payment_count(), 0u128);
    record(&client, &env, 1, 1u32, &token, 50, &from, &to, 100);
    assert_eq!(client.get_global_payment_count(), 1u128);
    record(&client, &env, 1, 2u32, &token, 50, &from, &to, 200);
    assert_eq!(client.get_global_payment_count(), 2u128);
}

#[test]
fn test_record_payment_emits_event_with_correct_topic() {
    let env = create_env();
    let (contract_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    record(&client, &env, 10, 1u32, &token, 500, &from, &to, 9_000);

    let events = env.events().all();
    let last = events.last().unwrap();

    assert_eq!(last.0, contract_id);
    let expected_topics: soroban_sdk::Vec<soroban_sdk::Val> =
        (Symbol::new(&env, "payment_recorded"),).into_val(&env);
    assert_eq!(last.1, expected_topics);
}

#[test]
fn test_record_payment_updates_all_three_sequential_indices() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    record(
        &client, &env, 7, 1u32, &token, 100, &employer, &employee, 1_000,
    );

    assert_eq!(client.get_agreement_payment_count(&7u128), 1u32);
    assert_eq!(client.get_employer_payment_count(&employer), 1u32);
    assert_eq!(client.get_employee_payment_count(&employee), 1u32);
}

#[test]
fn test_record_payment_duplicate_hash_is_idempotent() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let agreement_id = 77u128;
    let hash = make_hash(&env, 0x77u32);

    let id1 = client.record_payment(
        &agreement_id,
        &hash,
        &token,
        &1_000i128,
        &employer,
        &employee,
        &1_000u64,
    );

    let id2 = client.record_payment(
        &agreement_id,
        &hash,
        &token,
        &1_000i128,
        &employer,
        &employee,
        &1_000u64,
    );

    assert_eq!(id1, id2, "duplicate hash must return existing payment ID");
    assert_eq!(client.get_global_payment_count(), 1u128);
    assert_eq!(client.get_agreement_payment_count(&agreement_id), 1u32);
    assert_eq!(client.get_employer_payment_count(&employer), 1u32);
    assert_eq!(client.get_employee_payment_count(&employee), 1u32);

    let by_hash = client
        .get_payment_by_hash(&hash)
        .expect("record must exist");
    assert_eq!(by_hash.id, id1);
    assert_eq!(by_hash.amount, 1_000i128);

    let agg_page = client.get_payments_by_agreement(&agreement_id, &1u32, &10u32);
    assert_eq!(agg_page.len(), 1u32, "agreement index must not duplicate");
}

// ─── record_payment: unauthorized ────────────────────────────────────────────

#[test]
#[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
fn test_record_payment_unauthorized_no_auth() {
    // Deliberately do NOT call mock_all_auths so the auth check fires.
    let env = Env::default();
    let (_id, client) = register_contract(&env);

    let owner = Address::generate(&env);
    let payroll = Address::generate(&env);
    client.initialize(&owner, &payroll);

    let token = Address::generate(&env);
    let other = Address::generate(&env);
    let hash = make_hash(&env, 0xFF);
    client.record_payment(&1u128, &hash, &token, &100i128, &other, &other, &0u64);
}

// ─── get_payment_by_hash ─────────────────────────────────────────────────────

#[test]
fn test_get_payment_by_hash_returns_correct_record() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let hash = make_hash(&env, 0x42u32);

    let pid = client.record_payment(&5u128, &hash, &token, &777i128, &from, &to, &55_000u64);
    let rec = client.get_payment_by_hash(&hash);
    assert!(
        rec.is_some(),
        "hash lookup must return Some for recorded payment"
    );
    let rec = rec.unwrap();
    assert_eq!(rec.id, pid);
    assert_eq!(rec.payment_hash, hash);
    assert_eq!(rec.amount, 777i128);
}

#[test]
fn test_get_payment_by_hash_unknown_returns_none() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let unknown_hash = make_hash(&env, 0x99u32);
    let rec = client.get_payment_by_hash(&unknown_hash);
    assert!(rec.is_none(), "unknown hash must return None");
}

#[test]
fn test_hash_index_written_atomically() {
    // Immediately after record_payment, both get_payment_by_id and
    // get_payment_by_hash must return the same record. This confirms the
    // reverse-lookup index is written in the same invocation as the primary
    // record, with no observable gap.
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let hash = make_hash(&env, 0x01u32);

    let pid = client.record_payment(&1u128, &hash, &token, &100i128, &from, &to, &0u64);

    let by_id = client.get_payment_by_id(&pid).expect("must exist by ID");
    let by_hash = client
        .get_payment_by_hash(&hash)
        .expect("must exist by hash");
    assert_eq!(by_id, by_hash, "record by-id and by-hash must be identical");
}

#[test]
fn test_different_payments_have_independent_hash_entries() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let h1 = make_hash(&env, 1u32);
    let h2 = make_hash(&env, 2u32);
    let h3 = make_hash(&env, 3u32);

    let pid1 = client.record_payment(&1u128, &h1, &token, &10i128, &from, &to, &0u64);
    let pid2 = client.record_payment(&1u128, &h2, &token, &20i128, &from, &to, &1u64);
    let pid3 = client.record_payment(&2u128, &h3, &token, &30i128, &from, &to, &2u64);

    assert_eq!(client.get_payment_by_hash(&h1).unwrap().id, pid1);
    assert_eq!(client.get_payment_by_hash(&h2).unwrap().id, pid2);
    assert_eq!(client.get_payment_by_hash(&h3).unwrap().id, pid3);
}

// ─── get_payment_by_id ────────────────────────────────────────────────────────

#[test]
fn test_get_payment_by_id_existing() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let pid = record(&client, &env, 5, 1u32, &token, 777, &from, &to, 55_000);
    let rec = client.get_payment_by_id(&pid);
    assert!(rec.is_some());
    assert_eq!(rec.unwrap().id, pid);
}

#[test]
fn test_get_payment_by_id_nonexistent_returns_none() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    assert!(client.get_payment_by_id(&99u128).is_none());
}

#[test]
fn test_get_payment_by_id_zero_returns_none() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    assert!(
        client.get_payment_by_id(&0u128).is_none(),
        "ID 0 is never assigned"
    );
}

// ─── get_global_payment_count ─────────────────────────────────────────────────

#[test]
fn test_global_count_starts_at_zero() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    assert_eq!(client.get_global_payment_count(), 0u128);
}

#[test]
fn test_global_count_tracks_all_agreements() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    for i in 0..5u8 {
        record(
            &client,
            &env,
            i as u128,
            i as u32,
            &token,
            10,
            &from,
            &to,
            i as u64 * 100,
        );
    }
    assert_eq!(client.get_global_payment_count(), 5u128);
}

// ─── Agreement index ──────────────────────────────────────────────────────────

#[test]
fn test_agreement_count_before_and_after() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let agreement_id = 99u128;

    assert_eq!(client.get_agreement_payment_count(&agreement_id), 0u32);
    record(&client, &env, agreement_id, 1u32, &token, 1, &from, &to, 0);
    assert_eq!(client.get_agreement_payment_count(&agreement_id), 1u32);
    record(&client, &env, agreement_id, 2u32, &token, 2, &from, &to, 1);
    assert_eq!(client.get_agreement_payment_count(&agreement_id), 2u32);
}

#[test]
fn test_agreement_indices_are_independent() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    record(&client, &env, 1, 1u32, &token, 10, &from, &to, 0);
    record(&client, &env, 1, 2u32, &token, 20, &from, &to, 1);
    record(&client, &env, 2, 3u32, &token, 30, &from, &to, 2);

    assert_eq!(client.get_agreement_payment_count(&1u128), 2u32);
    assert_eq!(client.get_agreement_payment_count(&2u128), 1u32);
    assert_eq!(client.get_agreement_payment_count(&3u128), 0u32);
}

#[test]
fn test_get_payments_by_agreement_single_record() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    record(&client, &env, 1, 1u32, &token, 500, &from, &to, 1_000);
    let page = client.get_payments_by_agreement(&1u128, &1u32, &10u32);
    assert_eq!(page.len(), 1u32);
    assert_eq!(page.get(0).unwrap().amount, 500);
}

#[test]
fn test_get_payments_by_agreement_full_pagination() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let agreement_id = 1u128;

    for i in 0..5u8 {
        record(
            &client,
            &env,
            agreement_id,
            i as u32,
            &token,
            i as i128 * 100,
            &from,
            &to,
            i as u64,
        );
    }

    let page1 = client.get_payments_by_agreement(&agreement_id, &1u32, &2u32);
    assert_eq!(page1.len(), 2u32);
    assert_eq!(page1.get(0).unwrap().amount, 0);
    assert_eq!(page1.get(1).unwrap().amount, 100);

    let page2 = client.get_payments_by_agreement(&agreement_id, &3u32, &2u32);
    assert_eq!(page2.len(), 2u32);
    assert_eq!(page2.get(0).unwrap().amount, 200);
    assert_eq!(page2.get(1).unwrap().amount, 300);

    let page3 = client.get_payments_by_agreement(&agreement_id, &5u32, &2u32);
    assert_eq!(page3.len(), 1u32);
    assert_eq!(page3.get(0).unwrap().amount, 400);
}

#[test]
fn test_get_payments_by_agreement_start_index_zero_returns_empty() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    record(&client, &env, 1, 1u32, &token, 100, &from, &to, 0);

    assert_eq!(
        client
            .get_payments_by_agreement(&1u128, &0u32, &10u32)
            .len(),
        0u32
    );
}

#[test]
fn test_get_payments_by_agreement_start_index_above_count_returns_empty() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    record(&client, &env, 1, 1u32, &token, 100, &from, &to, 0);

    assert_eq!(
        client
            .get_payments_by_agreement(&1u128, &2u32, &10u32)
            .len(),
        0u32
    );
}

#[test]
fn test_get_payments_by_agreement_empty_history_returns_empty() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    assert_eq!(
        client
            .get_payments_by_agreement(&1u128, &1u32, &10u32)
            .len(),
        0u32
    );
}

#[test]
fn test_get_payments_by_agreement_limit_capped_at_max_page_size() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let agreement_id = 1u128;
    let total = MAX_PAGE_SIZE + 10;

    for i in 0..total as u8 {
        record(
            &client,
            &env,
            agreement_id,
            i as u32,
            &token,
            i as i128,
            &from,
            &to,
            i as u64,
        );
    }

    let page = client.get_payments_by_agreement(&agreement_id, &1u32, &(MAX_PAGE_SIZE + 50));
    assert_eq!(
        page.len(),
        MAX_PAGE_SIZE,
        "limit must be capped at MAX_PAGE_SIZE"
    );
}

#[test]
fn test_get_payments_by_agreement_exact_boundary_read() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let agreement_id = 1u128;

    for i in 0..3u8 {
        record(
            &client,
            &env,
            agreement_id,
            i as u32,
            &token,
            i as i128,
            &from,
            &to,
            i as u64,
        );
    }

    // start_index=3 (the last valid position), limit=10 must return exactly 1 record.
    let result = client.get_payments_by_agreement(&agreement_id, &3u32, &10u32);
    assert_eq!(result.len(), 1u32);
    assert_eq!(result.get(0).unwrap().amount, 2i128);
}

// ─── Employer index ───────────────────────────────────────────────────────────

#[test]
fn test_employer_count_before_and_after() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    assert_eq!(client.get_employer_payment_count(&employer), 0u32);
    record(&client, &env, 1, 1u32, &token, 100, &employer, &employee, 0);
    assert_eq!(client.get_employer_payment_count(&employer), 1u32);
    record(&client, &env, 2, 2u32, &token, 200, &employer, &employee, 1);
    assert_eq!(client.get_employer_payment_count(&employer), 2u32);
}

#[test]
fn test_employer_indices_are_independent() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer_a = Address::generate(&env);
    let employer_b = Address::generate(&env);
    let employee = Address::generate(&env);

    record(&client, &env, 1, 1u32, &token, 10, &employer_a, &employee, 0);
    record(&client, &env, 1, 2u32, &token, 20, &employer_a, &employee, 1);
    record(&client, &env, 1, 3u32, &token, 30, &employer_b, &employee, 2);

    assert_eq!(client.get_employer_payment_count(&employer_a), 2u32);
    assert_eq!(client.get_employer_payment_count(&employer_b), 1u32);
}

#[test]
fn test_get_payments_by_employer_pagination() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    for i in 0..5u8 {
        record(
            &client,
            &env,
            1,
            i as u32,
            &token,
            i as i128 * 10,
            &employer,
            &employee,
            i as u64,
        );
    }

    let page1 = client.get_payments_by_employer(&employer, &1u32, &2u32);
    assert_eq!(page1.len(), 2u32);

    let page2 = client.get_payments_by_employer(&employer, &3u32, &2u32);
    assert_eq!(page2.len(), 2u32);

    let page3 = client.get_payments_by_employer(&employer, &5u32, &2u32);
    assert_eq!(page3.len(), 1u32);
}

#[test]
fn test_get_payments_by_employer_start_index_zero_returns_empty() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    record(&client, &env, 1, 1u32, &token, 1, &from, &to, 0);

    assert_eq!(
        client.get_payments_by_employer(&from, &0u32, &10u32).len(),
        0u32
    );
}

#[test]
fn test_get_payments_by_employer_start_index_above_count_returns_empty() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    record(&client, &env, 1, 1u32, &token, 1, &from, &to, 0);

    assert_eq!(
        client.get_payments_by_employer(&from, &2u32, &10u32).len(),
        0u32
    );
}

#[test]
fn test_get_payments_by_employer_empty_history_returns_empty() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let employer = Address::generate(&env);
    assert_eq!(
        client
            .get_payments_by_employer(&employer, &1u32, &10u32)
            .len(),
        0u32
    );
}

// ─── Employee index ───────────────────────────────────────────────────────────

#[test]
fn test_employee_count_before_and_after() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    assert_eq!(client.get_employee_payment_count(&employee), 0u32);
    record(&client, &env, 1, 1u32, &token, 100, &employer, &employee, 0);
    assert_eq!(client.get_employee_payment_count(&employee), 1u32);
    record(&client, &env, 2, 2u32, &token, 200, &employer, &employee, 1);
    assert_eq!(client.get_employee_payment_count(&employee), 2u32);
}

#[test]
fn test_employee_indices_are_independent() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee_a = Address::generate(&env);
    let employee_b = Address::generate(&env);

    record(&client, &env, 1, 1u32, &token, 10, &employer, &employee_a, 0);
    record(&client, &env, 1, 2u32, &token, 20, &employer, &employee_a, 1);
    record(&client, &env, 1, 3u32, &token, 30, &employer, &employee_b, 2);

    assert_eq!(client.get_employee_payment_count(&employee_a), 2u32);
    assert_eq!(client.get_employee_payment_count(&employee_b), 1u32);
}

#[test]
fn test_get_payments_by_employee_pagination() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    for i in 0..5u8 {
        record(
            &client,
            &env,
            1,
            i as u32,
            &token,
            i as i128 * 10,
            &employer,
            &employee,
            i as u64,
        );
    }

    let page1 = client.get_payments_by_employee(&employee, &1u32, &3u32);
    assert_eq!(page1.len(), 3u32);

    let page2 = client.get_payments_by_employee(&employee, &4u32, &3u32);
    assert_eq!(page2.len(), 2u32);
}

#[test]
fn test_get_payments_by_employee_start_index_zero_returns_empty() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    record(&client, &env, 1, 1u32, &token, 1, &from, &to, 0);

    assert_eq!(
        client.get_payments_by_employee(&to, &0u32, &10u32).len(),
        0u32
    );
}

#[test]
fn test_get_payments_by_employee_start_index_above_count_returns_empty() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    record(&client, &env, 1, 1u32, &token, 1, &from, &to, 0);

    assert_eq!(
        client.get_payments_by_employee(&to, &2u32, &10u32).len(),
        0u32
    );
}

#[test]
fn test_get_payments_by_employee_empty_history_returns_empty() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let employee = Address::generate(&env);
    assert_eq!(
        client
            .get_payments_by_employee(&employee, &1u32, &10u32)
            .len(),
        0u32
    );
}

// ─── Cross-index consistency ──────────────────────────────────────────────────

#[test]
fn test_same_payment_visible_in_all_five_query_paths() {
    // Verifies that get_payment_by_hash, get_payment_by_id, and the three
    // sequential indices all return the exact same record for a given payment.
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let agreement_id = 55u128;
    let amount = 1_234i128;
    let hash = make_hash(&env, 0x55);

    let payment_id = client.record_payment(
        &agreement_id,
        &hash,
        &token,
        &amount,
        &employer,
        &employee,
        &9_999u64,
    );

    let by_hash = client
        .get_payment_by_hash(&hash)
        .expect("must exist by hash");
    let by_id = client
        .get_payment_by_id(&payment_id)
        .expect("must exist by id");
    let by_agg = client
        .get_payments_by_agreement(&agreement_id, &1u32, &1u32)
        .get(0)
        .unwrap();
    let by_empr = client
        .get_payments_by_employer(&employer, &1u32, &1u32)
        .get(0)
        .unwrap();
    let by_empe = client
        .get_payments_by_employee(&employee, &1u32, &1u32)
        .get(0)
        .unwrap();

    assert_eq!(by_hash, by_id);
    assert_eq!(by_id, by_agg);
    assert_eq!(by_agg, by_empr);
    assert_eq!(by_empr, by_empe);
    assert_eq!(by_id.payment_hash, hash);
    assert_eq!(by_id.amount, amount);
}

// ─── Security ─────────────────────────────────────────────────────────────────

#[test]
fn test_records_are_immutable_after_recording() {
    // Confirms that existing records are unchanged after more payments are added.
    // There is no overwrite path; this test validates that property at runtime.
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let pid = record(&client, &env, 1, 1u32, &token, 500, &from, &to, 1_000);

    // Add more payments after the first one.
    for i in 2..7u8 {
        record(&client, &env, 2, i as u32, &token, 9_999, &from, &to, 2_000);
    }

    let rec = client.get_payment_by_id(&pid).unwrap();
    assert_eq!(rec.id, pid);
    assert_eq!(rec.amount, 500, "original record must be unchanged");
    assert_eq!(rec.agreement_id, 1u128);
    assert_eq!(rec.payment_hash, make_hash(&env, 1u32));

    // Also verify via hash lookup.
    let by_hash = client.get_payment_by_hash(&make_hash(&env, 1u32)).unwrap();
    assert_eq!(by_hash.amount, 500);
}

#[test]
fn test_index_counts_only_increase() {
    // Validates the no-pruning guarantee: counts can only grow. A decrement
    // would allow entries to "fall off" the pagination range, effectively
    // pruning history without removing the underlying records.
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let agreement_id = 1u128;

    for i in 0..5u8 {
        record(
            &client,
            &env,
            agreement_id,
            i as u32,
            &token,
            i as i128,
            &from,
            &to,
            i as u64,
        );
        assert_eq!(
            client.get_agreement_payment_count(&agreement_id),
            (i + 1) as u32,
            "count must equal number of insertions after {} insertions",
            i + 1
        );
    }
}

// ─── Large history / boundary reads ──────────────────────────────────────────

#[test]
fn test_large_history_boundary_reads() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let agreement_id = 1u128;
    let total: u32 = 20;

    for i in 0..total as u8 {
        record(
            &client,
            &env,
            agreement_id,
            i as u32,
            &token,
            i as i128,
            &from,
            &to,
            i as u64,
        );
    }

    assert_eq!(client.get_agreement_payment_count(&agreement_id), total);

    // Read exactly the last record.
    let last = client.get_payments_by_agreement(&agreement_id, &total, &1u32);
    assert_eq!(last.len(), 1u32);
    assert_eq!(last.get(0).unwrap().amount, (total - 1) as i128);

    // One past the end must be empty.
    let oob = client.get_payments_by_agreement(&agreement_id, &(total + 1), &1u32);
    assert_eq!(oob.len(), 0u32);

    // Full page read.
    let full = client.get_payments_by_agreement(&agreement_id, &1u32, &total);
    assert_eq!(full.len(), total);
}

#[test]
fn test_multiple_agreements_large_history_independent() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // 10 payments under agreement 1, 5 under agreement 2.
    for i in 0..10u8 {
        record(&client, &env, 1, i as u32, &token, i as i128, &from, &to, i as u64);
    }
    for i in 10..15u8 {
        record(
            &client,
            &env,
            2,
            i as u32,
            &token,
            i as i128 * 10,
            &from,
            &to,
            (100 + i) as u64,
        );
    }

    assert_eq!(client.get_agreement_payment_count(&1u128), 10u32);
    assert_eq!(client.get_agreement_payment_count(&2u128), 5u32);
    assert_eq!(client.get_global_payment_count(), 15u128);

    let agg2 = client.get_payments_by_agreement(&2u128, &1u32, &10u32);
    assert_eq!(agg2.len(), 5u32);
    assert_eq!(agg2.get(0).unwrap().amount, 100i128);
    assert_eq!(agg2.get(4).unwrap().amount, 140i128);
}

// ─── Event-based reconciliation fixtures ─────────────────────────────────────

#[test]
fn test_event_based_reconciliation_across_payment_sources() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let scheduler_employer = Address::generate(&env);
    let escrow_manager = Address::generate(&env);
    let bonus_employer = Address::generate(&env);
    let expense_payer = Address::generate(&env);
    let worker = Address::generate(&env);

    let fixtures = [
        ReconciliationFixture {
            source: ReconciliationSource::SchedulerExecution,
            source_event_id: 11,
            agreement_id: 7001,
            hash_seed: 0x11u32,
            token: token.clone(),
            amount: 500,
            from: scheduler_employer.clone(),
            to: worker.clone(),
            timestamp: 1_001,
        },
        ReconciliationFixture {
            source: ReconciliationSource::EscrowRelease,
            source_event_id: 22,
            agreement_id: 7002,
            hash_seed: 0x22u32,
            token: token.clone(),
            amount: 800,
            from: escrow_manager.clone(),
            to: worker.clone(),
            timestamp: 1_002,
        },
        ReconciliationFixture {
            source: ReconciliationSource::BonusPayment,
            source_event_id: 33,
            agreement_id: 7003,
            hash_seed: 0x33u32,
            token: token.clone(),
            amount: 300,
            from: bonus_employer.clone(),
            to: worker.clone(),
            timestamp: 1_003,
        },
        ReconciliationFixture {
            source: ReconciliationSource::ExpenseReimbursement,
            source_event_id: 44,
            agreement_id: 7004,
            hash_seed: 0x44u32,
            token: token.clone(),
            amount: 650,
            from: expense_payer.clone(),
            to: worker.clone(),
            timestamp: 1_004,
        },
    ];

    for (idx, fixture) in fixtures.iter().enumerate() {
        assert!(
            !fixture.source.topic().is_empty(),
            "source topic must be defined"
        );
        assert!(
            fixture.source_event_id > 0,
            "source event id must be non-zero"
        );

        let id = reconcile_fixture(&client, &env, fixture);
        assert_eq!(id, (idx as u128) + 1);

        let hash = make_hash(&env, fixture.hash_seed);
        let rec = client
            .get_payment_by_hash(&hash)
            .expect("record must be queryable by hash");

        assert_eq!(rec.id, id);
        assert_eq!(rec.agreement_id, fixture.agreement_id);
        assert_eq!(rec.token, fixture.token);
        assert_eq!(rec.amount, fixture.amount);
        assert_eq!(rec.from, fixture.from);
        assert_eq!(rec.to, fixture.to);
        assert_eq!(rec.timestamp, fixture.timestamp);
    }

    assert_eq!(client.get_global_payment_count(), fixtures.len() as u128);
}

#[test]
fn test_reconciliation_out_of_order_events_are_stable() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let agreement_id = 9001u128;

    // Simulate indexer ingestion arriving out of chronological timestamp order.
    let newer_hash = make_hash(&env, 0x90u32);
    let older_hash = make_hash(&env, 0x91u32);

    let id1 = client.record_payment(
        &agreement_id,
        &newer_hash,
        &token,
        &1_000i128,
        &employer,
        &employee,
        &2_000u64,
    );
    let id2 = client.record_payment(
        &agreement_id,
        &older_hash,
        &token,
        &900i128,
        &employer,
        &employee,
        &1_000u64,
    );

    assert_eq!(id1, 1u128);
    assert_eq!(id2, 2u128);

    let newer = client
        .get_payment_by_hash(&newer_hash)
        .expect("newer payment must exist");
    let older = client
        .get_payment_by_hash(&older_hash)
        .expect("older payment must exist");

    assert_eq!(newer.id, id1);
    assert_eq!(older.id, id2);
    assert_eq!(newer.timestamp, 2_000u64);
    assert_eq!(older.timestamp, 1_000u64);

    let page = client.get_payments_by_agreement(&agreement_id, &1u32, &10u32);
    assert_eq!(page.len(), 2u32);
    assert_eq!(page.get(0).unwrap().id, id1);
    assert_eq!(page.get(1).unwrap().id, id2);
}

// ─── Gas benchmark: get_payments_by_employee at large history sizes ─────────────

#[test]
fn benchmark_get_payments_by_employee_scaling() {
    /// Benchmark helper that records N payments for a single employee and measures
    /// the read cost of retrieving them via get_payments_by_employee.
    ///
    /// @notice This test measures how read cost scales as an employee accumulates
    /// hundreds or thousands of recorded payments. The results inform pagination
    /// policy and response-size limits.
    ///
    /// @dev Uses env.cost_estimate() to capture CPU instruction counts before and
    /// after the read operation. The cost is measured for retrieving the full history
    /// (up to MAX_PAGE_SIZE) to simulate worst-case read scenarios.
    ///
    /// @param num_payments Number of payments to record for the employee.
    /// @return Tuple of (payment_count, cpu_insns) for the read operation.
    fn benchmark_employee_read(
        env: &Env,
        client: &PaymentHistoryContractClient<'_>,
        employee: &Address,
        num_payments: u32,
    ) -> (u32, u64) {
        let token = Address::generate(env);
        let employer = Address::generate(env);
        let agreement_id = 1u128;

        // Record N payments for the same employee
        for i in 0..num_payments {
            record(
                client,
                env,
                agreement_id,
                i,
                &token,
                i as i128 * 100,
                &employer,
                employee,
                i as u64 * 1000,
            );
        }

        let count = client.get_employee_payment_count(employee);
        assert_eq!(count, num_payments, "count must match recorded payments");

        // Reset cost budget to get clean measurement
        env.cost_estimate().budget().reset_default();

        // Measure cost of reading the full page (up to MAX_PAGE_SIZE)
        let limit = count.min(MAX_PAGE_SIZE);
        let _result = client.get_payments_by_employee(employee, &1u32, &limit);
        let cpu_insns = env.cost_estimate().budget().cpu_instruction_cost();

        (count, cpu_insns)
    }

    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let employee = Address::generate(&env);

    // Benchmark at 10 payments
    let (count_10, cost_10) = benchmark_employee_read(&env, &client, &employee, 10);
    println!(
        "get_payments_by_employee benchmark: {} payments = {} CPU instructions",
        count_10, cost_10
    );

    // Benchmark at 100 payments (equals MAX_PAGE_SIZE)
    let (count_100, cost_100) = benchmark_employee_read(&env, &client, &employee, 100);
    println!(
        "get_payments_by_employee benchmark: {} payments = {} CPU instructions",
        count_100, cost_100
    );

    // Benchmark at 1000 payments (well beyond MAX_PAGE_SIZE)
    let (count_1000, cost_1000) = benchmark_employee_read(&env, &client, &employee, 1000);
    println!(
        "get_payments_by_employee benchmark: {} payments = {} CPU instructions",
        count_1000, cost_1000
    );

    // Validate that costs are reasonable and scaling is predictable
    // Cost should increase with history size, but not linearly due to pagination cap
    assert!(cost_10 > 0, "cost must be positive");
    assert!(cost_100 > cost_10, "cost for 100 payments should exceed cost for 10");
    assert!(cost_1000 > cost_100, "cost for 1000 payments should exceed cost for 100");

    // The cost difference between 100 and 1000 should be bounded because
    // MAX_PAGE_SIZE caps the actual number of records read (100 vs 100)
    // The difference comes from index traversal overhead, not record deserialization
    let cost_ratio = cost_1000 as f64 / cost_100 as f64;
    println!(
        "Cost ratio (1000/100 payments): {:.2}x",
        cost_ratio
    );
    assert!(
        cost_ratio < 10.0,
        "Cost ratio should be < 10x due to pagination cap; got {:.2}x",
        cost_ratio
    );
}

// ─── Index-consistency: get_payment_by_hash vs get_payment_by_id (#912) ──────
//
// These tests directly satisfy the requirements of issue #912:
//
//  Req 1 — get_payment_by_hash and get_payment_by_id resolve to the EXACT same
//           record for a given payment, verified field-by-field.
//  Req 2 — An unknown hash (never recorded) returns None (not-found), not a
//           wrong record. A hash that belongs to payment A never resolves to
//           payment B.
//
// All tests use the established fixture helpers (make_hash, record,
// create_env, register_contract, initialize_contract) so they slot naturally
// into the existing CI suite and pass `cargo fmt --all -- --check`.

/// Req 1 (core): record a single payment and assert that every field returned
/// by get_payment_by_hash matches the corresponding field returned by
/// get_payment_by_id — not just structural equality but an explicit field-by-
/// field comparison so a future partial-record regression surfaces immediately.
#[test]
fn test_index_consistency_hash_and_id_return_identical_fields() {
    let env = create_env();
    let (_contract_addr, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let agreement_id = 1001u128;
    let amount = 4_200i128;
    let timestamp = 1_700_000_001u64;
    let hash = make_hash(&env, 0xA1u32);

    let payment_id = client.record_payment(
        &agreement_id,
        &hash,
        &token,
        &amount,
        &employer,
        &employee,
        &timestamp,
    );

    let by_id = client
        .get_payment_by_id(&payment_id)
        .expect("get_payment_by_id must return Some for a recorded payment");
    let by_hash = client
        .get_payment_by_hash(&hash)
        .expect("get_payment_by_hash must return Some for a recorded payment");

    // Structural equality — both paths dereference the same storage slot.
    assert_eq!(
        by_id, by_hash,
        "get_payment_by_id and get_payment_by_hash must return the same record"
    );

    // Field-by-field assertions so any partial mismatch is immediately obvious.
    assert_eq!(by_hash.id, payment_id, "id field must match");
    assert_eq!(by_hash.agreement_id, agreement_id, "agreement_id field must match");
    assert_eq!(by_hash.payment_hash, hash, "payment_hash field must match");
    assert_eq!(by_hash.token, token, "token field must match");
    assert_eq!(by_hash.amount, amount, "amount field must match");
    assert_eq!(by_hash.from, employer, "from field must match");
    assert_eq!(by_hash.to, employee, "to field must match");
    assert_eq!(by_hash.timestamp, timestamp, "timestamp field must match");

    // Mirror: same assertions via the by_id path.
    assert_eq!(by_id.id, payment_id);
    assert_eq!(by_id.agreement_id, agreement_id);
    assert_eq!(by_id.payment_hash, hash);
    assert_eq!(by_id.token, token);
    assert_eq!(by_id.amount, amount);
    assert_eq!(by_id.from, employer);
    assert_eq!(by_id.to, employee);
    assert_eq!(by_id.timestamp, timestamp);
}

/// Req 2 (core): a hash that was never recorded must return None from
/// get_payment_by_hash. The result must be definitively None — not a wrong
/// record, not a default record, not a panic.
#[test]
fn test_index_consistency_unknown_hash_returns_none_not_wrong_record() {
    let env = create_env();
    let (_contract_addr, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // Record a real payment so the contract's storage is non-empty.
    let real_hash = make_hash(&env, 0x01u32);
    let real_id = client.record_payment(
        &1u128,
        &real_hash,
        &token,
        &100i128,
        &from,
        &to,
        &1_000u64,
    );

    // A completely different hash that was never recorded.
    let unknown_hash = make_hash(&env, 0xFFu32);

    let result = client.get_payment_by_hash(&unknown_hash);

    // Must be None — not the real record, not any record.
    assert!(
        result.is_none(),
        "get_payment_by_hash for an unknown hash must return None, not a record"
    );

    // Defensive: ensure the real payment is still reachable and the unknown
    // hash did not corrupt or displace it.
    let real_record = client
        .get_payment_by_id(&real_id)
        .expect("real payment must still be retrievable by id after unknown-hash lookup");
    assert_eq!(
        real_record.payment_hash, real_hash,
        "real record must not have been replaced by the unknown-hash lookup"
    );
}

/// Req 2 (isolation): after recording multiple payments with distinct hashes,
/// each hash resolves only to its own record. Hash A never resolves to
/// payment B and vice versa.
#[test]
fn test_index_consistency_each_hash_resolves_only_to_its_own_record() {
    let env = create_env();
    let (_contract_addr, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let hash_a = make_hash(&env, 0x10u32);
    let hash_b = make_hash(&env, 0x20u32);
    let hash_c = make_hash(&env, 0x30u32);

    let id_a =
        client.record_payment(&1u128, &hash_a, &token, &111i128, &employer, &employee, &1u64);
    let id_b =
        client.record_payment(&1u128, &hash_b, &token, &222i128, &employer, &employee, &2u64);
    let id_c =
        client.record_payment(&2u128, &hash_c, &token, &333i128, &employer, &employee, &3u64);

    // Each hash must resolve to the correct record — not a neighbour's record.
    let rec_a = client.get_payment_by_hash(&hash_a).expect("hash_a must resolve");
    let rec_b = client.get_payment_by_hash(&hash_b).expect("hash_b must resolve");
    let rec_c = client.get_payment_by_hash(&hash_c).expect("hash_c must resolve");

    assert_eq!(rec_a.id, id_a, "hash_a must resolve to payment A");
    assert_eq!(rec_a.amount, 111i128);
    assert_ne!(rec_a.id, id_b, "hash_a must NOT resolve to payment B");
    assert_ne!(rec_a.id, id_c, "hash_a must NOT resolve to payment C");

    assert_eq!(rec_b.id, id_b, "hash_b must resolve to payment B");
    assert_eq!(rec_b.amount, 222i128);
    assert_ne!(rec_b.id, id_a, "hash_b must NOT resolve to payment A");
    assert_ne!(rec_b.id, id_c, "hash_b must NOT resolve to payment C");

    assert_eq!(rec_c.id, id_c, "hash_c must resolve to payment C");
    assert_eq!(rec_c.amount, 333i128);
    assert_ne!(rec_c.id, id_a, "hash_c must NOT resolve to payment A");
    assert_ne!(rec_c.id, id_b, "hash_c must NOT resolve to payment B");

    // Cross-verify: get_payment_by_id must match get_payment_by_hash for each pair.
    assert_eq!(client.get_payment_by_id(&id_a).unwrap(), rec_a);
    assert_eq!(client.get_payment_by_id(&id_b).unwrap(), rec_b);
    assert_eq!(client.get_payment_by_id(&id_c).unwrap(), rec_c);
}

/// After recording N payments, every (hash, id) pair is mutually consistent:
/// get_payment_by_hash(payment.payment_hash) == get_payment_by_id(payment.id).
/// This exercises the dual-index across a batch of payments.
#[test]
fn test_index_consistency_batch_all_pairs_agree() {
    let env = create_env();
    let (_contract_addr, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    const BATCH: u32 = 8;
    let mut recorded_ids: [u128; BATCH as usize] = [0u128; BATCH as usize];
    let mut recorded_hashes: [u32; BATCH as usize] = [0u32; BATCH as usize];

    for i in 0..BATCH {
        let seed = i + 1; // seeds 1..=8, all distinct
        let hash = make_hash(&env, seed);
        let id = client.record_payment(
            &(i as u128 + 1),
            &hash,
            &token,
            &(i as i128 * 500 + 100),
            &employer,
            &employee,
            &(i as u64 * 10),
        );
        recorded_ids[i as usize] = id;
        recorded_hashes[i as usize] = seed;
    }

    // For every recorded payment: both lookup paths must agree on all fields.
    for i in 0..BATCH as usize {
        let hash = make_hash(&env, recorded_hashes[i]);
        let id = recorded_ids[i];

        let by_id = client
            .get_payment_by_id(&id)
            .expect("get_payment_by_id must return Some");
        let by_hash = client
            .get_payment_by_hash(&hash)
            .expect("get_payment_by_hash must return Some");

        assert_eq!(
            by_id, by_hash,
            "payment {} (id={}) must be identical via both lookup paths",
            i, id
        );

        // The record's stored hash must round-trip through both indices.
        assert_eq!(by_id.payment_hash, hash);
        assert_eq!(by_hash.id, id);
    }
}

/// Idempotency + index consistency: replaying the same hash does not create
/// a second record or break the reverse index. Both lookup paths must still
/// return the original record, unchanged.
#[test]
fn test_index_consistency_duplicate_hash_preserves_original_record() {
    let env = create_env();
    let (_contract_addr, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let agreement_id = 5u128;
    let amount = 7_777i128;
    let timestamp = 42_000u64;
    let hash = make_hash(&env, 0xBBu32);

    // First recording.
    let id_first = client.record_payment(
        &agreement_id,
        &hash,
        &token,
        &amount,
        &employer,
        &employee,
        &timestamp,
    );

    // Replay the same hash — must be idempotent.
    let id_replay = client.record_payment(
        &agreement_id,
        &hash,
        &token,
        &amount,
        &employer,
        &employee,
        &timestamp,
    );

    assert_eq!(id_first, id_replay, "duplicate hash must return the original ID");

    // Both indices must still resolve to the same single record.
    let by_id = client
        .get_payment_by_id(&id_first)
        .expect("original record must still exist by id");
    let by_hash = client
        .get_payment_by_hash(&hash)
        .expect("original record must still exist by hash");

    assert_eq!(by_id, by_hash, "both paths must still agree after duplicate recording");
    assert_eq!(by_id.amount, amount, "amount must be unchanged after duplicate");
    assert_eq!(by_id.id, id_first, "id must be unchanged after duplicate");

    // Only one record in storage — global count must be 1.
    assert_eq!(
        client.get_global_payment_count(),
        1u128,
        "duplicate recording must not increment the global count"
    );
}

/// The hash stored inside the record itself must match the key used to look it
/// up. This guards against a future refactor that could write the record under
/// one hash while indexing it under another.
#[test]
fn test_index_consistency_stored_hash_matches_lookup_key() {
    let env = create_env();
    let (_contract_addr, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let lookup_hash = make_hash(&env, 0xCCu32);
    let payment_id = client.record_payment(
        &10u128,
        &lookup_hash,
        &token,
        &250i128,
        &from,
        &to,
        &5_000u64,
    );

    let by_hash = client
        .get_payment_by_hash(&lookup_hash)
        .expect("must exist by hash");
    let by_id = client
        .get_payment_by_id(&payment_id)
        .expect("must exist by id");

    // The hash field inside the record must equal the key we queried with.
    assert_eq!(
        by_hash.payment_hash, lookup_hash,
        "the payment_hash field inside the record must equal the lookup key"
    );
    assert_eq!(
        by_id.payment_hash, lookup_hash,
        "the payment_hash field must be the same whether retrieved by id or by hash"
    );

    // And both records must agree on every field.
    assert_eq!(by_hash, by_id);
}

/// Req 2 (post-recording unknown): even after several real payments exist in
/// storage, a hash that was never submitted still returns None — the presence
/// of other records in the map must not cause a false positive.
#[test]
fn test_index_consistency_unknown_hash_returns_none_with_populated_storage() {
    let env = create_env();
    let (_contract_addr, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // Populate storage with several real payments.
    for seed in 1u32..=5 {
        let hash = make_hash(&env, seed);
        client.record_payment(
            &(seed as u128),
            &hash,
            &token,
            &(seed as i128 * 10),
            &from,
            &to,
            &(seed as u64),
        );
    }

    assert_eq!(client.get_global_payment_count(), 5u128);

    // A hash that was never recorded must still return None.
    let never_recorded = make_hash(&env, 0xDEAD_BEEFu32);
    let result = client.get_payment_by_hash(&never_recorded);
    assert!(
        result.is_none(),
        "unknown hash must return None even when storage contains other payments"
    );

    // Similarly, an ID beyond the recorded range must return None.
    assert!(
        client.get_payment_by_id(&6u128).is_none(),
        "id beyond recorded range must return None"
    );
    assert!(
        client.get_payment_by_id(&999u128).is_none(),
        "large unassigned id must return None"
    );
}

// ─── Date-range filtering ─────────────────────────────────────────────────────
//
// Tests for get_agreement_payments_in_range,
//         get_employer_payments_in_range,
//         get_employee_payments_in_range.
//
// Coverage:
//   Range filtering: from_ts only / to_ts only / both
//   Boundary inclusion: exact timestamp matches at from_ts and to_ts
//   Empty result: no records in range
//   Entire history returned: no-range call matches base query
//   Validation: from_ts > to_ts panics with ERR_INVALID_RANGE
//   Pagination: start_index / limit operate on the filtered set
//   Multi-page: multiple pages within a filtered range
//   Composition: existing index filters still work alongside range filters
//   Single-record range: exactly one match
//   All three index variants: agreement, employer, employee

// ── Helpers (shared with the range tests) ────────────────────────────────────

/// Record N payments with sequential timestamps (ts = 1000 * (i+1))
/// and return the common token/employer/employee addresses.
fn setup_range_payments(
    client: &PaymentHistoryContractClient<'_>,
    env: &Env,
    agreement_id: u128,
    n: u8,
) -> (Address, Address, Address) {
    let token = Address::generate(env);
    let employer = Address::generate(env);
    let employee = Address::generate(env);
    for i in 0..n {
        record(
            client,
            env,
            agreement_id,
            100 + i as u32,
            &token,
            (i as i128 + 1) * 10,
            &employer,
            &employee,
            (i as u64 + 1) * 1_000,
        );
    }
    (token, employer, employee)
}

// ── Backward-compatibility: no-range == base query ────────────────────────────

#[test]
fn test_range_agreement_no_range_matches_base_query() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let agreement_id = 1u128;
    setup_range_payments(&client, &env, agreement_id, 5);

    let base = client.get_payments_by_agreement(&agreement_id, &1u32, &10u32);
    let ranged =
        client.get_agreement_payments_in_range(&agreement_id, &1u32, &10u32, &None, &None);
    assert_eq!(base.len(), ranged.len());
    for i in 0..base.len() {
        assert_eq!(base.get(i).unwrap(), ranged.get(i).unwrap());
    }
}

#[test]
fn test_range_employer_no_range_matches_base_query() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let agreement_id = 1u128;
    let (_, employer, _) = setup_range_payments(&client, &env, agreement_id, 5);

    let base = client.get_payments_by_employer(&employer, &1u32, &10u32);
    let ranged =
        client.get_employer_payments_in_range(&employer, &1u32, &10u32, &None, &None);
    assert_eq!(base.len(), ranged.len());
}

#[test]
fn test_range_employee_no_range_matches_base_query() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let agreement_id = 1u128;
    let (_, _, employee) = setup_range_payments(&client, &env, agreement_id, 5);

    let base = client.get_payments_by_employee(&employee, &1u32, &10u32);
    let ranged =
        client.get_employee_payments_in_range(&employee, &1u32, &10u32, &None, &None);
    assert_eq!(base.len(), ranged.len());
}

// ── from_ts only ──────────────────────────────────────────────────────────────

#[test]
fn test_range_agreement_from_ts_only() {
    // 5 records with timestamps 1000, 2000, 3000, 4000, 5000
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let agreement_id = 1u128;
    setup_range_payments(&client, &env, agreement_id, 5);

    // from_ts = 3000 -> should return records with ts >= 3000 (3000, 4000, 5000)
    let result = client.get_agreement_payments_in_range(
        &agreement_id,
        &1u32,
        &10u32,
        &Some(3_000u64),
        &None,
    );
    assert_eq!(result.len(), 3u32);
    assert_eq!(result.get(0).unwrap().timestamp, 3_000u64);
    assert_eq!(result.get(2).unwrap().timestamp, 5_000u64);
}

#[test]
fn test_range_employer_from_ts_only() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let (_, employer, _) = setup_range_payments(&client, &env, 1u128, 5);

    let result = client.get_employer_payments_in_range(
        &employer,
        &1u32,
        &10u32,
        &Some(4_000u64),
        &None,
    );
    assert_eq!(result.len(), 2u32); // ts 4000 and 5000
}

#[test]
fn test_range_employee_from_ts_only() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let (_, _, employee) = setup_range_payments(&client, &env, 1u128, 5);

    let result = client.get_employee_payments_in_range(
        &employee,
        &1u32,
        &10u32,
        &Some(2_000u64),
        &None,
    );
    assert_eq!(result.len(), 4u32); // ts 2000..5000
}

// ── to_ts only ────────────────────────────────────────────────────────────────

#[test]
fn test_range_agreement_to_ts_only() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let agreement_id = 2u128;
    setup_range_payments(&client, &env, agreement_id, 5);

    // to_ts = 2000 -> returns ts 1000 and 2000
    let result = client.get_agreement_payments_in_range(
        &agreement_id,
        &1u32,
        &10u32,
        &None,
        &Some(2_000u64),
    );
    assert_eq!(result.len(), 2u32);
    assert_eq!(result.get(0).unwrap().timestamp, 1_000u64);
    assert_eq!(result.get(1).unwrap().timestamp, 2_000u64);
}

#[test]
fn test_range_employer_to_ts_only() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let (_, employer, _) = setup_range_payments(&client, &env, 1u128, 5);

    let result =
        client.get_employer_payments_in_range(&employer, &1u32, &10u32, &None, &Some(3_000u64));
    assert_eq!(result.len(), 3u32);
}

#[test]
fn test_range_employee_to_ts_only() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let (_, _, employee) = setup_range_payments(&client, &env, 1u128, 5);

    let result =
        client.get_employee_payments_in_range(&employee, &1u32, &10u32, &None, &Some(1_000u64));
    assert_eq!(result.len(), 1u32);
    assert_eq!(result.get(0).unwrap().timestamp, 1_000u64);
}

// ── Both from_ts and to_ts ────────────────────────────────────────────────────

#[test]
fn test_range_agreement_both_bounds() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let agreement_id = 3u128;
    setup_range_payments(&client, &env, agreement_id, 5);

    // [2000, 4000] -> timestamps 2000, 3000, 4000
    let result = client.get_agreement_payments_in_range(
        &agreement_id,
        &1u32,
        &10u32,
        &Some(2_000u64),
        &Some(4_000u64),
    );
    assert_eq!(result.len(), 3u32);
    assert_eq!(result.get(0).unwrap().timestamp, 2_000u64);
    assert_eq!(result.get(2).unwrap().timestamp, 4_000u64);
}

#[test]
fn test_range_employer_both_bounds() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let (_, employer, _) = setup_range_payments(&client, &env, 1u128, 5);

    let result = client.get_employer_payments_in_range(
        &employer,
        &1u32,
        &10u32,
        &Some(2_000u64),
        &Some(3_000u64),
    );
    assert_eq!(result.len(), 2u32);
}

#[test]
fn test_range_employee_both_bounds() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let (_, _, employee) = setup_range_payments(&client, &env, 1u128, 5);

    let result = client.get_employee_payments_in_range(
        &employee,
        &1u32,
        &10u32,
        &Some(3_000u64),
        &Some(5_000u64),
    );
    assert_eq!(result.len(), 3u32);
}

// ── Boundary inclusion (exact matches at from_ts and to_ts) ───────────────────

#[test]
fn test_range_boundary_inclusive_from_ts_exact() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let agreement_id = 10u128;
    setup_range_payments(&client, &env, agreement_id, 5);

    // from_ts == 3000 exactly — record at 3000 must be included
    let result = client.get_agreement_payments_in_range(
        &agreement_id,
        &1u32,
        &10u32,
        &Some(3_000u64),
        &Some(3_000u64),
    );
    assert_eq!(result.len(), 1u32, "exact timestamp must be included");
    assert_eq!(result.get(0).unwrap().timestamp, 3_000u64);
}

#[test]
fn test_range_boundary_inclusive_to_ts_exact() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let agreement_id = 11u128;
    setup_range_payments(&client, &env, agreement_id, 5);

    // to_ts == 1000 exactly — only the first record
    let result = client.get_agreement_payments_in_range(
        &agreement_id,
        &1u32,
        &10u32,
        &None,
        &Some(1_000u64),
    );
    assert_eq!(result.len(), 1u32);
    assert_eq!(result.get(0).unwrap().timestamp, 1_000u64);
}

// ── Empty result set ──────────────────────────────────────────────────────────

#[test]
fn test_range_empty_result_no_records_in_range() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let agreement_id = 20u128;
    setup_range_payments(&client, &env, agreement_id, 3); // ts: 1000, 2000, 3000

    // Range [9000, 9999] — no records exist there
    let result = client.get_agreement_payments_in_range(
        &agreement_id,
        &1u32,
        &10u32,
        &Some(9_000u64),
        &Some(9_999u64),
    );
    assert_eq!(result.len(), 0u32);
}

#[test]
fn test_range_empty_result_no_history_at_all() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let agreement_id = 21u128;

    let result = client.get_agreement_payments_in_range(
        &agreement_id,
        &1u32,
        &10u32,
        &Some(1_000u64),
        &Some(5_000u64),
    );
    assert_eq!(result.len(), 0u32);
}

// ── Validation: invalid range panics ─────────────────────────────────────────

#[test]
#[should_panic(expected = "InvalidRange: from_ts must be <= to_ts")]
fn test_range_agreement_invalid_range_panics() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    // from_ts > to_ts is invalid
    client.get_agreement_payments_in_range(
        &1u128,
        &1u32,
        &10u32,
        &Some(5_000u64),
        &Some(1_000u64),
    );
}

#[test]
#[should_panic(expected = "InvalidRange: from_ts must be <= to_ts")]
fn test_range_employer_invalid_range_panics() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let employer = Address::generate(&env);
    client.get_employer_payments_in_range(
        &employer,
        &1u32,
        &10u32,
        &Some(9_999u64),
        &Some(1u64),
    );
}

#[test]
#[should_panic(expected = "InvalidRange: from_ts must be <= to_ts")]
fn test_range_employee_invalid_range_panics() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let employee = Address::generate(&env);
    client.get_employee_payments_in_range(
        &employee,
        &1u32,
        &10u32,
        &Some(100u64),
        &Some(99u64),
    );
}

// from_ts == to_ts is NOT invalid (single-timestamp range)
#[test]
fn test_range_from_ts_equals_to_ts_is_valid() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let agreement_id = 30u128;
    setup_range_payments(&client, &env, agreement_id, 5);

    // from_ts == to_ts == 3000 must NOT panic and must return exactly 1 record
    let result = client.get_agreement_payments_in_range(
        &agreement_id,
        &1u32,
        &10u32,
        &Some(3_000u64),
        &Some(3_000u64),
    );
    assert_eq!(result.len(), 1u32);
}

// ── Pagination over filtered set ──────────────────────────────────────────────

#[test]
fn test_range_pagination_page1() {
    // 6 records ts: 1000..6000, filter [2000,6000] gives 5 records
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let agreement_id = 40u128;
    setup_range_payments(&client, &env, agreement_id, 6);

    // Page 1: start=1, limit=2 of filtered set [2000,3000,4000,5000,6000]
    let page1 = client.get_agreement_payments_in_range(
        &agreement_id,
        &1u32,
        &2u32,
        &Some(2_000u64),
        &None,
    );
    assert_eq!(page1.len(), 2u32);
    assert_eq!(page1.get(0).unwrap().timestamp, 2_000u64);
    assert_eq!(page1.get(1).unwrap().timestamp, 3_000u64);
}

#[test]
fn test_range_pagination_page2() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let agreement_id = 41u128;
    setup_range_payments(&client, &env, agreement_id, 6);

    // filtered set: ts 2000,3000,4000,5000,6000 (5 items)
    // Page 2: start=3, limit=2 => items at positions 3,4 => ts 4000, 5000
    let page2 = client.get_agreement_payments_in_range(
        &agreement_id,
        &3u32,
        &2u32,
        &Some(2_000u64),
        &None,
    );
    assert_eq!(page2.len(), 2u32);
    assert_eq!(page2.get(0).unwrap().timestamp, 4_000u64);
    assert_eq!(page2.get(1).unwrap().timestamp, 5_000u64);
}

#[test]
fn test_range_pagination_last_partial_page() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let agreement_id = 42u128;
    setup_range_payments(&client, &env, agreement_id, 6);

    // filtered 5 items, page3: start=5, limit=2 => only 1 item remains (ts 6000)
    let page3 = client.get_agreement_payments_in_range(
        &agreement_id,
        &5u32,
        &2u32,
        &Some(2_000u64),
        &None,
    );
    assert_eq!(page3.len(), 1u32);
    assert_eq!(page3.get(0).unwrap().timestamp, 6_000u64);
}

#[test]
fn test_range_pagination_start_index_above_filtered_count_returns_empty() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let agreement_id = 43u128;
    setup_range_payments(&client, &env, agreement_id, 3); // filtered count <= 3

    // start_index=10 is beyond any possible filtered result
    let result = client.get_agreement_payments_in_range(
        &agreement_id,
        &10u32,
        &5u32,
        &Some(1_000u64),
        &Some(3_000u64),
    );
    assert_eq!(result.len(), 0u32);
}

#[test]
fn test_range_pagination_limit_capped_at_max_page_size() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let agreement_id = 50u128;

    // Insert MAX_PAGE_SIZE + 10 records all with timestamp 1000
    let total = MAX_PAGE_SIZE + 10;
    for i in 0..total {
        record(
            &client,
            &env,
            agreement_id,
            200 + i,
            &token,
            i as i128,
            &employer,
            &employee,
            1_000u64,
        );
    }

    let result = client.get_agreement_payments_in_range(
        &agreement_id,
        &1u32,
        &(MAX_PAGE_SIZE + 50),
        &Some(1_000u64),
        &Some(1_000u64),
    );
    assert_eq!(result.len(), MAX_PAGE_SIZE, "limit must be capped at MAX_PAGE_SIZE");
}

// ── Single-record range ───────────────────────────────────────────────────────

#[test]
fn test_range_single_record_match() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let agreement_id = 60u128;
    setup_range_payments(&client, &env, agreement_id, 5);

    // Exact match on ts = 4000
    let result = client.get_agreement_payments_in_range(
        &agreement_id,
        &1u32,
        &10u32,
        &Some(4_000u64),
        &Some(4_000u64),
    );
    assert_eq!(result.len(), 1u32);
    assert_eq!(result.get(0).unwrap().timestamp, 4_000u64);
}

// ── Existing index filters still compose (multi-agreement isolation) ──────────

#[test]
fn test_range_agreement_isolation_unaffected_by_other_agreements() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    // Agreement 70: ts 1000,2000,3000
    setup_range_payments(&client, &env, 70u128, 3);
    // Agreement 71: ts 1000,2000,3000,4000
    setup_range_payments(&client, &env, 71u128, 4);

    // Range filter on agreement 70 must not be contaminated by agreement 71
    let result = client.get_agreement_payments_in_range(
        &70u128,
        &1u32,
        &10u32,
        &Some(2_000u64),
        &Some(3_000u64),
    );
    assert_eq!(result.len(), 2u32);
    for i in 0..result.len() {
        assert_eq!(result.get(i).unwrap().agreement_id, 70u128);
    }
}

// ── Entire history returned when bounds encompass all records ─────────────────

#[test]
fn test_range_entire_history_when_bounds_are_very_wide() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);
    let agreement_id = 80u128;
    setup_range_payments(&client, &env, agreement_id, 5);

    let result = client.get_agreement_payments_in_range(
        &agreement_id,
        &1u32,
        &10u32,
        &Some(0u64),
        &Some(u64::MAX),
    );
    assert_eq!(result.len(), 5u32);
}
