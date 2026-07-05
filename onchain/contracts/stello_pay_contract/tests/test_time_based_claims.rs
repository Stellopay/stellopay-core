#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};
use stello_pay_contract::{
    storage::{AgreementStatus, DataKey, PayrollError},
    PayrollContract, PayrollContractClient,
};

const PERIOD_SECONDS: u64 = 86_400;

fn setup() -> (
    Env,
    PayrollContractClient<'static>,
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
    let contributor = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();

    (env, client, employer, contributor, token)
}

/// @notice Creates and funds an escrow agreement for `claim_time_based`.
/// @dev The contract has no public deposit function, so tests mint tokens to
/// the contract address and mirror that amount into escrow-balance storage.
fn create_funded_time_based_agreement(
    env: &Env,
    client: &PayrollContractClient,
    employer: &Address,
    contributor: &Address,
    token: &Address,
    amount_per_period: i128,
    num_periods: u32,
) -> u128 {
    let agreement_id = client.create_escrow_agreement(
        employer,
        contributor,
        token,
        &amount_per_period,
        &PERIOD_SECONDS,
        &num_periods,
    );
    let total = amount_per_period * i128::from(num_periods);

    StellarAssetClient::new(env, token).mint(&client.address, &total);
    env.as_contract(&client.address, || {
        DataKey::set_agreement_escrow_balance(env, agreement_id, token, total);
    });

    agreement_id
}

fn advance_time(env: &Env, seconds: u64) {
    env.ledger().with_mut(|ledger| {
        ledger.timestamp += seconds;
    });
}

#[test]
fn creates_time_based_escrow_agreement_with_claim_schedule() {
    let (env, client, employer, contributor, token) = setup();

    let agreement_id = create_funded_time_based_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1_500,
        4,
    );

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.amount_per_period, Some(1_500));
    assert_eq!(agreement.period_seconds, Some(PERIOD_SECONDS));
    assert_eq!(agreement.num_periods, Some(4));
    assert_eq!(agreement.claimed_periods, Some(0));
    assert_eq!(agreement.status, AgreementStatus::Created);
}

#[test]
fn time_based_claim_pays_all_elapsed_unclaimed_periods() {
    let (env, client, employer, contributor, token) = setup();
    let agreement_id = create_funded_time_based_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        2_000,
        5,
    );

    client.activate_agreement(&agreement_id);
    advance_time(&env, PERIOD_SECONDS * 3);
    client.claim_time_based(&agreement_id);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.claimed_periods, Some(3));
    assert_eq!(agreement.paid_amount, 6_000);
    assert_eq!(TokenClient::new(&env, &token).balance(&contributor), 6_000);
}

#[test]
fn time_based_claim_rounds_partial_periods_down() {
    let (env, client, employer, contributor, token) = setup();
    let agreement_id = create_funded_time_based_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        2_000,
        5,
    );

    client.activate_agreement(&agreement_id);
    advance_time(&env, PERIOD_SECONDS - 1);

    assert_eq!(
        client.try_claim_time_based(&agreement_id),
        Err(Ok(PayrollError::NoPeriodsToClaim))
    );
    assert_eq!(TokenClient::new(&env, &token).balance(&contributor), 0);
}

#[test]
fn time_based_claim_completes_when_final_period_is_claimed() {
    let (env, client, employer, contributor, token) = setup();
    let agreement_id = create_funded_time_based_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1_000,
        2,
    );

    client.activate_agreement(&agreement_id);
    advance_time(&env, PERIOD_SECONDS * 10);
    client.claim_time_based(&agreement_id);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.claimed_periods, Some(2));
    assert_eq!(agreement.status, AgreementStatus::Completed);
    assert_eq!(
        client.try_claim_time_based(&agreement_id),
        Err(Ok(PayrollError::AllPeriodsClaimed))
    );
}

#[test]
fn time_based_claim_during_grace_period_still_pays_elapsed_periods() {
    let (env, client, employer, contributor, token) = setup();
    let agreement_id = create_funded_time_based_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1_000,
        10,
    );

    client.activate_agreement(&agreement_id);
    advance_time(&env, PERIOD_SECONDS * 2);
    client.cancel_agreement(&agreement_id);
    client.claim_time_based(&agreement_id);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.claimed_periods, Some(2));
    assert_eq!(agreement.status, AgreementStatus::Cancelled);
    assert_eq!(TokenClient::new(&env, &token).balance(&contributor), 2_000);
}

#[test]
fn time_based_claim_after_grace_period_expires_is_rejected() {
    let (env, client, employer, contributor, token) = setup();
    let agreement_id = create_funded_time_based_agreement(
        &env,
        &client,
        &employer,
        &contributor,
        &token,
        1_000,
        2,
    );

    client.activate_agreement(&agreement_id);
    advance_time(&env, PERIOD_SECONDS);
    client.cancel_agreement(&agreement_id);
    advance_time(&env, PERIOD_SECONDS * 2 + 1);

    assert_eq!(
        client.try_claim_time_based(&agreement_id),
        Err(Ok(PayrollError::NotInGracePeriod))
    );
    assert_eq!(TokenClient::new(&env, &token).balance(&contributor), 0);
}
