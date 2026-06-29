//! Proptest fuzz harness for `payroll_escrow` fund / release / refund sequences.
//!
//! # Escrow conservation invariant
//!
//! For every agreement the contract must satisfy:
//!
//! ```text
//! total_funded == total_released + total_refunded + remaining_balance
//! ```
//!
//! Equivalently, cumulative outflow (`released + refunded`) must never exceed
//! cumulative deposits (`total_funded`). These tests generate randomized
//! operation sequences and assert the invariant after every successful step.

#![cfg(test)]

use payroll_escrow::{PayrollEscrowContract, PayrollEscrowContractClient};
use proptest::prelude::*;
use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

/// Number of proptest cases (override with `PROPTEST_CASES` in CI).
fn proptest_cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(32)
}

/// Tracks per-agreement escrow accounting for conservation checks.
#[derive(Clone, Debug, Default)]
struct EscrowLedger {
    total_funded: i128,
    total_released: i128,
    total_refunded: i128,
}

impl EscrowLedger {
    /// Asserts `funded == released + refunded + remaining` for the live balance.
    fn assert_conservation(
        &self,
        client: &PayrollEscrowContractClient<'_>,
        agreement_id: u128,
    ) -> Result<(), TestCaseError> {
        let remaining = client.get_agreement_balance(&agreement_id);
        let outflow = self
            .total_released
            .checked_add(self.total_refunded)
            .expect("outflow overflow in test ledger");
        let accounted = outflow
            .checked_add(remaining)
            .expect("accounted overflow in test ledger");
        prop_assert_eq!(
            self.total_funded,
            accounted,
            "conservation violated: funded={} released={} refunded={} remaining={}",
            self.total_funded,
            self.total_released,
            self.total_refunded,
            remaining
        );
        prop_assert!(
            outflow <= self.total_funded,
            "outflow {} exceeded funded {}",
            outflow,
            self.total_funded
        );
        Ok(())
    }
}

/// Deploys a token, initialized escrow, and mints `mint_amount` to `employer`.
fn setup_escrow(
    env: &Env,
    mint_amount: i128,
) -> (
    PayrollEscrowContractClient<'_>,
    TokenClient<'_>,
    Address,
    Address,
    Address,
) {
    env.mock_all_auths();

    let admin = Address::generate(env);
    let manager = Address::generate(env);
    let employer = Address::generate(env);
    let token_admin = Address::generate(env);

    let token_id = env.register_stellar_asset_contract(token_admin.clone());
    let token = TokenClient::new(env, &token_id);
    StellarAssetClient::new(env, &token_id).mint(&employer, &mint_amount);

    let contract_id = env.register_contract(None, PayrollEscrowContract);
    let client = PayrollEscrowContractClient::new(env, &contract_id);
    client.initialize(&admin, &token_id, &manager);

    (client, token, employer, manager, token_admin)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(proptest_cases()))]

    /// Randomized valid fund → release → refund sequences preserve conservation.
    #[test]
    fn prop_fund_release_refund_sequences_preserve_conservation(
        initial_fund in 1i128..=50_000i128,
        extra_funds in prop::collection::vec(1i128..=10_000i128, 0..=4),
        release_amounts in prop::collection::vec(1i128..=5_000i128, 0..=8),
        refund_after_releases in proptest::bool::ANY,
    ) {
        let env = Env::default();
        let agreement_id = 42u128;
        let (client, _token, employer, manager, _token_admin) =
            setup_escrow(&env, i128::MAX / 4);

        let mut ledger = EscrowLedger::default();
        let recipient = Address::generate(&env);

        client.fund_agreement(&employer, &agreement_id, &employer, &initial_fund);
        ledger.total_funded = initial_fund;
        ledger.assert_conservation(&client, agreement_id)?;

        for amount in extra_funds {
            client.fund_agreement(&employer, &agreement_id, &employer, &amount);
            ledger.total_funded = ledger
                .total_funded
                .checked_add(amount)
                .expect("funded overflow");
            ledger.assert_conservation(&client, agreement_id)?;
        }

        for amount in release_amounts {
            let balance = client.get_agreement_balance(&agreement_id);
            if balance == 0 {
                break;
            }
            let release_amt = amount.min(balance);
            client.release(&manager, &agreement_id, &recipient, &release_amt);
            ledger.total_released = ledger
                .total_released
                .checked_add(release_amt)
                .expect("released overflow");
            ledger.assert_conservation(&client, agreement_id)?;
        }

        if refund_after_releases && client.get_agreement_balance(&agreement_id) > 0 {
            let remaining = client.get_agreement_balance(&agreement_id);
            client.refund_remaining(&manager, &agreement_id);
            ledger.total_refunded = ledger
                .total_refunded
                .checked_add(remaining)
                .expect("refunded overflow");
            ledger.assert_conservation(&client, agreement_id)?;
            prop_assert_eq!(client.get_agreement_balance(&agreement_id), 0);
        }
    }

    /// Over-release attempts never mutate balances or break conservation.
    #[test]
    fn prop_over_release_is_rejected_without_state_drift(
        funded in 100i128..=10_000i128,
        excess in 1i128..=10_000i128,
    ) {
        let env = Env::default();
        let agreement_id = 7u128;
        let (client, _token, employer, manager, _) = setup_escrow(&env, funded * 2);
        let recipient = Address::generate(&env);

        client.fund_agreement(&employer, &agreement_id, &employer, &funded);
        let ledger = EscrowLedger {
            total_funded: funded,
            ..Default::default()
        };

        let release_amount = funded.checked_add(excess).expect("release overflow");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.release(&manager, &agreement_id, &recipient, &release_amount);
        }));
        prop_assert!(result.is_err(), "over-release must fail");

        ledger.assert_conservation(&client, agreement_id)?;
        prop_assert_eq!(client.get_agreement_balance(&agreement_id), funded);
    }

    /// Only the configured manager may release; unauthorized callers are rejected.
    #[test]
    fn prop_unauthorized_release_is_rejected(
        funded in 100i128..=10_000i128,
        release_amt in 1i128..=5_000i128,
    ) {
        let env = Env::default();
        let agreement_id = 9u128;
        let (client, _token, employer, _manager, _) = setup_escrow(&env, funded * 2);
        let intruder = Address::generate(&env);
        let recipient = Address::generate(&env);

        client.fund_agreement(&employer, &agreement_id, &employer, &funded);
        let release_amount = release_amt.min(funded);

        let ledger = EscrowLedger {
            total_funded: funded,
            ..Default::default()
        };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.release(&intruder, &agreement_id, &recipient, &release_amount);
        }));
        prop_assert!(result.is_err(), "unauthorized release must fail");

        ledger.assert_conservation(&client, agreement_id)?;
        prop_assert_eq!(client.get_agreement_balance(&agreement_id), funded);
    }

    /// Double refund and release-after-refund must not drain extra funds.
    #[test]
    fn prop_refund_then_release_or_second_refund_fails(
        funded in 200i128..=20_000i128,
        partial_release in 1i128..=10_000i128,
    ) {
        let env = Env::default();
        let agreement_id = 11u128;
        let (client, _token, employer, manager, _) = setup_escrow(&env, funded * 2);
        let recipient = Address::generate(&env);

        client.fund_agreement(&employer, &agreement_id, &employer, &funded);
        let release_amt = partial_release.min(funded - 1);
        client.release(&manager, &agreement_id, &recipient, &release_amt);

        let mut ledger = EscrowLedger {
            total_funded: funded,
            total_released: release_amt,
            ..Default::default()
        };
        ledger.assert_conservation(&client, agreement_id)?;

        let remaining = client.get_agreement_balance(&agreement_id);
        client.refund_remaining(&manager, &agreement_id);
        ledger.total_refunded = remaining;
        ledger.assert_conservation(&client, agreement_id)?;
        prop_assert_eq!(client.get_agreement_balance(&agreement_id), 0);

        let second_refund = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.refund_remaining(&manager, &agreement_id);
        }));
        prop_assert!(second_refund.is_err(), "double refund must fail");

        let release_after = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.release(&manager, &agreement_id, &recipient, &1);
        }));
        prop_assert!(release_after.is_err(), "release after refund must fail");

        ledger.assert_conservation(&client, agreement_id)?;
    }
}
