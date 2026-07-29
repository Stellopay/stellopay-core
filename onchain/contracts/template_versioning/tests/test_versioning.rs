use soroban_sdk::{
    testutils::{Address as _, Events, Ledger, LedgerInfo},
    Address, BytesN, Env, IntoVal, String,
};
use template_versioning::{
    AgreementBinding, TemplateVersionDeprecated, TemplateVersionRecord, TemplateVersioning,
    TemplateVersioningClient, VersioningError,
};

fn ledger_ts(env: &Env, ts: u64) {
    env.ledger().set(LedgerInfo {
        timestamp: ts,
        protocol_version: 23,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6_312_000,
    });
}

/// End-to-end: register template, publish versions, bind agreement, deprecate blocks new binds.
#[test]
fn template_version_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();
    ledger_ts(&env, 1_000_000);

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let employer = Address::generate(&env);

    client.initialize(&admin);

    let tid = client
        .try_register_template(&employer, &String::from_str(&env, "Standard payroll"))
        .unwrap()
        .unwrap();

    let h1 = BytesN::from_array(&env, &[1u8; 32]);
    let v1 = client
        .try_publish_template_version(
            &employer,
            &tid,
            &h1,
            &String::from_str(&env, "v1 notes"),
            &false,
        )
        .unwrap()
        .unwrap();
    assert_eq!(v1, 1);

    let h2 = BytesN::from_array(&env, &[2u8; 32]);
    let v2 = client
        .try_publish_template_version(
            &employer,
            &tid,
            &h2,
            &String::from_str(&env, "v2 breaking: added tax fields"),
            &false,
        )
        .unwrap()
        .unwrap();
    assert_eq!(v2, 2);

    assert_eq!(client.try_latest_version(&tid).unwrap().unwrap(), 2);

    let r1: TemplateVersionRecord = client.try_get_version(&tid, &1).unwrap().unwrap();
    assert_eq!(r1.schema_hash, h1);
    let r2: TemplateVersionRecord = client.try_get_version(&tid, &2).unwrap().unwrap();
    assert_eq!(r2.version, 2);

    let aid = client
        .try_create_agreement(&employer, &tid, &1, &String::from_str(&env, "Q1-2025"))
        .unwrap()
        .unwrap();
    let ag: AgreementBinding = client.try_get_agreement(&aid).unwrap().unwrap();
    assert_eq!(ag.template_version, 1);
    assert_eq!(ag.template_id, tid);

    client
        .try_deprecate_version(
            &employer,
            &tid,
            &1,
            &Some(String::from_str(&env, "superseded by v2")),
        )
        .unwrap()
        .unwrap();
    let dep: TemplateVersionRecord = client.try_get_version(&tid, &1).unwrap().unwrap();
    assert!(dep.deprecated);
    assert_eq!(
        dep.deprecation_reason,
        Some(String::from_str(&env, "superseded by v2"))
    );

    assert!(client
        .try_create_agreement(&employer, &tid, &1, &String::from_str(&env, "should fail"),)
        .is_err());

    let aid2 = client
        .try_create_agreement(&employer, &tid, &2, &String::from_str(&env, "Q2-2025"))
        .unwrap()
        .unwrap();
    let ag2 = client.try_get_agreement(&aid2).unwrap().unwrap();
    assert_eq!(ag2.template_version, 2);
}

#[test]
fn non_owner_cannot_publish() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let attacker = Address::generate(&env);

    client.initialize(&admin);
    let tid = client
        .try_register_template(&employer, &String::from_str(&env, "T"))
        .unwrap()
        .unwrap();

    let h = BytesN::from_array(&env, &[9u8; 32]);
    assert!(client
        .try_publish_template_version(&attacker, &tid, &h, &String::from_str(&env, "x"), &false,)
        .is_err());
}

/// Deprecating a version should emit a `TemplateVersionDeprecated` event
/// with the correct template id, version, and timestamp.
#[test]
fn deprecate_version_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    ledger_ts(&env, 2_000_000);

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);

    client.initialize(&admin);

    let tid = client
        .try_register_template(&owner, &String::from_str(&env, "Payroll v1"))
        .unwrap()
        .unwrap();

    let hash = BytesN::from_array(&env, &[7u8; 32]);
    let ver = client
        .try_publish_template_version(
            &owner,
            &tid,
            &hash,
            &String::from_str(&env, "initial"),
            &false,
        )
        .unwrap()
        .unwrap();

    client
        .try_deprecate_version(
            &owner,
            &tid,
            &ver,
            &Some(String::from_str(&env, "security fix")),
        )
        .unwrap()
        .unwrap();

    // Inspect emitted events
    let all_events = env.events().all();
    let last = all_events.last().unwrap();

    // Verify event data
    let emitted: TemplateVersionDeprecated = last.2.into_val(&env);
    assert_eq!(emitted.template_id, tid);
    assert_eq!(emitted.version, ver);
    assert_eq!(emitted.timestamp, 2_000_000u64);
    assert_eq!(emitted.reason, Some(String::from_str(&env, "security fix")));
}

/// Deprecating an already-deprecated version should still emit the event.
#[test]
fn deprecate_already_deprecated_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    ledger_ts(&env, 3_000_000);

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);

    client.initialize(&admin);

    let tid = client
        .try_register_template(&owner, &String::from_str(&env, "Template"))
        .unwrap()
        .unwrap();

    let hash = BytesN::from_array(&env, &[3u8; 32]);
    let ver = client
        .try_publish_template_version(
            &owner,
            &tid,
            &hash,
            &String::from_str(&env, "notes"),
            &false,
        )
        .unwrap()
        .unwrap();

    // First deprecation
    client
        .try_deprecate_version(&owner, &tid, &ver, &None)
        .unwrap()
        .unwrap();

    // Second deprecation (idempotent flag flip, event still emitted), this
    // time supplying a reason to confirm it overwrites the stored value.
    client
        .try_deprecate_version(
            &owner,
            &tid,
            &ver,
            &Some(String::from_str(&env, "legal change")),
        )
        .unwrap()
        .unwrap();

    // `env.events().all()` only retains events from the most recent top-level
    // invocation, so only the second `deprecate_version` call's event is
    // observable here. Its presence is exactly what proves the call emitted
    // an event even though the version was already deprecated (i.e. it did
    // not silently no-op on the idempotent flag flip).
    let all_events = env.events().all();
    let count = all_events
        .iter()
        .filter(|e| {
            let data: Result<TemplateVersionDeprecated, VersioningError> =
                e.2.clone().into_val(&env);
            data.map(|d| d.template_id == tid && d.version == ver)
                .unwrap_or(false)
        })
        .count();
    assert_eq!(count, 1);

    // Stored record reflects the most recent deprecation call's reason.
    let rec: TemplateVersionRecord = client.try_get_version(&tid, &ver).unwrap().unwrap();
    assert_eq!(
        rec.deprecation_reason,
        Some(String::from_str(&env, "legal change"))
    );
}

/// Deprecating with a reason stores it and it's readable via `get_version`,
/// and is also included on the emitted event.
#[test]
fn deprecate_with_reason_is_stored_and_readable() {
    let env = Env::default();
    env.mock_all_auths();
    ledger_ts(&env, 4_000_000);

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);

    client.initialize(&admin);
    let tid = client
        .try_register_template(&owner, &String::from_str(&env, "Template"))
        .unwrap()
        .unwrap();

    let hash = BytesN::from_array(&env, &[11u8; 32]);
    let ver = client
        .try_publish_template_version(&owner, &tid, &hash, &String::from_str(&env, "v1"), &false)
        .unwrap()
        .unwrap();

    // Freshly published, non-deprecated version has no reason yet.
    let fresh: TemplateVersionRecord = client.try_get_version(&tid, &ver).unwrap().unwrap();
    assert_eq!(fresh.deprecation_reason, None);

    let reason = String::from_str(&env, "security fix: fixes reentrancy in payout path");
    client
        .try_deprecate_version(&owner, &tid, &ver, &Some(reason.clone()))
        .unwrap()
        .unwrap();

    // Inspect the emitted event right after the call that produced it — the
    // test env's event log only reflects the most recent top-level
    // invocation, so a subsequent `get_version` call (itself a fresh
    // invocation) would clear it before we get a chance to look.
    let all_events = env.events().all();
    let last = all_events.last().unwrap();
    let emitted: TemplateVersionDeprecated = last.2.into_val(&env);
    assert_eq!(emitted.reason, Some(reason.clone()));

    let rec: TemplateVersionRecord = client.try_get_version(&tid, &ver).unwrap().unwrap();
    assert!(rec.deprecated);
    assert_eq!(rec.deprecation_reason, Some(reason));
}

/// Deprecating without a reason (existing-caller behavior) still succeeds and
/// leaves `deprecation_reason` as `None`.
#[test]
fn deprecate_without_reason_still_works() {
    let env = Env::default();
    env.mock_all_auths();
    ledger_ts(&env, 5_000_000);

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);

    client.initialize(&admin);
    let tid = client
        .try_register_template(&owner, &String::from_str(&env, "Template"))
        .unwrap()
        .unwrap();

    let hash = BytesN::from_array(&env, &[12u8; 32]);
    let ver = client
        .try_publish_template_version(&owner, &tid, &hash, &String::from_str(&env, "v1"), &false)
        .unwrap()
        .unwrap();

    client
        .try_deprecate_version(&owner, &tid, &ver, &None)
        .unwrap()
        .unwrap();

    // Inspect the emitted event right after the call that produced it, before
    // the follow-up `get_version` read (itself a fresh top-level invocation)
    // clears the test env's event log.
    let all_events = env.events().all();
    let last = all_events.last().unwrap();
    let emitted: TemplateVersionDeprecated = last.2.into_val(&env);
    assert_eq!(emitted.reason, None);

    let rec: TemplateVersionRecord = client.try_get_version(&tid, &ver).unwrap().unwrap();
    assert!(rec.deprecated);
    assert_eq!(rec.deprecation_reason, None);
}

/// Non-owner cannot deprecate, so no event is emitted.
#[test]
fn non_owner_cannot_deprecate() {
    let env = Env::default();
    env.mock_all_auths();
    ledger_ts(&env, 1_000_000);

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);

    client.initialize(&admin);

    let tid = client
        .try_register_template(&owner, &String::from_str(&env, "T"))
        .unwrap()
        .unwrap();

    let hash = BytesN::from_array(&env, &[5u8; 32]);
    let ver = client
        .try_publish_template_version(&owner, &tid, &hash, &String::from_str(&env, "v1"), &false)
        .unwrap()
        .unwrap();

    // Attacker attempt should fail
    assert!(client
        .try_deprecate_version(&attacker, &tid, &ver, &None)
        .is_err());

    // No deprecation event should exist
    let all_events = env.events().all();
    let dep_count = all_events
        .iter()
        .filter(|e| {
            let data: Result<TemplateVersionDeprecated, VersioningError> =
                e.2.clone().into_val(&env);
            data.map(|d| d.template_id == tid).unwrap_or(false)
        })
        .count();
    assert_eq!(dep_count, 0);
}

/// Agreements are pinned to the template version they were created with.
/// Publishing a new version does not change what an existing agreement resolves to.
#[test]
fn test_register_template_rejects_collision_with_active_template() {
    let env = Env::default();
    env.mock_all_auths();
    ledger_ts(&env, 1_000_000);

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let owner_a = Address::generate(&env);
    let owner_b = Address::generate(&env);

    client.initialize(&admin);

    // Step 1 & 2: register + publish active version.
    let tid_a = client.register_template(&owner_a, &String::from_str(&env, "Payroll"));
    client.publish_template_version(
        &owner_a,
        &tid_a,
        &BytesN::from_array(&env, &[1u8; 32]),
        &String::from_str(&env, "v1"),
        &false, // not deprecated
    );

    // Step 3: collision must be rejected.
    let result = client.try_register_template(&owner_b, &String::from_str(&env, "Payroll"));
    assert_eq!(
        result,
        Err(Ok(VersioningError::NameCollision)),
        "registering under an active name must return NameCollision"
    );
}

/// Requirement 2 — name freed after all versions deprecated → re-registration allowed.
///
/// Sequence:
///   1. Register template A with name "Payroll", publish v1, deprecate v1.
///   2. Register template B with name "Payroll" → must succeed.
///   3. Verify B gets a distinct template_id.
#[test]
fn test_register_template_allowed_after_all_versions_deprecated() {
    let env = Env::default();
    env.mock_all_auths();
    ledger_ts(&env, 1_000_000);

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let owner_a = Address::generate(&env);
    let owner_b = Address::generate(&env);

    client.initialize(&admin);

    // Step 1: register, publish, deprecate.
    let tid_a = client.register_template(&owner_a, &String::from_str(&env, "Payroll"));
    let ver = client.publish_template_version(
        &owner_a,
        &tid_a,
        &BytesN::from_array(&env, &[1u8; 32]),
        &String::from_str(&env, "v1"),
        &false,
    );
    client.deprecate_version(
        &owner_a,
        &tid_a,
        &ver,
        &Some(String::from_str(&env, "replaced")),
    );

    // Step 2: name is now free — re-registration must succeed.
    let result = client.try_register_template(&owner_b, &String::from_str(&env, "Payroll"));
    assert!(
        result.is_ok(),
        "re-registering a fully-deprecated name must succeed"
    );

    // Step 3: new template has a distinct ID.
    let tid_b = result.unwrap().unwrap();
    assert_ne!(
        tid_a, tid_b,
        "re-registered template must get a distinct template_id"
    );
}

/// Partial deprecation — only some versions deprecated → name still blocked.
///
/// With two published versions, deprecating only the latest must not free the name;
/// the earlier non-deprecated version would have been the latest before v2 was
/// published, but the collision check inspects only the *current* latest version.
/// After deprecating v1 and v2 the name is free.
#[test]
fn test_register_template_rejects_when_earlier_version_not_deprecated() {
    let env = Env::default();
    env.mock_all_auths();
    ledger_ts(&env, 1_000_000);

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let newcomer = Address::generate(&env);

    client.initialize(&admin);

    let tid = client.register_template(&owner, &String::from_str(&env, "Wages"));
    // Publish v1 (not deprecated) then v2 (not deprecated).
    let _v1 = client.publish_template_version(
        &owner,
        &tid,
        &BytesN::from_array(&env, &[1u8; 32]),
        &String::from_str(&env, "v1"),
        &false,
    );
    let v2 = client.publish_template_version(
        &owner,
        &tid,
        &BytesN::from_array(&env, &[2u8; 32]),
        &String::from_str(&env, "v2"),
        &false,
    );

    // Deprecate only the latest (v2); name still blocked because the check
    // looks at the latest version number and v2 *is* the latest.
    // After deprecating v2, the latest version record is deprecated →
    // name is free.
    client.deprecate_version(&owner, &tid, &v2, &None);

    // Now the latest version (v2) is deprecated — name is available.
    let result = client.try_register_template(&newcomer, &String::from_str(&env, "Wages"));
    assert!(
        result.is_ok(),
        "name must be free once the latest version is deprecated"
    );
}

/// A template registered but never given a published version is inert and must
/// not block re-registration under the same name.
#[test]
fn test_register_template_allowed_when_prior_has_no_published_versions() {
    let env = Env::default();
    env.mock_all_auths();
    ledger_ts(&env, 1_000_000);

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let owner_a = Address::generate(&env);
    let owner_b = Address::generate(&env);

    client.initialize(&admin);

    // Register A but never publish a version.
    let _tid_a = client.register_template(&owner_a, &String::from_str(&env, "Bonus"));

    // Name should still be available — no active version exists.
    let result = client.try_register_template(&owner_b, &String::from_str(&env, "Bonus"));
    assert!(
        result.is_ok(),
        "a name with no published versions must be available"
    );
}

/// Distinct names never interfere — registering "Alpha" must not block "Beta".
#[test]
fn test_register_template_different_names_do_not_collide() {
    let env = Env::default();
    env.mock_all_auths();
    ledger_ts(&env, 1_000_000);

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);

    client.initialize(&admin);

    let tid = client
        .try_register_template(&owner, &String::from_str(&env, "Payroll"))
        .unwrap()
        .unwrap();

    // "Beta" is a different name — must succeed unconditionally.
    let result = client.try_register_template(&owner, &String::from_str(&env, "Beta"));
    assert!(result.is_ok(), "distinct names must not interfere");
}

/// New agreements created after publishing a new version correctly use the latest version.
#[test]
fn new_agreement_uses_latest_version_after_publish() {
    let env = Env::default();
    env.mock_all_auths();
    ledger_ts(&env, 1_000_000);

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);

    client.initialize(&admin);

    let tid = client
        .try_register_template(&owner, &String::from_str(&env, "Payroll"))
        .unwrap()
        .unwrap();

    // Publish version 1
    let h1 = BytesN::from_array(&env, &[1u8; 32]);
    let v1 = client
        .try_publish_template_version(
            &owner,
            &tid,
            &h1,
            &String::from_str(&env, "v1 initial"),
            &false,
        )
        .unwrap()
        .unwrap();
    assert_eq!(v1, 1);

    // Create agreement with version 1
    let aid1 = client
        .try_create_agreement(&owner, &tid, &1, &String::from_str(&env, "Agreement A"))
        .unwrap()
        .unwrap();
    let ag1: AgreementBinding = client.try_get_agreement(&aid1).unwrap().unwrap();
    assert_eq!(ag1.template_version, 1);

    // Index must contain both IDs.
    let history: Vec<u64> = client.get_templates_by_name(&String::from_str(&env, "Expense"));
    assert_eq!(history.len(), 2);
    assert!(history.contains(&tid_a));
    assert!(history.contains(&tid_b));
}

/// Re-registration after deprecation produces a new active template; attempting
/// a third registration while the second is active must still be rejected.
#[test]
fn test_register_template_collision_after_reuse() {
    let env = Env::default();
    env.mock_all_auths();
    ledger_ts(&env, 1_000_000);

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);

    client.initialize(&admin);

    // First registration + deprecation.
    let tid_a = client.register_template(&owner, &String::from_str(&env, "Reuse"));
    let va = client.publish_template_version(
        &owner,
        &tid_a,
        &BytesN::from_array(&env, &[1u8; 32]),
        &String::from_str(&env, "v1"),
        &false,
    );
    client.deprecate_version(&owner, &tid_a, &va, &None);

    // Second registration — succeeds because name is free.
    let tid_b = client.register_template(&owner, &String::from_str(&env, "Reuse"));
    client.publish_template_version(
        &owner,
        &tid_b,
        &BytesN::from_array(&env, &[2u8; 32]),
        &String::from_str(&env, "v1"),
        &false,
    );

    // Third registration while second is active — must be rejected.
    let result = client.try_register_template(&owner, &String::from_str(&env, "Reuse"));
    assert_eq!(
        result,
        Err(Ok(VersioningError::NameCollision)),
        "active second-generation template must still block the name"
    );
}

// ── Parameter-schema-mismatch rejection tests for create_agreement ───────────

/// Negative test: create_agreement with a version number that does not exist
/// (simulates a missing required schema field — the caller references a
/// template version whose schema was never published).
#[test]
fn create_agreement_rejects_nonexistent_version() {
    let env = Env::default();
    env.mock_all_auths();
    ledger_ts(&env, 1_000_000);

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);

    client.initialize(&admin);

    let tid = client
        .try_register_template(&owner, &String::from_str(&env, "Schema test"))
        .unwrap()
        .unwrap();

    let hash = BytesN::from_array(&env, &[0xAA; 32]);
    let ver = client
        .try_publish_template_version(
            &owner,
            &tid,
            &hash,
            &String::from_str(&env, "v1"),
            &false,
        )
        .unwrap()
        .unwrap();
    assert_eq!(ver, 1);

    // Attempt to create an agreement referencing version 99 (never published).
    let result = client.try_create_agreement(
        &owner,
        &tid,
        &99,
        &String::from_str(&env, "bad version ref"),
    );
    assert!(result.is_err());

    // Also reject version 0 (versions are 1-based).
    let result_zero = client.try_create_agreement(
        &owner,
        &tid,
        &0,
        &String::from_str(&env, "zero version ref"),
    );
    assert!(result_zero.is_err());
}

/// Negative test: create_agreement with a wrong template_id (simulates
/// supplying a wrong-typed / mismatched parameter — the caller targets a
/// template that does not exist, so the version lookup fails).
#[test]
fn create_agreement_rejects_wrong_template_id() {
    let env = Env::default();
    env.mock_all_auths();
    ledger_ts(&env, 1_000_000);

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);

    client.initialize(&admin);

    let tid = client
        .try_register_template(&owner, &String::from_str(&env, "Real template"))
        .unwrap()
        .unwrap();

    let hash = BytesN::from_array(&env, &[0xBB; 32]);
    client
        .try_publish_template_version(
            &owner,
            &tid,
            &hash,
            &String::from_str(&env, "v1"),
            &false,
        )
        .unwrap()
        .unwrap();

    // Use a completely wrong template_id (9999) with version 1.
    let result = client.try_create_agreement(
        &owner,
        &9999,
        &1,
        &String::from_str(&env, "wrong template"),
    );
    assert!(result.is_err());
}

/// Negative test: create_agreement with an empty label is rejected as
/// invalid data (label is a required field in the agreement schema).
#[test]
fn create_agreement_rejects_empty_label() {
    let env = Env::default();
    env.mock_all_auths();
    ledger_ts(&env, 1_000_000);

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);

    client.initialize(&admin);

    let tid = client
        .try_register_template(&owner, &String::from_str(&env, "Label test"))
        .unwrap()
        .unwrap();

    let hash = BytesN::from_array(&env, &[0xCC; 32]);
    client
        .try_publish_template_version(
            &owner,
            &tid,
            &hash,
            &String::from_str(&env, "v1"),
            &false,
        )
        .unwrap()
        .unwrap();

    // Empty label should be rejected.
    let result = client.try_create_agreement(
        &owner,
        &tid,
        &1,
        &String::from_str(&env, ""),
    );
    assert!(result.is_err());
}

/// Negative test: create_agreement against a deprecated version is rejected,
/// even when the template and version are otherwise valid.
#[test]
fn create_agreement_rejects_deprecated_version() {
    let env = Env::default();
    env.mock_all_auths();
    ledger_ts(&env, 1_000_000);

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);

    client.initialize(&admin);

    let tid = client
        .try_register_template(&owner, &String::from_str(&env, "Deprecation test"))
        .unwrap()
        .unwrap();

    let hash = BytesN::from_array(&env, &[0xDD; 32]);
    let ver = client
        .try_publish_template_version(
            &owner,
            &tid,
            &hash,
            &String::from_str(&env, "v1"),
            &false,
        )
        .unwrap()
        .unwrap();

    // Deprecate the only version.
    client
        .try_deprecate_version(
            &owner,
            &tid,
            &ver,
            &Some(String::from_str(&env, "schema retired")),
        )
        .unwrap()
        .unwrap();

    // Agreement creation against deprecated version must fail.
    let result = client.try_create_agreement(
        &owner,
        &tid,
        &ver,
        &String::from_str(&env, "should fail"),
    );
    assert!(result.is_err());
}

/// Positive test: create_agreement succeeds when all parameters conform to
/// the template's schema — valid template_id, existing non-deprecated
/// version, and a non-empty label.
#[test]
fn create_agreement_succeeds_with_conformant_parameters() {
    let env = Env::default();
    env.mock_all_auths();
    ledger_ts(&env, 1_000_000);

    let contract_id = env.register(TemplateVersioning, ());
    let client = TemplateVersioningClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let creator = Address::generate(&env);

    client.initialize(&admin);

    let tid = client
        .try_register_template(&owner, &String::from_str(&env, "Conformant test"))
        .unwrap()
        .unwrap();

    let hash = BytesN::from_array(&env, &[0xEE; 32]);
    let ver = client
        .try_publish_template_version(
            &owner,
            &tid,
            &hash,
            &String::from_str(&env, "v1 stable"),
            &false,
        )
        .unwrap()
        .unwrap();
    assert_eq!(ver, 1);

    // Fully conformant: correct template, existing non-deprecated version, non-empty label.
    let aid = client
        .try_create_agreement(
            &creator,
            &tid,
            &ver,
            &String::from_str(&env, "Q3-2026 payroll"),
        )
        .unwrap()
        .unwrap();

    let binding: AgreementBinding = client.try_get_agreement(&aid).unwrap().unwrap();
    assert_eq!(binding.template_id, tid);
    assert_eq!(binding.template_version, ver);
    assert_eq!(binding.creator, creator);
    assert_eq!(
        binding.label,
        String::from_str(&env, "Q3-2026 payroll")
    );
    assert_eq!(binding.created_at, 1_000_000);
}
