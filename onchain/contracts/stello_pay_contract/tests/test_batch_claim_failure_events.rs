#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Symbol, TryFromVal, Vec,
};
use stello_pay_contract::storage::{
    Agreement, AgreementMode, AgreementStatus, DataKey, DisputeStatus, PayrollError, StorageKey,
};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

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
    let employee0 = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();

    (env, client, employer, employee0, employee1, token)
}

/// Seeds a payroll agreement with per-employee claim metadata so the money path
/// can be exercised without relying on the higher-level creation flow.
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

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

/// Counts events whose topic symbol equals `event_name`.
fn count_events(env: &Env, event_name: &str) -> usize {
    env.events()
        .all()
        .iter()
        .filter(|e| {
            if e.1.len() > 0 {
                let topic = e.1.get(0).unwrap();
                if let Ok(sym) = Symbol::try_from_val(env, &topic) {
                    return sym.to_string() == event_name;
                }
            }
            false
        })
        .count()
}

/// `batch_claim_payroll` must emit one `batch_claim_failed_event` per failed
/// entry and still pay out the valid entries (partial success, no abort).
#[test]
fn batch_claim_payroll_emits_failure_event_per_failed_entry() {
    let (env, client, employer, employee0, employee1, token) = setup();
    let salary0 = 1_000i128;
    let salary1 = 2_500i128;
    let agreement_id = create_funded_payroll(
        &env,
        &client,
        &employer,
        &[(employee0.clone(), salary0), (employee1.clone(), salary1)],
        &token,
        30_000,
    );

    advance_time(&env, PERIOD_SECONDS); // one period elapsed -> employee0 can claim

    // caller = employee0 (valid for index 0, not employee1, and index 99 is OOB)
    let indices = Vec::from_array(&env, [0u32, 1u32, 99u32]);
    let result = client.batch_claim_payroll(&employee0, &agreement_id, &indices);

    // index 0 succeeds; index 1 (Unauthorized) and index 99 (InvalidEmployeeIndex) fail.
    assert_eq!(result.successful_claims, 1);
    assert_eq!(result.failed_claims, 2);

    // One per-failure event, counted before any contract read resets the buffer.
    assert_eq!(count_events(&env, "batch_claim_failed_event"), 2);

    // The batch continues: valid employee got paid for the elapsed period.
    assert_eq!(TokenClient::new(&env, &token).balance(&employee0), salary0);
    assert_eq!(TokenClient::new(&env, &token).balance(&employee1), 0);
}

/// `batch_claim_milestones` must emit one `batch_claim_failed_event` per failed
/// entry (duplicate + out-of-bounds) while still claiming the valid milestone.
#[test]
fn batch_claim_milestones_emits_failure_event_per_failed_entry() {
    let env = Env::default();
    env.mock_all_auths();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    let amount = 5_000i128;
    client.add_milestone(&agreement_id, &amount);
    mint(&env, &token, &employer, amount);
    client.fund_milestone_agreement(&agreement_id, &employer, &amount);
    client.approve_milestone(&agreement_id, &1);

    // caller = contributor (valid id 1, duplicate id 1, out-of-bounds id 99)
    let ids = Vec::from_array(&env, [1u32, 1u32, 99u32]);
    let result = client.batch_claim_milestones(&agreement_id, &ids);

    assert_eq!(result.successful_claims, 1);
    assert_eq!(result.failed_claims, 2);
    assert_eq!(count_events(&env, "batch_claim_failed_event"), 2);
}

#[test]
fn batch_claim_failed_event_reveals_only_claimant() {
    let (env, client, employer, employee0, employee1, token) = setup();
    let agreement_id = create_funded_payroll(
        &env,
        &client,
        &employer,
        &[(employee0.clone(), 1_000), (employee1.clone(), 1_000)],
        &token,
        10_000,
    );
    advance_time(&env, PERIOD_SECONDS);

    // All-fail batch: index 99 OOB + index 1 unauthorized (caller is employee0).
    let indices = Vec::from_array(&env, [99u32, 1u32]);
    let result = client.batch_claim_payroll(&employee0, &agreement_id, &indices);
    assert_eq!(result.failed_claims, 2);
    assert_eq!(count_events(&env, "batch_claim_failed_event"), 2);

    // No tokens moved despite two failure events.
    assert_eq!(TokenClient::new(&env, &token).balance(&employee0), 0);
    assert_eq!(TokenClient::new(&env, &token).balance(&employee1), 0);
}
