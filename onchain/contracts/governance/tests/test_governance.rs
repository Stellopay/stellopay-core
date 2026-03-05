#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env, Symbol,
};

use governance::{
    GovernanceContract, GovernanceContractClient, ProposalKind, ProposalStatus, VoteChoice,
};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_initialized(env: &Env) -> (GovernanceContractClient<'static>, Address, Address, Address) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, GovernanceContract);
    let client = GovernanceContractClient::new(env, &contract_id);

    let owner = Address::generate(env);
    let voter1 = Address::generate(env);
    let voter2 = Address::generate(env);

    // quorum 50%, voting period 100 seconds, timelock 10 seconds
    client.initialize(&owner, &5000u32, &100u64, &10u64);

    // give both voters equal power
    client.set_voter_power(&owner, &voter1, &10i128);
    client.set_voter_power(&owner, &voter2, &10i128);

    (client, owner, voter1, voter2)
}

fn advance_time(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| {
        li.timestamp += seconds;
    });
}

#[test]
fn initialize_and_config() {
    let env = create_env();
    let (client, owner, _, _) = setup_initialized(&env);

    let (cfg_owner, quorum_bps, voting_period, timelock) = client.get_config();
    assert_eq!(cfg_owner, owner);
    assert_eq!(quorum_bps, 5000u32);
    assert_eq!(voting_period, 100u64);
    assert_eq!(timelock, 10u64);

    // owner can update config
    client.update_config(&owner, &6000u32, &200u64, &20u64);
    let (_o, q2, vp2, tl2) = client.get_config();
    assert_eq!(q2, 6000u32);
    assert_eq!(vp2, 200u64);
    assert_eq!(tl2, 20u64);
}

#[test]
fn proposal_lifecycle_parameter_change() {
    let env = create_env();
    let (client, _owner, voter1, voter2) = setup_initialized(&env);

    // create a parameter-change proposal
    let key = Symbol::new(&env, "max_slippage_bps");
    let kind = ProposalKind::ParameterChange(key.clone(), 123i128);

    let proposal_id = client.propose(&voter1, &kind);
    let p = client.get_proposal(&proposal_id).unwrap();
    assert_eq!(p.status, ProposalStatus::Active);

    // both voters vote FOR
    client.vote(&voter1, &proposal_id, &VoteChoice::For);
    client.vote(&voter2, &proposal_id, &VoteChoice::For);

    // advance time beyond voting period
    advance_time(&env, 101);

    // queue the proposal (compute quorum + approval, set eta)
    client.queue(&proposal_id);
    let p = client.get_proposal(&proposal_id).unwrap();
    assert_eq!(p.status, ProposalStatus::Succeeded);
    assert!(p.eta.is_some());

    // timelock not yet expired
    let res = client.try_execute(&proposal_id);
    assert!(res.is_err());

    // advance past timelock and execute
    advance_time(&env, 10);
    client.execute(&proposal_id);

    let p = client.get_proposal(&proposal_id).unwrap();
    assert_eq!(p.status, ProposalStatus::Executed);

    // parameter should now be stored
    let stored = client.get_parameter(&key).unwrap();
    assert_eq!(stored, 123i128);
}

#[test]
fn quorum_and_rejection() {
    let env = create_env();
    let (client, _owner, voter1, voter2) = setup_initialized(&env);

    // Only voter1 has power; voter2 is disabled
    client.set_voter_power(&_owner, &voter2, &0i128);

    let key = Symbol::new(&env, "param");
    let kind = ProposalKind::ParameterChange(key, 1i128);

    let proposal_id = client.propose(&voter1, &kind);

    // only voter1 votes against; participation is 10 of total 20 (below 50% quorum)
    client.vote(&voter1, &proposal_id, &VoteChoice::Against);

    advance_time(&env, 101);
    client.queue(&proposal_id);

    let p = client.get_proposal(&proposal_id).unwrap();
    assert_eq!(p.status, ProposalStatus::Defeated);
}

#[test]
fn cannot_double_vote_or_vote_without_power() {
    let env = create_env();
    let (client, owner, voter1, voter2) = setup_initialized(&env);

    // remove power from voter2 entirely
    client.set_voter_power(&owner, &voter2, &0i128);

    let kind = ProposalKind::ArbiterChange(Address::generate(&env));
    let proposal_id = client.propose(&voter1, &kind);

    // first vote ok
    client.vote(&voter1, &proposal_id, &VoteChoice::For);

    // second vote from same voter should fail
    let res = client.try_vote(&voter1, &proposal_id, &VoteChoice::For);
    assert!(res.is_err());

    // voter2 has no power, cannot vote
    let res = client.try_vote(&voter2, &proposal_id, &VoteChoice::For);
    assert!(res.is_err());
}

#[test]
fn arbiter_and_upgrade_proposals_record_intent() {
    let env = create_env();
    let (client, _owner, voter1, voter2) = setup_initialized(&env);

    // Arbiter change
    let new_arbiter = Address::generate(&env);
    let arb_kind = ProposalKind::ArbiterChange(new_arbiter.clone());
    let arb_id = client.propose(&voter1, &arb_kind);
    client.vote(&voter1, &arb_id, &VoteChoice::For);
    client.vote(&voter2, &arb_id, &VoteChoice::For);
    advance_time(&env, 101);
    client.queue(&arb_id);
    advance_time(&env, 10);
    client.execute(&arb_id);

    let stored_arbiter = client.get_arbiter().unwrap();
    assert_eq!(stored_arbiter, new_arbiter);

    // Upgrade contract
    let target = Address::generate(&env);
    let hash: BytesN<32> = BytesN::from_array(&env, &[7u8; 32]);
    let up_kind = ProposalKind::UpgradeContract(target.clone(), hash.clone());
    let up_id = client.propose(&voter1, &up_kind);
    client.vote(&voter1, &up_id, &VoteChoice::For);
    client.vote(&voter2, &up_id, &VoteChoice::For);
    advance_time(&env, 101);
    client.queue(&up_id);
    advance_time(&env, 10);
    client.execute(&up_id);

    let stored_hash = client.get_approved_upgrade(&target).unwrap();
    assert_eq!(stored_hash, hash);
}

#[test]
fn owner_can_cancel_before_execution() {
    let env = create_env();
    let (client, owner, voter1, voter2) = setup_initialized(&env);

    let key = Symbol::new(&env, "cancel_me");
    let kind = ProposalKind::ParameterChange(key, 10i128);
    let proposal_id = client.propose(&voter1, &kind);

    client.vote(&voter1, &proposal_id, &VoteChoice::For);
    client.vote(&voter2, &proposal_id, &VoteChoice::For);
    advance_time(&env, 101);
    client.queue(&proposal_id);

    // Owner cancels after success but before execute
    client.cancel(&owner, &proposal_id);

    let p = client.get_proposal(&proposal_id).unwrap();
    assert_eq!(p.status, ProposalStatus::Cancelled);

    // execute should now fail
    let res = client.try_execute(&proposal_id);
    assert!(res.is_err());
}
