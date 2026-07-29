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
- **Interface trait**: `onchain/contracts/rbac-interface/src/lib.rs`
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

### Employee Role Mapping

The `rbac` contract serves as the source of truth for the `employee_roles` module. The following mapping is used:

| RBAC Role | Employee Role |
|-----------|---------------|
| `Admin`    | `Admin`        |
| `Employer` | `Manager`      |
| `Employee` | `Employee`     |
| `Arbiter`  | (N/A)          |

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

## Override-safety classification

Every method on `RbacContractInterface` is tagged as either
`SECURITY-CRITICAL` or `SAFELY-CUSTOMIZABLE`. The classification lives
in the Rust doc comments in `onchain/contracts/rbac-interface/src/lib.rs`
as the `@customization-safety` NatSpec-style tag, accompanied by
`@invariant` and `@risk-if-overridden` tags.

> **NatSpec-style tags used in this codebase**
>
> | Tag                       | Meaning                                                                 |
> |---------------------------|-------------------------------------------------------------------------|
> | `@customization-safety`   | Override-safety classification (`SECURITY-CRITICAL` / `SAFELY-CUSTOMIZABLE`). |
> | `@invariant`              | The invariant the method protects. Must hold after any implementation.  |
> | `@risk-if-overridden`     | Concrete ways an override can weaken the system.                        |
> | `@safe-customizations`    | (Optional, on `SAFELY-CUSTOMIZABLE` methods.) Customizations that are explicitly allowed. |
>
> Standard tags (`@notice`, `@dev`, `@param`, `@return`, `@panic`) are
> preserved from the original contract documentation.

| Method                                   | Classification         | Invariant summary                                                              |
|------------------------------------------|------------------------|--------------------------------------------------------------------------------|
| `initialize(env, owner)`                 | **SECURITY-CRITICAL**  | One-shot bootstrap; `owner` always receives `Admin`.                           |
| `has_role(env, addr, required)`          | **SECURITY-CRITICAL**  | Returns `true` iff `addr` holds a role implying `required`.                    |
| `get_roles(env, addr)`                   | **SAFELY-CUSTOMIZABLE**| Returns exactly the directly-assigned roles for `addr`.                        |
| `owner(env)`                             | **SECURITY-CRITICAL**  | Returns the address recorded by the most recent `accept_ownership` (or init).  |
| `grant_role(env, caller, target, role)`  | **SECURITY-CRITICAL**  | Only callers holding a role implying `Admin` may grant.                        |
| `revoke_role(env, caller, target, role)` | **SECURITY-CRITICAL**  | Admin-only; owner's `Admin` is protected from revocation.                     |
| `bulk_grant(env, caller, target, roles)` | **SECURITY-CRITICAL**  | Per-call Admin-only check (not per-element).                                   |
| `revoke_all(env, caller, target)`        | **SECURITY-CRITICAL**  | Admin-only; target must not be the owner.                                      |
| `require_role(env, addr, required)`      | **SECURITY-CRITICAL**  | Primary integration guard; panics iff `has_role` would return `false`.        |
| `transfer_ownership(env, caller, new)`   | **SECURITY-CRITICAL**  | Caller must equal `owner()` (Admin alone is insufficient).                     |
| `accept_ownership(env, caller)`          | **SECURITY-CRITICAL**  | Caller must equal the recorded pending owner; atomic Admin grant/revoke.       |

> **Rule of thumb:** if a method name appears in any authorization check
> elsewhere in the system, it is `SECURITY-CRITICAL`. The single exception
> is `get_roles`, which is purely informational; it is `SAFELY-CUSTOMIZABLE`
> but its output must remain truthful with respect to the underlying
> role set.

## Reviewer checklist for new `RbacContractInterface` implementers

Use this checklist when reviewing a PR that introduces a new
implementation of `RbacContractInterface` (e.g. an alternative `rbac`
contract, a shim for testing, or a hardened production variant).
Every box must be checkable; if any item is unchecked, **block** the PR.

### A. Initialization invariant

- [ ] `initialize` reverts if the contract is already initialized (`"Already initialized"`).
- [ ] `initialize` requires `owner.require_auth()` so the bootstrap admin consents.
- [ ] On success, `owner` is stored in the owner slot **and** is granted `Admin`.
- [ ] No other address receives any role during `initialize`.
- [ ] The implementation emits an event equivalent to `("RBAC", "init", owner)` (off-chain indexers depend on it).

### B. Core authorization invariants

- [ ] `has_role(addr, required)` agrees with `require_role(addr, required)` for every (addr, required) pair: `has_role` is `true` ⇔ `require_role` does **not** panic.
- [ ] `has_role` honors the inheritance rules: `Admin` implies all roles; `Employer` implies `Employer` and `Employee`; `Employee` implies `Employee`; `Arbiter` implies `Arbiter`.
- [ ] `require_role` requires `addr.require_auth()` before the role check; calling it without authentication must panic.
- [ ] `require_role` panics with `"Missing required role"` (or a stable equivalent string used by tests) when `has_role` is `false`.

### C. Privilege-escalation invariants

- [ ] `grant_role` reverts with `"Only admin can grant roles"` (or stable equivalent) when the caller does not hold a role implying `Admin`.
- [ ] `grant_role` requires `caller.require_auth()`.
- [ ] `bulk_grant` enforces the **same** Admin-only check as `grant_role` once per call, not per element.
- [ ] Granting a role that is already held is a silent no-op (no duplicate entries in storage).

### D. Privilege-revocation invariants

- [ ] `revoke_role` reverts with `"Only admin can revoke roles"` when the caller is not Admin.
- [ ] `revoke_role` reverts with `"Cannot revoke Admin from owner"` when `target == owner && role == Admin`.
- [ ] `revoke_role` requires `caller.require_auth()`.
- [ ] `revoke_all` reverts with `"Cannot revoke all roles from owner"` when `target == owner`.
- [ ] `revoke_all` requires the Admin-only check and `caller.require_auth()`.
- [ ] Revoking a role the target does not hold is a silent no-op.
- [ ] (Implementation-only) If the implementation additionally exposes `bulk_revoke`, it must enforce the same Admin-only check as `revoke_role` **once per call** (not per element), and must skip attempts to revoke `Admin` from the owner within the batch — mirroring `bulk_grant`'s all-or-nothing rule. **Note:** `bulk_revoke` is not part of the `RbacContractInterface` trait; this item applies only if the implementer chooses to add it.

### E. Ownership tracking invariants

- [ ] `owner()` returns exactly the address most recently written by `accept_ownership` (or `initialize`, if no transfer has occurred).
- [ ] `owner()` panics if `initialize` has not been called (`"Contract not initialized"` or stable equivalent).
- [ ] `owner()` is consistent with `revoke_role`/`revoke_all` lockout protection — i.e. the address reported by `owner()` is the one whose `Admin` role cannot be revoked.

### F. Two-step ownership-transfer invariants

- [ ] `transfer_ownership` requires `caller.require_auth()`.
- [ ] `transfer_ownership` reverts with `"Only owner can transfer ownership"` when `caller != owner()`.
- [ ] Holding `Admin` alone is **not** sufficient to call `transfer_ownership`.
- [ ] `accept_ownership` requires `caller.require_auth()`.
- [ ] `accept_ownership` reverts with `"No pending owner"` if `transfer_ownership` was not called first.
- [ ] `accept_ownership` reverts with `"Caller is not pending owner"` if the caller is not the recorded pending owner.
- [ ] On a successful `accept_ownership`: (1) the new owner is granted `Admin`, (2) the previous owner has `Admin` revoked, (3) `owner()` returns the new owner, and (4) the pending-owner slot is cleared.

### G. `get_roles` invariants (the one safely-customizable method)

> ⚠️ **Even though `get_roles` is `SAFELY-CUSTOMIZABLE`, fabricating a role
> in the response is a security violation.** A malicious implementer
> could otherwise make arbitrary addresses appear to hold any role
> (social-engineering vector against off-chain UIs and indexers). The
> set of returned roles must always equal the authoritative stored role
> set; only ordering, pagination, and additional metadata may differ.

- [ ] `get_roles(addr)` returns a vector containing **only** roles that have been granted and not revoked for `addr`.
- [ ] `get_roles(addr)` never fabricates roles the address does not actually hold.
- [ ] The returned vector has no duplicates.
- [ ] For an address with no granted roles, `get_roles(addr)` returns an empty vector (never panics, never returns a phantom role).
- [ ] The implementation does not include inherited roles in the response (inheritance is reported via `has_role`, not `get_roles`).

### H. Initialization guard

- [ ] Every method that touches state panics with `"Contract not initialized"` (or stable equivalent) when called before `initialize`.

### I. Documentation hygiene

- [ ] Every method on the implementation has a doc comment including the `@customization-safety`, `@invariant`, and (where applicable) `@risk-if-overridden` tags.
- [ ] The classification matches the table above; if it does not, the PR description explains why and the change is gated on a security review.
- [ ] At least one test exists for each `SECURITY-CRITICAL` invariant on the new implementer.
- [ ] All tests pass locally and in CI: `cargo test -p rbac`.
- [ ] A coverage report is attached to the PR (e.g. `cargo tarpaulin -p rbac --out Html` or equivalent) showing ≥ 95% line coverage on the new implementer. Coverage below the threshold must be justified in the PR description.
- [ ] CI runs `cargo doc -p rbac-interface --no-deps` and fails the build if any method on `RbacContractInterface` is missing a `@customization-safety` tag — this turns the classification into an enforced invariant rather than documentation that can rot.

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
| Subtle override weakening access | `@customization-safety` doc tags + reviewer checklist above                         |

## Events

All state-changing operations emit Soroban events for off-chain indexing:

| Topic                  | Data                              | Emitted by           |
|------------------------|-----------------------------------|----------------------|
| `("RBAC", "init")`    | `owner`                           | `initialize`         |
| `("RBAC", "grant")`   | `(target, role)`                  | `grant_role`         |
| `("RBAC", "revoke")`  | `(target, role)`                  | `revoke_role`        |
| `("RBAC", "propose")` | `new_owner`                       | `transfer_ownership` |
| `("RBAC", "owner")`   | `(previous_owner, new_owner)`     | `accept_ownership`   |

## Integration

Other contracts can call the RBAC contract to enforce permissions:

```rust
// In your contract:
let rbac = RbacContractClient::new(&env, &rbac_contract_id);
rbac.require_role(&caller, &Role::Employer);
// ... proceed with employer-only logic
```

## Test coverage

The test suite in `onchain/contracts/rbac/tests/test_rbac.rs` validates:

- **Initialization** — single-init enforcement, owner bootstrapping, re-init with different owner.
- **Happy paths** — grant, revoke, duplicate grant no-op, revoke absent role no-op, second admin, multi-user grants.
- **Forbidden grant paths** — non-admin, employer, employee, and arbiter all blocked from granting.
- **Forbidden revoke paths** — non-admin, employer blocked from revoking; owner Admin protected.
- **Inheritance matrix** — exhaustive 4×4 truth table plus individual role checks and multi-role combinations.
- **`require_role` enforcement** — valid role, inherited role, missing role, cross-role failures.
- **Bulk operations** — bulk grant, duplicate skip, non-admin blocked, revoke-all, owner protected, non-admin blocked.
- **Ownership transfer** — full lifecycle, post-transfer grant, non-owner blocked, non-owner-admin blocked, wrong acceptor, no proposal, old-owner-loses-power.
- **Uninitialized guards** — every public function reverts before init.
- **Security scenarios** — role cycling, zero-role address, address isolation, delegated admin, post-transfer protection, revoked-admin-loses-power.
- **Override-safety classification** — see the dedicated test module at the bottom of `tests/test_rbac.rs`. Every `SECURITY-CRITICAL` invariant is exercised by a named test, and `get_roles` is verified to be the only `SAFELY-CUSTOMIZABLE` method.