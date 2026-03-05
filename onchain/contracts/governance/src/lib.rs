#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, BytesN, Env, Symbol, Vec};

/// Basis points denominator used for quorum configuration (100% = 10_000 bps).
const BPS_DENOMINATOR: u32 = 10_000;

/// Errors for the governance contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GovernanceError {
    NotInitialized,
    AlreadyInitialized,
    NotOwner,
    InvalidQuorum,
    InvalidVotingPeriod,
    InvalidTimelock,
    UnknownProposal,
    VotingClosed,
    VotingNotStarted,
    AlreadyVoted,
    NoVotingPower,
    ProposalNotSucceeded,
    TimelockNotExpired,
    ProposalNotActive,
}

/// Type of a governance proposal.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalKind {
    /// Generic parameter change stored under the given key.
    /// Tuple layout: (key, value)
    ParameterChange(Symbol, i128),
    /// Governance approval for a contract upgrade. The actual upgrade
    /// execution is expected to be handled by off-chain tooling or a
    /// separate upgrade executor contract.
    /// Tuple layout: (target, new_wasm_hash)
    UpgradeContract(Address, BytesN<32>),
    /// Arbiter address change for downstream contracts or off-chain
    /// dispute-resolution flows.
    /// Tuple layout: (new_arbiter)
    ArbiterChange(Address),
}

/// Status of a governance proposal.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalStatus {
    /// Proposal is open for voting.
    Active,
    /// Proposal voting finished and it met quorum and approval threshold.
    Succeeded,
    /// Proposal voting finished but did not meet quorum or was rejected.
    Defeated,
    /// Proposal was cancelled by the owner before execution.
    Cancelled,
    /// Proposal has been executed after the timelock.
    Executed,
}

/// Vote choices for a proposal.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VoteChoice {
    For,
    Against,
    Abstain,
}

/// Proposal data stored on-chain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proposal {
    pub id: u128,
    pub proposer: Address,
    pub kind: ProposalKind,
    pub status: ProposalStatus,
    pub for_votes: i128,
    pub against_votes: i128,
    pub abstain_votes: i128,
    pub start_time: u64,
    pub end_time: u64,
    /// Earliest timestamp at which the proposal can be executed,
    /// set when the proposal moves to `Succeeded`.
    pub eta: Option<u64>,
}

/// Storage keys for governance state.
#[contracttype]
#[derive(Clone)]
pub enum StorageKey {
    Initialized,
    Owner,
    /// Total voting power across all voters.
    TotalVotingPower,
    /// Voter-specific voting power: Address -> i128.
    VoterPower(Address),
    /// Next proposal id sequence.
    NextProposalId,
    /// Stored proposal by id.
    Proposal(u128),
    /// Tracks that an address has voted on a proposal:
    /// (proposal_id, voter) -> VoteChoice.
    Vote(u128, Address),
    /// Quorum in basis points (1-10_000).
    QuorumBps,
    /// Voting period in seconds.
    VotingPeriodSeconds,
    /// Execution timelock in seconds applied after proposal success.
    TimelockSeconds,
    /// Parameter storage for `ParameterChange` proposals: key -> value.
    Parameter(Symbol),
    /// Last approved arbiter address from `ArbiterChange` proposals.
    Arbiter,
    /// Last approved upgrade hash for a target contract: target -> hash.
    ApprovedUpgrade(Address),
}

fn require_initialized(env: &Env) {
    let initialized: bool = env
        .storage()
        .persistent()
        .get(&StorageKey::Initialized)
        .unwrap_or(false);
    assert!(initialized, "governance not initialized");
}

fn read_owner(env: &Env) -> Address {
    env.storage()
        .persistent()
        .get::<_, Address>(&StorageKey::Owner)
        .expect("owner not set")
}

fn require_owner(env: &Env, caller: &Address) {
    caller.require_auth();
    let owner = read_owner(env);
    assert!(owner == *caller, "only owner");
}

fn next_proposal_id(env: &Env) -> u128 {
    let current: u128 = env
        .storage()
        .persistent()
        .get(&StorageKey::NextProposalId)
        .unwrap_or(0);
    let next = current.checked_add(1).expect("proposal id overflow");
    env.storage()
        .persistent()
        .set(&StorageKey::NextProposalId, &next);
    next
}

fn read_proposal(env: &Env, proposal_id: u128) -> Proposal {
    env.storage()
        .persistent()
        .get::<_, Proposal>(&StorageKey::Proposal(proposal_id))
        .expect("proposal not found")
}

fn write_proposal(env: &Env, proposal: &Proposal) {
    env.storage()
        .persistent()
        .set(&StorageKey::Proposal(proposal.id), proposal);
}

fn read_voter_power(env: &Env, voter: &Address) -> i128 {
    env.storage()
        .persistent()
        .get::<_, i128>(&StorageKey::VoterPower(voter.clone()))
        .unwrap_or(0)
}

fn write_voter_power(env: &Env, voter: &Address, power: i128) {
    env.storage()
        .persistent()
        .set(&StorageKey::VoterPower(voter.clone()), &power);
}

fn read_total_power(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get::<_, i128>(&StorageKey::TotalVotingPower)
        .unwrap_or(0)
}

fn write_total_power(env: &Env, power: i128) {
    env.storage()
        .persistent()
        .set(&StorageKey::TotalVotingPower, &power);
}

fn get_quorum_bps(env: &Env) -> u32 {
    env.storage()
        .persistent()
        .get::<_, u32>(&StorageKey::QuorumBps)
        .unwrap_or(0)
}

fn get_voting_period(env: &Env) -> u64 {
    env.storage()
        .persistent()
        .get::<_, u64>(&StorageKey::VotingPeriodSeconds)
        .unwrap_or(0)
}

fn get_timelock(env: &Env) -> u64 {
    env.storage()
        .persistent()
        .get::<_, u64>(&StorageKey::TimelockSeconds)
        .unwrap_or(0)
}

#[contract]
pub struct GovernanceContract;

#[contractimpl]
impl GovernanceContract {
    /// @notice Initializes the governance contract.
    /// @dev Can only be called once by the designated owner.
    /// @param owner Address that controls configuration and can manage voter weights.
    /// @param quorum_bps Quorum requirement in basis points (1-10_000).
    /// @param voting_period_seconds Duration of the voting period in seconds.
    /// @param timelock_seconds Delay between proposal success and execution.
    pub fn initialize(
        env: Env,
        owner: Address,
        quorum_bps: u32,
        voting_period_seconds: u64,
        timelock_seconds: u64,
    ) {
        owner.require_auth();

        let initialized: bool = env
            .storage()
            .persistent()
            .get(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(!initialized, "already initialized");

        assert!(
            quorum_bps > 0 && quorum_bps <= BPS_DENOMINATOR,
            "invalid quorum"
        );
        assert!(voting_period_seconds > 0, "invalid voting period");
        // Timelock of zero is allowed (immediate execution after success).

        env.storage().persistent().set(&StorageKey::Owner, &owner);
        env.storage()
            .persistent()
            .set(&StorageKey::QuorumBps, &quorum_bps);
        env.storage()
            .persistent()
            .set(&StorageKey::VotingPeriodSeconds, &voting_period_seconds);
        env.storage()
            .persistent()
            .set(&StorageKey::TimelockSeconds, &timelock_seconds);
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);
    }

    /// @notice Updates governance configuration parameters.
    /// @dev Only callable by the owner.
    /// @param caller Owner address; must authenticate.
    /// @param quorum_bps New quorum in basis points (1-10_000).
    /// @param voting_period_seconds New voting period in seconds.
    /// @param timelock_seconds New timelock in seconds.
    pub fn update_config(
        env: Env,
        caller: Address,
        quorum_bps: u32,
        voting_period_seconds: u64,
        timelock_seconds: u64,
    ) {
        require_initialized(&env);
        require_owner(&env, &caller);

        assert!(
            quorum_bps > 0 && quorum_bps <= BPS_DENOMINATOR,
            "invalid quorum"
        );
        assert!(voting_period_seconds > 0, "invalid voting period");

        env.storage()
            .persistent()
            .set(&StorageKey::QuorumBps, &quorum_bps);
        env.storage()
            .persistent()
            .set(&StorageKey::VotingPeriodSeconds, &voting_period_seconds);
        env.storage()
            .persistent()
            .set(&StorageKey::TimelockSeconds, &timelock_seconds);
    }

    /// @notice Sets or updates the voting power for a voter.
    /// @dev Owner-only. Adjusts the global total voting power accordingly.
    /// @param caller Owner address; must authenticate.
    /// @param voter Address whose voting power is being updated.
    /// @param power New voting power (must be >= 0).
    pub fn set_voter_power(env: Env, caller: Address, voter: Address, power: i128) {
        require_initialized(&env);
        require_owner(&env, &caller);
        assert!(power >= 0, "negative power");

        let prev = read_voter_power(&env, &voter);
        let mut total = read_total_power(&env);
        total = total.checked_sub(prev).expect("total power underflow");
        total = total.checked_add(power).expect("total power overflow");

        write_voter_power(&env, &voter, power);
        write_total_power(&env, total);
    }

    /// @notice Creates a new governance proposal.
    /// @dev Proposer must have non-zero voting power.
    /// @param proposer Address creating the proposal; must authenticate.
    /// @param kind Encoded proposal kind and payload.
    /// @return proposal_id Newly created proposal identifier.
    pub fn propose(env: Env, proposer: Address, kind: ProposalKind) -> u128 {
        require_initialized(&env);
        proposer.require_auth();

        let power = read_voter_power(&env, &proposer);
        assert!(power > 0, "no voting power");

        let voting_period = get_voting_period(&env);
        assert!(voting_period > 0, "voting period not set");

        let now = env.ledger().timestamp();
        let id = next_proposal_id(&env);

        let proposal = Proposal {
            id,
            proposer,
            kind,
            status: ProposalStatus::Active,
            for_votes: 0,
            against_votes: 0,
            abstain_votes: 0,
            start_time: now,
            end_time: now.checked_add(voting_period).expect("voting end overflow"),
            eta: None,
        };

        write_proposal(&env, &proposal);
        id
    }

    /// @notice Casts a vote on an active proposal.
    /// @dev Each voter may vote at most once per proposal.
    /// @param voter Address casting the vote; must authenticate.
    /// @param proposal_id Proposal identifier.
    /// @param choice Vote choice (For, Against, Abstain).
    pub fn vote(env: Env, voter: Address, proposal_id: u128, choice: VoteChoice) {
        require_initialized(&env);
        voter.require_auth();

        let power = read_voter_power(&env, &voter);
        assert!(power > 0, "no voting power");

        let mut proposal = read_proposal(&env, proposal_id);
        assert!(
            proposal.status == ProposalStatus::Active,
            "proposal not active"
        );

        let now = env.ledger().timestamp();
        assert!(now >= proposal.start_time, "voting not started");
        assert!(now <= proposal.end_time, "voting closed");

        let vote_key = StorageKey::Vote(proposal_id, voter.clone());
        let already_voted: Option<VoteChoice> = env.storage().persistent().get(&vote_key);
        assert!(already_voted.is_none(), "already voted");

        match choice {
            VoteChoice::For => {
                proposal.for_votes = proposal
                    .for_votes
                    .checked_add(power)
                    .expect("for_votes overflow");
            }
            VoteChoice::Against => {
                proposal.against_votes = proposal
                    .against_votes
                    .checked_add(power)
                    .expect("against_votes overflow");
            }
            VoteChoice::Abstain => {
                proposal.abstain_votes = proposal
                    .abstain_votes
                    .checked_add(power)
                    .expect("abstain_votes overflow");
            }
        }

        env.storage().persistent().set(&vote_key, &choice);
        write_proposal(&env, &proposal);
    }

    /// @notice Queues a proposal for execution if it has succeeded.
    /// @dev Anyone can call this after the voting period has ended. This
    ///      computes quorum and approval and, if satisfied, marks the
    ///      proposal as `Succeeded` and sets the execution ETA based on
    ///      the configured timelock.
    /// @param proposal_id Proposal identifier.
    pub fn queue(env: Env, proposal_id: u128) {
        require_initialized(&env);

        let mut proposal = read_proposal(&env, proposal_id);
        assert!(
            proposal.status == ProposalStatus::Active,
            "proposal not active"
        );

        let now = env.ledger().timestamp();
        assert!(now > proposal.end_time, "voting still in progress");

        let total_power = read_total_power(&env);
        let quorum_bps = get_quorum_bps(&env);

        let total_participation = proposal
            .for_votes
            .checked_add(proposal.against_votes)
            .and_then(|v| v.checked_add(proposal.abstain_votes))
            .expect("participation overflow");

        let mut succeeded = false;

        if total_power > 0 && quorum_bps > 0 {
            // quorum requirement: participation >= total_power * quorum_bps / BPS_DENOMINATOR
            let quorum_threshold =
                (total_power * i128::from(quorum_bps as i64)) / i128::from(BPS_DENOMINATOR as i64);
            let has_quorum = total_participation >= quorum_threshold;
            let approved = proposal.for_votes > proposal.against_votes;

            if has_quorum && approved {
                succeeded = true;
            }
        }

        if succeeded {
            let timelock = get_timelock(&env);
            let eta = now.checked_add(timelock).expect("eta overflow");
            proposal.status = ProposalStatus::Succeeded;
            proposal.eta = Some(eta);
        } else {
            proposal.status = ProposalStatus::Defeated;
        }

        write_proposal(&env, &proposal);
    }

    /// @notice Executes a previously succeeded proposal after the timelock.
    /// @dev For `ParameterChange`, the key/value pair is written to storage.
    ///      For `ArbiterChange` and `UpgradeContract`, the intent is recorded
    ///      in storage for downstream contracts or off-chain tooling to act on.
    /// @param proposal_id Proposal identifier.
    pub fn execute(env: Env, proposal_id: u128) {
        require_initialized(&env);

        let mut proposal = read_proposal(&env, proposal_id);
        assert!(
            proposal.status == ProposalStatus::Succeeded,
            "proposal not succeeded"
        );

        let eta = proposal.eta.expect("eta not set");
        let now = env.ledger().timestamp();
        assert!(now >= eta, "timelock not expired");

        match &proposal.kind {
            ProposalKind::ParameterChange(key, value) => {
                env.storage()
                    .persistent()
                    .set(&StorageKey::Parameter(key.clone()), value);
            }
            ProposalKind::ArbiterChange(new_arbiter) => {
                env.storage()
                    .persistent()
                    .set(&StorageKey::Arbiter, new_arbiter);
            }
            ProposalKind::UpgradeContract(target, new_wasm_hash) => {
                env.storage()
                    .persistent()
                    .set(&StorageKey::ApprovedUpgrade(target.clone()), new_wasm_hash);
            }
        }

        proposal.status = ProposalStatus::Executed;
        write_proposal(&env, &proposal);
    }

    /// @notice Cancels a proposal that has not yet been executed.
    /// @dev Only the owner can cancel; typically used for emergency overrides.
    /// @param caller Owner address; must authenticate.
    /// @param proposal_id Proposal identifier.
    pub fn cancel(env: Env, caller: Address, proposal_id: u128) {
        require_initialized(&env);
        require_owner(&env, &caller);

        let mut proposal = read_proposal(&env, proposal_id);
        assert!(
            proposal.status == ProposalStatus::Active
                || proposal.status == ProposalStatus::Succeeded,
            "cannot cancel"
        );

        proposal.status = ProposalStatus::Cancelled;
        write_proposal(&env, &proposal);
    }

    /// @notice Returns the current governance configuration.
    /// @return owner, quorum_bps, voting_period_seconds, timelock_seconds.
    /// @dev Requires caller authentication
    pub fn get_config(env: Env) -> (Address, u32, u64, u64) {
        require_initialized(&env);
        let owner = read_owner(&env);
        let quorum_bps = get_quorum_bps(&env);
        let voting_period = get_voting_period(&env);
        let timelock = get_timelock(&env);
        (owner, quorum_bps, voting_period, timelock)
    }

    /// @notice Returns the voting power for a given voter.
    /// @param voter voter parameter
    /// @dev Requires caller authentication
    pub fn get_voter_power(env: Env, voter: Address) -> i128 {
        read_voter_power(&env, &voter)
    }

    /// @notice Returns the total voting power across all voters.
    /// @dev Requires caller authentication
    pub fn get_total_voting_power(env: Env) -> i128 {
        read_total_power(&env)
    }

    /// @notice Returns a stored proposal by id, if any.
    /// @param proposal_id proposal_id parameter
    /// @dev Requires caller authentication
    pub fn get_proposal(env: Env, proposal_id: u128) -> Option<Proposal> {
        env.storage()
            .persistent()
            .get(&StorageKey::Proposal(proposal_id))
    }

    /// @notice Returns whether a voter has already voted on a proposal.
    /// @param proposal_id proposal_id parameter
    /// @param voter voter parameter
    /// @dev Requires caller authentication
    pub fn get_vote(env: Env, proposal_id: u128, voter: Address) -> Option<VoteChoice> {
        env.storage()
            .persistent()
            .get(&StorageKey::Vote(proposal_id, voter))
    }

    /// @notice Returns a parameter value previously set via a `ParameterChange` proposal.
    /// @dev Requires caller authentication
    pub fn get_parameter(env: Env, key: Symbol) -> Option<i128> {
        env.storage().persistent().get(&StorageKey::Parameter(key))
    }

    /// @notice Returns the last approved arbiter address, if any.
    /// @dev Requires caller authentication
    pub fn get_arbiter(env: Env) -> Option<Address> {
        env.storage().persistent().get(&StorageKey::Arbiter)
    }

    /// @notice Returns the last approved upgrade hash for a target contract, if any.
    /// @param target target parameter
    /// @dev Requires caller authentication
    pub fn get_approved_upgrade(env: Env, target: Address) -> Option<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&StorageKey::ApprovedUpgrade(target))
    }
}
