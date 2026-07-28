//! Cross-contract integration test verifying the composition pattern between
//! stello_pay_contract's `activate_agreement` and nft_payroll_badge's `mint`.
//!
//! This test deploys both PayrollContract and NftPayrollBadgeContract, creates
//! and activates a payroll agreement, then mints a badge for the employee that
//! references the activated agreement in its metadata. It verifies end-to-end
//! that the two contracts can be composed correctly by an off-chain orchestrator.
//!
//! ## Composition pattern
//!
//! This test models an **orchestrated** pattern: an off-chain service calls
//! `activate_agreement` on the payroll contract, reads the resulting
//! `agreement_id`, and then calls `mint` on the badge contract, embedding the
//! agreement reference in `metadata_uri`. A future direct on-chain integration
//! would have the payroll contract call the badge contract inline during
//! activation — not yet implemented.
//!
//! ## Security assumptions validated
//!
//! - Only the badge contract owner can mint badges
//! - Badge metadata faithfully references the activated agreement
//! - `get_badge` returns the expected badge after minting
//! - `badges_of` correctly reports badge holdings for the employee
//! - Mismatched callers cannot mint badges on behalf of others
//!
//! Scope: test only — no runtime logic, storage schema, or APIs are changed.
#![cfg(test)]

use nft_payroll_badge::{Badge, NftPayrollBadgeContract, NftPayrollBadgeContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};
use stello_pay_contract::{
    storage::{AgreementStatus, DataKey},
    PayrollContract, PayrollContractClient,
};

// ============================================================================
// CONSTANTS
// ============================================================================

const ONE_DAY: u64 = 86_400;
const ONE_WEEK: u64 = 604_800;
const EMPLOYEE_SALARY: i128 = 2_000;
const PAYROLL_FUND: i128 = 100_000;

// ============================================================================
// HELPERS
// ============================================================================

fn env() -> Env {
    let e = Env::default();
    e.mock_all_auths();
    e
}

fn addr(env: &Env) -> Address {
    Address::generate(env)
}

fn set_time(env: &Env, ts: u64) {
    env.ledger().with_mut(|li| li.timestamp = ts);
}

/// Seeds payroll internal storage so the agreement can be activated and
/// employees can claim payroll. This mirrors the pattern used in other
/// integration tests such as `test_salary_adjustment_payroll_integration.rs`.
fn seed_payroll_for_activation(
    env: &Env,
    payroll_id: &Address,
    agreement_id: u128,
    token: &Address,
    employee: &Address,
    salary: i128,
) {
    env.as_contract(payroll_id, || {
        // Set activation time to current ledger timestamp so period arithmetic
        // is well-defined once the agreement is activated.
        DataKey::set_agreement_activation_time(env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(env, agreement_id, token);
        DataKey::set_employee(env, agreement_id, 0, employee);
        DataKey::set_employee_salary(env, agreement_id, 0, salary);
        DataKey::set_employee_claimed_periods(env, agreement_id, 0, 0);
        DataKey::set_employee_count(env, agreement_id, 1);
        // Fund escrow so claims succeed after activation
        DataKey::set_agreement_escrow_balance(env, agreement_id, token, PAYROLL_FUND);
    });
}

// ============================================================================
// TESTS
// ============================================================================

/// Core happy-path test: activates a payroll agreement and mints a badge for
/// the employee whose metadata references the activated agreement.
///
/// # Steps
/// 1. Deploy payroll and badge contracts, initialize both.
/// 2. Create a payroll agreement and add an employee.
/// 3. Seed internal storage and activate the agreement.
/// 4. Mint a badge for the employee with metadata referencing the agreement.
/// 5. Assert the badge is correctly stored and linked to the agreement.
/// 6. Verify badge count remains stable after all operations.
#[test]
fn test_activate_agreement_and_mint_badge_end_to_end() {
    let env = env();
    set_time(&env, 1_000_000);

    // Deploy contracts
    let payroll_id = env.register_contract(None, PayrollContract);
    let payroll_client = PayrollContractClient::new(&env, &payroll_id);
    let badge_id = env.register_contract(None, NftPayrollBadgeContract);
    let badge_client = NftPayrollBadgeContractClient::new(&env, &badge_id);

    // Setup actors
    let payroll_owner = addr(&env);
    let badge_owner = addr(&env);
    let employer = addr(&env);
    let employee = addr(&env);
    let token = addr(&env);

    // Initialize contracts
    payroll_client.initialize(&payroll_owner);
    badge_client.initialize(&badge_owner);

    // ── Step 1: Create payroll agreement ──────────────────────────────────
    let agreement_id = payroll_client.create_payroll_agreement(&employer, &token, &ONE_WEEK);

    // Verify agreement starts in Created status
    let agreement = payroll_client.get_agreement(&agreement_id).unwrap();
    assert_eq!(
        agreement.status,
        AgreementStatus::Created,
        "Agreement must be in Created status before activation"
    );

    // ── Step 2: Add employee ──────────────────────────────────────────────
    payroll_client.add_employee_to_agreement(&agreement_id, &employee, &EMPLOYEE_SALARY);

    let employees = payroll_client.get_agreement_employees(&agreement_id);
    assert_eq!(employees.len(), 1, "Exactly one employee expected");
    assert_eq!(employees.first().unwrap(), employee);

    // ── Step 3: Activate the agreement ────────────────────────────────────
    seed_payroll_for_activation(
        &env,
        &payroll_id,
        agreement_id,
        &token,
        &employee,
        EMPLOYEE_SALARY,
    );
    payroll_client.activate_agreement(&agreement_id);

    let activated = payroll_client.get_agreement(&agreement_id).unwrap();
    assert_eq!(
        activated.status,
        AgreementStatus::Active,
        "Agreement must be Active after activation"
    );
    assert!(
        activated.activated_at.is_some(),
        "activated_at must be set after activation"
    );

    // ── Step 4: Mint badge for the employee (orchestrated follow-up) ──────
    let badge_name = soroban_sdk::String::from_str(&env, "Active Payroll Badge");
    // Embed the agreement reference in the metadata URI so that off-chain
    // indexers and UI can correlate the badge with the payroll agreement.
    let metadata_uri = soroban_sdk::String::from_str(
        &env,
        &format!("ipfs://stellopay/badge/{}/employee", agreement_id),
    );

    let minted_id = badge_client.mint(&badge_owner, &employee, &badge_name, &metadata_uri);

    // ── Step 5: Assert badge correctness ──────────────────────────────────

    // Verify badge is retrievable and metadata references the agreement
    let badge: Badge = badge_client
        .get_badge(&minted_id)
        .expect("Badge must exist after minting");

    assert_eq!(badge.id, minted_id, "Badge ID must match minted ID");
    assert_eq!(badge.owner, employee, "Badge owner must be the employee");
    assert_eq!(
        badge.name, badge_name,
        "Badge name must match the minted name"
    );
    assert!(
        badge
            .metadata_uri
            .to_string()
            .contains(&agreement_id.to_string()),
        "Badge metadata_uri must contain the agreement_id reference"
    );
    assert!(
        badge.issued_at >= 1_000_000,
        "Badge issued_at must be at or after the initial ledger time"
    );

    // Verify badge shows up in the employee's badge list
    let owned = badge_client.badges_of(&employee);
    assert_eq!(owned.len(), 1, "Employee must hold exactly one badge");
    assert_eq!(owned.first().unwrap(), minted_id);

    // Verify badge count
    let count = badge_client.badge_count(&employee);
    assert_eq!(count, 1, "Employee badge count must be 1");

    // ── Step 6: Verify badge count / pagination after all operations ───────
    let count_final = badge_client.badge_count(&employee);
    assert_eq!(count_final, 1, "Employee badge count must remain 1");
}

/// Verifies that a badge can be minted for multiple employees after
/// agreement activation, each with distinct agreement-aware metadata.
#[test]
fn test_multiple_employee_badges_after_activation() {
    let env = env();
    set_time(&env, 2_000_000);

    let payroll_id = env.register_contract(None, PayrollContract);
    let payroll_client = PayrollContractClient::new(&env, &payroll_id);
    let badge_id = env.register_contract(None, NftPayrollBadgeContract);
    let badge_client = NftPayrollBadgeContractClient::new(&env, &badge_id);

    let payroll_owner = addr(&env);
    let badge_owner = addr(&env);
    let employer = addr(&env);
    let employee_a = addr(&env);
    let employee_b = addr(&env);
    let token = addr(&env);

    payroll_client.initialize(&payroll_owner);
    badge_client.initialize(&badge_owner);

    let agreement_id = payroll_client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    payroll_client.add_employee_to_agreement(&agreement_id, &employee_a, &EMPLOYEE_SALARY);
    payroll_client.add_employee_to_agreement(&agreement_id, &employee_b, &(EMPLOYEE_SALARY + 500));

    // Seed internal storage for both employees
    env.as_contract(&payroll_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(&env, agreement_id, &token);
        DataKey::set_employee(&env, agreement_id, 0, &employee_a);
        DataKey::set_employee_salary(&env, agreement_id, 0, EMPLOYEE_SALARY);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee(&env, agreement_id, 1, &employee_b);
        DataKey::set_employee_salary(&env, agreement_id, 1, EMPLOYEE_SALARY + 500);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 1, 0);
        DataKey::set_employee_count(&env, agreement_id, 2);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, PAYROLL_FUND * 2);
    });

    payroll_client.activate_agreement(&agreement_id);

    // Mint distinct badges for each employee
    let id_a = badge_client.mint(
        &badge_owner,
        &employee_a,
        &soroban_sdk::String::from_str(&env, "Payroll A"),
        &soroban_sdk::String::from_str(
            &env,
            &format!("ipfs://stellopay/badge/{}/employee/0", agreement_id),
        ),
    );
    let id_b = badge_client.mint(
        &badge_owner,
        &employee_b,
        &soroban_sdk::String::from_str(&env, "Payroll B"),
        &soroban_sdk::String::from_str(
            &env,
            &format!("ipfs://stellopay/badge/{}/employee/1", agreement_id),
        ),
    );

    // Verify each badge references the agreement
    let badge_a = badge_client.get_badge(&id_a).unwrap();
    let badge_b = badge_client.get_badge(&id_b).unwrap();

    assert_eq!(badge_a.owner, employee_a);
    assert_eq!(badge_b.owner, employee_b);
    assert_ne!(id_a, id_b, "Badge IDs must be unique");
    assert!(
        badge_a
            .metadata_uri
            .to_string()
            .contains(&agreement_id.to_string()),
        "Badge A metadata must reference the agreement"
    );
    assert!(
        badge_b
            .metadata_uri
            .to_string()
            .contains(&agreement_id.to_string()),
        "Badge B metadata must reference the agreement"
    );

    // Verify each employee only sees their own badge
    assert_eq!(badge_client.badge_count(&employee_a), 1);
    assert_eq!(badge_client.badge_count(&employee_b), 1);
}

/// Verifies that only the badge contract owner can mint badges, even after
/// the payroll agreement has been activated. This confirms the security
/// boundary between the two contracts in an orchestrated setup.
#[test]
fn test_non_owner_cannot_mint_badge_after_activation() {
    let env = env();
    set_time(&env, 3_000_000);

    let payroll_id = env.register_contract(None, PayrollContract);
    let payroll_client = PayrollContractClient::new(&env, &payroll_id);
    let badge_id = env.register_contract(None, NftPayrollBadgeContract);
    let badge_client = NftPayrollBadgeContractClient::new(&env, &badge_id);

    let payroll_owner = addr(&env);
    let badge_owner = addr(&env);
    let employer = addr(&env);
    let employee = addr(&env);
    let random_caller = addr(&env);
    let token = addr(&env);

    payroll_client.initialize(&payroll_owner);
    badge_client.initialize(&badge_owner);

    let agreement_id = payroll_client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    payroll_client.add_employee_to_agreement(&agreement_id, &employee, &EMPLOYEE_SALARY);

    seed_payroll_for_activation(
        &env,
        &payroll_id,
        agreement_id,
        &token,
        &employee,
        EMPLOYEE_SALARY,
    );
    payroll_client.activate_agreement(&agreement_id);

    // A non-owner (random_caller) tries to mint a badge — this should panic
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        badge_client.mint(
            &random_caller,
            &employee,
            &soroban_sdk::String::from_str(&env, "Unauthorized"),
            &soroban_sdk::String::from_str(&env, "ipfs://fake"),
        );
    }));
    assert!(result.is_err(), "Non-owner must not be able to mint badges");

    // Verify no badge was minted in the failed attempt
    let badge_count = badge_client.badge_count(&employee);
    assert_eq!(
        badge_count, 0,
        "Employee must hold zero badges after failed mint"
    );
}

/// Verifies that a badge minted for a deactivated agreement (agreement later
/// cancelled) still correctly reflects the original activation metadata.
#[test]
fn test_badge_persists_after_agreement_cancellation() {
    let env = env();
    set_time(&env, 4_000_000);

    let payroll_id = env.register_contract(None, PayrollContract);
    let payroll_client = PayrollContractClient::new(&env, &payroll_id);
    let badge_id = env.register_contract(None, NftPayrollBadgeContract);
    let badge_client = NftPayrollBadgeContractClient::new(&env, &badge_id);

    let payroll_owner = addr(&env);
    let badge_owner = addr(&env);
    let employer = addr(&env);
    let employee = addr(&env);
    let token = addr(&env);

    payroll_client.initialize(&payroll_owner);
    badge_client.initialize(&badge_owner);

    let agreement_id = payroll_client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    payroll_client.add_employee_to_agreement(&agreement_id, &employee, &EMPLOYEE_SALARY);

    seed_payroll_for_activation(
        &env,
        &payroll_id,
        agreement_id,
        &token,
        &employee,
        EMPLOYEE_SALARY,
    );
    payroll_client.activate_agreement(&agreement_id);

    // Mint badge while agreement is active
    let minted_id = badge_client.mint(
        &badge_owner,
        &employee,
        &soroban_sdk::String::from_str(&env, "Active Badge"),
        &soroban_sdk::String::from_str(&env, &format!("ipfs://stellopay/badge/{}", agreement_id)),
    );

    // Cancel the agreement
    payroll_client.cancel_agreement(&agreement_id);
    let cancelled = payroll_client.get_agreement(&agreement_id).unwrap();
    assert_eq!(
        cancelled.status,
        AgreementStatus::Cancelled,
        "Agreement must be Cancelled"
    );

    // Badge metadata must still reference the original agreement
    let badge = badge_client.get_badge(&minted_id).unwrap();
    assert!(
        badge
            .metadata_uri
            .to_string()
            .contains(&agreement_id.to_string()),
        "Badge metadata must persist the agreement reference after cancellation"
    );
    assert_eq!(badge.owner, employee);
    assert_eq!(badge.id, minted_id);
}

/// Verifies the paginated badge query works correctly when multiple badges
/// are minted for the same employee across multiple agreements.
#[test]
fn test_badges_of_paged_after_multiple_activations() {
    let env = env();
    set_time(&env, 5_000_000);

    let payroll_id = env.register_contract(None, PayrollContract);
    let payroll_client = PayrollContractClient::new(&env, &payroll_id);
    let badge_id = env.register_contract(None, NftPayrollBadgeContract);
    let badge_client = NftPayrollBadgeContractClient::new(&env, &badge_id);

    let payroll_owner = addr(&env);
    let badge_owner = addr(&env);
    let employer = addr(&env);
    let employee = addr(&env);
    let token = addr(&env);

    payroll_client.initialize(&payroll_owner);
    badge_client.initialize(&badge_owner);

    // Create and activate three agreements, minting a badge for each
    for i in 0..3 {
        let agreement_id = payroll_client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
        payroll_client.add_employee_to_agreement(&agreement_id, &employee, &EMPLOYEE_SALARY);

        env.as_contract(&payroll_id, || {
            DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
            DataKey::set_agreement_period_duration(&env, agreement_id, ONE_DAY);
            DataKey::set_agreement_token(&env, agreement_id, &token);
            DataKey::set_employee(&env, agreement_id, 0, &employee);
            DataKey::set_employee_salary(&env, agreement_id, 0, EMPLOYEE_SALARY);
            DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
            DataKey::set_employee_count(&env, agreement_id, 1);
            DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, PAYROLL_FUND / 3);
        });

        payroll_client.activate_agreement(&agreement_id);

        badge_client.mint(
            &badge_owner,
            &employee,
            &soroban_sdk::String::from_str(&env, &format!("Badge {}", i)),
            &soroban_sdk::String::from_str(
                &env,
                &format!("ipfs://stellopay/badge/{}", agreement_id),
            ),
        );
    }

    // Total badges for the employee
    assert_eq!(badge_client.badge_count(&employee), 3);

    // Paginated query: page 1 (limit 2)
    let page1 = badge_client.badges_of_paged(&employee, &0u32, &2u32);
    assert_eq!(page1.items.len(), 2);
    assert!(page1.next_cursor.is_some());

    // Page 2 (from cursor of page 1)
    let page2 = badge_client.badges_of_paged(&employee, &page1.next_cursor.unwrap(), &2u32);
    assert_eq!(page2.items.len(), 1);
    assert!(
        page2.next_cursor.is_none(),
        "Final page must have no cursor"
    );

    // Verify all badge metadata references their respective agreements
    for id in page1.items.iter().chain(page2.items.iter()) {
        let badge = badge_client.get_badge(&id).unwrap();
        assert!(
            badge
                .metadata_uri
                .to_string()
                .contains("ipfs://stellopay/badge/"),
            "Each badge must reference a Stellopay agreement"
        );
    }
}
