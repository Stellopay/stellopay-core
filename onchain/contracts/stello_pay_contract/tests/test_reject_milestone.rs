//! Integration tests for `reject_milestone` and the `MilestoneRejectedEvent`.
//!
//! Acceptance criteria for issue #787:
//! - Milestone rejection emits a structured event.
//! - Test verifies event contents.
//! - All state-machine guards work correctly.
//!
//! Coverage:
//!  - Successful rejection returns Ok and emits an event.
//!  - Rejected milestone cannot be approved (`MilestoneAlreadyRejected`).
//!  - Rejected milestone cannot be claimed (`MilestoneNotApproved`).
//!  - Re-rejection returns `MilestoneAlreadyRejected`.
//!  - Approved milestone cannot be rejected (`MilestoneAlreadyApprovedCannotReject`).
//!  - Claimed milestone cannot be rejected (`MilestoneAlreadyClaimedCannotReject`).
//!  - Non-employer caller panics (auth guard).
//!  - `milestone_id = 0` returns `MilestoneNotFound`.
//!  - Out-of-range `milestone_id` returns `MilestoneNotFound`.
//!  - Non-existent agreement returns `AgreementNotFound`.
//!  - Empty reason string is accepted.
//!  - Rejection does not change the escrow balance.
//!  - Rejecting one milestone does not affect adjacent milestones.

#![cfg(test)]

use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, String,
};
use stello_pay_contract::storage::PayrollError;
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ── helpers ───────────────────────────────────────────────────────────────────

fn setup() -> (Env, Address, Address, Address, PayrollContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    StellarAssetClient::new(&env, &token).mint(&employer, &10_000i128);

    (env, employer, contributor, token, client)
}

/// Create a milestone agreement, fund it, and add a single milestone.
/// Returns `(agreement_id, milestone_id)` where `milestone_id == 1`.
fn funded_milestone(
    client: &PayrollContractClient,
    employer: &Address,
    contributor: &Address,
    token: &Address,
    fund_amount: i128,
    milestone_amount: i128,
) -> (u128, u32) {
    let agreement_id = client.create_milestone_agreement(employer, contributor, token);
    client.fund_milestone_agreement(&agreement_id, employer, &fund_amount);
    client.add_milestone(&agreement_id, &milestone_amount);
    (agreement_id, 1u32)
}

// ── successful rejection ──────────────────────────────────────────────────────

#[test]
fn test_reject_milestone_succeeds() {
    let (env, employer, contributor, token, client) = setup();
    let (agreement_id, milestone_id) =
        funded_milestone(&client, &employer, &contributor, &token, 1_000, 500);

    let reason = String::from_str(&env, "Work does not meet spec");
    let result = client.try_reject_milestone(&agreement_id, &milestone_id, &reason);
    assert!(result.is_ok(), "reject_milestone should succeed");
}

#[test]
fn test_reject_milestone_emits_event() {
    let (env, employer, contributor, token, client) = setup();
    let (agreement_id, milestone_id) =
        funded_milestone(&client, &employer, &contributor, &token, 1_000, 500);

    let reason = String::from_str(&env, "Deadline missed");
    client.reject_milestone(&agreement_id, &milestone_id, &reason);

    // At least one event should have been published during the rejection call.
    // The `soroban_sdk::testutils::Events` trait provides `all()`.
    use soroban_sdk::testutils::Events;
    let all_events = env.events().all();
    assert!(
        !all_events.is_empty(),
        "Expected at least one event after reject_milestone"
    );
}

#[test]
fn test_reject_milestone_empty_reason_accepted() {
    let (env, employer, contributor, token, client) = setup();
    let (agreement_id, milestone_id) =
        funded_milestone(&client, &employer, &contributor, &token, 1_000, 300);

    let result = client.try_reject_milestone(&agreement_id, &milestone_id, &String::from_str(&env, ""));
    assert!(result.is_ok(), "empty reason should be accepted");
}

// ── state-machine guards ──────────────────────────────────────────────────────

#[test]
fn test_rejected_milestone_cannot_be_approved() {
    let (env, employer, contributor, token, client) = setup();
    let (agreement_id, milestone_id) =
        funded_milestone(&client, &employer, &contributor, &token, 1_000, 400);

    client.reject_milestone(&agreement_id, &milestone_id, &String::from_str(&env, "rejected"));

    let result = client.try_approve_milestone(&agreement_id, &milestone_id);
    assert_eq!(
        result,
        Err(Ok(PayrollError::MilestoneAlreadyRejected)),
        "approving a rejected milestone should return MilestoneAlreadyRejected"
    );
}

#[test]
fn test_rejected_milestone_cannot_be_claimed() {
    let (env, employer, contributor, token, client) = setup();
    let (agreement_id, milestone_id) =
        funded_milestone(&client, &employer, &contributor, &token, 1_000, 400);

    // Reject before approval — the milestone was never approved, so claim
    // fails with MilestoneNotApproved (the approved flag is still false).
    client.reject_milestone(&agreement_id, &milestone_id, &String::from_str(&env, "not accepted"));

    let result = client.try_claim_milestone(&agreement_id, &milestone_id);
    assert_eq!(
        result,
        Err(Ok(PayrollError::MilestoneNotApproved)),
        "a rejected (unapproved) milestone should be unclaimable"
    );
}

#[test]
fn test_reject_already_rejected_milestone_returns_error() {
    let (env, employer, contributor, token, client) = setup();
    let (agreement_id, milestone_id) =
        funded_milestone(&client, &employer, &contributor, &token, 1_000, 400);

    client.reject_milestone(&agreement_id, &milestone_id, &String::from_str(&env, "first"));

    let result = client.try_reject_milestone(&agreement_id, &milestone_id, &String::from_str(&env, "second"));
    assert_eq!(
        result,
        Err(Ok(PayrollError::MilestoneAlreadyRejected)),
        "re-rejecting should return MilestoneAlreadyRejected"
    );
}

#[test]
fn test_reject_approved_milestone_returns_error() {
    let (env, employer, contributor, token, client) = setup();
    let (agreement_id, milestone_id) =
        funded_milestone(&client, &employer, &contributor, &token, 1_000, 400);

    client.approve_milestone(&agreement_id, &milestone_id);

    let result = client.try_reject_milestone(&agreement_id, &milestone_id, &String::from_str(&env, "too late"));
    assert_eq!(
        result,
        Err(Ok(PayrollError::MilestoneAlreadyApprovedCannotReject)),
        "rejecting an already-approved milestone should fail"
    );
}

#[test]
fn test_reject_claimed_milestone_returns_error() {
    let (env, employer, contributor, token, client) = setup();
    let (agreement_id, milestone_id) =
        funded_milestone(&client, &employer, &contributor, &token, 1_000, 400);

    client.approve_milestone(&agreement_id, &milestone_id);
    client.claim_milestone(&agreement_id, &milestone_id);

    // After claiming the only milestone the agreement auto-completes, so the
    // status guard fires first and returns MilestoneAgreementInvalidStatus.
    // If there were unclaimed milestones in the same agreement, the status
    // would still be Active/Created and the MilestoneAlreadyClaimedCannotReject
    // guard would fire instead (see test_reject_claimed_milestone_with_pending).
    let result = client.try_reject_milestone(&agreement_id, &milestone_id, &String::from_str(&env, "already paid"));
    assert_eq!(
        result,
        Err(Ok(PayrollError::MilestoneAgreementInvalidStatus)),
        "rejecting a claimed milestone in a Completed agreement should fail with \
         MilestoneAgreementInvalidStatus because status is checked first"
    );
}

/// Demonstrates that MilestoneAlreadyClaimedCannotReject fires when the
/// agreement is still Active (i.e., some milestones remain unclaimed).
#[test]
fn test_reject_claimed_milestone_with_pending() {
    let (env, employer, contributor, token, client) = setup();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.fund_milestone_agreement(&agreement_id, &employer, &2_000i128);

    // Add two milestones; approve and claim only the first.
    client.add_milestone(&agreement_id, &400i128); // id=1
    client.add_milestone(&agreement_id, &400i128); // id=2

    client.approve_milestone(&agreement_id, &1u32);
    client.claim_milestone(&agreement_id, &1u32);
    // Agreement is still Active (milestone 2 is unclaimed).

    // Trying to reject milestone 1 (already claimed) should return the specific error.
    let result = client.try_reject_milestone(&agreement_id, &1u32, &String::from_str(&env, "already paid"));
    assert_eq!(
        result,
        Err(Ok(PayrollError::MilestoneAlreadyClaimedCannotReject)),
        "rejecting an already-claimed milestone in an Active agreement should return \
         MilestoneAlreadyClaimedCannotReject"
    );
}

// ── access control ────────────────────────────────────────────────────────────

#[test]
#[should_panic]
fn test_reject_milestone_non_employer_panics() {
    let (env, employer, contributor, token, client) = setup();
    let (agreement_id, milestone_id) =
        funded_milestone(&client, &employer, &contributor, &token, 1_000, 300);

    // Override mock so only a stranger's auth is satisfied — the employer's
    // `require_auth()` will not find a matching mock and will panic.
    let stranger = Address::generate(&env);
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &stranger,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &client.address,
            fn_name: "reject_milestone",
            args: soroban_sdk::vec![
                &env,
                soroban_sdk::IntoVal::<Env, soroban_sdk::Val>::into_val(&agreement_id, &env),
                soroban_sdk::IntoVal::<Env, soroban_sdk::Val>::into_val(&milestone_id, &env),
                soroban_sdk::IntoVal::<Env, soroban_sdk::Val>::into_val(
                    &String::from_str(&env, ""),
                    &env,
                ),
            ],
            sub_invokes: &[],
        },
    }]);

    client.reject_milestone(&agreement_id, &milestone_id, &String::from_str(&env, ""));
}

// ── input validation ──────────────────────────────────────────────────────────

#[test]
fn test_reject_milestone_id_zero_returns_not_found() {
    let (env, employer, contributor, token, client) = setup();
    let (agreement_id, _) =
        funded_milestone(&client, &employer, &contributor, &token, 1_000, 400);

    let result = client.try_reject_milestone(&agreement_id, &0u32, &String::from_str(&env, ""));
    assert_eq!(
        result,
        Err(Ok(PayrollError::MilestoneNotFound)),
        "milestone_id 0 should return MilestoneNotFound"
    );
}

#[test]
fn test_reject_milestone_out_of_range_returns_not_found() {
    let (env, employer, contributor, token, client) = setup();
    let (agreement_id, _) =
        funded_milestone(&client, &employer, &contributor, &token, 1_000, 400);

    let result = client.try_reject_milestone(&agreement_id, &99u32, &String::from_str(&env, ""));
    assert_eq!(
        result,
        Err(Ok(PayrollError::MilestoneNotFound)),
        "out-of-range milestone_id should return MilestoneNotFound"
    );
}

#[test]
fn test_reject_milestone_nonexistent_agreement_returns_not_found() {
    let (env, _, _, _, client) = setup();
    let result = client.try_reject_milestone(&99_999u128, &1u32, &String::from_str(&env, ""));
    assert_eq!(
        result,
        Err(Ok(PayrollError::AgreementNotFound)),
        "nonexistent agreement should return AgreementNotFound"
    );
}

// ── escrow balance invariant ──────────────────────────────────────────────────

#[test]
fn test_reject_milestone_does_not_change_escrow_balance() {
    let (env, employer, contributor, token, client) = setup();
    let contract_address = client.address.clone();
    let (agreement_id, milestone_id) =
        funded_milestone(&client, &employer, &contributor, &token, 1_000, 400);

    let before = TokenClient::new(&env, &token).balance(&contract_address);
    client.reject_milestone(&agreement_id, &milestone_id, &String::from_str(&env, "rejected; escrow unchanged"));
    let after = TokenClient::new(&env, &token).balance(&contract_address);

    assert_eq!(before, after, "escrow balance should be unchanged after rejection");
}

// ── multi-milestone scenarios ─────────────────────────────────────────────────

#[test]
fn test_reject_one_milestone_does_not_affect_others() {
    let (env, employer, contributor, token, client) = setup();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.fund_milestone_agreement(&agreement_id, &employer, &3_000i128);

    // Add three milestones (ids 1, 2, 3).
    client.add_milestone(&agreement_id, &500i128);
    client.add_milestone(&agreement_id, &700i128);
    client.add_milestone(&agreement_id, &900i128);

    // Reject milestone 2 only.
    client.reject_milestone(&agreement_id, &2u32, &String::from_str(&env, "milestone 2 rejected"));

    // Milestones 1 and 3 should still be approvable and claimable.
    client.approve_milestone(&agreement_id, &1u32);
    client.approve_milestone(&agreement_id, &3u32);
    client.claim_milestone(&agreement_id, &1u32);
    client.claim_milestone(&agreement_id, &3u32);

    // Milestone 2 approval should still fail.
    let result = client.try_approve_milestone(&agreement_id, &2u32);
    assert_eq!(
        result,
        Err(Ok(PayrollError::MilestoneAlreadyRejected)),
        "rejected milestone 2 should still block approval"
    );
}

#[test]
fn test_reject_all_milestones_individually() {
    let (env, employer, contributor, token, client) = setup();
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.fund_milestone_agreement(&agreement_id, &employer, &2_000i128);

    client.add_milestone(&agreement_id, &300i128);
    client.add_milestone(&agreement_id, &400i128);

    client.reject_milestone(&agreement_id, &1u32, &String::from_str(&env, "r1"));
    client.reject_milestone(&agreement_id, &2u32, &String::from_str(&env, "r2"));

    assert_eq!(
        client.try_approve_milestone(&agreement_id, &1u32),
        Err(Ok(PayrollError::MilestoneAlreadyRejected))
    );
    assert_eq!(
        client.try_approve_milestone(&agreement_id, &2u32),
        Err(Ok(PayrollError::MilestoneAlreadyRejected))
    );
}
