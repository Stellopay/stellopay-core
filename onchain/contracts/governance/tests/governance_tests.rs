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
    governance.initialize(&owner, &rbac_id, &multisig_id, &timelock_id, &2u32, &3600u64);

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
    let page2: ProposalPage = setup
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
    advance_time(&env, 101);
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
    advance_time(&env, 101);
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
        advance_time(&env, 101);
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
