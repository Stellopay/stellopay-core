#![no_std]

use core::cmp::Ordering;

use multisig::MultisigContractClient;
use rbac::{RbacContractClient, Role};
use soroban_sdk::{
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, IntoVal, Symbol,
    Val, Vec,
};
use withdrawal_timelock::{
    OperationKind as TimelockOperationKind, OperationStatus as TimelockOperationStatus,
    TimelockedOperation, WithdrawalTimelockClient,
};

/// Errors returned by the governance contract.
#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum GovernanceError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotOwner = 3,
    InvalidQuorum = 4,
    InvalidVotingPeriod = 5,
    ProposalNotFound = 6,
    ProposalNotActive = 7,
    VotingStillOpen = 8,
    VotingClosed = 9,
    VotingPeriodTooLong = 18,
    AlreadyVoted = 10,
    NotEligibleVoter = 11,
    ProposalNotSucceeded = 12,
    TimelockNotReady = 13,
    UnauthorizedExecutor = 14,
    TimelockQueueFailed = 15,
    TimelockExecutionFailed = 16,
    TimelockCancellationFailed = 17,
}

/// Types of governance actions supported by the contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalKind {
    /// Store a generic integer parameter under a symbol key.
    ///
    /// Layout: `(key, value)`.
    ParameterChange(Symbol, i128),
    /// Record an approved upgrade hash for a target contract.
    ///
    /// Layout: `(target, new_wasm_hash)`.
    UpgradeContract(Address, BytesN<32>),
    /// Record an approved downstream arbiter address.
    ///
    /// Layout: `(new_arbiter)`.
    ArbiterChange(Address),
}

/// Lifecycle status for a proposal.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalStatus {
    Active,
    Succeeded,
    Defeated,
    Cancelled,
    Executed,
}

/// Supported vote options.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VoteChoice {
    For,
    Against,
    Abstain,
}

/// Stored proposal data.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proposal {
    pub id: u128,
    pub proposer: Address,
    pub kind: ProposalKind,
    pub status: ProposalStatus,
    pub for_votes: u32,
    pub against_votes: u32,
    pub abstain_votes: u32,
    pub start_time: u64,
    pub end_time: u64,
    /// Timelock operation created when the proposal succeeds.
    pub timelock_operation_id: Option<u128>,
    /// Earliest execution timestamp returned by the timelock contract.
    pub eta: Option<u64>,
}

/// Persistent storage keys.
#[contracttype]
#[derive(Clone)]
pub enum StorageKey {
    Initialized,
    Owner,
    RbacContract,
    MultisigContract,
    TimelockContract,
    QuorumVotes,
    VotingPeriodSeconds,
    NextProposalId,
    Proposal(u128),
    Vote(u128, Address),
    Parameter(Symbol),
    Arbiter,
    ApprovedUpgrade(Address),
}

fn require_initialized(env: &Env) -> Result<(), GovernanceError> {
    let initialized = env
        .storage()
        .persistent()
        .get::<_, bool>(&StorageKey::Initialized)
        .unwrap_or(false);
    if initialized {
        Ok(())
    } else {
        Err(GovernanceError::NotInitialized)
    }
}

fn read_owner(env: &Env) -> Result<Address, GovernanceError> {
    env.storage()
        .persistent()
        .get::<_, Address>(&StorageKey::Owner)
        .ok_or(GovernanceError::NotInitialized)
}

fn require_owner(env: &Env, caller: &Address) -> Result<(), GovernanceError> {
    caller.require_auth();
    if read_owner(env)? == *caller {
        Ok(())
    } else {
        Err(GovernanceError::NotOwner)
    }
}

fn read_address(env: &Env, key: &StorageKey) -> Result<Address, GovernanceError> {
    env.storage()
        .persistent()
        .get::<_, Address>(key)
        .ok_or(GovernanceError::NotInitialized)
}

fn read_quorum_votes(env: &Env) -> Result<u32, GovernanceError> {
    env.storage()
        .persistent()
        .get::<_, u32>(&StorageKey::QuorumVotes)
        .ok_or(GovernanceError::NotInitialized)
}

fn read_voting_period(env: &Env) -> Result<u64, GovernanceError> {
    env.storage()
        .persistent()
        .get::<_, u64>(&StorageKey::VotingPeriodSeconds)
        .ok_or(GovernanceError::NotInitialized)
}

fn next_proposal_id(env: &Env) -> u128 {
    let current = env
        .storage()
        .persistent()
        .get::<_, u128>(&StorageKey::NextProposalId)
        .unwrap_or(0);
    let next = current.checked_add(1).expect("proposal id overflow");
    env.storage()
        .persistent()
        .set(&StorageKey::NextProposalId, &next);
    next
}

fn read_proposal(env: &Env, proposal_id: u128) -> Result<Proposal, GovernanceError> {
    env.storage()
        .persistent()
        .get::<_, Proposal>(&StorageKey::Proposal(proposal_id))
        .ok_or(GovernanceError::ProposalNotFound)
}

fn write_proposal(env: &Env, proposal: &Proposal) {
    env.storage()
        .persistent()
        .set(&StorageKey::Proposal(proposal.id), proposal);
}

fn is_eligible_voter(env: &Env, voter: &Address) -> Result<bool, GovernanceError> {
    let rbac_address = read_address(env, &StorageKey::RbacContract)?;
    let client = RbacContractClient::new(env, &rbac_address);
    Ok(client.has_role(voter, &Role::Admin) || client.has_role(voter, &Role::Employer))
}

fn require_eligible_voter(env: &Env, voter: &Address) -> Result<(), GovernanceError> {
    if is_eligible_voter(env, voter)? {
        Ok(())
    } else {
        Err(GovernanceError::NotEligibleVoter)
    }
}

fn is_multisig_signer(env: &Env, signer: &Address) -> Result<bool, GovernanceError> {
    let multisig_address = read_address(env, &StorageKey::MultisigContract)?;
    let client = MultisigContractClient::new(env, &multisig_address);
    let signers = client.get_signers();
    for idx in 0..signers.len() {
        if signers.get(idx).unwrap() == *signer {
            return Ok(true);
        }
    }
    Ok(false)
}

fn authorize_timelock_call(
    env: &Env,
    fn_name: &str,
    args: Vec<Val>,
) -> Result<(), GovernanceError> {
    let timelock_address = read_address(env, &StorageKey::TimelockContract)?;
    env.authorize_as_current_contract(Vec::from_array(
        env,
        [InvokerContractAuthEntry::Contract(SubContractInvocation {
            context: ContractContext {
                contract: timelock_address,
                fn_name: Symbol::new(env, fn_name),
                args,
            },
            sub_invocations: Vec::new(env),
        })],
    ));
    Ok(())
}

fn timelock_queue_kind(env: &Env, proposal_id: u128) -> TimelockOperationKind {
    TimelockOperationKind::AdminChange(
        env.current_contract_address(),
        proposal_payload_hash(env, proposal_id),
    )
}

fn proposal_payload_hash(env: &Env, proposal_id: u128) -> BytesN<32> {
    let mut payload = [0u8; 32];
    payload[16..].copy_from_slice(&proposal_id.to_be_bytes());
    BytesN::from_array(env, &payload)
}

fn queue_timelock_operation(
    env: &Env,
    proposal_id: u128,
) -> Result<TimelockedOperation, GovernanceError> {
    let timelock_address = read_address(env, &StorageKey::TimelockContract)?;
    let timelock_client = WithdrawalTimelockClient::new(env, &timelock_address);
    let caller = env.current_contract_address();
    let kind = timelock_queue_kind(env, proposal_id);

    authorize_timelock_call(
        env,
        "queue",
        Vec::from_array(
            env,
            [caller.clone().into_val(env), kind.clone().into_val(env)],
        ),
    )?;

    let op_id = timelock_client.queue(&caller, &kind);

    timelock_client
        .get_operation(&op_id)
        .ok_or(GovernanceError::TimelockQueueFailed)
}

fn execute_timelock_operation(env: &Env, op_id: u128) -> Result<(), GovernanceError> {
    let timelock_address = read_address(env, &StorageKey::TimelockContract)?;
    let timelock_client = WithdrawalTimelockClient::new(env, &timelock_address);
    let caller = env.current_contract_address();
    let operation = timelock_client
        .get_operation(&op_id)
        .ok_or(GovernanceError::TimelockExecutionFailed)?;

    if operation.status != TimelockOperationStatus::Queued {
        return Err(GovernanceError::TimelockExecutionFailed);
    }
    if env.ledger().timestamp() < operation.eta {
        return Err(GovernanceError::TimelockNotReady);
    }

    authorize_timelock_call(
        env,
        "execute",
        Vec::from_array(env, [caller.clone().into_val(env), op_id.into_val(env)]),
    )?;

    timelock_client.execute(&caller, &op_id);
    Ok(())
}

fn cancel_timelock_operation(env: &Env, op_id: u128) -> Result<(), GovernanceError> {
    let timelock_address = read_address(env, &StorageKey::TimelockContract)?;
    let timelock_client = WithdrawalTimelockClient::new(env, &timelock_address);
    let caller = env.current_contract_address();
    let operation = timelock_client
        .get_operation(&op_id)
        .ok_or(GovernanceError::TimelockCancellationFailed)?;

    if operation.status != TimelockOperationStatus::Queued {
        return Err(GovernanceError::TimelockCancellationFailed);
    }

    authorize_timelock_call(
        env,
        "cancel",
        Vec::from_array(env, [caller.clone().into_val(env), op_id.into_val(env)]),
    )?;

    timelock_client.cancel(&caller, &op_id);
    Ok(())
}

/// Maximum voting period: 90 days in seconds.
/// Prevents misconfiguration that could trap proposals in perpetual voting.
const MAX_VOTING_PERIOD_SECONDS: u64 = 90 * 24 * 3600;

#[contract]
pub struct GovernanceContract;

#[contractimpl]
impl GovernanceContract {
    /// @notice Initializes the governance contract and links its external dependencies.
    /// @dev Can only be called once. The linked timelock must already be configured
    ///      to treat this governance contract address as its admin.
    /// @param owner Address allowed to update configuration and cancel proposals.
    /// @param rbac_contract RBAC contract used to determine proposal and voting eligibility.
    /// @param multisig_contract Multisig contract whose signer set authorizes execution.
    /// @param timelock_contract Timelock contract that queues passed proposals before execution.
    /// @param quorum_votes Minimum number of votes required for a proposal to pass quorum.
    /// @param voting_period_seconds Duration of the voting window in seconds.
    pub fn initialize(
        env: Env,
        owner: Address,
        rbac_contract: Address,
        multisig_contract: Address,
        timelock_contract: Address,
        quorum_votes: u32,
        voting_period_seconds: u64,
    ) -> Result<(), GovernanceError> {
        owner.require_auth();

        if env
            .storage()
            .persistent()
            .get::<_, bool>(&StorageKey::Initialized)
            .unwrap_or(false)
        {
            return Err(GovernanceError::AlreadyInitialized);
        }
        if quorum_votes == 0 {
            return Err(GovernanceError::InvalidQuorum);
        }
        if voting_period_seconds == 0 {
            return Err(GovernanceError::InvalidVotingPeriod);
        }
        if voting_period_seconds > MAX_VOTING_PERIOD_SECONDS {
            return Err(GovernanceError::VotingPeriodTooLong);
        }

        env.storage().persistent().set(&StorageKey::Owner, &owner);
        env.storage()
            .persistent()
            .set(&StorageKey::RbacContract, &rbac_contract);
        env.storage()
            .persistent()
            .set(&StorageKey::MultisigContract, &multisig_contract);
        env.storage()
            .persistent()
            .set(&StorageKey::TimelockContract, &timelock_contract);
        env.storage()
            .persistent()
            .set(&StorageKey::QuorumVotes, &quorum_votes);
        env.storage()
            .persistent()
            .set(&StorageKey::VotingPeriodSeconds, &voting_period_seconds);
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);
        Ok(())
    }

    /// @notice Updates the governance quorum and voting period.
    /// @dev Owner-only. Dependency contract addresses remain fixed after initialization.
    /// @param caller Owner address.
    /// @param quorum_votes New minimum number of participating votes required.
    /// @param voting_period_seconds New voting window length.
    pub fn update_config(
        env: Env,
        caller: Address,
        quorum_votes: u32,
        voting_period_seconds: u64,
    ) -> Result<(), GovernanceError> {
        require_initialized(&env)?;
        require_owner(&env, &caller)?;
        if quorum_votes == 0 {
            return Err(GovernanceError::InvalidQuorum);
        }
        if voting_period_seconds == 0 {
            return Err(GovernanceError::InvalidVotingPeriod);
        }
        if voting_period_seconds > MAX_VOTING_PERIOD_SECONDS {
            return Err(GovernanceError::VotingPeriodTooLong);
        }

        env.storage()
            .persistent()
            .set(&StorageKey::QuorumVotes, &quorum_votes);
        env.storage()
            .persistent()
            .set(&StorageKey::VotingPeriodSeconds, &voting_period_seconds);
        Ok(())
    }

    /// @notice Creates a proposal if the caller has the RBAC `Admin` or `Employer` role.
    /// @dev Eligibility is checked at proposal creation time by querying the linked RBAC contract.
    /// @param proposer Address creating the proposal.
    /// @param kind Encoded proposal action and payload.
    /// @return proposal_id Newly created proposal identifier.
    pub fn create_proposal(
        env: Env,
        proposer: Address,
        kind: ProposalKind,
    ) -> Result<u128, GovernanceError> {
        require_initialized(&env)?;
        proposer.require_auth();
        require_eligible_voter(&env, &proposer)?;

        let voting_period = read_voting_period(&env)?;
        let now = env.ledger().timestamp();
        let proposal_id = next_proposal_id(&env);
        let proposal = Proposal {
            id: proposal_id,
            proposer,
            kind,
            status: ProposalStatus::Active,
            for_votes: 0,
            against_votes: 0,
            abstain_votes: 0,
            start_time: now,
            end_time: now
                .checked_add(voting_period)
                .ok_or(GovernanceError::InvalidVotingPeriod)?,
            timelock_operation_id: None,
            eta: None,
        };

        write_proposal(&env, &proposal);
        Ok(proposal_id)
    }

    /// @notice Casts a single vote on an active proposal.
    /// @dev Only addresses with the RBAC `Admin` or `Employer` role may vote.
    /// @param voter Address casting the vote.
    /// @param proposal_id Proposal identifier.
    /// @param choice Vote choice (`For`, `Against`, or `Abstain`).
    pub fn cast_vote(
        env: Env,
        voter: Address,
        proposal_id: u128,
        choice: VoteChoice,
    ) -> Result<(), GovernanceError> {
        require_initialized(&env)?;
        voter.require_auth();
        require_eligible_voter(&env, &voter)?;

        let mut proposal = read_proposal(&env, proposal_id)?;
        if proposal.status != ProposalStatus::Active {
            return Err(GovernanceError::ProposalNotActive);
        }

        let now = env.ledger().timestamp();
        if now > proposal.end_time {
            return Err(GovernanceError::VotingClosed);
        }

        let vote_key = StorageKey::Vote(proposal_id, voter);
        if env.storage().persistent().has(&vote_key) {
            return Err(GovernanceError::AlreadyVoted);
        }

        match choice {
            VoteChoice::For => proposal.for_votes = proposal.for_votes.saturating_add(1),
            VoteChoice::Against => {
                proposal.against_votes = proposal.against_votes.saturating_add(1)
            }
            VoteChoice::Abstain => {
                proposal.abstain_votes = proposal.abstain_votes.saturating_add(1)
            }
        }

        env.storage().persistent().set(&vote_key, &choice);
        write_proposal(&env, &proposal);
        Ok(())
    }

    /// @notice Finalizes a proposal after voting closes and queues timelocked execution if it passed.
    /// @dev A proposal passes when total participation reaches quorum and `for_votes > against_votes`.
    /// @param env Contract environment.
    /// @param proposal_id Proposal identifier.
    pub fn finalize_proposal(env: Env, proposal_id: u128) -> Result<(), GovernanceError> {
        require_initialized(&env)?;
        let mut proposal = read_proposal(&env, proposal_id)?;
        if proposal.status != ProposalStatus::Active {
            return Err(GovernanceError::ProposalNotActive);
        }

        let now = env.ledger().timestamp();
        if now <= proposal.end_time {
            return Err(GovernanceError::VotingStillOpen);
        }

        let total_votes = proposal
            .for_votes
            .saturating_add(proposal.against_votes)
            .saturating_add(proposal.abstain_votes);
        let quorum_votes = read_quorum_votes(&env)?;
        let outcome = proposal.for_votes.cmp(&proposal.against_votes);

        if total_votes < quorum_votes || outcome != Ordering::Greater {
            proposal.status = ProposalStatus::Defeated;
            write_proposal(&env, &proposal);
            return Ok(());
        }

        let timelock_operation = queue_timelock_operation(&env, proposal_id)?;
        proposal.status = ProposalStatus::Succeeded;
        proposal.timelock_operation_id = Some(timelock_operation.id);
        proposal.eta = Some(timelock_operation.eta);
        write_proposal(&env, &proposal);
        Ok(())
    }

    /// @notice Executes a passed proposal after the timelock has matured.
    /// @dev Execution is restricted to addresses present in the linked multisig signer set.
    /// @param executor Multisig signer authorizing execution.
    /// @param proposal_id Proposal identifier.
    pub fn execute_proposal(
        env: Env,
        executor: Address,
        proposal_id: u128,
    ) -> Result<(), GovernanceError> {
        require_initialized(&env)?;
        executor.require_auth();

        if !is_multisig_signer(&env, &executor)? {
            return Err(GovernanceError::UnauthorizedExecutor);
        }

        let mut proposal = read_proposal(&env, proposal_id)?;
        if proposal.status != ProposalStatus::Succeeded {
            return Err(GovernanceError::ProposalNotSucceeded);
        }

        let timelock_operation_id = proposal
            .timelock_operation_id
            .ok_or(GovernanceError::TimelockExecutionFailed)?;
        execute_timelock_operation(&env, timelock_operation_id)?;

        match &proposal.kind {
            ProposalKind::ParameterChange(key, value) => {
                env.storage()
                    .persistent()
                    .set(&StorageKey::Parameter(key.clone()), value);
            }
            ProposalKind::UpgradeContract(target, new_wasm_hash) => {
                env.storage()
                    .persistent()
                    .set(&StorageKey::ApprovedUpgrade(target.clone()), new_wasm_hash);
            }
            ProposalKind::ArbiterChange(new_arbiter) => {
                env.storage()
                    .persistent()
                    .set(&StorageKey::Arbiter, new_arbiter);
            }
        }

        proposal.status = ProposalStatus::Executed;
        write_proposal(&env, &proposal);
        Ok(())
    }

    /// @notice Cancels a proposal that has not yet been executed.
    /// @dev Owner-only. If a passed proposal already queued a timelock operation, that
    ///      operation is cancelled first so execution cannot later proceed.
    /// @param caller Owner address.
    /// @param proposal_id Proposal identifier.
    pub fn cancel_proposal(
        env: Env,
        caller: Address,
        proposal_id: u128,
    ) -> Result<(), GovernanceError> {
        require_initialized(&env)?;
        require_owner(&env, &caller)?;

        let mut proposal = read_proposal(&env, proposal_id)?;
        if proposal.status == ProposalStatus::Executed
            || proposal.status == ProposalStatus::Defeated
        {
            return Err(GovernanceError::ProposalNotActive);
        }

        if proposal.status == ProposalStatus::Succeeded {
            if let Some(op_id) = proposal.timelock_operation_id {
                cancel_timelock_operation(&env, op_id)?;
            }
        }

        proposal.status = ProposalStatus::Cancelled;
        write_proposal(&env, &proposal);
        Ok(())
    }

    /// @notice Backward-compatible alias for `create_proposal`.
    pub fn propose(
        env: Env,
        proposer: Address,
        kind: ProposalKind,
    ) -> Result<u128, GovernanceError> {
        Self::create_proposal(env, proposer, kind)
    }

    /// @notice Backward-compatible alias for `cast_vote`.
    pub fn vote(
        env: Env,
        voter: Address,
        proposal_id: u128,
        choice: VoteChoice,
    ) -> Result<(), GovernanceError> {
        Self::cast_vote(env, voter, proposal_id, choice)
    }

    /// @notice Backward-compatible alias for `finalize_proposal`.
    pub fn queue(env: Env, proposal_id: u128) -> Result<(), GovernanceError> {
        Self::finalize_proposal(env, proposal_id)
    }

    /// @notice Backward-compatible alias for `execute_proposal`.
    pub fn execute(env: Env, executor: Address, proposal_id: u128) -> Result<(), GovernanceError> {
        Self::execute_proposal(env, executor, proposal_id)
    }

    /// @notice Backward-compatible alias for `cancel_proposal`.
    pub fn cancel(env: Env, caller: Address, proposal_id: u128) -> Result<(), GovernanceError> {
        Self::cancel_proposal(env, caller, proposal_id)
    }

    /// @notice Returns the current governance configuration.
    /// @return owner, rbac_contract, multisig_contract, timelock_contract, quorum_votes, voting_period_seconds.
    pub fn get_config(
        env: Env,
    ) -> Result<(Address, Address, Address, Address, u32, u64), GovernanceError> {
        require_initialized(&env)?;
        Ok((
            read_owner(&env)?,
            read_address(&env, &StorageKey::RbacContract)?,
            read_address(&env, &StorageKey::MultisigContract)?,
            read_address(&env, &StorageKey::TimelockContract)?,
            read_quorum_votes(&env)?,
            read_voting_period(&env)?,
        ))
    }

    /// @notice Returns a proposal by id if it exists.
    pub fn get_proposal(env: Env, proposal_id: u128) -> Option<Proposal> {
        env.storage()
            .persistent()
            .get(&StorageKey::Proposal(proposal_id))
    }

    /// @notice Returns the vote choice cast by a voter on a proposal, if any.
    pub fn get_vote(env: Env, proposal_id: u128, voter: Address) -> Option<VoteChoice> {
        env.storage()
            .persistent()
            .get(&StorageKey::Vote(proposal_id, voter))
    }

    /// @notice Returns a stored governance parameter.
    pub fn get_parameter(env: Env, key: Symbol) -> Option<i128> {
        env.storage().persistent().get(&StorageKey::Parameter(key))
    }

    /// @notice Returns the last approved arbiter address.
    pub fn get_arbiter(env: Env) -> Option<Address> {
        env.storage().persistent().get(&StorageKey::Arbiter)
    }

    /// @notice Returns the last approved upgrade hash for a target contract.
    pub fn get_approved_upgrade(env: Env, target: Address) -> Option<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&StorageKey::ApprovedUpgrade(target))
    }
}
