#![cfg(test)]

use governance::{
    GovernanceContract, GovernanceContractClient, GovernanceError, ProposalKind, ProposalPage,
    ProposalStatus, VoteChoice,
};
use multisig::{MultisigContract, MultisigContractClient};
use rbac::{RbacContract, RbacContractClient, Role};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env, Symbol, Vec,
};
use withdrawal_timelock::{OperationStatus, WithdrawalTimelock, WithdrawalTimelockClient};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

struct TestContracts {
    governance: GovernanceContractClient<'static>,
    rbac: RbacContractClient<'static>,
    multisig: MultisigContractClient<'static>,
    timelock: WithdrawalTimelockClient<'static>,
    owner: Address,
    employer_a: Address,
    employer_b: Address,
    outsider: Address,
    signer_a: Address,
    signer_b: Address,
}

fn setup(env: &Env) -> TestContracts {
    #[allow(deprecated)]
    let governance_id = env.register_contract(None, GovernanceContract);
    #[allow(deprecated)]
    let rbac_id = env.register_contract(None, RbacContract);
    #[allow(deprecated)]
    let multisig_id = env.register_contract(None, MultisigContract);
    #[allow(deprecated)]
    let timelock_id = env.register_contract(None, WithdrawalTimelock);

    let governance = GovernanceContractClient::new(env, &governance_id);
    let rbac = RbacContractClient::new(env, &rbac_id);
    let multisig = MultisigContractClient::new(env, &multisig_id);
    let timelock = WithdrawalTimelockClient::new(env, &timelock_id);

    let owner = Address::generate(env);
    let employer_a = Address::generate(env);
    let employer_b = Address::generate(env);
    let outsider = Address::generate(env);
    let signer_a = Address::generate(env);
    let signer_b = Address::generate(env);

    rbac.initialize(&owner);
    rbac.grant_role(&owner, &employer_a, &Role::Employer);
    rbac.grant_role(&owner, &employer_b, &Role::Employer);

    let signers = Vec::from_array(env, [signer_a.clone(), signer_b.clone()]);
    multisig.initialize(&owner, &signers, &2u32, &None);

    timelock.initialize(&governance_id, &60u64);
    governance.initialize(
        &owner,
        &rbac_id,
        &multisig_id,
        &timelock_id,
        &2u32,
        &3600u64,
    );

    TestContracts {
        governance,
        rbac,
        multisig,
        timelock,
        owner,
        employer_a,
        employer_b,
        outsider,
        signer_a,
        signer_b,
    }
}

fn advance_time(env: &Env, seconds: u64) {
    env.ledger().with_mut(|ledger| {
        ledger.timestamp += seconds;
    });
}

#[test]
fn initialize_links_external_contracts() {
    let env = create_env();
    let setup = setup(&env);

    let (owner, rbac_id, multisig_id, timelock_id, quorum_votes, voting_period) =
        setup.governance.get_config();

    assert_eq!(owner, setup.owner);
    assert_eq!(rbac_id, setup.rbac.address);
    assert_eq!(multisig_id, setup.multisig.address);
    assert_eq!(timelock_id, setup.timelock.address);
    assert_eq!(quorum_votes, 2u32);
    assert_eq!(voting_period, 3600u64);
}

#[test]
fn employer_can_create_vote_finalize_and_multisig_signer_executes() {
    let env = create_env();
    let setup = setup(&env);

    let key = Symbol::new(&env, "withdraw_fee_bps");
    let proposal_id = setup.governance.create_proposal(
        &setup.employer_a,
        &ProposalKind::ParameterChange(key.clone(), 125i128),
    );

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_b, &proposal_id, &VoteChoice::For);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Succeeded);
    assert!(proposal.timelock_operation_id.is_some());
    assert!(proposal.eta.is_some());

    let timelock_op = setup
        .timelock
        .get_operation(&proposal.timelock_operation_id.unwrap())
        .unwrap();
    assert_eq!(timelock_op.status, OperationStatus::Queued);

    let early = setup
        .governance
        .try_execute_proposal(&setup.signer_a, &proposal_id);
    assert_eq!(early, Err(Ok(GovernanceError::TimelockNotReady)));

    advance_time(&env, 60);
    setup
        .governance
        .execute_proposal(&setup.signer_a, &proposal_id);

    let executed = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(executed.status, ProposalStatus::Executed);
    assert_eq!(setup.governance.get_parameter(&key).unwrap(), 125i128);

    let executed_timelock_op = setup
        .timelock
        .get_operation(&proposal.timelock_operation_id.unwrap())
        .unwrap();
    assert_eq!(executed_timelock_op.status, OperationStatus::Executed);
}

#[test]
fn backward_compatible_aliases_follow_full_lifecycle() {
    let env = create_env();
    let setup = setup(&env);
    let key = Symbol::new(&env, "alias_parameter");

    let proposal_id = setup.governance.propose(
        &setup.owner,
        &ProposalKind::ParameterChange(key.clone(), 42i128),
    );
    setup
        .governance
        .vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .vote(&setup.employer_a, &proposal_id, &VoteChoice::Abstain);

    assert_eq!(
        setup.governance.get_vote(&proposal_id, &setup.owner),
        Some(VoteChoice::For)
    );
    assert_eq!(
        setup.governance.get_vote(&proposal_id, &setup.employer_a),
        Some(VoteChoice::Abstain)
    );

    advance_time(&env, 3601);
    setup.governance.queue(&proposal_id);
    advance_time(&env, 60);
    setup.governance.execute(&setup.signer_a, &proposal_id);

    assert_eq!(
        setup.governance.get_proposal(&proposal_id).unwrap().status,
        ProposalStatus::Executed
    );
    assert_eq!(setup.governance.get_parameter(&key), Some(42i128));

    let cancelled_id = setup.governance.propose(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );
    setup.governance.cancel(&setup.owner, &cancelled_id);
    assert_eq!(
        setup.governance.get_proposal(&cancelled_id).unwrap().status,
        ProposalStatus::Cancelled
    );
}

#[test]
fn outsider_cannot_propose_or_vote() {
    let env = create_env();
    let setup = setup(&env);
    let kind = ProposalKind::ArbiterChange(Address::generate(&env));

    let proposal_res = setup.governance.try_create_proposal(&setup.outsider, &kind);
    assert_eq!(proposal_res, Err(Ok(GovernanceError::NotEligibleVoter)));

    let proposal_id = setup.governance.create_proposal(&setup.owner, &kind);
    let vote_res = setup
        .governance
        .try_cast_vote(&setup.outsider, &proposal_id, &VoteChoice::For);
    assert_eq!(vote_res, Err(Ok(GovernanceError::NotEligibleVoter)));
}

#[test]
fn double_vote_is_rejected() {
    let env = create_env();
    let setup = setup(&env);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );

    setup
        .governance
        .cast_vote(&setup.employer_a, &proposal_id, &VoteChoice::For);

    let second_vote =
        setup
            .governance
            .try_cast_vote(&setup.employer_a, &proposal_id, &VoteChoice::Against);
    assert_eq!(second_vote, Err(Ok(GovernanceError::AlreadyVoted)));
}

#[test]
fn proposal_is_defeated_without_quorum() {
    let env = create_env();
    let setup = setup(&env);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Defeated);
    assert!(proposal.timelock_operation_id.is_none());
}

#[test]
fn quorum_is_snapshotted_when_voting_power_changes_mid_vote() {
    let env = create_env();
    let setup = setup(&env);

    // This proposal captures the initial quorum of two votes.
    let existing_proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );
    assert_eq!(
        setup
            .governance
            .get_proposal(&existing_proposal_id)
            .unwrap()
            .quorum_votes,
        2u32
    );

    // Add a voter and raise the configured quorum while voting is active.
    // Neither change may retroactively alter the proposal's snapshot.
    setup
        .rbac
        .grant_role(&setup.owner, &setup.outsider, &Role::Employer);
    setup
        .governance
        .update_config(&setup.owner, &3u32, &3600u64);
    setup
        .governance
        .cast_vote(&setup.owner, &existing_proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_a, &existing_proposal_id, &VoteChoice::For);

    let still_open = setup
        .governance
        .try_finalize_proposal(&existing_proposal_id);
    assert_eq!(still_open, Err(Ok(GovernanceError::VotingStillOpen)));

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&existing_proposal_id);

    let existing_proposal = setup
        .governance
        .get_proposal(&existing_proposal_id)
        .unwrap();
    assert_eq!(existing_proposal.quorum_votes, 2u32);
    assert_eq!(existing_proposal.status, ProposalStatus::Succeeded);

    // A proposal created after the update captures the new quorum of three.
    let new_proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );
    assert_eq!(
        setup
            .governance
            .get_proposal(&new_proposal_id)
            .unwrap()
            .quorum_votes,
        3u32
    );

    // Lowering the global configuration mid-vote cannot make this proposal
    // pass with only one vote.
    setup
        .governance
        .cast_vote(&setup.owner, &new_proposal_id, &VoteChoice::For);
    setup
        .governance
        .update_config(&setup.owner, &1u32, &3600u64);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&new_proposal_id);

    let new_proposal = setup.governance.get_proposal(&new_proposal_id).unwrap();
    assert_eq!(new_proposal.quorum_votes, 3u32);
    assert_eq!(new_proposal.status, ProposalStatus::Defeated);
}

#[test]
fn proposal_is_defeated_when_against_votes_win() {
    let env = create_env();
    let setup = setup(&env);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::Against);
    setup
        .governance
        .cast_vote(&setup.employer_a, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_b, &proposal_id, &VoteChoice::Against);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Defeated);
}

#[test]
fn only_multisig_signer_can_execute() {
    let env = create_env();
    let setup = setup(&env);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_a, &proposal_id, &VoteChoice::For);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);
    advance_time(&env, 60);

    let res = setup
        .governance
        .try_execute_proposal(&setup.outsider, &proposal_id);
    assert_eq!(res, Err(Ok(GovernanceError::UnauthorizedExecutor)));
}

#[test]
fn canceling_succeeded_proposal_cancels_timelock_operation() {
    let env = create_env();
    let setup = setup(&env);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_a, &proposal_id, &VoteChoice::For);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    let op_id = proposal.timelock_operation_id.unwrap();

    setup.governance.cancel_proposal(&setup.owner, &proposal_id);

    let cancelled = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(cancelled.status, ProposalStatus::Cancelled);

    let timelock_op = setup.timelock.get_operation(&op_id).unwrap();
    assert_eq!(timelock_op.status, OperationStatus::Cancelled);
}

#[test]
fn upgrade_and_arbiter_proposals_apply_expected_state() {
    let env = create_env();
    let setup = setup(&env);

    let new_arbiter = Address::generate(&env);
    let arbiter_proposal = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(new_arbiter.clone()),
    );

    setup
        .governance
        .cast_vote(&setup.owner, &arbiter_proposal, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_a, &arbiter_proposal, &VoteChoice::For);
    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&arbiter_proposal);
    advance_time(&env, 60);
    setup
        .governance
        .execute_proposal(&setup.signer_b, &arbiter_proposal);

    assert_eq!(setup.governance.get_arbiter().unwrap(), new_arbiter);

    let target = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[9u8; 32]);
    let upgrade_proposal = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::UpgradeContract(target.clone(), wasm_hash.clone()),
    );

    setup
        .governance
        .cast_vote(&setup.owner, &upgrade_proposal, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_b, &upgrade_proposal, &VoteChoice::For);
    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&upgrade_proposal);
    advance_time(&env, 60);
    setup
        .governance
        .execute_proposal(&setup.signer_a, &upgrade_proposal);

    assert_eq!(
        setup.governance.get_approved_upgrade(&target).unwrap(),
        wasm_hash
    );
}

#[test]
fn losing_employer_role_blocks_future_votes() {
    let env = create_env();
    let setup = setup(&env);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );

    setup
        .rbac
        .revoke_role(&setup.owner, &setup.employer_a, &Role::Employer);

    let res = setup
        .governance
        .try_cast_vote(&setup.employer_a, &proposal_id, &VoteChoice::For);
    assert_eq!(res, Err(Ok(GovernanceError::NotEligibleVoter)));
}

#[test]
fn list_proposals_empty_set() {
    let env = create_env();
    let setup = setup(&env);

    let page: ProposalPage = setup.governance.list_proposals(&0, &10, &None);
    assert_eq!(page.proposals.len(), 0);
    assert!(page.next_cursor.is_none());
}

#[test]
fn list_proposals_basic_pagination() {
    let env = create_env();
    let setup = setup(&env);

    // Create 5 proposals
    let mut proposal_ids = Vec::new(&env);
    for _i in 0..5 {
        let id = setup.governance.create_proposal(
            &setup.owner,
            &ProposalKind::ArbiterChange(Address::generate(&env)),
        );
        proposal_ids.push_back(id);
    }

    // Fetch first page with limit 3
    let page: ProposalPage = setup.governance.list_proposals(&0, &3, &None);
    assert_eq!(page.proposals.len(), 3);
    assert_eq!(page.next_cursor, Some(3));

    // Verify proposals are in ascending order by ID
    assert_eq!(page.proposals.get(0).unwrap().id, 1);
    assert_eq!(page.proposals.get(1).unwrap().id, 2);
    assert_eq!(page.proposals.get(2).unwrap().id, 3);

    // Fetch second page using cursor
    let page2: ProposalPage =
        setup
            .governance
            .list_proposals(&page.next_cursor.unwrap(), &3, &None);
    assert_eq!(page2.proposals.len(), 2);
    assert_eq!(page2.proposals.get(0).unwrap().id, 4);
    assert_eq!(page2.proposals.get(1).unwrap().id, 5);
    assert!(page2.next_cursor.is_none());
}

#[test]
fn list_proposals_status_filter() {
    let env = create_env();
    let setup = setup(&env);

    // Create proposals with different statuses
    let active_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );

    let defeated_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );
    // Vote against to ensure defeat
    setup
        .governance
        .cast_vote(&setup.owner, &defeated_id, &VoteChoice::Against);
    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&defeated_id);

    let succeeded_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );
    setup
        .governance
        .cast_vote(&setup.owner, &succeeded_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_a, &succeeded_id, &VoteChoice::For);
    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&succeeded_id);

    // Filter by Active status
    let active_page: ProposalPage =
        setup
            .governance
            .list_proposals(&0, &10, &Some(ProposalStatus::Active));
    assert_eq!(active_page.proposals.len(), 1);
    assert_eq!(active_page.proposals.get(0).unwrap().id, active_id);

    // Filter by Defeated status
    let defeated_page: ProposalPage =
        setup
            .governance
            .list_proposals(&0, &10, &Some(ProposalStatus::Defeated));
    assert_eq!(defeated_page.proposals.len(), 1);
    assert_eq!(defeated_page.proposals.get(0).unwrap().id, defeated_id);

    // Filter by Succeeded status
    let succeeded_page: ProposalPage =
        setup
            .governance
            .list_proposals(&0, &10, &Some(ProposalStatus::Succeeded));
    assert_eq!(succeeded_page.proposals.len(), 1);
    assert_eq!(succeeded_page.proposals.get(0).unwrap().id, succeeded_id);

    // No filter returns all
    let all_page: ProposalPage = setup.governance.list_proposals(&0, &10, &None);
    assert_eq!(all_page.proposals.len(), 3);
}

#[test]
fn list_proposals_oversized_limit_clamped() {
    let env = create_env();
    let setup = setup(&env);

    // Create 10 proposals
    for _ in 0..10 {
        setup.governance.create_proposal(
            &setup.owner,
            &ProposalKind::ArbiterChange(Address::generate(&env)),
        );
    }

    // Request 100 proposals (should be clamped to MAX_PAGE_SIZE=50)
    let page: ProposalPage = setup.governance.list_proposals(&0, &100, &None);
    assert_eq!(page.proposals.len(), 10); // Only 10 exist
    assert!(page.next_cursor.is_none());
}

#[test]
fn list_proposals_cursor_resume_across_pages() {
    let env = create_env();
    let setup = setup(&env);

    // Create 15 proposals
    for _ in 0..15 {
        setup.governance.create_proposal(
            &setup.owner,
            &ProposalKind::ArbiterChange(Address::generate(&env)),
        );
    }

    // Page through all proposals with page size 5
    let mut all_proposals = Vec::new(&env);
    let mut cursor = Some(0u128);

    while let Some(start) = cursor {
        let page: ProposalPage = setup.governance.list_proposals(&start, &5, &None);
        for i in 0..page.proposals.len() {
            all_proposals.push_back(page.proposals.get(i).unwrap().clone());
        }
        cursor = page.next_cursor;
    }

    assert_eq!(all_proposals.len(), 15);

    // Verify all IDs are present and in order
    for i in 0..15 {
        assert_eq!(all_proposals.get(i).unwrap().id, (i + 1) as u128);
    }
}

#[test]
fn list_proposals_start_beyond_max_id() {
    let env = create_env();
    let setup = setup(&env);

    // Create 3 proposals
    for _ in 0..3 {
        setup.governance.create_proposal(
            &setup.owner,
            &ProposalKind::ArbiterChange(Address::generate(&env)),
        );
    }

    // Start from ID 100 (beyond the max)
    let page: ProposalPage = setup.governance.list_proposals(&100, &10, &None);
    assert_eq!(page.proposals.len(), 0);
    assert!(page.next_cursor.is_none());
}

#[test]
fn list_proposals_status_filter_with_pagination() {
    let env = create_env();
    let setup = setup(&env);

    // Create 5 active proposals
    let mut active_ids = Vec::new(&env);
    for _ in 0..5 {
        let id = setup.governance.create_proposal(
            &setup.owner,
            &ProposalKind::ArbiterChange(Address::generate(&env)),
        );
        active_ids.push_back(id);
    }

    // Create 3 defeated proposals
    for _ in 0..3 {
        let id = setup.governance.create_proposal(
            &setup.owner,
            &ProposalKind::ArbiterChange(Address::generate(&env)),
        );
        setup
            .governance
            .cast_vote(&setup.owner, &id, &VoteChoice::Against);
        advance_time(&env, 3601);
        setup.governance.finalize_proposal(&id);
    }

    // Page through active proposals with page size 2
    let mut active_proposals = Vec::new(&env);
    let mut cursor = Some(0u128);

    while let Some(start) = cursor {
        let page: ProposalPage =
            setup
                .governance
                .list_proposals(&start, &2, &Some(ProposalStatus::Active));
        for i in 0..page.proposals.len() {
            active_proposals.push_back(page.proposals.get(i).unwrap().clone());
        }
        cursor = page.next_cursor;
    }

    assert_eq!(active_proposals.len(), 5);
}

// ---------------------------------------------------------------------------
// Repeat-execution safety tests
// ---------------------------------------------------------------------------

/// Verifies that a second call to execute_proposal on an already-executed
/// proposal is rejected and side effects are not applied twice.
#[test]
fn repeat_execute_proposal_is_rejected_and_side_effect_applied_once() {
    let env = create_env();
    let setup = setup(&env);

    // Use a ParameterChange so we can verify the side effect value.
    let key = Symbol::new(&env, "withdraw_fee_bps");
    let expected_value: i128 = 125;
    let proposal_id = setup.governance.create_proposal(
        &setup.employer_a,
        &ProposalKind::ParameterChange(key.clone(), expected_value),
    );

    // Vote — two For votes reach quorum (quorum = 2)
    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_b, &proposal_id, &VoteChoice::For);

    // Finalize after voting ends
    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Succeeded);

    // Advance past the timelock delay (60s) and execute
    advance_time(&env, 60);
    setup
        .governance
        .execute_proposal(&setup.signer_a, &proposal_id);

    // Verify first execution succeeded
    let executed = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(executed.status, ProposalStatus::Executed);

    // Verify the side effect was applied once with the correct value
    assert_eq!(
        setup.governance.get_parameter(&key),
        Some(expected_value),
        "parameter must be set after the first execution"
    );

    // Second execution must be rejected — the proposal is already Executed,
    // not Succeeded.
    let second = setup
        .governance
        .try_execute_proposal(&setup.signer_a, &proposal_id);
    assert_eq!(
        second,
        Err(Ok(GovernanceError::ProposalNotSucceeded)),
        "repeat execute_proposal on an already-executed proposal must be rejected"
    );

    // Verify the side effect is unchanged — the payload was NOT applied twice.
    assert_eq!(
        setup.governance.get_parameter(&key),
        Some(expected_value),
        "parameter value must remain unchanged after rejected second execution"
    );
}

/// Verifies that re-executing an ArbiterChange proposal is also rejected and
/// the arbiter address is not overwritten.
#[test]
fn repeat_execute_arbiter_change_proposal_side_effect_applied_once() {
    let env = create_env();
    let setup = setup(&env);

    let new_arbiter = Address::generate(&env);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(new_arbiter.clone()),
    );

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_a, &proposal_id, &VoteChoice::For);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);
    advance_time(&env, 60);

    // First execution
    setup
        .governance
        .execute_proposal(&setup.signer_a, &proposal_id);
    assert_eq!(
        setup.governance.get_arbiter().unwrap(),
        new_arbiter,
        "arbiter must be set after first execution"
    );

    // Second execution must be rejected
    let second = setup
        .governance
        .try_execute_proposal(&setup.signer_b, &proposal_id);
    assert_eq!(second, Err(Ok(GovernanceError::ProposalNotSucceeded)));

    // Verify arbiter is unchanged
    assert_eq!(
        setup.governance.get_arbiter().unwrap(),
        new_arbiter,
        "arbiter must remain unchanged after rejected second execution"
    );
}

/// Verifies that even an unrelated caller (another signer) cannot re-execute
/// an already-executed proposal.
#[test]
fn different_signer_cannot_re_execute_proposal() {
    let env = create_env();
    let setup = setup(&env);

    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_a, &proposal_id, &VoteChoice::For);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);
    advance_time(&env, 60);

    // Execute as signer_a
    setup
        .governance
        .execute_proposal(&setup.signer_a, &proposal_id);

    // Different signer (signer_b) tries to execute again
    let second = setup
        .governance
        .try_execute_proposal(&setup.signer_b, &proposal_id);
    assert_eq!(
        second,
        Err(Ok(GovernanceError::ProposalNotSucceeded)),
        "even a different multisig signer cannot re-execute an executed proposal"
    );
}

// ---------------------------------------------------------------------------
// proposer_cancel_proposal tests
// ---------------------------------------------------------------------------

#[test]
fn proposer_cancels_successfully_pre_quorum() {
    let env = create_env();
    let setup = setup(&env);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );

    // Cast only 1 vote (quorum is 2)
    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);

    // Proposer cancels before quorum is reached
    setup
        .governance
        .proposer_cancel_proposal(&setup.owner, &proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Cancelled);
}

#[test]
fn non_proposer_cannot_cancel_proposal() {
    let env = create_env();
    let setup = setup(&env);
    let proposal_id = setup.governance.create_proposal(
        &setup.employer_a,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );

    let res = setup
        .governance
        .try_proposer_cancel_proposal(&setup.owner, &proposal_id);
    assert_eq!(res, Err(Ok(GovernanceError::NotOwner)));
}

#[test]
fn proposer_cannot_cancel_after_quorum_reached() {
    let env = create_env();
    let setup = setup(&env);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );

    // Cast enough votes to reach quorum (2 votes, quorum is 2)
    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_a, &proposal_id, &VoteChoice::For);

    // Proposer tries to cancel after quorum is reached
    let res = setup
        .governance
        .try_proposer_cancel_proposal(&setup.owner, &proposal_id);
    assert_eq!(res, Err(Ok(GovernanceError::ProposalNotCancellable)));
}

#[test]
fn proposer_cannot_cancel_already_cancelled_proposal() {
    let env = create_env();
    let setup = setup(&env);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );

    setup
        .governance
        .proposer_cancel_proposal(&setup.owner, &proposal_id);

    let res = setup
        .governance
        .try_proposer_cancel_proposal(&setup.owner, &proposal_id);
    assert_eq!(res, Err(Ok(GovernanceError::ProposalNotActive)));
}

#[test]
fn boundary_quorum_minus_one_allows_cancel() {
    let env = create_env();
    let setup = setup(&env);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );

    // Cast 1 vote when quorum is 2 (one short of quorum)
    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);

    // Should still be allowed to cancel
    setup
        .governance
        .proposer_cancel_proposal(&setup.owner, &proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Cancelled);
}

#[test]
fn exactly_at_quorum_rejects_cancel() {
    let env = create_env();
    let setup = setup(&env);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );

    // Cast exactly quorum votes (2 = quorum)
    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_a, &proposal_id, &VoteChoice::Against);

    // Exactly at quorum — cancellation must be rejected
    let res = setup
        .governance
        .try_proposer_cancel_proposal(&setup.owner, &proposal_id);
    assert_eq!(res, Err(Ok(GovernanceError::ProposalNotCancellable)));
}

#[test]
fn proposer_cannot_cancel_defeated_proposal() {
    let env = create_env();
    let setup = setup(&env);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::ArbiterChange(Address::generate(&env)),
    );

    // Cast only against votes so the proposal is defeated on finalization
    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::Against);
    setup
        .governance
        .cast_vote(&setup.employer_a, &proposal_id, &VoteChoice::Against);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let res = setup
        .governance
        .try_proposer_cancel_proposal(&setup.owner, &proposal_id);
    assert_eq!(res, Err(Ok(GovernanceError::ProposalNotActive)));
}

// ---------------------------------------------------------------------------
// timelock enforcement tests
// ---------------------------------------------------------------------------

#[test]
fn execution_rejected_one_second_before_timelock_elapses() {
    let env = create_env();
    let setup = setup(&env);
    let key = Symbol::new(&env, "timelock_test_param");

    let proposal_id = setup.governance.create_proposal(
        &setup.employer_a,
        &ProposalKind::ParameterChange(key.clone(), 999i128),
    );

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_b, &proposal_id, &VoteChoice::For);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Succeeded);
    assert!(proposal.eta.is_some());

    let eta = proposal.eta.unwrap();
    let current_time = env.ledger().timestamp();

    // Advance time to one second before the timelock elapses
    let time_to_advance = eta.saturating_sub(current_time).saturating_sub(1);
    advance_time(&env, time_to_advance);

    // Attempt execution one second before timelock elapses - should fail
    let early_execution = setup
        .governance
        .try_execute_proposal(&setup.signer_a, &proposal_id);
    assert_eq!(early_execution, Err(Ok(GovernanceError::TimelockNotReady)));

    // Verify proposal is still not executed
    let not_executed = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(not_executed.status, ProposalStatus::Succeeded);
    assert_eq!(setup.governance.get_parameter(&key), None);
}

#[test]
fn execution_succeeds_exactly_at_timelock_boundary() {
    let env = create_env();
    let setup = setup(&env);
    let key = Symbol::new(&env, "boundary_test_param");

    let proposal_id = setup.governance.create_proposal(
        &setup.employer_a,
        &ProposalKind::ParameterChange(key.clone(), 777i128),
    );

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_b, &proposal_id, &VoteChoice::For);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    let eta = proposal.eta.unwrap();
    let current_time = env.ledger().timestamp();

    // Advance time exactly to the timelock boundary
    let time_to_advance = eta.saturating_sub(current_time);
    advance_time(&env, time_to_advance);

    // Execution should succeed exactly at the boundary
    setup
        .governance
        .execute_proposal(&setup.signer_a, &proposal_id);

    let executed = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(executed.status, ProposalStatus::Executed);
    assert_eq!(setup.governance.get_parameter(&key).unwrap(), 777i128);
}

#[test]
fn execution_succeeds_after_timelock_boundary() {
    let env = create_env();
    let setup = setup(&env);
    let key = Symbol::new(&env, "after_boundary_param");

    let proposal_id = setup.governance.create_proposal(
        &setup.employer_a,
        &ProposalKind::ParameterChange(key.clone(), 555i128),
    );

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_b, &proposal_id, &VoteChoice::For);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    let eta = proposal.eta.unwrap();
    let current_time = env.ledger().timestamp();

    // Advance time past the timelock boundary by 10 seconds
    let time_to_advance = eta.saturating_sub(current_time).saturating_add(10);
    advance_time(&env, time_to_advance);

    // Execution should succeed after the boundary
    setup
        .governance
        .execute_proposal(&setup.signer_b, &proposal_id);

    let executed = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(executed.status, ProposalStatus::Executed);
    assert_eq!(setup.governance.get_parameter(&key).unwrap(), 555i128);
}
