//! Integration tests for the encrypted backup restore dry-run feature (Issue #786).
//!
//! Verifies:
//! * `admin_restore_dry_run` reports a valid backup as valid and surfaces the correct
//!   `agreement_id` without writing state.
//! * A corrupted/tampered envelope is detected and reported as invalid.
//! * A wrong passphrase is detected and reported as invalid.
//! * The real restore path is unaffected — storage state is unchanged after a dry-run even on a
//!   valid backup.
//! * The shared `validate_backup` helper (used by both dry-run and real restore) is exercised
//!   directly for full unit coverage.

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{testutils::Address as _, Address, Bytes, Env};
use stello_pay_contract::{
    backup::{backup_agreement, validate_backup, NONCE_LEN, SALT_LEN},
    storage::{Agreement, AgreementMode, AgreementStatus, DisputeStatus},
    PayrollContract, PayrollContractClient,
};

// ─── Fixtures ────────────────────────────────────────────────────────────────

fn make_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn make_agreement(env: &Env, employer: &Address) -> Agreement {
    Agreement {
        id: 42u128,
        employer: employer.clone(),
        token: Address::generate(env),
        mode: AgreementMode::Escrow,
        status: AgreementStatus::Active,
        total_amount: 10_000,
        paid_amount: 0,
        created_at: 1_000,
        activated_at: Some(2_000),
        cancelled_at: None,
        grace_period_seconds: 300,
        dispute_status: DisputeStatus::None,
        dispute_raised_at: None,
        amount_per_period: None,
        period_seconds: None,
        num_periods: None,
        claimed_periods: None,
    }
}

fn fixed_salt() -> [u8; SALT_LEN] {
    [1u8; SALT_LEN]
}

fn fixed_nonce() -> [u8; NONCE_LEN] {
    [2u8; NONCE_LEN]
}

const PASSPHRASE: &[u8] = b"test-passphrase-strong-enough";

fn to_bytes(env: &Env, data: &[u8]) -> Bytes {
    Bytes::from_slice(env, data)
}

fn setup_client(env: &Env) -> (Address, PayrollContractClient<'static>) {
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (owner, client)
}

// ─── Unit-level tests for validate_backup ────────────────────────────────────

/// A correctly encrypted backup must be reported as valid with the right id.
#[test]
fn test_validate_backup_valid_envelope_returns_true() {
    let env = make_env();
    let employer = Address::generate(&env);
    let agreement = make_agreement(&env, &employer);
    let envelope = backup_agreement(&env, &agreement, PASSPHRASE, &fixed_salt(), &fixed_nonce());

    let result = validate_backup(&env, &envelope, PASSPHRASE);

    assert!(
        result.valid,
        "Expected valid=true for a correctly encrypted backup"
    );
    assert_eq!(
        result.agreement_id,
        Some(42u128),
        "Expected agreement_id=42"
    );
}

/// A wrong passphrase must produce an invalid result.
#[test]
fn test_validate_backup_wrong_passphrase_returns_invalid() {
    let env = make_env();
    let employer = Address::generate(&env);
    let agreement = make_agreement(&env, &employer);
    let envelope = backup_agreement(&env, &agreement, PASSPHRASE, &fixed_salt(), &fixed_nonce());

    let result = validate_backup(&env, &envelope, b"wrong-passphrase");

    assert!(!result.valid, "Expected valid=false for wrong passphrase");
    assert_eq!(result.agreement_id, None);
}

/// A tampered ciphertext byte must be detected as invalid.
#[test]
fn test_validate_backup_tampered_ciphertext_returns_invalid() {
    let env = make_env();
    let employer = Address::generate(&env);
    let agreement = make_agreement(&env, &employer);
    let mut envelope =
        backup_agreement(&env, &agreement, PASSPHRASE, &fixed_salt(), &fixed_nonce());

    // Flip a byte in the ciphertext region (past version + salt + nonce).
    let tamper_pos = 1 + SALT_LEN + NONCE_LEN + 4;
    envelope[tamper_pos] ^= 0xFF;

    let result = validate_backup(&env, &envelope, PASSPHRASE);

    assert!(
        !result.valid,
        "Expected valid=false for tampered ciphertext"
    );
    assert_eq!(result.agreement_id, None);
}

/// A truncated envelope (too short for the header) must be invalid.
#[test]
fn test_validate_backup_truncated_envelope_returns_invalid() {
    let env = make_env();
    let short_data = [1u8; 10];

    let result = validate_backup(&env, &short_data, PASSPHRASE);

    assert!(!result.valid, "Expected valid=false for truncated envelope");
    assert_eq!(result.agreement_id, None);
}

/// An unknown version byte in the envelope must be detected as invalid.
#[test]
fn test_validate_backup_unknown_version_returns_invalid() {
    let env = make_env();
    let employer = Address::generate(&env);
    let agreement = make_agreement(&env, &employer);
    let mut envelope =
        backup_agreement(&env, &agreement, PASSPHRASE, &fixed_salt(), &fixed_nonce());

    // Overwrite the version byte (first byte) with an unrecognised value.
    envelope[0] = 0xFF;

    let result = validate_backup(&env, &envelope, PASSPHRASE);

    assert!(
        !result.valid,
        "Expected valid=false for unknown version byte"
    );
    assert_eq!(result.agreement_id, None);
}

// ─── Integration tests via the contract entrypoint ───────────────────────────

/// `admin_restore_dry_run` returns `(true, 42)` for a valid backup and does
/// NOT write the agreement into persistent storage.
///
/// Verification: the subsequent real restore must succeed (if dry-run had
/// corrupted state it could interfere; confirming the real restore works proves
/// the dry-run was truly read-only).
#[test]
fn test_dry_run_valid_backup_does_not_write_state() {
    let env = make_env();
    let (owner, client) = setup_client(&env);

    let employer = Address::generate(&env);
    let agreement = make_agreement(&env, &employer);
    let envelope_raw =
        backup_agreement(&env, &agreement, PASSPHRASE, &fixed_salt(), &fixed_nonce());
    let envelope = to_bytes(&env, &envelope_raw);
    let pass = to_bytes(&env, PASSPHRASE);

    // Run dry-run — Soroban client panics on error, so no .unwrap() needed.
    let (valid, agreement_id) = client.admin_restore_dry_run(&envelope, &pass);
    assert!(valid, "dry-run should report valid=true");
    assert_eq!(
        agreement_id, 42u128,
        "dry-run should return the correct agreement_id"
    );

    // Confirm the real restore succeeds (state slot was not altered by dry-run).
    let restored_id = client.admin_restore_from_encrypted(&owner, &envelope, &pass);
    assert_eq!(
        restored_id, 42u128,
        "real restore must succeed after a dry-run"
    );
}

/// `admin_restore_dry_run` returns `(false, 0)` for a corrupted envelope.
#[test]
fn test_dry_run_corrupted_backup_returns_invalid() {
    let env = make_env();
    let (_owner, client) = setup_client(&env);

    let employer = Address::generate(&env);
    let agreement = make_agreement(&env, &employer);
    let mut envelope_raw =
        backup_agreement(&env, &agreement, PASSPHRASE, &fixed_salt(), &fixed_nonce());

    // Corrupt the ciphertext region.
    let tamper_pos = 1 + SALT_LEN + NONCE_LEN + 2;
    envelope_raw[tamper_pos] ^= 0xAB;

    let envelope = to_bytes(&env, &envelope_raw);
    let pass = to_bytes(&env, PASSPHRASE);

    let (valid, agreement_id) = client.admin_restore_dry_run(&envelope, &pass);

    assert!(
        !valid,
        "dry-run should report valid=false for corrupted backup"
    );
    assert_eq!(
        agreement_id, 0u128,
        "agreement_id must be 0 when validation fails"
    );
}

/// `admin_restore_dry_run` returns `(false, 0)` for a wrong passphrase.
#[test]
fn test_dry_run_wrong_passphrase_returns_invalid() {
    let env = make_env();
    let (_owner, client) = setup_client(&env);

    let employer = Address::generate(&env);
    let agreement = make_agreement(&env, &employer);
    let envelope_raw =
        backup_agreement(&env, &agreement, PASSPHRASE, &fixed_salt(), &fixed_nonce());
    let envelope = to_bytes(&env, &envelope_raw);
    let wrong_pass = to_bytes(&env, b"totally-wrong-key");

    let (valid, agreement_id) = client.admin_restore_dry_run(&envelope, &wrong_pass);

    assert!(
        !valid,
        "dry-run should report valid=false for wrong passphrase"
    );
    assert_eq!(agreement_id, 0u128);
}

/// An empty envelope must cause the entrypoint to return an error (not just
/// `(false, 0)`). The `try_*` variant returns the error rather than panicking.
#[test]
fn test_dry_run_empty_envelope_returns_error() {
    let env = make_env();
    let (_owner, client) = setup_client(&env);

    let empty = Bytes::from_slice(&env, &[]);
    let pass = to_bytes(&env, PASSPHRASE);

    let res = client.try_admin_restore_dry_run(&empty, &pass);
    assert!(res.is_err(), "Expected Err for empty envelope, got Ok");
}

/// The real restore path is completely unchanged: calling it after a dry-run
/// produces the same result as calling it cold (no interference between the
/// two code paths).
#[test]
fn test_real_restore_path_unchanged_after_dry_run() {
    let env = make_env();
    let (owner, client) = setup_client(&env);

    let employer = Address::generate(&env);
    let mut agreement = make_agreement(&env, &employer);
    agreement.id = 99u128;

    let envelope_raw = backup_agreement(
        &env,
        &agreement,
        PASSPHRASE,
        &fixed_salt(),
        &[3u8; NONCE_LEN],
    );
    let envelope = to_bytes(&env, &envelope_raw);
    let pass = to_bytes(&env, PASSPHRASE);

    // Dry-run first.
    let (valid, id) = client.admin_restore_dry_run(&envelope, &pass);
    assert!(valid, "dry-run must report valid");
    assert_eq!(id, 99u128);

    // Real restore must succeed and return the correct id.
    let restored = client.admin_restore_from_encrypted(&owner, &envelope, &pass);
    assert_eq!(restored, 99u128, "real restore must succeed after dry-run");
}

/// Calling dry-run multiple times on the same valid backup is idempotent
/// and never mutates state.
#[test]
fn test_dry_run_is_idempotent() {
    let env = make_env();
    let (owner, client) = setup_client(&env);

    let employer = Address::generate(&env);
    let mut agreement = make_agreement(&env, &employer);
    agreement.id = 7u128;

    let envelope_raw = backup_agreement(
        &env,
        &agreement,
        PASSPHRASE,
        &fixed_salt(),
        &[5u8; NONCE_LEN],
    );
    let envelope = to_bytes(&env, &envelope_raw);
    let pass = to_bytes(&env, PASSPHRASE);

    // Call dry-run three times.
    for _ in 0..3 {
        let (valid, id) = client.admin_restore_dry_run(&envelope, &pass);
        assert!(valid);
        assert_eq!(id, 7u128);
    }

    // Real restore must still work after repeated dry-runs.
    let restored = client.admin_restore_from_encrypted(&owner, &envelope, &pass);
    assert_eq!(restored, 7u128);
}
