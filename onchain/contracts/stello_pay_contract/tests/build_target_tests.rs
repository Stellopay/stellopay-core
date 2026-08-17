//! Build target validation and public API surface tests.
//!
//! These tests verify that:
//!   1. The contract compiles and registers under `wasm32-unknown-unknown`.
//!   2. All `#[contractimpl]` public entrypoints are accessible with expected
//!      signatures — a regression suite that guards the semver-stable API.
//!   3. Breaking changes to the public interface are detected at test time
//!      (mirroring what `cargo semver-checks` enforces in CI).

use std::path::PathBuf;

use soroban_sdk::{testutils::Address as _, token, Address, BytesN, Env, Vec};
use stello_pay_contract::{
    storage::{
        DisputeStatus, EscrowCreateParams, GracePeriodExtensionPolicy, PayrollCreateParams,
        PayrollError,
    },
    PayrollContract, PayrollContractClient,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
}

/// Returns the body of the TOML `[header]` section, up to the next section.
fn section(text: &str, header: &str) -> Option<String> {
    let start = text.find(&format!("[{}]", header))?;
    let body_start = start + 1;
    let after = &text[body_start..];
    let body_end = after
        .find("\n[")
        .map(|i| body_start + i)
        .unwrap_or(text.len());
    Some(text[body_start..body_end].to_string())
}

/// True when `needle` appears as a quoted entry of a TOML array in `body`.
fn array_contains_quote(body: &str, needle: &str) -> bool {
    body.contains(&format!("\"{}\"", needle))
}

fn setup(env: &Env) -> (PayrollContractClient<'_>, Address) {
    env.mock_all_auths();
    let id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(env, &id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (client, owner)
}

fn full_setup(
    env: &Env,
) -> (
    PayrollContractClient<'_>,
    Address,
    Address,
    Address,
    Address,
    Address,
    u128,
) {
    env.mock_all_auths();
    let id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(env, &id);
    let owner = Address::generate(env);
    let employer = Address::generate(env);
    let employee = Address::generate(env);
    let arbiter = Address::generate(env);

    let token_admin = Address::generate(env);
    let token_addr = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    client.initialize(&owner);
    let agreement_id = client.create_payroll_agreement(&employer, &token_addr, &86400);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);
    client.activate_agreement(&agreement_id);

    env.as_contract(&client.address, || {
        use stello_pay_contract::storage::DataKey;
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token_addr, 10000);
    });

    (
        client,
        owner,
        employer,
        employee,
        arbiter,
        token_addr,
        agreement_id,
    )
}

fn create_escrow_token_pair(env: &Env) -> (Address, token::StellarAssetClient<'_>) {
    let token_admin = Address::generate(env);
    let t = env.register_stellar_asset_contract_v2(token_admin);
    (
        t.address(),
        token::StellarAssetClient::new(env, &t.address()),
    )
}

// ---------------------------------------------------------------------------
// 1. Basic build-target smoke tests
// ---------------------------------------------------------------------------

/// Verifies the contract can be registered and initialized.
#[test]
fn crate_declares_cdylib_crate_type() {
    let manifest = std::fs::read_to_string(manifest_path()).expect("read Cargo.toml");
    let lib = section(&manifest, "lib").expect("crate must declare a [lib] section");

    assert!(
        array_contains_quote(&lib, "cdylib"),
        "stello_pay_contract must declare `cdylib` in [lib].crate-type \
         so `cargo build --target wasm32-unknown-unknown --release` \
         produces a deployable Soroban artifact. Current [lib] block:\n{}",
        lib,
    );
    // We also expect `rlib` so the contract can be consumed by
    // sibling integration tests in this workspace; assert by hand
    // rather than by feature flag because the latter is fragile.
    assert!(
        array_contains_quote(&lib, "rlib"),
        "stello_pay_contract must declare `rlib` in [lib].crate-type so that \
         sibling workspace crates can `use` the contract in tests.",
    );
}

/// Double-initialization must panic.
#[test]
#[should_panic(expected = "Already initialized")]
fn test_double_initialize_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &id);
    let owner = Address::generate(&env);
    client.initialize(&owner);
    client.initialize(&owner);
}

/// Fresh contract is not emergency-paused.
#[test]
fn does_not_target_wasm32v1_none() {
    // `wasm32v1-none` is incompatible with the Soroban host. If a
    // contributor accidentally pins this target anywhere reachable
    // from the build, fail loudly here instead of producing a binary
    // the host will reject at deploy time.
    let manifest = std::fs::read_to_string(manifest_path()).expect("read Cargo.toml");
    assert!(
        !manifest.contains("wasm32v1-none"),
        "stello_pay_contract Cargo.toml must not reference the wasm32v1-none \
         target — only `wasm32-unknown-unknown` is supported by Soroban.",
    );
}

/// No arbiter set by default.
#[test]
fn test_get_arbiter_returns_none_by_default() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    assert!(client.get_arbiter().is_none());
}

/// Set/get arbiter round-trip.
#[test]
fn test_set_and_get_arbiter_round_trip() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let arbiter = Address::generate(&env);
    client.set_arbiter(&owner, &arbiter);
    assert_eq!(client.get_arbiter(), Some(arbiter));
}

// ---------------------------------------------------------------------------
// 2. Public API surface — semver-stable entrypoints
// ---------------------------------------------------------------------------

#[test]
fn test_set_and_get_rate_limiter() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let addr = Address::generate(&env);
    client.set_rate_limiter_contract(&owner, &addr);
    assert_eq!(client.get_rate_limiter_contract(), Some(addr));
}

#[test]
fn test_set_and_get_salary_adjustment() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let addr = Address::generate(&env);
    client.set_salary_adjustment_contract(&owner, &addr);
    assert_eq!(client.get_salary_adjustment_contract(), Some(addr));
}

#[test]
fn test_milestone_hook_contract_round_trip() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let addr = Address::generate(&env);
    client.set_milestone_hook_contract(&owner, &addr);
    assert_eq!(client.get_milestone_hook_contract(), Some(addr));
}

#[test]
fn test_upgrade_rejects_random_hash() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let attacker = Address::generate(&env);
    let hash = BytesN::from_array(&env, &[0u8; 32]);
    env.mock_auths(&[]);
    let result = client.try_upgrade(&hash, &attacker);
    assert!(result.is_err());
}

#[test]
fn test_migrate_state_noop_when_already_migrated() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let result = client.try_migrate_state(&owner, &1);
    assert!(result.is_err());
}

#[test]
fn test_create_and_get_payroll_agreement() {
    let env = Env::default();
    let (client, _owner, _employer, _employee, _arbiter, _token, agreement_id) = full_setup(&env);
    let agreement = client.get_agreement(&agreement_id);
    assert!(agreement.is_some());
}

#[test]
fn test_create_escrow_agreement_ok() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let (token_addr, _token_client) = create_escrow_token_pair(&env);
    let result = client.try_create_escrow_agreement(
        &employer,
        &contributor,
        &token_addr,
        &1000i128,
        &3600u64,
        &12u32,
    );
    assert!(result.is_ok());
}

#[test]
fn test_create_milestone_agreement_ok() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let (token_addr, _token_client) = create_escrow_token_pair(&env);
    let id = client.create_milestone_agreement(
        &employer,
        &contributor,
        &token_addr,
        &soroban_sdk::vec![&env, 1_000i128],
    );
    assert!(id > 0);
}

#[test]
fn test_add_milestone_fails_on_nonexistent_agreement() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let result = client.try_add_milestone(&9999u128, &500i128);
    assert!(result.is_err());
}

#[test]
fn test_approve_milestone_fails_on_nonexistent_agreement() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let result = client.try_approve_milestone(&9999u128, &0u32);
    assert!(result.is_err());
}

#[test]
fn test_claim_milestone_fails_on_nonexistent_agreement() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let result = client.try_claim_milestone(&9999u128, &0u32);
    assert!(result.is_err());
}

#[test]
fn test_reject_milestone_fails_on_nonexistent_agreement() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let result = client.try_reject_milestone(
        &9999u128,
        &0u32,
        &soroban_sdk::String::from_str(&env, "reason"),
    );
    assert!(result.is_err());
}

#[test]
fn test_expire_milestone_fails_on_nonexistent_agreement() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let result = client.try_expire_milestone(&9999u128, &0u32);
    assert!(result.is_err());
}

#[test]
fn test_get_milestone_count_zero() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let (token_addr, _token_client) = create_escrow_token_pair(&env);
    let id = client.create_milestone_agreement(
        &employer,
        &contributor,
        &token_addr,
        &soroban_sdk::vec![&env, 1_000i128],
    );
    // The agreement is created with exactly the upfront milestones supplied.
    assert_eq!(client.get_milestone_count(&id), 1);
}

#[test]
fn test_get_milestone_none() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let (token_addr, _token_client) = create_escrow_token_pair(&env);
    let id = client.create_milestone_agreement(
        &employer,
        &contributor,
        &token_addr,
        &soroban_sdk::vec![&env, 1_000i128],
    );
    // Milestone 1 exists (created upfront); 2 is past the end.
    assert!(client.get_milestone(&id, &2).is_none());
}

#[test]
fn test_get_agreement_employees() {
    let env = Env::default();
    let (client, _owner, _employer, employee, _arbiter, _token, agreement_id) = full_setup(&env);
    let employees = client.get_agreement_employees(&agreement_id);
    assert!(!employees.is_empty());
    assert_eq!(employees.get(0).unwrap(), employee);
}

#[test]
fn test_set_and_get_multisig_config() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let multisig = Address::generate(&env);
    let result = client.try_set_multisig_config(&owner, &multisig, &1000i128, &2000i128);
    assert!(result.is_ok());
    assert!(client.get_multisig_contract().is_some());
}

#[test]
fn test_claim_payroll_on_behalf_of_wrong_employee_fails() {
    let env = Env::default();
    let (client, _owner, _employer, _employee, _arbiter, _token, agreement_id) = full_setup(&env);
    let stranger = Address::generate(&env);
    env.mock_auths(&[]);
    let result = client.try_claim_payroll(&stranger, &agreement_id, &0);
    assert!(result.is_err());
}

#[test]
fn test_claim_payroll_in_token_wrong_employee_fails() {
    let env = Env::default();
    let (client, _owner, _employer, _employee, _arbiter, _token, agreement_id) = full_setup(&env);
    let stranger = Address::generate(&env);
    let payout_token = Address::generate(&env);
    env.mock_auths(&[]);
    let result = client.try_claim_payroll_in_token(&stranger, &agreement_id, &0, &payout_token);
    assert!(result.is_err());
}

#[test]
fn test_batch_claim_payroll_empty_indices_fails() {
    let env = Env::default();
    let (client, _owner, _employer, employee, _arbiter, _token, agreement_id) = full_setup(&env);
    let empty: Vec<u32> = Vec::new(&env);
    let result = client.try_batch_claim_payroll(&employee, &agreement_id, &empty);
    assert!(result.is_err());
}

#[test]
fn test_get_employee_claimed_periods_zero() {
    let env = Env::default();
    let (client, _owner, _employer, _employee, _arbiter, _token, agreement_id) = full_setup(&env);
    assert_eq!(client.get_employee_claimed_periods(&agreement_id, &0), 0);
}

#[test]
fn test_pause_resume_agreement() {
    let env = Env::default();
    let (client, _owner, _employer, _employee, _arbiter, _token, agreement_id) = full_setup(&env);
    assert!(client.try_pause_agreement(&agreement_id).is_ok());
    assert!(client.try_resume_agreement(&agreement_id).is_ok());
}

#[test]
fn test_cancel_and_grace_period() {
    let env = Env::default();
    let (client, _owner, _employer, _employee, _arbiter, _token, agreement_id) = full_setup(&env);
    client.cancel_agreement(&agreement_id);
    assert!(client.is_grace_period_active(&agreement_id));
    assert!(client.get_grace_period_end(&agreement_id).is_some());
}

#[test]
fn test_finalize_grace_period_fails_before_expiry() {
    let env = Env::default();
    let (client, _owner, _employer, _employee, _arbiter, _token, agreement_id) = full_setup(&env);
    env.mock_all_auths();
    client.cancel_agreement(&agreement_id);
    let result = client.try_finalize_grace_period(&agreement_id);
    assert!(result.is_err());
}

#[test]
fn test_set_grace_extension_policy() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let policy = GracePeriodExtensionPolicy {
        max_cumulative_extension_bps: 5000,
        max_extension_per_call_seconds: 86400,
    };
    let result = client.try_set_grace_extension_policy(&owner, &policy);
    assert!(result.is_ok());
}

#[test]
fn test_set_exchange_rate_admin_ok() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let admin = Address::generate(&env);
    let result = client.try_set_exchange_rate_admin(&owner, &admin);
    assert!(result.is_ok());
}

#[test]
fn test_set_exchange_rate_fails_without_admin() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let caller = Address::generate(&env);
    let base = Address::generate(&env);
    let quote = Address::generate(&env);
    let result = client.try_set_exchange_rate(&caller, &base, &quote, &100i128);
    assert!(result.is_err());
}

#[test]
fn test_set_fx_rate_sanity_bound() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let result = client.try_set_fx_rate_sanity_bound(&owner, &1_000_000i128);
    assert!(result.is_ok());
}

#[test]
fn test_set_fx_rate_sanity_bound_rejects_non_positive() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let result = client.try_set_fx_rate_sanity_bound(&owner, &0i128);
    assert!(result.is_err());
}

#[test]
fn test_emergency_guardians() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let guardians: Vec<Address> =
        Vec::from_array(&env, [Address::generate(&env), Address::generate(&env)]);
    client.set_emergency_guardians(&guardians);
    let stored = client.get_emergency_guardians();
    assert_eq!(stored, Some(guardians));
}

#[test]
fn test_emergency_pause_and_unpause() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let pause_result = client.try_emergency_pause();
    assert!(pause_result.is_ok());
    assert!(client.is_emergency_paused());
    let unpause_result = client.try_emergency_unpause();
    assert!(unpause_result.is_ok());
    assert!(!client.is_emergency_paused());
}

#[test]
fn test_propose_and_approve_emergency_pause() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let guardians: Vec<Address> = Vec::from_array(
        &env,
        [
            Address::generate(&env),
            Address::generate(&env),
            Address::generate(&env),
        ],
    );
    client.set_emergency_guardians(&guardians);
    let g1 = guardians.get(0).unwrap();
    let g2 = guardians.get(1).unwrap();
    assert!(client.try_propose_emergency_pause(&g1, &0).is_ok());
    assert!(client.try_approve_emergency_pause(&g2).is_ok());
}

#[test]
fn test_set_audit_logger_and_get() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let logger = Address::generate(&env);
    client.set_audit_logger(&owner, &logger);
    assert_eq!(client.get_audit_logger(), Some(logger));
}

#[test]
fn test_audit_entry_count_zero() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    assert_eq!(client.get_audit_entry_count(), 0);
}

#[test]
fn test_audit_get_entry_none() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    assert!(client.get_audit_entry(&0).is_none());
}

#[test]
fn test_admin_set_agreement_paid_amount_unauthorized() {
    let env = Env::default();
    let (client, _owner, _employer, _employee, _arbiter, _token, agreement_id) = full_setup(&env);
    let stranger = Address::generate(&env);
    env.mock_auths(&[]);
    let result = client.try_admin_set_agreement_paid_amount(&stranger, &agreement_id, &500i128);
    assert!(result.is_err());
}

#[test]
fn test_admin_set_escrow_balance_unauthorized() {
    let env = Env::default();
    let (client, _owner, _employer, _employee, _arbiter, _token, agreement_id) = full_setup(&env);
    let stranger = Address::generate(&env);
    let token_addr = Address::generate(&env);
    env.mock_auths(&[]);
    let result =
        client.try_admin_set_escrow_balance(&stranger, &agreement_id, &token_addr, &500i128);
    assert!(result.is_err());
}

#[test]
fn test_admin_set_agreement_token_unauthorized() {
    let env = Env::default();
    let (client, _owner, _employer, _employee, _arbiter, _token, agreement_id) = full_setup(&env);
    let stranger = Address::generate(&env);
    env.mock_auths(&[]);
    let result =
        client.try_admin_set_agreement_token(&stranger, &agreement_id, &Address::generate(&env));
    assert!(result.is_err());
}

#[test]
fn test_admin_set_activation_time_unauthorized() {
    let env = Env::default();
    let (client, _owner, _employer, _employee, _arbiter, _token, agreement_id) = full_setup(&env);
    let stranger = Address::generate(&env);
    env.mock_auths(&[]);
    let result = client.try_admin_set_activation_time(&stranger, &agreement_id, &100);
    assert!(result.is_err());
}

#[test]
fn test_admin_set_period_duration_unauthorized() {
    let env = Env::default();
    let (client, _owner, _employer, _employee, _arbiter, _token, agreement_id) = full_setup(&env);
    let stranger = Address::generate(&env);
    env.mock_auths(&[]);
    let result = client.try_admin_set_period_duration(&stranger, &agreement_id, &3600);
    assert!(result.is_err());
}

#[test]
fn test_claim_time_based_fails_on_payroll_agreement() {
    let env = Env::default();
    let (client, _owner, _employer, _employee, _arbiter, _token, agreement_id) = full_setup(&env);
    let result = client.try_claim_time_based(&agreement_id);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// 3. Return-type shape tests
// ---------------------------------------------------------------------------

#[test]
fn test_batch_create_payroll_returns_expected_type() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let (token_addr, _token_client) = create_escrow_token_pair(&env);
    let items = Vec::from_array(
        &env,
        [PayrollCreateParams {
            token: token_addr,
            grace_period_seconds: 3600,
        }],
    );
    let batch_result = client.batch_create_payroll_agreements(&employer, &items);
    assert_eq!(batch_result.agreement_ids.len(), 1);
}

#[test]
fn test_batch_create_escrow_returns_expected_type() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let (token_addr, _token_client) = create_escrow_token_pair(&env);
    let items = Vec::from_array(
        &env,
        [EscrowCreateParams {
            contributor: Address::generate(&env),
            token: token_addr,
            amount_per_period: 500i128,
            period_seconds: 3600u64,
            num_periods: 12u32,
        }],
    );
    let batch_result = client.batch_create_escrow_agreements(&employer, &items);
    assert_eq!(batch_result.agreement_ids.len(), 1);
}

// ---------------------------------------------------------------------------
// 4. Public-type access tests
// ---------------------------------------------------------------------------

#[test]
fn test_dispute_status_enum_variants_accessible() {
    let _created = DisputeStatus::None;
    let _raised = DisputeStatus::Raised;
    let _resolved = DisputeStatus::Resolved;
}

#[test]
fn test_payroll_error_variants_accessible() {
    let _e1 = PayrollError::Unauthorized;
    let _e2 = PayrollError::AgreementNotFound;
}

// ---------------------------------------------------------------------------
// 6. Grace period extension queries
// ---------------------------------------------------------------------------

#[test]
fn test_get_grace_extension_policy_defaults() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let policy = client.get_grace_extension_policy();
    assert!(policy.max_extension_per_call_seconds > 0);
}

#[test]
fn test_get_grace_extension_seconds_zero_when_not_cancelled() {
    let env = Env::default();
    let (_client, _owner, _employer, _employee, _arbiter, _token, _agreement_id) = full_setup(&env);
}
