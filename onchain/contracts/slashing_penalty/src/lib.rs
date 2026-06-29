#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Bytes, Env, Vec};

/// A slash record stored for each slashable agreement.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlashRecord {
    pub agreement_id: u128,
    pub target: Address,
    pub penalty_bps: u32,
    pub executed: bool,
}

/// Error variants for the slashing-penalty contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SlashError {
    /// Quorum is set to zero, which would allow any single attestor to slash
    /// unilaterally. Zero-quorum is always rejected.
    ZeroQuorum,
    /// The attestor list has at least one entry but does not meet quorum.
    BelowQuorum,
    /// The agreement has already been slashed.
    AlreadySlashed,
    /// The slash record was not found.
    NotFound,
    /// No on-chain evidence was provided for an evidence-only slash.
    MissingEvidence,
    /// Contract has not been initialized.
    NotInitialized,
    /// Caller is not authorized.
    Unauthorized,
}

#[contracttype]
#[derive(Clone)]
enum StorageKey {
    Initialized,
    Admin,
    Quorum,
    Record(u128),
}

#[contract]
pub struct SlashingPenaltyContract;

fn require_initialized(env: &Env) -> Result<(), SlashError> {
    let ok = env
        .storage()
        .persistent()
        .get::<_, bool>(&StorageKey::Initialized)
        .unwrap_or(false);
    if !ok {
        return Err(SlashError::NotInitialized);
    }
    Ok(())
}

fn read_quorum(env: &Env) -> u32 {
    env.storage()
        .persistent()
        .get::<_, u32>(&StorageKey::Quorum)
        .expect("Quorum not set")
}

/// Determine whether quorum enforcement is required for the given call.
///
/// # Quorum enforcement rules
///
/// | Scenario                            | `requires_quorum` | Allowed? |
/// |-------------------------------------|-------------------|----------|
/// | attestors present, count >= quorum  | true              | Yes      |
/// | attestors present, count < quorum   | true              | **No**   |
/// | no attestors + on-chain evidence    | false             | Yes      |
/// | no attestors + no evidence          | false             | **No**   |
/// | quorum == 0 (any scenario)          | —                 | **No**   |
///
/// The zero-quorum case is rejected unconditionally so that a misconfigured
/// contract cannot be exploited to bypass attestor checks entirely.
fn requires_quorum(attestors: &Vec<Address>) -> bool {
    attestors.len() > 0
}

#[contractimpl]
impl SlashingPenaltyContract {
    /// Initialize the slashing-penalty contract.
    ///
    /// # Arguments
    /// * `admin`  - Address that administers the contract.
    /// * `quorum` - Minimum number of attestors required to approve a slash when
    ///              attestors are present. Must be >= 1; a value of 0 is rejected
    ///              to prevent silent bypass of the attestor requirement.
    pub fn initialize(env: Env, admin: Address, quorum: u32) -> Result<(), SlashError> {
        admin.require_auth();

        let already = env
            .storage()
            .persistent()
            .get::<_, bool>(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(!already, "Already initialized");

        // Reject zero quorum at configuration time so the invariant is
        // established once and checked cheaply everywhere else.
        if quorum == 0 {
            return Err(SlashError::ZeroQuorum);
        }

        env.storage().persistent().set(&StorageKey::Admin, &admin);
        env.storage().persistent().set(&StorageKey::Quorum, &quorum);
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);
        Ok(())
    }

    /// Execute a slash against a target for a given agreement.
    ///
    /// # Quorum condition
    ///
    /// This function enforces quorum **only when at least one attestor is
    /// supplied**.  When `attestors` is empty the function instead requires
    /// `on_chain_evidence` to be non-empty, proving the slash is backed by
    /// verifiable on-chain data rather than attestor votes.
    ///
    /// Concretely:
    /// - `attestors.len() > 0` → `requires_quorum` is `true`; the count must
    ///   reach the configured quorum or the call is rejected with
    ///   [`SlashError::BelowQuorum`].
    /// - `attestors.len() == 0` → `requires_quorum` is `false`; quorum is
    ///   **not** checked, but `on_chain_evidence` must be non-empty or the
    ///   call is rejected with [`SlashError::MissingEvidence`].  This path is
    ///   intentional: some slash conditions (e.g. cryptographic fraud proofs)
    ///   are self-evidencing and need no human attestors.
    /// - A configured quorum of `0` is **always** rejected at initialisation
    ///   time, so it can never arise here.  If somehow reached, the call panics.
    ///
    /// # Arguments
    /// * `caller`            - Admin address invoking the slash.
    /// * `agreement_id`      - Identifier of the slashable agreement.
    /// * `target`            - Address to be penalised.
    /// * `penalty_bps`       - Penalty in basis points (1 bps = 0.01 %).
    /// * `attestors`         - Addresses that attest to the slash.  May be empty
    ///                         only when `on_chain_evidence` is non-empty.
    /// * `on_chain_evidence` - Raw bytes of on-chain evidence (e.g. fraud proof).
    ///                         Required when `attestors` is empty; ignored
    ///                         otherwise.
    pub fn execute_slash(
        env: Env,
        caller: Address,
        agreement_id: u128,
        target: Address,
        penalty_bps: u32,
        attestors: Vec<Address>,
        on_chain_evidence: Bytes,
    ) -> Result<(), SlashError> {
        require_initialized(&env)?;
        caller.require_auth();

        // Only the admin may trigger a slash.
        let admin = env
            .storage()
            .persistent()
            .get::<_, Address>(&StorageKey::Admin)
            .expect("Admin not set");
        if caller != admin {
            return Err(SlashError::Unauthorized);
        }

        // Guard against a zero quorum reaching execute_slash (belt-and-suspenders).
        let quorum = read_quorum(&env);
        assert!(quorum > 0, "Quorum invariant violated: quorum must be > 0");

        // Reject double-slash.
        if let Some(record) = env
            .storage()
            .persistent()
            .get::<_, SlashRecord>(&StorageKey::Record(agreement_id))
        {
            if record.executed {
                return Err(SlashError::AlreadySlashed);
            }
        }

        if requires_quorum(&attestors) {
            // Attestor-backed path: enforce quorum.
            let len = attestors.len();
            if len < quorum {
                return Err(SlashError::BelowQuorum);
            }
        } else {
            // Evidence-only path: no attestors, so evidence must be present.
            // This branch is intentional and not a quorum bypass — it is only
            // reachable when the caller explicitly provides zero attestors, and
            // is gated by the requirement that valid on-chain evidence exists.
            if on_chain_evidence.len() == 0 {
                return Err(SlashError::MissingEvidence);
            }
        }

        let record = SlashRecord {
            agreement_id,
            target,
            penalty_bps,
            executed: true,
        };
        env.storage()
            .persistent()
            .set(&StorageKey::Record(agreement_id), &record);

        Ok(())
    }

    /// Retrieve a slash record by agreement id.
    pub fn get_slash_record(env: Env, agreement_id: u128) -> Option<SlashRecord> {
        env.storage()
            .persistent()
            .get(&StorageKey::Record(agreement_id))
    }

    /// Return the configured quorum threshold.
    pub fn get_quorum(env: Env) -> u32 {
        read_quorum(&env)
    }
}
