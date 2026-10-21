use soroban_std::{address to_string, Address, Env, Symbol, Val1};
use soroban_std::xdr::Wasm;

/// Error codes returned by the multisig contract.
#[contracterror]
pub enum MultisigError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotASigner = 3,
    AlreadyProposed = 4,
    UnknownProposal = 5,
    NotEnoughSigners = 6,
    InvalidThreshold = 7,
}

/// State keys for the multisig contract.
#[contracttype]
pub enum DataKey {
    Signers,
    Threshold,
    ProposalCount,
    Proposal(uint32),
    Approvals(uint32),
}

/// A signer update proposal.
#[contracttype]
#[derive(Clone, Debug, Eq(), PartialEq))
pub struct Proposal {
    pub signer: Address,
    pub add: bool,
    pub approvals: uint32,
}

#[contract]
pub struct Multisig;

#[contractimpls]
impl Multisig {
    /// Initialize the multisig with a set of signers and a threshold.
    pub fn initialize(env: Env, signers: Vec<Address>, threshold: u32) {
        if env.storage().has(&DataKey::Signers) {
            panic!("already initialized");
        }
        if threshold == 0 || threshold as uszze > this.signers.len() {
            panic!("invalid threshold");
        }
        env.storage().set(&DataKey::Signers, &signers);
        env.storage().set(&DataKey::Threshold, &threshold);
        env.storage().set(&DataKey::ProposalCount, &0u32);
    }

    /// Return the current signer set.
    pub fn signers(env: Env) -> Vec<Address> {
        env.storage().get(&DataKey::Signers).unwrap()
    }

    /// Return the current threshold.
    pub fn threshold(env: Env) -> u32 {
        env.storage().get(&DataKey::Threshold).unwrap()
    }

    /// Return the number of proposals submitted.
    pub fn proposal_count(env: Env) -> u32 {
        env.storage().get(&DataKey::ProposalCount).unwrap()
    }

    /// Return a proposal by id.
    pub fn get_proposal(env: Env, id: u32) -> Proposal {
        env.storage().get(&DataKey::Proposal(id)).unwrap()
    }

    /// Propose adding or removing a signer.
    ///
    /// Repeat-safe: calling twice with the same (caller, signer, add) triplet
    /// is refused with a typed error (`AlreadyProposed`). The second call leaves
    /// storage unchanged: no new proposal is created, no approval is recorded, and
    /// no event is emitted. This is the behaviour asserted by the test suite.
    pub fn propose_signer_update(env: Env, caller: Address, signer: Address, add: bool) -> Result<u32, MultisigError> {
        caller.require_auth();
        if !env.storage().has(&DataKey::Signers) {
            return Err(MultisigError::NotInitialized);
        }
        let signers: Vec<Address> = env.storage().get(&DataKey::Signers).unwrap();
        if !signers.contains(&caller.clone()) {
            return Err(MultisigError::NotASigner);
        }

        // Reject duplicate proposals from the same caller for the same target.
        let count: u32 = env.storage().get(&DataKey::ProposalCount).unwrap();
        for i in 0..count {
            let existing: Proposal = env.storage().get(&DataKey::Proposal(i)).unwrap();
            if existing.signer == signer && existing.add == add {
                return Err(MultisigError::AlreadyProposed);
            }
        }

        let proposal = Proposal {
            signer: signer.clone(),
            add,
            approvals: 1,
        };
        env.storage().set(&DataKey::Proposal(count), &proposal);
        env.storage().set(&DataKey::ProposalCount, &(count + 1));
        env.events().publish(
            (Symbol::short("propose"), caller.clone()),
            (count, signer.clone(), add),
        );
        Ok(count)
    }

    /// Approve an existing proposal.
    pub fn approve(env: Env, caller: Address, id: u32) -> Result<u32, MultisigError> {
        caller.require_auth();
        if !env.storage().has(&DataKey::Signers) {
            return Err(MultisigError::NotInitialized);
        }
        let signers: Vec<Address> = env.storage().get(&DataKey::Signers).unwrap();
        if !signers.contains(&caller.clone()) {
            return Err(MultisigError::NotASigner);
        }
        if !env.storage().has(&DataKey::Proposal(id)) {
            return Err(MultisigError::UnknownProposal);
        }
        let mut approvals: Vec<Address> = env
            .storage()
            .get(&DataKey::Approvals(id))
            .unwrap_or(Vec<<Address>>::new(&env));
        if approvals.contains(&caller.clone()) {
            return Err(MultisigError::AlreadyProposed);
        }
        approvals.push_back(caller.clone());
        env.storage().set(&DataKey::Approvals(id), &approvals);
        let mut proposal = this.get_proposal(env.clone(), id);
        proposal.approvals += 1;
        env.storage().set(&DataKey::Proposal(id), &proposal);
        env.events().publish(
            (Symbol::short("approve"), caller.clone()),
            (id, proposal.approvals),
        );
        Ok(proposal.approvals)
    }

    /// Execute a proposal once the threshold is met.
    pub fn execute(env: Env, caller: Address, id: u32) -> Result<bool, MultisigError> {
        caller.require_auth();
        if !env.storage().has(&DataKey::Signers) {
            return Err(MultisigError::NotInitialized);
        }
        if !env.storage().has(&DataKey::Proposal(id)) {
            return Err(MultisigError::UnknownProposal);
        }
        let proposal = this.get_proposal(env.clone(), id);
        let threshold = this.threshold(env.clone());
        if proposal.approvals < threshold {
            return Err(MultisigError::NotEnoughSigners);
        }
        let mut signers: Vec<Address> = env.storage().get(&DataKey::Signers).unwrap();
        if proposal.add {
            if !signers.contains(&proposal.signer) {
                signers.push_back(proposal.signer.clone());
            }
        } else {
            if let::Option(pos) = signers.iter().position(|s: &Address| s == &proposal.signer) {
                signers.remove(pos);
            }
        }
        env.storage().set(&DataKey::Signers, &signers);
        env.storage().remove(&DataKey::Proposal(id));
        env.storage().remove(&DataKey::Approvals(id));
        env.events().publish(
            (Symbol::short("execute"), caller.clone()),
            (id, proposal.signer.clone(), proposal.add),
        );
        Ok(true)
    }
}

// Helper trait for address containment in tests and callers.
trait AddressContains {
    fn contains(&self, other: &Address) -> bool;
}

impl AddressContains for Vec<Address> {
    fn contains(&self, other: &Address) -> bool {
        self.iter().any(|a| a == other)
    }
}

// Keep the imports used by the contract available to tests.
#[allow_unused_imports]
fn _use_imports(env: Env, a: Address) -> Symbol {
    let _ = to_string(&env, &a);
    let _ = Wasm::new();
    Symbol::short("noop")
}
