//! # Encrypted Backup & Recovery Integration Tests

#![cfg(test)]
#![allow(deprecated)]

extern crate alloc;
use alloc::vec::Vec as StdVec;

use soroban_sdk::{testutils::Address as _, Address, Bytes, Env};
use stello_pay_contract::backup::{
    backup_agreement, decrypt_backup, deserialize_agreement, encrypt_backup, restore_agreement,
    serialize_agreement, AgreementBalance, BackupError, BACKUP_VERSION, NONCE_LEN, SALT_LEN,
};
use stello_pay_contract::storage::{Agreement, AgreementMode, AgreementStatus, DisputeStatus};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// Fixed test vectors — deterministic, no PRNG needed outside a contract.
const TEST_SALT: [u8; SALT_LEN] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
const TEST_NONCE: [u8; NONCE_LEN] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
const TEST_SALT_2: [u8; SALT_LEN] = [16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1];
const TEST_NONCE_2: [u8; NONCE_LEN] = [12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1];

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_contract(env: &Env) -> (Address, PayrollContractClient<'_>) {
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (owner, client)
}

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

#[test]
fn test_serialize_deserialize_payroll_agreement() {
    let env = create_env();
    let original = make_agreement(&env);

    let bytes = serialize_agreement(&env, &original);
    assert!(!bytes.is_empty());

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
    assert_eq!(
        recovered.grace_period_seconds,
        original.grace_period_seconds
    );
    assert_eq!(recovered.dispute_status, original.dispute_status);
    assert_eq!(recovered.dispute_raised_at, original.dispute_raised_at);
    assert_eq!(recovered.amount_per_period, original.amount_per_period);
    assert_eq!(recovered.period_seconds, original.period_seconds);
    assert_eq!(recovered.num_periods, original.num_periods);
    assert_eq!(recovered.claimed_periods, original.claimed_periods);
}

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

#[test]
fn test_encrypt_decrypt_round_trip() {
    let passphrase = b"super-secret-passphrase-for-test";
    let plaintext = b"hello backup world";

    let envelope = encrypt_backup(plaintext, passphrase, &TEST_SALT, &TEST_NONCE);

    assert_eq!(envelope[0], BACKUP_VERSION);
    assert!(envelope.len() > 1 + SALT_LEN + NONCE_LEN + 16);

    let recovered = decrypt_backup(&envelope, passphrase).expect("decryption must succeed");
    assert_eq!(recovered, plaintext);
}

#[test]
fn test_backup_restore_agreement_round_trip() {
    let env = create_env();
    let original = make_agreement(&env);
    let passphrase = b"payroll-backup-key-2024";

    let envelope = backup_agreement(&env, &original, passphrase, &TEST_SALT, &TEST_NONCE);
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

#[test]
fn test_backup_restore_escrow_agreement_round_trip() {
    let env = create_env();
    let original = make_escrow_agreement(&env);
    let passphrase = b"escrow-backup-key-2024";

    let envelope = backup_agreement(&env, &original, passphrase, &TEST_SALT, &TEST_NONCE);
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

#[test]
fn test_wrong_passphrase_fails() {
    let env = create_env();
    let original = make_agreement(&env);
    let correct = b"correct-passphrase";
    let wrong = b"wrong-passphrase!!";

    let envelope = backup_agreement(&env, &original, correct, &TEST_SALT, &TEST_NONCE);
    let err = restore_agreement(&env, &envelope, wrong).unwrap_err();
    assert_eq!(err, BackupError::DecryptionFailed);
}

#[test]
fn test_tampered_ciphertext_fails() {
    let env = create_env();
    let original = make_agreement(&env);
    let passphrase = b"tamper-test-key";

    let mut envelope = backup_agreement(&env, &original, passphrase, &TEST_SALT, &TEST_NONCE);
    let flip_pos = 1 + SALT_LEN + NONCE_LEN + 5;
    envelope[flip_pos] ^= 0xFF;

    let err = restore_agreement(&env, &envelope, passphrase).unwrap_err();
    assert_eq!(err, BackupError::DecryptionFailed);
}

#[test]
fn test_truncated_envelope_fails() {
    let passphrase = b"truncation-test";
    let short_envelope = &[BACKUP_VERSION, 0x01, 0x02];

    let result = decrypt_backup(short_envelope, passphrase);
    assert_eq!(result, Err(BackupError::BufferTooShort));
}

#[test]
fn test_unknown_version_fails() {
    let env = create_env();
    let passphrase = b"version-test";
    let original = make_agreement(&env);

    let mut envelope = backup_agreement(&env, &original, passphrase, &TEST_SALT, &TEST_NONCE);
    envelope[0] = 0xFF;

    let result = decrypt_backup(&envelope, passphrase);
    assert_eq!(result, Err(BackupError::UnknownVersion));
}

#[test]
fn test_tampered_salt_fails() {
    let env = create_env();
    let original = make_agreement(&env);
    let passphrase = b"salt-tamper-test";

    let mut envelope = backup_agreement(&env, &original, passphrase, &TEST_SALT, &TEST_NONCE);
    envelope[1] ^= 0xAB;

    let err = restore_agreement(&env, &envelope, passphrase).unwrap_err();
    assert_eq!(err, BackupError::DecryptionFailed);
}

#[test]
fn test_tampered_nonce_fails() {
    let env = create_env();
    let original = make_agreement(&env);
    let passphrase = b"nonce-tamper-test";

    let mut envelope = backup_agreement(&env, &original, passphrase, &TEST_SALT, &TEST_NONCE);
    envelope[1 + SALT_LEN] ^= 0x55;

    let err = restore_agreement(&env, &envelope, passphrase).unwrap_err();
    assert_eq!(err, BackupError::DecryptionFailed);
}

// ---------------------------------------------------------------------------
// 5. On-chain admin entrypoints
// ---------------------------------------------------------------------------

#[test]
fn test_admin_restore_agreement_entrypoint() {
    let env = create_env();
    let (owner, client) = setup_contract(&env);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let agreement_id = client.create_payroll_agreement(&employer, &token, &604_800u64);

    let mut snapshot = client.get_agreement(&agreement_id).unwrap();
    snapshot.paid_amount = 999_999;

    client.admin_restore_agreement(&owner, &snapshot);

    let restored = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(restored.paid_amount, 999_999);
    assert_eq!(restored.id, agreement_id);
}

#[test]
fn test_admin_restore_from_encrypted_entrypoint() {
    let env = create_env();
    let (owner, client) = setup_contract(&env);

    let token = Address::generate(&env);
    let employer = Address::generate(&env);
    let agreement_id = client.create_payroll_agreement(&employer, &token, &604_800u64);

    let live = client.get_agreement(&agreement_id).unwrap();
    let passphrase = b"on-chain-restore-test-key";
    // Encrypt off-chain with fixed test vectors; the on-chain entrypoint only decrypts.
    let envelope_vec = backup_agreement(&env, &live, passphrase, &TEST_SALT, &TEST_NONCE);

    let envelope_bytes = Bytes::from_slice(&env, &envelope_vec);
    let pass_bytes = Bytes::from_slice(&env, passphrase);

    let restored_id = client.admin_restore_from_encrypted(&owner, &envelope_bytes, &pass_bytes);

    assert_eq!(restored_id, agreement_id);

    let restored = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(restored.id, agreement_id);
    assert_eq!(restored.employer, employer);
    assert_eq!(restored.token, token);
}

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
    client.admin_restore_agreement(&attacker, &snapshot);
}

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
    let envelope_vec = backup_agreement(&env, &live, passphrase, &TEST_SALT, &TEST_NONCE);
    let envelope_bytes = Bytes::from_slice(&env, &envelope_vec);
    let pass_bytes = Bytes::from_slice(&env, passphrase);

    let attacker = Address::generate(&env);
    client.admin_restore_from_encrypted(&attacker, &envelope_bytes, &pass_bytes);
}

// ---------------------------------------------------------------------------
// 6. Multiple agreements
// ---------------------------------------------------------------------------

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

    // Each agreement uses a distinct nonce to avoid reuse.
    let envelopes: StdVec<StdVec<u8>> = agreements
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let mut nonce = TEST_NONCE;
            nonce[0] = i as u8; // unique per agreement
            backup_agreement(&env, a, passphrase, &TEST_SALT, &nonce)
        })
        .collect();

    for (i, envelope) in envelopes.iter().enumerate() {
        let recovered = restore_agreement(&env, envelope, passphrase).unwrap();
        assert_eq!(recovered.id, agreements[i].id);
        assert_eq!(recovered.total_amount, agreements[i].total_amount);
    }
}

// ---------------------------------------------------------------------------
// 7. Edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_agreement_id_zero_round_trip() {
    let env = create_env();
    let mut agreement = make_agreement(&env);
    agreement.id = 0;

    let passphrase = b"edge-case-zero-id";
    let envelope = backup_agreement(&env, &agreement, passphrase, &TEST_SALT, &TEST_NONCE);
    let recovered = restore_agreement(&env, &envelope, passphrase).unwrap();
    assert_eq!(recovered.id, 0);
}

#[test]
fn test_max_amount_round_trip() {
    let env = create_env();
    let mut agreement = make_agreement(&env);
    agreement.total_amount = i128::MAX;
    agreement.paid_amount = i128::MAX / 2;

    let passphrase = b"max-amount-test";
    let envelope = backup_agreement(&env, &agreement, passphrase, &TEST_SALT, &TEST_NONCE);
    let recovered = restore_agreement(&env, &envelope, passphrase).unwrap();
    assert_eq!(recovered.total_amount, i128::MAX);
    assert_eq!(recovered.paid_amount, i128::MAX / 2);
}

/// Different salt/nonce inputs produce different envelopes even for the same
/// agreement and passphrase — no PRNG needed to demonstrate this property.
#[test]
fn test_two_backups_produce_different_envelopes() {
    let env = create_env();
    let agreement = make_agreement(&env);
    let passphrase = b"nonce-uniqueness-test";

    let envelope1 = backup_agreement(&env, &agreement, passphrase, &TEST_SALT, &TEST_NONCE);
    let envelope2 = backup_agreement(&env, &agreement, passphrase, &TEST_SALT_2, &TEST_NONCE_2);

    assert_ne!(
        envelope1, envelope2,
        "different salt/nonce must produce distinct envelopes"
    );
}

#[test]
fn test_completed_status_round_trip() {
    let env = create_env();
    let mut agreement = make_agreement(&env);
    agreement.status = AgreementStatus::Completed;

    let passphrase = b"completed-status-test";
    let envelope = backup_agreement(&env, &agreement, passphrase, &TEST_SALT, &TEST_NONCE);
    let recovered = restore_agreement(&env, &envelope, passphrase).unwrap();
    assert_eq!(recovered.status, AgreementStatus::Completed);
}

#[test]
fn test_paused_status_round_trip() {
    let env = create_env();
    let mut agreement = make_agreement(&env);
    agreement.status = AgreementStatus::Paused;

    let passphrase = b"paused-status-test";
    let envelope = backup_agreement(&env, &agreement, passphrase, &TEST_SALT, &TEST_NONCE);
    let recovered = restore_agreement(&env, &envelope, passphrase).unwrap();
    assert_eq!(recovered.status, AgreementStatus::Paused);
}
