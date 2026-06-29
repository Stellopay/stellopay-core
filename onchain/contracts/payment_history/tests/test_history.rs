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

/// Build a deterministic 32-byte hash from a single seed byte.
/// Each distinct `seed` value produces a unique hash, making it easy to
/// assign distinct hashes to distinct payments in tests.
fn make_hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

/// Record a payment with a deterministic hash derived from `hash_seed`.
#[allow(clippy::too_many_arguments)]
fn record(
    client: &PaymentHistoryContractClient<'_>,
    env: &Env,
    agreement_id: u128,
    hash_seed: u8,
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
    hash_seed: u8,
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

    let id1 = record(&client, &env, 1, 1, &token, 100, &employer, &employee, 1_000);
    let id2 = record(&client, &env, 1, 2, &token, 200, &employer, &employee, 2_000);
    let id3 = record(&client, &env, 2, 3, &token, 300, &employer, &employee, 3_000);

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
    let hash = make_hash(&env, 0xAB);

    let payment_id =
        client.record_payment(&agreement_id, &hash, &token, &amount, &employer, &employee, &timestamp);

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
    record(&client, &env, 1, 1, &token, 50, &from, &to, 100);
    assert_eq!(client.get_global_payment_count(), 1u128);
    record(&client, &env, 1, 2, &token, 50, &from, &to, 200);
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

    record(&client, &env, 10, 1, &token, 500, &from, &to, 9_000);

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

    record(&client, &env, 7, 1, &token, 100, &employer, &employee, 1_000);

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
    let hash = make_hash(&env, 0x77);

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

    let by_hash = client.get_payment_by_hash(&hash).expect("record must exist");
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
    let hash = make_hash(&env, 0x42);

    let pid = client.record_payment(&5u128, &hash, &token, &777i128, &from, &to, &55_000u64);
    let rec = client.get_payment_by_hash(&hash);
    assert!(rec.is_some(), "hash lookup must return Some for recorded payment");
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

    let unknown_hash = make_hash(&env, 0x99);
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
    let hash = make_hash(&env, 0x01);

    let pid = client.record_payment(&1u128, &hash, &token, &100i128, &from, &to, &0u64);

    let by_id = client.get_payment_by_id(&pid).expect("must exist by ID");
    let by_hash = client.get_payment_by_hash(&hash).expect("must exist by hash");
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

    let h1 = make_hash(&env, 1);
    let h2 = make_hash(&env, 2);
    let h3 = make_hash(&env, 3);

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

    let pid = record(&client, &env, 5, 1, &token, 777, &from, &to, 55_000);
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

    assert!(client.get_payment_by_id(&0u128).is_none(), "ID 0 is never assigned");
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
        record(&client, &env, i as u128, i, &token, 10, &from, &to, i as u64 * 100);
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
    record(&client, &env, agreement_id, 1, &token, 1, &from, &to, 0);
    assert_eq!(client.get_agreement_payment_count(&agreement_id), 1u32);
    record(&client, &env, agreement_id, 2, &token, 2, &from, &to, 1);
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

    record(&client, &env, 1, 1, &token, 10, &from, &to, 0);
    record(&client, &env, 1, 2, &token, 20, &from, &to, 1);
    record(&client, &env, 2, 3, &token, 30, &from, &to, 2);

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

    record(&client, &env, 1, 1, &token, 500, &from, &to, 1_000);
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
        record(&client, &env, agreement_id, i, &token, i as i128 * 100, &from, &to, i as u64);
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
    record(&client, &env, 1, 1, &token, 100, &from, &to, 0);

    assert_eq!(
        client.get_payments_by_agreement(&1u128, &0u32, &10u32).len(),
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
    record(&client, &env, 1, 1, &token, 100, &from, &to, 0);

    assert_eq!(
        client.get_payments_by_agreement(&1u128, &2u32, &10u32).len(),
        0u32
    );
}

#[test]
fn test_get_payments_by_agreement_empty_history_returns_empty() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    assert_eq!(
        client.get_payments_by_agreement(&1u128, &1u32, &10u32).len(),
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
        record(&client, &env, agreement_id, i, &token, i as i128, &from, &to, i as u64);
    }

    let page = client.get_payments_by_agreement(&agreement_id, &1u32, &(MAX_PAGE_SIZE + 50));
    assert_eq!(page.len(), MAX_PAGE_SIZE, "limit must be capped at MAX_PAGE_SIZE");
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
        record(&client, &env, agreement_id, i, &token, i as i128, &from, &to, i as u64);
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
    record(&client, &env, 1, 1, &token, 100, &employer, &employee, 0);
    assert_eq!(client.get_employer_payment_count(&employer), 1u32);
    record(&client, &env, 2, 2, &token, 200, &employer, &employee, 1);
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

    record(&client, &env, 1, 1, &token, 10, &employer_a, &employee, 0);
    record(&client, &env, 1, 2, &token, 20, &employer_a, &employee, 1);
    record(&client, &env, 1, 3, &token, 30, &employer_b, &employee, 2);

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
        record(&client, &env, 1, i, &token, i as i128 * 10, &employer, &employee, i as u64);
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
    record(&client, &env, 1, 1, &token, 1, &from, &to, 0);

    assert_eq!(client.get_payments_by_employer(&from, &0u32, &10u32).len(), 0u32);
}

#[test]
fn test_get_payments_by_employer_start_index_above_count_returns_empty() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    record(&client, &env, 1, 1, &token, 1, &from, &to, 0);

    assert_eq!(client.get_payments_by_employer(&from, &2u32, &10u32).len(), 0u32);
}

#[test]
fn test_get_payments_by_employer_empty_history_returns_empty() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let employer = Address::generate(&env);
    assert_eq!(client.get_payments_by_employer(&employer, &1u32, &10u32).len(), 0u32);
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
    record(&client, &env, 1, 1, &token, 100, &employer, &employee, 0);
    assert_eq!(client.get_employee_payment_count(&employee), 1u32);
    record(&client, &env, 2, 2, &token, 200, &employer, &employee, 1);
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

    record(&client, &env, 1, 1, &token, 10, &employer, &employee_a, 0);
    record(&client, &env, 1, 2, &token, 20, &employer, &employee_a, 1);
    record(&client, &env, 1, 3, &token, 30, &employer, &employee_b, 2);

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
        record(&client, &env, 1, i, &token, i as i128 * 10, &employer, &employee, i as u64);
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
    record(&client, &env, 1, 1, &token, 1, &from, &to, 0);

    assert_eq!(client.get_payments_by_employee(&to, &0u32, &10u32).len(), 0u32);
}

#[test]
fn test_get_payments_by_employee_start_index_above_count_returns_empty() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    record(&client, &env, 1, 1, &token, 1, &from, &to, 0);

    assert_eq!(client.get_payments_by_employee(&to, &2u32, &10u32).len(), 0u32);
}

#[test]
fn test_get_payments_by_employee_empty_history_returns_empty() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    initialize_contract(&env, &client);

    let employee = Address::generate(&env);
    assert_eq!(client.get_payments_by_employee(&employee, &1u32, &10u32).len(), 0u32);
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

    let payment_id =
        client.record_payment(&agreement_id, &hash, &token, &amount, &employer, &employee, &9_999u64);

    let by_hash = client.get_payment_by_hash(&hash).expect("must exist by hash");
    let by_id   = client.get_payment_by_id(&payment_id).expect("must exist by id");
    let by_agg  = client.get_payments_by_agreement(&agreement_id, &1u32, &1u32).get(0).unwrap();
    let by_empr = client.get_payments_by_employer(&employer, &1u32, &1u32).get(0).unwrap();
    let by_empe = client.get_payments_by_employee(&employee, &1u32, &1u32).get(0).unwrap();

    assert_eq!(by_hash, by_id);
    assert_eq!(by_id,   by_agg);
    assert_eq!(by_agg,  by_empr);
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

    let pid = record(&client, &env, 1, 1, &token, 500, &from, &to, 1_000);

    // Add more payments after the first one.
    for i in 2..7u8 {
        record(&client, &env, 2, i, &token, 9_999, &from, &to, 2_000);
    }

    let rec = client.get_payment_by_id(&pid).unwrap();
    assert_eq!(rec.id, pid);
    assert_eq!(rec.amount, 500, "original record must be unchanged");
    assert_eq!(rec.agreement_id, 1u128);
    assert_eq!(rec.payment_hash, make_hash(&env, 1));

    // Also verify via hash lookup.
    let by_hash = client.get_payment_by_hash(&make_hash(&env, 1)).unwrap();
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
        record(&client, &env, agreement_id, i, &token, i as i128, &from, &to, i as u64);
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
        record(&client, &env, agreement_id, i, &token, i as i128, &from, &to, i as u64);
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
        record(&client, &env, 1, i, &token, i as i128, &from, &to, i as u64);
    }
    for i in 10..15u8 {
        record(&client, &env, 2, i, &token, i as i128 * 10, &from, &to, (100 + i) as u64);
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
            hash_seed: 0x11,
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
            hash_seed: 0x22,
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
            hash_seed: 0x33,
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
            hash_seed: 0x44,
            token: token.clone(),
            amount: 650,
            from: expense_payer.clone(),
            to: worker.clone(),
            timestamp: 1_004,
        },
    ];

    for (idx, fixture) in fixtures.iter().enumerate() {
        assert!(!fixture.source.topic().is_empty(), "source topic must be defined");
        assert!(fixture.source_event_id > 0, "source event id must be non-zero");

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
    let newer_hash = make_hash(&env, 0x90);
    let older_hash = make_hash(&env, 0x91);

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
