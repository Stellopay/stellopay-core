//! Comprehensive tests for the compliance reporting contract.
//!
//! Coverage targets:
//! - Initialization (happy path, double-init guard, pre-init rejection)
//! - Publisher management (grant, revoke, admin-only enforcement)
//! - Emergency pause (write blocked, reads unaffected, unpause restores writes)
//! - Record logging (happy path, auth enforcement, amount validation,
//!   monotonic IDs, global sequence, publisher tracking, metadata)
//! - Report generation (date filtering, type filtering, limit enforcement,
//!   empty results, early-exit, newest-first ordering)
//! - Edge cases (zero records, single record, limit boundary, equal dates)
//! - Tamper-evidence (contiguous IDs, global seq, immutable reads)
//! - Multi-employer isolation

#![cfg(test)]
#![allow(deprecated)]

use compliance_reporting::{
    ComplianceError, ComplianceReportingContract, ComplianceReportingContractClient, ReportType,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Bytes, Env,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup() -> (Env, ComplianceReportingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, ComplianceReportingContract);
    let client = ComplianceReportingContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    (env, client, admin)
}

/// Convenience: log a record where employer == publisher.
fn log_as_employer(
    client: &ComplianceReportingContractClient,
    env: &Env,
    employer: &Address,
    employee: &Address,
    token: &Address,
    amount: i128,
    report_type: &ReportType,
) -> u32 {
    client.log_record(
        employer,
        employer,
        employee,
        token,
        &amount,
        report_type,
        &Bytes::new(env),
    )
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_happy_path() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, ComplianceReportingContract);
    let client = ComplianceReportingContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    assert!(client.is_publisher(&admin));
    assert!(!client.is_paused());
    assert_eq!(client.get_global_seq(), 0);
}

#[test]
fn test_initialize_double_init_rejected() {
    let (_, client, admin) = setup();
    let err = client.try_initialize(&admin).unwrap_err().unwrap();
    assert_eq!(err, ComplianceError::AlreadyInitialized);
}

#[test]
fn test_log_record_before_init_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ComplianceReportingContract);
    let client = ComplianceReportingContractClient::new(&env, &contract_id);
    let addr = Address::generate(&env);

    let err = client
        .try_log_record(&addr, &addr, &addr, &addr, &100, &ReportType::Payroll, &Bytes::new(&env))
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ComplianceError::NotInitialized);
}

#[test]
fn test_generate_report_before_init_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ComplianceReportingContract);
    let client = ComplianceReportingContractClient::new(&env, &contract_id);
    let addr = Address::generate(&env);

    let err = client
        .try_generate_report(&addr, &0, &1000, &None, &10)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ComplianceError::NotInitialized);
}

// ---------------------------------------------------------------------------
// Publisher management
// ---------------------------------------------------------------------------

#[test]
fn test_set_publisher_grant_and_revoke() {
    let (env, client, admin) = setup();
    let publisher = Address::generate(&env);

    assert!(!client.is_publisher(&publisher));

    client.set_publisher(&admin, &publisher, &true);
    assert!(client.is_publisher(&publisher));

    client.set_publisher(&admin, &publisher, &false);
    assert!(!client.is_publisher(&publisher));
}

#[test]
fn test_set_publisher_non_admin_rejected() {
    let (env, client, _) = setup();
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);

    let err = client
        .try_set_publisher(&attacker, &target, &true)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ComplianceError::NotAuthorized);
}

// ---------------------------------------------------------------------------
// Emergency pause
// ---------------------------------------------------------------------------

#[test]
fn test_pause_blocks_log_record() {
    let (env, client, admin) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    client.set_paused(&admin, &true);
    assert!(client.is_paused());

    let err = client
        .try_log_record(
            &employer, &employer, &employee, &token,
            &100, &ReportType::Payroll, &Bytes::new(&env),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ComplianceError::ContractPaused);
}

#[test]
fn test_pause_does_not_block_reads() {
    let (env, client, admin) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    log_as_employer(&client, &env, &employer, &employee, &token, 500, &ReportType::Payroll);

    client.set_paused(&admin, &true);

    assert_eq!(client.get_record_count(&employer), 1);
    assert!(client.get_record(&employer, &1).is_some());
    let report = client.generate_report(&employer, &0, &2000, &None, &10);
    assert_eq!(report.record_count, 1);
}

#[test]
fn test_unpause_restores_writes() {
    let (env, client, admin) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    client.set_paused(&admin, &true);
    client.set_paused(&admin, &false);
    assert!(!client.is_paused());

    env.ledger().set_timestamp(1000);
    let id = log_as_employer(&client, &env, &employer, &employee, &token, 100, &ReportType::Payroll);
    assert_eq!(id, 1);
}

#[test]
fn test_set_paused_non_admin_rejected() {
    let (env, client, _) = setup();
    let attacker = Address::generate(&env);

    let err = client
        .try_set_paused(&attacker, &true)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ComplianceError::NotAuthorized);
}

// ---------------------------------------------------------------------------
// Record logging
// ---------------------------------------------------------------------------

#[test]
fn test_log_record_employer_as_publisher() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    let id = log_as_employer(&client, &env, &employer, &employee, &token, 5000, &ReportType::Payroll);

    assert_eq!(id, 1);
    assert_eq!(client.get_record_count(&employer), 1);
    assert_eq!(client.get_global_seq(), 1);

    let record = client.get_record(&employer, &1).unwrap();
    assert_eq!(record.id, 1);
    assert_eq!(record.global_seq, 1);
    assert_eq!(record.amount, 5000);
    assert_eq!(record.timestamp, 1000);
    assert_eq!(record.report_type, ReportType::Payroll);
    assert_eq!(record.publisher, employer);
}

#[test]
fn test_log_record_authorized_publisher() {
    let (env, client, admin) = setup();
    let publisher = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    client.set_publisher(&admin, &publisher, &true);

    env.ledger().set_timestamp(2000);
    let id = client.log_record(
        &publisher, &employer, &employee, &token,
        &1000, &ReportType::Tax, &Bytes::new(&env),
    );

    assert_eq!(id, 1);
    let record = client.get_record(&employer, &1).unwrap();
    assert_eq!(record.publisher, publisher);
    assert_eq!(record.employer, employer);
}

#[test]
fn test_log_record_unauthorized_publisher_rejected() {
    let (env, client, _) = setup();
    let attacker = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let err = client
        .try_log_record(
            &attacker, &employer, &employee, &token,
            &100, &ReportType::Payroll, &Bytes::new(&env),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ComplianceError::NotAuthorized);
}

#[test]
fn test_log_record_revoked_publisher_rejected() {
    let (env, client, admin) = setup();
    let publisher = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    client.set_publisher(&admin, &publisher, &true);
    client.set_publisher(&admin, &publisher, &false);

    let err = client
        .try_log_record(
            &publisher, &employer, &employee, &token,
            &100, &ReportType::Payroll, &Bytes::new(&env),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ComplianceError::NotAuthorized);
}

#[test]
fn test_log_record_zero_amount_rejected() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let err = client
        .try_log_record(
            &employer, &employer, &employee, &token,
            &0, &ReportType::Payroll, &Bytes::new(&env),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ComplianceError::InvalidAmount);
}

#[test]
fn test_log_record_negative_amount_rejected() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let err = client
        .try_log_record(
            &employer, &employer, &employee, &token,
            &-1, &ReportType::Payroll, &Bytes::new(&env),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ComplianceError::InvalidAmount);
}

#[test]
fn test_log_record_monotonic_ids_per_employer() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    for expected_id in 1u32..=5 {
        let id = log_as_employer(&client, &env, &employer, &employee, &token, 100, &ReportType::Payroll);
        assert_eq!(id, expected_id);
    }
    assert_eq!(client.get_record_count(&employer), 5);
}

#[test]
fn test_log_record_global_seq_increments_across_employers() {
    let (env, client, _) = setup();
    let employer_a = Address::generate(&env);
    let employer_b = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    log_as_employer(&client, &env, &employer_a, &employee, &token, 100, &ReportType::Payroll);
    log_as_employer(&client, &env, &employer_b, &employee, &token, 200, &ReportType::Tax);
    log_as_employer(&client, &env, &employer_a, &employee, &token, 300, &ReportType::Payroll);

    assert_eq!(client.get_global_seq(), 3);
    assert_eq!(client.get_record_count(&employer_a), 2);
    assert_eq!(client.get_record_count(&employer_b), 1);

    let rec_a1 = client.get_record(&employer_a, &1).unwrap();
    let rec_b1 = client.get_record(&employer_b, &1).unwrap();
    let rec_a2 = client.get_record(&employer_a, &2).unwrap();

    assert_eq!(rec_a1.global_seq, 1);
    assert_eq!(rec_b1.global_seq, 2);
    assert_eq!(rec_a2.global_seq, 3);
}

#[test]
fn test_get_record_nonexistent_returns_none() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);

    assert!(client.get_record(&employer, &1).is_none());
    assert!(client.get_record(&employer, &999).is_none());
}

#[test]
fn test_log_record_with_metadata() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let mut metadata = Bytes::new(&env);
    metadata.push_back(0x51); // 'Q'
    metadata.push_back(0x6d); // 'm'

    env.ledger().set_timestamp(1000);
    let id = client.log_record(
        &employer, &employer, &employee, &token,
        &999, &ReportType::Regulatory, &metadata,
    );

    let record = client.get_record(&employer, &id).unwrap();
    assert_eq!(record.metadata, metadata);
}

// ---------------------------------------------------------------------------
// Report generation
// ---------------------------------------------------------------------------

#[test]
fn test_generate_report_empty_employer() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);

    let report = client.generate_report(&employer, &0, &9999, &None, &10);
    assert_eq!(report.record_count, 0);
    assert_eq!(report.total_amount, 0);
    assert_eq!(report.records.len(), 0);
}

#[test]
fn test_generate_report_date_filtering() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    log_as_employer(&client, &env, &employer, &employee, &token, 100, &ReportType::Payroll);
    env.ledger().set_timestamp(2000);
    log_as_employer(&client, &env, &employer, &employee, &token, 200, &ReportType::Tax);
    env.ledger().set_timestamp(3000);
    log_as_employer(&client, &env, &employer, &employee, &token, 300, &ReportType::Regulatory);

    // Only t=2000 falls in [1500, 2500].
    let report = client.generate_report(&employer, &1500, &2500, &None, &50);
    assert_eq!(report.record_count, 1);
    assert_eq!(report.total_amount, 200);
    assert_eq!(report.records.get(0).unwrap().report_type, ReportType::Tax);
}

#[test]
fn test_generate_report_inclusive_boundaries() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    log_as_employer(&client, &env, &employer, &employee, &token, 100, &ReportType::Payroll);
    env.ledger().set_timestamp(2000);
    log_as_employer(&client, &env, &employer, &employee, &token, 200, &ReportType::Payroll);

    let report = client.generate_report(&employer, &1000, &2000, &None, &50);
    assert_eq!(report.record_count, 2);
    assert_eq!(report.total_amount, 300);
}

#[test]
fn test_generate_report_type_filtering() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    log_as_employer(&client, &env, &employer, &employee, &token, 100, &ReportType::Tax);
    log_as_employer(&client, &env, &employer, &employee, &token, 500, &ReportType::Payroll);
    log_as_employer(&client, &env, &employer, &employee, &token, 200, &ReportType::Tax);

    let report = client.generate_report(&employer, &0, &2000, &Some(ReportType::Tax), &50);
    assert_eq!(report.record_count, 2);
    assert_eq!(report.total_amount, 300);
    for record in report.records.into_iter() {
        assert_eq!(record.report_type, ReportType::Tax);
    }
}

#[test]
fn test_generate_report_all_types_when_no_filter() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    log_as_employer(&client, &env, &employer, &employee, &token, 100, &ReportType::Payroll);
    log_as_employer(&client, &env, &employer, &employee, &token, 200, &ReportType::Tax);
    log_as_employer(&client, &env, &employer, &employee, &token, 300, &ReportType::Regulatory);

    let report = client.generate_report(&employer, &0, &2000, &None, &50);
    assert_eq!(report.record_count, 3);
    assert_eq!(report.total_amount, 600);
}

#[test]
fn test_generate_report_limit_caps_results() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    for _ in 0..10 {
        log_as_employer(&client, &env, &employer, &employee, &token, 100, &ReportType::Payroll);
    }

    let report = client.generate_report(&employer, &0, &2000, &None, &3);
    assert_eq!(report.record_count, 3);
    assert_eq!(report.total_amount, 300);
}

#[test]
fn test_generate_report_limit_zero_rejected() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);

    let err = client
        .try_generate_report(&employer, &0, &2000, &None, &0)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ComplianceError::QueryLimitExceeded);
}

#[test]
fn test_generate_report_limit_over_max_rejected() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);

    let err = client
        .try_generate_report(&employer, &0, &2000, &None, &101)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ComplianceError::QueryLimitExceeded);
}

#[test]
fn test_generate_report_limit_at_max_accepted() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);

    let report = client.generate_report(&employer, &0, &2000, &None, &100);
    assert_eq!(report.record_count, 0);
}

#[test]
fn test_generate_report_invalid_date_range() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);

    let err = client
        .try_generate_report(&employer, &2000, &1000, &None, &50)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ComplianceError::InvalidDateRange);
}

#[test]
fn test_generate_report_equal_start_end_date() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    log_as_employer(&client, &env, &employer, &employee, &token, 100, &ReportType::Payroll);

    let report = client.generate_report(&employer, &1000, &1000, &None, &10);
    assert_eq!(report.record_count, 1);
}

#[test]
fn test_generate_report_no_records_in_range() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().set_timestamp(5000);
    log_as_employer(&client, &env, &employer, &employee, &token, 100, &ReportType::Payroll);

    let report = client.generate_report(&employer, &0, &4999, &None, &10);
    assert_eq!(report.record_count, 0);
    assert_eq!(report.total_amount, 0);
}

#[test]
fn test_generate_report_returns_newest_first() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    log_as_employer(&client, &env, &employer, &employee, &token, 100, &ReportType::Payroll);
    env.ledger().set_timestamp(2000);
    log_as_employer(&client, &env, &employer, &employee, &token, 200, &ReportType::Payroll);
    env.ledger().set_timestamp(3000);
    log_as_employer(&client, &env, &employer, &employee, &token, 300, &ReportType::Payroll);

    let report = client.generate_report(&employer, &0, &5000, &None, &10);
    assert_eq!(report.record_count, 3);

    // Newest first: 300, 200, 100.
    let records = report.records;
    assert_eq!(records.get(0).unwrap().amount, 300);
    assert_eq!(records.get(1).unwrap().amount, 200);
    assert_eq!(records.get(2).unwrap().amount, 100);
}

// ---------------------------------------------------------------------------
// Tamper-evidence / replay resistance
// ---------------------------------------------------------------------------

#[test]
fn test_global_seq_never_resets() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    for _ in 0..5 {
        log_as_employer(&client, &env, &employer, &employee, &token, 100, &ReportType::Payroll);
    }
    assert_eq!(client.get_global_seq(), 5);
}

#[test]
fn test_record_ids_are_contiguous() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    for i in 1u32..=5 {
        let id = log_as_employer(&client, &env, &employer, &employee, &token, 100, &ReportType::Payroll);
        assert_eq!(id, i, "Expected contiguous ID {i}");
    }
}

#[test]
fn test_record_timestamp_is_ledger_time() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().set_timestamp(42_000);
    let id = log_as_employer(&client, &env, &employer, &employee, &token, 100, &ReportType::Payroll);

    let record = client.get_record(&employer, &id).unwrap();
    assert_eq!(record.timestamp, 42_000);
}

#[test]
fn test_records_are_immutable_after_write() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    let id = log_as_employer(&client, &env, &employer, &employee, &token, 777, &ReportType::Tax);

    let r1 = client.get_record(&employer, &id).unwrap();
    let r2 = client.get_record(&employer, &id).unwrap();
    assert_eq!(r1, r2);
    assert_eq!(r1.amount, 777);
}

// ---------------------------------------------------------------------------
// Multi-employer isolation
// ---------------------------------------------------------------------------

#[test]
fn test_employer_records_are_isolated() {
    let (env, client, _) = setup();
    let employer_a = Address::generate(&env);
    let employer_b = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    log_as_employer(&client, &env, &employer_a, &employee, &token, 100, &ReportType::Payroll);
    log_as_employer(&client, &env, &employer_a, &employee, &token, 200, &ReportType::Payroll);
    log_as_employer(&client, &env, &employer_b, &employee, &token, 999, &ReportType::Tax);

    assert_eq!(client.get_record_count(&employer_a), 2);
    assert_eq!(client.get_record_count(&employer_b), 1);

    // employer_b's record 1 is its own, not employer_a's.
    assert_eq!(client.get_record(&employer_b, &1).unwrap().amount, 999);
}

// ---------------------------------------------------------------------------
// Large batch (stress)
// ---------------------------------------------------------------------------

#[test]
fn test_large_batch_logging_and_report() {
    let (env, client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    for i in 1u64..=50 {
        env.ledger().set_timestamp(i * 100);
        log_as_employer(&client, &env, &employer, &employee, &token, 10, &ReportType::Payroll);
    }

    assert_eq!(client.get_record_count(&employer), 50);
    assert_eq!(client.get_global_seq(), 50);

    let report = client.generate_report(&employer, &0, &6000, &None, &100);
    assert_eq!(report.record_count, 50);
    assert_eq!(report.total_amount, 500);
}
