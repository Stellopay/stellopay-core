#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};
use stello_pay_contract::{
    storage::{
        Agreement, AgreementMode, AgreementStatus, DataKey, DisputeStatus, PayrollError, StorageKey,
    },
    PayrollContract, PayrollContractClient,
};

const PERIOD_SECONDS: u64 = 86_400;
const GRACE_SECONDS: u64 = PERIOD_SECONDS * 7;

fn setup() -> (
    Env,
    PayrollContractClient<'static>,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let other_employee = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();

    (env, client, employer, employee, other_employee, token)
}

/// @notice Builds a payroll agreement fixture with per-employee claim metadata.
/// @dev Payroll creation currently stores employees in `AgreementEmployees`, while
/// `claim_payroll` reads the legacy `DataKey` employee indexes. This helper
/// intentionally seeds both the agreement and the legacy claim indexes so the
/// restored tests exercise the money-path logic without relying on disabled code.
fn create_funded_payroll(
    env: &Env,
    client: &PayrollContractClient,
    employer: &Address,
    employees: &[(Address, i128)],
    token: &Address,
    escrow_amount: i128,
) -> u128 {
    let agreement_id = 1u128;
    let now = env.ledger().timestamp();

    env.as_contract(&client.address, || {
        let agreement = Agreement {
            id: agreement_id,
            employer: employer.clone(),
            token: token.clone(),
            mode: AgreementMode::Payroll,
            status: AgreementStatus::Active,
            total_amount: employees.iter().map(|(_, salary)| *salary).sum(),
            paid_amount: 0,
            created_at: now,
            activated_at: Some(now),
            cancelled_at: None,
            grace_period_seconds: GRACE_SECONDS,
            amount_per_period: None,
            period_seconds: Some(PERIOD_SECONDS),
            num_periods: None,
            claimed_periods: None,
            dispute_raised_at: None,
            dispute_status: DisputeStatus::None,
        };

        env.storage()
            .persistent()
            .set(&StorageKey::Agreement(agreement_id), &agreement);
        DataKey::set_employee_count(env, agreement_id, employees.len() as u32);
        DataKey::set_agreement_activation_time(env, agreement_id, now);
        DataKey::set_agreement_period_duration(env, agreement_id, PERIOD_SECONDS);
        DataKey::set_agreement_token(env, agreement_id, token);
        DataKey::set_agreement_escrow_balance(env, agreement_id, token, escrow_amount);

        for (index, (employee, salary)) in employees.iter().enumerate() {
            let index = index as u32;
            DataKey::set_employee(env, agreement_id, index, employee);
            DataKey::set_employee_salary(env, agreement_id, index, *salary);
            DataKey::set_employee_claimed_periods(env, agreement_id, index, 0);
        }
    });

    StellarAssetClient::new(env, token).mint(&client.address, &escrow_amount);
    agreement_id
}

fn advance_time(env: &Env, seconds: u64) {
    env.ledger().with_mut(|ledger| {
        ledger.timestamp += seconds;
    });
}

#[test]
fn employee_can_claim_elapsed_payroll_periods() {
    let (env, client, employer, employee, _, token) = setup();
    let salary = 1_000i128;
    let agreement_id = create_funded_payroll(
        &env,
        &client,
        &employer,
        &[(employee.clone(), salary)],
        &token,
        10_000,
    );

    advance_time(&env, PERIOD_SECONDS * 2);
    client.claim_payroll(&employee, &agreement_id, &0);

    assert_eq!(
        TokenClient::new(&env, &token).balance(&employee),
        salary * 2
    );
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0), 2);
}

#[test]
fn payroll_claims_are_tracked_per_employee() {
    let (env, client, employer, employee, other_employee, token) = setup();
    let agreement_id = create_funded_payroll(
        &env,
        &client,
        &employer,
        &[(employee.clone(), 1_000), (other_employee.clone(), 2_500)],
        &token,
        20_000,
    );

    advance_time(&env, PERIOD_SECONDS);
    client.claim_payroll(&employee, &agreement_id, &0);

    assert_eq!(TokenClient::new(&env, &token).balance(&employee), 1_000);
    assert_eq!(TokenClient::new(&env, &token).balance(&other_employee), 0);
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0), 1);
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &1), 0);

    client.claim_payroll(&other_employee, &agreement_id, &1);

    assert_eq!(
        TokenClient::new(&env, &token).balance(&other_employee),
        2_500
    );
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &1), 1);
}

#[test]
fn employee_cannot_claim_another_employee_index() {
    let (env, client, employer, employee, other_employee, token) = setup();
    let agreement_id = create_funded_payroll(
        &env,
        &client,
        &employer,
        &[(employee.clone(), 1_000), (other_employee, 1_000)],
        &token,
        10_000,
    );

    advance_time(&env, PERIOD_SECONDS);

    assert_eq!(
        client.try_claim_payroll(&employee, &agreement_id, &1),
        Err(Ok(PayrollError::Unauthorized))
    );
    assert_eq!(TokenClient::new(&env, &token).balance(&employee), 0);
}

#[test]
fn cannot_claim_same_payroll_period_twice() {
    let (env, client, employer, employee, _, token) = setup();
    let agreement_id = create_funded_payroll(
        &env,
        &client,
        &employer,
        &[(employee.clone(), 1_000)],
        &token,
        10_000,
    );

    advance_time(&env, PERIOD_SECONDS);
    client.claim_payroll(&employee, &agreement_id, &0);

    assert_eq!(
        client.try_claim_payroll(&employee, &agreement_id, &0),
        Err(Ok(PayrollError::NoPeriodsToClaim))
    );
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0), 1);
}

#[test]
fn insufficient_payroll_escrow_rejects_without_state_change() {
    let (env, client, employer, employee, _, token) = setup();
    let agreement_id = create_funded_payroll(
        &env,
        &client,
        &employer,
        &[(employee.clone(), 1_000)],
        &token,
        500,
    );

    advance_time(&env, PERIOD_SECONDS);

    assert_eq!(
        client.try_claim_payroll(&employee, &agreement_id, &0),
        Err(Ok(PayrollError::InsufficientEscrowBalance))
    );
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0), 0);
    assert_eq!(TokenClient::new(&env, &token).balance(&employee), 0);
}
