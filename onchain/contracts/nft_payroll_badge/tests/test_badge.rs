//! Comprehensive test suite for nft_payroll_badge contract.
//!
//! Covers: initialization, minting, admin metadata URI updates, `badges_of`,
//! `badges_of_paged` (empty, single page, multi-page, exact-multiple-of-limit,
//! oversized-limit clamping).

#![cfg(test)]

use nft_payroll_badge::{
    BadgeBurned, MetadataUpdated, NftPayrollBadgeContract, NftPayrollBadgeContractClient,
    MAX_PAGE_SIZE,
};
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, IntoVal, String,
};

// ============================================================================
// Helpers
// ============================================================================

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup(env: &Env) -> (Address, NftPayrollBadgeContractClient<'static>) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, NftPayrollBadgeContract);
    let client = NftPayrollBadgeContractClient::new(env, &contract_id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (owner, client)
}

fn mint_n(
    env: &Env,
    client: &NftPayrollBadgeContractClient,
    owner: &Address,
    recipient: &Address,
    n: u32,
) {
    for _ in 0..n {
        let name = String::from_str(env, "Payroll Badge");
        let metadata_uri = String::from_str(env, "ipfs://payroll-badge");
        client.mint(owner, recipient, &name, &metadata_uri);
    }
}

// ============================================================================
// Initialization tests
// ============================================================================

#[test]
fn test_initialize_sets_owner() {
    let env = create_env();
    let (owner, client) = setup(&env);
    assert_eq!(client.get_owner(), Some(owner));
}

#[test]
#[should_panic(expected = "Contract already initialized")]
fn test_double_initialize_panics() {
    let env = create_env();
    let (owner, client) = setup(&env);
    client.initialize(&owner);
}

// ============================================================================
// Minting tests
// ============================================================================

#[test]
fn test_mint_assigns_sequential_ids() {
    let env = create_env();
    let (owner, client) = setup(&env);
    let recipient = Address::generate(&env);

    let id1 = client.mint(
        &owner,
        &recipient,
        &String::from_str(&env, "First"),
        &String::from_str(&env, "ipfs://first"),
    );
    let id2 = client.mint(
        &owner,
        &recipient,
        &String::from_str(&env, "Second"),
        &String::from_str(&env, "ipfs://second"),
    );
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
}

#[test]
fn test_mint_records_badge_metadata() {
    let env = create_env();
    let (owner, client) = setup(&env);
    let recipient = Address::generate(&env);
    let name = String::from_str(&env, "Q1 2025 Payroll");
    let metadata_uri = String::from_str(&env, "ipfs://q1-2025-payroll");

    let id = client.mint(&owner, &recipient, &name, &metadata_uri);
    let badge = client.get_badge(&id).expect("badge should exist");

    assert_eq!(badge.id, id);
    assert_eq!(badge.owner, recipient);
    assert_eq!(badge.name, name);
    assert_eq!(badge.metadata_uri, metadata_uri);
}

#[test]
#[should_panic(expected = "Only owner can manage badges")]
fn test_non_owner_cannot_mint() {
    let env = create_env();
    let (_owner, client) = setup(&env);
    let attacker = Address::generate(&env);
    let recipient = Address::generate(&env);
    client.mint(
        &attacker,
        &recipient,
        &String::from_str(&env, "Fake"),
        &String::from_str(&env, "ipfs://fake"),
    );
}

#[test]
fn test_admin_can_update_metadata_uri_for_existing_token() {
    let env = create_env();
    let (owner, client) = setup(&env);
    let recipient = Address::generate(&env);
    let old_uri = String::from_str(&env, "ipfs://old-payroll-badge");
    let new_uri = String::from_str(&env, "ipfs://new-payroll-badge");

    let id = client.mint(
        &owner,
        &recipient,
        &String::from_str(&env, "Payroll Badge"),
        &old_uri,
    );

    client.update_metadata_uri(&owner, &id, &new_uri);

    let events = env.events().all();
    let event: MetadataUpdated = events.last().unwrap().2.into_val(&env);
    assert_eq!(event.token_id, id);
    assert_eq!(event.old_uri, old_uri);
    assert_eq!(event.new_uri, new_uri);

    let badge = client.get_badge(&id).expect("badge should exist");
    assert_eq!(badge.metadata_uri, new_uri);
    assert_eq!(badge.owner, recipient);
}

#[test]
#[should_panic(expected = "Only owner can manage badges")]
fn test_non_admin_cannot_update_metadata_uri() {
    let env = create_env();
    let (owner, client) = setup(&env);
    let attacker = Address::generate(&env);
    let recipient = Address::generate(&env);
    let old_uri = String::from_str(&env, "ipfs://original");
    let id = client.mint(
        &owner,
        &recipient,
        &String::from_str(&env, "Payroll Badge"),
        &old_uri,
    );

    client.update_metadata_uri(&attacker, &id, &String::from_str(&env, "ipfs://attack"));
}

// ============================================================================
// badges_of tests
// ============================================================================

#[test]
fn test_badges_of_empty_owner() {
    let env = create_env();
    let (_owner, client) = setup(&env);
    let stranger = Address::generate(&env);
    let result = client.badges_of(&stranger);
    assert_eq!(result.len(), 0);
}

#[test]
fn test_badges_of_returns_all() {
    let env = create_env();
    let (owner, client) = setup(&env);
    let recipient = Address::generate(&env);
    mint_n(&env, &client, &owner, &recipient, 5);

    let ids = client.badges_of(&recipient);
    assert_eq!(ids.len(), 5);
    // IDs should be 1..=5 in order
    for (i, id) in ids.iter().enumerate() {
        assert_eq!(id, (i as u64) + 1);
    }
}

// ============================================================================
// badges_of_paged edge-case tests
// ============================================================================

#[test]
fn test_paged_empty_owner_returns_empty_page() {
    let env = create_env();
    let (_owner, client) = setup(&env);
    let stranger = Address::generate(&env);

    let page = client.badges_of_paged(&stranger, &0, &10);
    assert_eq!(page.items.len(), 0);
    assert_eq!(page.next_cursor, None);
}

#[test]
fn test_paged_single_page_no_cursor() {
    let env = create_env();
    let (owner, client) = setup(&env);
    let recipient = Address::generate(&env);
    mint_n(&env, &client, &owner, &recipient, 3);

    let page = client.badges_of_paged(&recipient, &0, &10);
    assert_eq!(page.items.len(), 3);
    assert_eq!(page.next_cursor, None);
}

#[test]
fn test_paged_first_page_has_cursor() {
    let env = create_env();
    let (owner, client) = setup(&env);
    let recipient = Address::generate(&env);
    mint_n(&env, &client, &owner, &recipient, 5);

    let page = client.badges_of_paged(&recipient, &0, &3);
    assert_eq!(page.items.len(), 3);
    assert_eq!(page.next_cursor, Some(3));
}

#[test]
fn test_paged_second_page_no_cursor() {
    let env = create_env();
    let (owner, client) = setup(&env);
    let recipient = Address::generate(&env);
    mint_n(&env, &client, &owner, &recipient, 5);

    let page = client.badges_of_paged(&recipient, &3, &3);
    assert_eq!(page.items.len(), 2); // only 2 remain
    assert_eq!(page.next_cursor, None);
}

#[test]
fn test_paged_exact_multiple_of_limit() {
    // 6 badges, page size 3 → page 0 has cursor=3; page 1 has cursor=None
    let env = create_env();
    let (owner, client) = setup(&env);
    let recipient = Address::generate(&env);
    mint_n(&env, &client, &owner, &recipient, 6);

    let page0 = client.badges_of_paged(&recipient, &0, &3);
    assert_eq!(page0.items.len(), 3);
    assert_eq!(page0.next_cursor, Some(3));

    let page1 = client.badges_of_paged(&recipient, &3, &3);
    assert_eq!(page1.items.len(), 3);
    assert_eq!(page1.next_cursor, None);
}

#[test]
fn test_paged_oversized_limit_clamped_to_max() {
    let env = create_env();
    let (owner, client) = setup(&env);
    let recipient = Address::generate(&env);
    // Mint fewer than MAX_PAGE_SIZE badges so the result is all of them
    mint_n(&env, &client, &owner, &recipient, 10);

    // Pass a limit larger than MAX_PAGE_SIZE
    let huge_limit = MAX_PAGE_SIZE + 100;
    let page = client.badges_of_paged(&recipient, &0, &huge_limit);
    assert_eq!(page.items.len(), 10);
    assert_eq!(page.next_cursor, None);
}

#[test]
fn test_paged_zero_limit_clamped_to_max() {
    let env = create_env();
    let (owner, client) = setup(&env);
    let recipient = Address::generate(&env);
    mint_n(&env, &client, &owner, &recipient, 5);

    let page = client.badges_of_paged(&recipient, &0, &0);
    // 0 is clamped to MAX_PAGE_SIZE; 5 < MAX_PAGE_SIZE so all returned
    assert_eq!(page.items.len(), 5);
    assert_eq!(page.next_cursor, None);
}

#[test]
fn test_paged_cursor_ordering_is_stable() {
    // Walk all pages and reconstruct the full list; compare with badges_of.
    let env = create_env();
    let (owner, client) = setup(&env);
    let recipient = Address::generate(&env);
    mint_n(&env, &client, &owner, &recipient, 11);

    let mut all_ids: soroban_sdk::Vec<u64> = soroban_sdk::Vec::new(&env);
    let mut cursor: u32 = 0;
    loop {
        let page = client.badges_of_paged(&recipient, &cursor, &4);
        for id in page.items.iter() {
            all_ids.push_back(id);
        }
        match page.next_cursor {
            Some(next) => cursor = next,
            None => break,
        }
    }

    let expected = client.badges_of(&recipient);
    assert_eq!(all_ids.len(), expected.len());
    for (a, b) in all_ids.iter().zip(expected.iter()) {
        assert_eq!(a, b);
    }
}

#[test]
fn test_badge_count() {
    let env = create_env();
    let (owner, client) = setup(&env);
    let recipient = Address::generate(&env);

    assert_eq!(client.badge_count(&recipient), 0);
    mint_n(&env, &client, &owner, &recipient, 7);
    assert_eq!(client.badge_count(&recipient), 7);
}

#[test]
fn test_start_beyond_count_returns_empty() {
    let env = create_env();
    let (owner, client) = setup(&env);
    let recipient = Address::generate(&env);
    mint_n(&env, &client, &owner, &recipient, 3);

    let page = client.badges_of_paged(&recipient, &10, &5);
    assert_eq!(page.items.len(), 0);
    assert_eq!(page.next_cursor, None);
}

// ============================================================================
// Burn tests
// ============================================================================

#[test]
fn test_mint_then_burn_removes_badge() {
    let env = create_env();
    let (owner, client) = setup(&env);
    let recipient = Address::generate(&env);
    let id = client.mint(
        &owner,
        &recipient,
        &String::from_str(&env, "Q1 2025 Payroll"),
        &String::from_str(&env, "ipfs://q1-2025-payroll"),
    );

    // Badge exists pre-burn
    assert!(client.get_badge(&id).is_some());
    assert_eq!(client.badge_count(&recipient), 1);

    client.burn(&owner, &id);

    // Check the event immediately after burn, before any other calls
    let events = env.events().all();
    let event: BadgeBurned = events.last().unwrap().2.into_val(&env);
    assert_eq!(event.token_id, id);
    assert_eq!(event.owner, recipient);

    // get_badge reflects the badge no longer exists
    assert_eq!(client.get_badge(&id), None);
    assert_eq!(client.badge_count(&recipient), 0);
    assert_eq!(client.badges_of(&recipient).len(), 0);
}

#[test]
fn test_burn_removes_correct_badge_among_several() {
    let env = create_env();
    let (owner, client) = setup(&env);
    let recipient = Address::generate(&env);
    mint_n(&env, &client, &owner, &recipient, 3);

    let all_ids = client.badges_of(&recipient);
    let middle_id = all_ids.get(1).unwrap();

    client.burn(&owner, &middle_id);

    assert_eq!(client.get_badge(&middle_id), None);
    assert_eq!(client.badge_count(&recipient), 2);

    let remaining = client.badges_of(&recipient);
    assert_eq!(remaining.len(), 2);
    assert!(!remaining.iter().any(|id| id == middle_id));
}

#[test]
#[should_panic(expected = "Only owner can manage badges")]
fn test_non_admin_cannot_burn_badge() {
    let env = create_env();
    let (owner, client) = setup(&env);
    let attacker = Address::generate(&env);
    let recipient = Address::generate(&env);
    let id = client.mint(
        &owner,
        &recipient,
        &String::from_str(&env, "Payroll Badge"),
        &String::from_str(&env, "ipfs://payroll-badge"),
    );

    client.burn(&attacker, &id);
}

#[test]
#[should_panic(expected = "Badge not found")]
fn test_burn_nonexistent_badge_panics() {
    let env = create_env();
    let (owner, client) = setup(&env);

    client.burn(&owner, &999u64);
}
