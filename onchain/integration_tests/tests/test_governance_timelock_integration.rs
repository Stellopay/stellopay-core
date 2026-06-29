//! Integration tests for governance, RBAC, multisig, and withdrawal_timelock.
//!
//! These tests validate the production governance flow introduced for issue
//! #443:
//! - RBAC `Admin` / `Employer` role-gated proposing and voting
//! - automatic timelock queueing after proposal success
//! - multisig-signer-gated execution after timelock maturity
//! - cancellation of queued timelock operations when governance cancels

#![cfg(test)]
#![allow(deprecated)]

use governance::{
    GovernanceContract, GovernanceContractClient, GovernanceError, ProposalKind, ProposalStatus,
    VoteChoice,
};
use multisig::{MultisigContract, MultisigContractClient};
use rbac::{RbacContract, RbacContractClient, Role};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env, Symbol, Vec,
};
use withdrawal_timelock::{
    OperationKind, OperationStatus, WithdrawalTimelock, WithdrawalTimelockClient,
};

const QUORUM_VOTES: u32 = 2;
const VOTING_PERIOD: u64 = 86_400;
const TIMELOCK_DELAY: u64 = 604_800;

fn env() -> Env {
    let e = Env::default();
    e.mock_all_auths();
    e
}

fn addr(env: &Env) -> Address {
    Address::generate(env)
}

fn advance(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| li.timestamp += seconds);
}

fn expected_timelock_payload_hash(env: &Env, proposal_id: u128) -> BytesN<32> {
    let mut payload = [0u8; 32];
    payload[16..].copy_from_slice(&proposal_id.to_be_bytes());
    BytesN::from_array(env, &payload)
}

struct Setup {
    governance: GovernanceContractClient<'static>,
    timelock: WithdrawalTimelockClient<'static>,
    rbac: RbacContractClient<'static>,
    multisig: MultisigContractClient<'static>,
    owner: Address,
    employer1: Address,
    employer2: Address,
    outsider: Address,
    signer1: Address,
    signer2: Address,
}

fn setup_contracts(env: &Env) -> Setup {
    let owner = addr(env);
    let employer1 = addr(env);
    let employer2 = addr(env);
    let outsider = addr(env);
    let signer1 = addr(env);
    let signer2 = addr(env);

    let governance_id = env.register_contract(None, GovernanceContract);
    let timelock_id = env.register_contract(None, WithdrawalTimelock);
    let rbac_id = env.register_contract(None, RbacContract);
    let multisig_id = env.register_contract(None, MultisigContract);

    let governance = GovernanceContractClient::new(env, &governance_id);
    let timelock = WithdrawalTimelockClient::new(env, &timelock_id);
    let rbac = RbacContractClient::new(env, &rbac_id);
    let multisig = MultisigContractClient::new(env, &multisig_id);

    rbac.initialize(&owner);
    rbac.grant_role(&owner, &employer1, &Role::Employer);
    rbac.grant_role(&owner, &employer2, &Role::Employer);

    let signers = Vec::from_array(env, [signer1.clone(), signer2.clone()]);
    multisig.initialize(&owner, &signers, &2u32, &None);

    // Governance must be the timelock admin for cross-contract queue/execute/cancel.
    timelock.initialize(&governance_id, &TIMELOCK_DELAY);
    governance.initialize(
        &owner,
        &rbac_id,
        &multisig_id,
        &timelock_id,
        &QUORUM_VOTES,
        &VOTING_PERIOD,
    );

    Setup {
        governance,
        timelock,
        rbac,
        multisig,
        owner,
        employer1,
        employer2,
        outsider,
        signer1,
        signer2,
    }
}

#[test]
fn governance_timelock_execute_flow() {
    let env = env();
    let setup = setup_contracts(&env);

    let new_arbiter = addr(&env);
    let proposal_id = setup.governance.create_proposal(
        &setup.employer1,
        &ProposalKind::ArbiterChange(new_arbiter.clone()),
    );

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer2, &proposal_id, &VoteChoice::For);

    advance(&env, VOTING_PERIOD + 1);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Succeeded);

    let op_id = proposal.timelock_operation_id.unwrap();
    let operation = setup.timelock.get_operation(&op_id).unwrap();
    assert_eq!(operation.status, OperationStatus::Queued);
    match operation.kind {
        OperationKind::AdminChange(target, payload_hash) => {
            assert_eq!(target, setup.governance.address);
            assert_eq!(
                payload_hash,
                expected_timelock_payload_hash(&env, proposal_id)
            );
        }
        _ => panic!("expected timelock AdminChange operation"),
    }

    let early = setup
        .governance
        .try_execute_proposal(&setup.signer1, &proposal_id);
    assert_eq!(early, Err(Ok(GovernanceError::TimelockNotReady)));

    advance(&env, TIMELOCK_DELAY);
    setup
        .governance
        .execute_proposal(&setup.signer1, &proposal_id);

    let executed = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(executed.status, ProposalStatus::Executed);
    assert_eq!(setup.governance.get_arbiter().unwrap(), new_arbiter);

    let executed_op = setup.timelock.get_operation(&op_id).unwrap();
    assert_eq!(executed_op.status, OperationStatus::Executed);
    assert!(executed_op.executed_at.is_some());
}

#[test]
fn governance_cancel_cancels_queued_timelock_operation() {
    let env = env();
    let setup = setup_contracts(&env);

    let proposal_id = setup
        .governance
        .create_proposal(&setup.owner, &ProposalKind::ArbiterChange(addr(&env)));

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer1, &proposal_id, &VoteChoice::For);

    advance(&env, VOTING_PERIOD + 1);
    setup.governance.finalize_proposal(&proposal_id);

    let proposal = setup.governance.get_proposal(&proposal_id).unwrap();
    let op_id = proposal.timelock_operation_id.unwrap();
    assert_eq!(
        setup.timelock.get_operation(&op_id).unwrap().status,
        OperationStatus::Queued
    );

    setup.governance.cancel_proposal(&setup.owner, &proposal_id);

    let cancelled = setup.governance.get_proposal(&proposal_id).unwrap();
    assert_eq!(cancelled.status, ProposalStatus::Cancelled);
    assert_eq!(
        setup.timelock.get_operation(&op_id).unwrap().status,
        OperationStatus::Cancelled
    );
}

#[test]
fn timelock_delay_updates_are_non_retroactive() {
    let env = env();
    let setup = setup_contracts(&env);

    let admin = setup.governance.address.clone();
    let op_id = setup.timelock.queue(
        &admin,
        &OperationKind::AdminChange(addr(&env), expected_timelock_payload_hash(&env, 1)),
    );
    let first = setup.timelock.get_operation(&op_id).unwrap();

    let new_delay = TIMELOCK_DELAY * 2;
    setup.timelock.update_delay(&admin, &new_delay);

    let op_id2 = setup.timelock.queue(
        &admin,
        &OperationKind::AdminChange(addr(&env), expected_timelock_payload_hash(&env, 2)),
    );
    let second = setup.timelock.get_operation(&op_id2).unwrap();
    let first_after = setup.timelock.get_operation(&op_id).unwrap();

    assert_eq!(first_after.eta, first.eta);
    assert_eq!(second.eta, second.created_at + new_delay);
    assert!(second.eta > first.eta);
}

#[test]
fn duplicate_proposals_are_independent() {
    let env = env();
    let setup = setup_contracts(&env);
    let new_arbiter = addr(&env);

    let proposal_id1 = setup.governance.create_proposal(
        &setup.employer1,
        &ProposalKind::ArbiterChange(new_arbiter.clone()),
    );
    let proposal_id2 = setup
        .governance
        .create_proposal(&setup.employer1, &ProposalKind::ArbiterChange(new_arbiter));

    assert_ne!(proposal_id1, proposal_id2);

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id1, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer2, &proposal_id1, &VoteChoice::For);

    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id2, &VoteChoice::Against);
    setup
        .governance
        .cast_vote(&setup.employer2, &proposal_id2, &VoteChoice::Against);

    advance(&env, VOTING_PERIOD + 1);
    setup.governance.finalize_proposal(&proposal_id1);
    setup.governance.finalize_proposal(&proposal_id2);

    assert_eq!(
        setup.governance.get_proposal(&proposal_id1).unwrap().status,
        ProposalStatus::Succeeded
    );
    assert_eq!(
        setup.governance.get_proposal(&proposal_id2).unwrap().status,
        ProposalStatus::Defeated
    );
}

#[test]
fn access_control_is_enforced_across_governance_and_timelock() {
    let env = env();
    let setup = setup_contracts(&env);

    let kind = ProposalKind::ParameterChange(Symbol::new(&env, "limit"), 5);
    let outsider_proposal = setup.governance.try_create_proposal(&setup.outsider, &kind);
    assert_eq!(
        outsider_proposal,
        Err(Ok(GovernanceError::NotEligibleVoter))
    );

    let proposal_id = setup.governance.create_proposal(&setup.owner, &kind);
    setup
        .governance
        .cast_vote(&setup.owner, &proposal_id, &VoteChoice::For);
    setup
        .governance
        .cast_vote(&setup.employer1, &proposal_id, &VoteChoice::For);
    advance(&env, VOTING_PERIOD + 1);
    setup.governance.finalize_proposal(&proposal_id);
    advance(&env, TIMELOCK_DELAY);

    let bad_execute = setup
        .governance
        .try_execute_proposal(&setup.outsider, &proposal_id);
    assert_eq!(bad_execute, Err(Ok(GovernanceError::UnauthorizedExecutor)));

    let unauthorized_timelock = setup.timelock.try_queue(
        &setup.outsider,
        &OperationKind::AdminChange(addr(&env), expected_timelock_payload_hash(&env, 99)),
    );
    assert!(unauthorized_timelock.is_err());
}

#[test]
fn live_rbac_changes_affect_future_voting() {
    let env = env();
    let setup = setup_contracts(&env);

    let proposal_id = setup
        .governance
        .create_proposal(&setup.owner, &ProposalKind::ArbiterChange(addr(&env)));

    setup
        .rbac
        .revoke_role(&setup.owner, &setup.employer1, &Role::Employer);

    let vote_res = setup
        .governance
        .try_cast_vote(&setup.employer1, &proposal_id, &VoteChoice::For);
    assert_eq!(vote_res, Err(Ok(GovernanceError::NotEligibleVoter)));
}

#[test]
fn governance_uses_configured_multisig_signers() {
    let env = env();
    let setup = setup_contracts(&env);

    let signers = setup.multisig.get_signers();
    assert_eq!(signers.len(), 2);
    assert_eq!(signers.get(0).unwrap(), setup.signer1);
    assert_eq!(signers.get(1).unwrap(), setup.signer2);
}
