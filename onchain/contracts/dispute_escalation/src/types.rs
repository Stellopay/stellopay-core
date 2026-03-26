use soroban_sdk::{contracterror, contracttype, Address};

/// Represents the level of escalation for a dispute.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EscalationLevel {
    /// Level 1: Initial dispute handling (primary arbiter).
    Level1,
    /// Level 2: Escalated review (senior arbiter).
    Level2,
    /// Level 3: Final appeal tier (committee / external oracle). Binding; no further appeal.
    Level3,
}

/// Outcome recorded when a dispute is resolved.
///
/// The outcome is binding once stored and drives payroll-state integration:
/// downstream contracts (e.g. payroll escrow) listen for `dispute_resolved`
/// events and act on the `outcome` field.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisputeOutcome {
    /// No ruling yet (dispute open or under appeal).
    Unset,
    /// Ruling in favour of the employer / payer — withheld payment is released.
    UpholdPayment,
    /// Ruling in favour of the employee / claimant — payment is awarded.
    GrantClaim,
    /// Partial settlement — escrow is split per off-chain agreement.
    PartialSettlement,
}

/// The state of a dispute in the escalation state machine.
///
/// ## State machine transitions
///
/// ```text
/// ┌─────────────────────────────────────────────────────────────────────┐
/// │  file_dispute                                                       │
/// │      │                                                             │
/// │      ▼                                                             │
/// │    Open ──── escalate_dispute ──────────────────────► Escalated   │
/// │      │   (within deadline)              (within deadline)    │     │
/// │      │                                                        │     │
/// │      │ expire_dispute (deadline passed)                       │     │
/// │      ▼                                                        │     │
/// │   Expired ◄─────── expire_dispute ────────────────────────────     │
/// │  (terminal)                                                         │
/// │                                                                     │
/// │  resolve_dispute (admin, Level1/2)                                  │
/// │      │                                                              │
/// │      ▼                                                              │
/// │   Resolved ──── appeal_ruling (within window, level < 3) ──► Appealed
/// │  (Level1/2)                                                    │    │
/// │      │                                                         │    │
/// │      │ appeal window passes → de-facto binding                │    │
/// │                                                                │    │
/// │  resolve_dispute (admin, Level3)                               │    │
/// │      │◄───────────────────────────────────────────────────────     │
/// │      ▼                                                              │
/// │  Finalised ─── no further appeal (AlreadyFinalised)                │
/// │  (terminal)                                                         │
/// └─────────────────────────────────────────────────────────────────────┘
/// ```
///
/// Terminal states: `Finalised`, `Expired`.
/// Cannot double-resolve: `AlreadyResolved` / `AlreadyFinalised` guard every
/// resolve path.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisputeStatus {
    /// Dispute has been opened but not yet resolved.
    Open,
    /// Dispute has been moved to a higher escalation level.
    Escalated,
    /// A party has appealed a previous ruling.
    Appealed,
    /// Admin has issued a ruling (appeal window is open for Level1/2).
    Resolved,
    /// Level3 resolution — truly final, no further appeal possible.
    Finalised,
    /// Deadline passed without action; dispute is closed with no ruling.
    Expired,
}

/// Holds all relevant information for an active dispute.
#[contracttype]
#[derive(Clone, Debug)]
pub struct DisputeDetails {
    /// ID of the agreement or transaction under dispute.
    pub agreement_id: u128,
    /// The party who filed the dispute or most recent appeal.
    pub initiator: Address,
    /// The current status of the dispute.
    pub status: DisputeStatus,
    /// The current escalation level.
    pub level: EscalationLevel,
    /// The timestamp when the current phase started.
    pub phase_started_at: u64,
    /// The timestamp when the current phase expires.
    pub phase_deadline: u64,
    /// The binding outcome once resolved or finalised; [`DisputeOutcome::Unset`] while open.
    pub outcome: DisputeOutcome,
}

/// Storage keys for the dispute escalation contract.
#[contracttype]
#[derive(Clone)]
pub enum StorageKey {
    Owner,
    Admin,
    /// Dispute Details: agreement_id → DisputeDetails
    Dispute(u128),
    /// Max time allowed per escalation level in seconds.
    LevelTimeLimit(EscalationLevel),
}

/// Errors specific to the dispute escalation logic.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum DisputeError {
    /// Action requires administrative privileges.
    Unauthorized = 1,
    /// The specified dispute does not exist.
    DisputeNotFound = 2,
    /// Dispute is already resolved (cannot resolve twice).
    AlreadyResolved = 3,
    /// Attempted to escalate or appeal beyond Level3.
    MaxEscalationReached = 4,
    /// The time limit to take the current action has passed.
    TimeLimitExpired = 5,
    /// Improper state transition (e.g. appealing a non-resolved dispute).
    InvalidTransition = 6,
    /// Only a party to the dispute can appeal.
    NotParty = 7,
    /// Dispute is at Level3 — binding and final, no further appeal.
    AlreadyFinalised = 8,
    /// Dispute deadline has not yet passed; cannot expire it early.
    DeadlineNotPassed = 9,
    /// Dispute is already in a terminal state (Finalised or Expired).
    AlreadyTerminal = 10,
}
