#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    token, Address, Env, Vec,
};
use stello_pay_contract::{storage::PayrollError, PayrollContract, PayrollContractClient};

fn create_token_contract<'a>(env: &Env, admin: &Address) -> token::StellarAssetClient<'a> {
    let contract_address = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    token::StellarAssetClient::new(env, &contract_address)
}

fn setup_contract(
    env: &Env,
) -> (
    PayrollContractClient<'_>,
    Address,
    Address,
    Address,
    Address,
) {
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(env, &contract_id);

    let owner = Address::generate(env);
    let guardian1 = Address::generate(env);
    let guardian2 = Address::generate(env);
    let guardian3 = Address::generate(env);

    client.initialize(&owner);

    (client, owner, guardian1, guardian2, guardian3)
}

#[test]
fn test_owner_can_emergency_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, _, _, _) = setup_contract(&env);

    // Initially not paused
    assert!(!client.is_emergency_paused());

    // Owner activates emergency pause
    client.emergency_pause();

    // Verify paused
    assert!(client.is_emergency_paused());

    let state = client.get_emergency_pause_state().unwrap();
    assert!(state.is_paused);
    assert!(state.paused_at.is_some());
}

#[test]
fn test_owner_can_unpause() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, _, _, _) = setup_contract(&env);

    // Pause
    client.emergency_pause();
    assert!(client.is_emergency_paused());

    // Unpause
    client.emergency_unpause();
    assert!(!client.is_emergency_paused());
}

#[test]
fn test_set_emergency_guardians() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);

    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());

    client.set_emergency_guardians(&guardians);

    let stored = client.get_emergency_guardians().unwrap();
    assert_eq!(stored.len(), 3);
}

#[test]
fn test_multisig_pause_proposal() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);

    // Set guardians
    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());
    client.set_emergency_guardians(&guardians);

    // Guardian1 proposes pause with no timelock
    client.propose_emergency_pause(&guardian1, &0);

    // Not paused yet
    assert!(!client.is_emergency_paused());
}

#[test]
fn test_propose_emergency_pause_from_uninitialized_state() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);
    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2);
    guardians.push_back(guardian3);
    client.set_emergency_guardians(&guardians);

    let result = client.try_propose_emergency_pause(&guardian1, &0);

    assert_eq!(result, Ok(Ok(())));
    assert_eq!(client.get_emergency_pause_state(), None);
    assert!(!client.is_emergency_paused());
}

#[test]
fn test_propose_emergency_pause_from_pending_state() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);
    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3);
    client.set_emergency_guardians(&guardians);

    assert_eq!(
        client.try_propose_emergency_pause(&guardian1, &3600),
        Ok(Ok(()))
    );
    let result = client.try_propose_emergency_pause(&guardian1, &0);

    assert_eq!(result, Ok(Ok(())));
    assert!(!client.is_emergency_paused());
    assert_eq!(client.try_approve_emergency_pause(&guardian2), Ok(Ok(())));
    assert!(client.is_emergency_paused());
}

#[test]
fn test_propose_emergency_pause_from_paused_terminal_state_rejects_non_guardian_without_mutation() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);
    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2);
    guardians.push_back(guardian3);
    client.set_emergency_guardians(&guardians);
    assert_eq!(client.try_emergency_pause(), Ok(Ok(())));

    let state_before = client.get_emergency_pause_state();
    let non_guardian = Address::generate(&env);
    let result = client.try_propose_emergency_pause(&non_guardian, &0);

    assert_eq!(result, Err(Ok(PayrollError::NotGuardian)));
    assert_eq!(client.get_emergency_pause_state(), state_before);
    assert!(client.is_emergency_paused());
}

#[test]
fn test_propose_emergency_pause_from_unpaused_recovery_state() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);
    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2);
    guardians.push_back(guardian3);
    client.set_emergency_guardians(&guardians);
    assert_eq!(client.try_emergency_pause(), Ok(Ok(())));
    assert_eq!(client.try_emergency_unpause(), Ok(Ok(())));

    let state_before = client.get_emergency_pause_state();
    let result = client.try_propose_emergency_pause(&guardian1, &0);

    assert_eq!(result, Ok(Ok(())));
    assert_eq!(client.get_emergency_pause_state(), state_before);
    assert!(!client.is_emergency_paused());
}

#[test]
fn test_multisig_pause_approval_threshold() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);

    // Set 3 guardians (threshold = 2)
    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());
    client.set_emergency_guardians(&guardians);

    // Guardian1 proposes
    client.propose_emergency_pause(&guardian1, &0);
    assert!(!client.is_emergency_paused());

    // Guardian2 approves - reaches threshold (2/3)
    client.approve_emergency_pause(&guardian2);

    // Should be paused now
    assert!(client.is_emergency_paused());
}

#[test]
fn test_timelock_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);

    // Set guardians
    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());
    client.set_emergency_guardians(&guardians);

    // Propose with 1 hour timelock
    let timelock = 3600u64;
    client.propose_emergency_pause(&guardian1, &timelock);

    // Approve immediately - should fail due to timelock
    let result = client.try_approve_emergency_pause(&guardian2);

    // Timelock prevents immediate activation
    assert!(result.is_err() || !client.is_emergency_paused());
}

#[test]
fn test_paused_blocks_claims() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, _, _, _) = setup_contract(&env);

    // Create token and agreement
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    // Create and activate payroll agreement
    let agreement_id = client.create_payroll_agreement(&employer, &token.address, &86400);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    client.activate_agreement(&agreement_id);

    // Mint tokens to contract
    token.mint(&client.address, &10000);

    // Emergency pause
    client.emergency_pause();

    // Attempt to claim should fail
    let result = client.try_claim_payroll(&employee, &agreement_id, &0);
    assert!(result.is_err());
}

#[test]
fn test_paused_blocks_milestone_claims() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, _, _, _) = setup_contract(&env);

    // Create token and milestone agreement
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);

    let agreement_id = client.create_milestone_agreement(
        &employer,
        &contributor,
        &token.address,
        &soroban_sdk::vec![&env, 1i128],
    );
    client.add_milestone(&agreement_id, &1000);

    // Fund the accounted escrow so the approve_milestone invariant passes.
    token.mint(&employer, &10000);
    client.fund_milestone_agreement(&agreement_id, &employer, &10000);

    client.approve_milestone(&agreement_id, &1);

    // Emergency pause
    client.emergency_pause();

    // Attempt to claim milestone should fail
    let result = client.try_claim_milestone(&agreement_id, &1);
    assert!(result.is_err());
}

#[test]
fn test_unpause_restores_functionality() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, _, _, _) = setup_contract(&env);

    // Create token and milestone agreement (simpler than payroll)
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);

    let agreement_id = client.create_milestone_agreement(
        &employer,
        &contributor,
        &token.address,
        &soroban_sdk::vec![&env, 1i128],
    );
    client.add_milestone(&agreement_id, &1000);

    // Fund the accounted escrow so the approve_milestone invariant passes.
    token.mint(&employer, &10000);
    client.fund_milestone_agreement(&agreement_id, &employer, &10000);

    client.approve_milestone(&agreement_id, &1);

    // Pause
    client.emergency_pause();

    // Verify claim fails
    assert!(client.try_claim_milestone(&agreement_id, &1).is_err());

    // Unpause
    client.emergency_unpause();

    // Claim should work now
    client.claim_milestone(&agreement_id, &1);

    // Verify milestone was claimed
    let milestone = client.get_milestone(&agreement_id, &1).unwrap();
    assert!(milestone.claimed);
}

#[test]
fn test_guardian_duplicate_approval_ignored() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, _) = setup_contract(&env);

    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    client.set_emergency_guardians(&guardians);

    // Guardian1 proposes
    client.propose_emergency_pause(&guardian1, &0);

    // Guardian1 tries to approve again (should be ignored)
    client.approve_emergency_pause(&guardian1);

    // Still not paused (needs 2/2)
    assert!(!client.is_emergency_paused());

    // Guardian2 approves
    client.approve_emergency_pause(&guardian2);

    // Now paused
    assert!(client.is_emergency_paused());
}

#[test]
fn test_pause_state_details() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, owner, _, _, _) = setup_contract(&env);

    // Pause
    client.emergency_pause();

    let state = client.get_emergency_pause_state().unwrap();
    assert!(state.is_paused);
    assert!(state.paused_at.is_some());
    assert_eq!(state.paused_by.unwrap(), owner);
    assert!(state.timelock_end.is_none());
}

#[test]
fn test_emergency_recovery_workflow() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);

    // Setup guardians
    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());
    client.set_emergency_guardians(&guardians);

    // Simulate security incident detection
    // Guardian1 proposes immediate pause
    client.propose_emergency_pause(&guardian1, &0);

    // Guardian2 confirms
    client.approve_emergency_pause(&guardian2);

    // Contract is now paused
    assert!(client.is_emergency_paused());

    // After incident resolved, owner unpauses
    client.emergency_unpause();
    assert!(!client.is_emergency_paused());
}

// ============================================================================
// Employer-scoped bulk pause / unpause
// ============================================================================

/// Helper: create an active payroll agreement so it is indexable via
/// `EmployerAgreements(employer)` and claimable.
fn create_active_payroll_agreement(
    env: &Env,
    client: &PayrollContractClient<'_>,
    employer: &Address,
    token: &token::StellarAssetClient<'_>,
    grace: u64,
) -> u128 {
    let aid = client.create_payroll_agreement(employer, &token.address, &grace);
    let employee = Address::generate(env);
    client.add_employee_to_agreement(&aid, &employee, &1000);
    client.activate_agreement(&aid);
    token.mint(&client.address, &10000);
    aid
}

#[test]
fn test_bulk_pause_all_active_agreements() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _, _, _, _) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let aid1 = create_active_payroll_agreement(&env, &client, &employer, &token, 86400);
    let aid2 = create_active_payroll_agreement(&env, &client, &employer, &token, 86400);
    let aid3 = create_active_payroll_agreement(&env, &client, &employer, &token, 86400);

    // Confirm all are Active
    for aid in [aid1, aid2, aid3] {
        let a = client.get_agreement(&aid).unwrap();
        assert_eq!(
            a.status,
            stello_pay_contract::storage::AgreementStatus::Active
        );
    }

    let paused = client.pause_employer_agreements(&employer);
    assert_eq!(paused, 3);

    // Confirm all are now Paused
    for aid in [aid1, aid2, aid3] {
        let a = client.get_agreement(&aid).unwrap();
        assert_eq!(
            a.status,
            stello_pay_contract::storage::AgreementStatus::Paused
        );
    }
}

#[test]
fn test_bulk_unpause_all_paused_agreements() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _, _, _, _) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let aid1 = create_active_payroll_agreement(&env, &client, &employer, &token, 86400);
    let aid2 = create_active_payroll_agreement(&env, &client, &employer, &token, 86400);

    // Pause individually to set up the test
    client.pause_agreement(&aid1);
    client.pause_agreement(&aid2);

    let unpaused = client.unpause_employer_agreements(&employer);
    assert_eq!(unpaused, 2);

    // Confirm all are Active again
    for aid in [aid1, aid2] {
        let a = client.get_agreement(&aid).unwrap();
        assert_eq!(
            a.status,
            stello_pay_contract::storage::AgreementStatus::Active
        );
    }
}

#[test]
fn test_bulk_pause_respects_employer_boundary() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _, _, _, _) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer_a = Address::generate(&env);
    let employer_b = Address::generate(&env);

    let aid_a = create_active_payroll_agreement(&env, &client, &employer_a, &token, 86400);
    let aid_b = create_active_payroll_agreement(&env, &client, &employer_b, &token, 86400);

    // Pause employer_a's agreements only
    let paused = client.pause_employer_agreements(&employer_a);
    assert_eq!(paused, 1);

    // employer_a's agreement is paused
    let a = client.get_agreement(&aid_a).unwrap();
    assert_eq!(
        a.status,
        stello_pay_contract::storage::AgreementStatus::Paused
    );

    // employer_b's agreement is still Active
    let b = client.get_agreement(&aid_b).unwrap();
    assert_eq!(
        b.status,
        stello_pay_contract::storage::AgreementStatus::Active
    );
}

#[test]
fn test_bulk_pause_cross_employer_auth_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _, _, _, _) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer_a = Address::generate(&env);
    let employer_b = Address::generate(&env);

    // Create agreements for both employers
    create_active_payroll_agreement(&env, &client, &employer_a, &token, 86400);
    create_active_payroll_agreement(&env, &client, &employer_b, &token, 86400);

    // Clear all auths — employer_b cannot auth as employer_a
    env.mock_auths(&[]);
    let result = client.try_pause_employer_agreements(&employer_a);
    assert!(result.is_err());
}

#[test]
fn test_bulk_pause_no_agreements_returns_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _, _, _, _) = setup_contract(&env);
    let employer = Address::generate(&env);

    let paused = client.pause_employer_agreements(&employer);
    assert_eq!(paused, 0);
}

#[test]
fn test_bulk_unpause_no_agreements_returns_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _, _, _, _) = setup_contract(&env);
    let employer = Address::generate(&env);

    let unpaused = client.unpause_employer_agreements(&employer);
    assert_eq!(unpaused, 0);
}

#[test]
fn test_bulk_pause_skips_non_active_agreements() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _, _, _, _) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    // Active agreement — will be paused
    let aid_active = create_active_payroll_agreement(&env, &client, &employer, &token, 86400);

    // Created (not yet activated) agreement — will be skipped
    let aid_created = client.create_payroll_agreement(&employer, &token.address, &86400);
    client.add_employee_to_agreement(&aid_created, &employee, &1000);
    // deliberately NOT activating

    // Cancelled agreement — will be skipped
    let aid_cancelled = create_active_payroll_agreement(&env, &client, &employer, &token, 86400);
    client.cancel_agreement(&aid_cancelled);

    let paused = client.pause_employer_agreements(&employer);
    assert_eq!(paused, 1);

    // Only the Active one changed
    let a = client.get_agreement(&aid_active).unwrap();
    assert_eq!(
        a.status,
        stello_pay_contract::storage::AgreementStatus::Paused
    );

    let c = client.get_agreement(&aid_created).unwrap();
    assert_eq!(
        c.status,
        stello_pay_contract::storage::AgreementStatus::Created
    );

    let cancelled = client.get_agreement(&aid_cancelled).unwrap();
    assert_eq!(
        cancelled.status,
        stello_pay_contract::storage::AgreementStatus::Cancelled
    );
}

#[test]
fn test_bulk_pause_milestone_agreements() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _, _, _, _) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);

    let aid1 = client.create_milestone_agreement(
        &employer,
        &contributor,
        &token.address,
        &soroban_sdk::vec![&env, 1_000i128],
    );
    client.add_milestone(&aid1, &1000);
    // Milestone agreements are Created by default — bulk pause also pauses Created milestone
    // agreements (since they can have approved, claimable milestones).

    let aid2 = client.create_milestone_agreement(
        &employer,
        &contributor,
        &token.address,
        &soroban_sdk::vec![&env, 1_000i128],
    );
    client.add_milestone(&aid2, &2000);

    let paused = client.pause_employer_agreements(&employer);
    assert_eq!(paused, 2);

    // Both milestone agreements should now be Paused
    use stello_pay_contract::storage::MilestoneKey;
    for aid in [aid1, aid2] {
        let status: stello_pay_contract::storage::AgreementStatus =
            env.as_contract(&client.address, || {
                env.storage()
                    .persistent()
                    .get(&MilestoneKey::Status(aid))
                    .unwrap()
            });
        assert_eq!(
            status,
            stello_pay_contract::storage::AgreementStatus::Paused
        );
    }
}

#[test]
fn test_bulk_unpause_milestone_agreements() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _, _, _, _) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);

    let aid = client.create_milestone_agreement(
        &employer,
        &contributor,
        &token.address,
        &soroban_sdk::vec![&env, 1_000i128],
    );
    client.add_milestone(&aid, &1000);

    // Use per-agreement pause first, then bulk-unpause
    client.pause_agreement(&aid);

    let unpaused = client.unpause_employer_agreements(&employer);
    assert_eq!(unpaused, 1);

    // Should be Active now
    use stello_pay_contract::storage::MilestoneKey;
    let status: stello_pay_contract::storage::AgreementStatus =
        env.as_contract(&client.address, || {
            env.storage()
                .persistent()
                .get(&MilestoneKey::Status(aid))
                .unwrap()
        });
    assert_eq!(
        status,
        stello_pay_contract::storage::AgreementStatus::Active
    );
}

#[test]
fn test_propose_emergency_pause_boundary_zero_and_one() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);
    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());
    client.set_emergency_guardians(&guardians);

    let start_ts = 1_000u64;
    env.ledger().with_mut(|li| li.timestamp = start_ts);

    // Boundary: 0 seconds (immediate pause, timelock_end is None)
    assert!(client.try_propose_emergency_pause(&guardian1, &0).is_ok());
    let pending_zero: stello_pay_contract::storage::EmergencyPause =
        env.as_contract(&client.address, || {
            env.storage()
                .persistent()
                .get(&stello_pay_contract::storage::StorageKey::PendingPause)
                .unwrap()
        });
    assert_eq!(pending_zero.timelock_end, None);
    assert!(!pending_zero.is_paused);

    // Boundary: 1 second delay (timelock_end is start_ts + 1)
    assert!(client.try_propose_emergency_pause(&guardian1, &1).is_ok());
    let pending_one: stello_pay_contract::storage::EmergencyPause =
        env.as_contract(&client.address, || {
            env.storage()
                .persistent()
                .get(&stello_pay_contract::storage::StorageKey::PendingPause)
                .unwrap()
        });
    assert_eq!(pending_one.timelock_end, Some(start_ts + 1));
    assert!(!pending_one.is_paused);
}

#[test]
fn test_propose_emergency_pause_boundary_maximum_accepted_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);
    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());
    client.set_emergency_guardians(&guardians);

    let start_ts = 500u64;
    env.ledger().with_mut(|li| li.timestamp = start_ts);

    // Maximum accepted value: timestamp + timelock == u64::MAX
    let max_accepted = u64::MAX - start_ts;
    let res = client.try_propose_emergency_pause(&guardian1, &max_accepted);
    assert!(res.is_ok(), "maximum accepted timelock value must succeed");

    let pending: stello_pay_contract::storage::EmergencyPause =
        env.as_contract(&client.address, || {
            env.storage()
                .persistent()
                .get(&stello_pay_contract::storage::StorageKey::PendingPause)
                .unwrap()
        });
    assert_eq!(pending.timelock_end, Some(u64::MAX));
}

#[test]
fn test_propose_emergency_pause_boundary_one_past_maximum_refused_typed_error() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);
    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());
    client.set_emergency_guardians(&guardians);

    let start_ts = 500u64;
    env.ledger().with_mut(|li| li.timestamp = start_ts);

    let max_accepted = u64::MAX - start_ts;
    let one_past_max = max_accepted + 1;

    // One past maximum: arithmetic overflow refused with typed error InvalidTimelock
    let res = client.try_propose_emergency_pause(&guardian1, &one_past_max);
    assert_eq!(
        res,
        Err(Ok(
            stello_pay_contract::storage::PayrollError::InvalidTimelock
        )),
        "one past the maximum must be refused with typed InvalidTimelock error without panicking"
    );

    // u64::MAX when timestamp > 0 also overflows and must be refused with typed error
    let res_max_u64 = client.try_propose_emergency_pause(&guardian1, &u64::MAX);
    assert_eq!(
        res_max_u64,
        Err(Ok(
            stello_pay_contract::storage::PayrollError::InvalidTimelock
        )),
        "u64::MAX with non-zero timestamp must be refused with typed InvalidTimelock error"
    );
}

// ============================================================================
// RETRY / DUPLICATE SUBMISSION (ISSUE #1370)
// ============================================================================

/// Reads the pending proposal and the approval list written by
/// `propose_emergency_pause`.
fn read_proposal_state(
    env: &Env,
    client: &PayrollContractClient<'_>,
) -> (
    Option<stello_pay_contract::storage::EmergencyPause>,
    Vec<Address>,
) {
    env.as_contract(&client.address, || {
        (
            env.storage()
                .persistent()
                .get(&stello_pay_contract::storage::StorageKey::PendingPause),
            env.storage()
                .persistent()
                .get(&stello_pay_contract::storage::StorageKey::PauseApprovals)
                .unwrap_or_else(|| Vec::new(env)),
        )
    })
}

/// Counts the events published by the payroll contract itself, ignoring
/// anything published by the SAC token or by other contracts in the test.
fn payroll_events(env: &Env, client: &PayrollContractClient<'_>) -> usize {
    env.events()
        .all()
        .iter()
        .filter(|event| event.0 == client.address)
        .count()
}

/// A client that retries `propose_emergency_pause` after a timeout resubmits
/// the identical proposal (#1370).  The second call must leave exactly the
/// state a single call leaves: the same pending proposal, the proposer
/// counted once in the approval list, no pause, no token movement and no
/// events for either call.
#[test]
fn test_propose_emergency_pause_called_twice_leaves_the_state_of_one_call() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);
    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());
    client.set_emergency_guardians(&guardians);

    // Escrowed funds that a duplicated proposal must never touch.
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    token.mint(&employer, &5_000);
    token.mint(&client.address, &2_500);

    let token_client = token::TokenClient::new(&env, &token.address);
    let balances_before = (
        token_client.balance(&employer),
        token_client.balance(&client.address),
        token_client.balance(&employee),
        token_client.balance(&guardian1),
    );

    let start_ts = 1_000u64;
    env.ledger().with_mut(|li| li.timestamp = start_ts);
    let timelock = 3_600u64;

    let events_before = payroll_events(&env, &client);
    assert_eq!(
        events_before, 0,
        "set-up must not publish any payroll contract event"
    );

    // First submission.
    assert_eq!(
        client.try_propose_emergency_pause(&guardian1, &timelock),
        Ok(Ok(()))
    );
    let events_after_first = payroll_events(&env, &client);
    let state_after_first = read_proposal_state(&env, &client);
    assert_eq!(
        events_after_first, events_before,
        "the first propose_emergency_pause must publish no events"
    );
    assert!(!client.is_emergency_paused());
    assert_eq!(client.get_emergency_pause_state(), None);

    // Retried submission: same guardian, same arguments, same ledger time.
    assert_eq!(
        client.try_propose_emergency_pause(&guardian1, &timelock),
        Ok(Ok(()))
    );
    let events_after_second = payroll_events(&env, &client);
    let state_after_second = read_proposal_state(&env, &client);
    assert_eq!(
        events_after_second, events_after_first,
        "the retried propose_emergency_pause must publish no events"
    );
    assert_eq!(
        state_after_second, state_after_first,
        "calling twice must leave exactly the state a single call leaves"
    );
    assert!(!client.is_emergency_paused());
    assert_eq!(client.get_emergency_pause_state(), None);

    let (pending, approvals) = state_after_first;
    let pending = pending.expect("the first call must leave a pending proposal");
    assert!(!pending.is_paused);
    assert_eq!(pending.paused_by, Some(guardian1.clone()));
    assert_eq!(
        pending.timelock_end,
        Some(start_ts + timelock),
        "the retry must not move the timelock deadline"
    );
    assert_eq!(
        approvals,
        soroban_sdk::vec![&env, guardian1.clone()],
        "the proposer must be counted once, never appended twice"
    );

    // Balances are identical after both calls.
    assert_eq!(
        (
            token_client.balance(&employer),
            token_client.balance(&client.address),
            token_client.balance(&employee),
            token_client.balance(&guardian1),
        ),
        balances_before,
        "a retried proposal must not move tokens"
    );

    // The retried proposal is still a single proposal: once the timelock has
    // elapsed, one approval from a second guardian reaches the 2-of-3
    // threshold and pauses, exactly as it would after a single proposal.
    env.ledger()
        .with_mut(|li| li.timestamp = start_ts + timelock);
    assert_eq!(client.try_approve_emergency_pause(&guardian2), Ok(Ok(())));
    assert!(client.is_emergency_paused());
}

/// A retry issued after the ledger has moved on re-bases `timelock_end` onto
/// the new timestamp and changes nothing else: the approval list is still
/// reset to the single proposer, the contract is still unpaused, no tokens
/// move and no events are published by either call.
#[test]
fn test_propose_emergency_pause_retry_at_later_ledger_time_rebases_timelock_only() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, guardian1, guardian2, guardian3) = setup_contract(&env);
    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());
    client.set_emergency_guardians(&guardians);

    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    token.mint(&employer, &4_000);
    token.mint(&client.address, &1_500);

    let token_client = token::TokenClient::new(&env, &token.address);
    let balances_before = (
        token_client.balance(&employer),
        token_client.balance(&client.address),
        token_client.balance(&guardian1),
    );

    let first_ts = 1_000u64;
    let timelock = 3_600u64;
    env.ledger().with_mut(|li| li.timestamp = first_ts);
    let events_before = payroll_events(&env, &client);

    assert_eq!(
        client.try_propose_emergency_pause(&guardian1, &timelock),
        Ok(Ok(()))
    );
    let events_after_first = payroll_events(&env, &client);
    assert_eq!(
        events_after_first, events_before,
        "the first propose_emergency_pause must publish no events"
    );

    // The retry lands one ledger period later.
    let retry_ts = first_ts + 500;
    env.ledger().with_mut(|li| li.timestamp = retry_ts);
    assert_eq!(
        client.try_propose_emergency_pause(&guardian1, &timelock),
        Ok(Ok(()))
    );

    let events_after_second = payroll_events(&env, &client);
    assert_eq!(
        events_after_second, events_after_first,
        "the retried propose_emergency_pause must publish no events"
    );

    let (pending, approvals) = read_proposal_state(&env, &client);
    let pending = pending.expect("the retry must leave a pending proposal");
    assert!(!pending.is_paused);
    assert_eq!(pending.paused_by, Some(guardian1.clone()));
    assert_eq!(
        pending.timelock_end,
        Some(retry_ts + timelock),
        "a retry re-bases the timelock deadline on the current ledger time"
    );
    assert_eq!(
        approvals,
        soroban_sdk::vec![&env, guardian1.clone()],
        "the approval list must hold the proposer exactly once"
    );
    assert!(!client.is_emergency_paused());
    assert_eq!(client.get_emergency_pause_state(), None);

    assert_eq!(
        (
            token_client.balance(&employer),
            token_client.balance(&client.address),
            token_client.balance(&guardian1),
        ),
        balances_before,
        "a retried proposal must not move tokens"
    );
}

/// Control for the event counter the two tests above rely on.  They assert
/// that neither call publishes anything; those assertions would hold even if
/// the filter never matched an event, so this test pins the measurement: an
/// event published by the payroll contract is counted, one published by any
/// other contract is not.
#[test]
fn test_payroll_events_counts_only_events_published_by_the_payroll_contract() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, _guardian1, _guardian2, _guardian3) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    token.mint(&Address::generate(&env), &1_000);

    assert_eq!(
        payroll_events(&env, &client),
        0,
        "setup events belong to the token contract and must not be counted"
    );

    env.as_contract(&client.address, || {
        env.events().publish((symbol_short!("probe"), 1u32), 1u32);
    });
    env.as_contract(&token.address, || {
        env.events().publish((symbol_short!("probe"), 2u32), 2u32);
    });

    assert_eq!(
        payroll_events(&env, &client),
        1,
        "only the event published by the payroll contract may be counted"
    );
    assert!(
        env.events().all().len() >= 2,
        "both events must be present in the log for the filter to be tested"
    );
}
