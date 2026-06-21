#![cfg(test)]

use rate_limiter::{RateLimiter, RateLimiterClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient as TokenAdminClient},
    Address, Env, Vec,
};
use stello_pay_contract::storage::PayrollError;
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn addr(env: &Env) -> Address {
    Address::generate(env)
}

fn setup_token(env: &Env) -> (Address, Address) {
    let admin = addr(env);
    let token = env.register_stellar_asset_contract(admin.clone());
    (token, admin)
}

fn setup(
    env: &Env,
) -> (
    Address,
    PayrollContractClient<'static>,
    RateLimiterClient<'static>,
    Address,
    Address,
) {
    let payroll_id = env.register_contract(None, PayrollContract);
    let payroll_client = PayrollContractClient::new(env, &payroll_id);
    let owner = addr(env);
    payroll_client.initialize(&owner);

    let rl_id = env.register_contract(None, RateLimiter);
    let rl_client = RateLimiterClient::new(env, &rl_id);

    // Initialize rate limiter: 2 token burst, 0 refill
    rl_client.initialize(&owner, &2, &0, &false);

    // Wire them up
    payroll_client.set_rate_limiter_contract(&owner, &rl_id);

    (payroll_id, payroll_client, rl_client, owner, rl_id)
}

#[test]
fn test_rate_limited_claim() {
    let env = create_test_env();
    let (payroll_id, client, rl_client, owner, _) = setup(&env);
    let (token, _token_admin) = setup_token(&env);

    let employer = addr(&env);
    let employee = addr(&env);

    // Create payroll
    let agreement_id = client.create_payroll_agreement(&employer, &token, &3600);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    client.activate_agreement(&agreement_id);

    // Setup DataKey storage for claiming
    env.as_contract(&payroll_id, || {
        stello_pay_contract::storage::DataKey::set_agreement_activation_time(
            &env,
            agreement_id,
            env.ledger().timestamp(),
        );
        stello_pay_contract::storage::DataKey::set_agreement_period_duration(
            &env,
            agreement_id,
            3600,
        );
        stello_pay_contract::storage::DataKey::set_agreement_token(&env, agreement_id, &token);

        stello_pay_contract::storage::DataKey::set_employee(&env, agreement_id, 0, &employee);
        stello_pay_contract::storage::DataKey::set_employee_salary(&env, agreement_id, 0, 1000);
        stello_pay_contract::storage::DataKey::set_employee_claimed_periods(
            &env,
            agreement_id,
            0,
            0,
        );

        stello_pay_contract::storage::DataKey::set_employee_count(&env, agreement_id, 1);

        stello_pay_contract::storage::DataKey::set_agreement_escrow_balance(
            &env,
            agreement_id,
            &token,
            10000000,
        );
    });

    // Fund it
    TokenAdminClient::new(&env, &token).mint(&employer, &10000000);
    TokenClient::new(&env, &token).transfer(&employer, &client.address, &10000000);

    // Advance time by 3 periods (simulate 1 period = 30 days roughly, though period defaults to month if not escrow)

    // Wait, payroll agreement periods logic: 30 days is default period if not escrow
    env.ledger()
        .with_mut(|li| li.timestamp = li.timestamp + 30 * 24 * 3600);

    // First claim should succeed (consumes 1 rate limit token)
    let res1 = client.try_claim_payroll(&employee, &agreement_id, &0);
    assert_eq!(res1, Ok(Ok(())));

    // Advance time again to accrue more payroll
    env.ledger()
        .with_mut(|li| li.timestamp = li.timestamp + 30 * 24 * 3600);

    // Second claim should succeed (consumes 2nd rate limit token)
    let res2 = client.try_claim_payroll(&employee, &agreement_id, &0);
    assert_eq!(res2, Ok(Ok(())));

    // Advance time again to accrue more payroll
    env.ledger()
        .with_mut(|li| li.timestamp = li.timestamp + 30 * 24 * 3600);

    // Third claim should fail (0 rate limit tokens left)
    let res3 = client.try_claim_payroll(&employee, &agreement_id, &0);
    assert_eq!(res3, Err(Ok(PayrollError::RateLimited)));
}

#[test]
fn test_batch_claim_rate_limited() {
    let env = create_test_env();
    let (payroll_id, client, _, _owner, _) = setup(&env);
    let (token, _token_admin) = setup_token(&env);

    let employer = addr(&env);
    let employee1 = addr(&env);
    let employee2 = addr(&env);
    let employee3 = addr(&env);

    // Create payroll
    let agreement_id = client.create_payroll_agreement(&employer, &token, &3600);
    client.add_employee_to_agreement(&agreement_id, &employee1, &1000);
    client.add_employee_to_agreement(&agreement_id, &employee2, &1000);
    client.add_employee_to_agreement(&agreement_id, &employee3, &1000);
    client.activate_agreement(&agreement_id);

    // Setup DataKey storage for claiming
    env.as_contract(&payroll_id, || {
        stello_pay_contract::storage::DataKey::set_agreement_activation_time(
            &env,
            agreement_id,
            env.ledger().timestamp(),
        );
        stello_pay_contract::storage::DataKey::set_agreement_period_duration(
            &env,
            agreement_id,
            3600,
        );
        stello_pay_contract::storage::DataKey::set_agreement_token(&env, agreement_id, &token);

        stello_pay_contract::storage::DataKey::set_employee(&env, agreement_id, 0, &employee1);
        stello_pay_contract::storage::DataKey::set_employee_salary(&env, agreement_id, 0, 1000);
        stello_pay_contract::storage::DataKey::set_employee_claimed_periods(
            &env,
            agreement_id,
            0,
            0,
        );

        stello_pay_contract::storage::DataKey::set_employee(&env, agreement_id, 1, &employee2);
        stello_pay_contract::storage::DataKey::set_employee_salary(&env, agreement_id, 1, 1000);
        stello_pay_contract::storage::DataKey::set_employee_claimed_periods(
            &env,
            agreement_id,
            1,
            0,
        );

        stello_pay_contract::storage::DataKey::set_employee(&env, agreement_id, 2, &employee3);
        stello_pay_contract::storage::DataKey::set_employee_salary(&env, agreement_id, 2, 1000);
        stello_pay_contract::storage::DataKey::set_employee_claimed_periods(
            &env,
            agreement_id,
            2,
            0,
        );

        stello_pay_contract::storage::DataKey::set_employee_count(&env, agreement_id, 3);

        stello_pay_contract::storage::DataKey::set_agreement_escrow_balance(
            &env,
            agreement_id,
            &token,
            1000000,
        );
    });

    // Fund it
    TokenAdminClient::new(&env, &token).mint(&employer, &3000000);
    TokenClient::new(&env, &token).transfer(&employer, &client.address, &3000000);

    // Fast forward
    env.ledger()
        .with_mut(|li| li.timestamp = li.timestamp + 30 * 24 * 3600);

    let mut indices = Vec::new(&env);
    indices.push_back(0);

    // Claim 1
    assert_eq!(
        client
            .try_batch_claim_payroll(&employee1, &agreement_id, &indices)
            .is_ok(),
        true
    );

    // Claim 2
    assert_eq!(
        client
            .try_batch_claim_payroll(&employee1, &agreement_id, &indices)
            .is_ok(),
        true
    );

    // Claim 3 -> limited
    let res3 = client.try_batch_claim_payroll(&employee1, &agreement_id, &indices);
    assert_eq!(res3, Err(Ok(PayrollError::RateLimited)));
}
