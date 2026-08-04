//! Minimal mock implementer of [`RbacContractInterface`] and conformance tests.
//!
//! # Purpose
//!
//! This module serves two goals:
//!
//! 1. **Guard future implementers.** Any new contract that claims to implement
//!    `RbacContractInterface` should copy the pattern here: define an
//!    implementer, register it against the Soroban test environment, and run
//!    the same invariant assertions against the generated `RbacContractClient`
//!    (`try_` variants so a panic surfaces as `Err`, not a test unwinder crash).
//!
//! 2. **Document the conformance contract in executable form.** The prose in
//!    [`RbacContractInterface`] explains each `@invariant`; this file proves
//!    them for a minimal in-crate mock so that `cargo test -p rbac-interface`
//!    is self-contained.
//!
//! # Why call through `RbacContractClient`?
//!
//! Downstream contracts integrate through the generated `RbacContractClient`,
//! not through a hand-rolled invoke. Exercising the interface through that
//! client covers the generated client code shipped in this crate's `src/` and
//! proves the wire-level selectors line up between the interface and any
//! implementer — the exact property a new implementer must not break.
//!
//! # Security assumptions validated here
//!
//! * `initialize` can only run once; a second call must revert.
//! * `Admin` implies every role; `Employer` implies `Employer`/`Employee`;
//!   `Employee` and `Arbiter` imply only themselves.
//! * `has_role` and `require_role` stay in lock-step: `require_role` must panic
//!   exactly when `has_role` is `false`.
//! * Grant/revoke mutations require an authenticated caller **and** an
//!   `Admin`-implying role; both failure modes must revert without a
//!   partial write.
//! * `bulk_grant` is all-or-nothing and never fabricates or duplicates roles.
//! * The owner's `Admin` role cannot be revoked and `revoke_all` cannot target
//!   the owner (lockout protection).
//! * `get_roles` returns exactly the granted set — no fabricated roles, no
//!   omissions, no duplicates.
//! * Ownership transfer is two-step: only the current owner may propose, only
//!   the pending owner may accept, and the pending-owner slot is cleared on
//!   acceptance (a stale proposal cannot be reused).
//! * `transfer_ownership` authorization is `caller == owner` — holding `Admin`
//!   alone is **not** sufficient.

#![allow(deprecated)] // env.register_contract() — codebase-wide pattern

use rbac_interface::{RbacContractClient, RbacContractInterface, Role};
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Vec};

// ---------------------------------------------------------------------------
// Storage keys used by the mock (mirrors the reference `rbac` contract)
// ---------------------------------------------------------------------------

/// Storage keys for the mock contract.
#[contracttype]
enum MockStorageKey {
    /// One-time initialization flag.
    Initialized,
    /// Contract owner / bootstrap admin.
    Owner,
    /// Pending owner for two-step ownership transfer.
    PendingOwner,
    /// Roles assigned to an address: Address -> Vec<Role>.
    Roles(Address),
}

// ---------------------------------------------------------------------------
// Internal helpers (kept in the mock crate; mirrors `rbac::role_implies`)
// ---------------------------------------------------------------------------

/// @dev Returns true if a granted role implies `required` per the inheritance
///      rules documented on [`RbacContractInterface`].
fn role_implies(granted: &Role, required: &Role) -> bool {
    use Role::*;
    match (granted, required) {
        (Admin, _) => true,
        (Employer, Employer) | (Employer, Employee) => true,
        (Employee, Employee) => true,
        (Arbiter, Arbiter) => true,
        _ => false,
    }
}

/// @dev Reverts if the mock has not been initialized.
fn require_initialized(env: &Env) {
    let initialized: bool = env
        .storage()
        .persistent()
        .get(&MockStorageKey::Initialized)
        .unwrap_or(false);
    assert!(initialized, "Contract not initialized");
}

/// @dev Reads the role vector for `addr`.
fn read_roles(env: &Env, addr: &Address) -> Vec<Role> {
    env.storage()
        .persistent()
        .get::<_, Vec<Role>>(&MockStorageKey::Roles(addr.clone()))
        .unwrap_or_else(|| Vec::new(env))
}

/// @dev Returns true if `addr` holds any role that implies `required`.
fn has_implied_role(env: &Env, addr: &Address, required: &Role) -> bool {
    let roles = read_roles(env, addr);
    for i in 0..roles.len() {
        if role_implies(&roles.get(i).unwrap(), required) {
            return true;
        }
    }
    false
}

/// @dev Reads the current contract owner.
fn read_owner(env: &Env) -> Address {
    env.storage()
        .persistent()
        .get::<_, Address>(&MockStorageKey::Owner)
        .expect("Owner not set")
}

// ---------------------------------------------------------------------------
// Mock contract implementation
// ---------------------------------------------------------------------------

/// A minimal Soroban contract that implements `RbacContractInterface`.
///
/// It faithfully mirrors the security-relevant behavior of the reference `rbac`
/// contract: one-time initialization, inheritance-aware role checks, owner
/// lockout protection, and two-step ownership transfer. Any deviation from
/// these semantics is a conformance failure for an implementer.
#[contract]
pub struct MockRbacContract;

#[contractimpl]
impl RbacContractInterface for MockRbacContract {
    fn initialize(env: Env, owner: Address) {
        owner.require_auth();

        let initialized: bool = env
            .storage()
            .persistent()
            .get(&MockStorageKey::Initialized)
            .unwrap_or(false);
        assert!(!initialized, "Already initialized");

        env.storage().persistent().set(&MockStorageKey::Owner, &owner);
        env.storage().persistent().set(&MockStorageKey::Initialized, &true);

        let mut roles = Vec::new(&env);
        roles.push_back(Role::Admin);
        env.storage()
            .persistent()
            .set(&MockStorageKey::Roles(owner.clone()), &roles);
    }

    fn has_role(env: Env, addr: Address, required: Role) -> bool {
        require_initialized(&env);
        has_implied_role(&env, &addr, &required)
    }

    fn get_roles(env: Env, addr: Address) -> Vec<Role> {
        require_initialized(&env);
        read_roles(&env, &addr)
    }

    fn owner(env: Env) -> Address {
        require_initialized(&env);
        read_owner(&env)
    }

    fn grant_role(env: Env, caller: Address, target: Address, role: Role) {
        require_initialized(&env);
        caller.require_auth();

        assert!(
            has_implied_role(&env, &caller, &Role::Admin),
            "Only admin can grant roles"
        );

        let mut roles = read_roles(&env, &target);
        let mut found = false;
        for i in 0..roles.len() {
            if roles.get(i).unwrap() == role {
                found = true;
                break;
            }
        }
        if !found {
            roles.push_back(role.clone());
            env.storage()
                .persistent()
                .set(&MockStorageKey::Roles(target.clone()), &roles);
        }
    }

    fn revoke_role(env: Env, caller: Address, target: Address, role: Role) {
        require_initialized(&env);
        caller.require_auth();

        assert!(
            has_implied_role(&env, &caller, &Role::Admin),
            "Only admin can revoke roles"
        );

        let owner = read_owner(&env);
        assert!(
            !(target == owner && role == Role::Admin),
            "Cannot revoke Admin from owner"
        );

        let mut roles = read_roles(&env, &target);
        for i in 0..roles.len() {
            if roles.get(i).unwrap() == role {
                roles.remove(i);
                break;
            }
        }
        env.storage()
            .persistent()
            .set(&MockStorageKey::Roles(target.clone()), &roles);
    }

    fn bulk_grant(env: Env, caller: Address, target: Address, roles_to_grant: Vec<Role>) {
        require_initialized(&env);
        caller.require_auth();

        assert!(
            has_implied_role(&env, &caller, &Role::Admin),
            "Only admin can grant roles"
        );

        let mut current = read_roles(&env, &target);
        for i in 0..roles_to_grant.len() {
            let role = roles_to_grant.get(i).unwrap();
            let mut found = false;
            for j in 0..current.len() {
                if current.get(j).unwrap() == role {
                    found = true;
                    break;
                }
            }
            if !found {
                current.push_back(role);
            }
        }
        env.storage()
            .persistent()
            .set(&MockStorageKey::Roles(target.clone()), &current);
    }

    fn revoke_all(env: Env, caller: Address, target: Address) {
        require_initialized(&env);
        caller.require_auth();

        assert!(
            has_implied_role(&env, &caller, &Role::Admin),
            "Only admin can revoke roles"
        );

        let owner = read_owner(&env);
        assert!(target != owner, "Cannot revoke all roles from owner");

        env.storage()
            .persistent()
            .set(&MockStorageKey::Roles(target.clone()), &Vec::new(&env));
    }

    fn require_role(env: Env, addr: Address, required: Role) {
        require_initialized(&env);
        addr.require_auth();
        assert!(
            has_implied_role(&env, &addr, &required),
            "Missing required role"
        );
    }

    fn transfer_ownership(env: Env, caller: Address, new_owner: Address) {
        require_initialized(&env);
        caller.require_auth();

        let owner = read_owner(&env);
        assert!(caller == owner, "Only owner can transfer ownership");

        env.storage()
            .persistent()
            .set(&MockStorageKey::PendingOwner, &new_owner);
    }

    fn accept_ownership(env: Env, caller: Address) {
        require_initialized(&env);
        caller.require_auth();

        let pending: Address = env
            .storage()
            .persistent()
            .get(&MockStorageKey::PendingOwner)
            .expect("No pending owner");
        assert!(caller == pending, "Caller is not pending owner");

        let old_owner = read_owner(&env);

        let mut new_roles = read_roles(&env, &caller);
        let mut has_admin = false;
        for i in 0..new_roles.len() {
            if new_roles.get(i).unwrap() == Role::Admin {
                has_admin = true;
                break;
            }
        }
        if !has_admin {
            new_roles.push_back(Role::Admin);
            env.storage()
                .persistent()
                .set(&MockStorageKey::Roles(caller.clone()), &new_roles);
        }

        let mut old_roles = read_roles(&env, &old_owner);
        for i in 0..old_roles.len() {
            if old_roles.get(i).unwrap() == Role::Admin {
                old_roles.remove(i);
                break;
            }
        }
        env.storage()
            .persistent()
            .set(&MockStorageKey::Roles(old_owner.clone()), &old_roles);

        env.storage().persistent().set(&MockStorageKey::Owner, &caller);
        env.storage()
            .persistent()
            .remove(&MockStorageKey::PendingOwner);
    }
}

// ---------------------------------------------------------------------------
// Conformance tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    /// Stand up a fresh [`MockRbacContract`] and return
    /// `(contract_id, RbacContractClient, owner)` ready for testing.
    ///
    /// `mock_all_auths` is enabled so every `require_auth` passes by default;
    /// tests that exercise authentication narrow the set with `mock_auths`.
    fn setup(env: &Env) -> (Address, RbacContractClient<'_>, Address) {
        let contract_id = env.register_contract(None, MockRbacContract);
        let client = RbacContractClient::new(env, &contract_id);
        let owner = Address::generate(env);
        client.initialize(&owner);
        (contract_id, client, owner)
    }

    /// Stand up a fresh mock *without* calling `initialize` (for the
    /// uninitialized-guard tests).
    fn setup_uninitialized(env: &Env) -> (Address, RbacContractClient<'_>) {
        let contract_id = env.register_contract(None, MockRbacContract);
        let client = RbacContractClient::new(env, &contract_id);
        (contract_id, client)
    }

    // ── Initialization ──────────────────────────────────────────────────────

    /// `initialize` assigns the `Admin` role and records the owner.
    #[test]
    fn test_initialize_sets_admin_and_owner() {
        let env = Env::default();
        env.mock_all_auths();
        let (_cid, client, owner) = setup(&env);
        assert_eq!(client.owner(), owner);
        assert!(client.has_role(&owner, &Role::Admin));
        assert_eq!(client.get_roles(&owner).len(), 1);
    }

    /// `initialize` must only be able to run once.
    #[test]
    fn test_initialize_twice_reverts() {
        let env = Env::default();
        env.mock_all_auths();
        let (_cid, client, owner) = setup(&env);
        assert!(
            client.try_initialize(&owner).is_err(),
            "re-initializing a contract must revert"
        );
    }

    /// All queries must revert before `initialize` has been called.
    #[test]
    fn test_uninitialized_queries_revert() {
        let env = Env::default();
        env.mock_all_auths();
        let (_cid, client) = setup_uninitialized(&env);
        let owner = Address::generate(&env);
        assert!(client.try_has_role(&owner, &Role::Admin).is_err());
        assert!(client.try_owner().is_err());
        assert!(client.try_get_roles(&owner).is_err());
        assert!(client.try_grant_role(&owner, &owner, &Role::Employee).is_err());
        assert!(client.try_require_role(&owner, &Role::Admin).is_err());
    }

    // ── Role inheritance ────────────────────────────────────────────────────

    /// `Admin` implies every role; `Employer` implies `Employer`/`Employee`;
    /// `Employee`/`Arbiter` imply only themselves.
    #[test]
    fn test_role_inheritance_rules() {
        let env = Env::default();
        env.mock_all_auths();
        let (_cid, client, owner) = setup(&env);

        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        client.grant_role(&owner, &employer, &Role::Employer);
        client.grant_role(&owner, &employee, &Role::Employee);

        assert!(client.has_role(&owner, &Role::Arbiter)); // Admin implies all
        assert!(client.has_role(&owner, &Role::Employee));
        assert!(client.has_role(&employer, &Role::Employer));
        assert!(client.has_role(&employer, &Role::Employee)); // Employer -> Employee
        assert!(!client.has_role(&employer, &Role::Arbiter));
        assert!(client.has_role(&employee, &Role::Employee));
        assert!(!client.has_role(&employee, &Role::Employer));
        assert!(!client.has_role(&employee, &Role::Arbiter));
    }

    // ── get_roles truthfulness ──────────────────────────────────────────────

    /// `get_roles` returns exactly the granted set — duplicates in a bulk
    /// grant are skipped and no role is fabricated.
    #[test]
    fn test_get_roles_truthful_no_duplicates() {
        let env = Env::default();
        env.mock_all_auths();
        let (_cid, client, owner) = setup(&env);

        let target = Address::generate(&env);
        let mut roles = Vec::new(&env);
        roles.push_back(Role::Employee);
        roles.push_back(Role::Employee);
        roles.push_back(Role::Arbiter);
        client.bulk_grant(&owner, &target, &roles);

        let got = client.get_roles(&target);
        assert_eq!(got.len(), 2, "duplicates within a bulk grant must be skipped");
        assert_eq!(got.get(0).unwrap(), Role::Employee);
        assert_eq!(got.get(1).unwrap(), Role::Arbiter);

        // A role that was never granted must not appear.
        assert!(!client.has_role(&target, &Role::Employer));
    }

    // ── grant_role / bulk_grant authorization ───────────────────────────────

    /// A non-admin caller must not be able to grant roles (logical check,
    /// independent of host-level authentication).
    #[test]
    fn test_non_admin_cannot_grant() {
        let env = Env::default();
        env.mock_all_auths();
        let (_cid, client, owner) = setup(&env);

        let stranger = Address::generate(&env);
        let target = Address::generate(&env);
        assert!(
            client.try_grant_role(&stranger, &target, &Role::Employee).is_err(),
            "a non-admin caller must not be able to grant roles"
        );
        assert!(!client.has_role(&target, &Role::Employee));

        let mut roles = Vec::new(&env);
        roles.push_back(Role::Employee);
        assert!(
            client.try_bulk_grant(&stranger, &target, &roles).is_err(),
            "a non-admin caller must not be able to bulk-grant roles"
        );
        assert!(!client.has_role(&target, &Role::Employee));
    }

    /// Even an admin must authenticate; a caller that is not pre-authorized
    /// must be rejected at the host-auth layer.
    #[test]
    fn test_grant_requires_caller_auth() {
        let env = Env::default();
        env.mock_all_auths();
        let (_cid, client, owner) = setup(&env);

        let target = Address::generate(&env);
        // Only `owner` is pre-authorized; a stranger call fails `require_auth`.
        env.mock_auths(&[&owner]);
        assert!(
            client.try_grant_role(&target, &target, &Role::Employee).is_err(),
            "grant_role must revert when the caller is not authenticated"
        );
        assert!(!client.has_role(&target, &Role::Employee));
    }

    // ── revoke_role / revoke_all ────────────────────────────────────────────

    /// The owner's `Admin` role cannot be revoked (lockout protection), and a
    /// failed attempt leaves state intact.
    #[test]
    fn test_owner_admin_protected_from_revoke() {
        let env = Env::default();
        env.mock_all_auths();
        let (_cid, client, owner) = setup(&env);

        assert!(
            client.try_revoke_role(&owner, &owner, &Role::Admin).is_err(),
            "revoking Admin from the owner must revert"
        );
        assert!(
            client.has_role(&owner, &Role::Admin),
            "owner Admin must survive a rejected revoke (no partial write)"
        );
        assert!(
            client.try_revoke_all(&owner, &owner).is_err(),
            "revoke_all on the owner must revert"
        );
        assert_eq!(client.get_roles(&owner).len(), 1);
    }

    /// A successful revoke removes exactly the target role.
    #[test]
    fn test_revoke_role_removes_role() {
        let env = Env::default();
        env.mock_all_auths();
        let (_cid, client, owner) = setup(&env);

        let target = Address::generate(&env);
        client.grant_role(&owner, &target, &Role::Employee);
        client.grant_role(&owner, &target, &Role::Arbiter);
        assert!(client.has_role(&target, &Role::Arbiter));

        client.revoke_role(&owner, &target, &Role::Arbiter);
        assert!(!client.has_role(&target, &Role::Arbiter));
        assert!(client.has_role(&target, &Role::Employee), "other roles must survive");
    }

    /// A non-admin caller must not be able to revoke roles.
    #[test]
    fn test_non_admin_cannot_revoke() {
        let env = Env::default();
        env.mock_all_auths();
        let (_cid, client, owner) = setup(&env);

        let stranger = Address::generate(&env);
        assert!(
            client.try_revoke_role(&stranger, &owner, &Role::Admin).is_err(),
            "a non-admin caller must not be able to revoke roles"
        );
        assert!(client.has_role(&owner, &Role::Admin));
    }

    // ── require_role / has_role lock-step ───────────────────────────────────

    /// `require_role` must panic exactly when `has_role` returns `false`.
    #[test]
    fn test_require_role_lockstep_with_has_role() {
        let env = Env::default();
        env.mock_all_auths();
        let (_cid, client, owner) = setup(&env);

        let employee = Address::generate(&env);
        client.grant_role(&owner, &employee, &Role::Employee);

        // has_role == true  => require_role succeeds.
        assert!(client.has_role(&employee, &Role::Employee));
        client.require_role(&employee, &Role::Employee);

        // has_role == false => require_role panics.
        assert!(!client.has_role(&employee, &Role::Arbiter));
        assert!(
            client.try_require_role(&employee, &Role::Arbiter).is_err(),
            "require_role must revert when has_role is false"
        );

        // Admin satisfies every requirement.
        client.require_role(&owner, &Role::Arbiter);
    }

    /// `require_role` requires the checked address to authenticate.
    #[test]
    fn test_require_role_requires_auth() {
        let env = Env::default();
        env.mock_all_auths();
        let (_cid, client, owner) = setup(&env);

        let employee = Address::generate(&env);
        client.grant_role(&owner, &employee, &Role::Employee);

        env.mock_auths(&[&employee]);
        assert!(
            client.try_require_role(&owner, &Role::Admin).is_err(),
            "require_role must revert when the checked address is not authenticated"
        );
    }

    // ── Ownership transfer (two-step) ───────────────────────────────────────

    /// Full two-step transfer: only the owner proposes, only the pending owner
    /// accepts, Admin roles migrate, and ownership is recorded.
    #[test]
    fn test_transfer_ownership_two_step() {
        let env = Env::default();
        env.mock_all_auths();
        let (_cid, client, owner) = setup(&env);

        let new_owner = Address::generate(&env);
        let stranger = Address::generate(&env);

        // Step 1: only the current owner may propose.
        assert!(
            client.try_transfer_ownership(&stranger, &new_owner).is_err(),
            "non-owner must not be able to propose an ownership transfer"
        );
        client.transfer_ownership(&owner, &new_owner);

        // A stranger cannot accept.
        assert!(
            client.try_accept_ownership(&stranger).is_err(),
            "only the pending owner may accept"
        );

        // Step 2: the pending owner accepts.
        client.accept_ownership(&new_owner);
        assert_eq!(client.owner(), new_owner);
        assert!(client.has_role(&new_owner, &Role::Admin));
        assert!(
            !client.has_role(&owner, &Role::Admin),
            "old owner must lose Admin after accepting a transfer"
        );
    }

    /// Holding `Admin` is NOT sufficient to transfer ownership — only the
    /// recorded owner may propose.
    #[test]
    fn test_transfer_ownership_admin_is_not_sufficient() {
        let env = Env::default();
        env.mock_all_auths();
        let (_cid, client, owner) = setup(&env);

        let admin2 = Address::generate(&env);
        client.grant_role(&owner, &admin2, &Role::Admin);
        assert!(client.has_role(&admin2, &Role::Admin));

        let new_owner = Address::generate(&env);
        assert!(
            client.try_transfer_ownership(&admin2, &new_owner).is_err(),
            "an Admin who is not the owner must not be able to propose a transfer"
        );
        assert_eq!(client.owner(), owner, "ownership must be unchanged");
    }

    /// The pending-owner slot must be cleared on acceptance — a stale proposal
    /// cannot be reused to accept a later, unrelated transfer.
    #[test]
    fn test_accept_ownership_pending_cleared() {
        let env = Env::default();
        env.mock_all_auths();
        let (_cid, client, owner) = setup(&env);

        let new_owner = Address::generate(&env);
        client.transfer_ownership(&owner, &new_owner);
        client.accept_ownership(&new_owner);

        assert!(
            client.try_accept_ownership(&new_owner).is_err(),
            "accepting ownership twice must revert (pending slot cleared)"
        );
    }
}
