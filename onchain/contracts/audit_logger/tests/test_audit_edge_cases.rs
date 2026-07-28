#![cfg(test)]

use audit_logger::{AuditError, AuditLoggerContract, AuditLoggerContractClient, MAX_PAGE_SIZE};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Symbol,
};

fn setup() -> (Env, Address, AuditLoggerContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, AuditLoggerContract);
    let client = AuditLoggerContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner, &10u32);

    (env, owner, client)
}

// ==================== Initialization ====================

#[test]
fn initialize_sets_defaults() {
    let (env, _owner, client) = setup();
    assert_eq!(client.get_log_count(), 0u64);
    assert_eq!(client.get_retention_limit(), 10u32);
}

#[test]
fn initialize_with_zero_retention_unlimited() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, AuditLoggerContract);
    let client = AuditLoggerContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);

    client.initialize(&owner, &0u32);
    assert_eq!(client.get_retention_limit(), 0u32);
}

// ==================== Append Log ====================

#[test]
fn append_log_returns_monotonic_ids() {
    let (env, _owner, client) = setup();
    let actor = Address::generate(&env);
    let action = Symbol::new(&env, "test");

    let id1 = client.append_log(&actor, &action, &None, &None);
    let id2 = client.append_log(&actor, &action, &None, &None);
    let id3 = client.append_log(&actor, &action, &None, &None);

    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert_eq!(id3, 3);
}

#[test]
fn append_log_increments_count() {
    let (env, _owner, client) = setup();
    let actor = Address::generate(&env);
    let action = Symbol::new(&env, "test");

    assert_eq!(client.get_log_count(), 0);

    client.append_log(&actor, &action, &None, &None);
    assert_eq!(client.get_log_count(), 1);

    client.append_log(&actor, &action, &None, &None);
    assert_eq!(client.get_log_count(), 2);
}

#[test]
fn append_log_records_all_fields() {
    let (env, _owner, client) = setup();
    // Advance the ledger so the recorded timestamp is non-zero; a fresh
    // `Env::default()` starts at timestamp 0.
    env.ledger().with_mut(|li| li.timestamp += 1);
    let actor = Address::generate(&env);
    let subject = Address::generate(&env);
    let action = Symbol::new(&env, "pay_salary");

    let id = client.append_log(&actor, &action, &Some(subject.clone()), &Some(5_000i128));
    let log = client.get_log(&id).unwrap();

    assert_eq!(log.id, id);
    assert_eq!(log.actor, actor);
    assert_eq!(log.action, Symbol::new(&env, "pay_salary"));
    assert_eq!(log.subject, Some(subject));
    assert_eq!(log.amount, Some(5_000i128));
    assert!(log.timestamp > 0);
}

#[test]
fn append_log_with_negative_amount() {
    let (env, _owner, client) = setup();
    let actor = Address::generate(&env);
    let action = Symbol::new(&env, "refund");

    let id = client.append_log(&actor, &action, &None, &Some(-500i128));
    let log = client.get_log(&id).unwrap();
    assert_eq!(log.amount, Some(-500i128));
}

// ==================== Retention ====================

#[test]
fn retention_zero_means_unlimited() {
    let (env, owner, client) = setup();
    client.set_retention_limit(&owner, &0u32);

    let actor = Address::generate(&env);
    let action = Symbol::new(&env, "evt");

    for _ in 0..20 {
        client.append_log(&actor, &action, &None, &None);
        env.ledger().with_mut(|li| li.timestamp += 1);
    }

    assert_eq!(client.get_log_count(), 20);
}

#[test]
fn retention_one_keeps_only_latest() {
    let (env, owner, client) = setup();
    client.set_retention_limit(&owner, &1u32);

    let actor = Address::generate(&env);

    client.append_log(&actor, &Symbol::new(&env, "first"), &None, &None);
    env.ledger().with_mut(|li| li.timestamp += 1);
    client.append_log(&actor, &Symbol::new(&env, "second"), &None, &None);
    env.ledger().with_mut(|li| li.timestamp += 1);
    client.append_log(&actor, &Symbol::new(&env, "third"), &None, &None);

    assert_eq!(client.get_log_count(), 1);
    assert!(client.get_log(&1u64).is_none());
    assert!(client.get_log(&2u64).is_none());

    let log = client.get_log(&3u64).unwrap();
    assert_eq!(log.action, Symbol::new(&env, "third"));
}

#[test]
fn retention_boundary_exact() {
    let (env, owner, client) = setup();
    client.set_retention_limit(&owner, &3u32);

    let actor = Address::generate(&env);
    let action = Symbol::new(&env, "evt");

    // Exactly 3 entries
    for _ in 0..3 {
        client.append_log(&actor, &action, &None, &None);
        env.ledger().with_mut(|li| li.timestamp += 1);
    }

    assert_eq!(client.get_log_count(), 3);
    assert!(client.get_log(&1u64).is_some());
    assert!(client.get_log(&3u64).is_some());
}

// ==================== Get Log ====================

#[test]
fn get_log_nonexistent_returns_none() {
    let (_env, _owner, client) = setup();
    assert!(client.get_log(&999u64).is_none());
}

#[test]
fn get_log_before_first_returns_none() {
    let (env, owner, client) = setup();
    client.set_retention_limit(&owner, &2u32);

    let actor = Address::generate(&env);
    client.append_log(&actor, &Symbol::new(&env, "a"), &None, &None);
    env.ledger().with_mut(|li| li.timestamp += 1);
    client.append_log(&actor, &Symbol::new(&env, "b"), &None, &None);
    env.ledger().with_mut(|li| li.timestamp += 1);
    client.append_log(&actor, &Symbol::new(&env, "c"), &None, &None);

    assert!(client.get_log(&0u64).is_none());
}

// ==================== Pagination ====================

#[test]
fn get_logs_empty_result() {
    let (env, _owner, client) = setup();
    let result = client.try_get_logs(&0u32, &5u32).unwrap().unwrap();
    assert_eq!(result.entries.len(), 0);
}

#[test]
fn get_logs_limit_zero_returns_error() {
    let (_env, _owner, client) = setup();
    let result = client.try_get_logs(&0u32, &0u32);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().unwrap(), AuditError::InvalidArguments);
}

#[test]
fn get_logs_offset_beyond_count() {
    let (env, _owner, client) = setup();
    let actor = Address::generate(&env);
    client.append_log(&actor, &Symbol::new(&env, "a"), &None, &None);

    let result = client.try_get_logs(&10u32, &5u32).unwrap().unwrap();
    assert_eq!(result.entries.len(), 0);
}

#[test]
fn get_logs_partial_page() {
    let (env, _owner, client) = setup();
    let actor = Address::generate(&env);

    for i in 0..3u64 {
        client.append_log(&actor, &Symbol::new(&env, "evt"), &None, &Some(i as i128));
        env.ledger().with_mut(|li| li.timestamp += 1);
    }

    let page = client.try_get_logs(&1u32, &10u32).unwrap().unwrap();
    assert_eq!(page.entries.len(), 2); // Only 2 entries after offset 1
    assert_eq!(page.entries.get(0).unwrap().id, 2);
}

// ==================== Latest Logs ====================

#[test]
fn get_latest_logs_returns_newest_first() {
    let (env, _owner, client) = setup();
    let actor = Address::generate(&env);

    for i in 0..5u64 {
        client.append_log(&actor, &Symbol::new(&env, "evt"), &None, &Some(i as i128));
        env.ledger().with_mut(|li| li.timestamp += 1);
    }

    let latest = client.try_get_latest_logs(&3u32).unwrap().unwrap();
    assert_eq!(latest.len(), 3);
    assert_eq!(latest.get(0).unwrap().id, 3); // oldest of the 3
    assert_eq!(latest.get(2).unwrap().id, 5); // newest
}

#[test]
fn get_latest_logs_limit_zero_returns_error() {
    let (_env, _owner, client) = setup();
    let result = client.try_get_latest_logs(&0u32);
    assert!(result.is_err());
}

#[test]
fn get_latest_logs_empty_collection() {
    let (_env, _owner, client) = setup();
    let result = client.try_get_latest_logs(&5u32).unwrap().unwrap();
    assert_eq!(result.len(), 0);
}

// ==================== Set Retention Limit ====================

#[test]
fn only_owner_can_set_retention() {
    let (env, _owner, client) = setup();
    let non_owner = Address::generate(&env);

    let result = client.try_set_retention_limit(&non_owner, &5u32);
    assert!(result.is_err());
}

#[test]
fn owner_can_update_retention() {
    let (env, owner, client) = setup();

    client.set_retention_limit(&owner, &20u32);
    assert_eq!(client.get_retention_limit(), 20u32);
}

#[test]
fn retention_update_affects_new_logs() {
    let (env, owner, client) = setup();
    let actor = Address::generate(&env);

    // Fill up to default retention of 10
    for _ in 0..10 {
        client.append_log(&actor, &Symbol::new(&env, "evt"), &None, &None);
        env.ledger().with_mut(|li| li.timestamp += 1);
    }
    assert_eq!(client.get_log_count(), 10);

    // Reduce retention to 5
    client.set_retention_limit(&owner, &5u32);
    client.append_log(&actor, &Symbol::new(&env, "new"), &None, &None);

    assert_eq!(client.get_log_count(), 5);
}

// ==================== Prune-on-Lower Retention Limit ====================

// Ensures lowering retention limit permanently discards oldest entries
// rather than just hiding them.
#[test]
fn test_lowering_retention_limit_prunes_oldest_entries() {
    let (env, owner, client) = setup();
    let actor = Address::generate(&env);

    // Append 10 entries with distinct actions so we can identify them.
    for i in 0..10u64 {
        let label = format!("entry_{}", i);
        let action = Symbol::new(&env, label.as_str());
        client.append_log(&actor, &action, &None, &Some(i as i128));
        env.ledger().with_mut(|li| li.timestamp += 1);
    }
    assert_eq!(client.get_log_count(), 10);

    // Lower retention limit to 4 — must immediately prune the 6 oldest entries.
    client.set_retention_limit(&owner, &4u32);

    // Confirm count reflects the new lower window.
    assert_eq!(client.get_log_count(), 4);

    // Verify get_logs returns exactly the 4 newest entries (ids 7,8,9,10).
    let page = client.get_logs(&0u32, &10u32);
    assert_eq!(page.entries.len(), 4);
    assert_eq!(page.entries.get(0).unwrap().id, 7u64);
    assert_eq!(page.entries.get(1).unwrap().id, 8u64);
    assert_eq!(page.entries.get(2).unwrap().id, 9u64);
    assert_eq!(page.entries.get(3).unwrap().id, 10u64);
    assert_eq!(page.next_cursor, None);

    // Verify the pruned (oldest) entries are absent from get_logs.
    let pruned_ids = [1u64, 2u64, 3u64, 4u64, 5u64, 6u64];
    for &pruned_id in &pruned_ids {
        assert!(
            client.get_log(&pruned_id).is_none(),
            "Pruned entry {} should not be retrievable",
            pruned_id
        );
    }

    // Verify the surviving entries are still retrievable individually.
    for id in 7u64..=10u64 {
        let log = client.get_log(&id).unwrap();
        assert_eq!(log.id, id);
        let expected_label = format!("entry_{}", id - 1);
        assert_eq!(log.action, Symbol::new(&env, expected_label.as_str()));
    }
}

// Ensures raising the retention limit after a prune does not bring back
// entries that were permanently discarded.
#[test]
fn test_raising_limit_after_prune_does_not_resurrect_entries() {
    let (env, owner, client) = setup();
    let actor = Address::generate(&env);

    // Reuse the same setup as the lowering test: append 10, then lower to 4.
    for i in 0..10u64 {
        let label = format!("entry_{}", i);
        let action = Symbol::new(&env, label.as_str());
        client.append_log(&actor, &action, &None, &Some(i as i128));
        env.ledger().with_mut(|li| li.timestamp += 1);
    }
    assert_eq!(client.get_log_count(), 10);

    // Lower to 4 — prune oldest 6.
    client.set_retention_limit(&owner, &4u32);
    assert_eq!(client.get_log_count(), 4);

    // Now raise the limit back above the current count (e.g. to 10).
    client.set_retention_limit(&owner, &10u32);

    // Count must remain unchanged — pruned entries are permanently gone.
    assert_eq!(client.get_log_count(), 4);

    // Verify the same 4 survivors are present and in the same order.
    let page = client.get_logs(&0u32, &10u32);
    assert_eq!(page.entries.len(), 4);
    assert_eq!(page.entries.get(0).unwrap().id, 7u64);
    assert_eq!(page.entries.get(1).unwrap().id, 8u64);
    assert_eq!(page.entries.get(2).unwrap().id, 9u64);
    assert_eq!(page.entries.get(3).unwrap().id, 10u64);

    // Old entries must not be resurrected.
    for id in 1u64..=6u64 {
        assert!(
            client.get_log(&id).is_none(),
            "Pruned entry {} should remain gone after raising limit",
            id
        );
    }
}

// Ensures setting retention limit to 0 (unlimited) does not prune anything;
// 0 means unlimited retention per the contract's convention.
#[test]
fn test_set_retention_limit_zero_does_not_prune() {
    let (env, owner, client) = setup();
    let actor = Address::generate(&env);

    // Append 5 entries.
    for i in 0..5u64 {
        let label = format!("evt_{}", i);
        let action = Symbol::new(&env, label.as_str());
        client.append_log(&actor, &action, &None, &None);
        env.ledger().with_mut(|li| li.timestamp += 1);
    }
    assert_eq!(client.get_log_count(), 5);

    // Set limit to 0 (unlimited). No pruning should occur.
    client.set_retention_limit(&owner, &0u32);
    assert_eq!(client.get_log_count(), 5);
    assert_eq!(client.get_retention_limit(), 0u32);

    // All 5 entries must still be retrievable.
    for id in 1u64..=5u64 {
        assert!(
            client.get_log(&id).is_some(),
            "Entry {} should still exist after setting limit to 0",
            id
        );
    }
}

// Ensures setting retention limit equal to the current log count is a no-op.
#[test]
fn test_set_retention_limit_equal_to_count_is_noop() {
    let (env, owner, client) = setup();
    let actor = Address::generate(&env);

    // Append 5 entries.
    for i in 0..5u64 {
        let label = format!("evt_{}", i);
        let action = Symbol::new(&env, label.as_str());
        client.append_log(&actor, &action, &None, &None);
        env.ledger().with_mut(|li| li.timestamp += 1);
    }
    assert_eq!(client.get_log_count(), 5);

    // Set limit equal to current count (5). Must be a no-op.
    client.set_retention_limit(&owner, &5u32);
    assert_eq!(client.get_log_count(), 5);

    // All 5 entries must still be retrievable.
    for id in 1u64..=5u64 {
        assert!(
            client.get_log(&id).is_some(),
            "Entry {} should still exist after setting limit equal to count",
            id
        );
    }
}

// ==================== Tamper Evidence ====================

#[test]
fn log_entries_are_immutable() {
    let (env, _owner, client) = setup();
    let actor = Address::generate(&env);
    let action = Symbol::new(&env, "original");

    let id = client.append_log(&actor, &action, &None, &Some(100i128));

    // Re-read the same entry - values must be identical
    let log1 = client.get_log(&id).unwrap();
    let log2 = client.get_log(&id).unwrap();

    assert_eq!(log1.id, log2.id);
    assert_eq!(log1.timestamp, log2.timestamp);
    assert_eq!(log1.actor, log2.actor);
    assert_eq!(log1.action, log2.action);
    assert_eq!(log1.amount, log2.amount);
}

#[test]
fn timestamps_are_monotonic() {
    let (env, _owner, client) = setup();
    let actor = Address::generate(&env);

    let id1 = client.append_log(&actor, &Symbol::new(&env, "a"), &None, &None);
    env.ledger().with_mut(|li| li.timestamp += 100);
    let id2 = client.append_log(&actor, &Symbol::new(&env, "b"), &None, &None);
    env.ledger().with_mut(|li| li.timestamp += 100);
    let id3 = client.append_log(&actor, &Symbol::new(&env, "c"), &None, &None);

    let log1 = client.get_log(&id1).unwrap();
    let log2 = client.get_log(&id2).unwrap();
    let log3 = client.get_log(&id3).unwrap();

    assert!(log1.timestamp < log2.timestamp);
    assert!(log2.timestamp < log3.timestamp);
}

// ==================== Multiple Actors ====================

#[test]
fn multiple_actors_can_append() {
    let (env, _owner, client) = setup();
    let actor1 = Address::generate(&env);
    let actor2 = Address::generate(&env);

    let id1 = client.append_log(&actor1, &Symbol::new(&env, "a"), &None, &None);
    let id2 = client.append_log(&actor2, &Symbol::new(&env, "b"), &None, &None);

    let log1 = client.get_log(&id1).unwrap();
    let log2 = client.get_log(&id2).unwrap();

    assert_eq!(log1.actor, actor1);
    assert_eq!(log2.actor, actor2);
}

// ==================== get_latest_logs MAX_PAGE_SIZE enforcement ====================

#[test]
fn get_latest_logs_oversized_limit_clamped() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, AuditLoggerContract);
    let client = AuditLoggerContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    client.initialize(&owner, &0u32);

    let actor = Address::generate(&env);
    let total = MAX_PAGE_SIZE + 30;
    for i in 0..total {
        let label = format!("e{}", i);
        let action = Symbol::new(&env, label.as_str());
        client.append_log(&actor, &action, &None, &None);
        env.ledger().with_mut(|li| li.timestamp += 1);
    }

    let result = client.try_get_latest_logs(&u32::MAX).unwrap().unwrap();
    assert!(
        result.len() as u32 <= MAX_PAGE_SIZE,
        "get_latest_logs must clamp oversized limit to MAX_PAGE_SIZE"
    );
}

#[test]
fn get_latest_logs_exact_max_page_size() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, AuditLoggerContract);
    let client = AuditLoggerContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    client.initialize(&owner, &0u32);

    let actor = Address::generate(&env);
    // Append exactly MAX_PAGE_SIZE entries.
    for i in 0..MAX_PAGE_SIZE {
        let label = format!("e{}", i);
        let action = Symbol::new(&env, label.as_str());
        client.append_log(&actor, &action, &None, &None);
        env.ledger().with_mut(|li| li.timestamp += 1);
    }

    let result = client.try_get_latest_logs(&MAX_PAGE_SIZE).unwrap().unwrap();
    assert_eq!(result.len() as u32, MAX_PAGE_SIZE);
}

#[test]
fn get_latest_logs_under_max_returns_all() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, AuditLoggerContract);
    let client = AuditLoggerContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    client.initialize(&owner, &0u32);

    let actor = Address::generate(&env);
    for i in 0..5u64 {
        let label = format!("e{}", i);
        let action = Symbol::new(&env, label.as_str());
        client.append_log(&actor, &action, &None, &None);
        env.ledger().with_mut(|li| li.timestamp += 1);
    }

    // limit=5 is under MAX_PAGE_SIZE, so all 5 are returned.
    let result = client.try_get_latest_logs(&5u32).unwrap().unwrap();
    assert_eq!(result.len(), 5);
    assert_eq!(result.get(0).unwrap().id, 1);
    assert_eq!(result.get(4).unwrap().id, 5);
}
