# Encrypted Backup & Recovery

This document describes the full lifecycle for backing up and recovering
`Agreement` (and `AgreementBalance`) state from the `stello_pay_contract`
using AES-256-GCM encryption.

---

## Overview

The Soroban ledger is the source of truth for all agreement state.  In a
catastrophic failure scenario (e.g. ledger data loss, contract migration gone
wrong, or a critical bug requiring a state rollback) the operator needs a way
to reconstruct agreement state from a trusted off-chain snapshot.

The backup module (`src/backup.rs`) provides:

| Concern | Solution |
|---|---|
| Serialisation | Compact, deterministic little-endian byte layout |
| Key derivation | PBKDF2-HMAC-SHA256, 100 000 iterations, 16-byte random salt |
| Encryption | AES-256-GCM with a 12-byte random nonce |
| Integrity | AES-GCM authentication tag (16 bytes) |
| Recovery | Admin-only on-chain entrypoints |

---

## Envelope Format

Every encrypted backup is a self-contained byte string:

```
[ version: 1 byte ][ salt: 16 bytes ][ nonce: 12 bytes ][ ciphertext: variable ]
```

| Field | Size | Description |
|---|---|---|
| `version` | 1 | Always `0x01` for the current format |
| `salt` | 16 | Random salt used for PBKDF2 key derivation |
| `nonce` | 12 | Random AES-GCM nonce |
| `ciphertext` | variable | AES-256-GCM encrypted payload + 16-byte auth tag |

The salt and nonce are generated from `env.prng()` (Stellar VRF-seeded) so
they are unique per backup call.

---

## Key Derivation

```
key = PBKDF2-HMAC-SHA256(passphrase, salt, iterations=100_000, dklen=32)
```

The passphrase is **never stored on-chain**.  It must be managed by the
operator in a secure key-management system (HSM, KMS, secrets manager, etc.).

---

## Backup Lifecycle

### 1. Trigger conditions

Operators should trigger a backup:

- Before any contract upgrade or migration.
- After significant state changes (e.g. large batch of agreements created).
- On a scheduled cadence (e.g. daily snapshot of all active agreements).
- Immediately after detecting anomalous on-chain behaviour.

### 2. Serialise the Agreement

Call `serialize_agreement(env, &agreement)` to produce a compact byte vector.
The layout encodes all fields of the `Agreement` struct including optional
escrow fields (`amount_per_period`, `period_seconds`, `num_periods`,
`claimed_periods`).

### 3. Encrypt

```rust
let envelope: Vec<u8> = backup_agreement(&env, &agreement, passphrase);
```

This derives a 256-bit key from the passphrase + a fresh random salt, then
encrypts the serialised bytes with AES-256-GCM using a fresh random nonce.

### 4. Store off-chain

Store the `envelope` bytes in a durable, access-controlled off-chain store
(e.g. encrypted S3 bucket, HSM-backed database).  Tag the record with:

- `agreement_id`
- `ledger_sequence` at backup time
- `timestamp`

### 5. Manage the passphrase

Store the passphrase in a separate, access-controlled secret store (e.g. AWS
Secrets Manager, HashiCorp Vault).  The passphrase must **not** be stored
alongside the envelope.

---

## Recovery Procedure

### Prerequisites

- Access to the encrypted envelope bytes.
- Access to the passphrase used during backup.
- The contract owner's signing key.

### Option A — Decrypt off-chain, restore on-chain

1. Retrieve the envelope from off-chain storage.
2. Retrieve the passphrase from the secret store.
3. Decrypt and deserialise off-chain:

   ```rust
   let agreement = restore_agreement(&env, &envelope, passphrase)?;
   ```

4. Verify the deserialised data (check `agreement_id`, `employer`, amounts).
5. Call the admin entrypoint to write the state back:

   ```rust
   client.admin_restore_agreement(&owner, &agreement);
   ```

### Option B — Decrypt on-chain in a single call

If the operator trusts the on-chain environment and wants a single-transaction
recovery:

```rust
client.admin_restore_from_encrypted(&owner, &envelope_bytes, &passphrase_bytes);
```

This combines decryption, deserialisation, and storage write in one call.
Returns the restored `agreement_id` on success, or `PayrollError::InvalidData`
if decryption fails.

> **Security note:** The passphrase is passed as a `Bytes` argument.  It is
> visible in the transaction payload on-chain.  Rotate the passphrase after
> using Option B.

---

## AgreementBalance Recovery

`AgreementBalance` is not a first-class on-chain struct; escrow balances are
stored under `DataKey::AgreementEscrowBalance(agreement_id, token)`.

When taking a backup, snapshot the balance separately:

```rust
let balance = AgreementBalance {
    agreement_id,
    token: token.clone(),
    escrow_balance: DataKey::get_agreement_escrow_balance(&env, agreement_id, &token),
    paid_amount: DataKey::get_agreement_paid_amount(&env, agreement_id),
};
```

During recovery, after restoring the `Agreement` struct, restore the balance
entries directly via `DataKey::set_agreement_escrow_balance` and
`DataKey::set_agreement_paid_amount` (requires a custom admin entrypoint or
direct storage access during a migration).

---

## Security Assumptions

| Assumption | Rationale |
|---|---|
| Passphrase is high-entropy | PBKDF2 provides brute-force resistance but cannot compensate for weak passphrases |
| Passphrase is never stored on-chain | On-chain data is public; storing the key alongside the ciphertext would be catastrophic |
| Nonce is not reused | `env.prng()` is VRF-seeded; nonce reuse under the same key breaks AES-GCM confidentiality |
| Envelope integrity is verified | AES-GCM auth tag detects any ciphertext or header tampering |
| Admin entrypoints are owner-only | `require_auth` + owner check prevent unauthorised state writes |
| Passphrase rotation after on-chain use | Option B exposes the passphrase in the transaction; rotate immediately |

---

## Key Rotation Procedure

### Overview

Key rotation is the process of replacing the backup passphrase with a new one. This is recommended:

- On a scheduled cadence (e.g., annually or quarterly)
- After any suspected passphrase compromise
- When personnel with passphrase access leaves the organization
- After a security audit recommends rotation

### Blast Radius

**Critical:** Key rotation has a specific blast radius on existing backups:

- **Old backups remain decryptable only with the old passphrase** - Backups created before rotation cannot be decrypted with the new passphrase
- **New backups require the new passphrase** - Backups created after rotation cannot be decrypted with the old passphrase
- **Both passphrases must be retained** - To recover historical data, you must keep the old passphrase accessible until all old backups are no longer needed

This isolation is intentional and provides cryptographic security: rotating the key immediately invalidates the ability to decrypt old backups with the new key, preventing forward/backward key compromise.

### Rotation Steps

1. **Generate a new passphrase**
   - Use a cryptographically secure random generator
   - Ensure high entropy (at least 256 bits of randomness)
   - Store the new passphrase in your secure key-management system

2. **Create fresh backups with the new passphrase**
   - For all active agreements, create new backups using the new passphrase
   - Tag these backups with the rotation timestamp and new passphrase version
   - Store them alongside (not replacing) the old backups

3. **Verify new backups**
   - Decrypt and verify a sample of new backups to ensure they work correctly
   - Run the regression test suite to confirm key isolation:
     ```bash
     cargo test -p stello_pay_contract test_key_rotation
     ```

4. **Retain the old passphrase**
   - Do not delete the old passphrase immediately
   - Keep it accessible for as long as old backups may need to be restored
   - Document which passphrase version corresponds to which backup batch

5. **Archive old backups (optional)**
   - Once you are confident new backups are sufficient, archive old backups to cold storage
   - This reduces the operational burden of managing multiple passphrase versions

### Regression Test Coverage

The test suite includes key-rotation regression tests in `tests/backup_recovery_tests.rs`:

- `test_key_rotation_old_backup_fails_with_new_key` - Verifies that a backup encrypted with the old key fails to decrypt with the new key
- `test_key_rotation_correct_keys_decrypt_matching_backups` - Verifies that:
  - Old key decrypts old backups
  - New key decrypts new backups
  - Cross-decryption fails (old key cannot decrypt new backups, new key cannot decrypt old backups)

Run these tests before and after key rotation to ensure the system behaves as expected:

```bash
cargo test -p stello_pay_contract test_key_rotation
```

### Example Rotation Timeline

| Date | Action | Passphrase Version |
|---|---|---|
| 2024-01-01 | Initial backups created | v1 |
| 2024-07-01 | Key rotation initiated | v1 → v2 |
| 2024-07-01 | New backups created with v2 | v2 |
| 2024-07-01 | Old backups (v1) retained for recovery | v1, v2 |
| 2025-01-01 | Old backups (v1) archived to cold storage | v2 active, v1 archived |

### Security Notes

- **Never delete the old passphrase before archiving old backups** - If you lose the old passphrase, you lose the ability to recover old backups
- **Document passphrase versions** - Maintain a mapping of passphrase versions to time periods to know which key to use for which backup
- **Test recovery periodically** - Periodically test restoring old backups with the old passphrase to ensure the process still works
- **Consider a key versioning scheme** - Include a version identifier in your backup metadata to track which passphrase was used

---

## API Reference

### `backup.rs` (off-chain / test use)

| Function | Description |
|---|---|
| `serialize_agreement(env, agreement)` | Serialise an `Agreement` to bytes |
| `deserialize_agreement(env, bytes)` | Deserialise bytes back to an `Agreement` |
| `derive_key(passphrase, salt)` | PBKDF2-HMAC-SHA256 key derivation |
| `encrypt_backup(env, plaintext, passphrase)` | AES-256-GCM encrypt |
| `decrypt_backup(envelope, passphrase)` | AES-256-GCM decrypt |
| `backup_agreement(env, agreement, passphrase)` | Serialise + encrypt |
| `restore_agreement(env, envelope, passphrase)` | Decrypt + deserialise |

### Contract entrypoints (`lib.rs`)

| Entrypoint | Description |
|---|---|
| `admin_restore_agreement(caller, agreement)` | Write a pre-verified `Agreement` back to storage |
| `admin_restore_from_encrypted(caller, envelope, passphrase)` | Decrypt envelope on-chain and restore |

Both entrypoints require the caller to be the contract owner.

---

## Test Coverage

`tests/backup_recovery_tests.rs` covers:

- Serialisation round-trip for payroll and escrow agreements
- All `AgreementStatus` and `DisputeStatus` variants
- Full encrypt → decrypt → restore round-trip
- `AgreementBalance` snapshot construction
- Wrong passphrase → `DecryptionFailed`
- Tampered ciphertext → `DecryptionFailed`
- Tampered salt → `DecryptionFailed`
- Tampered nonce → `DecryptionFailed`
- Truncated envelope → `BufferTooShort`
- Unknown version byte → `UnknownVersion`
- On-chain `admin_restore_agreement` entrypoint
- On-chain `admin_restore_from_encrypted` entrypoint
- Unauthorised caller rejection (both entrypoints)
- Multiple agreements backed up and restored independently
- Edge cases: `id=0`, `i128::MAX` amounts, nonce uniqueness across backups
