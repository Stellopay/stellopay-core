use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    Address, BytesN, Env, String,
};
use template_versioning::{AgreementBinding, TemplateVersionRecord, TemplateVersioning, TemplateVersioningClient};

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
        .try_register_template(
            &employer,
            &String::from_str(&env, "Standard payroll"),
        )
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

    assert_eq!(
        client.try_latest_version(&tid).unwrap().unwrap(),
        2
    );

    let r1: TemplateVersionRecord = client
        .try_get_version(&tid, &1)
        .unwrap()
        .unwrap();
    assert_eq!(r1.schema_hash, h1);
    let r2: TemplateVersionRecord = client
        .try_get_version(&tid, &2)
        .unwrap()
        .unwrap();
    assert_eq!(r2.version, 2);

    let aid = client
        .try_create_agreement(
            &employer,
            &tid,
            &1,
            &String::from_str(&env, "Q1-2025"),
        )
        .unwrap()
        .unwrap();
    let ag: AgreementBinding = client.try_get_agreement(&aid).unwrap().unwrap();
    assert_eq!(ag.template_version, 1);
    assert_eq!(ag.template_id, tid);

    client
        .try_deprecate_version(&employer, &tid, &1)
        .unwrap()
        .unwrap();
    let dep: TemplateVersionRecord = client
        .try_get_version(&tid, &1)
        .unwrap()
        .unwrap();
    assert!(dep.deprecated);

    assert!(
        client
            .try_create_agreement(
                &employer,
                &tid,
                &1,
                &String::from_str(&env, "should fail"),
            )
            .is_err()
    );

    let aid2 = client
        .try_create_agreement(
            &employer,
            &tid,
            &2,
            &String::from_str(&env, "Q2-2025"),
        )
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
        .try_register_template(
            &employer,
            &String::from_str(&env, "T"),
        )
        .unwrap()
        .unwrap();

    let h = BytesN::from_array(&env, &[9u8; 32]);
    assert!(
        client
            .try_publish_template_version(
                &attacker,
                &tid,
                &h,
                &String::from_str(&env, "x"),
                &false,
            )
            .is_err()
    );
}
