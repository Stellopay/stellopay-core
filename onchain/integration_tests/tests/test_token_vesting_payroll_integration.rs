//! Cross-contract integration coverage for payroll lifecycle orchestration and
//! token vesting schedules.
//!
//! These tests intentionally use the generated clients for both contracts and a
//! shared SAC token. The current contracts do not call each other directly, so
//! the integration boundary is the off-chain workflow that binds one payroll
//! agreement to one vesting schedule for the same employer/employee pair.

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

use stello_pay_contract::storage::{AgreementStatus, DataKey, DisputeStatus};
use stello_pay_contract::{PayrollContract, PayrollContractClient};
use token_vesting::{TokenVestingContract, TokenVestingContractClient, VestingStatus};

const ONE_DAY: u64 = 86_400;
const ONE_WEEK: u64 = 604_800;
const SALARY_PER_DAY: i128 = 1_000;
const PAYROLL_FUND: i128 = 20_000;
const VESTING_GRANT: i128 = 12_000;
const EMPLOYER_FLOAT: i128 = 100_000;

struct PayrollDeployment<'a> {
    id: Address,
    client: PayrollContractClient<'a>,
}

struct VestingDeployment<'a> {
    id: Address,
    owner: Address,
    client: TokenVestingContractClient<'a>,
}

fn env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn addr(env: &Env) -> Address {
    Address::generate(env)
}

fn set_time(env: &Env, timestamp: u64) {
    env.ledger().with_mut(|li| li.timestamp = timestamp);
}

fn advance(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| li.timestamp += seconds);
}

fn token(env: &Env) -> Address {
    let admin = addr(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

fn transfer(env: &Env, token: &Address, from: &Address, to: &Address, amount: i128) {
    TokenClient::new(env, token).transfer(from, to, &amount);
}

fn balance(env: &Env, token: &Address, address: &Address) -> i128 {
    TokenClient::new(env, token).balance(address)
}

fn deploy_payroll(env: &Env) -> PayrollDeployment<'_> {
    let id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &id);
    let owner = addr(env);
    client.initialize(&owner);
    PayrollDeployment { id, client }
}

fn deploy_vesting(env: &Env) -> VestingDeployment<'_> {
    let id = env.register_contract(None, TokenVestingContract);
    let client = TokenVestingContractClient::new(env, &id);
    let owner = addr(env);
    client.initialize(&owner);
    VestingDeployment { id, owner, client }
}

fn seed_payroll_funding(
    env: &Env,
    payroll_id: &Address,
    agreement_id: u128,
    token: &Address,
    employee: &Address,
    salary: i128,
    total_fund: i128,
) {
    env.as_contract(payroll_id, || {
        DataKey::set_agreement_escrow_balance(env, agreement_id, token, total_fund);
        DataKey::set_agreement_activation_time(env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(env, agreement_id, token);
        DataKey::set_employee(env, agreement_id, 0, employee);
        DataKey::set_employee_salary(env, agreement_id, 0, salary);
        DataKey::set_employee_claimed_periods(env, agreement_id, 0, 0);
        DataKey::set_employee_count(env, agreement_id, 1);
    });
}

fn create_hired_employee_with_vesting(
    env: &Env,
    payroll: &PayrollDeployment<'_>,
    vesting: &VestingDeployment<'_>,
    employer: &Address,
    employee: &Address,
    token: &Address,
) -> (u128, u128) {
    let agreement_id = payroll
        .client
        .create_payroll_agreement(employer, token, &ONE_WEEK);
    payroll
        .client
        .add_employee_to_agreement(&agreement_id, employee, &SALARY_PER_DAY);
    payroll.client.activate_agreement(&agreement_id);

    transfer(env, token, employer, &payroll.id, PAYROLL_FUND);
    seed_payroll_funding(
        env,
        &payroll.id,
        agreement_id,
        token,
        employee,
        SALARY_PER_DAY,
        PAYROLL_FUND,
    );

    let schedule_id = vesting.client.create_linear_schedule(
        employer,
        employee,
        token,
        &VESTING_GRANT,
        &env.ledger().timestamp(),
        &(env.ledger().timestamp() + (ONE_DAY * 12)),
        &None,
        &true,
    );

    (agreement_id, schedule_id)
}

#[test]
fn employer_hires_employee_and_employee_claims_payroll_and_vesting_over_time() {
    let env = env();
    set_time(&env, 1_000);
    let payroll = deploy_payroll(&env);
    let vesting = deploy_vesting(&env);
    let token = token(&env);
    let employer = addr(&env);
    let employee = addr(&env);
    mint(&env, &token, &employer, EMPLOYER_FLOAT);

    let (agreement_id, schedule_id) =
        create_hired_employee_with_vesting(&env, &payroll, &vesting, &employer, &employee, &token);

    assert_eq!(balance(&env, &token, &payroll.id), PAYROLL_FUND);
    assert_eq!(balance(&env, &token, &vesting.id), VESTING_GRANT);

    advance(&env, ONE_DAY * 3);
    payroll.client.claim_payroll(&employee, &agreement_id, &0);
    let first_vesting_claim = vesting.client.claim(&employee, &schedule_id);

    assert_eq!(
        payroll
            .client
            .get_employee_claimed_periods(&agreement_id, &0),
        3
    );
    assert_eq!(first_vesting_claim, 3_000);
    assert_eq!(balance(&env, &token, &employee), 6_000);

    let replay_payroll = payroll
        .client
        .try_claim_payroll(&employee, &agreement_id, &0);
    let replay_vesting = vesting.client.try_claim(&employee, &schedule_id);
    assert!(replay_payroll.is_err());
    assert!(replay_vesting.is_err());

    advance(&env, ONE_DAY * 2);
    payroll.client.claim_payroll(&employee, &agreement_id, &0);
    let second_vesting_claim = vesting.client.claim(&employee, &schedule_id);

    assert_eq!(
        payroll
            .client
            .get_employee_claimed_periods(&agreement_id, &0),
        5
    );
    assert_eq!(second_vesting_claim, 2_000);
    assert_eq!(balance(&env, &token, &employee), 10_000);
}

#[test]
fn termination_revokes_unvested_tokens_and_freezes_later_vesting_claims() {
    let env = env();
    set_time(&env, 7_000);
    let payroll = deploy_payroll(&env);
    let vesting = deploy_vesting(&env);
    let token = token(&env);
    let employer = addr(&env);
    let employee = addr(&env);
    mint(&env, &token, &employer, EMPLOYER_FLOAT);

    let (agreement_id, schedule_id) =
        create_hired_employee_with_vesting(&env, &payroll, &vesting, &employer, &employee, &token);

    advance(&env, ONE_DAY * 3);
    payroll.client.claim_payroll(&employee, &agreement_id, &0);
    assert_eq!(vesting.client.claim(&employee, &schedule_id), 3_000);

    advance(&env, ONE_DAY * 2);
    payroll.client.cancel_agreement(&agreement_id);
    let employer_before_revoke = balance(&env, &token, &employer);
    let refunded = vesting.client.revoke(&employer, &schedule_id);

    assert_eq!(refunded, 7_000);
    assert_eq!(
        balance(&env, &token, &employer) - employer_before_revoke,
        7_000
    );
    assert_eq!(
        payroll.client.get_agreement(&agreement_id).unwrap().status,
        AgreementStatus::Cancelled
    );
    assert!(payroll.client.is_grace_period_active(&agreement_id));

    let frozen_vested_remainder = vesting.client.claim(&employee, &schedule_id);
    assert_eq!(frozen_vested_remainder, 2_000);
    assert_eq!(
        vesting.client.get_schedule(&schedule_id).unwrap().status,
        VestingStatus::Revoked
    );

    advance(&env, ONE_DAY * 30);
    assert_eq!(vesting.client.get_releasable_amount(&schedule_id), 0);
    assert!(vesting.client.try_claim(&employee, &schedule_id).is_err());
}

#[test]
fn admin_early_release_can_follow_payroll_dispute_or_grace_window_decision() {
    let env = env();
    set_time(&env, 20_000);
    let payroll = deploy_payroll(&env);
    let vesting = deploy_vesting(&env);
    let token = token(&env);
    let employer = addr(&env);
    let employee = addr(&env);
    let arbiter = addr(&env);
    mint(&env, &token, &employer, EMPLOYER_FLOAT);

    let (agreement_id, schedule_id) =
        create_hired_employee_with_vesting(&env, &payroll, &vesting, &employer, &employee, &token);

    advance(&env, ONE_DAY);
    payroll.client.cancel_agreement(&agreement_id);
    assert!(payroll.client.is_grace_period_active(&agreement_id));
    payroll.client.raise_dispute(&employee, &agreement_id);
    assert_eq!(
        payroll.client.get_dispute_status(&agreement_id),
        DisputeStatus::Raised
    );
    payroll.client.set_arbiter(&employer, &arbiter);
    payroll
        .client
        .resolve_dispute(&arbiter, &agreement_id, &1_000, &0);

    let employee_before = balance(&env, &token, &employee);
    let early_release = vesting
        .client
        .approve_early_release(&vesting.owner, &schedule_id, &2_000);

    assert_eq!(early_release, 2_000);
    assert_eq!(balance(&env, &token, &employee) - employee_before, 2_000);
    assert!(vesting
        .client
        .try_approve_early_release(&employee, &schedule_id, &1)
        .is_err());
}

#[test]
fn cannot_claim_or_revoke_with_mismatched_parties_across_payroll_and_vesting() {
    let env = env();
    set_time(&env, 30_000);
    let payroll = deploy_payroll(&env);
    let vesting = deploy_vesting(&env);
    let token = token(&env);
    let employer = addr(&env);
    let employee = addr(&env);
    let stranger = addr(&env);
    mint(&env, &token, &employer, EMPLOYER_FLOAT);

    let (agreement_id, schedule_id) =
        create_hired_employee_with_vesting(&env, &payroll, &vesting, &employer, &employee, &token);

    advance(&env, ONE_DAY * 4);

    assert!(payroll
        .client
        .try_claim_payroll(&stranger, &agreement_id, &0)
        .is_err());
    assert!(vesting.client.try_claim(&stranger, &schedule_id).is_err());
    assert!(vesting.client.try_revoke(&stranger, &schedule_id).is_err());

    payroll.client.claim_payroll(&employee, &agreement_id, &0);
    assert_eq!(vesting.client.claim(&employee, &schedule_id), 4_000);
}
