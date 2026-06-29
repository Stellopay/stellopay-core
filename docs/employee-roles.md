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

- **PayrollAction** (contract type)
  - Employee-level: `ViewPayrollStatus`, `ViewPayrollHistory`, `ClaimOwnPayroll`, `WithdrawOwnPayroll`
  - Manager-level: `CreatePayrollRecord`, `UpdatePayrollRecord`, `PauseEmployeePayroll`, `ResumeEmployeePayroll`
  - Admin-level: `AssignRoles`, `RevokeRoles`, `EmergencyPause`, `EmergencyUnpause`

- **StorageKey**
  - `Owner` – contract owner (top-level administrator)
  - `EmployeeRoles(Address) -> Vec<BuiltInRole>` – roles assigned to a given employee

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
- Duplicate assignments are ignored; revoking a non-present role overwrites storage (no-op for the role set).

---

### Role Queries

```rust
pub fn get_roles(env: Env, employee: Address) -> Vec<BuiltInRole>
pub fn has_role(env: Env, employee: Address, role: BuiltInRole) -> bool
pub fn has_role_at_least(env: Env, employee: Address, required: BuiltInRole) -> bool
```

- `get_roles` returns all roles explicitly granted to `employee`.
- `has_role` checks membership for a specific role.
- `has_role_at_least` enforces **role hierarchy**:
  - `has_role_at_least(emp, Manager)` is true if `emp` has `Manager` or `Admin`.
  - `has_role_at_least(emp, Employee)` is true for any non-empty role assignment.

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

**Assign Admin and Manager roles:**

```rust
client.assign_role(&owner, &admin, &BuiltInRole::Admin);
client.assign_role(&admin, &employee, &BuiltInRole::Manager);
```

**Gate operations with capability checks:**

```rust
// Option 1: Boolean check
if client.can_perform(&caller, &PayrollAction::CreatePayrollRecord) {
    // proceed with creating payroll record
}

// Option 2: Enforcing check (caller must be authenticated)
client.require_capability(&caller, &PayrollAction::CreatePayrollRecord)?;
```

**Legacy-style role checks:**

```rust
if client.has_role_at_least(&employee, &BuiltInRole::Manager) {
    // Department-level configuration changes, approvals, etc.
}
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
- **Delegation**: There is no delegated authority model. Only the Owner (from initialization) and accounts explicitly granted the Admin role can manage roles. Capability checks do not support time-limited or scope-limited delegation.
- **Owner vs Admin**: The Owner is stored separately and bypasses all role checks. The Owner is not required to hold any BuiltInRole. Only the Owner can authorize contract upgrades (via `UpgradeableInternal`).
- **Initialization**: The contract can be initialized only once. Double initialization panics.
- **Test coverage**: The test suite includes an allow/deny matrix for all roles and payroll actions, plus explicit tests for role mutation (assign/revoke) deny paths and initialization safeguards.

---

### Test Summary and Security Notes

**Test output** (25 tests, all passing):

- **Regression**: owner/admin assign/revoke, hierarchy (Admin/Manager/Employee), `has_role` / `has_role_at_least`
- **Allow matrix**: Owner, Admin, Manager, and Employee can each perform their permitted payroll actions
- **Deny matrix**: Employee denied Manager/Admin actions; Manager denied Admin actions; no-role denied all
- **Role mutation deny**: Non-admin cannot assign/revoke; employee cannot self-grant Admin; manager cannot assign Admin
- **`require_capability`**: Allow/deny paths for employee, manager, and admin actions
- **Initialization**: Double-initialization panics with `"Already initialized"`

**Security validations covered by tests**:

- Role escalation prevention: only Owner/Admin can mutate roles; self-grant and cross-role escalation attempts fail
- Capability checks are monotonic: Admin implies Manager+Employee; Manager implies Employee
- Unauthorized callers cannot mutate role state; capability helpers enforce role hierarchy
