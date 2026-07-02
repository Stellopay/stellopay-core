#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Vec};

/// Maximum number of badge IDs returned in a single paginated query.
/// Clamps any caller-supplied `limit` to prevent memory/instruction exhaustion.
pub const MAX_PAGE_SIZE: u32 = 50;

#[contract]
pub struct NftPayrollBadgeContract;

/// Represents a single payroll badge held by an address.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Badge {
    /// Unique badge identifier.
    pub id: u64,
    /// Address that received this badge.
    pub owner: Address,
    /// Human-readable badge name (e.g., "Q1 2025 Payroll").
    pub name: soroban_sdk::String,
    /// Ledger timestamp at which the badge was minted.
    pub issued_at: u64,
}

/// Result returned by [`NftPayrollBadgeContract::badges_of_paged`].
///
/// ## Cursor semantics
/// - `next_cursor` is `Some(n)` when more badges exist after the current page.
///   Pass that value as `start` in the next call to retrieve the subsequent page.
/// - `next_cursor` is `None` when the returned page is the final (or only) page.
/// - Badges are returned in ascending badge-ID order, which is stable and
///   deterministic across calls as long as no badges are removed.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PagedBadges {
    /// Badge IDs in the current page (length <= MAX_PAGE_SIZE).
    pub items: Vec<u64>,
    /// Cursor to pass as `start` for the next page, or `None` if this is the last page.
    pub next_cursor: Option<u32>,
}

#[contracttype]
#[derive(Clone)]
enum StorageKey {
    Initialized,
    Owner,
    NextBadgeId,
    Badge(u64),
    /// Maps an owner address to the count of badges they hold.
    OwnerBadgeCount(Address),
    /// Maps (owner, index) to a badge id — the owner's badge list stored as individual entries.
    OwnerBadgeAt(Address, u32),
}

fn require_initialized(env: &Env) {
    let initialized = env
        .storage()
        .persistent()
        .get::<_, bool>(&StorageKey::Initialized)
        .unwrap_or(false);
    assert!(initialized, "Contract not initialized");
}

fn next_badge_id(env: &Env) -> u64 {
    let current = env
        .storage()
        .persistent()
        .get::<_, u64>(&StorageKey::NextBadgeId)
        .unwrap_or(0);
    let next = current.checked_add(1).expect("Badge id overflow");
    env.storage()
        .persistent()
        .set(&StorageKey::NextBadgeId, &next);
    next
}

fn owner_badge_count(env: &Env, owner: &Address) -> u32 {
    env.storage()
        .persistent()
        .get::<_, u32>(&StorageKey::OwnerBadgeCount(owner.clone()))
        .unwrap_or(0)
}

fn append_badge_to_owner(env: &Env, owner: &Address, badge_id: u64) {
    let count = owner_badge_count(env, owner);
    env.storage()
        .persistent()
        .set(&StorageKey::OwnerBadgeAt(owner.clone(), count), &badge_id);
    env.storage()
        .persistent()
        .set(&StorageKey::OwnerBadgeCount(owner.clone()), &(count + 1));
}

#[contractimpl]
impl NftPayrollBadgeContract {
    /// Initializes the contract and designates an admin owner.
    pub fn initialize(env: Env, owner: Address) {
        owner.require_auth();
        let initialized = env
            .storage()
            .persistent()
            .get::<_, bool>(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(!initialized, "Contract already initialized");

        env.storage().persistent().set(&StorageKey::Owner, &owner);
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);
    }

    /// Mints a new payroll badge and assigns it to `recipient`.
    ///
    /// Only the contract owner may mint badges.
    ///
    /// # Returns
    /// The newly assigned badge ID.
    pub fn mint(env: Env, caller: Address, recipient: Address, name: soroban_sdk::String) -> u64 {
        require_initialized(&env);
        caller.require_auth();
        let owner: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::Owner)
            .expect("Owner not set");
        assert!(caller == owner, "Only owner can mint badges");

        let badge_id = next_badge_id(&env);
        let badge = Badge {
            id: badge_id,
            owner: recipient.clone(),
            name,
            issued_at: env.ledger().timestamp(),
        };

        env.storage()
            .persistent()
            .set(&StorageKey::Badge(badge_id), &badge);
        append_badge_to_owner(&env, &recipient, badge_id);

        badge_id
    }

    /// Returns ALL badge IDs held by `owner` in a single call.
    ///
    /// **Deprecated** — for high-volume holders this may exhaust memory or
    /// instruction budgets.  Prefer [`badges_of_paged`] for bounded reads.
    ///
    /// [`badges_of_paged`]: NftPayrollBadgeContract::badges_of_paged
    pub fn badges_of(env: Env, owner: Address) -> Vec<u64> {
        require_initialized(&env);
        let count = owner_badge_count(&env, &owner);
        let mut ids: Vec<u64> = Vec::new(&env);
        for i in 0..count {
            if let Some(badge_id) = env
                .storage()
                .persistent()
                .get::<_, u64>(&StorageKey::OwnerBadgeAt(owner.clone(), i))
            {
                ids.push_back(badge_id);
            }
        }
        ids
    }

    /// Returns a bounded, paginated slice of badge IDs held by `owner`.
    ///
    /// ## Arguments
    /// - `owner`  – Address whose badges to query.
    /// - `start`  – Zero-based index into the owner's badge list (cursor from a
    ///              previous call, or `0` for the first page).
    /// - `limit`  – Maximum number of items to return.  Values above
    ///              [`MAX_PAGE_SIZE`] are silently clamped to `MAX_PAGE_SIZE`.
    ///
    /// ## Returns
    /// A [`PagedBadges`] struct containing:
    /// - `items` — badge IDs for positions `[start, start + effective_limit)`.
    /// - `next_cursor` — `Some(start + effective_limit)` when more items follow;
    ///   `None` when the page covers the end of the list.
    ///
    /// ## Ordering
    /// Badges are returned in the order they were minted (ascending badge-ID),
    /// which is stable and deterministic; cursors never skip or duplicate entries.
    pub fn badges_of_paged(env: Env, owner: Address, start: u32, limit: u32) -> PagedBadges {
        require_initialized(&env);
        let effective_limit = if limit == 0 || limit > MAX_PAGE_SIZE {
            MAX_PAGE_SIZE
        } else {
            limit
        };

        let count = owner_badge_count(&env, &owner);
        let mut items: Vec<u64> = Vec::new(&env);

        let end = start.saturating_add(effective_limit).min(count);
        for i in start..end {
            if let Some(badge_id) = env
                .storage()
                .persistent()
                .get::<_, u64>(&StorageKey::OwnerBadgeAt(owner.clone(), i))
            {
                items.push_back(badge_id);
            }
        }

        let next_cursor = if end < count { Some(end) } else { None };

        PagedBadges { items, next_cursor }
    }

    /// Returns the badge metadata for a given `badge_id`, or `None` if not found.
    pub fn get_badge(env: Env, badge_id: u64) -> Option<Badge> {
        env.storage()
            .persistent()
            .get(&StorageKey::Badge(badge_id))
    }

    /// Returns the total number of badges held by `owner`.
    pub fn badge_count(env: Env, owner: Address) -> u32 {
        require_initialized(&env);
        owner_badge_count(&env, &owner)
    }

    /// Returns the contract owner address.
    pub fn get_owner(env: Env) -> Option<Address> {
        env.storage().persistent().get(&StorageKey::Owner)
    }
}
