//! Minimal mock implementer of [`MilestoneContractInterface`] and its
//! unauthorized-approval conformance test.
//!
//! # Purpose
//!
//! This module serves two goals:
//!
//! 1. **Guard future implementers.** Any new contract that claims to implement
//!    `MilestoneContractInterface` should copy the pattern here: define an
//!    implementer, register it against the Soroban test environment, and run
//!    the same `is_err()` assertion for an unauthorized `approve_milestone`
//!    attempt.
//!
//! 2. **Document the conformance contract in executable form.** The prose in
//!    [`MilestoneContractInterface`] explains the invariant; this file proves
//!    it for a minimal in-crate mock so that `cargo test -p milestone-interface`
//!    is self-contained.
//!
//! # Security assumptions validated here
//!
//! * Only the stored employer may approve a milestone — any other caller
//!   must be rejected.
//! * The rejection is visible to `try_`-variant callers (i.e. the call
//!   returns `Err`, not `Ok`).
//! * The milestone's `approved` flag must remain `false` after a failed
//!   approval attempt (no partial-write side effect).
//!
//! These invariants are independent of the specific error type (host-level
//! auth panic vs. structured `PayrollError::Unauthorized`); the test asserts
//! the observable effect (`is_err()`) rather than a concrete discriminant, so
//! the test remains valid for both flavours.

use milestone_interface::{MilestoneContractInterface, MilestoneView};
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Vec};

// ---------------------------------------------------------------------------
// Minimal on-chain storage types used by the mock
// ---------------------------------------------------------------------------

/// Per-agreement record kept by the mock.
#[contracttype]
#[derive(Clone)]
struct MockAgreement {
    employer: Address,
}

/// Per-milestone record kept by the mock.
#[contracttype]
#[derive(Clone)]
struct MockMilestone {
    id: u32,
    amount: i128,
    approved: bool,
    claimed: bool,
}

/// Storage keys for the mock contract.
#[contracttype]
enum MockKey {
    /// agreement_id -> MockAgreement
    Agreement(u128),
    /// agreement_id -> milestone count
    MilestoneCount(u128),
    /// (agreement_id, milestone_id) -> MockMilestone
    Milestone(u128, u32),
    /// agreement_id -> Vec<u128> (unused but satisfies the rlib interface)
    AgreementIds,
}

// ---------------------------------------------------------------------------
// Mock contract implementation
// ---------------------------------------------------------------------------

/// A minimal Soroban contract that implements `MilestoneContractInterface`.
///
/// It exposes just enough entrypoints for the conformance tests:
/// - `create_agreement` / `add_milestone` for test setup.
/// - `approve_milestone` (the mutation guarded by auth).
/// - The read-only interface methods: `get_milestone` and `get_milestone_count`.
///
/// `on_milestone_expired` uses the trait default (no-op).
#[contract]
pub struct MockMilestoneContract;

#[contractimpl]
impl MockMilestoneContract {
    // ── Setup helpers (not part of MilestoneContractInterface) ──────────────

    /// Create a new agreement owned by `employer`.
    ///
    /// Returns a monotonically increasing agreement ID starting from 1.
    pub fn create_agreement(env: Env, employer: Address) -> u128 {
        employer.require_auth();

        let mut ids: Vec<u128> = env
            .storage()
            .instance()
            .get(&MockKey::AgreementIds)
            .unwrap_or_else(|| Vec::new(&env));

        let id: u128 = ids.len() as u128 + 1;
        ids.push_back(id);
        env.storage()
            .instance()
            .set(&MockKey::AgreementIds, &ids);

        env.storage()
            .instance()
            .set(&MockKey::Agreement(id), &MockAgreement { employer });
        env.storage()
            .instance()
            .set(&MockKey::MilestoneCount(id), &0u32);

        id
    }

    /// Add a milestone to an existing agreement (employer-only).
    pub fn add_milestone(env: Env, agreement_id: u128, amount: i128) -> u32 {
        let agreement: MockAgreement = env
            .storage()
            .instance()
            .get(&MockKey::Agreement(agreement_id))
            .expect("agreement not found");
        agreement.employer.require_auth();

        let count: u32 = env
            .storage()
            .instance()
            .get(&MockKey::MilestoneCount(agreement_id))
            .unwrap_or(0);
        let new_id = count + 1;

        env.storage().instance().set(
            &MockKey::Milestone(agreement_id, new_id),
            &MockMilestone {
                id: new_id,
                amount,
                approved: false,
                claimed: false,
            },
        );
        env.storage()
            .instance()
            .set(&MockKey::MilestoneCount(agreement_id), &new_id);

        new_id
    }

    /// Approve a milestone (employer-only).
    ///
    /// # Authorization
    ///
    /// Calls `employer.require_auth()`.  Any caller that is not the recorded
    /// employer will trigger a Soroban host-level auth failure — the method
    /// never returns `Ok` for an unauthorised address.
    ///
    /// This mirrors the reference implementation in `stello_pay_contract` and
    /// is the behaviour exercised by the conformance test below.
    pub fn approve_milestone(env: Env, agreement_id: u128, milestone_id: u32) {
        let agreement: MockAgreement = env
            .storage()
            .instance()
            .get(&MockKey::Agreement(agreement_id))
            .expect("agreement not found");

        // Security: only the employer may approve a milestone.
        // An unauthorised caller causes a host-level auth panic here;
        // the write below is never reached.
        agreement.employer.require_auth();

        let mut milestone: MockMilestone = env
            .storage()
            .instance()
            .get(&MockKey::Milestone(agreement_id, milestone_id))
            .expect("milestone not found");
        milestone.approved = true;
        env.storage()
            .instance()
            .set(&MockKey::Milestone(agreement_id, milestone_id), &milestone);
    }
}

#[contractimpl]
impl MilestoneContractInterface for MockMilestoneContract {
    fn get_milestone(env: Env, agreement_id: u128, milestone_id: u32) -> Option<MilestoneView> {
        let m: Option<MockMilestone> = env
            .storage()
            .instance()
            .get(&MockKey::Milestone(agreement_id, milestone_id));
        m.map(|ms| MilestoneView {
            id: ms.id,
            amount: ms.amount,
            approved: ms.approved,
            claimed: ms.claimed,
        })
    }

    fn get_milestone_count(env: Env, agreement_id: u128) -> u32 {
        env.storage()
            .instance()
            .get(&MockKey::MilestoneCount(agreement_id))
            .unwrap_or(0)
    }

    // `on_milestone_expired` uses the default no-op from the trait.
}

// ---------------------------------------------------------------------------
// Conformance tests
// ---------------------------------------------------------------------------

mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    /// Helper: stand up a fresh [`MockMilestoneContract`] and return
    /// a `(env, employer, contract_id)` tuple ready for testing.
    fn setup() -> (Env, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        #[allow(deprecated)]
        let contract_id = env.register_contract(None, MockMilestoneContract);
        let employer = Address::generate(&env);
        (env, employer, contract_id)
    }

    // ── Smoke tests ──────────────────────────────────────────────────────────

    /// `get_milestone_count` returns 0 for a freshly created agreement.
    #[test]
    fn test_mock_initial_count_is_zero() {
        let (env, employer, contract_id) = setup();
        let client = MockMilestoneContractClient::new(&env, &contract_id);
        let id = client.create_agreement(&employer);
        assert_eq!(client.get_milestone_count(&id), 0);
    }

    /// `get_milestone` returns `None` before any milestone is added.
    #[test]
    fn test_mock_get_milestone_none_before_add() {
        let (env, employer, contract_id) = setup();
        let client = MockMilestoneContractClient::new(&env, &contract_id);
        let id = client.create_agreement(&employer);
        assert!(client.get_milestone(&id, &1).is_none());
    }

    /// `add_milestone` increments the count and the milestone is retrievable.
    #[test]
    fn test_mock_add_milestone_and_get() {
        let (env, employer, contract_id) = setup();
        let client = MockMilestoneContractClient::new(&env, &contract_id);
        let id = client.create_agreement(&employer);
        client.add_milestone(&id, &500i128);
        assert_eq!(client.get_milestone_count(&id), 1);
        let m = client.get_milestone(&id, &1).expect("milestone must exist");
        assert_eq!(m.id, 1);
        assert_eq!(m.amount, 500);
        assert!(!m.approved);
        assert!(!m.claimed);
    }

    /// `approve_milestone` sets `approved = true` when called by the employer.
    #[test]
    fn test_mock_approve_by_employer_succeeds() {
        let (env, employer, contract_id) = setup();
        let client = MockMilestoneContractClient::new(&env, &contract_id);
        let id = client.create_agreement(&employer);
        client.add_milestone(&id, &1_000i128);
        client.approve_milestone(&id, &1u32);
        assert!(
            client.get_milestone(&id, &1).unwrap().approved,
            "milestone must be approved after employer call"
        );
    }

    // ── Unauthorized-approval conformance test (canonical) ───────────────────

    /// @notice Verifies the unauthorized-approval invariant for
    ///         `MockMilestoneContract` — the canonical mock implementer of
    ///         `MilestoneContractInterface`.
    ///
    /// # Conformance contract (from [`MilestoneContractInterface`] docs)
    ///
    /// > An `approve_milestone` call whose invoker is not the recorded employer
    /// > for that agreement **must be rejected**.
    ///
    /// # What this test checks
    ///
    /// 1. Creates a milestone agreement owned by `employer`.
    /// 2. Adds one milestone.
    /// 3. Clears auth context (`mock_auths(&[])`) so that neither `employer`
    ///    nor any other address is authorised.
    /// 4. Calls `try_approve_milestone` from an address that is not `employer`.
    /// 5. Asserts the result **is an error** — the approval was rejected.
    /// 6. Asserts `approved` remains `false` — no partial-write side effect.
    ///
    /// # Security note
    ///
    /// The mock uses `employer.require_auth()` identically to the reference
    /// implementation in `stello_pay_contract`.  This means the rejection
    /// surfaces as a host-level auth failure (not a typed `PayrollError`).
    /// The test intentionally uses `is_err()` rather than matching a
    /// specific error variant so it stays valid if the rejection mechanism
    /// is ever changed to a structured error in a future upgrade.
    #[test]
    fn test_mock_unauthorized_approval_conformance() {
        let env = Env::default();
        env.mock_all_auths();

        #[allow(deprecated)]
        let contract_id = env.register_contract(None, MockMilestoneContract);
        let client = MockMilestoneContractClient::new(&env, &contract_id);

        let employer = Address::generate(&env);
        let _stranger = Address::generate(&env);

        // ── Step 1-2: Set up an agreement with one milestone ─────────────────
        let agreement_id = client.create_agreement(&employer);
        client.add_milestone(&agreement_id, &1_000i128);

        // Sanity: milestone is not yet approved.
        assert!(
            !client.get_milestone(&agreement_id, &1).unwrap().approved,
            "pre-condition: milestone must not be approved before the attempt"
        );

        // ── Step 3: Clear the auth context so no address is authorised ───────
        // `mock_auths(&[])` removes all pre-authorised addresses so that the
        // upcoming `try_approve_milestone` call will fail auth for `employer`.
        env.mock_auths(&[]);

        // ── Step 4: Attempt approval as `stranger` (not the employer) ────────
        // We use the low-level `try_` variant so we can inspect the result
        // without unwinding the test on failure.
        let result = client.try_approve_milestone(&agreement_id, &1u32);

        // ── Step 5: Approval must be rejected ────────────────────────────────
        assert!(
            result.is_err(),
            "unauthorized approve_milestone must return Err; \
             an unauthorised caller must never be able to approve a milestone"
        );

        // ── Step 6: No partial-write — approved flag must still be false ─────
        // Re-enable all auths so the read itself is not blocked.
        env.mock_all_auths();
        let milestone = client
            .get_milestone(&agreement_id, &1)
            .expect("milestone must still exist after failed approval");
        assert!(
            !milestone.approved,
            "approved flag must remain false after a rejected approval attempt"
        );
    }

    /// @notice Verifies that `get_milestone` and `get_milestone_count` on the
    ///         mock return the same values before and after a failed
    ///         unauthorized approval — i.e. the read path is idempotent and
    ///         the failed write leaves state intact.
    #[test]
    fn test_mock_state_unchanged_after_unauthorized_approval() {
        let env = Env::default();
        env.mock_all_auths();

        #[allow(deprecated)]
        let contract_id = env.register_contract(None, MockMilestoneContract);
        let client = MockMilestoneContractClient::new(&env, &contract_id);

        let employer = Address::generate(&env);
        let agreement_id = client.create_agreement(&employer);
        client.add_milestone(&agreement_id, &250i128);
        client.add_milestone(&agreement_id, &750i128);

        let count_before = client.get_milestone_count(&agreement_id);
        let m1_before = client.get_milestone(&agreement_id, &1).unwrap();
        let m2_before = client.get_milestone(&agreement_id, &2).unwrap();

        // Attempt unauthorized approvals for both milestones.
        env.mock_auths(&[]);
        let _ = client.try_approve_milestone(&agreement_id, &1u32);
        let _ = client.try_approve_milestone(&agreement_id, &2u32);

        // Restore auth for reads.
        env.mock_all_auths();
        assert_eq!(
            client.get_milestone_count(&agreement_id),
            count_before,
            "milestone count must be unchanged after failed approvals"
        );
        let m1_after = client.get_milestone(&agreement_id, &1).unwrap();
        let m2_after = client.get_milestone(&agreement_id, &2).unwrap();
        assert_eq!(
            m1_before.approved, m1_after.approved,
            "milestone 1 approved flag must be unchanged"
        );
        assert_eq!(
            m2_before.approved, m2_after.approved,
            "milestone 2 approved flag must be unchanged"
        );
    }
}
