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

### Security Properties

#### Append-Only Guarantee
Logs cannot be modified after creation. There are no update or delete entrypoints. The `AuditLogEntry` struct is immutable once stored.

#### Tamper Evidence
- Each entry has a monotonically increasing ID and ledger timestamp
- IDs are assigned sequentially with no gaps possible within the retained window
- Timestamps are sourced from the Soroban ledger and cannot be spoofed

#### Access Control
- `append_log` requires `actor.require_auth()` — only the authenticated actor can create a log entry attributed to them
- `set_retention_limit` is owner-only — non-owners cannot change retention policy
- `initialize` is one-time only (owner must auth)

#### Retention as Pruning
Old logs age out of the queryable window when retention is exceeded. Underlying storage entries remain but are logically invisible. This prevents unbounded storage growth while maintaining tamper evidence within the window.

#### Log Injection Prevention
Since `actor.require_auth()` is enforced, a malicious contract cannot impersonate another address to inject false log entries. Each entry is cryptographically attributed to the authenticating signer.

---

### Usage Patterns

- **Compliance auditing**:
  - Log important lifecycle events such as agreement creation, activation, dispute resolution, and payout execution.
- **Security monitoring**:
  - Capture administrative actions (role assignments, rate changes, pause/resume) with `actor` and `subject` set appropriately.
- **Forensics**:
  - Use `get_latest_logs` for dashboards and `get_logs` for paginated history views.

### Expense Reimbursement Approval Linkage

The `expense_reimbursement` contract can be configured with an `audit_logger` address using:

```rust
set_audit_logger(owner, audit_logger_address)
```

When configured, each successful `approve_expense` call appends:

- `actor = approver`
- `action = "expense_approved"`
- `subject = Some(submitter)`
- `amount = Some(approved_amount)`

The returned `log_id` is persisted in the expense record (`audit_log_id`) and emitted in the approval event payload, providing a stable on-chain linkage between the financial state transition and append-only audit history.

#### Privacy Considerations for Expense Flows

- Approval logs should include only operational metadata (`actor`, action, `subject`, amount).
- Receipt material is not logged in plaintext by `audit_logger`; expense flows store only a domain-separated SHA-256 receipt commitment.

---

### Testing

```bash
cd onchain
cargo test -p audit_logger
```

#### Test Coverage

The test suite covers:
- Initialization with default and zero retention
- Append log returns monotonic IDs and increments count
- All fields recorded correctly (actor, action, subject, amount, timestamp)
- Negative amounts supported
- Retention enforcement (unlimited, exact boundary, single-entry retention)
- Pagination (empty, offset beyond count, partial pages, limit=0 error)
- Latest logs ordering
- Only owner can set retention
- Log entries are immutable (tamper evidence)
- Timestamps are monotonic
- Multiple actors can append independently

