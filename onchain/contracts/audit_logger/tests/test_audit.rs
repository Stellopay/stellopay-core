#![cfg(test)]

use audit_logger::{AuditLoggerContract, AuditLoggerContractClient, MAX_PAGE_SIZE};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Symbol,
};
use std::collections::HashSet;

use audit_logger::{AuditLoggerContract, AuditLoggerContractClient, MAX_PAGE_SIZE};

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

    // Fetch page starting at offset=1, limit=3 → entries 2,3,4.
    let page = client.get_logs(&1u32, &3u32);
    assert_eq!(page.entries.len(), 3);
    assert_eq!(page.entries.get(0).unwrap().id, 2u64);
    assert_eq!(page.entries.get(1).unwrap().id, 3u64);
    assert_eq!(page.entries.get(2).unwrap().id, 4u64);
    // next_cursor points to offset=4 (one entry remains: id=5).
    assert_eq!(page.next_cursor, Some(4u32));

    // Fetch the remaining page.
    let last_page = client.get_logs(&4u32, &3u32);
    assert_eq!(last_page.entries.len(), 1);
    assert_eq!(last_page.entries.get(0).unwrap().id, 5u64);
    assert_eq!(last_page.next_cursor, None);
}

/// A caller supplying limit > MAX_PAGE_SIZE must receive at most MAX_PAGE_SIZE
/// entries; no error is raised (silent clamp).
#[test]
fn test_get_logs_oversized_limit_clamped() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, AuditLoggerContract);
    let client = AuditLoggerContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    // Unlimited retention so all appended entries are visible.
    client.initialize(&owner, &0u32);

    let actor = Address::generate(&env);
    // Append MAX_PAGE_SIZE + 10 entries so there is enough data to expose an
    // uncapped loop.
    let total = MAX_PAGE_SIZE + 10;
    for i in 0..total {
        let label = format!("e{}", i);
        let action = Symbol::new(&env, label.as_str());
        client.append_log(&actor, &action, &None, &None);
        env.ledger().with_mut(|li| li.timestamp += 1);
    }

    // Request far more than MAX_PAGE_SIZE.
    let page = client.get_logs(&0u32, &u32::MAX);
    // Result must not exceed MAX_PAGE_SIZE regardless of the requested limit.
    assert!(page.entries.len() <= MAX_PAGE_SIZE);
    // A next_cursor must be present because we did not return all entries.
    assert!(page.next_cursor.is_some());
}

/// get_latest_logs must clamp the caller-supplied limit to MAX_PAGE_SIZE
/// regardless of how many entries are retained.
#[test]
fn test_get_latest_logs_oversized_limit_clamped() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, AuditLoggerContract);
    let client = AuditLoggerContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    client.initialize(&owner, &0u32); // unlimited retention

    let actor = Address::generate(&env);
    // Append MAX_PAGE_SIZE + 20 entries.
    let total = MAX_PAGE_SIZE + 20;
    for i in 0..total {
        let label = format!("e{}", i);
        let action = Symbol::new(&env, label.as_str());
        client.append_log(&actor, &action, &None, &None);
        env.ledger().with_mut(|li| li.timestamp += 1);
    }

    // Request far more than MAX_PAGE_SIZE.
    let latest = client.get_latest_logs(&u32::MAX);
    assert!(
        latest.len() <= MAX_PAGE_SIZE as usize,
        "get_latest_logs must clamp to MAX_PAGE_SIZE"
    );
}

/// Verifies that get_latest_logs returns the most recent entries up to the
/// clamped limit when log_count exceeds MAX_PAGE_SIZE.
#[test]
fn test_get_latest_logs_clamped_returns_newest() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, AuditLoggerContract);
    let client = AuditLoggerContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    client.initialize(&owner, &0u32);

    let actor = Address::generate(&env);
    let total = MAX_PAGE_SIZE + 5;
    for i in 0..total {
        let label = format!("e{}", i);
        let action = Symbol::new(&env, label.as_str());
        client.append_log(&actor, &action, &None, &Some(i as i128));
        env.ledger().with_mut(|li| li.timestamp += 1);
    }

    // Requesting u32::MAX should return exactly MAX_PAGE_SIZE entries (the newest).
    let latest = client.get_latest_logs(&u32::MAX);
    assert_eq!(latest.len() as u32, MAX_PAGE_SIZE);
    // The last entry should have the highest id.
    let last = latest.get(latest.len() - 1).unwrap();
    assert_eq!(last.id, total as u64);
}

/// Walk through all pages of get_logs using next_cursor and verify every entry
/// is returned exactly once with no gaps or duplicates.
#[test]
fn test_get_logs_full_pagination_walk() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, AuditLoggerContract);
    let client = AuditLoggerContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    client.initialize(&owner, &0u32); // unlimited retention

    let actor = Address::generate(&env);
    let total_entries = 7u64;
    for i in 0..total_entries {
        let label = format!("walk_{}", i);
        let action = Symbol::new(&env, label.as_str());
        client.append_log(&actor, &action, &None, &Some(i as i128));
        env.ledger().with_mut(|li| li.timestamp += 1);
    }

    // Page size of 3 → should take 3 pages (3 + 3 + 1).
    let page_size = 3u32;
    let mut offset = 0u32;
    let mut all_ids: soroban_sdk::Vec<u64> = soroban_sdk::Vec::new(&env);
    let mut pages = 0u32;

    loop {
        let page = client.get_logs(&offset, &page_size);
        for entry in page.entries.iter() {
            all_ids.push_back(entry.id);
        }
        pages += 1;
        match page.next_cursor {
            Some(next) => offset = next,
            None => break,
        }
    }

    assert_eq!(all_ids.len() as u64, total_entries);
    assert_eq!(pages, 3);
    // IDs should be sequential 1..=7
    for i in 0..all_ids.len() {
        assert_eq!(all_ids.get(i).unwrap(), (i + 1) as u64);
    }
}

/// Default page size: when caller requests limit=1 and there are many entries,
/// verify the max-page-size cap still applies (limit=1 is under cap, so 1 entry).
#[test]
fn test_get_logs_default_small_limit() {
    let (env, _owner, client) = setup();

    let actor = Address::generate(&env);
    for i in 0..5u64 {
        let label = format!("dflt_{}", i);
        let action = Symbol::new(&env, label.as_str());
        client.append_log(&actor, &action, &None, &None);
        env.ledger().with_mut(|li| li.timestamp += 1);
    }

    // limit=1 returns exactly 1 entry per page
    let page1 = client.get_logs(&0u32, &1u32);
    assert_eq!(page1.entries.len(), 1);
    assert_eq!(page1.next_cursor, Some(1));

    let page2 = client.get_logs(&1u32, &1u32);
    assert_eq!(page2.entries.len(), 1);
    assert_eq!(page2.next_cursor, Some(2));
}

/// Over-max page size: requesting limit > MAX_PAGE_SIZE is clamped.
#[test]
fn test_get_logs_over_max_page_size_clamped() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, AuditLoggerContract);
    let client = AuditLoggerContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    client.initialize(&owner, &0u32);

    let actor = Address::generate(&env);
    // Append exactly MAX_PAGE_SIZE entries.
    for i in 0..MAX_PAGE_SIZE {
        let label = format!("om{}", i);
        let action = Symbol::new(&env, label.as_str());
        client.append_log(&actor, &action, &None, &None);
        env.ledger().with_mut(|li| li.timestamp += 1);
    }

    // Request limit = MAX_PAGE_SIZE + 1 → should return exactly MAX_PAGE_SIZE.
    let page = client.get_logs(&0u32, &(MAX_PAGE_SIZE + 1));
    assert_eq!(page.entries.len() as u32, MAX_PAGE_SIZE);
    assert_eq!(page.next_cursor, None);
}

/// When retention has advanced first_id beyond some stored entry ids, those
/// orphaned entries must be skipped silently. The returned page should still
/// contain every retrievable entry in the window, and next_cursor allows
/// resumption.
#[test]
fn test_get_logs_skips_retention_orphans() {
    let (env, owner, client) = setup();

    // Set retention to 5.
    client.set_retention_limit(&owner, &5u32);

    let actor = Address::generate(&env);

    // Append 8 logs. With retention=5, logs 1–3 are evicted logically
    // (first_id advances to 4; log_count stays at 5).
    for i in 0..8u64 {
        let label = format!("ev_{}", i);
        let action = Symbol::new(&env, label.as_str());
        client.append_log(&actor, &action, &None, &Some(i as i128));
        env.ledger().with_mut(|li| li.timestamp += 1);
    }

    assert_eq!(client.get_log_count(), 5u64);

    // Fetch full retained window from offset=0.
    let page = client.get_logs(&0u32, &10u32);
    // All 5 retained entries should be present; orphans are skipped not counted.
    assert_eq!(page.entries.len(), 5);
    // Ids 4..=8 are retained.
    assert_eq!(page.entries.get(0).unwrap().id, 4u64);
    assert_eq!(page.entries.get(4).unwrap().id, 8u64);
    // No more pages.
    assert_eq!(page.next_cursor, None);
}

/// Verifies that get_latest_logs returns entries in strictly increasing append
/// order with no gaps when interleaved with append_log calls. This test confirms
/// the core invariant that reads reflect exactly the entries appended so far,
/// in order, with no skipped or duplicated entries.
#[test]
fn test_interleaved_append_and_get_latest_logs_maintains_order() {
    let (env, owner, client) = setup();

    // Set unlimited retention to ensure all entries remain visible.
    client.set_retention_limit(&owner, &0u32);

    let actor = Address::generate(&env);
    let mut expected_ids: Vec<u64> = Vec::new();

    // Interleave 10 append operations with reads after each append.
    for i in 1..=10u64 {
        let label = format!("action_{}", i);
        let action = Symbol::new(&env, label.as_str());
        
        // Append a new log entry.
        let id = client.append_log(&actor, &action, &None, &Some(i as i128));
        assert_eq!(id, i, "append_log should return sequential ID {}", i);
        
        expected_ids.push(i);
        env.ledger().with_mut(|li| li.timestamp += 1);

        // Read all latest logs after this append.
        let latest = client.get_latest_logs(&10u32);
        
        // Verify the count matches expected.
        assert_eq!(
            latest.len(),
            expected_ids.len() as u32,
            "After {} appends, get_latest_logs should return {} entries",
            i,
            expected_ids.len()
        );

        // Verify each entry is present and in strictly increasing order.
        for (idx, entry) in latest.iter().enumerate() {
            let expected_id = expected_ids[idx];
            assert_eq!(
                entry.id,
                expected_id,
                "Entry at index {} should have ID {} after {} appends",
                idx,
                expected_id,
                i
            );
            assert_eq!(
                entry.action,
                Symbol::new(&env, format!("action_{}", expected_id).as_str()),
                "Entry action should match appended action"
            );
        }

        // Verify no gaps: IDs should be consecutive from 1 to i.
        for (idx, entry) in latest.iter().enumerate() {
            assert_eq!(
                entry.id,
                (idx + 1) as u64,
                "IDs should be consecutive starting from 1, got {} at index {}",
                entry.id,
                idx
            );
        }
    }

    // Final verification: get_latest_logs with limit=10 should return all 10 entries.
    let final_latest = client.get_latest_logs(&10u32);
    assert_eq!(final_latest.len(), 10, "Should return all 10 entries");
    
    // Verify strict ordering one more time.
    for (idx, entry) in final_latest.iter().enumerate() {
        assert_eq!(
            entry.id,
            (idx + 1) as u64,
            "Final verification: IDs should be consecutive"
        );
    }
}

/// Verifies that get_log and get_latest_logs return consistent results when
/// called interleaved with append_log. No entry should be skipped or duplicated
/// across different read methods.
#[test]
fn test_interleaved_append_and_read_consistency() {
    let (env, owner, client) = setup();

    // Set unlimited retention.
    client.set_retention_limit(&owner, &0u32);

    let actor = Address::generate(&env);
    let mut appended_ids: Vec<u64> = Vec::new();

    // Append 5 entries, reading with both methods after each.
    for i in 1..=5u64 {
        let label = format!("event_{}", i);
        let action = Symbol::new(&env, label.as_str());
        
        let id = client.append_log(&actor, &action, &None, &Some(i as i128));
        appended_ids.push(id);
        env.ledger().with_mut(|li| li.timestamp += 1);

        // Verify get_log returns the just-appended entry.
        let single_entry = client.get_log(&id).unwrap();
        assert_eq!(single_entry.id, id);
        assert_eq!(single_entry.action, action);

        // Verify get_latest_logs includes all entries so far.
        let latest = client.get_latest_logs(&10u32);
        assert_eq!(latest.len(), i as u32);

        // Verify each appended ID is present in get_latest_logs.
        for (idx, entry) in latest.iter().enumerate() {
            assert_eq!(
                entry.id,
                appended_ids[idx],
                "get_latest_logs entry {} should match appended ID {}",
                idx,
                appended_ids[idx]
            );
        }

        // Verify no duplicates in get_latest_logs.
        let mut seen_ids = HashSet::new();
        for entry in latest.iter() {
            assert!(
                seen_ids.insert(entry.id),
                "Duplicate ID {} found in get_latest_logs after {} appends",
                entry.id,
                i
            );
        }
    }

    // Verify all individual get_log calls succeed.
    for id in &appended_ids {
        assert!(
            client.get_log(id).is_some(),
            "get_log should return entry for ID {}",
            id
        );
    }

    // Verify get_log fails for non-existent ID.
    assert!(client.get_log(&999u64).is_none());
}
