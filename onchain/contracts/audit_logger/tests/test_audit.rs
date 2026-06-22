#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Symbol,
};

use audit_logger::{AuditLoggerContract, AuditLoggerContractClient};

fn setup() -> (Env, Address, AuditLoggerContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, AuditLoggerContract);
    let client = AuditLoggerContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner, &10u32); // default retention 10

    (env, owner, client)
}

#[test]
fn test_append_and_get_log() {
    let (env, _owner, client) = setup();

    let actor = Address::generate(&env);
    let subject = Address::generate(&env);

    let action = Symbol::new(&env, "create_agreement");

    let id = client.append_log(&actor, &action, &Some(subject.clone()), &Some(1_000i128));

    assert_eq!(id, 1u64);

    let log = client.get_log(&id).unwrap();
    assert_eq!(log.id, 1u64);
    assert_eq!(log.actor, actor);
    assert_eq!(log.subject, Some(subject));
    assert_eq!(log.amount, Some(1_000i128));
}

#[test]
fn test_retention_limit_enforced() {
    let (env, owner, client) = setup();

    // Reduce retention to 3 entries.
    client.set_retention_limit(&owner, &3u32);

    let actor = Address::generate(&env);

    // Append 5 logs; with retention=3 we should keep only the last 3.
    for i in 0..5u64 {
        let label = format!("event_{}", i);
        let action = Symbol::new(&env, label.as_str());
        client.append_log(&actor, &action, &None, &None);
        env.ledger().with_mut(|li| li.timestamp += 1);
    }

    // Total retained count must be 3.
    assert_eq!(client.get_log_count(), 3u64);

    // First two IDs are logically outside retention window.
    assert!(client.get_log(&1u64).is_none());
    assert!(client.get_log(&2u64).is_none());

    // Last three IDs are still queryable.
    assert!(client.get_log(&3u64).is_some());
    assert!(client.get_log(&4u64).is_some());
    assert!(client.get_log(&5u64).is_some());

    // Latest logs API returns newest entries.
    let latest = client.get_latest_logs(&3u32);
    assert_eq!(latest.len(), 3);
    assert_eq!(latest.get(0).unwrap().id, 3u64);
    assert_eq!(latest.get(2).unwrap().id, 5u64);
}

#[test]
fn test_get_logs_pagination() {
    let (env, _owner, client) = setup();

    let actor = Address::generate(&env);

    for i in 0..5u64 {
        let label = format!("evt_{}", i);
        let action = Symbol::new(&env, label.as_str());
        client.append_log(&actor, &action, &None, &Some(i as i128));
        env.ledger().with_mut(|li| li.timestamp += 1);
    }

    // Fetch logs [1,3]
    let page = client.get_logs(&1u32, &3u32);
    assert_eq!(page.len(), 3);
    assert_eq!(page.get(0).unwrap().id, 2u64);
    assert_eq!(page.get(1).unwrap().id, 3u64);
    assert_eq!(page.get(2).unwrap().id, 4u64);
#[test]
fn test_get_logs_limit_clamped() {
    let env = Env::default();
    let owner = Address::generate(&env);

    let contract_id = env.register(AuditLoggerContract, ());
    let client = AuditLoggerContractClient::new(&env, &contract_id);

    client.initialize(&owner, &0);

    // Append 5 logs
    let actor = Address::generate(&env);
    for i in 0..5u64 {
        client.append_log_(&actor, &symbol_short!("test"), &None, &None);
    }

    // Request more than MAX_PAGE_SIZE - should be clamped
    let result = client.get_logs(&0, &(MAX_PAGE_SIZE + 1));
    assert_eq!(result.len(), 5);
}

#[test]
fn test_get_logs_beyond_max_page_size() {
    let env = Env::default();
    let owner = Address::generate(&env);

    let contract_id = env.register(AuditLoggerContract, ());
    let client = AuditLoggerContractClient::new(&env, &contract_id);

    client.initialize(&owner, &0);

    // Append more logs than MAX_PAGE_SIZE
    let actor = Address::generate(&env);
    for i in 0..200u64 {
        client.append_log_(&actor, &symbol_short!("test"), &None, &None);
    }

    // Request all - should be clamped to MAX_PAGE_SIZE
    let result = client.get_logs(&0, &200);
    assert_eq!(result.len(), MAX_PAGE_SIZE as usize);
}

#[test]
fn test_get_latest_logs_clamped() {
    let env = Env::default();
    let owner = Address::generate(&env);

    let contract_id = env.register(AuditLoggerContract, ());
    let client = AuditLoggerContractClient::new(&env, &contract_id);

    client.initialize(&owner, &0);

    let actor = Address::generate(&env);
    for i in 0..200u64 {
        client.append_log_(&actor, &symbol_short!("test"), &None, &None);
    }

    let result = client.get_latest_logs(&200);
    assert_eq!(result.len(), MAX_PAGE_SIZE as usize);
}

}
