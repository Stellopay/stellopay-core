# Role-Based Access Control (RBAC)

This document describes the RBAC contract for fine-grained, role-based permission management across Stellopay contracts.

## Overview

The `rbac` contract provides:

- **Multiple roles** – `Admin`, `Employer`, `Employee`, and `Arbiter`.
- **Role inheritance** – Higher-privilege roles implicitly satisfy lower-privilege checks.
- **Multiple roles per address** – An address can hold more than one role simultaneously.
- **Permission checks** – Helper functions for on-chain contracts to enforce role-based authorization.

## Contract Location

- **Contract**: `onchain/contracts/rbac/src/lib.rs`
- **Tests**: `onchain/contracts/rbac/tests/test_rbac.rs`

## Roles and Inheritance

The core roles are:

- `Admin` – Global administrator, implicitly has all roles.
- `Employer` – Represents an employer; implicitly has `Employee` privileges.
- `Employee` – Base role for payroll participants.
- `Arbiter` – Used for dispute resolution flows.

Inheritance rules:

- `Admin` ⇒ `Admin`, `Employer`, `Employee`, `Arbiter`
- `Employer` ⇒ `Employer`, `Employee`
- `Employee` ⇒ `Employee`
- `Arbiter` ⇒ `Arbiter`

When checking permissions, the contract evaluates whether any role assigned to an address implies the required role using these rules.

## API

### Initialization

- `initialize(owner)` – One-time initialization. Sets the `owner` and grants the `Admin` role to `owner`.

### Role Management

- `grant_role(caller, target, role)` – Grants `role` to `target`. `caller` must authenticate and have the `Admin` role.
- `revoke_role(caller, target, role)` – Revokes `role` from `target`. `caller` must authenticate and have the `Admin` role.
- `get_roles(addr)` – Returns the vector of roles directly assigned to `addr` (without inheritance).

### Permission Checks

- `has_role(addr, required)` – Returns `true` if `addr` has a role that (directly or via inheritance) implies `required`.
- `require_role(addr, required)` – Reverts if `addr` does not have a role that implies `required`. Intended for use by integrating contracts to enforce authorization.

## Security Considerations

- Initialization is one-time and restricted to the deployer/owner.
- Only addresses with the `Admin` role can grant or revoke roles.
- Role inheritance is explicit and documented; adding new roles or inheritance paths should be done carefully to avoid privilege escalation.
- The contract does not perform token transfers; it only manages access control state.

