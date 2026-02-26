//! Automated Compliance Checker tests (#233).

#![cfg(test)]
#![allow(deprecated)]

use compliance_checker::{
    ComplianceCheckerContract, ComplianceCheckerContractClient, RuleKind,
};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol, Vec};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup(env: &Env) -> (Address, ComplianceCheckerContractClient<'_>, Address) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, ComplianceCheckerContract);
    let client = ComplianceCheckerContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (contract_id, client, admin)
}

#[test]
fn test_add_and_get_rule() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);

    let rule_id = client.add_rule(
        &admin,
        &RuleKind::MaxValue(symbol_short!("amount"), 1_000),
        &symbol_short!("max_amt"),
        &5u32,
    );
    assert_eq!(rule_id, 1);

    let fetched = client.get_rule(&rule_id);
    assert_eq!(fetched.id, rule_id);
    assert!(fetched.active);
}

#[test]
fn test_list_rules() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);

    client.add_rule(
        &admin,
        &RuleKind::RequiredFlag(symbol_short!("kyc")),
        &symbol_short!("kyc_flag"),
        &3u32,
    );

    let rules = client.list_rules();
    assert_eq!(rules.len(), 1);
}

#[test]
fn test_check_compliance_passes() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);

    let _ = client.add_rule(
        &admin,
        &RuleKind::MaxValue(symbol_short!("amount"), 1_000),
        &symbol_short!("max_amt"),
        &5u32,
    );

    let subject_id: u128 = 42;
    let mut attrs: Vec<(Symbol, i128)> = Vec::new(&env);
    attrs.push_back((symbol_short!("amount"), 500));

    let report = client.check_compliance(&subject_id, &attrs);
    assert!(report.passed);
    assert_eq!(report.violations.len(), 0);

    let stored = client.get_last_report(&subject_id).unwrap();
    assert!(stored.passed);
}

#[test]
fn test_check_compliance_violates() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);

    let rule_id = client.add_rule(
        &admin,
        &RuleKind::MaxValue(symbol_short!("amount"), 1_000),
        &symbol_short!("max_amt"),
        &9u32,
    );

    let subject_id: u128 = 99;
    let mut attrs: Vec<(Symbol, i128)> = Vec::new(&env);
    attrs.push_back((symbol_short!("amount"), 2_000));

    let report = client.check_compliance(&subject_id, &attrs);
    assert!(!report.passed);
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations.get(0).unwrap().rule_id, rule_id);
}

#[test]
fn test_required_flag_violation() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);

    let _ = client.add_rule(
        &admin,
        &RuleKind::RequiredFlag(symbol_short!("kyc")),
        &symbol_short!("kyc_flag"),
        &7u32,
    );

    let subject_id: u128 = 7;
    let attrs: Vec<(Symbol, i128)> = Vec::new(&env);

    let report = client.check_compliance(&subject_id, &attrs);
    assert!(!report.passed);
    assert_eq!(report.violations.len(), 1);
}
