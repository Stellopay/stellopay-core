#![no_std]

//! Payment Splitting Contract
//!
//! Splits a single payment across multiple recipients with configurable
//! percentage-based (basis points) or fixed-amount splits.
//!
//! Hardened with:
//! - Deterministic rounding discipline (largest-remainder dust distribution)
//! - Arithmetic safety (checked operations)
//! - Validation helpers (duplicate recipient checks, zero-weight prevention)

use soroban_sdk::{contract, contractimpl, contracttype, xdr::ToXdr, Address, Bytes, Env, Vec};

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
    /// Optimization: Store whether it's a percentage-based split
    pub is_percent: bool,
}

#[contractimpl]
impl PaymentSplitterContract {
    /// Initializes the contract. Callable once.
    ///
    /// # Arguments
    /// * `admin` - The administrative address for the contract.
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        let init: bool = env
            .storage()
            .persistent()
            .get(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(!init, "Already initialized");
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);
        env.storage().persistent().set(&StorageKey::Admin, &admin);
        env.storage()
            .persistent()
            .set(&StorageKey::NextSplitId, &1u128);
    }

    /// Creates a split definition. Splits must be either all Percentage or all Fixed.
    ///
    /// # Arguments
    /// * `creator` - Caller (must authenticate).
    /// * `recipients` - List of (recipient, ShareKind).
    ///
    /// # Returns
    /// Unique split ID.
    pub fn create_split(env: Env, creator: Address, recipients: Vec<RecipientShare>) -> u128 {
        creator.require_auth();
        Self::require_initialized(&env);
        assert!(!recipients.is_empty(), "At least one recipient required");

        let mut has_percent = false;
        let mut has_fixed = false;
        let mut total_bps = 0u32;
        let mut seen_addresses = Vec::<Address>::new(&env);

        for i in 0..recipients.len() {
            let r = recipients.get_unchecked(i);
            
            // 1. Uniqueness check
            for seen in 0..seen_addresses.len() {
                assert!(seen_addresses.get_unchecked(seen) != r.recipient, "Duplicate recipient address");
            }
            seen_addresses.push_back(r.recipient.clone());

            match r.kind {
                ShareKind::Percent(bps) => {
                    has_percent = true;
                    assert!(bps > 0, "Percentage-based share must be > 0");
                    total_bps = total_bps.checked_add(bps).expect("Percentage overflow");
                }
                ShareKind::Fixed(amt) => {
                    has_fixed = true;
                    assert!(amt > 0, "Fixed share amount must be > 0");
                }
            }
        }

        // 2. Mutual exclusivity check (Percent vs Fixed)
        assert!(has_percent != has_fixed, "Split must be either all Percentage or all Fixed");

        // 3. Percentage sum validation
        if has_percent {
            assert!(total_bps == 10000, "Percentage shares must sum to 10000 (100%)");
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
            is_percent: has_percent,
        };
        env.storage()
            .persistent()
            .set(&StorageKey::Split(next_id), &def);
        next_id
    }

    /// Validates a split configuration against a total amount.
    ///
    /// For Percent splits: always true if sum is 100% (already checked at creation).
    /// For Fixed splits: sum of fixed amounts must equal total_amount.
    pub fn validate_split_for_amount(env: Env, split_id: u128, total_amount: i128) -> bool {
        assert!(total_amount > 0, "Total amount must be > 0");

        let def: SplitDefinition = env
            .storage()
            .persistent()
            .get(&StorageKey::Split(split_id))
            .expect("Split not found");

        if def.is_percent {
            true
        } else {
            let mut fixed_sum: i128 = 0;
            for i in 0..def.recipients.len() {
                let r = def.recipients.get_unchecked(i);
                if let ShareKind::Fixed(amt) = r.kind {
                    fixed_sum = fixed_sum.checked_add(amt).expect("Fixed sum overflow");
                }
            }
            fixed_sum == total_amount
        }
    }

    /// Computes the amount for each recipient given a total amount.
    ///
    /// Percent splits use largest-remainder apportionment:
    /// 1. Floor each exact percentage slice.
    /// 2. Distribute the remaining dust one unit at a time to the recipients
    ///    with the largest fractional remainder.
    /// 3. Break exact remainder ties using canonical recipient address order.
    ///
    /// # Returns
    /// `Vec<(Address, i128)>` where each tuple is a recipient and their share.
    pub fn compute_split(env: Env, split_id: u128, total_amount: i128) -> Vec<(Address, i128)> {
        assert!(total_amount > 0, "Total amount must be > 0");

        let def: SplitDefinition = env
            .storage()
            .persistent()
            .get(&StorageKey::Split(split_id))
            .expect("Split not found");

        if !def.is_percent {
            assert!(
                Self::validate_split_for_amount(env.clone(), split_id, total_amount),
                "Fixed split total must equal sum of fixed amounts"
            );
        }

        let mut out: Vec<(Address, i128)> = Vec::new(&env);
        let recipient_count = def.recipients.len();

        if def.is_percent {
            let mut floored_amounts = Vec::<i128>::new(&env);
            let mut remainders = Vec::<i128>::new(&env);
            let mut awarded_dust = Vec::<u32>::new(&env);
            let mut total_allocated: i128 = 0;

            for i in 0..recipient_count {
                let r = def.recipients.get_unchecked(i);
                let bps = match r.kind {
                    ShareKind::Percent(bps) => bps,
                    ShareKind::Fixed(_) => unreachable!(),
                };

                let exact_numerator = i128::from(bps)
                    .checked_mul(total_amount)
                    .expect("Mul overflow");
                let floored = exact_numerator.checked_div(10000).expect("Div error");
                let remainder = exact_numerator.checked_rem(10000).expect("Rem error");

                floored_amounts.push_back(floored);
                remainders.push_back(remainder);
                total_allocated = total_allocated
                    .checked_add(floored)
                    .expect("Total allocated overflow");
            }

            let dust = total_amount
                .checked_sub(total_allocated)
                .expect("Dust underflow");
            assert!(dust >= 0, "Negative dust detected");

            for _ in 0..dust {
                let best = Self::select_next_dust_recipient(&env, &def.recipients, &remainders, &awarded_dust);
                awarded_dust.push_back(best);
            }

            for i in 0..recipient_count {
                let r = def.recipients.get_unchecked(i);
                let mut amount = floored_amounts.get_unchecked(i);
                if Self::contains_index(&awarded_dust, i) {
                    amount = amount.checked_add(1).expect("Dust allocation overflow");
                }
                out.push_back((r.recipient.clone(), amount));
            }
        } else {
            for i in 0..recipient_count {
                let r = def.recipients.get_unchecked(i);
                let amount = match r.kind {
                    ShareKind::Fixed(amount) => amount,
                    ShareKind::Percent(_) => unreachable!(),
                };
                out.push_back((r.recipient.clone(), amount));
            }
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

    fn select_next_dust_recipient(
        env: &Env,
        recipients: &Vec<RecipientShare>,
        remainders: &Vec<i128>,
        awarded_dust: &Vec<u32>,
    ) -> u32 {
        let mut best_index = 0u32;
        let mut best_remainder = -1i128;

        for i in 0..recipients.len() {
            if Self::contains_index(awarded_dust, i) {
                continue;
            }

            let remainder = remainders.get_unchecked(i);
            if remainder > best_remainder {
                best_index = i;
                best_remainder = remainder;
                continue;
            }

            if remainder == best_remainder
                && Self::compare_addresses(env, &recipients.get_unchecked(i).recipient, &recipients.get_unchecked(best_index).recipient) < 0
            {
                best_index = i;
            }
        }

        best_index
    }

    fn contains_index(indexes: &Vec<u32>, needle: u32) -> bool {
        for i in 0..indexes.len() {
            if indexes.get_unchecked(i) == needle {
                return true;
            }
        }
        false
    }

    fn compare_addresses(env: &Env, left: &Address, right: &Address) -> i32 {
        let left_xdr: Bytes = left.clone().to_xdr(env);
        let right_xdr: Bytes = right.clone().to_xdr(env);
        let min_len = if left_xdr.len() < right_xdr.len() {
            left_xdr.len()
        } else {
            right_xdr.len()
        };

        for i in 0..min_len {
            let left_byte = left_xdr.get_unchecked(i);
            let right_byte = right_xdr.get_unchecked(i);
            if left_byte < right_byte {
                return -1;
            }
            if left_byte > right_byte {
                return 1;
            }
        }

        if left_xdr.len() < right_xdr.len() {
            -1
        } else if left_xdr.len() > right_xdr.len() {
            1
        } else {
            0
        }
    }
}
