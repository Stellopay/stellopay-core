#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Bytes, Env, Vec};

/// Errors for the NFT payroll badge contract.
#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BadgeError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    NotOwnerOrAdmin = 4,
    BadgeNotFound = 5,
    TransferNotAllowed = 6,
}

/// Types of payroll badges issued by the contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BadgeKind {
    /// Badge for verified employers.
    Employer,
    /// Badge for verified employees.
    Employee,
    /// Custom badge for integrations or UI labels.
    Custom(u32),
}

/// Represents a single NFT-style payroll badge.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Badge {
    pub id: u128,
    pub owner: Address,
    pub kind: BadgeKind,
    /// Arbitrary metadata URI or opaque bytes (e.g. IPFS CID).
    pub metadata: Bytes,
    /// Whether this badge can be transferred by the holder.
    pub transferable: bool,
    pub created_at: u64,
}

/// Storage keys for the badge contract.
#[contracttype]
#[derive(Clone)]
pub enum StorageKey {
    Initialized,
    Admin,
    NextBadgeId,
    /// Badge data by id.
    Badge(u128),
    /// Owner of a badge id.
    OwnerOf(u128),
    /// All badge ids owned by an address.
    BadgesOf(Address),
}

fn require_initialized(env: &Env) -> Result<(), BadgeError> {
    let initialized: bool = env
        .storage()
        .persistent()
        .get(&StorageKey::Initialized)
        .unwrap_or(false);
    if !initialized {
        return Err(BadgeError::NotInitialized);
    }
    Ok(())
}

fn read_admin(env: &Env) -> Result<Address, BadgeError> {
    env.storage()
        .persistent()
        .get(&StorageKey::Admin)
        .ok_or(BadgeError::NotInitialized)
}

fn require_admin(env: &Env, caller: &Address) -> Result<(), BadgeError> {
    caller.require_auth();
    let admin = read_admin(env)?;
    if *caller != admin {
        return Err(BadgeError::NotAdmin);
    }
    Ok(())
}

fn next_badge_id(env: &Env) -> u128 {
    let current: u128 = env
        .storage()
        .persistent()
        .get(&StorageKey::NextBadgeId)
        .unwrap_or(0);
    let next = current.checked_add(1).expect("badge id overflow");
    env.storage()
        .persistent()
        .set(&StorageKey::NextBadgeId, &next);
    next
}

fn read_badge(env: &Env, id: u128) -> Result<Badge, BadgeError> {
    env.storage()
        .persistent()
        .get(&StorageKey::Badge(id))
        .ok_or(BadgeError::BadgeNotFound)
}

fn write_badge(env: &Env, badge: &Badge) {
    env.storage()
        .persistent()
        .set(&StorageKey::Badge(badge.id), badge);
    env.storage()
        .persistent()
        .set(&StorageKey::OwnerOf(badge.id), &badge.owner);
}

fn remove_badge(env: &Env, id: u128) {
    if let Some(badge) = env
        .storage()
        .persistent()
        .get::<_, Badge>(&StorageKey::Badge(id))
    {
        // Remove from owner's list.
        let mut owned: Vec<u128> = env
            .storage()
            .persistent()
            .get(&StorageKey::BadgesOf(badge.owner.clone()))
            .unwrap_or(Vec::new(env));
        let mut i = 0u32;
        while i < owned.len() {
            if owned.get(i).as_ref().map(|x| *x == id).unwrap_or(false) {
                owned.remove(i);
                break;
            }
            i += 1;
        }
        env.storage()
            .persistent()
            .set(&StorageKey::BadgesOf(badge.owner), &owned);
    }

    env.storage().persistent().remove(&StorageKey::Badge(id));
    env.storage().persistent().remove(&StorageKey::OwnerOf(id));
}

fn push_badge_to_owner(env: &Env, owner: &Address, id: u128) {
    let mut owned: Vec<u128> = env
        .storage()
        .persistent()
        .get(&StorageKey::BadgesOf(owner.clone()))
        .unwrap_or(Vec::new(env));
    owned.push_back(id);
    env.storage()
        .persistent()
        .set(&StorageKey::BadgesOf(owner.clone()), &owned);
}

#[contract]
pub struct NftPayrollBadge;

#[contractimpl]
impl NftPayrollBadge {
    /// @notice Initializes the NFT payroll badge contract.
    /// @dev Can only be called once. Sets the admin who may mint/burn badges.
    /// @param admin Address that will administer badge issuance.
    /// @return Result<(), BadgeError>
    /// @notice Returns an error on failure.
    pub fn initialize(env: Env, admin: Address) -> Result<(), BadgeError> {
        if env
            .storage()
            .persistent()
            .get::<_, bool>(&StorageKey::Initialized)
            .unwrap_or(false)
        {
            return Err(BadgeError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().persistent().set(&StorageKey::Admin, &admin);
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);
        Ok(())
    }

    /// @notice Mints a new payroll badge NFT.
    /// @dev Only the admin may mint. Badges can represent verified employer or
    ///      employee identities, or custom integration-specific badges.
    /// @param caller Admin address; must authenticate.
    /// @param to Recipient address that will own the badge.
    /// @param kind Badge kind (Employer, Employee, or Custom).
    /// @param metadata Arbitrary metadata bytes (e.g. URI, IPFS hash).
    /// @param transferable Whether the badge may be transferred by the holder.
    /// @return badge_id Newly minted badge identifier.
    pub fn mint(
        env: Env,
        caller: Address,
        to: Address,
        kind: BadgeKind,
        metadata: Bytes,
        transferable: bool,
    ) -> Result<u128, BadgeError> {
        require_initialized(&env)?;
        require_admin(&env, &caller)?;

        let id = next_badge_id(&env);
        let badge = Badge {
            id,
            owner: to.clone(),
            kind,
            metadata,
            transferable,
            created_at: env.ledger().timestamp(),
        };

        write_badge(&env, &badge);
        push_badge_to_owner(&env, &to, id);

        // Event: ("badge_minted", id) -> (owner, kind, transferable)
        env.events().publish(
            ("badge_minted", id),
            (to, badge.kind.clone(), badge.transferable),
        );

        Ok(id)
    }

    /// @notice Burns an existing badge, removing it from circulation.
    /// @dev Callable by admin or current badge owner.
    /// @param caller Address requesting the burn; must authenticate.
    /// @param badge_id Identifier of the badge to burn.
    /// @return Result<(), BadgeError>
    /// @notice Returns an error on failure.
    pub fn burn(env: Env, caller: Address, badge_id: u128) -> Result<(), BadgeError> {
        require_initialized(&env)?;
        caller.require_auth();

        let admin = read_admin(&env)?;
        let badge = read_badge(&env, badge_id)?;

        if caller != admin && caller != badge.owner {
            return Err(BadgeError::NotOwnerOrAdmin);
        }

        remove_badge(&env, badge_id);

        env.events()
            .publish(("badge_burned", badge_id), badge.owner);

        Ok(())
    }

    /// @notice Transfers a badge to a new owner if it is marked transferable.
    /// @dev Caller must be current owner; non-transferable badges cannot be moved.
    /// @param caller Current owner; must authenticate.
    /// @param badge_id Identifier of the badge to transfer.
    /// @param to New owner address.
    /// @return Result<(), BadgeError>
    /// @notice Returns an error on failure.
    pub fn transfer(
        env: Env,
        caller: Address,
        badge_id: u128,
        to: Address,
    ) -> Result<(), BadgeError> {
        require_initialized(&env)?;
        caller.require_auth();

        let mut badge = read_badge(&env, badge_id)?;
        if !badge.transferable {
            return Err(BadgeError::TransferNotAllowed);
        }
        if badge.owner != caller {
            return Err(BadgeError::NotOwnerOrAdmin);
        }

        // Remove from current owner and add to new owner.
        remove_badge(&env, badge_id);
        badge.owner = to.clone();
        write_badge(&env, &badge);
        push_badge_to_owner(&env, &to, badge_id);

        env.events()
            .publish(("badge_transferred", badge_id), (caller, to));

        Ok(())
    }

    /// @notice Returns the badge data for a given id.
    /// @param badge_id badge_id parameter
    /// @dev Requires caller authentication
    pub fn get_badge(env: Env, badge_id: u128) -> Option<Badge> {
        env.storage().persistent().get(&StorageKey::Badge(badge_id))
    }

    /// @notice Returns the owner of a badge, if it exists.
    /// @param badge_id badge_id parameter
    /// @dev Requires caller authentication
    pub fn owner_of(env: Env, badge_id: u128) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&StorageKey::OwnerOf(badge_id))
    }

    /// @notice Returns all badge ids currently owned by an address.
    /// @param owner owner parameter
    /// @dev Requires caller authentication
    pub fn badges_of(env: Env, owner: Address) -> Vec<u128> {
        env.storage()
            .persistent()
            .get(&StorageKey::BadgesOf(owner))
            .unwrap_or(Vec::new(&env))
    }

    /// @notice Returns the configured admin address.
    /// @dev Requires caller authentication
    pub fn get_admin(env: Env) -> Option<Address> {
        env.storage().persistent().get(&StorageKey::Admin)
    }
}
