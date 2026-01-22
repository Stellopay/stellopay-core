#![cfg(test)]
#![allow(deprecated)]
use crate::{storage::AgreementStatus, PayrollContract, PayrollContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn create_test_env() -> (
    Env,
    Address,
    Address,
    Address,
    PayrollContractClient<'static>,
) {
    let env = Env::default();
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    (env, employer, contributor, token, client)
}

#[test]
fn test_pause_agreement() {
    let (env, employer, _contributor, token, client) = create_test_env();
    env.mock_all_auths();

    // Create and activate agreement
    let agreement_id = client.create_payroll_agreement(&employer, &token, &3600);
    client.activate_agreement(&agreement_id);

    // Initial state check
    let agreement_before = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement_before.status, AgreementStatus::Active);

    // Pause agreement
    client.pause_agreement(&agreement_id);

    // Verify paused state
    let agreement_after = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement_after.status, AgreementStatus::Paused);
}

#[test]
fn test_resume_agreement() {
    let (env, employer, _contributor, token, client) = create_test_env();
    env.mock_all_auths();

    // Create, activate, and pause agreement
    let agreement_id = client.create_payroll_agreement(&employer, &token, &3600);
    client.activate_agreement(&agreement_id);
    client.pause_agreement(&agreement_id);

    // Verify currently paused
    let agreement_paused = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement_paused.status, AgreementStatus::Paused);

    // Resume agreement
    client.resume_agreement(&agreement_id);

    // Verify active state
    let agreement_resumed = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement_resumed.status, AgreementStatus::Active);
}

#[test]
#[should_panic(expected = "HostError")]
fn test_pause_agreement_access_control() {
    let (env, employer, _contributor, token, client) = create_test_env();

    // Authenticate as employer to create updates
    env.mock_all_auths();
    let agreement_id = client.create_payroll_agreement(&employer, &token, &3600);
    client.activate_agreement(&agreement_id);

    // Now switch to non-mocked setup or different user
    // The easiest way to test access control failure in Soroban is:
    // 1. Create state with correct auth
    // 2. Clear auth or mock different user
    // 3. Call protected function

    // We can't easily "unmock" all auths cleanly in one environment in all versions.
    // However, if we specify mock_auths for specific calls, it might override.
    // Better: verifying that `client.pause_agreement` fails when we DON'T mock the employer.

    // Since we called `mock_all_auths` above, it persists.
    // We need a fresh environment or to reset auths.
    // Resetting auths is tricky.

    // Alternative: Just try to pause with a different user if the function accepted a caller arg.
    // But `pause_agreement` takes no args and relies on `require_auth` of the stored employer.

    // Workaround: We can disable global mock auths by re-mocking with empty list?
    // `env.mock_auths(&[])` only sets expected auths for next call.

    // Let's try to mock auth for a DIFFERENT user.
    let _stranger = Address::generate(&env);
    env.mock_auths(&[]); // This implies NO auths are expected/allowed matching "employer"

    // This call should fail because `employer.require_auth()` won't find signature/mock
    client.pause_agreement(&agreement_id);
}

#[test]
#[should_panic(expected = "Cannot claim when agreement is paused")]
fn test_claim_blocked_when_paused() {
    let (env, employer, _contributor, token, client) = create_test_env();
    let employee = Address::generate(&env);

    env.mock_all_auths();

    // Create, add employee, activate
    let agreement_id = client.create_payroll_agreement(&employer, &token, &3600);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    client.activate_agreement(&agreement_id);

    // Pause
    client.pause_agreement(&agreement_id);

    // Try to claim - should fail
    client.claim_payroll(&agreement_id, &employee);
}

#[test]
fn test_pause_resume_state_transitions() {
    let (env, employer, _contributor, token, client) = create_test_env();
    env.mock_all_auths();

    let agreement_id = client.create_payroll_agreement(&employer, &token, &3600);

    // 1. Created -> Active
    client.activate_agreement(&agreement_id);
    assert_eq!(
        client.get_agreement(&agreement_id).unwrap().status,
        AgreementStatus::Active
    );

    // 2. Active -> Paused
    client.pause_agreement(&agreement_id);
    assert_eq!(
        client.get_agreement(&agreement_id).unwrap().status,
        AgreementStatus::Paused
    );

    // 3. Paused -> Active
    client.resume_agreement(&agreement_id);
    assert_eq!(
        client.get_agreement(&agreement_id).unwrap().status,
        AgreementStatus::Active
    );
}
