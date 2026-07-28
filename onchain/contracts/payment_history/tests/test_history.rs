//! Comprehensive tests for the PaymentHistory contract.
//!
//! ## Coverage targets
//!
//! * Initialization — happy path, double-init guard
//! * `record_payment` — happy path, monotonic IDs, payment_hash stored, reverse-lookup index
//!   written, all three sequential indices updated, event emission, full field round-trip, multiple
//!   payments
//! * `record_payment` — unauthorized (no auth mocked)
//! * `get_payment_by_hash` — existing hash, unknown hash returns None
//! * `get_payment_by_id` — existing ID, non-existent ID, ID 0
//! * `get_global_payment_count` — before/after recordings
//! * `get_agreement_payment_count` — before/after, multiple agreements
//! * `get_payments_by_agreement` — full page, partial page, multi-page, start_index=0,
//!   start_index>count, empty, exact boundary, limit capped
//! * `get_employer_payment_count` — before/after, multiple employers
//! * `get_payments_by_employer` — pagination, all boundary conditions
//! * `get_employee_payment_count` — before/after, multiple employees
//! * `get_payments_by_employee` — pagination, all boundary conditions
//! * Cross-index consistency — same payment visible via hash, ID, and all three sequential indices;
//!   all return identical records
//! * Security — record immutability, index counts only increase (no pruning), hash index written
//!   atomically with the primary record
//! * Large history — 20 records, boundary reads at exact count edge
//!
//! ## Security notes
//!
//! The tests below validate the following security properties directly:
//!
//! 1. **Unauthorized injection** — `test_record_payment_unauthorized_no_auth` confirms that
//!    `record_payment` panics with `Auth(InvalidAction)` when called without mocked auth for the
//!    registered payroll contract.
//!
//! 2. **History tampering** — `test_records_are_immutable_after_recording` verifies that a payment
//!    returned by all query paths is bit-for-bit identical after additional payments are recorded.
//!    There is no overwrite path in the contract; the test confirms this property holds at runtime.
//!
//! 3. **Unauthorized pruning** — `test_index_counts_only_increase` asserts that every index count
//!    after N insertions equals exactly N. Because counts can only increment and there is no
//!    decrement or delete path, it is impossible for any caller to remove entries from the
//!    pagination range without corrupting the counter, which would cause every subsequent paginated
//!    read to skip entries.
//!
//! 4. **Hash-record atomicity** — `test_hash_index_written_atomically` records a payment and
//!    immediately queries by hash. The reverse-lookup succeeds, confirming the hash index and the
//!    primary record are written in the same invocation and are always in sync.
//!
//! 5. **Double-init guard** — `test_initialize_double_init_rejected` uses the `try_initialize` path
//!    to confirm the second call is rejected without corrupting the already-initialized state.

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

    record(
        &client,
        &env,
        1,
        1u32,
        &token,
        10,
        &employer_a,
        &employee,
        0,
    );
    record(
        &client,
        &env,
        1,
        2u32,
        &token,
        20,
        &employer_a,
        &employee,
        1,
    );
    record(
        &client,
        &env,
        1,
        3u32,
        &token,
        30,
        &employer_b,
        &employee,
        2,
    );

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

    record(
        &client,
        &env,
        1,
        1u32,
        &token,
        10,
        &employer,
        &employee_a,
        0,
    );
    record(
        &client,
        &env,
        1,
        2u32,
        &token,
        20,
        &employer,
        &employee_a,
        1,
    );
    record(
        &client,
        &env,
        1,
        3u32,
        &token,
        30,
        &employer,
        &employee_b,
        2,
    );

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
        record(
            &client, &env, 1, i as u32, &token, i as i128, &from, &to, i as u64,
        );
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
    assert!(
        cost_100 > cost_10,
        "cost for 100 payments should exceed cost for 10"
    );
    assert!(
        cost_1000 > cost_100,
        "cost for 1000 payments should exceed cost for 100"
    );

    // The cost difference between 100 and 1000 should be bounded because
    // MAX_PAGE_SIZE caps the actual number of records read (100 vs 100)
    // The difference comes from index traversal overhead, not record deserialization
    let cost_ratio = cost_1000 as f64 / cost_100 as f64;
    println!("Cost ratio (1000/100 payments): {:.2}x", cost_ratio);
    assert!(
        cost_ratio < 10.0,
        "Cost ratio should be < 10x due to pagination cap; got {:.2}x",
        cost_ratio
    );
}

// ─── Multi-agreement counter-consistency ──────────────────────────────────────

/// Helper: sum `get_agreement_payment_count` over a slice of agreement IDs.
fn sum_agreement_counts(client: &PaymentHistoryContractClient<'_>, ids: &[u128]) -> u32 {
    ids.iter()
        .map(|id| client.get_agreement_payment_count(id))
        .sum()
}

#[test]
fn test_multi_agreement_interleaved_employer_count_equals_sum_of_agreement_counts() {
    /// Verifies the **counter-consistency invariant**:
    ///
    /// `get_employer_payment_count(employer)` must equal the sum of
    /// `get_agreement_payment_count(id)` for every agreement that belongs to
    /// that employer — **after every individual recording step**, not only at
    /// the end.
    ///
    /// ## Why this matters
    ///
    /// The employer-level index and each per-agreement index are maintained by
    /// separate storage keys. A bug that increments one counter without
    /// incrementing the other would only surface when both are queried
    /// together. This test interleaves recordings across four agreements in a
    /// deliberately unbalanced order to stress-test the atomicity of the dual
    /// counter update inside `record_payment`.
    ///
    /// ## Interleaving pattern
    ///
    /// Agreements 1, 2, 3, 4 receive payments in this order:
    ///
    /// ```text
    /// step  1 → agreement 1   (employer count: 1,  agg counts: [1,0,0,0])
    /// step  2 → agreement 3   (employer count: 2,  agg counts: [1,0,1,0])
    /// step  3 → agreement 2   (employer count: 3,  agg counts: [1,1,1,0])
    /// step  4 → agreement 1   (employer count: 4,  agg counts: [2,1,1,0])
    /// step  5 → agreement 4   (employer count: 5,  agg counts: [2,1,1,1])
    /// step  6 → agreement 2   (employer count: 6,  agg counts: [2,2,1,1])
    /// step  7 → agreement 3   (employer count: 7,  agg counts: [2,2,2,1])
    /// step  8 → agreement 1   (employer count: 8,  agg counts: [3,2,2,1])
    /// step  9 → agreement 4   (employer count: 9,  agg counts: [3,2,2,2])
    /// step 10 → agreement 2   (employer count: 10, agg counts: [3,3,2,2])
    /// step 11 → agreement 3   (employer count: 11, agg counts: [3,3,3,2])
    /// step 12 → agreement 4   (employer count: 12, agg counts: [3,3,3,3])
    /// ```
    ///
    /// After every step the invariant `employer_count == sum(agg_counts)` is
    /// asserted, so any partial-update bug is caught at the exact step where
    /// it first appears rather than only at the end.
    ///
    /// ## Security note
    ///
    /// The invariant cannot be satisfied by an implementation that increments
    /// only one of the two counters per `record_payment` call.  Because the
    /// interleaving is unbalanced (different agreements receive different
    /// numbers of payments), any off-by-one divergence in either counter
    /// direction is immediately visible.
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    // Use a separate employee per agreement so employee indices do not
    // accidentally compensate for bugs in the employer/agreement indices.
    let employee_a = Address::generate(&env);
    let employee_b = Address::generate(&env);
    let employee_c = Address::generate(&env);
    let employee_d = Address::generate(&env);

    // Four distinct agreement IDs all owned by the same employer.
    let agg1: u128 = 1001;
    let agg2: u128 = 1002;
    let agg3: u128 = 1003;
    let agg4: u128 = 1004;
    let all_agreements = [agg1, agg2, agg3, agg4];

    // Ordered recording steps: (agreement_id, employee, hash_seed, amount, timestamp)
    // The hash_seed values are all distinct to prevent the idempotency guard
    // from suppressing any recording.
    let steps: &[(u128, &Address, u32, i128, u64)] = &[
        (agg1, &employee_a, 10, 100, 1_000),   // step 1
        (agg3, &employee_c, 20, 300, 2_000),   // step 2
        (agg2, &employee_b, 30, 200, 3_000),   // step 3
        (agg1, &employee_a, 40, 110, 4_000),   // step 4
        (agg4, &employee_d, 50, 400, 5_000),   // step 5
        (agg2, &employee_b, 60, 210, 6_000),   // step 6
        (agg3, &employee_c, 70, 310, 7_000),   // step 7
        (agg1, &employee_a, 80, 120, 8_000),   // step 8
        (agg4, &employee_d, 90, 410, 9_000),   // step 9
        (agg2, &employee_b, 100, 220, 10_000), // step 10
        (agg3, &employee_c, 110, 320, 11_000), // step 11
        (agg4, &employee_d, 120, 420, 12_000), // step 12
    ];

    for (step_idx, &(agreement_id, employee, hash_seed, amount, timestamp)) in
        steps.iter().enumerate()
    {
        let payment_id = record(
            &client,
            &env,
            agreement_id,
            hash_seed,
            &token,
            amount,
            &employer,
            employee,
            timestamp,
        );

        // IDs are globally monotonic starting at 1.
        assert_eq!(
            payment_id,
            (step_idx as u128) + 1,
            "step {}: global payment ID must be monotonically increasing",
            step_idx + 1
        );

        // ── Core invariant: employer count == sum of per-agreement counts ──
        let employer_count = client.get_employer_payment_count(&employer);
        let agreement_sum = sum_agreement_counts(&client, &all_agreements);

        assert_eq!(
            employer_count,
            agreement_sum,
            "step {}: employer count ({}) must equal sum of per-agreement counts ({}); \
             individual counts: agg1={}, agg2={}, agg3={}, agg4={}",
            step_idx + 1,
            employer_count,
            agreement_sum,
            client.get_agreement_payment_count(&agg1),
            client.get_agreement_payment_count(&agg2),
            client.get_agreement_payment_count(&agg3),
            client.get_agreement_payment_count(&agg4),
        );

        // Employer count must also equal the number of steps completed so far.
        assert_eq!(
            employer_count,
            (step_idx as u32) + 1,
            "step {}: employer count must equal total steps completed",
            step_idx + 1
        );
    }

    // ── Final state assertions ────────────────────────────────────────────
    // Each agreement received exactly 3 payments in the interleaved schedule.
    assert_eq!(
        client.get_agreement_payment_count(&agg1),
        3u32,
        "agg1 final count"
    );
    assert_eq!(
        client.get_agreement_payment_count(&agg2),
        3u32,
        "agg2 final count"
    );
    assert_eq!(
        client.get_agreement_payment_count(&agg3),
        3u32,
        "agg3 final count"
    );
    assert_eq!(
        client.get_agreement_payment_count(&agg4),
        3u32,
        "agg4 final count"
    );
    assert_eq!(
        client.get_employer_payment_count(&employer),
        12u32,
        "employer final count must be 12"
    );
    assert_eq!(
        client.get_global_payment_count(),
        12u128,
        "global count must be 12"
    );

    // ── Paginated index completeness ──────────────────────────────────────
    // Retrieve all records via the employer index and verify none are missing.
    let employer_page = client.get_payments_by_employer(&employer, &1u32, &50u32);
    assert_eq!(
        employer_page.len(),
        12u32,
        "employer paginated query must return all 12 records"
    );
    // Every record in the employer index must point back to our employer address.
    for i in 0..employer_page.len() {
        let rec = employer_page.get(i).unwrap();
        assert_eq!(
            rec.from, employer,
            "employer index record {} must have the correct employer address",
            i
        );
    }

    // Retrieve all records for each agreement and verify amounts are correct.
    // Expected amounts per agreement (recorded in order of their interleave steps):
    // agg1: 100, 110, 120  (steps 1, 4, 8)
    // agg2: 200, 210, 220  (steps 3, 6, 10)
    // agg3: 300, 310, 320  (steps 2, 7, 11)
    // agg4: 400, 410, 420  (steps 5, 9, 12)
    let expected_amounts: &[(u128, [i128; 3])] = &[
        (agg1, [100, 110, 120]),
        (agg2, [200, 210, 220]),
        (agg3, [300, 310, 320]),
        (agg4, [400, 410, 420]),
    ];
    for &(agg_id, ref amounts) in expected_amounts {
        let page = client.get_payments_by_agreement(&agg_id, &1u32, &10u32);
        assert_eq!(
            page.len(),
            3u32,
            "agreement {} must have exactly 3 records",
            agg_id
        );
        for (pos, &expected_amount) in amounts.iter().enumerate() {
            let rec = page.get(pos as u32).unwrap();
            assert_eq!(
                rec.amount,
                expected_amount,
                "agreement {} position {}: expected amount {} got {}",
                agg_id,
                pos + 1,
                expected_amount,
                rec.amount
            );
            assert_eq!(
                rec.agreement_id, agg_id,
                "agreement index record must have the correct agreement_id"
            );
        }
    }

    // ── Cross-index consistency: employer index == union of agreement indices ─
    // Collect all global IDs from the employer index and from all per-agreement
    // indices; the two sets must be identical.
    let mut employer_ids: soroban_sdk::Vec<u128> = soroban_sdk::Vec::new(&env);
    for i in 0..employer_page.len() {
        employer_ids.push_back(employer_page.get(i).unwrap().id);
    }

    let mut agreement_ids: soroban_sdk::Vec<u128> = soroban_sdk::Vec::new(&env);
    for &agg_id in &all_agreements {
        let page = client.get_payments_by_agreement(&agg_id, &1u32, &10u32);
        for i in 0..page.len() {
            agreement_ids.push_back(page.get(i).unwrap().id);
        }
    }

    // Both collections must have the same length (12 records each).
    assert_eq!(
        employer_ids.len(),
        agreement_ids.len(),
        "employer index and union of agreement indices must have equal length"
    );

    // Every ID present in the employer index must appear in the union of
    // agreement indices and vice-versa.  We sort both before comparison since
    // insertion order differs.
    //
    // soroban_sdk::Vec does not expose a sort method, so we collect into a
    // std::vec::Vec for sorting in the test harness.
    let mut employer_ids_std: std::vec::Vec<u128> = (0..employer_ids.len())
        .map(|i| employer_ids.get(i).unwrap())
        .collect();
    let mut agreement_ids_std: std::vec::Vec<u128> = (0..agreement_ids.len())
        .map(|i| agreement_ids.get(i).unwrap())
        .collect();
    employer_ids_std.sort_unstable();
    agreement_ids_std.sort_unstable();

    assert_eq!(
        employer_ids_std, agreement_ids_std,
        "the set of payment IDs reachable via the employer index must equal \
         the union of IDs reachable via all per-agreement indices"
    );
}

#[test]
fn test_multi_agreement_invariant_holds_for_two_employers() {
    /// Verifies the counter-consistency invariant independently for two
    /// employers whose payments are interleaved in the same recording
    /// sequence.
    ///
    /// ## Security note
    ///
    /// With a single employer one might imagine a bug that keeps a single
    /// shared counter up-to-date while neglecting per-agreement counters (or
    /// vice versa).  By operating two employers simultaneously in the same
    /// contract state we confirm the indices are partitioned correctly: neither
    /// employer's counts are contaminated by the other's recordings.
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer_x = Address::generate(&env);
    let employer_y = Address::generate(&env);
    let employee = Address::generate(&env);

    // Employer X owns agreements 2001 and 2002.
    // Employer Y owns agreements 3001 and 3002.
    let x_agg1: u128 = 2001;
    let x_agg2: u128 = 2002;
    let y_agg1: u128 = 3001;
    let y_agg2: u128 = 3002;

    // Interleaved recording schedule.
    // Column layout: (agreement_id, employer, hash_seed, amount, timestamp)
    let steps: &[(u128, &Address, u32, i128, u64)] = &[
        (x_agg1, &employer_x, 201, 10, 1_000),
        (y_agg1, &employer_y, 202, 20, 2_000),
        (x_agg2, &employer_x, 203, 30, 3_000),
        (y_agg2, &employer_y, 204, 40, 4_000),
        (x_agg1, &employer_x, 205, 11, 5_000),
        (y_agg1, &employer_y, 206, 21, 6_000),
        (x_agg2, &employer_x, 207, 31, 7_000),
        (y_agg2, &employer_y, 208, 41, 8_000),
    ];

    for (step_idx, &(agreement_id, employer, hash_seed, amount, timestamp)) in
        steps.iter().enumerate()
    {
        record(
            &client,
            &env,
            agreement_id,
            hash_seed,
            &token,
            amount,
            employer,
            &employee,
            timestamp,
        );

        // Invariant holds for employer X after each step.
        let x_employer_count = client.get_employer_payment_count(&employer_x);
        let x_sum = client.get_agreement_payment_count(&x_agg1)
            + client.get_agreement_payment_count(&x_agg2);
        assert_eq!(
            x_employer_count,
            x_sum,
            "step {}: employer_x count ({}) != sum of x agreements ({})",
            step_idx + 1,
            x_employer_count,
            x_sum
        );

        // Invariant holds for employer Y after each step.
        let y_employer_count = client.get_employer_payment_count(&employer_y);
        let y_sum = client.get_agreement_payment_count(&y_agg1)
            + client.get_agreement_payment_count(&y_agg2);
        assert_eq!(
            y_employer_count,
            y_sum,
            "step {}: employer_y count ({}) != sum of y agreements ({})",
            step_idx + 1,
            y_employer_count,
            y_sum
        );

        // Neither employer's count must bleed into the other's.
        assert_eq!(
            x_employer_count + y_employer_count,
            (step_idx as u32) + 1,
            "step {}: combined employer counts must equal total steps taken",
            step_idx + 1
        );
    }

    // Final state: each employer has 4 payments across 2 agreements.
    assert_eq!(client.get_employer_payment_count(&employer_x), 4u32);
    assert_eq!(client.get_employer_payment_count(&employer_y), 4u32);
    assert_eq!(client.get_agreement_payment_count(&x_agg1), 2u32);
    assert_eq!(client.get_agreement_payment_count(&x_agg2), 2u32);
    assert_eq!(client.get_agreement_payment_count(&y_agg1), 2u32);
    assert_eq!(client.get_agreement_payment_count(&y_agg2), 2u32);
    assert_eq!(client.get_global_payment_count(), 8u128);
}

#[test]
fn test_multi_agreement_duplicate_hash_does_not_corrupt_counts() {
    /// Confirms that replaying a duplicate payment hash inside an interleaved
    /// multi-agreement sequence does not corrupt any counter.
    ///
    /// The idempotency guard must return the existing ID *without* touching
    /// any counter when the hash is already known. This test sandwiches the
    /// duplicate replay between two legitimate recordings so any counter
    /// corruption is caught by the subsequent invariant assertion.
    let env = create_env();
    let (_contract_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let agg1: u128 = 4001;
    let agg2: u128 = 4002;

    // Step 1: record a payment on agg1 with hash_seed 10.
    let id1 = record(
        &client, &env, agg1, 10, &token, 100, &employer, &employee, 1_000,
    );
    assert_eq!(id1, 1u128);

    // Step 2: record a payment on agg2.
    let id2 = record(
        &client, &env, agg2, 20, &token, 200, &employer, &employee, 2_000,
    );
    assert_eq!(id2, 2u128);

    // Replay the hash from step 1 — must be idempotent.
    let id_dup = record(
        &client, &env, agg1, 10, &token, 100, &employer, &employee, 1_000,
    );
    assert_eq!(id_dup, id1, "duplicate must return original ID");

    // Counts must be unchanged after the replay.
    assert_eq!(client.get_agreement_payment_count(&agg1), 1u32);
    assert_eq!(client.get_agreement_payment_count(&agg2), 1u32);
    assert_eq!(client.get_employer_payment_count(&employer), 2u32);
    assert_eq!(client.get_global_payment_count(), 2u128);

    // Invariant holds after replay.
    let employer_count = client.get_employer_payment_count(&employer);
    let agreement_sum =
        client.get_agreement_payment_count(&agg1) + client.get_agreement_payment_count(&agg2);
    assert_eq!(
        employer_count, agreement_sum,
        "employer count ({}) must equal sum of agreement counts ({}) after duplicate replay",
        employer_count, agreement_sum
    );

    // Step 3: record a new payment on agg1 after the duplicate.
    let id3 = record(
        &client, &env, agg1, 30, &token, 300, &employer, &employee, 3_000,
    );
    assert_eq!(id3, 3u128);

    // Invariant must still hold after the new recording.
    let employer_count_after = client.get_employer_payment_count(&employer);
    let agreement_sum_after =
        client.get_agreement_payment_count(&agg1) + client.get_agreement_payment_count(&agg2);
    assert_eq!(
        employer_count_after, agreement_sum_after,
        "employer count ({}) must equal sum of agreement counts ({}) after post-duplicate recording",
        employer_count_after, agreement_sum_after
    );
    assert_eq!(client.get_agreement_payment_count(&agg1), 2u32);
    assert_eq!(client.get_agreement_payment_count(&agg2), 1u32);
    assert_eq!(client.get_employer_payment_count(&employer), 3u32);
}

// ─── Duplicate-metadata collision safety ──────────────────────────────────────

/// Two payments that share identical amount, timestamp, token, and parties but
/// carry distinct payment hashes are recorded as two independent entries.
/// Records are keyed by the caller-supplied `payment_hash` and a sequential
/// global id, never by a hash derived from the metadata, so matching metadata
/// can never cause one record to silently overwrite the other.
#[test]
fn test_record_payment_identical_metadata_distinct_hashes_stored_separately() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let agreement_id: u128 = 1;
    let amount: i128 = 100;
    let timestamp: u64 = 1_700_000_000;

    // Identical metadata, distinct payment hashes (two distinct on-chain transfers).
    let id_a = record(
        &client,
        &env,
        agreement_id,
        0xA1,
        &token,
        amount,
        &from,
        &to,
        timestamp,
    );
    let id_b = record(
        &client,
        &env,
        agreement_id,
        0xB2,
        &token,
        amount,
        &from,
        &to,
        timestamp,
    );

    // Distinct, monotonically increasing ids: neither overwrote the other.
    assert_eq!(id_a, 1u128);
    assert_eq!(id_b, 2u128);

    // Both retrievable by id, each preserving its own hash and the shared metadata.
    let rec_a = client.get_payment_by_id(&id_a).unwrap();
    let rec_b = client.get_payment_by_id(&id_b).unwrap();
    assert_eq!(rec_a.payment_hash, make_hash(&env, 0xA1));
    assert_eq!(rec_b.payment_hash, make_hash(&env, 0xB2));
    assert_eq!(rec_a.amount, amount);
    assert_eq!(rec_b.amount, amount);
    assert_eq!(rec_a.timestamp, timestamp);
    assert_eq!(rec_b.timestamp, timestamp);
    assert_eq!(rec_a.from, from);
    assert_eq!(rec_b.from, from);
    assert_eq!(rec_a.to, to);
    assert_eq!(rec_b.to, to);

    // Both retrievable by their distinct hashes: the reverse index does not collide.
    assert_eq!(
        client
            .get_payment_by_hash(&make_hash(&env, 0xA1))
            .unwrap()
            .id,
        id_a
    );
    assert_eq!(
        client
            .get_payment_by_hash(&make_hash(&env, 0xB2))
            .unwrap()
            .id,
        id_b
    );

    // Every counter reflects two independent records, not a collapsed single entry.
    assert_eq!(client.get_global_payment_count(), 2u128);
    assert_eq!(client.get_agreement_payment_count(&agreement_id), 2u32);
    assert_eq!(client.get_employer_payment_count(&from), 2u32);
    assert_eq!(client.get_employee_payment_count(&to), 2u32);
}

/// The only case that collapses to a single entry is an identical *hash* (a
/// replay of the same transfer), never identical metadata. Re-recording the
/// same hash returns the existing id and writes no new record or index entry.
#[test]
fn test_record_payment_identical_hash_is_idempotent_replay() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let first = record(
        &client,
        &env,
        1,
        0xCD,
        &token,
        100,
        &from,
        &to,
        1_700_000_000,
    );
    let second = record(
        &client,
        &env,
        1,
        0xCD,
        &token,
        100,
        &from,
        &to,
        1_700_000_000,
    );

    // Same hash → same id, and nothing new recorded across any counter.
    assert_eq!(first, second);
    assert_eq!(client.get_global_payment_count(), 1u128);
    assert_eq!(client.get_agreement_payment_count(&1u128), 1u32);
    assert_eq!(client.get_employer_payment_count(&from), 1u32);
    assert_eq!(client.get_employee_payment_count(&to), 1u32);
}
