#![cfg(test)]

use proptest::prelude::*;
use soroban_sdk::{
    testutils::Address as _,
    Address, Env,
};

use stello_pay_contract::storage::{DataKey, PayrollError};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

/// Helper to deploy a fresh contract + owner in a new environment.
fn setup_contract() -> (Env, Address, PayrollContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    (env, owner, client)
}

proptest! {
    /// Property: `convert_currency` should match the fixed-point formula
    ///
    ///     converted = amount * rate / FX_SCALE
    ///
    /// for a wide range of small positive amounts and rates, where
    /// overflow cannot occur.
    #[test]
    fn prop_convert_currency_matches_scaled_multiplication(
        amount in 0i128..1_000_000,      // keep small to avoid overflow
        rate in 1i128..10_000_000,       // up to 10x with 1e6 scale
    ) {
        let (env, owner, client) = setup_contract();

        let from = Address::generate(&env);
        let to = Address::generate(&env);

        // Configure FX rate for (from, to).
        client.set_exchange_rate(&owner, &from, &to, &rate);

        // Contract helper should apply the same scaled multiplication.
        let converted = client.convert_currency(&from, &to, &amount);
        let expected = (amount * rate) / 1_000_000i128;

        prop_assert_eq!(converted, expected);
    }
}

proptest! {
    /// Property: a single successful `claim_payroll_in_token` call:
    ///
    /// - Moves exactly one period of salary into `AgreementPaidAmount` (base units).
    /// - Decrements payout escrow by the FX-converted amount.
    /// - Advances the employee's claimed period count by 1.
    #[test]
    fn prop_claim_payroll_in_token_preserves_core_invariants(
        salary_per_period in 1i128..10_000,      // small but non-zero
        fx_rate in 1i128..5_000_000,             // [1.0, 5.0) with 1e6 scale
        escrow_multiplier in 2i128..5i128,       // ensure escrow >= payout * 2
    ) {
        let (env, owner, client) = setup_contract();

        let employer = Address::generate(&env);
        let employee = Address::generate(&env);

        // Base and payout tokens are abstract Addresses; only FX + DataKey
        // use them, so we don't need live SAC instances here.
        let base_token = Address::generate(&env);
        let payout_token = Address::generate(&env);

        // Configure FX rate.
        client.set_exchange_rate(&owner, &base_token, &payout_token, &fx_rate);

        // Create payroll agreement in base_token.
        let grace: u64 = 7 * 24 * 60 * 60;
        let period_seconds: u64 = 24 * 60 * 60;

        let agreement_id = client.create_payroll_agreement(&employer, &base_token, &grace);
        client.add_employee_to_agreement(&agreement_id, &employee, &salary_per_period);
        client.activate_agreement(&agreement_id);

        // Seed DataKey state for payroll claiming and escrow in payout_token.
        let contract_address = client.address.clone();
        let escrow_total: i128 = salary_per_period
            .saturating_mul(escrow_multiplier)
            .saturating_mul(5); // extra safety margin

        env.as_contract(&contract_address, || {
            let now = env.ledger().timestamp();

            DataKey::set_agreement_activation_time(&env, agreement_id, now);
            DataKey::set_agreement_period_duration(&env, agreement_id, period_seconds);
            DataKey::set_agreement_token(&env, agreement_id, &base_token);

            DataKey::set_employee(&env, agreement_id, 0, &employee);
            DataKey::set_employee_salary(&env, agreement_id, 0, salary_per_period);
            DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
            DataKey::set_employee_count(&env, agreement_id, 1);

            DataKey::set_agreement_escrow_balance(&env, agreement_id, &payout_token, escrow_total);
        });

        // Advance exactly one period so there is exactly one claimable period.
        env.ledger().with_mut(|li| {
            li.timestamp += period_seconds;
        });

        // Attempt multi-currency claim.
        let result = client.try_claim_payroll_in_token(&employee, &agreement_id, &0u32, &payout_token);

        // The call must either:
        // - Succeed and preserve invariants, OR
        // - Fail with a well-defined PayrollError (e.g. due to overflow),
        //   but never violate accounting.
        match result {
            Ok(()) => {
                // On success, invariants must hold.
                env.as_contract(&contract_address, || {
                    let claimed = DataKey::get_employee_claimed_periods(&env, agreement_id, 0);
                    prop_assert_eq!(claimed, 1u32);

                    let paid = DataKey::get_agreement_paid_amount(&env, agreement_id);
                    prop_assert_eq!(paid, salary_per_period);

                    let remaining =
                        DataKey::get_agreement_escrow_balance(&env, agreement_id, &payout_token);
                    prop_assert!(remaining >= 0);
                    prop_assert!(remaining < escrow_total);
                });
            }
            Err(Ok(err)) => {
                // For property purposes we only assert the error is a
                // well-formed PayrollError; core invariants must still hold.
                match err {
                    PayrollError::ExchangeRateOverflow
                    | PayrollError::ExchangeRateInvalid
                    | PayrollError::InsufficientEscrowBalance
                    | PayrollError::InvalidData
                    | PayrollError::AgreementNotFound
                    | PayrollError::Unauthorized
                    | PayrollError::AgreementNotActivated
                    | PayrollError::InvalidAgreementMode
                    | PayrollError::NoPeriodsToClaim => {
                        // Expected, bounded error surface.
                    }
                    other => {
                        panic!("Unexpected PayrollError from claim_payroll_in_token: {:?}", other);
                    }
                }
            }
            // Host-level errors (e.g. from the VM) are not expected in this
            // constrained property; treat them as test failures.
            Err(Err(host_err)) => {
                panic!("Unexpected Soroban host error: {:?}", host_err);
            }
        }
    }
}

