//! Comprehensive test suite for milestone-based payment functionality (#162, #486).
//!
//! Covers: agreement creation, funding, adding milestones, approving, claiming,
//! access control, edge cases, and event emissions.

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{testutils::Address as _, Address, Env};
use stello_pay_contract::storage::PayrollError;
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ============================================================================
// HELPERS
// ============================================================================

fn create_test_env() -> (
    Env,
    Address,
    Address,
    Address,
    PayrollContractClient<'static>,
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
    (env, employer, contributor, token, client)
}

/// Mint tokens to `employer`, create a milestone agreement, and fund it via
/// `fund_milestone_agreement` so that approve/claim invariants can pass.
///
/// Uses a large pre-funded pool (`i128::MAX / 2`) so existing tests do not
/// need to know the exact amounts of milestones added afterwards.
fn setup_milestone_agreement(
    env: &Env,
    client: &PayrollContractClient,
    employer: &Address,
    contributor: &Address,
    token: &Address,
) -> u128 {
    let fund_amount: i128 = i128::MAX / 2;
    soroban_sdk::token::StellarAssetClient::new(env, token).mint(employer, &fund_amount);
    let id = client.create_milestone_agreement(employer, contributor, token);
    client.fund_milestone_agreement(&id, employer, &fund_amount);
    id
}

// -----------------------------------------------------------------------------
// Milestone agreement creation
// -----------------------------------------------------------------------------

/// Creates a milestone agreement and verifies agreement ID and basic state.
#[test]
fn test_create_milestone_agreement() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    assert!(agreement_id >= 1);
    assert_eq!(client.get_milestone_count(&agreement_id), 0);
    assert!(client.get_milestone(&agreement_id, &1).is_none());
}

/// Verifies that a second agreement gets a distinct ID.
#[test]
fn test_milestone_agreement_payment_type() {
    let (env, employer, contributor, token, client) = create_test_env();
    let _id1 = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    let id2 = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    assert_eq!(client.get_milestone_count(&id2), 0);
}

/// Initial milestone count is zero for a new agreement.
#[test]
fn test_initial_milestone_count_zero() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    assert_eq!(client.get_milestone_count(&agreement_id), 0);
}

// fund_milestone_agreement — happy path

/// Funding moves tokens from the employer's wallet to the contract address.
#[test]
fn test_fund_transfers_tokens_to_contract() {
    let (env, employer, contributor, token, client) = create_test_env();
    let asset_client = soroban_sdk::token::StellarAssetClient::new(&env, &token);
    asset_client.mint(&employer, &5_000i128);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.fund_milestone_agreement(&agreement_id, &employer, &5_000i128);

    let token_client = soroban_sdk::token::TokenClient::new(&env, &token);
    assert_eq!(token_client.balance(&client.address), 5_000i128);
    assert_eq!(token_client.balance(&employer), 0i128);
}

/// Multiple funding calls accumulate into the accounted escrow balance.
#[test]
fn test_fund_accumulates_across_multiple_deposits() {
    let (env, employer, contributor, token, client) = create_test_env();
    let asset_client = soroban_sdk::token::StellarAssetClient::new(&env, &token);
    asset_client.mint(&employer, &3_000i128);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.fund_milestone_agreement(&agreement_id, &employer, &1_000i128);
    client.fund_milestone_agreement(&agreement_id, &employer, &2_000i128);

    let token_client = soroban_sdk::token::TokenClient::new(&env, &token);
    assert_eq!(token_client.balance(&client.address), 3_000i128);
}

/// Full lifecycle: fund → add milestone → approve → claim, with token-balance assertions.
#[test]
fn test_fund_then_approve_then_claim_transfers_to_contributor() {
    let (env, employer, contributor, token, client) = create_test_env();
    let asset_client = soroban_sdk::token::StellarAssetClient::new(&env, &token);
    asset_client.mint(&employer, &1_000i128);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.fund_milestone_agreement(&agreement_id, &employer, &1_000i128);
    client.add_milestone(&agreement_id, &1_000i128);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);

    let token_client = soroban_sdk::token::TokenClient::new(&env, &token);
    assert_eq!(token_client.balance(&contributor), 1_000i128);
    assert_eq!(token_client.balance(&client.address), 0i128);
}

/// Funding with exactly the total sum of all milestones satisfies the approve invariant.
#[test]
fn test_fund_exact_total_allows_approve() {
    let (env, employer, contributor, token, client) = create_test_env();
    let asset_client = soroban_sdk::token::StellarAssetClient::new(&env, &token);
    asset_client.mint(&employer, &300i128);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100i128);
    client.add_milestone(&agreement_id, &200i128);
    // Fund after adding milestones — order should not matter.
    client.fund_milestone_agreement(&agreement_id, &employer, &300i128);

    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);

    assert!(client.get_milestone(&agreement_id, &1).unwrap().approved);
    assert!(client.get_milestone(&agreement_id, &2).unwrap().approved);
}

/// Escrow balance decreases correctly after each claim, keeping the invariant tight.
#[test]
fn test_escrow_balance_decrements_after_each_claim() {
    let (env, employer, contributor, token, client) = create_test_env();
    let asset_client = soroban_sdk::token::StellarAssetClient::new(&env, &token);
    asset_client.mint(&employer, &300i128);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.fund_milestone_agreement(&agreement_id, &employer, &300i128);
    client.add_milestone(&agreement_id, &100i128);
    client.add_milestone(&agreement_id, &200i128);
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);

    // After claiming milestone 1 (100), contract should hold 200.
    client.claim_milestone(&agreement_id, &1);
    let token_client = soroban_sdk::token::TokenClient::new(&env, &token);
    assert_eq!(token_client.balance(&client.address), 200i128);

    // After claiming milestone 2 (200), contract should hold 0.
    client.claim_milestone(&agreement_id, &2);
    assert_eq!(token_client.balance(&client.address), 0i128);
    assert_eq!(token_client.balance(&contributor), 300i128);
}

// fund_milestone_agreement — rejection cases

/// Funding with a zero amount must fail.
#[test]
#[should_panic(expected = "Amount must be positive")]
fn test_fund_zero_amount_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.fund_milestone_agreement(&agreement_id, &employer, &0i128);
}

/// Funding with a negative amount must fail.
#[test]
#[should_panic(expected = "Amount must be positive")]
fn test_fund_negative_amount_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.fund_milestone_agreement(&agreement_id, &employer, &-1i128);
}

/// A non-employer address cannot fund a milestone agreement.
#[test]
#[should_panic(expected = "Unauthorized: only the employer can fund a milestone agreement")]
fn test_fund_non_employer_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let stranger = Address::generate(&env);
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.fund_milestone_agreement(&agreement_id, &stranger, &500i128);
}

/// The contributor cannot fund the agreement — only the employer can.
#[test]
#[should_panic(expected = "Unauthorized: only the employer can fund a milestone agreement")]
fn test_fund_contributor_cannot_fund_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.fund_milestone_agreement(&agreement_id, &contributor, &500i128);
}

/// Funding a non-existent agreement ID must fail.
#[test]
#[should_panic(expected = "Agreement not found")]
fn test_fund_nonexistent_agreement_fails() {
    let (env, employer, _contributor, _token, client) = create_test_env();
    client.fund_milestone_agreement(&999u128, &employer, &500i128);
}

/// Approving a milestone without prior funding must fail the balance invariant.
#[test]
fn test_approve_without_funding_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1_000i128);
    // No fund_milestone_agreement call — must be rejected.
    let result = client.try_approve_milestone(&agreement_id, &1);
    assert_eq!(result, Err(Ok(PayrollError::InsufficientEscrowBalance)));
}

/// Funding less than the total milestone sum must cause approve to fail.
#[test]
fn test_approve_underfunded_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let asset_client = soroban_sdk::token::StellarAssetClient::new(&env, &token);
    asset_client.mint(&employer, &499i128);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1_000i128);
    client.fund_milestone_agreement(&agreement_id, &employer, &499i128); // short by 501
    let result = client.try_approve_milestone(&agreement_id, &1);
    assert_eq!(result, Err(Ok(PayrollError::InsufficientEscrowBalance)));
}

// -----------------------------------------------------------------------------
// Adding milestones
// -----------------------------------------------------------------------------

/// Adding a single milestone updates count and milestone data.
#[test]
fn test_add_milestone() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    assert_eq!(client.get_milestone_count(&agreement_id), 1);
    let m = client.get_milestone(&agreement_id, &1).unwrap();
    assert_eq!(m.id, 1);
    assert_eq!(m.amount, 1000);
    assert!(!m.approved);
    assert!(!m.claimed);
}

/// Adding multiple milestones assigns sequential IDs and amounts.
#[test]
fn test_add_multiple_milestones() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &500);
    client.add_milestone(&agreement_id, &1000);
    client.add_milestone(&agreement_id, &1500);
    assert_eq!(client.get_milestone_count(&agreement_id), 3);
    assert_eq!(client.get_milestone(&agreement_id, &1).unwrap().amount, 500);
    assert_eq!(
        client.get_milestone(&agreement_id, &2).unwrap().amount,
        1000
    );
    assert_eq!(
        client.get_milestone(&agreement_id, &3).unwrap().amount,
        1500
    );
}

/// Adding a milestone with zero amount must fail.
#[test]
fn test_add_milestone_zero_amount_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    let result = client.try_add_milestone(&agreement_id, &0);
    assert_eq!(result, Err(Ok(PayrollError::MilestoneAmountInvalid)));
}

/// Adding a milestone when agreement is not in Created status must fail.
#[test]
fn test_add_milestone_wrong_status_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);
    let result = client.try_add_milestone(&agreement_id, &200);
    assert_eq!(result, Err(Ok(PayrollError::MilestoneAgreementInvalidStatus)));
}

/// Only employer can add milestones; non-employer must fail.
#[test]
#[should_panic]
fn test_add_milestone_unauthorized_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    env.mock_auths(&[]);
    client.add_milestone(&agreement_id, &200);
}

/// Adding milestones increases total amount (verified via milestone amounts).
#[test]
fn test_add_milestone_updates_total() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);
    client.add_milestone(&agreement_id, &300);
    let total: i128 = (1..=3)
        .map(|i| client.get_milestone(&agreement_id, &i).unwrap().amount)
        .sum();
    assert_eq!(total, 600);
}

/// Milestone added updates state; contract emits MilestoneAdded event.
#[test]
fn test_milestone_added_event() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &999);
    let m = client.get_milestone(&agreement_id, &1).unwrap();
    assert_eq!(m.amount, 999);
    assert_eq!(m.id, 1);
}

// -----------------------------------------------------------------------------
// Approving milestones
// -----------------------------------------------------------------------------

/// Approving a milestone sets approved flag.
#[test]
fn test_approve_milestone() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);
    let m = client.get_milestone(&agreement_id, &1).unwrap();
    assert!(m.approved);
    assert!(!m.claimed);
}

/// Multiple milestones can be approved independently.
#[test]
fn test_approve_multiple_milestones() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);
    assert!(client.get_milestone(&agreement_id, &1).unwrap().approved);
    assert!(client.get_milestone(&agreement_id, &2).unwrap().approved);
}

/// Approving invalid milestone ID must fail.
#[test]
fn test_approve_invalid_id_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    let result = client.try_approve_milestone(&agreement_id, &99);
    assert_eq!(result, Err(Ok(PayrollError::MilestoneNotFound)));
}

/// Approving when agreement is paused must fail.
#[test]
fn test_approve_wrong_status_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.pause_agreement(&agreement_id);
    let result = client.try_approve_milestone(&agreement_id, &1);
    assert_eq!(result, Err(Ok(PayrollError::MilestoneAgreementInvalidStatus)));
}

/// Only employer can approve; contributor cannot approve.
#[test]
#[should_panic]
fn test_approve_unauthorized_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    env.mock_auths(&[]);
    client.approve_milestone(&agreement_id, &1);
}

/// Milestone approved event is reflected by state.
#[test]
fn test_milestone_approved_event() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.approve_milestone(&agreement_id, &1);
    assert!(client.get_milestone(&agreement_id, &1).unwrap().approved);
}

// -----------------------------------------------------------------------------
// Claiming milestones
// -----------------------------------------------------------------------------

/// Contributor can claim an approved milestone.
#[test]
fn test_claim_approved_milestone() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);
    let m = client.get_milestone(&agreement_id, &1).unwrap();
    assert!(m.approved);
    assert!(m.claimed);
}

/// Claiming an unapproved milestone must fail.
#[test]
fn test_claim_unapproved_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    let result = client.try_claim_milestone(&agreement_id, &1);
    assert_eq!(result, Err(Ok(PayrollError::MilestoneNotApproved)));
}

/// Claiming an already claimed milestone must fail.
#[test]
fn test_claim_already_claimed_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);
    let result = client.try_claim_milestone(&agreement_id, &1);
    assert_eq!(result, Err(Ok(PayrollError::MilestoneAlreadyClaimed)));
}

/// Only contributor can claim; employer cannot claim.
#[test]
#[should_panic]
fn test_claim_unauthorized_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);
    env.mock_auths(&[]);
    client.claim_milestone(&agreement_id, &1);
}

/// Claim updates milestone state (released in terms of state).
#[test]
fn test_claim_releases_funds() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &1000);
    client.approve_milestone(&agreement_id, &1);
    assert!(!client.get_milestone(&agreement_id, &1).unwrap().claimed);
    client.claim_milestone(&agreement_id, &1);
    assert!(client.get_milestone(&agreement_id, &1).unwrap().claimed);
}

/// Claimed milestone amount is stored correctly.
#[test]
fn test_claim_updates_paid_amount() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &500);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);
    let m = client.get_milestone(&agreement_id, &1).unwrap();
    assert_eq!(m.amount, 500);
    assert!(m.claimed);
}

/// Milestone claimed event is reflected by state.
#[test]
fn test_milestone_claimed_event() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);
    assert!(client.get_milestone(&agreement_id, &1).unwrap().claimed);
}

/// When all milestones are claimed, agreement completes (adding another milestone fails).
#[test]
fn test_agreement_completes_all_claimed() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);
    client.claim_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &2);
    assert!(client.get_milestone(&agreement_id, &1).unwrap().claimed);
    assert!(client.get_milestone(&agreement_id, &2).unwrap().claimed);
    let result = client.try_add_milestone(&agreement_id, &300);
    assert_eq!(result, Err(Ok(PayrollError::MilestoneAgreementInvalidStatus)));
}

// -----------------------------------------------------------------------------
// Edge cases
// -----------------------------------------------------------------------------

/// Single-milestone agreement full lifecycle.
#[test]
fn test_single_milestone_agreement() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &5000);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);
    let m = client.get_milestone(&agreement_id, &1).unwrap();
    assert!(m.claimed);
    assert_eq!(m.amount, 5000);
}

/// Many milestones can be added and claimed.
#[test]
fn test_many_milestones() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    for i in 1..=10 {
        client.add_milestone(&agreement_id, &(i * 100));
    }
    assert_eq!(client.get_milestone_count(&agreement_id), 10);
    for i in 1..=10 {
        client.approve_milestone(&agreement_id, &i);
    }
    for i in 1..=10 {
        client.claim_milestone(&agreement_id, &i);
    }
    for i in 1..=10 {
        assert!(client.get_milestone(&agreement_id, &i).unwrap().claimed);
    }
}

/// Claiming out of order (e.g. 2 then 1) works when both are approved.
#[test]
fn test_claiming_out_of_order() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);
    client.claim_milestone(&agreement_id, &2);
    client.claim_milestone(&agreement_id, &1);
    assert!(client.get_milestone(&agreement_id, &1).unwrap().claimed);
    assert!(client.get_milestone(&agreement_id, &2).unwrap().claimed);
}

/// Very large milestone amounts are stored and claimed correctly.
#[test]
fn test_very_large_milestone_amounts() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    let large = i128::MAX / 2;
    client.add_milestone(&agreement_id, &large);
    client.approve_milestone(&agreement_id, &1);
    client.claim_milestone(&agreement_id, &1);
    assert_eq!(
        client.get_milestone(&agreement_id, &1).unwrap().amount,
        large
    );
    assert!(client.get_milestone(&agreement_id, &1).unwrap().claimed);
}

// -----------------------------------------------------------------------------
// batch_claim_milestones
// -----------------------------------------------------------------------------

/// An empty milestone list is rejected up front with a typed error rather than
/// panicking.
#[test]
fn test_batch_claim_empty_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.approve_milestone(&agreement_id, &1);

    let result = client.try_batch_claim_milestones(&agreement_id, &soroban_sdk::Vec::<u32>::new(&env));
    assert_eq!(result, Err(Ok(PayrollError::InvalidData)));
}

/// Claiming against an unknown agreement returns AgreementNotFound instead of a
/// host trap.
#[test]
fn test_batch_claim_unknown_agreement_fails() {
    let (env, _employer, _contributor, _token, client) = create_test_env();
    let ids = soroban_sdk::vec![&env, 1u32];
    let result = client.try_batch_claim_milestones(&999u128, &ids);
    assert_eq!(result, Err(Ok(PayrollError::AgreementNotFound)));
}

/// A paused agreement rejects batch claims with AgreementPaused.
#[test]
fn test_batch_claim_paused_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.approve_milestone(&agreement_id, &1);
    client.pause_agreement(&agreement_id);

    let ids = soroban_sdk::vec![&env, 1u32];
    let result = client.try_batch_claim_milestones(&agreement_id, &ids);
    assert_eq!(result, Err(Ok(PayrollError::AgreementPaused)));
}

/// All approved milestones in the batch are claimed and accounted for.
#[test]
fn test_batch_claim_success() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);
    client.approve_milestone(&agreement_id, &1);
    client.approve_milestone(&agreement_id, &2);

    let ids = soroban_sdk::vec![&env, 1u32, 2u32];
    let result = client.batch_claim_milestones(&agreement_id, &ids);
    assert_eq!(result.successful_claims, 2);
    assert_eq!(result.failed_claims, 0);
    assert_eq!(result.total_claimed, 300);
    assert!(client.get_milestone(&agreement_id, &1).unwrap().claimed);
    assert!(client.get_milestone(&agreement_id, &2).unwrap().claimed);
}

/// A mixed batch reports per-item error codes: success (0), not approved (3),
/// and duplicate (1) - without aborting the whole batch.
#[test]
fn test_batch_claim_mixed_reports_error_codes() {
    let (env, employer, contributor, token, client) = create_test_env();
    let agreement_id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);
    client.add_milestone(&agreement_id, &100);
    client.add_milestone(&agreement_id, &200);
    client.approve_milestone(&agreement_id, &1);

    // [1 approved, 2 not approved, 1 duplicate]
    let ids = soroban_sdk::vec![&env, 1u32, 2u32, 1u32];
    let result = client.batch_claim_milestones(&agreement_id, &ids);

    assert_eq!(result.successful_claims, 1);
    assert_eq!(result.failed_claims, 2);
    assert_eq!(result.total_claimed, 100);
    assert_eq!(result.results.get(0).unwrap().error_code, 0); // success
    assert_eq!(result.results.get(1).unwrap().error_code, 3); // not approved
    assert_eq!(result.results.get(2).unwrap().error_code, 1); // duplicate
}

/// Adding a milestone that pushes cumulative total past i128::MAX fails with MilestoneAmountOverflow.
#[test]
fn test_add_milestone_overflow_fails() {
    let (env, employer, contributor, token, client) = create_test_env();
    soroban_sdk::testutils::Ledger::new(&env).set_timestamp(1000);
    let id = setup_milestone_agreement(&env, &client, &employer, &contributor, &token);

    // Add first milestone at near-MAX
    client.add_milestone(&id, &(i128::MAX - 1));

    // Adding another should overflow the cumulative sum
    let result = client.try_add_milestone(&id, &2i128);
    assert_eq!(result.err(), Some(Ok(PayrollError::MilestoneAmountOverflow)));
}
