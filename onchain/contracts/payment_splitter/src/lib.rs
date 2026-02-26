#![no_std]

//! Payment Splitting Contract (#206)
//!
//! Splits a single payment across multiple recipients with configurable
//! percentage-based or fixed-amount splits. Validates that split totals
//! sum correctly before execution.

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Vec};

#[contract]
pub struct PaymentSplitterContract;

#[contracttype]
#[derive(Clone)]
enum StorageKey {
    Initialized,
    Admin,
    NextSplitId,
    /// Split definition: split_id -> SplitDefinition
    Split(u128),
}

/// Kind of share: percentage (basis points, 10000 = 100%) or fixed amount
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShareKind {
    /// Share in basis points (10000 = 100%)
    Percent(u32),
    /// Fixed amount in smallest token units
    Fixed(i128),
}

/// Single recipient share
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipientShare {
    pub recipient: Address,
    pub kind: ShareKind,
}

/// A split definition (multiple recipients)
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplitDefinition {
    pub id: u128,
    pub creator: Address,
    pub recipients: Vec<RecipientShare>,
}

#[contractimpl]
impl PaymentSplitterContract {
    /// Initializes the contract. Callable once.
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        let init: bool = env
            .storage()
            .persistent()
            .get(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(!init, "Already initialized");
        env.storage().persistent().set(&StorageKey::Initialized, &true);
        env.storage().persistent().set(&StorageKey::Admin, &admin);
        env.storage().persistent().set(&StorageKey::NextSplitId, &1u128);
    }

    /// Creates a split definition. Percent shares must sum to 10000 (100%). Fixed shares are validated at execute time.
    ///
    /// # Arguments
    /// * `creator` - Caller (must authenticate)
    /// * `recipients` - List of (recipient, ShareKind)
    /// # Returns
    /// Split ID
    pub fn create_split(env: Env, creator: Address, recipients: Vec<RecipientShare>) -> u128 {
        creator.require_auth();
        Self::require_initialized(&env);
        assert!(!recipients.is_empty(), "At least one recipient");
        let mut has_percent = false;
        let mut total_bps = 0u32;
        for i in 0..recipients.len() {
            let r = recipients.get(i).unwrap();
            match &r.kind {
                ShareKind::Percent(bps) => {
                    has_percent = true;
                    total_bps = total_bps
                        .checked_add(*bps)
                        .expect("Percent overflow");
                }
                ShareKind::Fixed(_) => {}
            }
        }
        if has_percent {
            assert!(total_bps == 10000, "Percent shares must sum to 10000 (100%)");
        }

        let next_id: u128 = env
            .storage()
            .persistent()
            .get(&StorageKey::NextSplitId)
            .unwrap_or(1);
        env.storage()
            .persistent()
            .set(&StorageKey::NextSplitId, &(next_id + 1));

        let def = SplitDefinition {
            id: next_id,
            creator: creator.clone(),
            recipients,
        };
        env.storage()
            .persistent()
            .set(&StorageKey::Split(next_id), &def);
        next_id
    }

    /// Validates a split definition against a total amount: for Percent shares no amount needed;
    /// for Fixed shares, sum of fixed amounts must equal total.
    pub fn validate_split_for_amount(env: Env, split_id: u128, total_amount: i128) -> bool {
        let def: SplitDefinition = env
            .storage()
            .persistent()
            .get(&StorageKey::Split(split_id))
            .expect("Split not found");
        let mut fixed_sum: i128 = 0;
        for i in 0..def.recipients.len() {
            let r = def.recipients.get(i).unwrap();
            match &r.kind {
                ShareKind::Percent(_) => {}
                ShareKind::Fixed(amt) => {
                    fixed_sum = fixed_sum.checked_add(*amt).expect("Fixed sum overflow");
                }
            }
        }
        let mut has_fixed = false;
        for i in 0..def.recipients.len() {
            if matches!(&def.recipients.get(i).unwrap().kind, ShareKind::Fixed(_)) {
                has_fixed = true;
                break;
            }
        }
        if has_fixed {
            fixed_sum == total_amount
        } else {
            true
        }
    }

    /// Computes the amount for each recipient given a total. Callable by anyone (view).
    pub fn compute_split(
        env: Env,
        split_id: u128,
        total_amount: i128,
    ) -> Vec<(Address, i128)> {
        let def: SplitDefinition = env
            .storage()
            .persistent()
            .get(&StorageKey::Split(split_id))
            .expect("Split not found");
        let mut out: Vec<(Address, i128)> = Vec::new(&env);
        for i in 0..def.recipients.len() {
            let r = def.recipients.get(i).unwrap();
            let amt = match &r.kind {
                ShareKind::Percent(bps) => {
                    (i128::from(*bps) * total_amount) / 10000
                }
                ShareKind::Fixed(a) => *a,
            };
            out.push_back((r.recipient.clone(), amt));
        }
        out
    }

    /// Returns the split definition.
    pub fn get_split(env: Env, split_id: u128) -> SplitDefinition {
        env.storage()
            .persistent()
            .get(&StorageKey::Split(split_id))
            .expect("Split not found")
    }

    fn require_initialized(env: &Env) {
        let init: bool = env
            .storage()
            .persistent()
            .get(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(init, "Contract not initialized");
    }
}
