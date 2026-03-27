# Role-Based Access Control (RBAC)

Centralized role management for all Stellopay on-chain modules.

## Overview

The `rbac` contract provides:

- **Multiple roles** – `Admin`, `Employer`, `Employee`, and `Arbiter`.
- **Role inheritance** – Higher-privilege roles implicitly satisfy lower-privilege checks.
- **Multiple roles per address** – An address can hold more than one role simultaneously.
- **Permission checks** – Helper functions for on-chain contracts to enforce role-based authorization.
- **Bulk operations** – Grant multiple roles in one call or revoke all roles at once.
- **Two-step ownership transfer** – Prevents accidental or malicious owner changes.
- **Owner lockout protection** – The owner's Admin role cannot be revoked.

## Contract Location

- **Contract**: `onchain/contracts/rbac/src/lib.rs`
- **Tests**: `onchain/contracts/rbac/tests/test_rbac.rs`

## Roles and Inheritance

| Role     | Implies                              | Typical use                       |
|----------|--------------------------------------|-----------------------------------|
| Admin    | Admin, Employer, Employee, Arbiter   | System administration, upgrades   |
| Employer | Employer, Employee                   | Create payrolls, manage employees |
| Employee | Employee                             | Claim payroll, view status        |
| Arbiter  | Arbiter                              | Dispute resolution                |

Inheritance graph:

```
Admin ──▶ Employer ──▶ Employee
  │
  └──▶ Arbiter
```

- `Admin` implies every other role.
- `Employer` implies `Employee` (an employer can do anything an employee can).
- `Employee` and `Arbiter` are leaf roles with no further implications.

When checking permissions, the contract evaluates whether any role assigned to an address implies the required role using these rules.

## API

### Initialization

| Function               | Access | Description                                |
|------------------------|--------|--------------------------------------------|
| `initialize(owner)`   | Once   | Bootstrap the contract; `owner` gets Admin |

### Role management

| Function                                   | Access | Description                      |
|--------------------------------------------|--------|----------------------------------|
| `grant_role(caller, target, role)`         | Admin  | Grant a single role              |
| `revoke_role(caller, target, role)`        | Admin  | Revoke a single role             |
| `bulk_grant(caller, target, roles)`        | Admin  | Grant multiple roles in one call |
| `revoke_all(caller, target)`              | Admin  | Strip all roles from an address  |

### Queries

| Function                        | Access | Description                                |
|---------------------------------|--------|--------------------------------------------|
| `get_roles(addr)`              | Any    | List directly assigned roles               |
| `has_role(addr, required)`     | Any    | Inheritance-aware role check               |
| `require_role(addr, required)` | Any    | Revert if role missing (for integrations)  |
| `owner()`                      | Any    | Current contract owner                     |

### Ownership transfer (two-step)

| Function                              | Access        | Description                             |
|---------------------------------------|---------------|-----------------------------------------|
| `transfer_ownership(caller, new)`     | Owner only    | Propose new owner (no immediate effect) |
| `accept_ownership(caller)`            | Pending owner | Accept and finalize the transfer        |

## Security properties

### Owner lockout protection

The contract prevents the owner's `Admin` role from being revoked via `revoke_role` or `revoke_all`. This ensures at least one address always retains administrative access.

### Two-step ownership transfer

Ownership cannot be transferred in a single call. The current owner proposes a new owner, and the new owner must explicitly accept. This prevents:

- Accidental transfer to a wrong address.
- Transfer to an address that cannot sign transactions (e.g., a contract without appropriate logic).

On acceptance, the old owner's `Admin` role is automatically revoked and the new owner receives `Admin`.

### Initialization guard

Every mutating and query function checks the `Initialized` flag. Calling any function before `initialize` reverts, preventing use of an unconfigured contract.

### Duplicate grant idempotency

Granting a role that is already held is a no-op — it does not create duplicate entries in storage.

## Threat model

| Threat                           | Mitigation                                                                          |
|----------------------------------|-------------------------------------------------------------------------------------|
| Admin takeover                   | Only existing Admin can grant Admin; two-step ownership transfer                    |
| Owner lockout                    | Cannot revoke Admin from owner via `revoke_role` or `revoke_all`                    |
| Privilege escalation             | Non-admin roles cannot call grant/revoke                                            |
| Re-initialization                | `Already initialized` guard                                                         |
| Role cycling (grant/revoke spam) | On-chain events emitted for all mutations; off-chain monitoring detects anomalies   |
| Stale permissions after transfer | `accept_ownership` atomically revokes old owner's Admin                             |

## Events

All state-changing operations emit Soroban events for off-chain indexing:

| Topic                  | Data             | Emitted by           |
|------------------------|------------------|----------------------|
| `("RBAC", "init")`    | `owner`          | `initialize`         |
| `("RBAC", "grant")`   | `(target, role)` | `grant_role`         |
| `("RBAC", "revoke")`  | `(target, role)` | `revoke_role`        |
| `("RBAC", "propose")` | `new_owner`      | `transfer_ownership` |
| `("RBAC", "owner")`   | `new_owner`      | `accept_ownership`   |

## Integration

Other contracts can call the RBAC contract to enforce permissions:

```rust
// In your contract:
let rbac = RbacContractClient::new(&env, &rbac_contract_id);
rbac.require_role(&caller, &Role::Employer);
// ... proceed with employer-only logic
```

## Test coverage (56 tests)

The test suite validates:

- **Initialization** (3 tests): single-init enforcement, owner bootstrapping, re-init with different owner
- **Happy paths** (6 tests): grant, revoke, duplicate grant no-op, revoke absent role no-op, second admin, multi-user grants
- **Forbidden grant paths** (4 tests): non-admin, employer, employee, and arbiter all blocked from granting
- **Forbidden revoke paths** (3 tests): non-admin, employer blocked from revoking; owner Admin protected
- **Inheritance matrix** (6 tests): exhaustive 4×4 truth table plus individual role checks and multi-role combinations
- **require_role enforcement** (5 tests): valid role, inherited role, missing role, cross-role failures
- **Bulk operations** (6 tests): bulk grant, duplicate skip, non-admin blocked, revoke-all, owner protected, non-admin blocked
- **Ownership transfer** (7 tests): full lifecycle, post-transfer grant, non-owner blocked, non-owner-admin blocked, wrong acceptor, no proposal, old-owner-loses-power
- **Uninitialized guards** (8 tests): every public function reverts before init
- **Security scenarios** (8 tests): role cycling, zero-role address, address isolation, delegated admin, post-transfer protection, revoked-admin-loses-power
