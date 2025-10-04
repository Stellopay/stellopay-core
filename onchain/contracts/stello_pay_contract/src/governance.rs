use soroban_sdk::{contracttype, Address, Env, Map, String, Symbol, Vec};

//-----------------------------------------------------------------------------
// Governance Data Structures
//-----------------------------------------------------------------------------

/// Governance token information
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct GovernanceToken {
    pub token_address: Address,
    pub total_supply: i128,
    pub voting_power_multiplier: u32, // Multiplier for voting power (e.g., 100 = 1:1, 200 = 2:1)
    pub min_balance_to_propose: i128, // Minimum token balance required to create proposals
    pub is_active: bool,
}

/// Proposal structure for governance decisions
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub proposer: Address,
    pub proposal_type: ProposalType,
    pub target_contract: Option<Address>,
    pub call_data: Option<String>, // Serialized call data for execution
    pub voting_start: u64,
    pub voting_end: u64,
    pub execution_delay: u64, // Time delay before execution after approval
    pub min_quorum: u64,      // Minimum participation required (percentage * 100)
    pub approval_threshold: u64, // Minimum approval percentage required (* 100)
    pub status: ProposalStatus,
    pub created_at: u64,
    pub executed_at: Option<u64>,
    pub cancelled_at: Option<u64>,
}

/// Types of governance proposals
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalType {
    ParameterUpdate,   // Update contract parameters
    TreasurySpend,     // Spend from community treasury
    TokenDistribution, // Distribute governance tokens
    ContractUpgrade,   // Upgrade contract functionality
    PolicyChange,      // Change governance policies
    EmergencyAction,   // Emergency governance action
    Custom(String),    // Custom proposal type
}

/// Proposal status enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalStatus {
    Draft,     // Proposal created but not yet active
    Active,    // Currently accepting votes
    Succeeded, // Passed voting requirements
    Defeated,  // Failed voting requirements
    Queued,    // Approved and waiting for execution delay
    Executed,  // Successfully executed
    Cancelled, // Cancelled by proposer or governance
    Expired,   // Voting period expired without execution
}

/// Vote record for a specific proposal
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Vote {
    pub voter: Address,
    pub proposal_id: u64,
    pub support: VoteType,
    pub voting_power: u64,
    pub reason: Option<String>,
    pub timestamp: u64,
}

/// Vote type enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum VoteType {
    For,     // Vote in favor
    Against, // Vote against
    Abstain, // Abstain from voting
}

/// Voting results for a proposal
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct VotingResults {
    pub proposal_id: u64,
    pub total_votes: u64,
    pub votes_for: u64,
    pub votes_against: u64,
    pub votes_abstain: u64,
    pub total_voting_power: u64,
    pub participation_rate: u64, // Percentage * 100
    pub approval_rate: u64,      // Percentage * 100
    pub quorum_reached: bool,
    pub threshold_met: bool,
}

/// Community treasury structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct CommunityTreasury {
    pub total_value: i128,
    pub token_balances: Map<Address, i128>, // token -> balance
    pub reserved_funds: i128,               // Funds reserved for approved proposals
    pub last_updated: u64,
}

/// Treasury spending proposal
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TreasurySpendProposal {
    pub proposal_id: u64,
    pub recipient: Address,
    pub token: Address,
    pub amount: i128,
    pub purpose: String,
    pub milestones: Vec<TreasuryMilestone>,
    pub approved: bool,
    pub executed: bool,
}

/// Treasury spending milestone
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TreasuryMilestone {
    pub id: u32,
    pub description: String,
    pub amount: i128,
    pub due_date: u64,
    pub completed: bool,
    pub completed_at: Option<u64>,
    pub evidence: Option<String>,
}

/// Governance parameter that can be updated
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct GovernanceParameter {
    pub name: String,
    pub current_value: String,
    pub proposed_value: Option<String>,
    pub parameter_type: ParameterType,
    pub min_value: Option<String>,
    pub max_value: Option<String>,
    pub last_updated: u64,
    pub update_proposal_id: Option<u64>,
}

/// Parameter type enumeration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ParameterType {
    Integer,
    Boolean,
    Address,
    String,
    Percentage,
    Duration,
}

/// Delegation record for voting power
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct VotingDelegation {
    pub delegator: Address,
    pub delegate: Address,
    pub voting_power: u64,
    pub delegated_at: u64,
    pub expires_at: Option<u64>,
    pub is_active: bool,
}

/// Governance statistics
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct GovernanceStats {
    pub total_proposals: u64,
    pub active_proposals: u64,
    pub executed_proposals: u64,
    pub total_voters: u64,
    pub total_voting_power: u64,
    pub average_participation: u64, // Percentage * 100
    pub treasury_value: i128,
    pub last_updated: u64,
}

/// Governance configuration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct GovernanceConfig {
    pub voting_delay: u64,    // Time before voting starts after proposal creation
    pub voting_period: u64,   // Duration of voting period
    pub execution_delay: u64, // Time delay before execution after approval
    pub proposal_threshold: i128, // Minimum tokens needed to create proposal
    pub quorum_threshold: u64, // Minimum participation required (percentage * 100)
    pub approval_threshold: u64, // Minimum approval percentage required (* 100)
    pub emergency_quorum: u64, // Lower quorum for emergency proposals
    pub emergency_threshold: u64, // Lower threshold for emergency proposals
    pub max_operations_per_proposal: u32, // Maximum operations in a single proposal
    pub guardian: Option<Address>, // Emergency guardian address
    pub timelock_enabled: bool, // Whether timelock is enabled for execution
}

//-----------------------------------------------------------------------------
// Governance Storage Keys
//-----------------------------------------------------------------------------

#[contracttype]
pub enum GovernanceDataKey {
    // Core governance
    GovernanceToken,  // GovernanceToken struct
    GovernanceConfig, // GovernanceConfig struct

    // Proposals
    Proposal(u64),                     // proposal_id -> Proposal
    NextProposalId,                    // Next available proposal ID
    ProposalsByStatus(ProposalStatus), // status -> Vec<u64> (proposal IDs)
    ProposerProposals(Address),        // proposer -> Vec<u64> (proposal IDs)
    ActiveProposals,                   // Vec<u64> (currently active proposal IDs)

    // Voting
    Vote(u64, Address),    // (proposal_id, voter) -> Vote
    VotingResults(u64),    // proposal_id -> VotingResults
    VoterHistory(Address), // voter -> Vec<u64> (proposal IDs voted on)
    ProposalVotes(u64),    // proposal_id -> Vec<Address> (voters)

    // Delegation
    VotingDelegation(Address), // delegator -> VotingDelegation
    DelegateVoters(Address),   // delegate -> Vec<Address> (delegators)
    VotingPower(Address),      // address -> current voting power

    // Treasury
    CommunityTreasury,          // CommunityTreasury struct
    TreasurySpendProposal(u64), // proposal_id -> TreasurySpendProposal
    TreasuryBalance(Address),   // token -> balance in treasury
    ReservedFunds(Address),     // token -> reserved amount

    // Parameters
    GovernanceParameter(String), // parameter_name -> GovernanceParameter
    ParameterHistory(String),    // parameter_name -> Vec<(u64, String)> (timestamp, value)

    // Statistics and tracking
    GovernanceStats,     // GovernanceStats struct
    VoterStats(Address), // voter -> participation stats
    ProposalStats(u64),  // proposal_id -> detailed stats

    // Token distribution
    TokenDistribution(Address), // recipient -> amount distributed
    DistributionRound(u64),     // round_id -> distribution details
    NextDistributionRound,      // Next distribution round ID

    // Emergency features
    EmergencyProposals, // Vec<u64> (emergency proposal IDs)
    GuardianActions,    // Vec<(u64, String)> (timestamp, action)
}

//-----------------------------------------------------------------------------
// Governance Events
//-----------------------------------------------------------------------------

/// Event emitted when a proposal is created
pub const PROPOSAL_CREATED_EVENT: Symbol = soroban_sdk::symbol_short!("prop_cr");

/// Event emitted when a vote is cast
pub const VOTE_CAST_EVENT: Symbol = soroban_sdk::symbol_short!("vote_cs");

/// Event emitted when a proposal is executed
pub const PROPOSAL_EXECUTED_EVENT: Symbol = soroban_sdk::symbol_short!("prop_ex");

/// Event emitted when voting power is delegated
pub const VOTING_DELEGATED_EVENT: Symbol = soroban_sdk::symbol_short!("vote_dl");

/// Event emitted when treasury funds are spent
pub const TREASURY_SPEND_EVENT: Symbol = soroban_sdk::symbol_short!("treas_sp");

/// Event emitted when governance parameters are updated
pub const PARAMETER_UPDATED_EVENT: Symbol = soroban_sdk::symbol_short!("param_up");

/// Event emitted when tokens are distributed
pub const TOKENS_DISTRIBUTED_EVENT: Symbol = soroban_sdk::symbol_short!("tok_dist");

//-----------------------------------------------------------------------------
// Governance Error Types
//-----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum GovernanceError {
    // Proposal errors
    ProposalNotFound,
    ProposalNotActive,
    ProposalAlreadyExecuted,
    ProposalExpired,
    InsufficientTokensToPropose,
    InvalidProposalPeriod,
    InvalidQuorumThreshold,
    InvalidApprovalThreshold,

    // Voting errors
    VotingNotStarted,
    VotingEnded,
    AlreadyVoted,
    InsufficientVotingPower,
    InvalidVoteType,

    // Delegation errors
    CannotDelegateToSelf,
    DelegationNotFound,
    DelegationExpired,
    InvalidDelegation,

    // Treasury errors
    InsufficientTreasuryFunds,
    TreasuryProposalNotApproved,
    InvalidTreasuryAmount,
    TreasuryTransferFailed,

    // Parameter errors
    ParameterNotFound,
    InvalidParameterValue,
    ParameterUpdateNotApproved,
    InvalidParameterType,

    // Authorization errors
    NotAuthorized,
    NotProposer,
    NotGuardian,

    // Execution errors
    ExecutionDelayNotMet,
    ExecutionFailed,
    InvalidCallData,

    // General errors
    InvalidConfiguration,
    GovernanceNotInitialized,
    EmergencyModeActive,
}

//-----------------------------------------------------------------------------
// Governance System Implementation
//-----------------------------------------------------------------------------

pub struct GovernanceSystem;

impl GovernanceSystem {
    /// Initialize the governance system
    pub fn initialize(
        env: &Env,
        governance_token: Address,
        initial_config: GovernanceConfig,
        initial_treasury: CommunityTreasury,
    ) -> Result<(), GovernanceError> {
        let storage = env.storage().persistent();

        // Check if already initialized
        if storage.has(&GovernanceDataKey::GovernanceConfig) {
            return Err(GovernanceError::GovernanceNotInitialized);
        }

        // Set up governance token
        let gov_token = GovernanceToken {
            token_address: governance_token,
            total_supply: 1_000_000_000,  // 1 billion tokens
            voting_power_multiplier: 100, // 1:1 ratio
            min_balance_to_propose: initial_config.proposal_threshold,
            is_active: true,
        };

        storage.set(&GovernanceDataKey::GovernanceToken, &gov_token);
        storage.set(&GovernanceDataKey::GovernanceConfig, &initial_config);
        storage.set(&GovernanceDataKey::CommunityTreasury, &initial_treasury);
        storage.set(&GovernanceDataKey::NextProposalId, &1u64);

        // Initialize governance statistics
        let stats = GovernanceStats {
            total_proposals: 0,
            active_proposals: 0,
            executed_proposals: 0,
            total_voters: 0,
            total_voting_power: 0,
            average_participation: 0,
            treasury_value: initial_treasury.total_value,
            last_updated: env.ledger().timestamp(),
        };

        storage.set(&GovernanceDataKey::GovernanceStats, &stats);

        Ok(())
    }

    /// Get governance configuration
    pub fn get_config(env: &Env) -> Option<GovernanceConfig> {
        env.storage()
            .persistent()
            .get(&GovernanceDataKey::GovernanceConfig)
    }

    /// Get governance token information
    pub fn get_governance_token(env: &Env) -> Option<GovernanceToken> {
        env.storage()
            .persistent()
            .get(&GovernanceDataKey::GovernanceToken)
    }

    /// Get community treasury information
    pub fn get_treasury(env: &Env) -> Option<CommunityTreasury> {
        env.storage()
            .persistent()
            .get(&GovernanceDataKey::CommunityTreasury)
    }

    /// Get governance statistics
    pub fn get_stats(env: &Env) -> Option<GovernanceStats> {
        env.storage()
            .persistent()
            .get(&GovernanceDataKey::GovernanceStats)
    }
}
