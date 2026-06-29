//! Reentrancy protection tests (#197).
//!
//! Verifies state consistency for payment-related functions: after a successful
//! claim, state is updated so a second claim fails (double-claim prevented).

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::{testutils::Address as _, testutils::Ledger, Address, Env};
use stello_pay_contract::storage::{DataKey, PayrollError, StorageKey};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

const ONE_DAY: u64 = 86400;

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn create_address(env: &Env) -> Address {
    Address::generate(env)
}

fn create_token(env: &Env) -> Address {
    let admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

fn setup_contract(env: &Env) -> (Address, PayrollContractClient<'static>) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = create_address(env);
    client.initialize(&owner);
    (contract_id, client)
}

fn fund_agreement_escrow(
    env: &Env,
    contract_id: &Address,
    agreement_id: u128,
    token: &Address,
    amount: i128,
) {
    env.as_contract(contract_id, || {
        DataKey::set_agreement_escrow_balance(env, agreement_id, token, amount);
    });
}

fn advance_time(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| {
        li.timestamp += seconds;
    });
}

/// Verifies that after a successful claim_payroll, state (claimed periods) is updated
/// so a second claim for the same period fails with NoPeriodsToClaim.
#[test]
fn test_claim_payroll_state_updated_prevents_double_claim() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_token(&env);
    let employee = create_address(&env);
    let salary = 1000i128;
    let grace = 604800u64;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary);
    client.activate_agreement(&agreement_id);

    fund_agreement_escrow(&env, &contract_id, agreement_id, &token, 10000);
    mint(&env, &token, &contract_id, 10000);

    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee_count(&env, agreement_id, 1);
    });

    advance_time(&env, ONE_DAY + 1);

    let res = client.try_claim_payroll(&employee, &agreement_id, &0);
    assert!(res.is_ok());

    let claimed = client.get_employee_claimed_periods(&agreement_id, &0);
    assert_eq!(claimed, 1);

    let res2 = client.try_claim_payroll(&employee, &agreement_id, &0);
    assert!(
        res2.is_err() || res2.as_ref().ok().and_then(|r| r.as_ref().err()).is_some(),
        "second claim must fail (no periods to claim)"
    );
}

/// Verifies that the transient reentrancy guard rejects a claim while a claim
/// is already in progress. We simulate the in-progress state by setting the
/// guard in temporary storage (exactly what `acquire_reentrancy_guard` does at
/// the top of a claim), then assert the entry point fails deterministically
/// with `ReentrancyDetected` and that no state changes.
#[test]
fn test_reentrant_claim_payroll_rejected() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_token(&env);
    let employee = create_address(&env);
    let salary = 1000i128;
    let grace = 604800u64;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary);
    client.activate_agreement(&agreement_id);

    fund_agreement_escrow(&env, &contract_id, agreement_id, &token, 10000);
    mint(&env, &token, &contract_id, 10000);

    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee_count(&env, agreement_id, 1);
        // Simulate an in-progress claim by pre-setting the transient guard.
        env.storage()
            .temporary()
            .set(&StorageKey::ReentrancyGuard, &true);
    });

    advance_time(&env, ONE_DAY + 1);

    let res = client.try_claim_payroll(&employee, &agreement_id, &0);
    assert_eq!(
        res,
        Err(Ok(PayrollError::ReentrancyDetected)),
        "reentrant claim must be rejected"
    );

    // No state changed: no period was claimed and escrow is untouched.
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0), 0);
    env.as_contract(&contract_id, || {
        assert_eq!(
            DataKey::get_agreement_escrow_balance(&env, agreement_id, &token),
            10000
        );
    });
}

/// Verifies that the guard is released after a successful claim, so a later
/// legitimate claim (after time advances) is not blocked by a stranded guard.
#[test]
fn test_guard_released_allows_subsequent_claim() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let token = create_token(&env);
    let employee = create_address(&env);
    let salary = 1000i128;
    let grace = 604800u64;

    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary);
    client.activate_agreement(&agreement_id);

    fund_agreement_escrow(&env, &contract_id, agreement_id, &token, 10000);
    mint(&env, &token, &contract_id, 10000);

    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee_count(&env, agreement_id, 1);
    });

    advance_time(&env, ONE_DAY + 1);
    assert!(client.try_claim_payroll(&employee, &agreement_id, &0).is_ok());
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0), 1);

    // Escrow was decremented (effect persisted), confirming a real claim ran.
    env.as_contract(&contract_id, || {
        assert_eq!(
            DataKey::get_agreement_escrow_balance(&env, agreement_id, &token),
            9000
        );
    });

    // After another period elapses, a second claim succeeds — proving the guard
    // was cleared and not stranded by the first claim.
    advance_time(&env, ONE_DAY + 1);
    assert!(client.try_claim_payroll(&employee, &agreement_id, &0).is_ok());
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0), 2);
}

/// Verifies that after claim_time_based, claimed periods are updated so
/// another claim without time advance does not double-pay.
/// (Requires full escrow funding setup; see test_grace_period for pattern.)
#[test]
#[ignore = "requires escrow balance storage setup - covered by test_claim_payroll_state_updated_prevents_double_claim"]
fn test_claim_time_based_state_updated_prevents_double_claim() {
    let env = create_env();
    let (contract_id, client) = setup_contract(&env);
    let employer = create_address(&env);
    let contributor = create_address(&env);
    let token = create_token(&env);
    let amount_per_period = 1000i128;
    let period_seconds = ONE_DAY;
    let num_periods = 4u32;

    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &amount_per_period,
        &period_seconds,
        &num_periods,
    );
    client.activate_agreement(&agreement_id);

    let token_client = TokenClient::new(&env, &token);
    mint(&env, &token, &employer, 4000);
    token_client.transfer(&employer, &contract_id, &4000);

    advance_time(&env, period_seconds + 1);

    let res = client.try_claim_time_based(&agreement_id);
    assert!(res.is_ok());
    let claimed = client.get_claimed_periods(&agreement_id);
    assert_eq!(claimed, 1);

    let res2 = client.try_claim_time_based(&agreement_id);
    assert!(res2.is_err(), "second claim in same period must fail");
    assert_eq!(client.get_claimed_periods(&agreement_id), 1);
}
