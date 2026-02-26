## Audit Logging Contract

The `audit_logger` contract provides **append-only, queryable audit logs** for on-chain operations. Each entry is assigned a **monotonically increasing ID** and a **ledger timestamp**, and once written it cannot be modified. Retention is controlled via a configurable maximum number of retained entries.

---

### Data Model

- **`AuditLogEntry`**
  - `id: u64` – sequential identifier
  - `timestamp: u64` – ledger timestamp when the entry was recorded
  - `actor: Address` – caller that triggered the event
  - `action: Symbol` – application-defined label (e.g. `"create_agreement"`)
  - `subject: Option<Address>` – optional related account
  - `amount: Option<i128>` – optional signed amount

Logs are stored under:

- `StorageKey::LogEntry(id) -> AuditLogEntry`
- `StorageKey::NextLogId` – next ID to allocate
- `StorageKey::LogCount` – number of entries within the current retention window
- `StorageKey::FirstLogId` – first retained ID

---

### Initialization

```rust
pub fn initialize(env: Env, owner: Address, retention_limit: u32)
```

- Sets the `Owner`, resets counters, and configures an initial **retention limit**:
  - `retention_limit = 0` → unlimited logs
  - `retention_limit > 0` → at most `retention_limit` entries retained

Only the `owner` may call `initialize`.

---

### Retention Configuration

```rust
pub fn set_retention_limit(env: Env, caller: Address, retention_limit: u32) -> Result<(), AuditError>
pub fn get_retention_limit(env: Env) -> u32
```

- `set_retention_limit`:
  - Only the **owner** may update the limit.
  - New limit applies to subsequent appends. When the number of retained logs exceeds the limit, the logical window is advanced and the oldest entries fall outside the queryable range.

---

### Writing Logs

```rust
pub fn append_log(
    env: Env,
    actor: Address,
    action: Symbol,
    subject: Option<Address>,
    amount: Option<i128>,
) -> u64
```

- **Access control**: `actor.require_auth()` is enforced.
- Creates a new `AuditLogEntry`, assigns the next sequential ID, stores it under `LogEntry(id)`, and returns the ID.
- Retention policy is applied after each append:
  - `LogCount` is updated.
  - `FirstLogId` may advance if the limit is exceeded.

Because there are no update or delete entrypoints, logs are **append-only** within the retained window; older logs can age out per retention policy without being mutated.

---

### Querying Logs

```rust
pub fn get_log(env: Env, id: u64) -> Option<AuditLogEntry>
pub fn get_log_count(env: Env) -> u64
pub fn get_logs(env: Env, offset: u32, limit: u32) -> Result<Vec<AuditLogEntry>, AuditError>
pub fn get_latest_logs(env: Env, limit: u32) -> Result<Vec<AuditLogEntry>, AuditError>
```

- **`get_log`**:
  - Returns `Some(entry)` if `id` is within `[FirstLogId, NextLogId)`, otherwise `None`.
- **`get_log_count`**:
  - Returns the number of entries inside the current retention window.
- **`get_logs(offset, limit)`**:
  - Pages forward from `FirstLogId + offset`.
  - `limit > 0` is required; otherwise `AuditError::InvalidArguments`.
- **`get_latest_logs(limit)`**:
  - Returns up to `limit` newest entries (newest last in the returned vector).
  - `limit > 0` is required; otherwise `AuditError::InvalidArguments`.

---

### Usage Patterns

- **Compliance auditing**:
  - Log important lifecycle events such as agreement creation, activation, dispute resolution, and payout execution.
- **Security monitoring**:
  - Capture administrative actions (role assignments, rate changes, pause/resume) with `actor` and `subject` set appropriately.
- **Forensics**:
  - Use `get_latest_logs` for dashboards and `get_logs` for paginated history views.

