#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, Vec,
};

use stello_pay_contract::storage::{DataKey, PayrollError};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

/// Create a fresh test environment with a deployed payroll contract, owner,
/// arbiter and employer. Returns `(env, owner, employer, arbiter, client)`.
fn create_test_env() -> (
    Env,
    Address,
    Address,
    Address,
    PayrollContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    let arbiter = Address::generate(&env);
    client.set_arbiter(&owner, &arbiter);

    let employer = Address::generate(&env);

    (env, owner, employer, arbiter, client)
}

/// Simple sanity check that the FX helper round-trips a basic conversion using
/// the configured rate.
#[test]
fn test_convert_currency_basic() {
    let (env, owner, _employer, _arbiter, client) = create_test_env();

    let base = Address::generate(&env);
    let quote = Address::generate(&env);

    // FX rate: 1 base = 2 quote (rate scaled by 1e6).
    let rate: i128 = 2_000_000;

    client.set_exchange_rate(&owner, &base, &quote, &rate);

    // Convert 10 base units → expect 20 quote units.
    let amount: i128 = 10;
    let converted = client.convert_currency(&base, &quote, &amount);
    assert_eq!(converted, 20);
}

/// End‑to‑end test for `claim_payroll_in_token`:
///
/// - Agreement is denominated in `base_token`.
/// - Escrow is funded in `payout_token`.
/// - Employee claims one period and is paid in `payout_token` using FX rate.
#[test]
fn test_claim_payroll_in_different_token_uses_fx_rate() {
    let (env, owner, employer, _arbiter, client) = create_test_env();

    // ---------------------------------------------------------------------
    // Token setup
    // ---------------------------------------------------------------------
    let base_admin = Address::generate(&env);
    let base_token = env.register_stellar_asset_contract_v2(base_admin).address();

    let payout_admin = Address::generate(&env);
    let payout_token = env
        .register_stellar_asset_contract_v2(payout_admin)
        .address();

    // FX: 1 base = 2 payout.
    let fx_rate: i128 = 2_000_000;
    client.set_exchange_rate(&owner, &base_token, &payout_token, &fx_rate);

    // ---------------------------------------------------------------------
    // Agreement + employee setup
    // ---------------------------------------------------------------------
    let grace_period: u64 = 604_800; // 7 days
    let period_seconds: u64 = 86_400; // 1 day
    let salary_per_period: i128 = 1_000;

    let agreement_id = client.create_payroll_agreement(&employer, &base_token, &grace_period);

    let employee = Address::generate(&env);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary_per_period);

    // Activate agreement so claims are allowed after setup.
    client.activate_agreement(&agreement_id);

    // ---------------------------------------------------------------------
    // Seed DataKey metadata and escrow for the payout token.
    // ---------------------------------------------------------------------
    let contract_address = client.address.clone();

    // Fund payout token escrow for this agreement.
    let escrow_total: i128 = 20_000;
    let payout_client = StellarAssetClient::new(&env, &payout_token);
    payout_client.mint(&contract_address, &escrow_total);

    env.as_contract(&contract_address, || {
        let now = env.ledger().timestamp();

        DataKey::set_agreement_activation_time(&env, agreement_id, now);
        DataKey::set_agreement_period_duration(&env, agreement_id, period_seconds);
        DataKey::set_agreement_token(&env, agreement_id, &base_token);

        // Single employee at index 0
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary_per_period);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee_count(&env, agreement_id, 1);

        // Escrow funded in payout token only.
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &payout_token, escrow_total);
    });

    // Advance one full period so exactly one salary is claimable.
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds;
    });

    // ---------------------------------------------------------------------
    // Employee claims in payout_token.
    // ---------------------------------------------------------------------
    client.claim_payroll_in_token(&employee, &agreement_id, &0u32, &payout_token);

    // Employee receives salary in payout token using FX rate.
    let payout_token_client = soroban_sdk::token::Client::new(&env, &payout_token);
    let expected_payout: i128 = salary_per_period * 2; // 1_000 base × 2 = 2_000 payout
    assert_eq!(payout_token_client.balance(&employee), expected_payout);

    // Escrow balance and paid amount are updated correctly.
    env.as_contract(&contract_address, || {
        let remaining = DataKey::get_agreement_escrow_balance(&env, agreement_id, &payout_token);
        assert_eq!(remaining, escrow_total - expected_payout);

        let paid = DataKey::get_agreement_paid_amount(&env, agreement_id);
        assert_eq!(paid, salary_per_period);
    });
}
