//! # Encrypted Backup & Recovery Utilities
//!
//! This module provides AES-256-GCM encryption/decryption helpers for
//! serialising and protecting off-chain backups of `Agreement` and
//! `AgreementBalance` state.
//!
//! ## Design
//!
//! * **Serialisation** – Agreement fields are encoded into a compact,
//!   deterministic byte layout (little-endian fixed-width integers + length-
//!   prefixed byte slices for `Address` values).
//! * **Key derivation** – A 256-bit AES key is derived from a caller-supplied
//!   passphrase using PBKDF2-HMAC-SHA256 with a 16-byte salt and 100 000
//!   iterations.  The salt is stored in the backup envelope so that the same
//!   passphrase can always reproduce the key.
//! * **Encryption** – AES-256-GCM with a random 12-byte nonce.  The nonce is
//!   prepended to the ciphertext in the envelope.
//! * **Envelope format** (all lengths in bytes):
//!
//!   ```text
//!   [ version: 1 ][ salt: 16 ][ nonce: 12 ][ ciphertext: variable ]
//!   ```
//!
//! * **Recovery** – The admin calls `restore_agreement_from_backup` with the
//!   encrypted envelope and the passphrase.  The function decrypts, deserialises,
//!   and re-writes the agreement into persistent storage.
//!
//! ## Security assumptions
//!
//! * The passphrase / key material is **never** stored on-chain.  It must be
//!   managed by the operator in a secure key-management system (HSM, KMS, etc.).
//! * The 12-byte nonce is generated from `env.prng()` which is seeded by the
//!   Stellar network's verifiable random function — it is not reused across
//!   backups as long as the ledger sequence advances between calls.
//! * AES-GCM authentication tags protect against ciphertext tampering; any
//!   modification to the envelope will cause decryption to fail.
//! * PBKDF2 with 100 000 iterations provides reasonable brute-force resistance
//!   for passphrases of adequate entropy.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec as StdVec;

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;
use soroban_sdk::{contracttype, Address, Bytes, BytesN, Env};

use crate::storage::{Agreement, AgreementMode, AgreementStatus, DisputeStatus, StorageKey};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Backup envelope version byte.
pub const BACKUP_VERSION: u8 = 1;

/// PBKDF2 iteration count — balances security vs. on-chain compute budget.
pub const PBKDF2_ITERATIONS: u32 = 100_000;

/// Salt length in bytes.
pub const SALT_LEN: usize = 16;

/// AES-GCM nonce length in bytes.
pub const NONCE_LEN: usize = 12;

/// AES-256 key length in bytes.
pub const KEY_LEN: usize = 32;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Lightweight balance snapshot stored alongside an agreement backup.
///
/// `AgreementBalance` is not a first-class on-chain struct; balances are
/// tracked via `DataKey::AgreementEscrowBalance`.  This struct captures the
/// relevant balance at backup time so that the recovery procedure can
/// re-initialise the escrow ledger entry.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AgreementBalance {
    /// Agreement this balance belongs to.
    pub agreement_id: u128,
    /// Token address for this balance entry.
    pub token: Address,
    /// Escrow balance at backup time.
    pub escrow_balance: i128,
    /// Cumulative paid amount at backup time.
    pub paid_amount: i128,
}

/// Error variants for backup / recovery operations.
#[derive(Debug, PartialEq)]
pub enum BackupError {
    /// Serialised payload is shorter than expected.
    BufferTooShort,
    /// Envelope version byte is not recognised.
    UnknownVersion,
    /// AES-GCM decryption failed (wrong key or tampered ciphertext).
    DecryptionFailed,
    /// Deserialised data contains an invalid discriminant.
    InvalidData,
    /// Caller is not the contract owner.
    Unauthorized,
}

// ---------------------------------------------------------------------------
// Serialisation helpers (no_std, no serde)
// ---------------------------------------------------------------------------

/// Serialise an `Agreement` into a compact byte vector.
///
/// Layout (all integers little-endian):
/// ```text
/// id:                  u128  (16 bytes)
/// employer_len:        u32   ( 4 bytes)
/// employer:            [u8]  (employer_len bytes)
/// token_len:           u32   ( 4 bytes)
/// token:               [u8]  (token_len bytes)
/// mode:                u8    ( 1 byte)   0=Escrow, 1=Payroll
/// status:              u8    ( 1 byte)   see AgreementStatus discriminants
/// total_amount:        i128  (16 bytes)
/// paid_amount:         i128  (16 bytes)
/// created_at:          u64   ( 8 bytes)
/// activated_at_flag:   u8    ( 1 byte)   0=None, 1=Some
/// activated_at:        u64   ( 8 bytes)  present only when flag=1
/// cancelled_at_flag:   u8    ( 1 byte)
/// cancelled_at:        u64   ( 8 bytes)  present only when flag=1
/// grace_period_secs:   u64   ( 8 bytes)
/// dispute_status:      u8    ( 1 byte)   0=None, 1=Raised, 2=Resolved
/// dispute_raised_flag: u8    ( 1 byte)
/// dispute_raised_at:   u64   ( 8 bytes)  present only when flag=1
/// amount_per_period_f: u8    ( 1 byte)
/// amount_per_period:   i128  (16 bytes)  present only when flag=1
/// period_seconds_f:    u8    ( 1 byte)
/// period_seconds:      u64   ( 8 bytes)  present only when flag=1
/// num_periods_f:       u8    ( 1 byte)
/// num_periods:         u32   ( 4 bytes)  present only when flag=1
/// claimed_periods_f:   u8    ( 1 byte)
/// claimed_periods:     u32   ( 4 bytes)  present only when flag=1
/// ```
pub fn serialize_agreement(env: &Env, agreement: &Agreement) -> StdVec<u8> {
    let mut buf: StdVec<u8> = StdVec::new();

    // id
    buf.extend_from_slice(&agreement.id.to_le_bytes());

    // employer address bytes
    let emp_bytes = agreement.employer.to_string().into_bytes();
    let emp_raw: StdVec<u8> = emp_bytes.iter().collect();
    buf.extend_from_slice(&(emp_raw.len() as u32).to_le_bytes());
    buf.extend_from_slice(&emp_raw);

    // token address bytes
    let tok_bytes = agreement.token.to_string().into_bytes();
    let tok_raw: StdVec<u8> = tok_bytes.iter().collect();
    buf.extend_from_slice(&(tok_raw.len() as u32).to_le_bytes());
    buf.extend_from_slice(&tok_raw);

    // mode
    let mode_byte: u8 = match agreement.mode {
        AgreementMode::Escrow => 0,
        AgreementMode::Payroll => 1,
    };
    buf.push(mode_byte);

    // status
    let status_byte: u8 = match agreement.status {
        AgreementStatus::Created => 0,
        AgreementStatus::Active => 1,
        AgreementStatus::Paused => 2,
        AgreementStatus::Cancelled => 3,
        AgreementStatus::Completed => 4,
        AgreementStatus::Disputed => 5,
    };
    buf.push(status_byte);

    // amounts
    buf.extend_from_slice(&agreement.total_amount.to_le_bytes());
    buf.extend_from_slice(&agreement.paid_amount.to_le_bytes());

    // timestamps
    buf.extend_from_slice(&agreement.created_at.to_le_bytes());

    push_option_u64(&mut buf, agreement.activated_at);
    push_option_u64(&mut buf, agreement.cancelled_at);

    buf.extend_from_slice(&agreement.grace_period_seconds.to_le_bytes());

    // dispute
    let dispute_byte: u8 = match agreement.dispute_status {
        DisputeStatus::None => 0,
        DisputeStatus::Raised => 1,
        DisputeStatus::Resolved => 2,
    };
    buf.push(dispute_byte);
    push_option_u64(&mut buf, agreement.dispute_raised_at);

    // escrow fields
    push_option_i128(&mut buf, agreement.amount_per_period);
    push_option_u64(&mut buf, agreement.period_seconds);
    push_option_u32(&mut buf, agreement.num_periods);
    push_option_u32(&mut buf, agreement.claimed_periods);

    let _ = env; // env kept for future use (e.g. logging)
    buf
}

/// Deserialise bytes produced by [`serialize_agreement`] back into an
/// `Agreement`.  Returns `Err(BackupError::BufferTooShort)` or
/// `Err(BackupError::InvalidData)` on malformed input.
pub fn deserialize_agreement(env: &Env, data: &[u8]) -> Result<Agreement, BackupError> {
    let mut pos = 0usize;

    let id = read_u128(data, &mut pos)?;

    let employer_str = read_address_str(data, &mut pos)?;
    let employer = Address::from_string(&soroban_sdk::String::from_str(env, &employer_str));

    let token_str = read_address_str(data, &mut pos)?;
    let token = Address::from_string(&soroban_sdk::String::from_str(env, &token_str));

    let mode_byte = read_u8(data, &mut pos)?;
    let mode = match mode_byte {
        0 => AgreementMode::Escrow,
        1 => AgreementMode::Payroll,
        _ => return Err(BackupError::InvalidData),
    };

    let status_byte = read_u8(data, &mut pos)?;
    let status = match status_byte {
        0 => AgreementStatus::Created,
        1 => AgreementStatus::Active,
        2 => AgreementStatus::Paused,
        3 => AgreementStatus::Cancelled,
        4 => AgreementStatus::Completed,
        5 => AgreementStatus::Disputed,
        _ => return Err(BackupError::InvalidData),
    };

    let total_amount = read_i128(data, &mut pos)?;
    let paid_amount = read_i128(data, &mut pos)?;
    let created_at = read_u64(data, &mut pos)?;
    let activated_at = read_option_u64(data, &mut pos)?;
    let cancelled_at = read_option_u64(data, &mut pos)?;
    let grace_period_seconds = read_u64(data, &mut pos)?;

    let dispute_byte = read_u8(data, &mut pos)?;
    let dispute_status = match dispute_byte {
        0 => DisputeStatus::None,
        1 => DisputeStatus::Raised,
        2 => DisputeStatus::Resolved,
        _ => return Err(BackupError::InvalidData),
    };
    let dispute_raised_at = read_option_u64(data, &mut pos)?;

    let amount_per_period = read_option_i128(data, &mut pos)?;
    let period_seconds = read_option_u64(data, &mut pos)?;
    let num_periods = read_option_u32(data, &mut pos)?;
    let claimed_periods = read_option_u32(data, &mut pos)?;

    Ok(Agreement {
        id,
        employer,
        token,
        mode,
        status,
        total_amount,
        paid_amount,
        created_at,
        activated_at,
        cancelled_at,
        grace_period_seconds,
        dispute_status,
        dispute_raised_at,
        amount_per_period,
        period_seconds,
        num_periods,
        claimed_periods,
    })
}

// ---------------------------------------------------------------------------
// Key derivation
// ---------------------------------------------------------------------------

/// Derive a 256-bit AES key from `passphrase` and `salt` using
/// PBKDF2-HMAC-SHA256.
///
/// # Arguments
/// * `passphrase` – caller-supplied secret; never stored on-chain.
/// * `salt`       – 16-byte random salt stored in the backup envelope.
pub fn derive_key(passphrase: &[u8], salt: &[u8]) -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    pbkdf2_hmac::<Sha256>(passphrase, salt, PBKDF2_ITERATIONS, &mut key);
    key
}

// ---------------------------------------------------------------------------
// Encryption / Decryption
// ---------------------------------------------------------------------------

/// Encrypt `plaintext` with AES-256-GCM.
///
/// Returns the envelope:
/// `[ version(1) | salt(16) | nonce(12) | ciphertext ]`
///
/// # Arguments
/// * `env`        – Soroban environment (used for PRNG).
/// * `plaintext`  – serialised agreement bytes.
/// * `passphrase` – encryption passphrase; never stored on-chain.
pub fn encrypt_backup(env: &Env, plaintext: &[u8], passphrase: &[u8]) -> StdVec<u8> {
    // Generate random salt and nonce from the Soroban PRNG.
    let salt_bn: BytesN<16> = env.prng().gen();
    let nonce_bn: BytesN<12> = env.prng().gen();

    let salt: StdVec<u8> = salt_bn.to_array().to_vec();
    let nonce_bytes: [u8; NONCE_LEN] = nonce_bn.to_array();

    let key_bytes = derive_key(passphrase, &salt);
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .expect("AES-GCM encryption must not fail for valid inputs");

    // Build envelope
    let mut envelope: StdVec<u8> = StdVec::with_capacity(1 + SALT_LEN + NONCE_LEN + ciphertext.len());
    envelope.push(BACKUP_VERSION);
    envelope.extend_from_slice(&salt);
    envelope.extend_from_slice(&nonce_bytes);
    envelope.extend_from_slice(&ciphertext);
    envelope
}

/// Decrypt an envelope produced by [`encrypt_backup`].
///
/// Returns the plaintext bytes on success, or a [`BackupError`] on failure.
///
/// # Arguments
/// * `envelope`   – bytes in the format `[ version | salt | nonce | ciphertext ]`.
/// * `passphrase` – must match the passphrase used during encryption.
pub fn decrypt_backup(envelope: &[u8], passphrase: &[u8]) -> Result<StdVec<u8>, BackupError> {
    let min_len = 1 + SALT_LEN + NONCE_LEN + 16; // 16 = AES-GCM tag
    if envelope.len() < min_len {
        return Err(BackupError::BufferTooShort);
    }

    let mut pos = 0usize;

    let version = envelope[pos];
    pos += 1;
    if version != BACKUP_VERSION {
        return Err(BackupError::UnknownVersion);
    }

    let salt = &envelope[pos..pos + SALT_LEN];
    pos += SALT_LEN;

    let nonce_bytes = &envelope[pos..pos + NONCE_LEN];
    pos += NONCE_LEN;

    let ciphertext = &envelope[pos..];

    let key_bytes = derive_key(passphrase, salt);
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| BackupError::DecryptionFailed)
}

// ---------------------------------------------------------------------------
// High-level backup / restore helpers
// ---------------------------------------------------------------------------

/// Produce an encrypted backup envelope for a single `Agreement`.
///
/// The caller is responsible for storing the returned bytes off-chain and
/// managing the passphrase securely.
///
/// # Arguments
/// * `env`        – Soroban environment.
/// * `agreement`  – agreement to back up.
/// * `passphrase` – encryption passphrase (raw bytes).
pub fn backup_agreement(env: &Env, agreement: &Agreement, passphrase: &[u8]) -> StdVec<u8> {
    let plaintext = serialize_agreement(env, agreement);
    encrypt_backup(env, &plaintext, passphrase)
}

/// Decrypt and deserialise an agreement backup envelope.
///
/// Returns the reconstructed `Agreement` on success.
///
/// # Arguments
/// * `env`        – Soroban environment.
/// * `envelope`   – encrypted backup bytes.
/// * `passphrase` – must match the passphrase used during [`backup_agreement`].
pub fn restore_agreement(
    env: &Env,
    envelope: &[u8],
    passphrase: &[u8],
) -> Result<Agreement, BackupError> {
    let plaintext = decrypt_backup(envelope, passphrase)?;
    deserialize_agreement(env, &plaintext)
}

// ---------------------------------------------------------------------------
// On-chain admin entrypoints (called from lib.rs)
// ---------------------------------------------------------------------------

/// Admin-only: write a previously decrypted `Agreement` back into persistent
/// storage, overwriting any existing entry for the same `agreement_id`.
///
/// This is the final step of the recovery procedure after the operator has
/// verified the decrypted data off-chain.
///
/// # Access control
/// Requires the contract owner to have called `require_auth` before this
/// function is invoked (enforced by the caller in `lib.rs`).
pub fn admin_restore_agreement(env: &Env, agreement: Agreement) {
    env.storage()
        .persistent()
        .set(&StorageKey::Agreement(agreement.id), &agreement);
}

/// Admin-only: restore an agreement directly from an encrypted envelope.
///
/// Combines decryption, deserialisation, and storage write in one call.
/// The passphrase is passed as a `Bytes` value so it can be supplied from
/// an off-chain signer without being stored on-chain.
///
/// # Access control
/// Caller must be the contract owner (enforced in `lib.rs`).
pub fn admin_restore_from_encrypted(
    env: &Env,
    envelope: Bytes,
    passphrase: Bytes,
) -> Result<u128, crate::storage::PayrollError> {
    let env_bytes: StdVec<u8> = envelope.iter().collect();
    let pass_bytes: StdVec<u8> = passphrase.iter().collect();

    let agreement = restore_agreement(env, &env_bytes, &pass_bytes)
        .map_err(|_| crate::storage::PayrollError::InvalidData)?;

    let id = agreement.id;
    admin_restore_agreement(env, agreement);
    Ok(id)
}

// ---------------------------------------------------------------------------
// Private serialisation primitives
// ---------------------------------------------------------------------------

fn push_option_u64(buf: &mut StdVec<u8>, val: Option<u64>) {
    match val {
        None => buf.push(0),
        Some(v) => {
            buf.push(1);
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }
}

fn push_option_u32(buf: &mut StdVec<u8>, val: Option<u32>) {
    match val {
        None => buf.push(0),
        Some(v) => {
            buf.push(1);
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }
}

fn push_option_i128(buf: &mut StdVec<u8>, val: Option<i128>) {
    match val {
        None => buf.push(0),
        Some(v) => {
            buf.push(1);
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }
}

// ---------------------------------------------------------------------------
// Private deserialisation primitives
// ---------------------------------------------------------------------------

fn read_u8(data: &[u8], pos: &mut usize) -> Result<u8, BackupError> {
    if *pos >= data.len() {
        return Err(BackupError::BufferTooShort);
    }
    let v = data[*pos];
    *pos += 1;
    Ok(v)
}

fn read_u32(data: &[u8], pos: &mut usize) -> Result<u32, BackupError> {
    let end = *pos + 4;
    if end > data.len() {
        return Err(BackupError::BufferTooShort);
    }
    let v = u32::from_le_bytes(data[*pos..end].try_into().unwrap());
    *pos = end;
    Ok(v)
}

fn read_u64(data: &[u8], pos: &mut usize) -> Result<u64, BackupError> {
    let end = *pos + 8;
    if end > data.len() {
        return Err(BackupError::BufferTooShort);
    }
    let v = u64::from_le_bytes(data[*pos..end].try_into().unwrap());
    *pos = end;
    Ok(v)
}

fn read_u128(data: &[u8], pos: &mut usize) -> Result<u128, BackupError> {
    let end = *pos + 16;
    if end > data.len() {
        return Err(BackupError::BufferTooShort);
    }
    let v = u128::from_le_bytes(data[*pos..end].try_into().unwrap());
    *pos = end;
    Ok(v)
}

fn read_i128(data: &[u8], pos: &mut usize) -> Result<i128, BackupError> {
    let end = *pos + 16;
    if end > data.len() {
        return Err(BackupError::BufferTooShort);
    }
    let v = i128::from_le_bytes(data[*pos..end].try_into().unwrap());
    *pos = end;
    Ok(v)
}

fn read_address_str<'a>(data: &'a [u8], pos: &mut usize) -> Result<&'a str, BackupError> {
    let len = read_u32(data, pos)? as usize;
    let end = *pos + len;
    if end > data.len() {
        return Err(BackupError::BufferTooShort);
    }
    let s = core::str::from_utf8(&data[*pos..end]).map_err(|_| BackupError::InvalidData)?;
    *pos = end;
    Ok(s)
}

fn read_option_u64(data: &[u8], pos: &mut usize) -> Result<Option<u64>, BackupError> {
    let flag = read_u8(data, pos)?;
    if flag == 0 {
        Ok(None)
    } else {
        Ok(Some(read_u64(data, pos)?))
    }
}

fn read_option_u32(data: &[u8], pos: &mut usize) -> Result<Option<u32>, BackupError> {
    let flag = read_u8(data, pos)?;
    if flag == 0 {
        Ok(None)
    } else {
        Ok(Some(read_u32(data, pos)?))
    }
}

fn read_option_i128(data: &[u8], pos: &mut usize) -> Result<Option<i128>, BackupError> {
    let flag = read_u8(data, pos)?;
    if flag == 0 {
        Ok(None)
    } else {
        Ok(Some(read_i128(data, pos)?))
    }
}
