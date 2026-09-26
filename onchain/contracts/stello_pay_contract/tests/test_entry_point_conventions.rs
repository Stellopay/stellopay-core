//! Entry-point convention tests (#1317).
//!
//! Every fallible public entry point must surface a typed `PayrollError`
//! instead of a host trap. These tests pin the exact variant returned by each
//! rejection path of the entry points converted in #1317:
//!
//! * `fund_milestone_agreement`
//! * `activate_agreement`
//! * `resume_agreement`
//! * `cancel_agreement`
//! * `finalize_grace_period`
//!
//! The convention itself is documented in
//! `docs/entry-point-conventions.md`.

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};
use stello_pay_contract::{
    storage::{AgreementStatus, MilestoneKey, PayrollError},
    PayrollContract, PayrollContractClient,
};

const ONE_WEEK: u64 = 604_800;

fn setup(env: &Env) -> (Address, PayrollContractClient<'static>) {
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (contract_id, client)
}

fn new_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

// ============================================================================
// fund_milestone_agreement
// ============================================================================

#[test]
fn fund_milestone_unknown_agreement_returns_agreement_not_found() {
    let env = new_env();
    let (_cid, client) = setup(&env);
    let employer = Address::generate(&env);

    assert_eq!(
        client.try_fund_milestone_agreement(&999u128, &employer, &100i128),
        Err(Ok(PayrollError::AgreementNotFound))
    );
}

#[test]
fn fund_milestone_non_employer_returns_unauthorized() {
    let env = new_env();
    let (_cid, client) = setup(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);
    let stranger = Address::generate(&env);

    let id = client.create_milestone_agreement(
        &employer,
        &contributor,
        &token,
        &soroban_sdk::vec![&env, 100i128],
    );

    assert_eq!(
        client.try_fund_milestone_agreement(&id, &stranger, &100i128),
        Err(Ok(PayrollError::Unauthorized))
    );
}

#[test]
fn fund_milestone_zero_amount_returns_milestone_amount_invalid() {
    let env = new_env();
    let (_cid, client) = setup(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    let id = client.create_milestone_agreement(
        &employer,
        &contributor,
        &token,
        &soroban_sdk::vec![&env, 100i128],
    );

    assert_eq!(
        client.try_fund_milestone_agreement(&id, &employer, &0i128),
        Err(Ok(PayrollError::MilestoneAmountInvalid))
    );
}

#[test]
fn fund_milestone_cancelled_agreement_returns_invalid_status() {
    let env = new_env();
    let (cid, client) = setup(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    let id = client.create_milestone_agreement(
        &employer,
        &contributor,
        &token,
        &soroban_sdk::vec![&env, 100i128],
    );

    env.as_contract(&cid, || {
        env.storage()
            .persistent()
            .set(&MilestoneKey::Status(id), &AgreementStatus::Cancelled);
    });

    assert_eq!(
        client.try_fund_milestone_agreement(&id, &employer, &100i128),
        Err(Ok(PayrollError::MilestoneAgreementInvalidStatus))
    );
}

#[test]
fn fund_milestone_completed_agreement_returns_invalid_status() {
    let env = new_env();
    let (cid, client) = setup(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    let id = client.create_milestone_agreement(
        &employer,
        &contributor,
        &token,
        &soroban_sdk::vec![&env, 100i128],
    );

    env.as_contract(&cid, || {
        env.storage()
            .persistent()
            .set(&MilestoneKey::Status(id), &AgreementStatus::Completed);
    });

    assert_eq!(
        client.try_fund_milestone_agreement(&id, &employer, &100i128),
        Err(Ok(PayrollError::MilestoneAgreementInvalidStatus))
    );
}

#[test]
fn fund_milestone_escrow_overflow_returns_invalid_data() {
    let env = new_env();
    let (cid, client) = setup(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    let id = client.create_milestone_agreement(
        &employer,
        &contributor,
        &token,
        &soroban_sdk::vec![&env, 100i128],
    );

    env.as_contract(&cid, || {
        env.storage()
            .persistent()
            .set(&MilestoneKey::MilestoneEscrowBalance(id), &i128::MAX);
    });

    assert_eq!(
        client.try_fund_milestone_agreement(&id, &employer, &1i128),
        Err(Ok(PayrollError::InvalidData))
    );
}

// ============================================================================
// activate_agreement
// ============================================================================

#[test]
fn activate_unknown_agreement_returns_agreement_not_found() {
    let env = new_env();
    let (_cid, client) = setup(&env);

    assert_eq!(
        client.try_activate_agreement(&123_456u128),
        Err(Ok(PayrollError::AgreementNotFound))
    );
}

#[test]
fn activate_non_created_agreement_returns_invalid_data() {
    let env = new_env();
    let (_cid, client) = setup(&env);
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let employee = Address::generate(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &1000);
    client.activate_agreement(&id);

    assert_eq!(
        client.try_activate_agreement(&id),
        Err(Ok(PayrollError::InvalidData))
    );
}

#[test]
fn activate_payroll_without_employees_returns_no_employee() {
    let env = new_env();
    let (_cid, client) = setup(&env);
    let employer = Address::generate(&env);
    let token = Address::generate(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);

    assert_eq!(
        client.try_activate_agreement(&id),
        Err(Ok(PayrollError::NoEmployee))
    );
}

// ============================================================================
// resume_agreement
// ============================================================================

#[test]
fn resume_unknown_agreement_returns_agreement_not_found() {
    let env = new_env();
    let (_cid, client) = setup(&env);

    assert_eq!(
        client.try_resume_agreement(&777_777u128),
        Err(Ok(PayrollError::AgreementNotFound))
    );
}

#[test]
fn resume_non_paused_agreement_returns_invalid_data() {
    let env = new_env();
    let (_cid, client) = setup(&env);
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let employee = Address::generate(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&id, &employee, &1000);
    client.activate_agreement(&id);

    assert_eq!(
        client.try_resume_agreement(&id),
        Err(Ok(PayrollError::InvalidData))
    );
}

// ============================================================================
// cancel_agreement
// ============================================================================

#[test]
fn cancel_unknown_agreement_returns_agreement_not_found() {
    let env = new_env();
    let (_cid, client) = setup(&env);

    assert_eq!(
        client.try_cancel_agreement(&555_555u128),
        Err(Ok(PayrollError::AgreementNotFound))
    );
}

#[test]
fn cancel_non_cancellable_status_returns_invalid_data() {
    let env = new_env();
    let (_cid, client) = setup(&env);
    let employer = Address::generate(&env);
    let token = Address::generate(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.cancel_agreement(&id);

    assert_eq!(
        client.try_cancel_agreement(&id),
        Err(Ok(PayrollError::InvalidData))
    );
}

// ============================================================================
// finalize_grace_period
// ============================================================================

#[test]
fn finalize_unknown_agreement_returns_agreement_not_found() {
    let env = new_env();
    let (_cid, client) = setup(&env);

    assert_eq!(
        client.try_finalize_grace_period(&444_444u128),
        Err(Ok(PayrollError::AgreementNotFound))
    );
}

#[test]
fn finalize_non_cancelled_agreement_returns_invalid_data() {
    let env = new_env();
    let (_cid, client) = setup(&env);
    let employer = Address::generate(&env);
    let token = Address::generate(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);

    assert_eq!(
        client.try_finalize_grace_period(&id),
        Err(Ok(PayrollError::InvalidData))
    );
}

#[test]
fn finalize_before_grace_expiry_returns_invalid_data() {
    let env = new_env();
    let (_cid, client) = setup(&env);
    let employer = Address::generate(&env);
    let token = Address::generate(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.cancel_agreement(&id);

    assert_eq!(
        client.try_finalize_grace_period(&id),
        Err(Ok(PayrollError::InvalidData))
    );
}

#[test]
fn finalize_after_grace_expiry_is_idempotent() {
    let env = new_env();
    let (_cid, client) = setup(&env);
    let employer = Address::generate(&env);
    let token = Address::generate(&env);

    let id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.cancel_agreement(&id);

    env.ledger().with_mut(|li| {
        li.timestamp += ONE_WEEK + 1;
    });

    // First finalization succeeds, and a second call is a no-op that still
    // returns `Ok(())` (no duplicate refund, no re-emitted event).
    client.finalize_grace_period(&id);
    assert_eq!(client.try_finalize_grace_period(&id), Ok(Ok(())));
}
