//! Comprehensive test suite for nft_payroll_badge contract.
//!
//! Covers: initialization, minting, admin metadata URI updates, `badges_of`,
//! `badges_of_paged` (empty, single page, multi-page, exact-multiple-of-limit,
//! oversized-limit clamping).

#![cfg(test)]

use nft_payroll_badge::{
    MetadataUpdated, NftPayrollBadgeContract, NftPayrollBadgeContractClient, MAX_PAGE_SIZE,
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
// badge_count accuracy tests
// ============================================================================

/// Mints one badge at a time to 30 distinct recipient addresses and asserts
/// that each recipient's badge_count equals exactly the number of badges minted
/// to them so far.
///
/// Invariant under test: badge_count(r) must equal the cumulative number of
/// successful mint calls whose `recipient` argument was `r`, independent of
/// how many badges were minted to any other address.
#[test]
fn test_badge_count_sequential_distinct_recipients() {
    let env = create_env();
    let (owner, client) = setup(&env);

    // Generate 30 distinct recipients up-front.
    const N: usize = 30;
    let recipients: Vec<Address> = (0..N).map(|_| Address::generate(&env)).collect();

    // Baseline: every recipient starts at 0.
    for r in &recipients {
        assert_eq!(
            client.badge_count(r),
            0,
            "expected 0 badges before any mints"
        );
    }

    // Mint one badge per recipient in sequence and check after each mint.
    for (i, r) in recipients.iter().enumerate() {
        client.mint(
            &owner,
            r,
            &String::from_str(&env, "Sequential Badge"),
            &String::from_str(&env, "ipfs://sequential"),
        );

        // The recipient just minted to must have exactly 1.
        assert_eq!(
            client.badge_count(r),
            1,
            "recipient {} should have 1 badge after their first mint",
            i
        );

        // All prior recipients still have exactly 1 — their count is stable.
        for (j, prev) in recipients.iter().enumerate().take(i) {
            assert_eq!(
                client.badge_count(prev),
                1,
                "recipient {} count should still be 1 after minting to recipient {}",
                j,
                i
            );
        }

        // All future recipients still have exactly 0.
        for future in recipients.iter().skip(i + 1) {
            assert_eq!(
                client.badge_count(future),
                0,
                "un-minted recipient should still have 0 after minting to recipient {}",
                i
            );
        }
    }

    // Final sanity: every recipient has exactly 1, no more, no less.
    for (i, r) in recipients.iter().enumerate() {
        assert_eq!(
            client.badge_count(r),
            1,
            "final check: recipient {} should have exactly 1 badge",
            i
        );
    }
}

/// Combines mints across 10 distinct recipients with repeated mints to some
/// of those same recipients.  Asserts that badge_count for each address
/// always equals the exact number of mints directed at that address,
/// regardless of mints sent to other addresses.
///
/// Invariant under test: badge_count is per-address; concurrent activity on
/// other addresses must not corrupt or inflate any single address's counter.
#[test]
fn test_badge_count_combined_distinct_and_repeated_recipients() {
    let env = create_env();
    let (owner, client) = setup(&env);

    // 10 distinct recipients; we will vary how many badges each gets.
    const N: usize = 10;
    let recipients: Vec<Address> = (0..N).map(|_| Address::generate(&env)).collect();

    // Desired final badge counts per recipient (index → count).
    // Deliberately non-uniform: some get 1, some get several, one gets many.
    let target_counts: [u32; N] = [1, 3, 1, 5, 2, 1, 8, 1, 4, 2];

    // Track how many badges we have minted to each recipient so far.
    let mut minted: [u32; N] = [0; N];

    // Interleave mints across all recipients in round-robin order until every
    // recipient has reached its target.  This exercises the counter under
    // concurrent "activity" from many addresses in the same storage context.
    let max_rounds = *target_counts.iter().max().unwrap();
    for round in 0..max_rounds {
        for (i, r) in recipients.iter().enumerate() {
            if minted[i] < target_counts[i] {
                client.mint(
                    &owner,
                    r,
                    &String::from_str(&env, "Combined Badge"),
                    &String::from_str(&env, "ipfs://combined"),
                );
                minted[i] += 1;
            }

            // After every individual mint, verify ALL recipients have the
            // expected count so far — no cross-contamination.
            for (j, check) in recipients.iter().enumerate() {
                assert_eq!(
                    client.badge_count(check),
                    minted[j],
                    "round {round}: recipient {j} count mismatch after minting to recipient {i}"
                );
            }
        }
    }

    // Final assertion: exact match between target and actual counts.
    for (i, r) in recipients.iter().enumerate() {
        assert_eq!(
            client.badge_count(r),
            target_counts[i],
            "final: recipient {i} expected {} badges, got {}",
            target_counts[i],
            client.badge_count(r)
        );
    }

    // Cross-check: badge_count must equal the length of badges_of for every
    // recipient, proving the counter is consistent with the stored badge list.
    for (i, r) in recipients.iter().enumerate() {
        let ids = client.badges_of(r);
        assert_eq!(
            client.badge_count(r),
            ids.len(),
            "recipient {i}: badge_count != badges_of.len()"
        );
    }
}
