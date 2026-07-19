#![cfg(test)]

use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn setup() -> (Env, PayrollContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let payroll_id = env.register(PayrollContract, ());
    let payroll_client = PayrollContractClient::new(&env, &payroll_id);
    let owner = Address::generate(&env);
    payroll_client.initialize(&owner);

    (env, payroll_client, owner)
}

#[test]
fn only_returns_entries_for_the_requested_employer() {
    let (env, payroll_client, _owner) = setup();
    let employer_a = Address::generate(&env);
    let employer_b = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_a = payroll_client.create_payroll_agreement(&employer_a, &token, &3600);
    let agreement_b = payroll_client.create_payroll_agreement(&employer_b, &token, &3600);
    payroll_client.cancel_agreement(&agreement_a);
    payroll_client.cancel_agreement(&agreement_b);

    // 4 entries total: A created, B created, A cancelled, B cancelled.
    assert_eq!(payroll_client.get_audit_entry_count(), 4);

    let page_a = payroll_client.get_audit_entries_by_employer(&employer_a, &1, &10);
    assert_eq!(page_a.entries.len(), 2);
    for entry in page_a.entries.iter() {
        assert_eq!(entry.agreement_id, agreement_a);
    }
    assert_eq!(page_a.next_start_id, None);

    let page_b = payroll_client.get_audit_entries_by_employer(&employer_b, &1, &10);
    assert_eq!(page_b.entries.len(), 2);
    for entry in page_b.entries.iter() {
        assert_eq!(entry.agreement_id, agreement_b);
    }
    assert_eq!(page_b.next_start_id, None);
}

#[test]
fn excludes_contract_level_entries_with_no_agreement() {
    let (env, payroll_client, _owner) = setup();
    let employer = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let token = Address::generate(&env);

    // `set_arbiter` is called by `employer` and records an ArbiterSet entry
    // with the sentinel agreement_id of 0, which has no employer to scope by.
    payroll_client.set_arbiter(&employer, &arbiter);
    let agreement_id = payroll_client.create_payroll_agreement(&employer, &token, &3600);

    assert_eq!(payroll_client.get_audit_entry_count(), 2);

    let page = payroll_client.get_audit_entries_by_employer(&employer, &1, &10);
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries.get(0).unwrap().agreement_id, agreement_id);
    assert_eq!(page.next_start_id, None);
}

#[test]
fn unfiltered_query_behavior_is_unchanged() {
    let (env, payroll_client, _owner) = setup();
    let employer_a = Address::generate(&env);
    let employer_b = Address::generate(&env);
    let token = Address::generate(&env);

    payroll_client.create_payroll_agreement(&employer_a, &token, &3600);
    payroll_client.create_payroll_agreement(&employer_b, &token, &3600);

    // get_audit_entry_count / get_audit_entry keep returning every entry
    // regardless of employer, exactly as before this change.
    assert_eq!(payroll_client.get_audit_entry_count(), 2);
    assert!(payroll_client.get_audit_entry(&1).is_some());
    assert!(payroll_client.get_audit_entry(&2).is_some());
}

#[test]
fn paginates_with_limit_and_resumes_from_next_start_id() {
    let (env, payroll_client, _owner) = setup();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);

    for _ in 0..5 {
        let agreement_id = payroll_client.create_payroll_agreement(&employer, &token, &3600);
        payroll_client.cancel_agreement(&agreement_id);
    }
    // 10 entries total for this employer (5 created + 5 cancelled).
    assert_eq!(payroll_client.get_audit_entry_count(), 10);

    let page1 = payroll_client.get_audit_entries_by_employer(&employer, &1, &4);
    assert_eq!(page1.entries.len(), 4);
    let resume_id = page1.next_start_id.expect("more entries remain");

    let page2 = payroll_client.get_audit_entries_by_employer(&employer, &resume_id, &4);
    assert_eq!(page2.entries.len(), 4);
    let resume_id2 = page2.next_start_id.expect("more entries remain");

    let page3 = payroll_client.get_audit_entries_by_employer(&employer, &resume_id2, &4);
    assert_eq!(page3.entries.len(), 2);
    assert_eq!(page3.next_start_id, None);
}

#[test]
fn returns_no_entries_for_an_employer_with_no_agreements() {
    let (env, payroll_client, _owner) = setup();
    let employer = Address::generate(&env);
    let uninvolved_employer = Address::generate(&env);
    let token = Address::generate(&env);

    payroll_client.create_payroll_agreement(&employer, &token, &3600);

    let page = payroll_client.get_audit_entries_by_employer(&uninvolved_employer, &1, &10);
    assert!(page.entries.is_empty());
    assert_eq!(page.next_start_id, None);
}
