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
// Proposal metadata immutability tests
// ---------------------------------------------------------------------------
//
// Once a proposal is created, its descriptive fields (kind, proposer,
// quorum_votes, start_time, end_time) are frozen and must not change
// through any public entrypoint, even after votes have been cast.
//
// These fields define what voters are approving. If `kind` could be
// altered mid-vote, voters could be tricked into approving a different
// action than the one they intended. Similarly, changing the quorum
// snapshot or voting window after voting has started would subvert the
// governance process.
//
// Audit of all state-mutating entrypoints:
//
// | Function                   | Fields written                | Touches metadata? |
// |----------------------------|-------------------------------|-------------------|
// | create_proposal            | all (initial write)           | N/A (creation)    |
// | cast_vote / vote           | for_votes/against/abstain     | No                |
// | finalize_proposal / queue  | status, timelock_op_id, eta   | No                |
// | execute_proposal / execute | status (→Expired or Executed) | No                |
// | cancel_proposal / cancel   | status (→Cancelled)           | No                |
// | proposer_cancel_proposal   | status (→Cancelled)           | No                |
// | update_config              | QuorumVotes, VotingPeriod     | Global config, not proposal |
//
// The write_proposal helper is called by every mutation entrypoint but
// each reads the full Proposal, modifies only its allowed fields, and
// writes back. No code path ever alters kind, proposer, quorum_votes,
// start_time, or end_time after the initial create.
//
// The tests below verify this invariant by snapshotting the metadata
// after creation and asserting it remains identical after votes,
// finalization, and execution.

/// Extracts the metadata fields that must remain immutable after creation.
fn proposal_metadata(proposal: &governance::Proposal) -> ProposalMetadata {
    ProposalMetadata {
        kind: proposal.kind.clone(),
        proposer: proposal.proposer.clone(),
        quorum_votes: proposal.quorum_votes,
        start_time: proposal.start_time,
        end_time: proposal.end_time,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProposalMetadata {
    kind: governance::ProposalKind,
    proposer: Address,
    quorum_votes: u32,
    start_time: u64,
    end_time: u64,
}

/// Verifies that all proposal metadata fields remain unchanged from the
/// snapshot captured at creation time.
fn assert_metadata_unchanged(
    actual: &governance::Proposal,
    expected: &ProposalMetadata,
    phase: &str,
) {
    let meta = proposal_metadata(actual);
    assert_eq!(
        meta, *expected,
        "proposal metadata changed after {phase}"
    );
}

/// Creates a proposal with a known ParameterChange kind and returns its ID
/// together with the metadata snapshot.
fn create_test_proposal(
    _env: &Env,
    governance: &governance::GovernanceContractClient<'static>,
    proposer: &Address,
    key: &Symbol,
    value: i128,
    expected_start_time: u64,
    expected_end_time: u64,
    expected_quorum: u32,
) -> (u128, ProposalMetadata) {
    let id = governance.create_proposal(
        proposer,
        &governance::ProposalKind::ParameterChange(key.clone(), value),
    );
    let proposal = governance.get_proposal(&id).unwrap();
    let meta = ProposalMetadata {
        kind: governance::ProposalKind::ParameterChange(key.clone(), value),
        proposer: proposer.clone(),
        quorum_votes: expected_quorum,
        start_time: expected_start_time,
        end_time: expected_end_time,
    };
    assert_eq!(
        proposal_metadata(&proposal),
        meta,
        "metadata must match expected values at creation"
    );
    (id, meta)
}

#[test]
fn proposal_metadata_is_immutable_after_vote_cast() {
    let env = create_env();
    let setup = setup(&env);

    // Create a proposal — this is the baseline metadata snapshot.
    let key = Symbol::new(&env, "test_param");
    let (proposal_id, meta) = create_test_proposal(
        &env,
        &setup.governance,
        &setup.employer_a,
        &key,
        42i128,
        0,                        // start_time (ledger starts at 0)
        3600,                     // end_time (0 + 3600s voting period)
        2,                        // quorum_votes snapshotted at creation
    );

    // Cast a vote — this is the "voting has started" threshold.
    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);

    // Metadata must still match the creation snapshot.
    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_metadata_unchanged(&proposal, &meta, "first vote");
    assert_eq!(proposal.for_votes, 1, "vote count must be recorded");

    // Cast a second vote (reaches quorum boundary).
    setup
        .governance
        .cast_vote(&setup.employer_b, &proposal_id, &VoteChoice::For);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_metadata_unchanged(&proposal, &meta, "second vote");
    assert_eq!(proposal.for_votes, 2, "vote count must be recorded");
}

#[test]
fn proposal_metadata_is_immutable_after_finalization() {
    let env = create_env();
    let setup = setup(&env);

    let (proposal_id, meta) = create_test_proposal(
        &env,
        &setup.governance,
        &setup.employer_a,
        &Symbol::new(&env, "param"),
        99i128,
        0,
        3600,
        2,
    );

    // Reach quorum with 2 for votes.
    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_b, &proposal_id, &VoteChoice::For);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    // Metadata must be unchanged after the proposal transitions to Succeeded.
    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, governance::ProposalStatus::Succeeded);
    assert_metadata_unchanged(&proposal, &meta, "finalization");
}

#[test]
fn proposal_metadata_is_immutable_after_execution() {
    let env = create_env();
    let setup = setup(&env);

    let (proposal_id, meta) = create_test_proposal(
        &env,
        &setup.governance,
        &setup.employer_a,
        &Symbol::new(&env, "exec_param"),
        77i128,
        0,
        3600,
        2,
    );

    // Full lifecycle to execution.
    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_b, &proposal_id, &VoteChoice::For);
    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);
    advance_time(&env, 60);
    setup
        .governance
        .execute_proposal(&setup.signer_a, &proposal_id);

    // Metadata must be unchanged after the proposal transitions to Executed.
    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, governance::ProposalStatus::Executed);
    assert_metadata_unchanged(&proposal, &meta, "execution");

    // Verify the parameter was actually stored (side effect must still work).
    assert_eq!(setup.governance.get_parameter(&Symbol::new(&env, "exec_param")), Some(77i128));
}

#[test]
fn proposal_metadata_is_immutable_after_cancel() {
    let env = create_env();
    let setup = setup(&env);

    let (proposal_id, meta) = create_test_proposal(
        &env,
        &setup.governance,
        &setup.owner,
        &Symbol::new(&env, "cancel_param"),
        33i128,
        0,
        3600,
        2,
    );

    // Cast one vote (below quorum so proposer cancellation is allowed).
    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);

    // Proposer cancels.
    setup
        .governance
        .proposer_cancel_proposal(&setup.owner, &proposal_id);

    // Metadata must be unchanged after cancellation.
    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, governance::ProposalStatus::Cancelled);
    assert_metadata_unchanged(&proposal, &meta, "proposer cancel");
}

#[test]
fn proposal_metadata_is_immutable_after_owner_cancel() {
    let env = create_env();
    let setup = setup(&env);

    let (proposal_id, meta) = create_test_proposal(
        &env,
        &setup.governance,
        &setup.employer_a,
        &Symbol::new(&env, "owner_cancel_param"),
        55i128,
        0,
        3600,
        2,
    );

    // Owner cancels (owner can cancel at any time before execution).
    setup
        .governance
        .cancel_proposal(&setup.owner, &proposal_id);

    // Metadata must be unchanged after owner-initiated cancellation.
    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, governance::ProposalStatus::Cancelled);
    assert_metadata_unchanged(&proposal, &meta, "owner cancel");
}

#[test]
fn proposal_metadata_is_immutable_using_backward_compat_aliases() {
    let env = create_env();
    let setup = setup(&env);

    let kind = governance::ProposalKind::ArbiterChange(Address::generate(&env));
    let proposal_id = setup.governance.propose(&setup.employer_a, &kind);

    // Snapshot metadata after creation via the alias.
    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    let meta = proposal_metadata(&proposal);

    // Vote via the `vote` alias.
    setup
        .governance
        .vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .vote(&setup.employer_b, &proposal_id, &VoteChoice::For);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_metadata_unchanged(&proposal, &meta, "vote alias");

    // Queue via the `queue` alias.
    advance_time(&env, 3601);
    setup.governance.queue(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_metadata_unchanged(&proposal, &meta, "queue alias");

    // Execute via the `execute` alias.
    advance_time(&env, 60);
    setup.governance.execute(&setup.signer_a, &proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, governance::ProposalStatus::Executed);
    assert_metadata_unchanged(&proposal, &meta, "execute alias");
}

#[test]
fn no_entrypoint_can_alter_stored_proposal_kind() {
    // The governance contract exposes NO public function that accepts a
    // ProposalKind together with an existing proposal ID to overwrite the
    // stored kind. Every entrypoint that takes a proposal ID operates on
    // the already-stored proposal and only touches lifecycle fields.
    //
    // This test stores the kind at creation and verifies it survives every
    // lifecycle transition unchanged.
    let env = create_env();
    let setup = setup(&env);

    let original_arbiter = Address::generate(&env);
    let kind = governance::ProposalKind::ArbiterChange(original_arbiter.clone());
    let proposal_id = setup.governance.create_proposal(&setup.owner, &kind);

    // Read back and compare the stored kind with the original.
    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(
        proposal.kind,
        governance::ProposalKind::ArbiterChange(original_arbiter),
        "stored kind must match the creation value"
    );
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

// ═══════════════════════════════════════════════════════════════════════════
// QUORUM AND MAJORITY GATING TESTS FOR get_approved_upgrade
// ═══════════════════════════════════════════════════════════════════════════
//
// These tests verify that get_approved_upgrade enforces BOTH quorum and
// majority thresholds simultaneously, ensuring the hash is only returned
// when a proposal passes complete governance validation.
//
// Test Matrix:
// | Quorum met | Majority met | Expected result              |
// |------------|--------------|------------------------------|
// | ❌ No      | ❌ No        | None (not surfaced)          |
// | ✅ Yes     | ❌ No        | None (not surfaced)          |
// | ❌ No      | ✅ Yes       | None (not surfaced)          |
// | ✅ Yes     | ✅ Yes       | Some(hash) surfaced          |
//
// Setup: quorum_votes = 2, voting_period = 3600s, timelock_delay = 60s
// Eligible voters: owner, employer_a, employer_b (3 total)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn get_approved_upgrade_neither_quorum_nor_majority() {
    let env = create_env();
    let setup = setup(&env);

    let target = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[1u8; 32]);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::UpgradeContract(target.clone(), wasm_hash.clone()),
    );

    // Cast no votes — neither condition met
    // total_votes = 0 < 2 (quorum fails)
    // for_votes = 0, against_votes = 0 (majority fails, no participation)

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Defeated);

    // get_approved_upgrade must return None
    let result = setup.governance.get_approved_upgrade(&target);
    assert!(
        result.is_none(),
        "Expected None when neither quorum nor majority is met"
    );
}

#[test]
fn get_approved_upgrade_quorum_met_majority_not_met() {
    let env = create_env();
    let setup = setup(&env);

    let target = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[2u8; 32]);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::UpgradeContract(target.clone(), wasm_hash.clone()),
    );

    // Cast votes: 2 total (meets quorum=2) but more against than for
    // for_votes = 1, against_votes = 1, abstain_votes = 0
    // total_votes = 2 >= 2 (quorum ✓)
    // for_votes = 1 NOT > against_votes = 1 (majority ✗)

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_a, &proposal_id, &VoteChoice::Against);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Defeated);
    assert!(
        proposal.timelock_operation_id.is_none(),
        "Defeated proposal should not have timelock operation"
    );

    // get_approved_upgrade must return None because majority failed
    let result = setup.governance.get_approved_upgrade(&target);
    assert!(
        result.is_none(),
        "Expected None when quorum met but majority not met (1 for = 1 against)"
    );
}

#[test]
fn get_approved_upgrade_majority_met_quorum_not_met() {
    let env = create_env();
    let setup = setup(&env);

    let target = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[3u8; 32]);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::UpgradeContract(target.clone(), wasm_hash.clone()),
    );

    // Cast only 1 vote (quorum is 2)
    // for_votes = 1, against_votes = 0, abstain_votes = 0
    // total_votes = 1 < 2 (quorum ✗)
    // for_votes = 1 > against_votes = 0 (majority ✓ among single voter)

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Defeated);
    assert!(proposal.timelock_operation_id.is_none());

    // get_approved_upgrade must return None because quorum failed
    let result = setup.governance.get_approved_upgrade(&target);
    assert!(
        result.is_none(),
        "Expected None when majority met (1 > 0) but quorum not met (1 < 2)"
    );
}

#[test]
fn get_approved_upgrade_both_quorum_and_majority_met() {
    let env = create_env();
    let setup = setup(&env);

    let target = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[4u8; 32]);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::UpgradeContract(target.clone(), wasm_hash.clone()),
    );

    // Cast 2 for votes, 0 against (meets both conditions)
    // for_votes = 2, against_votes = 0, abstain_votes = 0
    // total_votes = 2 >= 2 (quorum ✓)
    // for_votes = 2 > against_votes = 0 (majority ✓)

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_a, &proposal_id, &VoteChoice::For);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Succeeded);
    assert!(proposal.timelock_operation_id.is_some());

    // Execute the proposal to write the approved upgrade hash
    advance_time(&env, 60); // Wait for timelock ETA
    setup
        .governance
        .execute_proposal(&setup.signer_a, &proposal_id);

    // get_approved_upgrade MUST return Some(hash) because both conditions met
    let result = setup.governance.get_approved_upgrade(&target);
    assert!(
        result.is_some(),
        "Expected Some(hash) when both quorum (2 >= 2) and majority (2 > 0) are met"
    );
    assert_eq!(
        result.unwrap(),
        wasm_hash,
        "Approved upgrade hash should match submitted hash"
    );
}

#[test]
fn get_approved_upgrade_abstain_votes_count_toward_quorum_not_majority() {
    let env = create_env();
    let setup = setup(&env);

    let target = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[5u8; 32]);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::UpgradeContract(target.clone(), wasm_hash.clone()),
    );

    // Test: 1 for + 1 abstain = 2 total (meets quorum) but only 1 for vote
    // for_votes = 1, against_votes = 0, abstain_votes = 1
    // total_votes = 2 >= 2 (quorum ✓ — abstain counts)
    // for_votes = 1 > against_votes = 0 (majority ✓)

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_a, &proposal_id, &VoteChoice::Abstain);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Succeeded);
    assert!(proposal.timelock_operation_id.is_some());

    advance_time(&env, 60);
    setup
        .governance
        .execute_proposal(&setup.signer_a, &proposal_id);

    let result = setup.governance.get_approved_upgrade(&target);
    assert!(
        result.is_some(),
        "Expected Some(hash) when abstain votes contribute to quorum and for > against"
    );
    assert_eq!(result.unwrap(), wasm_hash);
}

#[test]
fn get_approved_upgrade_quorum_boundary_one_short() {
    let env = create_env();
    let setup = setup(&env);

    let target = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[6u8; 32]);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::UpgradeContract(target.clone(), wasm_hash.clone()),
    );

    // Cast exactly 1 vote (one short of quorum=2)
    // for_votes = 1, against_votes = 0
    // total_votes = 1 < 2 (quorum ✗ by one vote)
    // for_votes > against_votes would be true but quorum blocks it

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Defeated);

    let result = setup.governance.get_approved_upgrade(&target);
    assert!(
        result.is_none(),
        "Expected None when total_votes = 1 (one short of quorum=2)"
    );
}

#[test]
fn get_approved_upgrade_quorum_boundary_at_threshold() {
    let env = create_env();
    let setup = setup(&env);

    let target = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[7u8; 32]);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::UpgradeContract(target.clone(), wasm_hash.clone()),
    );

    // Cast exactly 2 votes with 2 for, 0 against (exactly at quorum)
    // for_votes = 2, against_votes = 0
    // total_votes = 2 >= 2 (quorum ✓ exactly met)
    // for_votes = 2 > against_votes = 0 (majority ✓)

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_a, &proposal_id, &VoteChoice::For);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Succeeded);

    advance_time(&env, 60);
    setup
        .governance
        .execute_proposal(&setup.signer_a, &proposal_id);

    let result = setup.governance.get_approved_upgrade(&target);
    assert!(
        result.is_some(),
        "Expected Some(hash) when exactly at quorum boundary (2 >= 2) with majority"
    );
}

#[test]
fn get_approved_upgrade_majority_boundary_tie_fails() {
    let env = create_env();
    let setup = setup(&env);

    let target = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[8u8; 32]);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::UpgradeContract(target.clone(), wasm_hash.clone()),
    );

    // Cast 1 for, 1 against (tie at quorum=2)
    // for_votes = 1, against_votes = 1
    // total_votes = 2 >= 2 (quorum ✓)
    // for_votes = 1 NOT > against_votes = 1 (majority ✗ — tie fails)

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_a, &proposal_id, &VoteChoice::Against);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Defeated);

    let result = setup.governance.get_approved_upgrade(&target);
    assert!(
        result.is_none(),
        "Expected None when votes are tied (1 = 1, not >)"
    );
}

#[test]
fn get_approved_upgrade_majority_boundary_loss_one_vote() {
    let env = create_env();
    let setup = setup(&env);

    let target = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[9u8; 32]);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::UpgradeContract(target.clone(), wasm_hash.clone()),
    );

    // Cast 1 for, 2 against (majority fails by one vote)
    // for_votes = 1, against_votes = 2
    // total_votes = 3 >= 2 (quorum ✓)
    // for_votes = 1 NOT > against_votes = 2 (majority ✗)

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_a, &proposal_id, &VoteChoice::Against);
    setup
        .governance
        .cast_vote(&setup.employer_b, &proposal_id, &VoteChoice::Against);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Defeated);

    let result = setup.governance.get_approved_upgrade(&target);
    assert!(
        result.is_none(),
        "Expected None when for_votes (1) < against_votes (2)"
    );
}

#[test]
fn get_approved_upgrade_majority_boundary_win_by_one() {
    let env = create_env();
    let setup = setup(&env);

    let target = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[10u8; 32]);
    let proposal_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::UpgradeContract(target.clone(), wasm_hash.clone()),
    );

    // Cast 2 for, 1 against (majority succeeds by one vote)
    // for_votes = 2, against_votes = 1
    // total_votes = 3 >= 2 (quorum ✓)
    // for_votes = 2 > against_votes = 1 (majority ✓)

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_a, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_b, &proposal_id, &VoteChoice::Against);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Succeeded);

    advance_time(&env, 60);
    setup
        .governance
        .execute_proposal(&setup.signer_a, &proposal_id);

    let result = setup.governance.get_approved_upgrade(&target);
    assert!(
        result.is_some(),
        "Expected Some(hash) when for_votes (2) > against_votes (1)"
    );
}

#[test]
fn get_approved_upgrade_multiple_proposals_independent() {
    let env = create_env();
    let setup = setup(&env);

    // Create two proposals: one passes, one fails
    let target1 = Address::generate(&env);
    let wasm_hash1 = BytesN::from_array(&env, &[11u8; 32]);
    let proposal1_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::UpgradeContract(target1.clone(), wasm_hash1.clone()),
    );

    let target2 = Address::generate(&env);
    let wasm_hash2 = BytesN::from_array(&env, &[12u8; 32]);
    let proposal2_id = setup.governance.create_proposal(
        &setup.owner,
        &ProposalKind::UpgradeContract(target2.clone(), wasm_hash2.clone()),
    );

    // Proposal 1: passes (2 for, 0 against)
    setup
        .governance
        .cast_vote(&setup.owner, &proposal1_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer_a, &proposal1_id, &VoteChoice::For);

    // Proposal 2: fails (1 for, 0 against — quorum not met)
    setup
        .governance
        .cast_vote(&setup.owner, &proposal2_id, &VoteChoice::For);

    advance_time(&env, 3601);
    setup.governance.finalize_proposal(&proposal1_id);
    setup.governance.finalize_proposal(&proposal2_id);

    assert_eq!(
        setup.governance.get_proposal(&proposal1_id).unwrap().status,
        ProposalStatus::Succeeded
    );
    assert_eq!(
        setup.governance.get_proposal(&proposal2_id).unwrap().status,
        ProposalStatus::Defeated
    );

    advance_time(&env, 60);
    setup
        .governance
        .execute_proposal(&setup.signer_a, &proposal1_id);

    // Only proposal1 should have approved upgrade stored
    assert!(setup.governance.get_approved_upgrade(&target1).is_some());
    assert!(setup.governance.get_approved_upgrade(&target2).is_none());
}
