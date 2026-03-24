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
Returns `(employee_count, child_department_ids, employee_addresses)` for a department.

---

## Events

All mutating operations publish events for indexer/integrator consumption:

| Event Topic | Data | Trigger |
|-------------|------|---------|
| `("org_crtd", org_id)` | `org_id: u128` | Organization created |
| `("dept_crtd", dept_id)` | `dept_id: u128` | Department created |
| `("emp_asgnd", dept_id)` | `employee: Address` | Employee assigned to department |
| `("emp_rmvd", dept_id)` | `employee: Address` | Employee removed from department |

---

## Security Assumptions

1. **Org ownership is irrevocable**: The owner address set at `create_organization` time is permanent. There is no ownership transfer function.
2. **Admin ≠ Org Owner**: The admin address (set during `initialize`) has no special permissions over org operations. Only the org owner controls their org.
3. **No token transfers**: This contract only manages structure. It holds no funds and cannot move funds.
4. **Single assignment per org**: Each employee has at most one department per org. Reassignment is atomic (remove then add).
5. **Initialization is one-time**: The `Initialized` flag in persistent storage prevents re-initialization even after admin key changes.
6. **Cross-org isolation**: Employee assignments are org-scoped. Being removed from one org does not affect assignments in others.

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

---

## Hierarchical Model

```
Organization (org_id)
 ├── Department A (top-level, parent_id = None)
 │    ├── Department B (parent_id = A)
 │    │    └── Department C (parent_id = B)   ← 3 levels deep
 │    └── Department D (parent_id = A)
 └── Department E (top-level, parent_id = None)
```

- One organization has many departments.
- A department can have a parent department (optional), forming a tree.
- Each employee in an org is assigned to **at most one** department at a time.
- Reassignment is atomic and removes from the previous department.

## Running Tests

```bash
cd onchain
cargo test -p department_manager -- --nocapture
```
