//! Cross-contract integration test verifying that a payroll agreement created
//! against a specific pinned template version correctly reflects that version's
//! terms and is unaffected by later published template versions.
//!
//! This test deploys both `TemplateVersioning` and `PayrollContract`,
//! registers a template, publishes v1, creates a template_versioning agreement
//! pinned to v1, creates a corresponding payroll agreement, then publishes v2
//! and asserts the payroll agreement still references the original pinned
//! version.
//!
//! Scope: test only — no runtime logic, storage schema, or APIs are changed.
//! The wiring is demonstrated at the caller / orchestration layer.
#![cfg(test)]

use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, BytesN, Env, String,
};

use stello_pay_contract::{PayrollContract, PayrollContractClient};
use template_versioning::{TemplateVersioning, TemplateVersioningClient};

// ============================================================================
// CONSTANTS
// ============================================================================

const ONE_WEEK: u64 = 604_800;
const BASE_SALARY: i128 = 2_000;
const PAYROLL_FUND: i128 = 100_000;
const EMPLOYER_FLOAT: i128 = 200_000;

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

fn token(env: &Env) -> Address {
    let admin = addr(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

fn mint(env: &Env, tok: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, tok).mint(to, &amount);
}

fn set_time(env: &Env, ts: u64) {
    env.ledger().with_mut(|li| li.timestamp = ts);
}

fn advance(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| li.timestamp += seconds);
}

fn balance(env: &Env, tok: &Address, who: &Address) -> i128 {
    TokenClient::new(env, tok).balance(who)
}

fn schema_hash(env: &Env, seed: u8) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    // Fill remaining bytes with a pattern so the hash is distinguishable
    for i in 1..32 {
        bytes[i] = seed.wrapping_add(i as u8);
    }
    BytesN::from_array(env, &bytes)
}

// ============================================================================
// TESTS
// ============================================================================

/// Full integration flow:
/// 1. Deploy and initialize both contracts
/// 2. Register a template and publish v1
/// 3. Create a template_versioning agreement pinned to v1
/// 4. Create the corresponding payroll agreement
/// 5. Publish v2 with different schema_hash
/// 6. Assert the pinned agreement still references v1 and the payroll
///    agreement exists with the expected parameters
#[test]
fn test_template_versioning_wired_to_payroll_agreement() {
    let env = env();
    set_time(&env, 100_000);

    // Deploy contracts
    let versioning_id = env.register_contract(None, TemplateVersioning);
    let versioning = TemplateVersioningClient::new(&env, &versioning_id);
    let payroll_id = env.register_contract(None, PayrollContract);
    let payroll = PayrollContractClient::new(&env, &payroll_id);

    // Setup addresses and tokens
    let admin = addr(&env);
    let employer = addr(&env);
    let employee = addr(&env);
    let tok = token(&env);
    mint(&env, &tok, &employer, EMPLOYER_FLOAT);

    // Initialize both contracts
    versioning.initialize(&admin).unwrap();
    payroll.initialize(&employer);

    // ------------------------------------------------------------------
    // Step 1: Register a template and publish v1
    // ------------------------------------------------------------------
    let template_name = String::from_str(&env, "Standard Payroll");
    let template_id = versioning
        .register_template(&admin, &template_name)
        .unwrap();

    let v1_schema = schema_hash(&env, 0x01);
    let v1_notes = String::from_str(&env, "Initial version");
    let v1_num = versioning
        .publish_template_version(&admin, &template_id, &v1_schema, &v1_notes, &false)
        .unwrap();
    assert_eq!(v1_num, 1, "First version should be 1");
    assert_eq!(
        versioning.latest_version(&template_id).unwrap(),
        1,
        "Latest version should be 1 after first publish"
    );

    // ------------------------------------------------------------------
    // Step 2: Create a template_versioning agreement pinned to v1
    // ------------------------------------------------------------------
    let label = String::from_str(&env, "Onboarding Agreement");
    let tv_agreement_id = versioning
        .create_agreement(&employer, &template_id, &v1_num, &label)
        .unwrap();

    let tv_agreement = versioning.get_agreement(&tv_agreement_id).unwrap();
    assert_eq!(tv_agreement.template_id, template_id);
    assert_eq!(tv_agreement.template_version, 1);
    assert_eq!(tv_agreement.creator, employer);
    assert_eq!(tv_agreement.label, label);

    // ------------------------------------------------------------------
    // Step 3: Create the corresponding payroll agreement
    // ------------------------------------------------------------------
    let payroll_agreement_id =
        payroll.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    payroll.add_employee_to_agreement(&payroll_agreement_id, &employee, &BASE_SALARY);
    payroll.activate_agreement(&payroll_agreement_id);

    // Fund and seed the payroll contract
    mint(&env, &tok, &payroll_id, PAYROLL_FUND);
    env.as_contract(&payroll_id, || {
        use stello_pay_contract::storage::DataKey;
        DataKey::set_agreement_escrow_balance(&env, payroll_agreement_id, &tok, PAYROLL_FUND);
        DataKey::set_agreement_activation_time(&env, payroll_agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, payroll_agreement_id, ONE_WEEK);
        DataKey::set_agreement_token(&env, payroll_agreement_id, &tok);
        DataKey::set_employee(&env, payroll_agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, payroll_agreement_id, 0, BASE_SALARY);
        DataKey::set_employee_claimed_periods(&env, payroll_agreement_id, 0, 0);
        DataKey::set_employee_count(&env, payroll_agreement_id, 1);
    });

    // Verify payroll agreement was created
    let payroll_agreement = payroll.get_agreement(&payroll_agreement_id).unwrap();
    assert_eq!(payroll_agreement.employer, employer);
    assert_eq!(payroll_agreement.token, tok);
    assert_eq!(
        payroll_agreement.grace_period_seconds,
        ONE_WEEK,
        "Payroll agreement should carry the correct grace period"
    );
    assert_eq!(
        payroll_agreement.created_at,
        100_000,
        "Payroll agreement should be created at the pinned timestamp"
    );

    // ------------------------------------------------------------------
    // Step 4: Publish v2 with a different schema_hash
    // ------------------------------------------------------------------
    advance(&env, ONE_WEEK); // advance time so v2 has a distinct created_at
    let v2_schema = schema_hash(&env, 0x02);
    let v2_notes = String::from_str(&env, "Updated terms - v2");
    let v2_num = versioning
        .publish_template_version(&admin, &template_id, &v2_schema, &v2_notes, &false)
        .unwrap();
    assert_eq!(v2_num, 2, "Second version should be 2");
    assert_eq!(versioning.latest_version(&template_id).unwrap(), 2);

    // ------------------------------------------------------------------
    // Step 5: Assert the pinned agreement is unchanged — it still points to v1
    // ------------------------------------------------------------------
    let tv_agreement_after = versioning.get_agreement(&tv_agreement_id).unwrap();
    assert_eq!(
        tv_agreement_after.template_version, 1,
        "Agreement must remain pinned to v1 after v2 is published"
    );
    assert_eq!(
        tv_agreement_after.created_at, 100_000,
        "Agreement created_at must not change"
    );

    // ------------------------------------------------------------------
    // Step 6: Verify the payroll agreement's creation timestamp is between
    //         v1 publication and v2 publication, confirming temporal ordering
    // ------------------------------------------------------------------
    let v1_record = versioning.get_version(&template_id, &1).unwrap();
    let v2_record = versioning.get_version(&template_id, &2).unwrap();
    assert!(
        v1_record.created_at <= payroll_agreement.created_at,
        "Payroll agreement should be created after (or at) v1 publication"
    );
    assert!(
        payroll_agreement.created_at < v2_record.created_at,
        "Payroll agreement should be created before v2 publication"
    );

    // ------------------------------------------------------------------
    // Step 7: Verify that a new agreement created after v2 can use v2
    // ------------------------------------------------------------------
    advance(&env, 1);
    let label_v2 = String::from_str(&env, "Post-update agreement");
    let tv_agreement_v2_id = versioning
        .create_agreement(&employer, &template_id, &v2_num, &label_v2)
        .unwrap();
    let tv_agreement_v2 = versioning.get_agreement(&tv_agreement_v2_id).unwrap();
    assert_eq!(
        tv_agreement_v2.template_version, 2,
        "New agreements can explicitly pin to v2"
    );
}

/// Verifies that a template_versioning agreement pinned to v1 retains its
/// binding even after v1 is deprecated, while new agreements cannot use v1.
#[test]
fn test_deprecated_version_does_not_affect_existing_agreements() {
    let env = env();
    set_time(&env, 100_000);

    let versioning_id = env.register_contract(None, TemplateVersioning);
    let versioning = TemplateVersioningClient::new(&env, &versioning_id);

    let admin = addr(&env);
    let employer = addr(&env);

    versioning.initialize(&admin).unwrap();

    // Register and publish v1
    let name = String::from_str(&env, "Deprecation Test Template");
    let template_id = versioning.register_template(&admin, &name).unwrap();
    let hash_v1 = schema_hash(&env, 0xAA);
    let notes_v1 = String::from_str(&env, "v1");
    versioning
        .publish_template_version(&admin, &template_id, &hash_v1, &notes_v1, &false)
        .unwrap();

    // Create agreement pinned to v1
    let label_v1 = String::from_str(&env, "Legacy agreement");
    let tv_agreement_id = versioning
        .create_agreement(&employer, &template_id, &1, &label_v1)
        .unwrap();
    assert_eq!(
        versioning.get_agreement(&tv_agreement_id).unwrap().template_version,
        1
    );

    // Publish v2 and deprecate v1
    advance(&env, 100);
    let hash_v2 = schema_hash(&env, 0xBB);
    let notes_v2 = String::from_str(&env, "v2");
    versioning
        .publish_template_version(&admin, &template_id, &hash_v2, &notes_v2, &false)
        .unwrap();
    let deprecation_reason = Some(String::from_str(&env, "Superseded by v2"));
    versioning
        .deprecate_version(&admin, &template_id, &1, &deprecation_reason)
        .unwrap();

    // Existing agreement is unaffected
    let binding = versioning.get_agreement(&tv_agreement_id).unwrap();
    assert_eq!(
        binding.template_version, 1,
        "Existing agreement must remain on v1 even after deprecation"
    );

    // New agreement against v1 is rejected
    let new_label = String::from_str(&env, "Should fail");
    let result = versioning.create_agreement(&employer, &template_id, &1, &new_label);
    assert!(
        result.is_err(),
        "Creating a new agreement on deprecated v1 must fail"
    );

    // New agreement against v2 succeeds
    let label_v2 = String::from_str(&env, "Post-deprecation agreement");
    let new_id = versioning
        .create_agreement(&employer, &template_id, &2, &label_v2)
        .unwrap();
    assert_eq!(
        versioning.get_agreement(&new_id).unwrap().template_version,
        2
    );

    // Verify v1 record shows deprecated = true with reason
    let v1_record = versioning.get_version(&template_id, &1).unwrap();
    assert!(v1_record.deprecated, "v1 should be marked deprecated");
    assert_eq!(
        v1_record.deprecation_reason,
        deprecation_reason,
        "Deprecation reason should be preserved"
    );
}
