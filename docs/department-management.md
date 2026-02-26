# Department and Organization Management

This document describes the Department/Organization Management contract (issue #237): organizing employees into departments and organizations with hierarchical structures.

## Overview

The `department_manager` contract provides:

- **Organizations** – Top-level entities owned by an address.
- **Departments** – Belong to an organization; can be top-level or nested under another department.
- **Employee assignment** – Assign addresses (employees) to a department within an organization.
- **Department-level reporting** – Employee counts, child departments, and employee lists per department.

## Contract Location

- **Contract**: `onchain/contracts/department_manager/src/lib.rs`
- **Tests**: `onchain/contracts/department_manager/tests/test_department.rs`

## API

### Initialization

- `initialize(admin)` – Sets the admin. Callable once.

### Organizations

- `create_organization(owner, name)` – Creates an organization; `owner` must authenticate. Returns `org_id`.
- `get_organization(org_id)` – Returns the organization record.

### Departments

- `create_department(caller, org_id, name, parent_id)` – Creates a department. `caller` must be the org owner. `parent_id` is `None` for top-level, or a department ID for a child. Returns `dept_id`.
- `get_department(department_id)` – Returns the department record.
- `get_org_departments(org_id)` – Returns the list of department IDs for the organization.

### Employee Assignment

- `assign_employee_to_department(caller, org_id, department_id, employee)` – Assigns `employee` to the given department. `caller` must be org owner. Re-assigning to another department in the same org moves the employee.

### Reporting

- `get_department_employees(department_id)` – Returns the list of employee addresses in the department.
- `get_employee_department(employee, org_id)` – Returns the department ID for the employee in that org, or none.
- `get_department_report(department_id)` – Returns `(employee_count, child_department_ids, employee_addresses)`.

## Security

- Only the org owner can create departments and assign employees.
- Initialization is one-time and restricted to the deployer (admin).
- No token transfers; the contract only manages structure and assignments.

## Hierarchical Model

- One organization has many departments.
- A department can have a parent department (optional), forming a tree.
- Each employee in an org is assigned to at most one department; reassignment updates the previous department’s list.
