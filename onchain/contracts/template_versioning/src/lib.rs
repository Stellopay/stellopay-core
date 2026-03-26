#![no_std]

//! Payroll / escrow template versioning: immutable version records, lookup, and agreement bindings.
//!
//! # Migration
//! When template fields change, publish a new version with a new `schema_hash` (e.g. hash of the
//! canonical schema). Existing agreements keep their `template_version`; new agreements pick an
//! explicit version or the latest non-deprecated version. Deprecate old versions after a cutover
//! window so reviewers can enforce that only current schemas are used for new payrolls.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, String,
};

/// Persistent storage keys.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    NextTemplateId,
    NextAgreementId,
    TemplateOwner(u64),
    TemplateName(u64),
    TemplateLatest(u64),
    TemplateVersion(u64, u32),
    Agreement(u64),
}

/// One immutable template revision.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TemplateVersionRecord {
    /// Owning template id.
    pub template_id: u64,
    /// Monotonic version number for this template (1-based).
    pub version: u32,
    /// Commitment to the off-chain schema / payload (e.g. SHA-256 of JSON ABI).
    pub schema_hash: BytesN<32>,
    /// Human-readable or IPFS CID for documentation.
    pub migration_notes: String,
    /// Ledger time when this version was published.
    pub created_at: u64,
    /// When true, `create_agreement` rejects this version unless explicitly allowed.
    pub deprecated: bool,
}

/// Agreement created from a specific template version.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AgreementBinding {
    pub agreement_id: u64,
    pub template_id: u64,
    pub template_version: u32,
    pub creator: Address,
    pub label: String,
    pub created_at: u64,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum VersioningError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    TemplateNotFound = 4,
    VersionNotFound = 5,
    VersionDeprecated = 6,
    InvalidData = 7,
    AgreementNotFound = 8,
}

#[contract]
pub struct TemplateVersioning;

#[contractimpl]
impl TemplateVersioning {
    /// One-time admin setup.
    pub fn initialize(env: Env, admin: Address) -> Result<(), VersioningError> {
        admin.require_auth();
        let storage = env.storage().persistent();
        if storage.has(&DataKey::Admin) {
            return Err(VersioningError::AlreadyInitialized);
        }
        storage.set(&DataKey::Admin, &admin);
        storage.set(&DataKey::NextTemplateId, &1u64);
        storage.set(&DataKey::NextAgreementId, &1u64);
        Ok(())
    }

    /// Register a named template; returns a stable `template_id`. The authenticated `owner` may later publish versions.
    pub fn register_template(env: Env, owner: Address, name: String) -> Result<u64, VersioningError> {
        owner.require_auth();
        Self::require_initialized(&env)?;
        if name.is_empty() {
            return Err(VersioningError::InvalidData);
        }
        let storage = env.storage().persistent();
        let id: u64 = storage
            .get(&DataKey::NextTemplateId)
            .ok_or(VersioningError::NotInitialized)?;
        storage.set(&DataKey::TemplateOwner(id), &owner);
        storage.set(&DataKey::TemplateName(id), &name);
        storage.set(&DataKey::TemplateLatest(id), &0u32);
        storage.set(&DataKey::NextTemplateId, &(id + 1));
        Ok(id)
    }

    /// Publish a new immutable version for `template_id`.
    pub fn publish_template_version(
        env: Env,
        owner: Address,
        template_id: u64,
        schema_hash: BytesN<32>,
        migration_notes: String,
        deprecated: bool,
    ) -> Result<u32, VersioningError> {
        owner.require_auth();
        let storage = env.storage().persistent();
        let template_owner: Address = storage
            .get(&DataKey::TemplateOwner(template_id))
            .ok_or(VersioningError::TemplateNotFound)?;
        if template_owner != owner {
            return Err(VersioningError::Unauthorized);
        }
        let latest: u32 = storage
            .get(&DataKey::TemplateLatest(template_id))
            .unwrap_or(0);
        let version = latest.saturating_add(1);
        if version == 0 {
            return Err(VersioningError::InvalidData);
        }
        let now = env.ledger().timestamp();
        let record = TemplateVersionRecord {
            template_id,
            version,
            schema_hash,
            migration_notes,
            created_at: now,
            deprecated,
        };
        storage.set(&DataKey::TemplateVersion(template_id, version), &record);
        storage.set(&DataKey::TemplateLatest(template_id), &version);
        Ok(version)
    }

    /// Mark a version as deprecated so new agreements cannot use it (unless caller uses force).
    pub fn deprecate_version(
        env: Env,
        owner: Address,
        template_id: u64,
        version: u32,
    ) -> Result<(), VersioningError> {
        owner.require_auth();
        let storage = env.storage().persistent();
        let template_owner: Address = storage
            .get(&DataKey::TemplateOwner(template_id))
            .ok_or(VersioningError::TemplateNotFound)?;
        if template_owner != owner {
            return Err(VersioningError::Unauthorized);
        }
        let key = DataKey::TemplateVersion(template_id, version);
        let mut rec: TemplateVersionRecord = storage.get(&key).ok_or(VersioningError::VersionNotFound)?;
        rec.deprecated = true;
        storage.set(&key, &rec);
        Ok(())
    }

    /// Return the latest published version number, if any.
    pub fn latest_version(env: Env, template_id: u64) -> Result<u32, VersioningError> {
        let storage = env.storage().persistent();
        storage
            .get(&DataKey::TemplateLatest(template_id))
            .filter(|v| *v > 0)
            .ok_or(VersioningError::VersionNotFound)
    }

    /// Fetch a specific version record.
    pub fn get_version(
        env: Env,
        template_id: u64,
        version: u32,
    ) -> Result<TemplateVersionRecord, VersioningError> {
        let storage = env.storage().persistent();
        storage
            .get(&DataKey::TemplateVersion(template_id, version))
            .ok_or(VersioningError::VersionNotFound)
    }

    /// Create an agreement bound to an exact template version (must not be deprecated).
    pub fn create_agreement(
        env: Env,
        creator: Address,
        template_id: u64,
        template_version: u32,
        label: String,
    ) -> Result<u64, VersioningError> {
        creator.require_auth();
        if label.is_empty() {
            return Err(VersioningError::InvalidData);
        }
        let storage = env.storage().persistent();
        let rec: TemplateVersionRecord = storage
            .get(&DataKey::TemplateVersion(template_id, template_version))
            .ok_or(VersioningError::VersionNotFound)?;
        if rec.deprecated {
            return Err(VersioningError::VersionDeprecated);
        }
        let id: u64 = storage
            .get(&DataKey::NextAgreementId)
            .ok_or(VersioningError::NotInitialized)?;
        let binding = AgreementBinding {
            agreement_id: id,
            template_id,
            template_version,
            creator: creator.clone(),
            label,
            created_at: env.ledger().timestamp(),
        };
        storage.set(&DataKey::Agreement(id), &binding);
        storage.set(&DataKey::NextAgreementId, &(id + 1));
        Ok(id)
    }

    /// Load an agreement by id.
    pub fn get_agreement(env: Env, agreement_id: u64) -> Result<AgreementBinding, VersioningError> {
        let storage = env.storage().persistent();
        storage
            .get(&DataKey::Agreement(agreement_id))
            .ok_or(VersioningError::AgreementNotFound)
    }

    fn require_initialized(env: &Env) -> Result<(), VersioningError> {
        if env.storage().persistent().has(&DataKey::Admin) {
            Ok(())
        } else {
            Err(VersioningError::NotInitialized)
        }
    }
}
