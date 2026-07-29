use soroban_sdk::{contracttype, Address, Env, IntoVal, Symbol, Val, Vec};

/// Canonical lifecycle audit events recorded by the payroll contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuditEvent {
    AgreementCreated,
    AgreementActivated,
    AgreementCancelled,
    DisputeRaised,
    DisputeResolved,
    /// A multisig threshold configuration change (`set_multisig_config`).
    ///
    /// This is a contract-level event not tied to an agreement; entries use a
    /// sentinel `agreement_id` of `0`.
    MultisigConfigChanged,
    /// An arbiter was assigned via `set_arbiter`.
    ///
    /// Contract-level event not tied to an agreement; entries use a sentinel
    /// `agreement_id` of `0` and `subject` is the newly-set arbiter.
    ArbiterSet,
}

/// Append-only audit entry for critical agreement lifecycle transitions.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LifecycleAuditEntry {
    pub id: u64,
    pub timestamp: u64,
    pub actor: Address,
    pub event: AuditEvent,
    pub agreement_id: u128,
    pub subject: Option<Address>,
    pub amount: Option<i128>,
    pub external_log_id: Option<u64>,
}

/// Paginated result of an employer-scoped audit query.
///
/// `next_start_id` is the append-only audit id to pass as `start_id` on a
/// follow-up call to continue scanning where this page left off. It is
/// `None` once the local audit log has been fully scanned, meaning there are
/// no further entries to check (not necessarily that more matches exist).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmployerAuditPage {
    pub entries: Vec<LifecycleAuditEntry>,
    pub next_start_id: Option<u64>,
}

#[contracttype]
#[derive(Clone)]
enum AuditStorageKey {
    AuditLogger,
    NextAuditEntryId,
    AuditEntry(u64),
    AuditEntryCount,
}

impl AuditEvent {
    fn action(&self, env: &Env) -> Symbol {
        match self {
            AuditEvent::AgreementCreated => Symbol::new(env, "agreement_created"),
            AuditEvent::AgreementActivated => Symbol::new(env, "agreement_activated"),
            AuditEvent::AgreementCancelled => Symbol::new(env, "agreement_cancelled"),
            AuditEvent::DisputeRaised => Symbol::new(env, "dispute_raised"),
            AuditEvent::DisputeResolved => Symbol::new(env, "dispute_resolved"),
            AuditEvent::MultisigConfigChanged => Symbol::new(env, "multisig_config_changed"),
            AuditEvent::ArbiterSet => Symbol::new(env, "arbiter_set"),
        }
    }
}

/// @notice Configures the shared audit logger contract used for lifecycle log linkage.
/// @dev Only the contract owner can set this address. Existing lifecycle events remain local
/// if no external audit logger is configured.
pub fn set_audit_logger(env: &Env, owner: Address, audit_logger: Address) {
    owner.require_auth();
    let configured_owner: Address = env
        .storage()
        .persistent()
        .get(&crate::storage::StorageKey::Owner)
        .expect("Owner not set");
    assert!(owner == configured_owner, "Unauthorized: not owner");

    env.storage()
        .persistent()
        .set(&AuditStorageKey::AuditLogger, &audit_logger);
}

/// @notice Returns the configured shared audit logger address, if present.
pub fn get_audit_logger(env: &Env) -> Option<Address> {
    env.storage()
        .persistent()
        .get(&AuditStorageKey::AuditLogger)
}

/// @notice Returns the number of local lifecycle audit entries appended.
pub fn get_audit_entry_count(env: &Env) -> u64 {
    env.storage()
        .persistent()
        .get(&AuditStorageKey::AuditEntryCount)
        .unwrap_or(0u64)
}

/// @notice Returns one local lifecycle audit entry by its append-only id.
pub fn get_audit_entry(env: &Env, audit_id: u64) -> Option<LifecycleAuditEntry> {
    env.storage()
        .persistent()
        .get(&AuditStorageKey::AuditEntry(audit_id))
}

/// @notice Returns lifecycle audit entries belonging to a specific employer's agreements.
/// @dev Read-only: does not introduce any new write authorization surface. Scans local
/// audit entries starting at `start_id` (an id of `0` is treated as `1`, i.e. the start of
/// the log) in append-only id order. For each entry, resolves `agreement_id` to its
/// `Agreement` and keeps only entries whose agreement `employer` matches the requested
/// address.
///
/// Contract-level entries that use the sentinel `agreement_id` of `0` (e.g.
/// `MultisigConfigChanged`, `ArbiterSet`) are not tied to any agreement and therefore have
/// no employer to scope by — they are always excluded from employer-filtered results,
/// even when their `actor` happens to be the requested employer.
///
/// Stops once `limit` matching entries have been collected or the audit log is exhausted.
/// The unfiltered `get_audit_entry_count` / `get_audit_entry` behavior is unchanged by
/// this function; it only adds a filtered view on top of the same underlying log.
pub fn get_audit_entries_by_employer(
    env: &Env,
    employer: Address,
    start_id: u64,
    limit: u32,
) -> EmployerAuditPage {
    let mut entries = Vec::new(env);
    let count = get_audit_entry_count(env);
    let mut id = if start_id == 0 { 1 } else { start_id };
    let mut next_start_id: Option<u64> = None;

    while id <= count {
        if entries.len() >= limit {
            next_start_id = Some(id);
            break;
        }
        if let Some(entry) = get_audit_entry(env, id) {
            if entry.agreement_id != 0 {
                if let Some(agreement) = crate::payroll::get_agreement(env, entry.agreement_id) {
                    if agreement.employer == employer {
                        entries.push_back(entry);
                    }
                }
            }
        }
        id += 1;
    }

    EmployerAuditPage {
        entries,
        next_start_id,
    }
}

/// @notice Records a successful lifecycle transition locally and in the configured audit logger.
/// @dev This helper is called only after all state changes and lifecycle events have succeeded.
/// If the external audit logger rejects the append, the transaction reverts and no partial audit
/// trail can be committed.
pub fn record_entry(
    env: &Env,
    actor: Address,
    event: AuditEvent,
    agreement_id: u128,
    subject: Option<Address>,
    amount: Option<i128>,
) -> u64 {
    let external_log_id = append_external_log(env, &actor, &event, &subject, &amount);

    let id = env
        .storage()
        .persistent()
        .get(&AuditStorageKey::NextAuditEntryId)
        .unwrap_or(1u64);

    let entry = LifecycleAuditEntry {
        id,
        timestamp: env.ledger().timestamp(),
        actor,
        event,
        agreement_id,
        subject,
        amount,
        external_log_id,
    };

    env.storage()
        .persistent()
        .set(&AuditStorageKey::AuditEntry(id), &entry);
    env.storage()
        .persistent()
        .set(&AuditStorageKey::NextAuditEntryId, &(id + 1));
    env.storage()
        .persistent()
        .set(&AuditStorageKey::AuditEntryCount, &id);

    id
}

fn append_external_log(
    env: &Env,
    actor: &Address,
    event: &AuditEvent,
    subject: &Option<Address>,
    amount: &Option<i128>,
) -> Option<u64> {
    get_audit_logger(env).map(|audit_logger| {
        let mut args = Vec::<Val>::new(env);
        args.push_back(actor.clone().into_val(env));
        args.push_back(event.action(env).into_val(env));
        args.push_back(subject.clone().into_val(env));
        args.push_back(amount.clone().into_val(env));

        env.invoke_contract::<u64>(&audit_logger, &Symbol::new(env, "append_log"), args)
    })
}
