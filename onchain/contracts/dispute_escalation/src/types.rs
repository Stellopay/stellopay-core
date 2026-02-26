use soroban_sdk::{contracterror, contracttype, Address};

/// Represents the level of escalation for a dispute.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EscalationLevel {
    /// Level 1: Initial dispute handling (e.g., automated resolution or primary arbiter)
    Level1,
    /// Level 2: Escalated dispute (e.g., senior arbiter review)
    Level2,
    /// Level 3: Final appeal (e.g., specialized committee or external oracle)
    Level3,
}

/// The state of a dispute.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisputeStatus {
    /// Dispute has been opened but not yet resolved
    Open,
    /// Dispute is currently escalated to a higher level
    Escalated,
    /// A party has appealed a previous ruling
    Appealed,
    /// Final resolution has been reached
    Resolved,
}

/// Holds all relevant information for an active dispute.
#[contracttype]
#[derive(Clone, Debug)]
pub struct DisputeDetails {
    /// ID of the agreement or transaction under dispute
    pub agreement_id: u128,
    /// The party who filed the dispute or appeal
    pub initiator: Address,
    /// The current status of the dispute
    pub status: DisputeStatus,
    /// The current escalation level
    pub level: EscalationLevel,
    /// The timestamp when the current phase started (for time limits)
    pub phase_started_at: u64,
    /// The timestamp when the current phase expires
    pub phase_deadline: u64,
}

/// Storage keys for the dispute escalation contract.
#[contracttype]
#[derive(Clone)]
pub enum StorageKey {
    Owner,
    Admin,
    /// Dispute Details: agreement_id -> DisputeDetails
    Dispute(u128),
    /// Max time allowed per escalation level in seconds
    LevelTimeLimit(EscalationLevel),
}

/// Errors specific to the dispute escalation logic.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum DisputeError {
    /// Action requires administrative privileges
    Unauthorized = 1,
    /// The specified dispute does not exist
    DisputeNotFound = 2,
    /// Dispute is already in a final state
    AlreadyResolved = 3,
    /// Attempted to escalate beyond the maximum level
    MaxEscalationReached = 4,
    /// The time limit to take the current action has expired
    TimeLimitExpired = 5,
    /// Improper state transition (e.g., appealing a non-resolved dispute)
    InvalidTransition = 6,
    /// Only a party to the dispute can appeal
    NotParty = 7,
}
