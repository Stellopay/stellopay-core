## Employee Role Management Contract

The `employee_roles` contract provides **hierarchical role management** for employees, with explicit payroll capability checks usable by other payroll-related contracts.

Roles are hierarchical:

- `Employee` (baseline)
- `Manager`
- `Admin` (highest)

An account with a higher role level implicitly satisfies checks for all lower levels (e.g. Admin satisfies Manager and Employee).

### Centralized RBAC Integration

The `employee_roles` contract can be linked to the centralized `rbac` contract. When linked, role checks will query the RBAC contract as a fallback/centralized source of truth.

| Employee Role | RBAC Role |
|---------------|-----------|
| `Employee`    | `Employee` |
| `Manager`     | `Employer` |
| `Admin`       | `Admin`    |

---

### Role-to-Capability Matrix (NatSpec)

| Role | Allowed Payroll Actions |
|------|-------------------------|
| **Employee** | ViewPayrollStatus, ViewPayrollHistory, ClaimOwnPayroll, WithdrawOwnPayroll |
| **Manager** | All Employee actions plus: CreatePayrollRecord, UpdatePayrollRecord, PauseEmployeePayroll, ResumeEmployeePayroll |
| **Admin** | All Manager actions plus: AssignRoles, RevokeRoles, EmergencyPause, EmergencyUnpause |
| **Owner** | All actions (contract owner bypasses role checks) |

---

### Data Model

- **BuiltInRole** (contract type)
  - `Employee`
  - `Manager`
  - `Admin`

- **RoleGrant** (contract type)
  - `role: BuiltInRole` – assigned built-in role
  - `expires_at: Option<u64>` – optional expiration timestamp (unix epoch seconds). `None` indicates a non-expiring grant.

- **PayrollAction** (contract type)
  - Employee-level: `ViewPayrollStatus`, `ViewPayrollHistory`, `ClaimOwnPayroll`, `WithdrawOwnPayroll`
  - Manager-level: `CreatePayrollRecord`, `UpdatePayrollRecord`, `PauseEmployeePayroll`, `ResumeEmployeePayroll`
  - Admin-level: `AssignRoles`, `RevokeRoles`, `EmergencyPause`, `EmergencyUnpause`

- **StorageKey**
  - `Owner` – contract owner (top-level administrator)
  - `EmployeeRoles(Address) -> Vec<RoleGrant>` – role grants assigned to a given employee (with backward compatibility for legacy `Vec<BuiltInRole>`)

---

### Initialization

```rust
pub fn initialize(env: Env, owner: Address)
```

- Sets the contract `Owner`.
- Only the `owner` provided to `initialize` may call it.
- Panics with `"Already initialized"` if called more than once.

---

### Role Assignment

```rust
pub fn assign_role(
    env: Env,
    caller: Address,
    employee: Address,
    role: BuiltInRole,
) -> Result<(), RoleError>

pub fn assign_role_with_expiration(
    env: Env,
    caller: Address,
    employee: Address,
    role: BuiltInRole,
    expires_at: Option<u64>,
) -> Result<(), RoleError>

pub fn revoke_role(
    env: Env,
    caller: Address,
    employee: Address,
    role: BuiltInRole,
) -> Result<(), RoleError>
```

- **Access control**:
  - `caller` must be either the contract `Owner` or an account with the `Admin` role.
  - **Escalation safeguard**: Non-owner callers must have at least the role they assign or revoke (e.g. an Admin cannot assign Admin if they lack it; in practice only Admin+ can assign, so this is defense-in-depth).
- **Time-bound grants**:
  - `assign_role` creates a non-expiring grant (`expires_at = None`).
  - `assign_role_with_expiration` allows specifying `expires_at: Option<u64>`.
  - Expiration timestamps must be strictly in the future (`expires_at > env.ledger().timestamp()`), otherwise `RoleError::InvalidExpiration` is returned.
- Re-assigning an existing role updates its expiration timestamp.
- Revoking a role removes the grant entry from storage regardless of its expiration state.

---

### Role Queries & Authorization Semantics

```rust
pub fn get_roles(env: Env, employee: Address) -> Vec<BuiltInRole>
pub fn get_role_grants(env: Env, employee: Address) -> Vec<RoleGrant>
pub fn has_role(env: Env, employee: Address, role: BuiltInRole) -> bool
pub fn has_role_at_least(env: Env, employee: Address, required: BuiltInRole) -> bool
```

- **Runtime expiration evaluation**:
  - Authorization checks (`has_role`, `has_role_at_least`, `can_perform`, `require_capability`) evaluate role expiration dynamically at execution time against the current ledger timestamp (`env.ledger().timestamp()`).
  - A grant with `expires_at = Some(exp)` is authorized when `current_timestamp < exp`, and fails when `current_timestamp >= exp`.
- `get_roles` returns only currently active (non-expired) roles assigned to `employee`.
- `get_role_grants` returns all `RoleGrant` entries for `employee` (including `expires_at` metadata).
- `has_role` checks active membership for a specific role.
- `has_role_at_least` enforces **role hierarchy** for active grants:
  - `has_role_at_least(emp, Manager)` is true if `emp` holds an active `Manager` or `Admin` grant.
  - `has_role_at_least(emp, Employee)` is true for any active role assignment.

---

### Expiration vs. Revocation

- **Expiration**: Temporary roles automatically stop granting permissions as soon as `env.ledger().timestamp() >= expires_at`. No cleanup transaction is required, avoiding gas overhead and administrative delay. Expired grants are simply ignored during authorization checks.
- **Revocation**: Explicitly calling `revoke_role` immediately removes the specified grant record from contract storage.

---

### Payroll Capability Helpers

```rust
pub fn can_perform(env: Env, employee: Address, action: PayrollAction) -> bool
pub fn require_capability(
    env: Env,
    employee: Address,
    action: PayrollAction,
) -> Result<(), RoleError>
```

- **`can_perform`**: Returns `true` if `employee` has sufficient role for the action (Owner always allowed). Use for read-only checks.
- **`require_capability`**: Enforces that `employee` can perform the action; returns `Err(RoleError::Unauthorized)` otherwise. Requires `employee` authentication. Use in integrating contracts to gate payroll operations.

---

### Integration Guidance

**Assign permanent and temporary contractor roles:**

```rust
// Grant permanent Admin role
client.assign_role(&owner, &admin, &BuiltInRole::Admin);

// Grant temporary Manager role to contractor expiring at unix timestamp 1750000000
let contractor_expiry = Some(1750000000u64);
client.assign_role_with_expiration(&admin, &contractor, &BuiltInRole::Manager, &contractor_expiry);
```

**Gate operations with capability checks:**

```rust
// Option 1: Boolean check (evaluates active non-expired roles)
if client.can_perform(&caller, &PayrollAction::CreatePayrollRecord) {
    // proceed with creating payroll record
}

// Option 2: Enforcing check (caller must be authenticated)
client.require_capability(&caller, &PayrollAction::CreatePayrollRecord)?;
```

---

### Centralized Role Configuration

```rust
pub fn set_rbac_address(env: Env, rbac_address: Address)
```

- **Access control**: Only the contract `Owner` can set the RBAC address.
- Once set, `has_role` and `has_role_at_least` (and by extension `can_perform`) will check the RBAC contract if the role is not found in local storage.

---

### Security Assumptions and Notes

- **Role escalation**: Only Owner or Admin can assign or revoke roles. Non-admin users (including Manager, Employee) cannot grant themselves or others elevated roles. The contract enforces that assigners have at least the role they assign.
- **Runtime Expiration Evaluation**: Expiration is always evaluated live against `env.ledger().timestamp()`. Authorization decisions are never cached, ensuring expired grants are immediately treated as revoked without risk of lingering access.
- **Owner vs Admin**: The Owner is stored separately and bypasses all role checks. The Owner is not required to hold any BuiltInRole. Only the Owner can authorize contract upgrades (via `UpgradeableInternal`).
- **Initialization**: The contract can be initialized only once. Double initialization panics.
- **Backward Storage Compatibility**: Legacy state stored as `Vec<BuiltInRole>` is automatically read as non-expiring `RoleGrant`s (`expires_at: None`), preserving complete compatibility for existing deployments.

---

### Test Summary and Security Notes

**Test output** (31 tests, all passing):

- **Regression**: owner/admin assign/revoke, hierarchy (Admin/Manager/Employee), `has_role` / `has_role_at_least`
- **Time-bound grants**: lifecycle, expiry boundary checks (`exp - 1` vs `exp` vs `exp + 1`), non-expiring longevity, past timestamp validation (`InvalidExpiration`), grant extension/update, legacy storage compatibility
- **Allow matrix**: Owner, Admin, Manager, and Employee can each perform their permitted payroll actions
- **Deny matrix**: Employee denied Manager/Admin actions; Manager denied Admin actions; no-role denied all
- **Role mutation deny**: Non-admin cannot assign/revoke; employee cannot self-grant Admin; manager cannot assign Admin
- **`require_capability`**: Allow/deny paths for employee, manager, and admin actions
- **Initialization**: Double-initialization panics with `"Already initialized"`

**Security validations covered by tests**:

- Role escalation prevention: only Owner/Admin can mutate roles; self-grant and cross-role escalation attempts fail
- Dynamic time checks: authorization immediately reverts after `expires_at` timestamp without explicit transaction
- Capability checks are monotonic: Admin implies Manager+Employee; Manager implies Employee
- Unauthorized callers cannot mutate role state; capability helpers enforce role hierarchy

