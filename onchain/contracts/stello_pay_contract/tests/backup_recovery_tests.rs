//! # Encrypted Backup & Recovery Integration Tests
//!
//! Exercises the full backup lifecycle:
//!   1. Serialise an `Agreement` to bytes.
//!   2. Encrypt with AES-256-GCM (PBKDF2-derived key).
//!   3. Decrypt and deserialise back to an `Agreement`.
//!   4. Restore via the admin on-chain entrypoints.
//!
//! Also covers `AgreementBalance` snapshots, error paths, and security
//! properties (wrong key, tampered ciphertext, truncated envelope).

#![cfg(test)]
#![allow(deprecated)]

extern crate alloc;
use alloc::vec::Vec as StdVec;

use soroban_sdk::{testutils::Address as _, Address, Bytes, Env};
use stello_pay_contract::backup::{
    backup_agreement, decrypt_backup, deserialize_agreement, encrypt_backup, restore_agreement,
    serialize_agreement, AgreementBalance, BackupError, BACKUP_VERSION, NONCE_LEN, SALT_LEN,
};
use stello_pay_contract::storage::{
    Agreement, AgreementMode, AgreementStatus, DisputeStatus, StorageKey,
};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_contract(env: &Env) -> (Address, PayrollContractClient) {
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (owner, client)
}

/// Build a minimal but fully-populated `Agreement` for testing.
fn make_agreement(env: &Env) -> Agreement {
    Agreement {
        id: 42,
        employer: Address::generate(env),
        token: Address::generate(env),
        mode: AgreementMode::Payroll,
        status: AgreementStatus::Active,
        total_amount: 1_000_000,
        paid_amount: 250_000,
        created_at: 1_700_000_000,
        activated_at: Some(1_700_001_000),
        cancelled_at: None,
        grace_period_seconds: 604_800,
        dispute_status: DisputeStatus::None,
        dispute_raised_at: None,
        amount_per_period: None,
        period_seconds: None,
        num_periods: None,
        claimed_periods: None,
    }
}

/// Build an escrow-mode agreement with all optional fields populated.
fn make_escrow_agreement(env: &Env) -> Agreement {
    Agreement {
        id: 99,
        employer: Address::generate(env),
        token: Address::generate(env),
        mode: AgreementMode::Escrow,
        status: AgreementStatus::Active,
        total_amount: 5_000_000,
        paid_amount: 1_000_000,
        created_at: 1_710_000_000,
        activated_at: Some(1_710_001_000),
        cancelled_at: None,
        grace_period_seconds: 2_592_000,
        dispute_status: DisputeStatus::None,
        dispute_raised_at: None,
        amount_per_period: Some(500_000),
        period_seconds: Some(2_592_000),
        num_periods: Some(10),
        claimed_periods: Some(2),
    }
}

// ---------------------------------------------------------------------------
// 1. Serialisation round-trip
// ---------------------------------------------------------------------------

/// Serialise a payroll `Agreement` and deserialise it back; all fields must
/// match exactly.
#[test]
fn test_serialize_deserialize_payroll_agreement() {
    let env = create_env();
    let original = make_agreement(&env);

    let bytes = serialize_agreement(&env, &original);
    assert!(!bytes.is_empty(), "serialised bytes must not be empty");

    let recovered = deserialize_agreement(&env, &bytes).expect("deserialisation must succeed");

    assert_eq!(recovered.id, original.id);
    assert_eq!(recovered.employer, original.employer);
    assert_eq!(recovered.token, original.token);
    assert_eq!(recovered.mode, original.mode);
    assert_eq!(recovered.status, original.status);
    assert_eq!(recovered.total_amount, original.total_amount);
    assert_eq!(recovered.paid_amount, original.paid_amount);
    assert_eq!(recovered.created_at, original.created_at);
    assert_eq!(recovered.activated_at, original.activated_at);
    assert_eq!(recovered.cancelled_at, original.cancelled_at);
    assert_eq!(recovered.grace_period_seconds, original.grace_period_seconds);
    assert_eq!(recovered.dispute_status, original.dispute_status);
    assert_eq!(recovered.dispute_raised_at, original.dispute_raised_at);
    assert_eq!(recovered.amount_per_period, original.amount_per_period);
    assert_eq!(recovered.period_seconds, original.period_seconds);
    assert_eq!(recovered.num_periods, original.num_periods);
    assert_eq!(recovered.claimed_periods, original.claimed_periods);
}

/// Serialise an escrow `Agreement` (all optional fields populated) and verify
/// round-trip fidelity.
#[test]
fn test_serialize_deserialize_escrow_agreement() {
    let env = create_env();
    let original = make_escrow_agreement(&env);

    let bytes = serialize_agreement(&env, &original);
    let recovered = deserialize_agreement(&env, &bytes).expect("deserialisation must succeed");

    assert_eq!(recovered.id, original.id);
    assert_eq!(recovered.mode, AgreementMode::Escrow);
    assert_eq!(recovered.amount_per_period, Some(500_000));
    assert_eq!(recovered.num_periods, Some(10));
    assert_eq!(recovered.claimed_periods, Some(2));
}

/// Verify that a cancelled agreement with `cancelled_at` set survives the
/// round-trip.
#[test]
fn test_serialize_cancelled_agreement() {
    let env = create_env();
    let mut agreement = make_agreement(&env);
    agreement.status = AgreementStatus::Cancelled;
    agreement.cancelled_at = Some(1_700_100_000);

    let bytes = serialize_agreement(&env, &agreement);
    let recovered = deserialize_agreement(&env, &bytes).unwrap();

    assert_eq!(recovered.status, AgreementStatus::Cancelled);
    assert_eq!(recovered.cancelled_at, Some(1_700_100_000));
}

/// Verify that a disputed agreement with `dispute_raised_at` set survives the
/// round-trip.
#[test]
fn test_serialize_disputed_agreement() {
    let env = create_env();
    let mut agreement = make_agreement(&env);
    agreement.status = AgreementStatus::Disputed;
    agreement.dispute_status = DisputeStatus::Raised;
    agreement.dispute_raised_at = Some(1_700_050_000);

    let bytes = serialize_agreement(&env, &agreement);
    let recovered = deserialize_agreement(&env, &bytes).unwrap();

    assert_eq!(recovered.dispute_status, DisputeStatus::Raised);
    assert_eq!(recovered.dispute_raised_at, Some(1_700_050_000));
}

// ---------------------------------------------------------------------------
// 2. Encryption / Decryption round-trip
// ---------------------------------------------------------------------------

/// Full encrypt → decrypt round-trip with a known passphrase.
#[test]
fn test_encrypt_decrypt_round_trip() {
    let env = create_env();
    let passphrase = b"super-secret-passphrase-for-test";
    let plaintext = b"hello backup world";

    let envelope = encrypt_backup(&env, plaintext, passphrase);

    // Envelope must start with the version byte.
    assert_eq!(envelope[0], BACKUP_VERSION);
    // Envelope must be longer than version + salt + nonce + tag.
    assert!(envelope.len() > 1 + SALT_LEN + NONCE_LEN + 16);

    let recovered = decrypt_backup(&envelope, passphrase).expect("decryption must succeed");
    assert_eq!(recovered, plaintext);
}

/// `backup_agreement` + `restore_agreement` round-trip with real `Agreement`
/// data — the primary integration test for the backup lifecycle.
#[test]
fn test_backup_restore_agreement_round_trip() {
    let env = create_env();
    let original = make_agreement(&env);
    let passphrase = b"payroll-backup-key-2024";

    let envelope = backup_agreement(&env, &original, passphrase);
    let recovered = restore_agreement(&env, &envelope, passphrase)
        .expect("restore must succeed with correct passphrase");

    assert_eq!(recovered.id, original.id);
    assert_eq!(recovered.employer, original.employer);
    assert_eq!(recovered.token, original.token);
    assert_eq!(recovered.total_amount, original.total_amount);
    assert_eq!(recovered.paid_amount, original.paid_amount);
    assert_eq!(recovered.status, original.status);
    assert_eq!(recovered.mode, original.mode);
}

/// Escrow agreement backup/restore round-trip.
#[test]
fn test_backup_restore_escrow_agreement_round_trip() {
    let env = create_env();
    let original = make_escrow_agreement(&env);
    let passphrase = b"escrow-backup-key-2024";

    let envelope = backup_agreement(&env, &original, passphrase);
    let recovered = restore_agreement(&env, &envelope, passphrase).unwrap();

    assert_eq!(recovered.id, original.id);
    assert_eq!(recovered.mode, AgreementMode::Escrow);
    assert_eq!(recovered.amount_per_period, original.amount_per_period);
    assert_eq!(recovered.num_periods, original.num_periods);
    assert_eq!(recovered.claimed_periods, original.claimed_periods);
}

// ---------------------------------------------------------------------------
// 3. AgreementBalance snapshot
// ---------------------------------------------------------------------------

/// Verify that `AgreementBalance` can be constructed and its fields are
/// accessible (struct integrity test).
#[test]
fn test_agreement_balance_snapshot() {
    let env = create_env();
    let token = Address::generate(&env);

    let balance = AgreementBalance {
        agreement_id: 42,
        token: token.clone(),
        escrow_balance: 750_000,
        paid_amount: 250_000,
    };

    assert_eq!(balance.agreement_id, 42);
    assert_eq!(balance.token, token);
    assert_eq!(balance.escrow_balance, 750_000);
    assert_eq!(balance.paid_amount, 250_000);
}

/// Verify that a zero-balance snapshot is valid (edge case).
#[test]
fn test_agreement_balance_zero_values() {
    let env = create_env();
    let balance = AgreementBalance {
        agreement_id: 1,
        token: Address::generate(&env),
        escrow_balance: 0,
        paid_amount: 0,
    };
    assert_eq!(balance.escrow_balance, 0);
    assert_eq!(balance.paid_amount, 0);
}

// ---------------------------------------------------------------------------
// 4. Security: wrong key / tampered ciphertext
// ---------------------------------------------------------------------------

/// Decryption with the wrong passphrase must fail with `DecryptionFailed`.
#[test]
fn test_wrong_passphrase_fails() {
    let env = create_env();
    let original = make_agreement(&env);
    let correct = b"correct-passphrase";
    let wrong = b"wrong-passphrase!!";

    let envelope = backup_agreement(&env, &original, correct);
    let result = restore_agreement(&env, &envelope, wrong);

    assert_eq!(result, Err(BackupError::DecryptionFailed));
}

/// Flipping a single byte in the ciphertext portion must cause decryption to
/// fail (AES-GCM authentication tag check).
#[test]
fn test_tampered_ciphertext_fails() {
    let env = create_env();
    let original = make_agreement(&env);
    let passphrase = b"tamper-test-key";

    let mut envelope = backup_agreement(&env, &original, passphrase);

    // Flip a byte in the ciphertext (after version + salt + nonce).
    let flip_pos = 1 + SALT_LEN + NONCE_LEN + 5;
    envelope[flip_pos] ^= 0xFF;

    let result = restore_agreement(&env, &envelope, passphrase);
    assert_eq!(result, Err(BackupError::DecryptionFailed));
}

/// A truncated envelope (too short to contain version + salt + nonce + tag)
/// must return `BufferTooShort`.
#[test]
fn test_truncated_envelope_fails() {
    let passphrase = b"truncation-test";
    let short_envelope = &[BACKUP_VERSION, 0x01, 0x02]; // far too short

    let result = decrypt_backup(short_envelope, passphrase);
    assert_eq!(result, Err(BackupError::BufferTooShort));
}

/// An envelope with an unknown version byte must return `UnknownVersion`.
#[test]
fn test_unknown_version_fails() {
    let env = create_env();
    let passphrase = b"version-test";
    let original = make_agreement(&env);

    let mut envelope = backup_agreement(&env, &original, passphrase);
    envelope[0] = 0xFF; // corrupt version byte

    let result = decrypt_backup(&envelope, passphrase);
    assert_eq!(result, Err(BackupError::UnknownVersion));
}

/// Flipping a byte in the salt portion changes the derived key, so decryption
/// must fail.
#[test]
fn test_tampered_salt_fails() {
    let env = create_env();
    let original = make_agreement(&env);
    let passphrase = b"salt-tamper-test";

    let mut envelope = backup_agreement(&env, &original, passphrase);
    // Salt starts at byte 1.
    envelope[1] ^= 0xAB;

    let result = restore_agreement(&env, &envelope, passphrase);
    assert_eq!(result, Err(BackupError::DecryptionFailed));
}

/// Flipping a byte in the nonce portion must cause decryption to fail.
#[test]
fn test_tampered_nonce_fails() {
    let env = create_env();
    let original = make_agreement(&env);
    let passphrase = b"nonce-tamper-test";

    let mut envelope = backup_agreement(&env, &original, passphrase);
    // Nonce starts at byte 1 + SALT_LEN.
    envelope[1 + SALT_LEN] ^= 0x55;

    let result = restore_agreement(&env, &envelope, passphrase);
    assert_eq!(result, Err(BackupError::DecryptionFailed));
}

// ---------------------------------------------------------------------------
// 5. On-chain admin entrypoints
// ---------------------------------------------------------------------------

/// `admin_restore_agreement` writes the agreement back into persistent storage
/// and `get_agreement` returns the restored data.
#[test]
fn test_admin_restore_agreement_entrypoint() {
    let env = create_env();
    let (owner, client) = setup_contract(&env);

    // Create an agreement on-chain so the ID counter is initialised.
    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let agreement_id = client.create_payroll_agreement(&employer, &token, &604_800u64);

    // Fetch the live agreement and mutate paid_amount to simulate a backup
    // taken at a later point in time.
    let mut snapshot = client.get_agreement(&agreement_id).unwrap();
    snapshot.paid_amount = 999_999;

    // Restore via the admin entrypoint.
    client.admin_restore_agreement(&owner, &snapshot);

    // Verify the restored state.
    let restored = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(restored.paid_amount, 999_999);
    assert_eq!(restored.id, agreement_id);
}

/// `admin_restore_from_encrypted` decrypts an envelope on-chain and writes the
/// agreement back — full end-to-end round-trip through the contract interface.
#[test]
fn test_admin_restore_from_encrypted_entrypoint() {
    let env = create_env();
    let (owner, client) = setup_contract(&env);

    // Create an agreement on-chain.
    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let agreement_id = client.create_payroll_agreement(&employer, &token, &604_800u64);

    // Fetch and encrypt the agreement off-chain (simulated here in the test).
    let live = client.get_agreement(&agreement_id).unwrap();
    let passphrase = b"on-chain-restore-test-key";
    let envelope_vec = backup_agreement(&env, &live, passphrase);

    // Convert to Soroban `Bytes`.
    let envelope_bytes = Bytes::from_slice(&env, &envelope_vec);
    let pass_bytes = Bytes::from_slice(&env, passphrase);

    // Restore via the encrypted entrypoint.
    let restored_id = client
        .admin_restore_from_encrypted(&owner, &envelope_bytes, &pass_bytes)
        .unwrap();

    assert_eq!(restored_id, agreement_id);

    // Verify the agreement is still readable.
    let restored = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(restored.id, agreement_id);
    assert_eq!(restored.employer, employer);
    assert_eq!(restored.token, token);
}

/// Non-owner callers must be rejected by `admin_restore_agreement`.
#[test]
#[should_panic]
fn test_admin_restore_unauthorized_caller() {
    let env = create_env();
    let (_owner, client) = setup_contract(&env);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let agreement_id = client.create_payroll_agreement(&employer, &token, &604_800u64);

    let snapshot = client.get_agreement(&agreement_id).unwrap();
    let attacker = Address::generate(&env);

    // This must panic / return Unauthorized.
    client.admin_restore_agreement(&attacker, &snapshot);
}

/// Non-owner callers must be rejected by `admin_restore_from_encrypted`.
#[test]
#[should_panic]
fn test_admin_restore_from_encrypted_unauthorized() {
    let env = create_env();
    let (_owner, client) = setup_contract(&env);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let agreement_id = client.create_payroll_agreement(&employer, &token, &604_800u64);

    let live = client.get_agreement(&agreement_id).unwrap();
    let passphrase = b"attacker-key";
    let envelope_vec = backup_agreement(&env, &live, passphrase);
    let envelope_bytes = Bytes::from_slice(&env, &envelope_vec);
    let pass_bytes = Bytes::from_slice(&env, passphrase);

    let attacker = Address::generate(&env);
    client.admin_restore_from_encrypted(&attacker, &envelope_bytes, &pass_bytes);
}

// ---------------------------------------------------------------------------
// 6. Multiple agreements backup
// ---------------------------------------------------------------------------

/// Back up and restore multiple agreements independently; IDs must not
/// collide and each agreement must restore to its own correct state.
#[test]
fn test_multiple_agreements_backup_restore() {
    let env = create_env();
    let passphrase = b"multi-agreement-backup-key";

    let mut agreements: StdVec<Agreement> = StdVec::new();
    for i in 0u128..5 {
        let mut a = make_agreement(&env);
        a.id = i + 1;
        a.total_amount = (i + 1) as i128 * 100_000;
        agreements.push(a);
    }

    // Encrypt all.
    let envelopes: StdVec<StdVec<u8>> = agreements
        .iter()
        .map(|a| backup_agreement(&env, a, passphrase))
        .collect();

    // Decrypt all and verify.
    for (i, envelope) in envelopes.iter().enumerate() {
        let recovered = restore_agreement(&env, envelope, passphrase).unwrap();
        assert_eq!(recovered.id, agreements[i].id);
        assert_eq!(recovered.total_amount, agreements[i].total_amount);
    }
}

// ---------------------------------------------------------------------------
// 7. Edge cases
// ---------------------------------------------------------------------------

/// An agreement with `id = 0` (edge case) must survive the round-trip.
#[test]
fn test_agreement_id_zero_round_trip() {
    let env = create_env();
    let mut agreement = make_agreement(&env);
    agreement.id = 0;

    let passphrase = b"edge-case-zero-id";
    let envelope = backup_agreement(&env, &agreement, passphrase);
    let recovered = restore_agreement(&env, &envelope, passphrase).unwrap();
    assert_eq!(recovered.id, 0);
}

/// An agreement with maximum `i128` amounts must not overflow during
/// serialisation/deserialisation.
#[test]
fn test_max_amount_round_trip() {
    let env = create_env();
    let mut agreement = make_agreement(&env);
    agreement.total_amount = i128::MAX;
    agreement.paid_amount = i128::MAX / 2;

    let passphrase = b"max-amount-test";
    let envelope = backup_agreement(&env, &agreement, passphrase);
    let recovered = restore_agreement(&env, &envelope, passphrase).unwrap();
    assert_eq!(recovered.total_amount, i128::MAX);
    assert_eq!(recovered.paid_amount, i128::MAX / 2);
}

/// Two successive backups of the same agreement with the same passphrase must
/// produce different envelopes (nonce / salt randomness).
#[test]
fn test_two_backups_produce_different_envelopes() {
    let env = create_env();
    let agreement = make_agreement(&env);
    let passphrase = b"nonce-uniqueness-test";

    let env2 = create_env();
    let envelope1 = backup_agreement(&env, &agreement, passphrase);
    let envelope2 = backup_agreement(&env2, &agreement, passphrase);

    // With overwhelming probability the random salt/nonce differ.
    assert_ne!(
        envelope1, envelope2,
        "two backups must produce distinct envelopes"
    );
}

/// Verify that the `Completed` status survives the round-trip.
#[test]
fn test_completed_status_round_trip() {
    let env = create_env();
    let mut agreement = make_agreement(&env);
    agreement.status = AgreementStatus::Completed;

    let passphrase = b"completed-status-test";
    let envelope = backup_agreement(&env, &agreement, passphrase);
    let recovered = restore_agreement(&env, &envelope, passphrase).unwrap();
    assert_eq!(recovered.status, AgreementStatus::Completed);
}

/// Verify that the `Paused` status survives the round-trip.
#[test]
fn test_paused_status_round_trip() {
    let env = create_env();
    let mut agreement = make_agreement(&env);
    agreement.status = AgreementStatus::Paused;

    let passphrase = b"paused-status-test";
    let envelope = backup_agreement(&env, &agreement, passphrase);
    let recovered = restore_agreement(&env, &envelope, passphrase).unwrap();
    assert_eq!(recovered.status, AgreementStatus::Paused);
}
