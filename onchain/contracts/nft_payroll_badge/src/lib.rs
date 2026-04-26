#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, Bytes, Env, Vec,
};

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
    MetadataTooLong = 7,
    BadgeRevoked = 8,
    BadgeExpired = 9,
    MetadataFrozen = 10,
    AlreadyRevoked = 11,
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

/// Lifecycle states for a badge.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BadgeState {
    Active,
    Revoked,
    Expired,
}

/// Events emitted by the NFT payroll badge contract.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BadgeMinted {
    pub id: u128,
    pub owner: Address,
    pub kind: BadgeKind,
    pub transferable: bool,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BadgeBurned {
    pub id: u128,
    pub owner: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BadgeTransferred {
    pub id: u128,
    pub from: Address,
    pub to: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BadgeMetadataUpdated {
    pub id: u128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BadgeMetadataFrozen {
    pub id: u128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BadgeRevokedEvent {
    pub id: u128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BadgeExpiredEvent {
    pub id: u128,
}

/// Represents a single NFT-style payroll badge.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Badge {
    pub id: u128,
    pub owner: Address,
    pub kind: BadgeKind,
    /// Arbitrary metadata URI or opaque bytes (e.g. IPFS CID).
    /// @dev Recommended maximum size: 1024 bytes.
    pub metadata: Bytes,
    /// Whether this badge can be transferred by the holder.
    pub transferable: bool,
    pub created_at: u64,
    /// Timestamp when the badge organically expires. 0 means never.
    pub expires_at: u64,
    /// Whether the badge has been permanently revoked.
    pub revoked: bool,
    /// Whether the metadata is permanently locked from updates.
    pub metadata_frozen: bool,
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
    /// @param expires_at Timestamp when the badge expires (0 for never).
    /// @return badge_id Newly minted badge identifier.
    pub fn mint(
        env: Env,
        caller: Address,
        to: Address,
        kind: BadgeKind,
        metadata: Bytes,
        transferable: bool,
        expires_at: u64,
    ) -> Result<u128, BadgeError> {
        require_initialized(&env)?;
        require_admin(&env, &caller)?;

        if metadata.len() > 1024 {
            return Err(BadgeError::MetadataTooLong);
        }

        let id = next_badge_id(&env);
        let badge = Badge {
            id,
            owner: to.clone(),
            kind,
            metadata,
            transferable,
            created_at: env.ledger().timestamp(),
            expires_at,
            revoked: false,
            metadata_frozen: false,
        };

        write_badge(&env, &badge);
        push_badge_to_owner(&env, &to, id);

        BadgeMinted {
            id,
            owner: to,
            kind: badge.kind.clone(),
            transferable: badge.transferable,
        }
        .publish(&env);

        Ok(id)
    }

    /// @notice Updates the metadata of an existing badge if it hasn't been frozen.
    pub fn update_metadata(
        env: Env,
        caller: Address,
        badge_id: u128,
        metadata: Bytes,
    ) -> Result<(), BadgeError> {
        require_initialized(&env)?;
        require_admin(&env, &caller)?;

        let mut badge = read_badge(&env, badge_id)?;
        if badge.metadata_frozen {
            return Err(BadgeError::MetadataFrozen);
        }
        if metadata.len() > 1024 {
            return Err(BadgeError::MetadataTooLong);
        }

        badge.metadata = metadata;
        write_badge(&env, &badge);

        BadgeMetadataUpdated { id: badge_id }.publish(&env);

        Ok(())
    }

    /// @notice Permanently freezes the metadata of a given badge.
    pub fn freeze_metadata(env: Env, caller: Address, badge_id: u128) -> Result<(), BadgeError> {
        require_initialized(&env)?;
        require_admin(&env, &caller)?;

        let mut badge = read_badge(&env, badge_id)?;
        if badge.metadata_frozen {
            return Err(BadgeError::MetadataFrozen);
        }

        badge.metadata_frozen = true;
        write_badge(&env, &badge);

        BadgeMetadataFrozen { id: badge_id }.publish(&env);

        Ok(())
    }

    /// @notice Revokes a badge permanently (e.g. employee termination).
    pub fn revoke(env: Env, caller: Address, badge_id: u128) -> Result<(), BadgeError> {
        require_initialized(&env)?;
        require_admin(&env, &caller)?;

        let mut badge = read_badge(&env, badge_id)?;
        if badge.revoked {
            return Err(BadgeError::AlreadyRevoked);
        }

        badge.revoked = true;
        write_badge(&env, &badge);

        BadgeRevokedEvent { id: badge_id }.publish(&env);

        Ok(())
    }

    /// @notice Manually marks a badge as expired right now.
    pub fn expire(env: Env, caller: Address, badge_id: u128) -> Result<(), BadgeError> {
        require_initialized(&env)?;
        require_admin(&env, &caller)?;

        let mut badge = read_badge(&env, badge_id)?;
        let current_time = env.ledger().timestamp();
        
        if badge.expires_at != 0 && current_time >= badge.expires_at {
            return Err(BadgeError::BadgeExpired);
        }

        badge.expires_at = current_time;
        write_badge(&env, &badge);

        BadgeExpiredEvent { id: badge_id }.publish(&env);

        Ok(())
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

        BadgeBurned {
            id: badge_id,
            owner: badge.owner,
        }
        .publish(&env);

        Ok(())
    }

    /// @notice Transfers a badge to a new owner if it is marked transferable.
    /// @dev Caller must be current owner; non-transferable or inactive badges cannot be moved.
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

        // Check lifecycle state before allowing transfer
        if badge.revoked {
            return Err(BadgeError::BadgeRevoked);
        }
        if badge.expires_at != 0 && env.ledger().timestamp() >= badge.expires_at {
            return Err(BadgeError::BadgeExpired);
        }

        // Remove from current owner and add to new owner.
        remove_badge(&env, badge_id);
        badge.owner = to.clone();
        write_badge(&env, &badge);
        push_badge_to_owner(&env, &to, badge_id);

        BadgeTransferred {
            id: badge_id,
            from: caller,
            to,
        }
        .publish(&env);

        Ok(())
    }

    /// @notice Returns the badge data for a given id.
    /// @param badge_id badge_id parameter
    pub fn get_badge(env: Env, badge_id: u128) -> Option<Badge> {
        env.storage().persistent().get(&StorageKey::Badge(badge_id))
    }

    /// @notice Computes the current lifecycle state of a badge.
    pub fn get_state(env: Env, badge_id: u128) -> Result<BadgeState, BadgeError> {
        let badge = read_badge(&env, badge_id)?;
        if badge.revoked {
            return Ok(BadgeState::Revoked);
        }
        if badge.expires_at != 0 && env.ledger().timestamp() >= badge.expires_at {
            return Ok(BadgeState::Expired);
        }
        Ok(BadgeState::Active)
    }

    /// @notice Returns the owner of a badge, if it exists.
    /// @param badge_id badge_id parameter
    pub fn owner_of(env: Env, badge_id: u128) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&StorageKey::OwnerOf(badge_id))
    }

    /// @notice Returns all badge ids currently owned by an address.
    /// @param owner owner parameter
    pub fn badges_of(env: Env, owner: Address) -> Vec<u128> {
        env.storage()
            .persistent()
            .get(&StorageKey::BadgesOf(owner))
            .unwrap_or(Vec::new(&env))
    }

    /// @notice Returns the configured admin address.
    pub fn get_admin(env: Env) -> Option<Address> {
        env.storage().persistent().get(&StorageKey::Admin)
    }
}
