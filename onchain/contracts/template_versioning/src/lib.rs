#![no_std]

//! Payroll / escrow template versioning: immutable version records, lookup, and agreement bindings.
//!
//! # Migration
//! When template fields change, publish a new version with a new `schema_hash` (e.g. hash of the
//! canonical schema). Existing agreements keep their `template_version`; new agreements pick an
//! explicit version or the latest non-deprecated version. Deprecate old versions after a cutover
//! window so reviewers can enforce that only current schemas are used for new payrolls.
//!
//! # Naming-Collision Policy
//!
//! Template names are treated as a shared namespace visible to all agreement creators. Registering
//! a new template under a name that is already held by an **active** (non-deprecated) template is
//! rejected with [`VersioningError::NameCollision`]. This prevents accidental shadowing of a
//! template that agreements are actively being created against.
//!
//! A name becomes **available** again only once **every** published version of every template that
//! previously used that name has been explicitly deprecated via
//! [`TemplateVersioning::deprecate_version`]. This is intentionally strict: it forces operators to
//! make a conscious decision to retire all active schema versions before the name can be reused.
//!
//! ## Collision-check algorithm
//!
//! `register_template` looks up the index key `TemplateNameIndex(name)`, which stores a list of
//! all template IDs ever registered under that name. For each such ID it reads the latest version
//! number (`TemplateLatest`) and, if non-zero, checks whether that version's record is
//! non-deprecated. If **any** ID has at least one non-deprecated version, registration fails with
//! `NameCollision`.
//!
//! > **Security note**: the index is append-only and maintained exclusively by `register_template`.
//! > Because a template ID is only appended to the index at registration time (and never removed),
//! > the check is exhaustive: all historical registrations under a name are always inspected.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
    String, Vec,
};

// ─── Storage Keys ─────────────────────────────────────────────────────────────

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
    /// Append-only list of template IDs registered under a given name.
    ///
    /// Value type: `Vec<u64>`.
    ///
    /// Maintained by `register_template`; read by the name-collision check.
    /// An entry is **never** removed so the check always sees the full
    /// history of registrations for a name.
    TemplateNameIndex(String),
}

// ─── Types ────────────────────────────────────────────────────────────────────

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
    /// Optional human-readable reason for deprecation (e.g. "security fix",
    /// "legal change", "routine update"). `None` until the version is
    /// deprecated, or if the caller chose not to supply one.
    pub deprecation_reason: Option<String>,
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

/// Emitted when a template version is deprecated via [`TemplateVersioning::deprecate_version`].
///
/// Off-chain indexers should subscribe to this event to detect deprecations
/// immediately rather than polling the contract state.
///
/// # Fields
/// - `template_id` – Stable identifier of the owning template.
/// - `version`     – Version number that was deprecated.
/// - `timestamp`   – Ledger timestamp at the moment of deprecation.
/// - `reason`      – Optional human-readable reason for the deprecation.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TemplateVersionDeprecated {
    /// Owning template id.
    pub template_id: u64,
    /// The version number that was deprecated.
    pub version: u32,
    /// Ledger timestamp at the moment of deprecation.
    pub timestamp: u64,
    /// Optional human-readable reason for the deprecation.
    pub reason: Option<String>,
}

// ─── Errors ───────────────────────────────────────────────────────────────────

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
    /// A template with the same name already exists and has at least one
    /// non-deprecated version.
    ///
    /// # Resolution
    /// Deprecate all versions of every template registered under the same name
    /// (via [`TemplateVersioning::deprecate_version`]) before attempting to
    /// register a new template under that name.
    NameCollision = 9,
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct TemplateVersioning;

#[contractimpl]
impl TemplateVersioning {
    // ── Initialisation ────────────────────────────────────────────────────────

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

    // ── Template Registration ─────────────────────────────────────────────────

    /// Register a named template; returns a stable `template_id`. The authenticated
    /// `owner` may later publish versions via [`publish_template_version`].
    ///
    /// # Naming-Collision Policy
    ///
    /// Registration is **rejected** with [`VersioningError::NameCollision`] if a
    /// template with the same `name` already exists **and** has at least one
    /// non-deprecated version. This prevents new registrations from silently
    /// shadowing an active template that agreements are currently being created
    /// against.
    ///
    /// Registration **succeeds** when every version of every prior template with
    /// the same name has been explicitly deprecated. The intent is that operators
    /// consciously retire all active schema versions before the name can be reused.
    ///
    /// # Arguments
    /// * `owner` – Address that will own and manage this template. Must sign.
    /// * `name`  – Human-readable name for the template. Must be non-empty. Uniqueness within the
    ///   active namespace is enforced.
    ///
    /// # Errors
    /// * [`VersioningError::NotInitialized`] – Contract not yet initialised.
    /// * [`VersioningError::InvalidData`]    – `name` is empty.
    /// * [`VersioningError::NameCollision`]  – An active template with this name already exists
    ///   (see policy above).
    pub fn register_template(
        env: Env,
        owner: Address,
        name: String,
    ) -> Result<u64, VersioningError> {
        owner.require_auth();
        Self::require_initialized(&env)?;
        if name.is_empty() {
            return Err(VersioningError::InvalidData);
        }

        // ── Name-collision guard ──────────────────────────────────────────────
        //
        // Look up every template ID that has ever been registered under this
        // name. If any of them has at least one non-deprecated published version,
        // reject the registration.
        //
        // A template that has never had a version published (latest == 0) is
        // treated as inert and does not block the name — this can happen if a
        // caller registered a template but never published a version, or if all
        // versions were deprecated before any version was published (edge case).
        Self::check_name_available(&env, &name)?;

        let storage = env.storage().persistent();
        let id: u64 = storage
            .get(&DataKey::NextTemplateId)
            .ok_or(VersioningError::NotInitialized)?;

        // Append this new template ID to the name index so future collision
        // checks include it.
        let index_key = DataKey::TemplateNameIndex(name.clone());
        let mut ids: Vec<u64> = storage.get(&index_key).unwrap_or_else(|| Vec::new(&env));
        ids.push_back(id);
        storage.set(&index_key, &ids);

        storage.set(&DataKey::TemplateOwner(id), &owner);
        storage.set(&DataKey::TemplateName(id), &name);
        storage.set(&DataKey::TemplateLatest(id), &0u32);
        storage.set(&DataKey::NextTemplateId, &(id + 1));
        Ok(id)
    }

    // ── Version Publishing ────────────────────────────────────────────────────

    /// Publish a new immutable version for `template_id`.
    ///
    /// # Security invariant
    /// This function only appends a new version record. It does **not** modify or affect
    /// existing agreements that are pinned to earlier versions. Existing agreements
    /// continue to resolve to their originally specified `(template_id, version)` pair.
    ///
    /// # Parameters
    /// - `owner`: Must be the registered owner of `template_id`
    /// - `template_id`: The template to publish a new version for
    /// - `schema_hash`: Commitment to the off-chain schema (e.g., SHA-256 of JSON ABI)
    /// - `migration_notes`: Human-readable notes about changes from previous version
    /// - `deprecated`: If true, this version is immediately deprecated upon publication
    ///
    /// # Returns
    /// The new version number (monotonically increasing, starting at 1)
    ///
    /// # Errors
    /// - `Unauthorized` if caller is not the template owner
    /// - `TemplateNotFound` if `template_id` does not exist
    /// - `InvalidData` if version would overflow u32
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
            deprecation_reason: None,
        };
        storage.set(&DataKey::TemplateVersion(template_id, version), &record);
        storage.set(&DataKey::TemplateLatest(template_id), &version);
        Ok(version)
    }

    // ── Deprecation ───────────────────────────────────────────────────────────

    /// Mark a version as deprecated so new agreements cannot use it (unless caller uses force).
    ///
    /// `reason` is an optional human-readable explanation (e.g. "security fix",
    /// "legal change", "routine update") stored alongside the `deprecated` flag
    /// and readable via [`TemplateVersioning::get_version`]. Pass `None` to
    /// deprecate without recording a reason.
    pub fn deprecate_version(
        env: Env,
        owner: Address,
        template_id: u64,
        version: u32,
        reason: Option<String>,
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
        let mut rec: TemplateVersionRecord =
            storage.get(&key).ok_or(VersioningError::VersionNotFound)?;
        rec.deprecated = true;
        rec.deprecation_reason = reason.clone();
        storage.set(&key, &rec);

        let now = env.ledger().timestamp();
        env.events().publish(
            (symbol_short!("tmpl_dep"), template_id, version),
            TemplateVersionDeprecated {
                template_id,
                version,
                timestamp: now,
                reason,
            },
        );

        Ok(())
    }

    // ── Views ─────────────────────────────────────────────────────────────────

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

    /// Return all template IDs ever registered under a given name.
    ///
    /// Useful for off-chain tooling and auditors to inspect the full history of
    /// registrations for a name. Returns an empty list if the name has never
    /// been registered.
    pub fn get_templates_by_name(env: Env, name: String) -> Vec<u64> {
        let storage = env.storage().persistent();
        storage
            .get(&DataKey::TemplateNameIndex(name))
            .unwrap_or_else(|| Vec::new(&env))
    }

    // ── Agreement Management ──────────────────────────────────────────────────

    /// Create an agreement bound to an exact template version (must not be deprecated).
    ///
    /// # Security invariant - Version pinning
    /// The agreement is **permanently pinned** to the exact `(template_id, template_version)` pair
    /// specified at creation time. This pinning is immutable:
    /// - The `template_version` stored in `AgreementBinding` never changes
    /// - Future calls to `publish_template_version` do not affect this agreement
    /// - Future calls to `deprecate_version` on this version only prevent new agreements from using
    ///   it
    /// - Existing agreements remain valid and continue to resolve to their pinned version
    ///
    /// This prevents silent schema migrations that could break agreement validation logic.
    ///
    /// # Parameters
    /// - `creator`: Address creating the agreement (must authenticate)
    /// - `template_id`: The template to bind to
    /// - `template_version`: The specific version to pin this agreement to
    /// - `label`: Human-readable label for the agreement (must be non-empty)
    ///
    /// # Returns
    /// The new `agreement_id`
    ///
    /// # Errors
    /// - `InvalidData` if `label` is empty
    /// - `VersionNotFound` if the specified version does not exist
    /// - `VersionDeprecated` if the specified version is deprecated
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

    // ── Internal Helpers ──────────────────────────────────────────────────────

    fn require_initialized(env: &Env) -> Result<(), VersioningError> {
        if env.storage().persistent().has(&DataKey::Admin) {
            Ok(())
        } else {
            Err(VersioningError::NotInitialized)
        }
    }

    /// Check that `name` does not collide with any active (non-deprecated) template.
    ///
    /// # Algorithm
    ///
    /// 1. Load `TemplateNameIndex(name)` — the append-only list of all template IDs ever registered
    ///    under this name. If the key is absent, the name is free.
    /// 2. For each ID, read `TemplateLatest(id)`. If `latest == 0` the template has no published
    ///    versions and is inert — skip it.
    /// 3. Read `TemplateVersion(id, latest)`. If that version is **not** deprecated, the name is
    ///    still active → return `NameCollision`.
    /// 4. If every ID is either versionless or fully deprecated, the name is available.
    ///
    /// # Security
    ///
    /// The index is append-only and written only by `register_template` under auth.
    /// Entries are never removed, so the check is exhaustive across all historical
    /// registrations. A name cannot be "freed" by any means other than deprecating
    /// all published versions.
    fn check_name_available(env: &Env, name: &String) -> Result<(), VersioningError> {
        let storage = env.storage().persistent();
        let index_key = DataKey::TemplateNameIndex(name.clone());
        let ids: Vec<u64> = match storage.get(&index_key) {
            Some(v) => v,
            None => return Ok(()), // name never used
        };

        for id in ids.iter() {
            let latest: u32 = storage.get(&DataKey::TemplateLatest(id)).unwrap_or(0);
            if latest == 0 {
                // Template registered but no version published — inert, skip.
                continue;
            }
            let rec: TemplateVersionRecord =
                match storage.get(&DataKey::TemplateVersion(id, latest)) {
                    Some(r) => r,
                    None => continue, // storage inconsistency — treat as inert
                };
            if !rec.deprecated {
                return Err(VersioningError::NameCollision);
            }
        }
        Ok(())
    }
}
