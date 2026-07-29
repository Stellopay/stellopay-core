# Department and Organization Management

This document describes the Department/Organization Management contract (Issue #326): organizing employees into departments and organizations with hierarchical structures.

## Overview

The `department_manager` contract provides:

- **Organizations** – Top-level entities owned by an address.
- **Departments** – Belong to an organization; can be top-level or nested under another department (multi-level hierarchy supported).
- **Employee assignment** – Assign employee addresses to a department within an organization. Re-assigning moves the employee automatically.
- **Employee removal** – Revoke an employee from their current department without re-assigning.
- **Department-level reporting** – Employee counts, child departments, and employee lists per department.

## Contract Location

- **Contract**: `onchain/contracts/department_manager/src/lib.rs`
- **Tests**: `onchain/contracts/department_manager/tests/test_department.rs`

## Role Model

| Role | Who | Allowed Operations |
|------|-----|-------------------|
| **Admin** | Address passed to `initialize` | Deploys the contract once |
| **Org Owner** | Address that calls `create_organization` | Create depts, assign/remove employees in their org |

> **Note**: All mutating functions (department creation, employee assignment/removal) require the org owner to authenticate via `require_auth()`. There is no global admin override for org-level operations.

## API

### Initialization (Admin)

```rust
initialize(admin: Address)
```
Sets the admin. **Callable once** — panics `"Already initialized"` on a second call.

---

### Organizations (Org Owner)

```rust
create_organization(owner: Address, name: Symbol) -> u128
```
Creates an org; `owner` must authenticate. Returns `org_id` (sequential from 1).

**Uniqueness Invariant (Issue #917):** Each `name` is **globally unique** across the
entire contract. `create_organization` rejects any call whose `name` already maps
to an existing organization (via the `OrgByName` reverse index), regardless of who
the caller is. A rejected attempt **rolls back all state mutations**, so the
original organization's record and its full `get_org_departments` tree are left
intact. Soroban `Symbol` is case-sensitive, so `"Acme"` and `"acme"` are distinct
identifiers.


```rust
get_organization(org_id: u128) -> Organization
```
Returns the organization record. Panics `"Organization not found"` for unknown IDs.

---

### Departments (Org Owner)

```rust
create_department(caller: Address, org_id: u128, name: Symbol, parent_id: Option<u128>) -> u128
```
Creates a department. `caller` must be the org owner. `parent_id = None` for top-level, or a dept ID for a child (must be in the same org). Returns `dept_id` (global counter from 1).

**Hierarchical constraints enforced:**
- Parent must exist and belong to the same org.
- The new department's depth (`parent_depth + 1`) must not exceed `MAX_DEPTH` (currently **10**). Panics `"Max hierarchy depth exceeded"` otherwise.

```rust
get_department(department_id: u128) -> Department
```
Returns the department record.

```rust
get_org_departments(org_id: u128) -> Vec<u128>
```
Returns all department IDs (top-level and nested) for an organization.

```rust
get_child_departments(department_id: u128) -> Vec<u128>
```
Returns the **direct child** department IDs of a given department. Returns empty `Vec` for leaf departments.

```rust
update_department(caller: Address, dept_id: u128, new_parent: Option<u128>)
```
Reparents `dept_id` to `new_parent` (or makes it top-level with `None`). `caller` must be the org owner.

**Constraints enforced:**
- New parent must exist and belong to the same org.
- New depth (`new_parent_depth + 1`) must not exceed `MAX_DEPTH`. Panics `"Max hierarchy depth exceeded"`.
- Moving a department under one of its own descendants is rejected. Panics `"Cycle detected"`.

> **Note on subtree moves**: only the moved department's `parent_id` changes. All descendants retain their existing `parent_id` links, so the entire subtree moves atomically.

```rust
remove_department(caller: Address, dept_id: u128)
```
Removes a department from an organization. `caller` must be the org owner.

**Constraints enforced (Issue #1094):**
- Department must have **no active employees**. Panics `"Cannot remove department with active employees"`.
- Department must have **no child departments**. Panics `"Cannot remove department with child departments"`.
- Properly cleans up storage: removes department record, children list, employees list, and updates parent's children list and org's department list.

> **Note on deleting departments**: To remove a department, first reassign or remove all its employees and ensure it has no child departments. Leaf departments with no employees can be safely removed.

---

### Employee Assignment (Org Owner)

```rust
assign_employee_to_department(caller: Address, org_id: u128, department_id: u128, employee: Address)
```
Assigns `employee` to the given department. `caller` must be org owner. Re-assigning to another department in the same org **automatically moves** the employee (removes from old dept).

```rust
remove_employee_from_department(caller: Address, org_id: u128, employee: Address)
```
Removes (un-assigns) an employee from their current department in an org. `caller` must be org owner. Panics `"Employee not assigned in this org"` if not assigned.

---

### Reporting (no auth required)

```rust
get_department_employees(department_id: u128) -> Vec<Address>
```
Returns all employee addresses currently in the department.

```rust
get_employee_department(employee: Address, org_id: u128) -> Option<u128>
```
Returns the department ID for the employee in that org, or `None` if not assigned.

```rust
get_department_report(department_id: u128) -> (u32, Vec<u128>, Vec<Address>)
```
Returns `(total_employee_count, direct_child_department_ids, all_employee_addresses)` for a department.  

**Aggregation rules:**
- `employee_count` and `employee_addresses` include employees from the queried department **and** all descendant departments (children, grandchildren, etc.), recursively.
- `child_department_ids` contains only **direct** children (one level), enabling tree traversal.
- A leaf department (no children) returns only its own employees.
- Zero employees in a subtree produces `count = 0` and an empty addresses vector.

---

## Events

All mutating operations publish events for indexer/integrator consumption:

| Event Topic | Data | Trigger |
|-------------|------|---------|
| `("org_crtd", org_id)` | `org_id: u128` | Organization created |
| `("dept_crtd", dept_id)` | `dept_id: u128` | Department created |
| `("dept_mvd", dept_id)` | `dept_id: u128` | Department reparented |
| `("dept_rmvd", dept_id)` | `dept_id: u128` | Department removed |
| `("emp_asgnd", dept_id)` | `employee: Address` | Employee assigned to department |
| `("emp_rmvd", dept_id)` | `employee: Address` | Employee removed from department |

---

## Reverse-Index Invariant

The contract maintains **two complementary storage indexes** for employee placement:

| Index | Storage key | Meaning |
|-------|-------------|---------|
| Forward | `EmployeeDepartment(employee, org_id) → dept_id` | Which department is the employee currently in? |
| Reverse | `DepartmentEmployees(dept_id) → Vec<Address>` | Which employees are currently in this department? |
| Membership flag | `EmployeeInDepartment(dept_id, employee) → ()` | Fast O(1) membership test (no list scan required) |

### The invariant

After every successful call to `assign_employee_to_department` or
`remove_employee_from_department`, **all three indexes are mutually consistent**:

> For any `(employee, org_id)` pair there is **at most one** department `d` such
> that the forward index returns `d` **and** the reverse index contains the
> employee in `d`. For every other department `d'` in the same org, the reverse
> index for `d'` must **not** contain the employee.

In plain terms:

- `get_employee_department(emp, org)` returns `Some(d)` ↔ `get_department_employees(d)` contains `emp`.
- If the employee is not assigned, `get_employee_department` returns `None` **and** no department's employee list contains them.

### How the invariant is maintained

`assign_employee_to_department` auto-removes from the previous department:

```text
assign(caller, org, new_dept, emp):
  if EmployeeDepartment(emp, org) = Some(old_dept):
    remove_employee_from_dept_internal(old_dept, emp)  ← clears reverse index + flag
  set EmployeeInDepartment(new_dept, emp)              ← set flag
  set EmployeeDepartment(emp, org) = new_dept          ← update forward index
  push emp to DepartmentEmployees(new_dept)            ← update reverse index
```

`remove_employee_from_department` clears all three:

```text
remove(caller, org, emp):
  old_dept = EmployeeDepartment(emp, org)              ← read forward index
  remove_employee_from_dept_internal(old_dept, emp)    ← clears reverse index + flag
  delete EmployeeDepartment(emp, org)                  ← clears forward index
```

The internal helper `remove_employee_from_dept_internal` only clears the
**reverse index** (`DepartmentEmployees`) and the **membership flag**
(`EmployeeInDepartment`). It intentionally does **not** touch the forward index
(`EmployeeDepartment`), which is the responsibility of each caller. Both callers
above satisfy this contract.

### Proven by tests

The following tests in `tests/test_department.rs` verify this invariant end-to-end:

- **`test_assign_remove_reassign_indexes_are_consistent`** — exercises the full
  assign → remove → reassign cycle and asserts that after each step both
  `get_employee_department` (forward) and `get_department_employees` (reverse)
  agree with each other and contain no stale references.

- **`test_original_department_excludes_moved_employee`** — assigns two employees
  to a department, moves one to a second department via direct reassignment, and
  asserts that the original department's employee list no longer includes the
  moved employee while the remaining employee is still present.

### Implications for integrators

- There is no need to call `remove_employee_from_department` before
  `assign_employee_to_department`: the assignment operation handles cleanup
  automatically if the employee is already placed in another department in the
  same org.
- Cross-org assignments are fully independent. Assigning an employee to
  `org_b.dept_x` does not alter their assignment in `org_a`.
- If an employee is removed and then re-assigned, both indexes start from a clean
  state for the new assignment — there are no lingering references to the old
  department.

---

## Security Assumptions

1. **Org ownership is irrevocable**: The owner address set at `create_organization` time is permanent. There is no ownership transfer function.
2. **Admin ≠ Org Owner**: The admin address (set during `initialize`) has no special permissions over org operations. Only the org owner controls their org.
3. **No token transfers**: This contract only manages structure. It holds no funds and cannot move funds.
4. **Single assignment per org**: Each employee has at most one department per org. Reassignment is atomic (remove then add).
5. **Initialization is one-time**: The `Initialized` flag in persistent storage prevents re-initialization even after admin key changes.
6. **Cross-org isolation**: Employee assignments are org-scoped. Being removed from one org does not affect assignments in others.
7. **Bounded hierarchy depth**: `create_department` and `update_department` both enforce `MAX_DEPTH = 10`. A department at depth 10 cannot have children. This prevents unbounded storage reads during depth traversal.
8. **No cycles**: `update_department` walks the ancestor chain of the proposed new parent and rejects the move if `dept_id` appears in that chain. Since `create_department` only appends to an existing tree (no reparenting), cycles can only arise through `update_department`, which is fully guarded.
9. **Subtree moves are safe**: Moving a department only updates its own `parent_id` and the children lists of the old and new parents. Descendants are unaffected, so the subtree is moved atomically without touching descendant records.
10. **Department deletion constraints (Issue #1094)**: Departments can only be removed when they have no active employees and no child departments. This prevents stranding employees' organizational references and orphaning the department hierarchy. The function properly cleans up all storage entries and updates parent/organization references.
11. **Organization name uniqueness (Issue #917)**: Each `name` is globally unique across the entire contract. The `OrgByName(name) -> org_id` reverse index is checked inside `create_organization` **before** any state mutation (counter increment, organization record, empty `OrgDepartments` vec, or reverse-index write). On rejection, Soroban rolls back the entire call, so the original organization's record and its department tree are guaranteed to be unaffected. Because there is no `delete_organization`, a name, once claimed, is permanently reserved for the lifetime of the contract instance. If a future version introduces `delete_organization`, it MUST also clear the corresponding `OrgByName(name)` entry to prevent orphaned reverse-index entries from permanently blocking re-creation under that name.

---

## Storage Layout (for integrators)

| Storage Key | Value Type | Description |
|-------------|-----------|-------------|
| `Admin` | `Address` | Contract administrator |
| `Initialized` | `bool` | One-time init guard |
| `NextOrgId` | `u128` | Auto-increment org ID counter |
| `NextDeptId` | `u128` | Auto-increment dept ID counter |
| `Organization(org_id)` | `Organization` | Org record |
| `Department(dept_id)` | `Department` | Department record |
| `OrgDepartments(org_id)` | `Vec<u128>` | All dept IDs in an org |
| `DepartmentChildren(parent_dept_id)` | `Vec<u128>` | Direct child dept IDs |
| `EmployeeInDepartment(dept_id, addr)` | `()` | Membership flag |
| `EmployeeDepartment(addr, org_id)` | `u128` | Employee → current dept ID in org |
| `DepartmentEmployees(dept_id)` | `Vec<Address>` | All employees in a dept |
| `OrgByName(name)` | `u128` | Reverse index: organization name → org_id (enforces name uniqueness) |

---

## Hierarchical Model

```
Organization (org_id)
 ├── Department A (top-level, parent_id = None)          depth 0
 │    ├── Department B (parent_id = A)                   depth 1
 │    │    └── Department C (parent_id = B)              depth 2
 │    └── Department D (parent_id = A)                   depth 1
 └── Department E (top-level, parent_id = None)          depth 0
```

- One organization has many departments.
- A department can have a parent department (optional), forming a tree.
- Maximum hierarchy depth is **10** (root = depth 0, deepest leaf = depth 10).
- Each employee in an org is assigned to **at most one** department at a time.
- Reassignment is atomic and removes from the previous department.
- Departments can be reparented via `update_department`. Cycles and depth violations are rejected.

### Failure Modes

| Condition | Error message |
|-----------|--------------|
| `create_organization` with a `name` already in use by any existing org | `"Organization name already in use"` |
| `create_organization` before `initialize` | `"Contract not initialized"` |
| `create_department` with non-existent org | `"Organization not found"` |
| `create_department` by non-owner | `"Not organization owner"` |
| `create_department` with non-existent parent | `"Parent department not found"` |
| `create_department` with parent in different org | `"Parent must be in same org"` |
| `create_department` that would exceed depth 10 | `"Max hierarchy depth exceeded"` |
| `update_department` on non-existent dept | `"Department not found"` |
| `update_department` by non-owner | `"Not organization owner"` |
| `update_department` with non-existent new parent | `"Parent department not found"` |
| `update_department` with new parent in different org | `"Parent must be in same org"` |
| `update_department` that would exceed depth 10 | `"Max hierarchy depth exceeded"` |
| `update_department` that would create a cycle | `"Cycle detected"` |
| `remove_department` with active employees | `"Cannot remove department with active employees"` |
| `remove_department` with child departments | `"Cannot remove department with child departments"` |
| `remove_department` by non-owner | `"Not organization owner"` |
| `remove_department` on non-existent dept | `"Department not found"` |

## Running Tests

```bash
cd onchain
cargo test -p department_manager -- --nocapture
```

### Test Coverage

The test suite covers:

- Initialization (once; double-init panics)
- Organization creation and retrieval
- Department creation: top-level, nested, sequential IDs
- Depth limit: boundary (depth 10 is valid), enforcement (depth 11 panics)
- `update_department` (reparent): valid moves, top-level promotion
- Cycle detection: direct, indirect, and self-cycles all rejected
- Depth enforcement on reparent
- Property tests:
  - Linear chain of MAX_DEPTH+1 nodes has correct parent links
  - Sequence of valid reparents leaves tree acyclic
  - All 6 possible cycle-creating moves in a 4-node chain are rejected
  - Subtree move preserves all descendant relationships
- Employee assignment, reassignment, removal
- Reverse-index consistency (see [Reverse-Index Invariant](#reverse-index-invariant)):
  - Full assign → remove → reassign cycle leaves both the forward index (`get_employee_department`) and the reverse index (`get_department_employees`) pointing exclusively at the new department, with no stale references to earlier departments
  - After an employee is moved to a new department, the original department's employee list no longer includes them, even when other employees remain in it
- Access control: all mutating ops reject non-owners
- Cross-org isolation
- Unique-organization-id guard (Issue #917):
  - Duplicate name with same owner is rejected with `"Organization name already in use"`
  - Duplicate name with different owner is rejected with `"Organization name already in use"`
  - Rejected duplicate-name attempt leaves the original org record and its full department tree (`get_org_departments`) intact
  - Symbol case-sensitivity documented (different case ⇒ different id)
  - Failed attempt does not consume a `NextOrgId` slot (sequential ids remain gap-free after a rejected attempt)
  - Many distinct names are accepted in sequence; verifies that legitimate creates still get strictly increasing, sequential ids (no gap, no skip)
- Department removal (Issue #1094):
  - Removal with active employees is rejected with `"Cannot remove department with active employees"`
  - Removal with child departments is rejected with `"Cannot remove department with child departments"`
  - Removal succeeds after employees are reassigned to another department
  - Removal succeeds after employees are removed from the org
  - Removal properly updates parent's children list and org's department list
  - Access control: non-owner cannot remove departments
