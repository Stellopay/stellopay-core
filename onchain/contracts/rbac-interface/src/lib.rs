//! Shared RBAC types and client for cross-contract calls.
//!
//! Depend on this crate (rlib only) from other contracts. Deploy the `rbac`
//! contract crate separately — do not link `rbac` as a cdylib dependency.
//!
//! # Override-safety classification
//!
//! Every method on [`RbacContractInterface`] is tagged with one of two
//! classifications that future implementers **must** respect:
//!
//! * **`SECURITY-CRITICAL`** — the method encodes an access-control or ownership invariant. The
//!   default implementation provided by `rbac` **must not** be weakened, bypassed, or omitted by an
//!   alternative implementation. A subtle override here can silently degrade access control across
//!   every contract that depends on this interface.
//!
//! * **`SAFELY-CUSTOMIZABLE`** — the method is informational. The implementation may add metadata,
//!   paginate, reorder, or otherwise reshape the result, **provided** the result remains truthful
//!   with respect to the underlying state (i.e. it never reports roles that have not actually been
//!   granted, and never hides roles that have).
//!
//! Reviewers must consult `docs/rbac.md` for the full checklist used
//! when auditing a new implementer's overrides.
//!
//! # NatSpec-style tags
//!
//! The doc comments in this file use a small NatSpec-derived vocabulary
//! in addition to standard Rust doc sections:
//!
//! | Tag                       | Meaning                                                                 |
//! |---------------------------|-------------------------------------------------------------------------|
//! | `@customization-safety`   | Override-safety classification (`SECURITY-CRITICAL` / `SAFELY-CUSTOMIZABLE`). |
//! | `@invariant`              | The invariant the method protects. Must hold after any implementation.  |
//! | `@risk-if-overridden`     | Concrete ways an override can weaken the system.                        |
//!
//! Standard tags (`@notice`, `@dev`, `@param`, `@return`, `@panic`) are
//! preserved from the original contract documentation.

#![no_std]

use soroban_sdk::{contractclient, contracttype, Address, Env, Vec};

/// Core roles supported by the RBAC contract (must stay in sync with `rbac`).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Role {
    Admin,
    Employer,
    Employee,
    Arbiter,
}

/// Client for the deployed RBAC contract.
///
/// Every method on this trait carries an `@customization-safety` tag and
/// an `@invariant` description. Reviewers must read both before approving
/// any new implementer.
#[contractclient(name = "RbacContractClient")]
pub trait RbacContractInterface {
    /// One-time initialization. Assigns `owner` the `Admin` role.
    ///
    /// @customization-safety SECURITY-CRITICAL
    /// @invariant After `initialize` succeeds:
    ///   1. The contract is permanently marked initialized (cannot be re-initialized).
    ///   2. The supplied `owner` address holds the `Admin` role and is recorded as the contract
    ///      owner.
    ///   3. No other address holds any role yet.
    /// @risk-if-overridden Skipping the `already initialized` guard allows
    ///   an attacker to overwrite the owner record and seize the contract.
    ///   Omitting the bootstrap `Admin` grant for `owner` creates a
    ///   permanently unowned contract (lockout on day one).
    ///
    /// # Caller authorization
    /// - `owner` must sign (caller authenticates themselves).
    ///
    /// # Panics
    /// - `"Already initialized"` — if `initialize` has already been called.
    fn initialize(env: Env, owner: Address);

    /// Returns `true` when `addr` holds a role that implies `required`
    /// (inheritance-aware: `Admin` implies all roles; `Employer` implies
    /// `Employer` and `Employee`; other roles imply only themselves).
    ///
    /// @customization-safety SECURITY-CRITICAL
    /// @invariant `has_role(addr, required)` returns `true` **iff** `addr`
    ///   holds a directly-assigned role `r` such that `role_implies(r, required)`.
    ///   Both directions must hold:
    ///   * `has_role(addr, required) == true` ⇒ `require_role(addr, required)` must NOT panic.
    ///   * `has_role(addr, required) == false` ⇒ `require_role(addr, required)` MUST panic with
    ///     `"Missing required role"` (or a stable equivalent used by tests).
    ///   Implementers must keep these two functions in lock-step; any drift
    ///   is a system-wide authorization bypass.
    /// @risk-if-overridden Returning `true` when no implied role is held
    ///   silently bypasses every authorization check in the system. Returning
    ///   `false` for a legitimately-held role locks the user out. Either
    ///   failure mode is exploitable. Returning `false` when `has_role`
    ///   would return `true` (or vice versa) desynchronizes this method
    ///   from `require_role` and produces inconsistent answers across
    ///   integrating contracts.
    ///
    /// # Caller authorization
    /// - Unauthenticated — any address may call.
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    fn has_role(env: Env, addr: Address, required: Role) -> bool;

    /// Returns all roles directly assigned to `addr` (no inheritance).
    ///
    /// @customization-safety SAFELY-CUSTOMIZABLE
    /// @invariant The returned vector MUST contain **exactly** the set of
    ///   roles that have been granted to `addr` and not subsequently
    ///   revoked. Concretely:
    ///   * `returned ⊇ { r | r is currently stored for addr }` — no omitted roles.
    ///   * `returned ⊆ { r | r is currently stored for addr }` — no fabricated roles, even if the
    ///     caller asks for an unrelated role to "appear" in the response.
    ///   * `returned` contains no duplicates.
    ///   * Order, pagination, and additional metadata are implementation-defined.
    /// @safe-customizations Implementers MAY add pagination wrappers,
    ///   apply additional filters, or change element ordering. Implementers
    ///   MUST NOT fabricate roles that were never granted and MUST NOT
    ///   omit roles that are currently held, because callers rely on this
    ///   query to inspect the authoritative role set (and off-chain indexers
    ///   use it to reconcile state). Even though this method is
    ///   `SAFELY-CUSTOMIZABLE`, **fabricating a role in the response is a
    ///   security violation** — it would let a malicious implementer make
    ///   arbitrary addresses appear to hold any role (social-engineering
    ///   vector against off-chain UIs and indexers).
    ///
    /// # Caller authorization
    /// - Unauthenticated — any address may call.
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    fn get_roles(env: Env, addr: Address) -> Vec<Role>;

    /// Returns the current contract owner.
    ///
    /// @customization-safety SECURITY-CRITICAL
    /// @invariant Returns the same address that was written by the most
    ///   recent successful `accept_ownership` (or `initialize`, when no
    ///   transfer has occurred). The returned address must equal the
    ///   address whose `Admin` role is protected by `revoke_role`,
    ///   `revoke_all`, and `bulk_revoke`.
    /// @risk-if-overridden Returning a stale or attacker-controlled address
    ///   breaks `transfer_ownership` authorization (any caller could
    ///   satisfy `caller == owner`) and disables the owner-lockout
    ///   protection in `revoke_role` / `revoke_all` / `bulk_revoke`.
    ///
    /// # Caller authorization
    /// - Unauthenticated — any address may call.
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    /// - `"Owner not set"` — if no owner has been stored (defensive; should not occur after a
    ///   successful `initialize`).
    fn owner(env: Env) -> Address;

    /// Grants `role` to `target`.
    ///
    /// Duplicate grants are silently skipped.
    ///
    /// @customization-safety SECURITY-CRITICAL
    /// @invariant The function reverts unless:
    ///   1. The contract has been initialized.
    ///   2. `caller` authenticates the transaction.
    ///   3. `caller` holds a role that implies `Admin`.
    /// After a successful call, `target` is recorded as holding `role`
    /// (if not already held).
    /// @risk-if-overridden Removing the admin-only check allows any
    ///   user to mint themselves `Admin` (full takeover). Removing
    ///   authentication allows a relayer to grant roles on behalf of
    ///   any address without consent.
    ///
    /// # Caller authorization
    /// - `caller` must sign and must hold a role that implies `Admin` (i.e. must have the `Admin`
    ///   role).
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    /// - `"Only admin can grant roles"` — if `caller` does not have a role implying `Admin`.
    fn grant_role(env: Env, caller: Address, target: Address, role: Role);

    /// Revokes `role` from `target`.
    ///
    /// Revoking a role the target does not hold is a silent no-op. The
    /// contract owner's `Admin` role is protected and cannot be revoked
    /// (prevents lockout).
    ///
    /// @customization-safety SECURITY-CRITICAL
    /// @invariant The function reverts unless:
    ///   1. The contract has been initialized.
    ///   2. `caller` authenticates the transaction.
    ///   3. `caller` holds a role that implies `Admin`.
    /// Additionally, the function reverts when `target == owner` and
    /// `role == Admin` (lockout protection).
    /// @risk-if-overridden Removing the admin-only check allows any user
    ///   to revoke anyone's roles. Removing the owner-Admin protection
    ///   allows revoking the last admin (permanent lockout).
    ///
    /// # Caller authorization
    /// - `caller` must sign and must hold a role that implies `Admin`.
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    /// - `"Only admin can revoke roles"` — if `caller` does not have a role implying `Admin`.
    /// - `"Cannot revoke Admin from owner"` — if `target` is the contract owner and `role` is
    ///   `Admin`.
    fn revoke_role(env: Env, caller: Address, target: Address, role: Role);

    /// Grants multiple roles to `target` in a single call.
    ///
    /// Duplicates within the batch or roles already held are silently
    /// skipped.
    ///
    /// @customization-safety SECURITY-CRITICAL
    /// @invariant Bulk grant enforces the **same** authorization checks as
    ///   `grant_role`, applied **once per call** (not per element):
    ///   caller must authenticate and must hold a role implying `Admin`.
    ///   After a successful call, `target` is recorded as holding every
    ///   role in `roles` that it did not already hold. Duplicates within
    ///   the batch and roles already held are silent no-ops. The call
    ///   must be all-or-nothing: either every role in `roles` is granted
    ///   (modulo duplicates) or none of them are.
    /// @risk-if-overridden Performing the admin check per-element (rather
    ///   than once for the whole batch) opens two real failure modes:
    ///   1. **Partial application on revert** — if the check happens inside the per-element loop, a
    ///      revert mid-batch leaves `target` with some roles granted and others not, violating the
    ///      all-or-nothing invariant and corrupting the role set.
    ///   2. **Bypass via batch splitting** — an implementer that, for example, only enforces the
    ///      admin check for `Role::Admin` and forgets it for `Role::Employer`, would let non-admins
    ///      grant Employer roles by packaging them in a batch.
    ///   Implementers must run the admin check once, before any element is
    ///   processed, exactly as `grant_role` does.
    ///
    /// # Caller authorization
    /// - `caller` must sign and must hold a role that implies `Admin`.
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    /// - `"Only admin can grant roles"` — if `caller` does not have a role implying `Admin`.
    fn bulk_grant(env: Env, caller: Address, target: Address, roles: Vec<Role>);

    /// Revokes every role from `target`.
    ///
    /// The contract owner is protected — calling `revoke_all` on the owner
    /// address is forbidden (prevents lockout).
    ///
    /// @customization-safety SECURITY-CRITICAL
    /// @invariant The function reverts unless:
    ///   1. The contract has been initialized.
    ///   2. `caller` authenticates the transaction.
    ///   3. `caller` holds a role that implies `Admin`.
    ///   4. `target != owner` (lockout protection).
    /// On success, every role currently assigned to `target` is removed.
    /// @risk-if-overridden Removing the owner-protection check creates a
    ///   one-call path to a fully-locked-out contract. Removing the
    ///   admin-only check lets any user strip roles from anyone.
    ///
    /// # Caller authorization
    /// - `caller` must sign and must hold a role that implies `Admin`.
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    /// - `"Only admin can revoke roles"` — if `caller` does not have a role implying `Admin`.
    /// - `"Cannot revoke all roles from owner"` — if `target` is the contract owner.
    fn revoke_all(env: Env, caller: Address, target: Address);

    /// Reverts unless `addr` holds a role that implies `required`.
    ///
    /// Designed as an inline guard for integrating contracts.
    ///
    /// @customization-safety SECURITY-CRITICAL
    /// @invariant The function panics with `"Missing required role"` if
    ///   and only if `has_role(addr, required)` would return `false`.
    ///   The function panics unless `addr` has authenticated the
    ///   transaction.
    /// @risk-if-overridden This is the **primary** integration guard used
    ///   by every downstream contract. Any override that allows the call
    ///   to succeed when `has_role` would return `false` is a system-wide
    ///   authorization bypass.
    ///
    /// # Caller authorization
    /// - `addr` must sign (the address being checked authenticates themselves).
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    /// - `"Missing required role"` — if `addr` does not hold a role implying `required`.
    fn require_role(env: Env, addr: Address, required: Role);

    /// Proposes a new contract owner (two-step transfer, step 1).
    ///
    /// The pending owner has no privileges until they call
    /// `accept_ownership`.
    ///
    /// @customization-safety SECURITY-CRITICAL
    /// @invariant The function reverts unless:
    ///   1. The contract has been initialized.
    ///   2. `caller` authenticates the transaction.
    ///   3. `caller == owner()` (holding `Admin` is **not** sufficient; ownership and Admin are
    ///      deliberately separated so an Admin-grant cannot unilaterally transfer ownership).
    /// On success, `new_owner` is stored as the pending owner but has
    /// no privileges until `accept_ownership` is called.
    /// @risk-if-overridden Allowing non-owners to set the pending owner
    ///   lets any admin-stage a takeover (the pending owner can later
    ///   accept). Storing the pending owner without requiring the current
    ///   owner disables the two-step guarantee.
    ///
    /// # Caller authorization
    /// - `caller` must sign and must be the current contract owner. Holding the `Admin` role alone
    ///   is **not** sufficient.
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    /// - `"Only owner can transfer ownership"` — if `caller` is not the current owner.
    fn transfer_ownership(env: Env, caller: Address, new_owner: Address);

    /// Accepts a pending ownership transfer (two-step transfer, step 2).
    ///
    /// On acceptance:
    /// 1. The `Admin` role is granted to the new owner.
    /// 2. The `Admin` role is revoked from the old owner.
    /// 3. The ownership record is updated.
    /// 4. The pending-owner slot is cleared.
    ///
    /// @customization-safety SECURITY-CRITICAL
    /// @invariant The function reverts unless:
    ///   1. The contract has been initialized.
    ///   2. `caller` authenticates the transaction.
    ///   3. A pending owner is recorded.
    ///   4. `caller` equals the recorded pending owner.
    /// On success:
    ///   * The contract owner becomes `caller`.
    ///   * The prior owner's `Admin` role is revoked.
    ///   * `caller` is granted `Admin` (if not already held; idempotent if already held).
    ///   * The pending-owner slot is cleared (no stale proposal remains).
    /// @risk-if-overridden Skipping the pending-owner check lets any
    ///   address finalize a transfer they were not proposed for. Skipping
    ///   the grant/revoke of `Admin` leaves stale permissions behind —
    ///   the old owner would still hold `Admin` after the transfer.
    ///   **Failing to clear the pending-owner slot** is also a security
    ///   violation: a stale proposal can be reused after a second
    ///   `transfer_ownership` cycle, letting the originally-proposed
    ///   pending owner accept a transfer they were never re-proposed for.
    ///
    /// # Caller authorization
    /// - `caller` must sign and must be the pending owner set by a prior `transfer_ownership` call.
    ///
    /// # Panics
    /// - `"Contract not initialized"` — if `initialize` has not been called.
    /// - `"No pending owner"` — if `transfer_ownership` has not been called first (no pending
    ///   proposal exists).
    /// - `"Caller is not pending owner"` — if `caller` does not match the stored pending owner.
    fn accept_ownership(env: Env, caller: Address);
}
