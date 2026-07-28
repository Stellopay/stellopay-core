//! Batch-creation test suite for `batch_create_payroll_agreements` and
//! `batch_create_escrow_agreements`.
//!
//! # Atomicity guarantee
//!
//! Both batch-creation entry-points are **all-or-nothing**: every item in
//! the batch is validated before any agreement is written to storage.  A
//! single invalid entry causes the whole call to return an error and leaves
//! **zero** agreements created.
//!
//! # Scenario matrix
//!
//! | Test | Agreement type | Scenario |
//! |------|---------------|----------|
//! | `batch_create_payroll_success` | Payroll | All-valid batch creates every entry |
//! | `batch_create_payroll_empty_err` | Payroll | Empty batch returns `InvalidData` |
//! | `batch_create_payroll_over_limit_errs_before_creating` | Payroll | Oversized batch returns `BatchTooLarge` |
//! | `batch_create_payroll_invalid_middle_rejects_whole_batch` | Payroll | Invalid-middle entry → zero agreements created |
//! | `batch_create_payroll_invalid_first_rejects_whole_batch` | Payroll | Invalid-first entry → zero agreements created |
//! | `batch_create_payroll_invalid_last_rejects_whole_batch` | Payroll | Invalid-last entry → zero agreements created |
//! | `batch_create_payroll_all_valid_creates_all` | Payroll | All-valid acceptance criteria re-check |
//! | `batch_create_escrow_all_valid_success` | Escrow | All-valid batch creates every entry |
//! | `batch_create_escrow_empty_err` | Escrow | Empty batch returns `InvalidData` |
//! | `batch_create_escrow_over_limit_errs_before_creating` | Escrow | Oversized batch returns `BatchTooLarge` |
//! | `batch_create_escrow_invalid_middle_rejects_whole_batch` | Escrow | Invalid-middle zero-period → zero agreements created |
//! | `batch_create_escrow_invalid_first_rejects_whole_batch` | Escrow | Invalid-first zero-amount → zero agreements created |
//! | `batch_create_escrow_invalid_last_rejects_whole_batch` | Escrow | Invalid-last zero-periods → zero agreements created |
//! | `batch_create_escrow_invalid_middle_zero_amount` | Escrow | Middle entry zero amount → zero agreements created |
//! | `batch_create_escrow_invalid_middle_zero_num_periods` | Escrow | Middle entry zero num_periods → zero agreements created |
//! | `batch_create_escrow_all_valid_creates_all` | Escrow | All-valid acceptance criteria re-check |
//! | `batch_create_escrow_single_valid_creates_one` | Escrow | Batch of one valid entry |
//! | `batch_create_escrow_single_invalid_rejects` | Escrow | Batch of one invalid entry → error |
//! | `batch_create_escrow_agreement_ids_are_monotone` | Escrow | IDs in result are strictly increasing |

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, Symbol, TryFromVal, Vec,
};
use stello_pay_contract::storage::{
    EscrowCreateParams, PayrollCreateParams, PayrollError, MAX_BATCH_SIZE,
};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ============================================================================
// Helpers
// ============================================================================

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn addr(env: &Env) -> Address {
    Address::generate(env)
}

fn setup(env: &Env) -> (Address, PayrollContractClient<'static>) {
    let id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &id);
    let owner = addr(env);
    client.initialize(&owner);
    (id, client)
}

/// Count events whose first topic symbol matches `name`.
fn count_events(env: &Env, name: &str) -> usize {
    env.events()
        .all()
        .iter()
        .filter(|e| {
            e.1.get(0)
                .and_then(|v| Symbol::try_from_val(env, &v).ok())
                .map(|s| s.to_string() == name)
                .unwrap_or(false)
        })
        .count()
}

/// Build a valid `PayrollCreateParams` (grace_period_seconds > 0).
fn valid_payroll_params(env: &Env) -> PayrollCreateParams {
    PayrollCreateParams {
        token: addr(env),
        grace_period_seconds: 3600,
    }
}

/// Build a valid `EscrowCreateParams`.
fn valid_escrow_params(env: &Env) -> EscrowCreateParams {
    EscrowCreateParams {
        contributor: addr(env),
        token: addr(env),
        amount_per_period: 1_000,
        period_seconds: 3_600,
        num_periods: 4,
    }
}

// ============================================================================
// PAYROLL — happy path
// ============================================================================

/// All-valid payroll batch: every entry is created and events are emitted.
#[test]
fn batch_create_payroll_success() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let mut items = Vec::<PayrollCreateParams>::new(&env);
    items.push_back(PayrollCreateParams {
        token: addr(&env),
        grace_period_seconds: 3_600,
    });
    items.push_back(PayrollCreateParams {
        token: addr(&env),
        grace_period_seconds: 7_200,
    });
    items.push_back(PayrollCreateParams {
        token: addr(&env),
        grace_period_seconds: 86_400,
    });

    let res = client.batch_create_payroll_agreements(&employer, &items);

    assert_eq!(res.total_created, 3);
    assert_eq!(res.total_failed, 0);
    assert_eq!(res.results.len(), 3);
    assert_eq!(res.agreement_ids.len(), 3);
    for r in res.results.iter() {
        assert!(r.success);
        assert_eq!(r.error_code, 0);
    }
    assert!(count_events(&env, "agreement_created_event") >= 3);
}

/// All-valid acceptance criteria: a batch where every entry is valid must
/// create all agreements (issue AC #2).
#[test]
fn batch_create_payroll_all_valid_creates_all() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let mut items = Vec::<PayrollCreateParams>::new(&env);
    for _ in 0..5 {
        items.push_back(valid_payroll_params(&env));
    }

    let res = client.batch_create_payroll_agreements(&employer, &items);

    assert_eq!(res.total_created, 5, "all 5 valid entries must be created");
    assert_eq!(res.total_failed, 0);
    assert_eq!(res.agreement_ids.len(), 5);
}

// ============================================================================
// PAYROLL — batch-level error guards
// ============================================================================

#[test]
fn batch_create_payroll_empty_err() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let items = Vec::<PayrollCreateParams>::new(&env);
    let result = client.try_batch_create_payroll_agreements(&employer, &items);
    assert_eq!(result, Err(Ok(PayrollError::InvalidData)));
}

#[test]
fn batch_create_payroll_over_limit_errs_before_creating() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let mut items = Vec::<PayrollCreateParams>::new(&env);
    for _ in 0..=MAX_BATCH_SIZE {
        items.push_back(valid_payroll_params(&env));
    }

    let result = client.try_batch_create_payroll_agreements(&employer, &items);
    assert_eq!(result, Err(Ok(PayrollError::BatchTooLarge)));
}

// ============================================================================
// PAYROLL — atomicity: invalid entry rejects the whole batch (issue AC #1)
// ============================================================================

/// A batch where the **middle** entry is invalid (grace_period_seconds == 0)
/// must be rejected in full — zero agreements created.
///
/// This is the primary acceptance-criteria test from issue #818-style: placing
/// the invalid entry in the middle specifically guards against an implementation
/// that validates only the first/last item, or that creates valid entries before
/// encountering the invalid one.
#[test]
fn batch_create_payroll_invalid_middle_rejects_whole_batch() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let mut items = Vec::<PayrollCreateParams>::new(&env);
    items.push_back(valid_payroll_params(&env)); // valid
    items.push_back(valid_payroll_params(&env)); // valid
                                                 // ↓ invalid: grace_period_seconds == 0
    items.push_back(PayrollCreateParams {
        token: addr(&env),
        grace_period_seconds: 0,
    });
    items.push_back(valid_payroll_params(&env)); // valid
    items.push_back(valid_payroll_params(&env)); // valid

    let result = client.try_batch_create_payroll_agreements(&employer, &items);

    // Whole batch must be rejected.
    assert_eq!(
        result,
        Err(Ok(PayrollError::InvalidData)),
        "batch with invalid-middle entry must return InvalidData"
    );

    // No events must have been emitted — zero state was written.
    assert_eq!(
        count_events(&env, "agreement_created_event"),
        0,
        "zero agreement_created events must be emitted when batch is rejected"
    );
}

/// Invalid **first** entry: the very first item is invalid.
#[test]
fn batch_create_payroll_invalid_first_rejects_whole_batch() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let mut items = Vec::<PayrollCreateParams>::new(&env);
    // ↓ invalid: grace_period_seconds == 0
    items.push_back(PayrollCreateParams {
        token: addr(&env),
        grace_period_seconds: 0,
    });
    items.push_back(valid_payroll_params(&env));
    items.push_back(valid_payroll_params(&env));

    let result = client.try_batch_create_payroll_agreements(&employer, &items);
    assert_eq!(result, Err(Ok(PayrollError::InvalidData)));
    assert_eq!(count_events(&env, "agreement_created_event"), 0);
}

/// Invalid **last** entry: all entries before it are valid.
#[test]
fn batch_create_payroll_invalid_last_rejects_whole_batch() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let mut items = Vec::<PayrollCreateParams>::new(&env);
    items.push_back(valid_payroll_params(&env));
    items.push_back(valid_payroll_params(&env));
    // ↓ invalid: grace_period_seconds == 0
    items.push_back(PayrollCreateParams {
        token: addr(&env),
        grace_period_seconds: 0,
    });

    let result = client.try_batch_create_payroll_agreements(&employer, &items);
    assert_eq!(result, Err(Ok(PayrollError::InvalidData)));
    assert_eq!(count_events(&env, "agreement_created_event"), 0);
}

// ============================================================================
// ESCROW — happy path
// ============================================================================

/// All-valid escrow batch: every entry is created, events are emitted.
#[test]
fn batch_create_escrow_all_valid_success() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let mut items = Vec::<EscrowCreateParams>::new(&env);
    for _ in 0..3 {
        items.push_back(valid_escrow_params(&env));
    }

    let res = client.batch_create_escrow_agreements(&employer, &items);

    assert_eq!(res.total_created, 3);
    assert_eq!(res.total_failed, 0);
    assert_eq!(res.agreement_ids.len(), 3);
    for r in res.results.iter() {
        assert!(r.success);
        assert!(r.agreement_id.is_some());
        assert_eq!(r.error_code, 0);
    }
    assert!(count_events(&env, "agreement_created_event") >= 3);
    assert!(count_events(&env, "employee_added_event") >= 3);
}

/// All-valid acceptance criteria: every entry in an all-valid batch is created
/// (issue AC #2).
#[test]
fn batch_create_escrow_all_valid_creates_all() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let mut items = Vec::<EscrowCreateParams>::new(&env);
    for _ in 0..5 {
        items.push_back(valid_escrow_params(&env));
    }

    let res = client.batch_create_escrow_agreements(&employer, &items);
    assert_eq!(res.total_created, 5, "all 5 valid entries must be created");
    assert_eq!(res.total_failed, 0);
    assert_eq!(res.agreement_ids.len(), 5);
}

/// Batch of exactly one valid entry succeeds.
#[test]
fn batch_create_escrow_single_valid_creates_one() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let mut items = Vec::<EscrowCreateParams>::new(&env);
    items.push_back(valid_escrow_params(&env));

    let res = client.batch_create_escrow_agreements(&employer, &items);
    assert_eq!(res.total_created, 1);
    assert_eq!(res.total_failed, 0);
}

/// Batch of exactly one invalid entry is rejected.
#[test]
fn batch_create_escrow_single_invalid_rejects() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let mut items = Vec::<EscrowCreateParams>::new(&env);
    items.push_back(EscrowCreateParams {
        contributor: addr(&env),
        token: addr(&env),
        amount_per_period: 1_000,
        period_seconds: 0, // invalid
        num_periods: 4,
    });

    let result = client.try_batch_create_escrow_agreements(&employer, &items);
    assert_eq!(result, Err(Ok(PayrollError::ZeroPeriodDuration)));
}

/// Agreement IDs in the result must be strictly monotone (the ID counter
/// always advances).
#[test]
fn batch_create_escrow_agreement_ids_are_monotone() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let mut items = Vec::<EscrowCreateParams>::new(&env);
    for _ in 0..5 {
        items.push_back(valid_escrow_params(&env));
    }

    let res = client.batch_create_escrow_agreements(&employer, &items);
    let ids = res.agreement_ids;
    for i in 1..ids.len() {
        assert!(
            ids.get(i).unwrap() > ids.get(i - 1).unwrap(),
            "agreement IDs must be strictly increasing"
        );
    }
}

// ============================================================================
// ESCROW — batch-level error guards
// ============================================================================

#[test]
fn batch_create_escrow_empty_err() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let items = Vec::<EscrowCreateParams>::new(&env);
    let result = client.try_batch_create_escrow_agreements(&employer, &items);
    assert_eq!(result, Err(Ok(PayrollError::InvalidData)));
}

#[test]
fn batch_create_escrow_over_limit_errs_before_creating() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let mut items = Vec::<EscrowCreateParams>::new(&env);
    for _ in 0..=MAX_BATCH_SIZE {
        items.push_back(valid_escrow_params(&env));
    }

    let result = client.try_batch_create_escrow_agreements(&employer, &items);
    assert_eq!(result, Err(Ok(PayrollError::BatchTooLarge)));
}

// ============================================================================
// ESCROW — atomicity: invalid entry rejects the whole batch (issue AC #1)
// ============================================================================

/// A batch where the **middle** entry has `period_seconds == 0` must be
/// rejected in full — zero agreements created, zero events emitted.
///
/// This is the primary acceptance-criteria test: placing the invalid entry
/// in the middle guards against an implementation that creates valid-before
/// entries and then stops at the invalid one.
#[test]
fn batch_create_escrow_invalid_middle_rejects_whole_batch() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let mut items = Vec::<EscrowCreateParams>::new(&env);
    items.push_back(valid_escrow_params(&env)); // valid — entry 0
    items.push_back(valid_escrow_params(&env)); // valid — entry 1
                                                // ↓ invalid middle entry: period_seconds == 0
    items.push_back(EscrowCreateParams {
        contributor: addr(&env),
        token: addr(&env),
        amount_per_period: 1_000,
        period_seconds: 0, // ← deliberately invalid
        num_periods: 4,
    });
    items.push_back(valid_escrow_params(&env)); // valid — entry 3
    items.push_back(valid_escrow_params(&env)); // valid — entry 4

    let result = client.try_batch_create_escrow_agreements(&employer, &items);

    // Whole batch rejected — the two entries before the bad one must NOT have
    // been created.
    assert_eq!(
        result,
        Err(Ok(PayrollError::ZeroPeriodDuration)),
        "batch with invalid-middle entry must return ZeroPeriodDuration"
    );

    // Zero events: no state was written.
    assert_eq!(
        count_events(&env, "agreement_created_event"),
        0,
        "zero agreement_created events must be emitted when escrow batch is rejected"
    );
    assert_eq!(
        count_events(&env, "employee_added_event"),
        0,
        "zero employee_added events must be emitted when escrow batch is rejected"
    );
}

/// Invalid **first** entry (`amount_per_period == 0`): whole batch rejected.
#[test]
fn batch_create_escrow_invalid_first_rejects_whole_batch() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let mut items = Vec::<EscrowCreateParams>::new(&env);
    // ↓ invalid first entry: amount_per_period == 0
    items.push_back(EscrowCreateParams {
        contributor: addr(&env),
        token: addr(&env),
        amount_per_period: 0, // ← deliberately invalid
        period_seconds: 3_600,
        num_periods: 4,
    });
    items.push_back(valid_escrow_params(&env));
    items.push_back(valid_escrow_params(&env));

    let result = client.try_batch_create_escrow_agreements(&employer, &items);
    assert_eq!(result, Err(Ok(PayrollError::ZeroAmountPerPeriod)));
    assert_eq!(count_events(&env, "agreement_created_event"), 0);
}

/// Invalid **last** entry (`num_periods == 0`): all entries before it are
/// valid, but the whole batch is still rejected.
#[test]
fn batch_create_escrow_invalid_last_rejects_whole_batch() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let mut items = Vec::<EscrowCreateParams>::new(&env);
    items.push_back(valid_escrow_params(&env));
    items.push_back(valid_escrow_params(&env));
    // ↓ invalid last entry: num_periods == 0
    items.push_back(EscrowCreateParams {
        contributor: addr(&env),
        token: addr(&env),
        amount_per_period: 1_000,
        period_seconds: 3_600,
        num_periods: 0, // ← deliberately invalid
    });

    let result = client.try_batch_create_escrow_agreements(&employer, &items);
    assert_eq!(result, Err(Ok(PayrollError::ZeroNumPeriods)));
    assert_eq!(count_events(&env, "agreement_created_event"), 0);
}

/// Middle entry with `amount_per_period == 0` triggers `ZeroAmountPerPeriod`
/// and the whole batch is rejected.
#[test]
fn batch_create_escrow_invalid_middle_zero_amount() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let mut items = Vec::<EscrowCreateParams>::new(&env);
    items.push_back(valid_escrow_params(&env));
    items.push_back(EscrowCreateParams {
        contributor: addr(&env),
        token: addr(&env),
        amount_per_period: 0, // ← deliberately invalid
        period_seconds: 3_600,
        num_periods: 4,
    });
    items.push_back(valid_escrow_params(&env));

    let result = client.try_batch_create_escrow_agreements(&employer, &items);
    assert_eq!(result, Err(Ok(PayrollError::ZeroAmountPerPeriod)));
    assert_eq!(count_events(&env, "agreement_created_event"), 0);
}

/// Middle entry with `num_periods == 0` triggers `ZeroNumPeriods` and the
/// whole batch is rejected.
#[test]
fn batch_create_escrow_invalid_middle_zero_num_periods() {
    let env = create_test_env();
    let (_id, client) = setup(&env);
    let employer = addr(&env);

    let mut items = Vec::<EscrowCreateParams>::new(&env);
    items.push_back(valid_escrow_params(&env));
    items.push_back(EscrowCreateParams {
        contributor: addr(&env),
        token: addr(&env),
        amount_per_period: 1_000,
        period_seconds: 3_600,
        num_periods: 0, // ← deliberately invalid
    });
    items.push_back(valid_escrow_params(&env));

    let result = client.try_batch_create_escrow_agreements(&employer, &items);
    assert_eq!(result, Err(Ok(PayrollError::ZeroNumPeriods)));
    assert_eq!(count_events(&env, "agreement_created_event"), 0);
}
