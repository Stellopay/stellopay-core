## Employee Role Management Contract

The `employee_roles` contract provides **hierarchical role management** for employees, enabling role-based permissions and simple access checks usable by other payroll-related contracts.

Roles are hierarchical:

- `Employee` (baseline)
- `Manager`
- `Admin` (highest)

An account with a higher role level implicitly satisfies checks for all lower levels (e.g. Admin satisfies Manager and Employee).

---

### Data Model

- `BuiltInRole` (contract type)
  - `Employee`
  - `Manager`
  - `Admin`

- `StorageKey`
  - `Owner` – contract owner (top-level administrator)
  - `EmployeeRoles(Address) -> Vec<BuiltInRole>` – roles assigned to a given employee

---

### Initialization

```rust
pub fn initialize(env: Env, owner: Address)
```

- Sets the contract `Owner`.
- Only the `owner` provided to `initialize` may call it.

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
  - `caller` must be either:
    - The contract `Owner`, or
    - An account with the `Admin` role.
- Duplicate assignments are ignored; revoking a non-present role is a no-op.

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

### Example Usage

**Assign Admin and Manager roles:**

```rust
client.assign_role(&owner, &admin, &BuiltInRole::Admin);
client.assign_role(&admin, &employee, &BuiltInRole::Manager);
```

**Enforce role-based access in an integrating contract:**

- Call `has_role_at_least(employee, BuiltInRole::Manager)` to gate:
  - Department-level configuration changes
  - Approval of high-risk operations

This contract is intentionally minimal and focused on **core RBAC primitives**; higher-level business rules can be implemented in consuming contracts.

