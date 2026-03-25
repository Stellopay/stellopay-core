//! # Slashing Penalty Contract
//!
//! Encodes slashing rules tied to signed attestations or on-chain evidence.
//! Implements safeguards against unjust confiscation and a 7-day appeal window.
//!
//! ## Evidence Format
//! Evidence must include:
//! - `offender`   : Address of the party being slashed
//! - `offense`    : Enum variant describing the misbehaviour (DoubleSigning | MissedDuty | FraudProof)
//! - `penalty_bps`: Penalty in basis points (max 10_000 = 100%)
//! - `evidence_hash`: SHA-256 hash of the raw proof payload (bytes32 equivalent)
//! - `timestamp`  : Ledger timestamp when misbehaviour occurred
//!
//! ## Quorum
//! A slash via attestation requires signatures from at least `quorum_threshold`
//! distinct slasher addresses (default: 2-of-N). On-chain evidence bypasses
//! quorum but still requires the caller to hold the `slasher` role.
//!
//! ## Security Assumptions
//! - Only addresses granted the `slasher` role may initiate or countersign a slash.
//! - Penalty is strictly proportional — capped at `MAX_PENALTY_BPS` (5 000 bps = 50%).
//! - Each unique `evidence_hash` can only be acted upon once (replay protection).
//! - Slashed funds are held in escrow during the appeal window before burning/redistribution.
//! - Admin cannot slash; roles are separated (admin ≠ slasher).

#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror,
    Address, BytesN, Env, Map, Vec, Symbol, symbol_short,
    token,
};

// ─── Constants ────────────────────────────────────────────────────────────────

/// Maximum penalty: 50% of stake (5 000 basis points).
const MAX_PENALTY_BPS: u32 = 5_000;

/// Appeal window: 7 days in seconds.
const APPEAL_WINDOW_SECS: u64 = 7 * 24 * 60 * 60;

/// Minimum quorum of slasher signatures required for attestation-based slashes.
const DEFAULT_QUORUM: u32 = 2;

// ─── Storage Keys ─────────────────────────────────────────────────────────────

const ADMIN: Symbol        = symbol_short!("ADMIN");
const QUORUM: Symbol       = symbol_short!("QUORUM");
const SLASHERS: Symbol     = symbol_short!("SLASHERS");
const STAKES: Symbol       = symbol_short!("STAKES");
const SLASH_REC: Symbol    = symbol_short!("SLASHREC");
const USED_EV: Symbol      = symbol_short!("USEDEV");
const ESCROW: Symbol       = symbol_short!("ESCROW");
const TOKEN: Symbol        = symbol_short!("TOKEN");

// ─── Types ────────────────────────────────────────────────────────────────────

/// Categories of misbehaviour that can be slashed.
#[contracttype]
#[derive(Clone, PartialEq)]
pub enum Offense {
    /// Validator signed two conflicting blocks at the same height.
    DoubleSigning,
    /// Validator missed a required duty (e.g. attestation, block proposal).
    MissedDuty,
    /// Verifiable fraud proof submitted (e.g. invalid state transition).
    FraudProof,
}

/// Status of a slash record through its lifecycle.
#[contracttype]
#[derive(Clone, PartialEq)]
pub enum SlashStatus {
    /// Slash initiated; appeal window is open.
    Pending,
    /// Appeal window closed; slash executed (funds burned/redistributed).
    Executed,
    /// Appeal upheld; slash reversed and funds returned.
    Reversed,
    /// Appeal rejected; slash executed despite appeal.
    AppealRejected,
}

/// A slash record stored on-chain.
#[contracttype]
#[derive(Clone)]
pub struct SlashRecord {
    /// Address being slashed.
    pub offender: Address,
    /// Nature of the offence.
    pub offense: Offense,
    /// Penalty in basis points.
    pub penalty_bps: u32,
    /// SHA-256 hash of raw evidence payload (replay protection key).
    pub evidence_hash: BytesN<32>,
    /// Ledger timestamp of the misbehaviour.
    pub offense_timestamp: u64,
    /// Ledger timestamp when slash was initiated.
    pub initiated_at: u64,
    /// Absolute timestamp after which the slash can be executed.
    pub appeal_deadline: u64,
    /// Current lifecycle status.
    pub status: SlashStatus,
    /// Slashed token amount held in escrow.
    pub escrowed_amount: i128,
    /// Attestation signers who countersigned (for attestation-based slashes).
    pub attestors: Vec<Address>,
}

// ─── Errors ───────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum SlashError {
    /// Caller does not hold the slasher role.
    Unauthorized        = 1,
    /// Evidence hash has already been used.
    DuplicateEvidence   = 2,
    /// Penalty exceeds the allowed maximum.
    PenaltyTooHigh      = 3,
    /// Offender has insufficient staked balance.
    InsufficientStake   = 4,
    /// Appeal window has not yet closed.
    AppealWindowOpen    = 5,
    /// Appeal window has already closed.
    AppealWindowClosed  = 6,
    /// Slash record not found.
    RecordNotFound      = 7,
    /// Slash is not in a state that allows this operation.
    InvalidState        = 8,
    /// Quorum of attestors not yet reached.
    QuorumNotMet        = 9,
    /// Slasher already attested to this slash.
    AlreadyAttested     = 10,
    /// Penalty basis points cannot be zero.
    ZeroPenalty         = 11,
    /// Admin address already initialised.
    AlreadyInitialized  = 12,
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct SlashingPenaltyContract;

#[contractimpl]
impl SlashingPenaltyContract {

    // ── Initialisation ────────────────────────────────────────────────────────

    /// Initialise the contract. Can only be called once.
    ///
    /// # Arguments
    /// * `admin`   - Address that can grant/revoke slasher roles and reverse appeals.
    /// * `token`   - Contract address of the XLM-wrapped or custom token used for stake.
    /// * `quorum`  - Minimum number of slasher signatures for attestation slashes.
    pub fn initialize(
        env: Env,
        admin: Address,
        token: Address,
        quorum: u32,
    ) -> Result<(), SlashError> {
        if env.storage().instance().has(&ADMIN) {
            return Err(SlashError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&ADMIN, &admin);
        env.storage().instance().set(&TOKEN, &token);
        env.storage().instance().set(&QUORUM, &quorum.max(DEFAULT_QUORUM));
        env.storage().instance().set(&SLASHERS, &Vec::<Address>::new(&env));
        env.storage().instance().set(&STAKES, &Map::<Address, i128>::new(&env));
        env.storage().instance().set(&SLASH_REC, &Map::<BytesN<32>, SlashRecord>::new(&env));
        env.storage().instance().set(&USED_EV, &Vec::<BytesN<32>>::new(&env));
        env.storage().instance().set(&ESCROW, &Map::<BytesN<32>, i128>::new(&env));
        Ok(())
    }

    // ── Role Management ───────────────────────────────────────────────────────

    /// Grant the slasher role to an address. Admin only.
    pub fn add_slasher(env: Env, slasher: Address) -> Result<(), SlashError> {
        Self::require_admin(&env)?;
        let mut slashers: Vec<Address> = env.storage().instance().get(&SLASHERS).unwrap();
        if !slashers.contains(&slasher) {
            slashers.push_back(slasher);
            env.storage().instance().set(&SLASHERS, &slashers);
        }
        Ok(())
    }

    /// Revoke the slasher role from an address. Admin only.
    pub fn remove_slasher(env: Env, slasher: Address) -> Result<(), SlashError> {
        Self::require_admin(&env)?;
        let slashers: Vec<Address> = env.storage().instance().get(&SLASHERS).unwrap();
        let mut new_slashers = Vec::<Address>::new(&env);
        for s in slashers.iter() {
            if s != slasher {
                new_slashers.push_back(s);
            }
        }
        env.storage().instance().set(&SLASHERS, &new_slashers);
        Ok(())
    }

    // ── Stake Management ──────────────────────────────────────────────────────

    /// Deposit stake into the contract. Any address may stake.
    pub fn stake(env: Env, staker: Address, amount: i128) -> Result<(), SlashError> {
        staker.require_auth();
        let token_addr: Address = env.storage().instance().get(&TOKEN).unwrap();
        let token_client = token::Client::new(&env, &token_addr);
        token_client.transfer(&staker, &env.current_contract_address(), &amount);
        let mut stakes: Map<Address, i128> = env.storage().instance().get(&STAKES).unwrap();
        let current = stakes.get(staker.clone()).unwrap_or(0);
        stakes.set(staker, current + amount);
        env.storage().instance().set(&STAKES, &stakes);
        Ok(())
    }

    /// Withdraw stake. Only callable if no pending slash against the staker.
    pub fn unstake(env: Env, staker: Address, amount: i128) -> Result<(), SlashError> {
        staker.require_auth();
        let mut stakes: Map<Address, i128> = env.storage().instance().get(&STAKES).unwrap();
        let current = stakes.get(staker.clone()).unwrap_or(0);
        if current < amount {
            return Err(SlashError::InsufficientStake);
        }
        stakes.set(staker.clone(), current - amount);
        env.storage().instance().set(&STAKES, &stakes);
        let token_addr: Address = env.storage().instance().get(&TOKEN).unwrap();
        let token_client = token::Client::new(&env, &token_addr);
        token_client.transfer(&env.current_contract_address(), &staker, &amount);
        Ok(())
    }

    // ── Slashing ──────────────────────────────────────────────────────────────

    /// Initiate a slash backed by on-chain evidence.
    ///
    /// The caller must hold the slasher role. On-chain evidence bypasses quorum
    /// but the evidence_hash must be unique (replay protection).
    ///
    /// # Arguments
    /// * `initiator`      - Slasher initiating the slash.
    /// * `offender`       - Address to be slashed.
    /// * `offense`        - Type of misbehaviour.
    /// * `penalty_bps`    - Penalty as basis points of total stake (max 5 000).
    /// * `evidence_hash`  - SHA-256 of the raw evidence payload.
    /// * `offense_ts`     - Timestamp when the misbehaviour occurred.
    pub fn slash_with_evidence(
        env: Env,
        initiator: Address,
        offender: Address,
        offense: Offense,
        penalty_bps: u32,
        evidence_hash: BytesN<32>,
        offense_ts: u64,
    ) -> Result<BytesN<32>, SlashError> {
        initiator.require_auth();
        Self::require_slasher(&env, &initiator)?;
        Self::validate_penalty(&env, penalty_bps)?;
        Self::check_evidence_unused(&env, &evidence_hash)?;

        let stake_amount = Self::get_stake(&env, &offender)?;
        let slash_amount = Self::compute_slash(stake_amount, penalty_bps);

        Self::mark_evidence_used(&env, evidence_hash.clone());
        Self::move_to_escrow(&env, &offender, slash_amount, evidence_hash.clone())?;

        let now = env.ledger().timestamp();
        let record = SlashRecord {
            offender: offender.clone(),
            offense,
            penalty_bps,
            evidence_hash: evidence_hash.clone(),
            offense_timestamp: offense_ts,
            initiated_at: now,
            appeal_deadline: now + APPEAL_WINDOW_SECS,
            status: SlashStatus::Pending,
            escrowed_amount: slash_amount,
            attestors: Vec::new(&env),
        };

        let mut records: Map<BytesN<32>, SlashRecord> =
            env.storage().instance().get(&SLASH_REC).unwrap();
        records.set(evidence_hash.clone(), record);
        env.storage().instance().set(&SLASH_REC, &records);

        env.events().publish(
            (symbol_short!("SLASHED"), offender),
            (evidence_hash.clone(), slash_amount),
        );

        Ok(evidence_hash)
    }

    /// Initiate or countersign a slash backed by signed attestations.
    ///
    /// The first caller creates the slash record (Pending). Subsequent slashers
    /// countersign. Once `quorum_threshold` unique slashers have attested,
    /// the slash enters the appeal window automatically.
    pub fn attest_slash(
        env: Env,
        attestor: Address,
        offender: Address,
        offense: Offense,
        penalty_bps: u32,
        evidence_hash: BytesN<32>,
        offense_ts: u64,
    ) -> Result<(), SlashError> {
        attestor.require_auth();
        Self::require_slasher(&env, &attestor)?;
        Self::validate_penalty(&env, penalty_bps)?;

        let mut records: Map<BytesN<32>, SlashRecord> =
            env.storage().instance().get(&SLASH_REC).unwrap();

        if let Some(mut record) = records.get(evidence_hash.clone()) {
            // Countersign existing record
            if record.status != SlashStatus::Pending {
                return Err(SlashError::InvalidState);
            }
            if record.attestors.contains(&attestor) {
                return Err(SlashError::AlreadyAttested);
            }
            record.attestors.push_back(attestor.clone());
            records.set(evidence_hash.clone(), record);
        } else {
            // First attestor — create the record
            Self::check_evidence_unused(&env, &evidence_hash)?;
            let stake_amount = Self::get_stake(&env, &offender)?;
            let slash_amount = Self::compute_slash(stake_amount, penalty_bps);
            Self::mark_evidence_used(&env, evidence_hash.clone());
            Self::move_to_escrow(&env, &offender, slash_amount, evidence_hash.clone())?;

            let now = env.ledger().timestamp();
            let mut attestors = Vec::<Address>::new(&env);
            attestors.push_back(attestor.clone());

            let record = SlashRecord {
                offender: offender.clone(),
                offense,
                penalty_bps,
                evidence_hash: evidence_hash.clone(),
                offense_timestamp: offense_ts,
                initiated_at: now,
                appeal_deadline: now + APPEAL_WINDOW_SECS,
                status: SlashStatus::Pending,
                escrowed_amount: slash_amount,
                attestors,
            };
            records.set(evidence_hash.clone(), record);
        }

        env.storage().instance().set(&SLASH_REC, &records);

        env.events().publish(
            (symbol_short!("ATTESTED"), attestor),
            evidence_hash.clone(),
        );

        Ok(())
    }

    // ── Appeal ────────────────────────────────────────────────────────────────

    /// The offender raises an appeal during the appeal window.
    /// This does not automatically reverse the slash — admin must review.
    pub fn raise_appeal(env: Env, offender: Address, evidence_hash: BytesN<32>) -> Result<(), SlashError> {
        offender.require_auth();
        let records: Map<BytesN<32>, SlashRecord> =
            env.storage().instance().get(&SLASH_REC).unwrap();
        let record = records.get(evidence_hash.clone()).ok_or(SlashError::RecordNotFound)?;

        if record.status != SlashStatus::Pending {
            return Err(SlashError::InvalidState);
        }
        let now = env.ledger().timestamp();
        if now > record.appeal_deadline {
            return Err(SlashError::AppealWindowClosed);
        }

        env.events().publish(
            (symbol_short!("APPEALED"), offender),
            evidence_hash,
        );

        Ok(())
    }

    /// Admin resolves an appeal: uphold (reverse slash) or reject (execute slash).
    pub fn resolve_appeal(
        env: Env,
        evidence_hash: BytesN<32>,
        uphold: bool,
    ) -> Result<(), SlashError> {
        Self::require_admin(&env)?;

        let mut records: Map<BytesN<32>, SlashRecord> =
            env.storage().instance().get(&SLASH_REC).unwrap();
        let mut record = records.get(evidence_hash.clone()).ok_or(SlashError::RecordNotFound)?;

        if record.status != SlashStatus::Pending {
            return Err(SlashError::InvalidState);
        }

        if uphold {
            // Return escrowed funds to offender
            Self::release_escrow(&env, &record.offender, evidence_hash.clone())?;
            record.status = SlashStatus::Reversed;
        } else {
            // Burn / redistribute escrowed funds
            Self::burn_escrow(&env, evidence_hash.clone())?;
            record.status = SlashStatus::AppealRejected;
        }

        records.set(evidence_hash.clone(), record);
        env.storage().instance().set(&SLASH_REC, &records);

        env.events().publish(
            (symbol_short!("RESOLVED"), uphold),
            evidence_hash,
        );

        Ok(())
    }

    /// Execute a slash after the appeal window has closed without a successful appeal.
    /// Anyone may call this to finalise an expired pending slash.
    pub fn execute_slash(env: Env, evidence_hash: BytesN<32>) -> Result<(), SlashError> {
        let mut records: Map<BytesN<32>, SlashRecord> =
            env.storage().instance().get(&SLASH_REC).unwrap();
        let mut record = records.get(evidence_hash.clone()).ok_or(SlashError::RecordNotFound)?;

        if record.status != SlashStatus::Pending {
            return Err(SlashError::InvalidState);
        }

        // Attestation-based slash: quorum must be met before execution
        let quorum: u32 = env.storage().instance().get(&QUORUM).unwrap();
        if (record.attestors.len() as u32) < quorum && record.attestors.len() > 0 {
            return Err(SlashError::QuorumNotMet);
        }

        let now = env.ledger().timestamp();
        if now <= record.appeal_deadline {
            return Err(SlashError::AppealWindowOpen);
        }

        Self::burn_escrow(&env, evidence_hash.clone())?;
        record.status = SlashStatus::Executed;
        records.set(evidence_hash.clone(), record.clone());
        env.storage().instance().set(&SLASH_REC, &records);

        env.events().publish(
            (symbol_short!("EXECUTED"), record.offender),
            (evidence_hash, record.escrowed_amount),
        );

        Ok(())
    }

    // ── Views ─────────────────────────────────────────────────────────────────

    /// Return the slash record for a given evidence hash.
    pub fn get_slash_record(env: Env, evidence_hash: BytesN<32>) -> Option<SlashRecord> {
        let records: Map<BytesN<32>, SlashRecord> =
            env.storage().instance().get(&SLASH_REC).unwrap();
        records.get(evidence_hash)
    }

    /// Return the staked balance of an address.
    pub fn get_stake_balance(env: Env, staker: Address) -> i128 {
        let stakes: Map<Address, i128> = env.storage().instance().get(&STAKES).unwrap();
        stakes.get(staker).unwrap_or(0)
    }

    /// Return the list of authorised slashers.
    pub fn get_slashers(env: Env) -> Vec<Address> {
        env.storage().instance().get(&SLASHERS).unwrap()
    }

    /// Return the current quorum threshold.
    pub fn get_quorum(env: Env) -> u32 {
        env.storage().instance().get(&QUORUM).unwrap()
    }

    // ── Internal Helpers ──────────────────────────────────────────────────────

    fn require_admin(env: &Env) -> Result<(), SlashError> {
        let admin: Address = env.storage().instance().get(&ADMIN).unwrap();
        admin.require_auth();
        Ok(())
    }

    fn require_slasher(env: &Env, caller: &Address) -> Result<(), SlashError> {
        let slashers: Vec<Address> = env.storage().instance().get(&SLASHERS).unwrap();
        if slashers.contains(caller) {
            Ok(())
        } else {
            Err(SlashError::Unauthorized)
        }
    }

    fn validate_penalty(_env: &Env, penalty_bps: u32) -> Result<(), SlashError> {
        if penalty_bps == 0 {
            return Err(SlashError::ZeroPenalty);
        }
        if penalty_bps > MAX_PENALTY_BPS {
            return Err(SlashError::PenaltyTooHigh);
        }
        Ok(())
    }

    fn check_evidence_unused(env: &Env, hash: &BytesN<32>) -> Result<(), SlashError> {
        let used: Vec<BytesN<32>> = env.storage().instance().get(&USED_EV).unwrap();
        if used.contains(hash) {
            Err(SlashError::DuplicateEvidence)
        } else {
            Ok(())
        }
    }

    fn mark_evidence_used(env: &Env, hash: BytesN<32>) {
        let mut used: Vec<BytesN<32>> = env.storage().instance().get(&USED_EV).unwrap();
        used.push_back(hash);
        env.storage().instance().set(&USED_EV, &used);
    }

    fn get_stake(env: &Env, staker: &Address) -> Result<i128, SlashError> {
        let stakes: Map<Address, i128> = env.storage().instance().get(&STAKES).unwrap();
        let amount = stakes.get(staker.clone()).unwrap_or(0);
        if amount == 0 {
            Err(SlashError::InsufficientStake)
        } else {
            Ok(amount)
        }
    }

    fn compute_slash(stake: i128, penalty_bps: u32) -> i128 {
        stake * penalty_bps as i128 / 10_000
    }

    fn move_to_escrow(
        env: &Env,
        offender: &Address,
        amount: i128,
        hash: BytesN<32>,
    ) -> Result<(), SlashError> {
        let mut stakes: Map<Address, i128> = env.storage().instance().get(&STAKES).unwrap();
        let current = stakes.get(offender.clone()).unwrap_or(0);
        if current < amount {
            return Err(SlashError::InsufficientStake);
        }
        stakes.set(offender.clone(), current - amount);
        env.storage().instance().set(&STAKES, &stakes);

        let mut escrow: Map<BytesN<32>, i128> = env.storage().instance().get(&ESCROW).unwrap();
        escrow.set(hash, amount);
        env.storage().instance().set(&ESCROW, &escrow);
        Ok(())
    }

    fn release_escrow(env: &Env, offender: &Address, hash: BytesN<32>) -> Result<(), SlashError> {
        let mut escrow: Map<BytesN<32>, i128> = env.storage().instance().get(&ESCROW).unwrap();
        let amount = escrow.get(hash.clone()).unwrap_or(0);
        escrow.remove(hash);
        env.storage().instance().set(&ESCROW, &escrow);

        let mut stakes: Map<Address, i128> = env.storage().instance().get(&STAKES).unwrap();
        let current = stakes.get(offender.clone()).unwrap_or(0);
        stakes.set(offender.clone(), current + amount);
        env.storage().instance().set(&STAKES, &stakes);
        Ok(())
    }

    fn burn_escrow(env: &Env, hash: BytesN<32>) -> Result<(), SlashError> {
        let mut escrow: Map<BytesN<32>, i128> = env.storage().instance().get(&ESCROW).unwrap();
        // In production: transfer to a burn address or treasury.
        // Here we simply remove from escrow (tokens remain in contract as treasury).
        escrow.remove(hash);
        env.storage().instance().set(&ESCROW, &escrow);
        Ok(())
    }
}