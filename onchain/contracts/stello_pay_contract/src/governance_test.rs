#[cfg(test)]
mod governance_tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, Env, String};

    use crate::governance::{
        CommunityTreasury, GovernanceConfig, GovernanceSystem, GovernanceToken, Proposal,
        ProposalStatus, ProposalType, Vote, VoteType, VotingResults,
    };
    use crate::payroll::PayrollContract;

    fn create_test_env() -> (Env, Address, Address, Address) {
        let env = Env::default();
        let owner = Address::generate(&env);
        let governance_token = Address::generate(&env);
        let user = Address::generate(&env);
        (env, owner, governance_token, user)
    }

    #[test]
    fn test_initialize_governance() {
        let (env, owner, governance_token, _user) = create_test_env();

        // Initialize the main contract first
        PayrollContract::initialize(env.clone(), owner.clone());

        // Initialize governance
        let result = PayrollContract::initialize_governance(
            env.clone(),
            owner.clone(),
            governance_token.clone(),
            86400,   // 1 day voting delay
            604800,  // 7 days voting period
            172800,  // 2 days execution delay
            1000000, // 1M tokens minimum to propose
            2000,    // 20% quorum threshold
            5100,    // 51% approval threshold
        );

        assert!(result.is_ok());

        // Verify governance is initialized
        let config = PayrollContract::get_governance_config(env.clone());
        assert!(config.is_some());

        let config = config.unwrap();
        assert_eq!(config.voting_delay, 86400);
        assert_eq!(config.voting_period, 604800);
        assert_eq!(config.proposal_threshold, 1000000);
    }

    #[test]
    fn test_create_proposal() {
        let (env, owner, governance_token, proposer) = create_test_env();

        // Initialize contracts
        PayrollContract::initialize(env.clone(), owner.clone());
        PayrollContract::initialize_governance(
            env.clone(),
            owner.clone(),
            governance_token.clone(),
            86400,
            604800,
            172800,
            1000000,
            2000,
            5100,
        )
        .unwrap();

        // Mock token balance for proposer
        let storage = env.storage().persistent();
        storage.set(
            &crate::storage::DataKey::Balance(proposer.clone(), governance_token.clone()),
            &2000000i128, // 2M tokens
        );

        // Create proposal
        let proposal_id = PayrollContract::create_proposal(
            env.clone(),
            proposer.clone(),
            String::from_str(&env, "Test Proposal"),
            String::from_str(&env, "A test governance proposal"),
            ProposalType::ParameterUpdate,
            None,
            Some(String::from_str(&env, "test_param:new_value")),
            None,
        );

        assert!(proposal_id.is_ok());
        let proposal_id = proposal_id.unwrap();
        assert_eq!(proposal_id, 1);

        // Verify proposal was created
        let proposal = PayrollContract::get_proposal(env.clone(), proposal_id);
        assert!(proposal.is_some());

        let proposal = proposal.unwrap();
        assert_eq!(proposal.proposer, proposer);
        assert_eq!(proposal.title, String::from_str(&env, "Test Proposal"));
    }

    #[test]
    fn test_cast_vote() {
        let (env, owner, governance_token, voter) = create_test_env();

        // Initialize contracts
        PayrollContract::initialize(env.clone(), owner.clone());
        PayrollContract::initialize_governance(
            env.clone(),
            owner.clone(),
            governance_token.clone(),
            0,      // No voting delay for test
            604800, // 7 days voting period
            172800,
            1000000,
            2000,
            5100,
        )
        .unwrap();

        // Mock token balances
        let storage = env.storage().persistent();
        storage.set(
            &crate::storage::DataKey::Balance(owner.clone(), governance_token.clone()),
            &2000000i128,
        );
        storage.set(
            &crate::storage::DataKey::Balance(voter.clone(), governance_token.clone()),
            &500000i128,
        );

        // Create proposal
        let proposal_id = PayrollContract::create_proposal(
            env.clone(),
            owner.clone(),
            String::from_str(&env, "Test Proposal"),
            String::from_str(&env, "A test governance proposal"),
            ProposalType::ParameterUpdate,
            None,
            Some(String::from_str(&env, "test_param:new_value")),
            None,
        )
        .unwrap();

        // Cast vote
        let result = PayrollContract::cast_vote(
            env.clone(),
            voter.clone(),
            proposal_id,
            VoteType::For,
            Some(String::from_str(&env, "I support this proposal")),
        );

        assert!(result.is_ok());

        // Verify voting results
        let results = PayrollContract::get_voting_results(env.clone(), proposal_id);
        assert!(results.is_some());

        let results = results.unwrap();
        assert_eq!(results.total_votes, 1);
        assert!(results.votes_for > 0);
    }

    #[test]
    fn test_delegate_voting_power() {
        let (env, owner, governance_token, delegator) = create_test_env();
        let delegate = Address::generate(&env);

        // Initialize contracts
        PayrollContract::initialize(env.clone(), owner.clone());
        PayrollContract::initialize_governance(
            env.clone(),
            owner.clone(),
            governance_token.clone(),
            86400,
            604800,
            172800,
            1000000,
            2000,
            5100,
        )
        .unwrap();

        // Mock token balance
        let storage = env.storage().persistent();
        storage.set(
            &crate::storage::DataKey::Balance(delegator.clone(), governance_token.clone()),
            &1000000i128,
        );

        // Delegate voting power
        let result = PayrollContract::delegate_voting_power(
            env.clone(),
            delegator.clone(),
            delegate.clone(),
            None, // No expiration
        );

        assert!(result.is_ok());

        // Verify delegation was recorded
        let delegation = env
            .storage()
            .persistent()
            .get::<crate::storage::RoleDataKey, crate::governance::VotingDelegation>(
                &crate::storage::RoleDataKey::GovernanceDelegation(delegator.clone()),
            );
        assert!(delegation.is_some());

        let delegation = delegation.unwrap();
        assert_eq!(delegation.delegate, delegate);
        assert!(delegation.is_active);
    }

    #[test]
    fn test_governance_config_retrieval() {
        let (env, owner, governance_token, _user) = create_test_env();

        // Initialize contracts
        PayrollContract::initialize(env.clone(), owner.clone());
        PayrollContract::initialize_governance(
            env.clone(),
            owner.clone(),
            governance_token.clone(),
            86400,
            604800,
            172800,
            1000000,
            2000,
            5100,
        )
        .unwrap();

        // Test config retrieval
        let config = PayrollContract::get_governance_config(env.clone());
        assert!(config.is_some());

        let config = config.unwrap();
        assert_eq!(config.voting_delay, 86400);
        assert_eq!(config.voting_period, 604800);
        assert_eq!(config.execution_delay, 172800);
        assert_eq!(config.proposal_threshold, 1000000);
        assert_eq!(config.quorum_threshold, 2000);
        assert_eq!(config.approval_threshold, 5100);
    }

    #[test]
    fn test_treasury_initialization() {
        let (env, owner, governance_token, _user) = create_test_env();

        // Initialize contracts
        PayrollContract::initialize(env.clone(), owner.clone());
        PayrollContract::initialize_governance(
            env.clone(),
            owner.clone(),
            governance_token.clone(),
            86400,
            604800,
            172800,
            1000000,
            2000,
            5100,
        )
        .unwrap();

        // Test treasury retrieval
        let treasury = PayrollContract::get_community_treasury(env.clone());
        assert!(treasury.is_some());

        let treasury = treasury.unwrap();
        assert_eq!(treasury.total_value, 0);
        assert_eq!(treasury.reserved_funds, 0);
    }

    #[test]
    fn test_governance_stats() {
        let (env, owner, governance_token, _user) = create_test_env();

        // Initialize contracts
        PayrollContract::initialize(env.clone(), owner.clone());
        PayrollContract::initialize_governance(
            env.clone(),
            owner.clone(),
            governance_token.clone(),
            86400,
            604800,
            172800,
            1000000,
            2000,
            5100,
        )
        .unwrap();

        // Test stats retrieval
        let stats = PayrollContract::get_governance_stats(env.clone());
        assert!(stats.is_some());

        let stats = stats.unwrap();
        assert_eq!(stats.total_proposals, 0);
        assert_eq!(stats.active_proposals, 0);
        assert_eq!(stats.executed_proposals, 0);
        assert_eq!(stats.total_voters, 0);
    }

    #[test]
    fn test_insufficient_tokens_to_propose() {
        let (env, owner, governance_token, proposer) = create_test_env();

        // Initialize contracts
        PayrollContract::initialize(env.clone(), owner.clone());
        PayrollContract::initialize_governance(
            env.clone(),
            owner.clone(),
            governance_token.clone(),
            86400,
            604800,
            172800,
            1000000,
            2000,
            5100,
        )
        .unwrap();

        // Mock insufficient token balance
        let storage = env.storage().persistent();
        storage.set(
            &crate::storage::DataKey::Balance(proposer.clone(), governance_token.clone()),
            &500000i128, // Less than required 1M tokens
        );

        // Try to create proposal
        let result = PayrollContract::create_proposal(
            env.clone(),
            proposer.clone(),
            String::from_str(&env, "Test Proposal"),
            String::from_str(&env, "A test governance proposal"),
            ProposalType::ParameterUpdate,
            None,
            None,
            None,
        );

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            crate::payroll::PayrollError::InsufficientVotingPower
        );
    }

    #[test]
    fn test_double_voting_prevention() {
        let (env, owner, governance_token, voter) = create_test_env();

        // Initialize contracts
        PayrollContract::initialize(env.clone(), owner.clone());
        PayrollContract::initialize_governance(
            env.clone(),
            owner.clone(),
            governance_token.clone(),
            0, // No voting delay
            604800,
            172800,
            1000000,
            2000,
            5100,
        )
        .unwrap();

        // Mock token balances
        let storage = env.storage().persistent();
        storage.set(
            &crate::storage::DataKey::Balance(owner.clone(), governance_token.clone()),
            &2000000i128,
        );
        storage.set(
            &crate::storage::DataKey::Balance(voter.clone(), governance_token.clone()),
            &500000i128,
        );

        // Create proposal
        let proposal_id = PayrollContract::create_proposal(
            env.clone(),
            owner.clone(),
            String::from_str(&env, "Test Proposal"),
            String::from_str(&env, "A test governance proposal"),
            ProposalType::ParameterUpdate,
            None,
            None,
            None,
        )
        .unwrap();

        // Cast first vote
        let result1 = PayrollContract::cast_vote(
            env.clone(),
            voter.clone(),
            proposal_id,
            VoteType::For,
            None,
        );
        assert!(result1.is_ok());

        // Try to cast second vote
        let result2 = PayrollContract::cast_vote(
            env.clone(),
            voter.clone(),
            proposal_id,
            VoteType::Against,
            None,
        );
        assert!(result2.is_err());
        assert_eq!(
            result2.unwrap_err(),
            crate::payroll::PayrollError::AlreadyVoted
        );
    }
}
