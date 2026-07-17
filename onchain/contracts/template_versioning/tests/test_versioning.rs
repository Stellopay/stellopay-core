use soroban_sdk::{
    testutils::{Address as _, Events, Ledger, LedgerInfo},
    Address, BytesN, Env, IntoVal, String, Vec,
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

    let rec: TemplateVersionRecord = client.try_get_version(&tid, &ver).unwrap().unwrap();
    assert!(rec.deprecated);
    assert_eq!(rec.deprecation_reason, Some(reason.clone()));

    let all_events = env.events().all();
    let last = all_events.last().unwrap();
    let emitted: TemplateVersionDeprecated = last.2.into_val(&env);
    assert_eq!(emitted.reason, Some(reason));
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

    let rec: TemplateVersionRecord = client.try_get_version(&tid, &ver).unwrap().unwrap();
    assert!(rec.deprecated);
    assert_eq!(rec.deprecation_reason, None);

    let all_events = env.events().all();
    let last = all_events.last().unwrap();
    let emitted: TemplateVersionDeprecated = last.2.into_val(&env);
    assert_eq!(emitted.reason, None);
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
