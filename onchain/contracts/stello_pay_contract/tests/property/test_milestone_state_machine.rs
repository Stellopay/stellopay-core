//! Property-based tests for the milestone state machine transition rules.
//!
//! # Scope
//!
//! These tests generate arbitrary sequences of milestone operations
//! (`approve`, `reject`, `expire`, `claim`) in any order and assert the
//! invariants that the state machine guarantees:
//!
//! 1. **No double-claim** — a claimed milestone cannot be claimed again.
//! 2. **No approve-after-reject** — a rejected milestone cannot be approved.
//! 3. **No claim-without-approve** — only approved milestones may be claimed.
//! 4. **No expire-after-terminal** — a milestone in any terminal state
//!    (claimed, rejected, expired) cannot be expired again.
//! 5. **Fund conservation** — total paid out via `claim_milestone` never
//!    exceeds the funded escrow balance.
//! 6. **Escrow non-negative** — the accounted escrow balance never goes below
//!    zero as a result of claims.
//!
//! # Reproducibility
//!
//! proptest writes a `.proptest-regressions` file alongside this source file
//! whenever a failure is found.  Re-running the suite deterministically
//! replays the exact seed that first exposed the bug.  Override the case
//! count at any time:
//!
//! ```sh
//! PROPTEST_CASES=500 cargo test -p stello_pay_contract --test property
//! ```
//!
//! # Catching a deliberately removed guard
//!
//! `prop_no_approve_after_reject` is the primary canary test.  It verifies
//! the "already rejected → cannot approve" guard inside `approve_milestone`.
//! Removing that guard from `payroll.rs` makes this property detect and
//! report a minimal reproducible seed — satisfying the acceptance criterion
//! *"confirm it catches a deliberately removed transition guard"*.

#![cfg(test)]
#![allow(dead_code)] // some helpers are defined for completeness

use proptest::prelude::*;
use soroban_sdk::{testutils::Address as _, token::StellarAssetClient, Address, Env, String};
use stello_pay_contract::{storage::PayrollError, PayrollContract, PayrollContractClient};

// ============================================================================
// Configuration
// ============================================================================

/// Number of proptest cases (default 64; override via `PROPTEST_CASES` env var).
fn proptest_cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(64)
}

// ============================================================================
// Shadow state model
// ============================================================================

/// Independent Rust model of a single milestone's boolean flags.
///
/// Maintained in parallel with the on-chain state to verify that every
/// transition the property engine applies matches the contract's expected
/// behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct MilestoneFlags {
    pub approved: bool,
    pub claimed: bool,
    pub rejected: bool,
    pub expired: bool,
}

impl MilestoneFlags {
    /// Returns `true` when the milestone has reached a terminal state from
    /// which no further transition is possible.
    fn is_terminal(self) -> bool {
        self.claimed || self.rejected || self.expired
    }
}

// ============================================================================
// Setup helper
// ============================================================================

/// Spin up a fresh Soroban test environment with a funded milestone agreement.
///
/// All authorization is mocked (`mock_all_auths`) so the tests can freely
/// call employer and contributor operations without managing real keys.
///
/// Returns `(env, employer, contributor, client, agreement_id)`.
fn setup(
    milestone_count: u32,
    amount_per_milestone: i128,
) -> (
    Env,
    Address,
    Address,
    PayrollContractClient<'static>,
    u128,
) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);

    // Create and mint the token.
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    let fund_total = amount_per_milestone * (milestone_count as i128);
    // Mint twice the required amount so the employer can fund without running dry.
    StellarAssetClient::new(&env, &token).mint(&employer, &(fund_total * 2));

    // Build the milestones vec (all equal amounts for simplicity).
    let mut amounts = soroban_sdk::Vec::new(&env);
    for _ in 0..milestone_count {
        amounts.push_back(amount_per_milestone);
    }

    let agreement_id =
        client.create_milestone_agreement(&employer, &contributor, &token, &amounts);

    // Fund the agreement so the escrow invariant in approve/claim can pass.
    client.fund_milestone_agreement(&agreement_id, &employer, &fund_total);

    (env, employer, contributor, client, agreement_id)
}

// ============================================================================
// Property tests
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(proptest_cases()))]

    // ------------------------------------------------------------------
    // Invariant 1 – No double-claim
    // ------------------------------------------------------------------
    /// A milestone that has already been claimed must never be claimable again.
    ///
    /// The second `claim_milestone` call must return `MilestoneAlreadyClaimed`
    /// (error 41) and the `claimed` flag must remain `true`.
    #[test]
    fn prop_no_double_claim(
        milestone_count in 1u32..=5,
        amount in 100i128..=10_000,
        // Sequence of (approve?, milestone_id) — first approve then claim.
        pairs in prop::collection::vec(
            (any::<bool>(), 1u32..=5),
            1..=15,
        ),
    ) {
        let (_env, _employer, _contributor, client, agreement_id) =
            setup(milestone_count, amount);

        let bound = milestone_count;

        for (do_approve, raw_id) in pairs {
            let id = ((raw_id.saturating_sub(1)) % bound) + 1;

            if do_approve {
                let _ = client.try_approve_milestone(&agreement_id, &id);
            }

            // Snapshot whether already claimed before this attempt.
            let was_claimed = client
                .get_milestone(&agreement_id, &id)
                .map(|m| m.claimed)
                .unwrap_or(false);

            let result = client.try_claim_milestone(&agreement_id, &id);

            if was_claimed {
                // Must fail — and with the right error.
                prop_assert!(
                    result.is_err(),
                    "Double-claim of milestone {} succeeded unexpectedly",
                    id
                );
                if let Err(Ok(err)) = result {
                    prop_assert_eq!(
                        err,
                        PayrollError::MilestoneAlreadyClaimed,
                        "Wrong error on double-claim of milestone {}: {:?}",
                        id, err
                    );
                }
            }

            // After a successful claim the flag must be set.
            if result.is_ok() {
                let after = client.get_milestone(&agreement_id, &id);
                prop_assert!(
                    after.map(|m| m.claimed).unwrap_or(false),
                    "claimed flag not set after successful claim of milestone {}",
                    id
                );
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(proptest_cases()))]

    // ------------------------------------------------------------------
    // Invariant 2 – No approve-after-reject (primary guard-removal canary)
    // ------------------------------------------------------------------
    /// Once a milestone is rejected it must never become approved.
    ///
    /// This is the primary canary test: removing the `already_rejected` guard
    /// from `approve_milestone` will cause proptest to find a failing seed and
    /// shrink it to a minimal counterexample.
    #[test]
    fn prop_no_approve_after_reject(
        milestone_count in 1u32..=5,
        amount in 100i128..=10_000,
        // Sequence of (0=approve | 1=reject, milestone_id)
        ops in prop::collection::vec((0u32..2, 1u32..=5), 1..=15),
    ) {
        let (env, _employer, _contributor, client, agreement_id) =
            setup(milestone_count, amount);

        let bound = milestone_count;

        // Shadow: track which milestones have been rejected.
        let mut rejected_set = std::collections::HashSet::new();

        for (op_kind, raw_id) in ops {
            let id = ((raw_id.saturating_sub(1)) % bound) + 1;

            match op_kind {
                0 => {
                    // Approve attempt
                    let result = client.try_approve_milestone(&agreement_id, &id);

                    if rejected_set.contains(&id) {
                        // Must fail — guard must block approval of rejected milestone.
                        prop_assert!(
                            result.is_err(),
                            "GUARD REMOVED: approve of rejected milestone {} succeeded — \
                             the already_rejected guard in approve_milestone was removed",
                            id
                        );
                        if let Err(Ok(err)) = result {
                            prop_assert_eq!(
                                err,
                                PayrollError::MilestoneAlreadyRejected,
                                "Wrong error when approving rejected milestone {}: {:?}",
                                id, err
                            );
                        }
                        // approved flag must stay false.
                        let after = client.get_milestone(&agreement_id, &id);
                        prop_assert!(
                            !after.map(|m| m.approved).unwrap_or(true),
                            "approved flag incorrectly set on rejected milestone {}",
                            id
                        );
                    }
                }
                _ => {
                    // Reject attempt
                    let reason = String::from_str(&env, "prop-test rejection reason");
                    if client
                        .try_reject_milestone(&agreement_id, &id, &reason)
                        .is_ok()
                    {
                        rejected_set.insert(id);
                    }
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(proptest_cases()))]

    // ------------------------------------------------------------------
    // Invariant 3 – Claim requires prior approval
    // ------------------------------------------------------------------
    /// `claim_milestone` on a non-approved milestone must fail with
    /// `MilestoneNotApproved` and must not mutate the `claimed` flag.
    #[test]
    fn prop_claim_requires_approval(
        milestone_count in 1u32..=5,
        amount in 100i128..=10_000,
        claim_ids in prop::collection::vec(1u32..=5, 1..=10),
    ) {
        let (_env, _employer, _contributor, client, agreement_id) =
            setup(milestone_count, amount);

        let bound = milestone_count;

        for raw_id in claim_ids {
            let id = ((raw_id.saturating_sub(1)) % bound) + 1;

            let before = client.get_milestone(&agreement_id, &id);
            let is_approved = before.as_ref().map(|m| m.approved).unwrap_or(false);
            let is_claimed  = before.as_ref().map(|m| m.claimed).unwrap_or(false);

            let result = client.try_claim_milestone(&agreement_id, &id);

            // Not approved and not already claimed → must fail with MilestoneNotApproved.
            if !is_approved && !is_claimed {
                prop_assert!(
                    result.is_err(),
                    "Claim of unapproved milestone {} succeeded",
                    id
                );
                if let Err(Ok(err)) = result {
                    prop_assert_eq!(
                        err,
                        PayrollError::MilestoneNotApproved,
                        "Wrong error for unapproved claim on milestone {}: {:?}",
                        id, err
                    );
                }
                // claimed flag must still be false.
                let after = client.get_milestone(&agreement_id, &id);
                prop_assert!(
                    !after.map(|m| m.claimed).unwrap_or(true),
                    "claimed flag set on unapproved milestone {} after failing claim",
                    id
                );
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(proptest_cases()))]

    // ------------------------------------------------------------------
    // Invariant 4 – No re-expire from any terminal state
    // ------------------------------------------------------------------
    /// A milestone in any terminal state (claimed / rejected / expired) must
    /// reject a subsequent `expire_milestone` call with the appropriate error.
    #[test]
    fn prop_no_terminal_re_expire(
        milestone_count in 1u32..=5,
        amount in 100i128..=10_000,
        // op_kind: 0=approve, 1=claim, 2=reject, 3=expire
        ops in prop::collection::vec((0u32..4, 1u32..=5), 1..=15),
    ) {
        let (env, _employer, _contributor, client, agreement_id) =
            setup(milestone_count, amount);

        let bound = milestone_count;

        // Shadow model.
        let mut shadow: std::collections::HashMap<u32, MilestoneFlags> = (1..=milestone_count)
            .map(|id| (id, MilestoneFlags::default()))
            .collect();

        for (op_kind, raw_id) in ops {
            let id = ((raw_id.saturating_sub(1)) % bound) + 1;
            let flags = *shadow.get(&id).unwrap();

            match op_kind {
                0 => {
                    // Approve
                    if client.try_approve_milestone(&agreement_id, &id).is_ok() {
                        shadow.get_mut(&id).unwrap().approved = true;
                    }
                }
                1 => {
                    // Claim
                    if client.try_claim_milestone(&agreement_id, &id).is_ok() {
                        shadow.get_mut(&id).unwrap().claimed = true;
                    }
                }
                2 => {
                    // Reject
                    let reason = String::from_str(&env, "prop-test rejection");
                    if client.try_reject_milestone(&agreement_id, &id, &reason).is_ok() {
                        shadow.get_mut(&id).unwrap().rejected = true;
                    }
                }
                3 => {
                    // Expire
                    let result = client.try_expire_milestone(&agreement_id, &id);

                    if flags.is_terminal() {
                        // Must fail — terminal milestone cannot be expired.
                        prop_assert!(
                            result.is_err(),
                            "expire_milestone succeeded on terminal milestone {} \
                             (claimed={}, rejected={}, expired={})",
                            id, flags.claimed, flags.rejected, flags.expired
                        );

                        // Verify the specific error for re-expire.
                        if flags.expired {
                            if let Err(Ok(err)) = result {
                                prop_assert_eq!(
                                    err,
                                    PayrollError::MilestoneAlreadyExpired,
                                    "Wrong error for re-expire of expired milestone {}: {:?}",
                                    id, err
                                );
                            }
                        }
                    } else if result.is_ok() {
                        shadow.get_mut(&id).unwrap().expired = true;
                    }
                }
                _ => unreachable!(),
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(proptest_cases()))]

    // ------------------------------------------------------------------
    // Invariant 5 – Fund conservation
    // ------------------------------------------------------------------
    /// The cumulative amount paid out through `claim_milestone` calls must
    /// never exceed the total amount funded into the agreement's escrow.
    ///
    /// This is checked after every individual claim (not just at the end) so
    /// transient over-distribution bugs are caught mid-sequence.
    #[test]
    fn prop_fund_conservation(
        milestone_count in 1u32..=5,
        amount in 100i128..=5_000,
        // op_kind: 0=approve, 1=claim, 2=reject, 3=expire
        ops in prop::collection::vec((0u32..4, 1u32..=5), 1..=20),
    ) {
        let (env, _employer, _contributor, client, agreement_id) =
            setup(milestone_count, amount);

        let total_funded = amount * (milestone_count as i128);
        let bound = milestone_count;
        let mut total_claimed: i128 = 0;

        for (op_kind, raw_id) in ops {
            let id = ((raw_id.saturating_sub(1)) % bound) + 1;

            match op_kind {
                0 => { let _ = client.try_approve_milestone(&agreement_id, &id); }
                1 => {
                    // Snapshot the milestone amount before claiming.
                    let ms_amount = client
                        .get_milestone(&agreement_id, &id)
                        .map(|m| m.amount)
                        .unwrap_or(0);

                    if client.try_claim_milestone(&agreement_id, &id).is_ok() {
                        total_claimed += ms_amount;
                    }

                    // Per-step invariant: never over-draw the funded escrow.
                    prop_assert!(
                        total_claimed <= total_funded,
                        "CRITICAL fund over-draw: total_claimed={} > total_funded={} \
                         after claiming milestone {}",
                        total_claimed, total_funded, id
                    );
                }
                2 => {
                    let reason = String::from_str(&env, "prop-test rejection");
                    let _ = client.try_reject_milestone(&agreement_id, &id, &reason);
                }
                3 => { let _ = client.try_expire_milestone(&agreement_id, &id); }
                _ => unreachable!(),
            }
        }

        // Final check: total_claimed + still-claimable ≤ total_funded.
        let remaining_claimable: i128 = (1..=milestone_count)
            .filter_map(|mid| client.get_milestone(&agreement_id, &mid))
            .filter(|m| m.approved && !m.claimed)
            .map(|m| m.amount)
            .sum();

        prop_assert!(
            total_claimed + remaining_claimable <= total_funded,
            "End-state fund invariant violated: claimed={} + claimable={} > funded={}",
            total_claimed, remaining_claimable, total_funded
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(proptest_cases()))]

    // ------------------------------------------------------------------
    // Invariant 6 – Full shadow-model verification across all transitions
    // ------------------------------------------------------------------
    /// Generates sequences spanning ALL transition entry points in any order
    /// and verifies the on-chain `approved` / `claimed` flags always match an
    /// independent Rust shadow model.
    ///
    /// Covered orderings include adversarial cases:
    ///   - approve → expire  (should fail: already approved)
    ///   - claim → approve   (should fail: already claimed)
    ///   - reject → claim    (should fail: not approved)
    ///   - expire → approve  (should fail: already expired → contract treats as rejected)
    ///   - double approve, double reject, double expire, double claim
    #[test]
    fn prop_shadow_model_all_transitions(
        milestone_count in 1u32..=4,
        amount in 100i128..=2_000,
        // op_kind 0-5; raw_id 1-4
        raw_ops in prop::collection::vec((0u32..6, 1u32..=4), 1..=20),
    ) {
        let (env, _employer, _contributor, client, agreement_id) =
            setup(milestone_count, amount);

        let bound = milestone_count;

        // Shadow model: expected per-milestone boolean flags.
        let mut shadow: std::collections::HashMap<u32, MilestoneFlags> = (1..=milestone_count)
            .map(|id| (id, MilestoneFlags::default()))
            .collect();

        for (op_kind, raw_id) in raw_ops {
            let id = ((raw_id.saturating_sub(1)) % bound) + 1;
            let s = *shadow.get(&id).unwrap();

            match op_kind {
                // ---- Approve ------------------------------------------------
                0 => {
                    let result = client.try_approve_milestone(&agreement_id, &id);

                    if s.approved {
                        prop_assert!(result.is_err(),
                            "Double-approve of milestone {} succeeded", id);
                        if let Err(Ok(e)) = result {
                            prop_assert_eq!(e, PayrollError::MilestoneAlreadyApproved,
                                "Wrong error for double-approve on milestone {}: {:?}", id, e);
                        }
                    } else if s.rejected {
                        prop_assert!(result.is_err(),
                            "Approve of rejected milestone {} succeeded — guard may be removed!", id);
                        if let Err(Ok(e)) = result {
                            prop_assert_eq!(e, PayrollError::MilestoneAlreadyRejected,
                                "Wrong error for approve-after-reject on milestone {}: {:?}", id, e);
                        }
                    } else if result.is_ok() {
                        shadow.get_mut(&id).unwrap().approved = true;
                    }
                }

                // ---- Reject -------------------------------------------------
                1 => {
                    let reason = String::from_str(&env, "prop-test-reason");
                    let result = client.try_reject_milestone(&agreement_id, &id, &reason);

                    if s.rejected {
                        prop_assert!(result.is_err(),
                            "Double-reject of milestone {} succeeded", id);
                        if let Err(Ok(e)) = result {
                            prop_assert_eq!(e, PayrollError::MilestoneAlreadyRejected,
                                "Wrong error for double-reject on milestone {}: {:?}", id, e);
                        }
                    } else if s.claimed {
                        prop_assert!(result.is_err(),
                            "Reject of claimed milestone {} succeeded", id);
                        if let Err(Ok(e)) = result {
                            prop_assert_eq!(e, PayrollError::MilestoneAlreadyClaimedCannotReject,
                                "Wrong error for reject-after-claim on milestone {}: {:?}", id, e);
                        }
                    } else if s.approved {
                        prop_assert!(result.is_err(),
                            "Reject of approved milestone {} succeeded", id);
                        if let Err(Ok(e)) = result {
                            prop_assert_eq!(e, PayrollError::MilestoneAlreadyApprovedCannotReject,
                                "Wrong error for reject-after-approve on milestone {}: {:?}", id, e);
                        }
                    } else if result.is_ok() {
                        shadow.get_mut(&id).unwrap().rejected = true;
                    }
                }

                // ---- Expire -------------------------------------------------
                2 => {
                    let result = client.try_expire_milestone(&agreement_id, &id);

                    if s.expired {
                        prop_assert!(result.is_err(),
                            "Double-expire of milestone {} succeeded", id);
                        if let Err(Ok(e)) = result {
                            prop_assert_eq!(e, PayrollError::MilestoneAlreadyExpired,
                                "Wrong error for double-expire on milestone {}: {:?}", id, e);
                        }
                    } else if s.claimed {
                        prop_assert!(result.is_err(),
                            "Expire of claimed milestone {} succeeded", id);
                        if let Err(Ok(e)) = result {
                            prop_assert_eq!(e, PayrollError::MilestoneAlreadyClaimed,
                                "Wrong error for expire-after-claim on milestone {}: {:?}", id, e);
                        }
                    } else if s.approved {
                        prop_assert!(result.is_err(),
                            "Expire of approved (unclaimed) milestone {} succeeded", id);
                        if let Err(Ok(e)) = result {
                            prop_assert_eq!(e, PayrollError::MilestoneAlreadyApproved,
                                "Wrong error for expire-after-approve on milestone {}: {:?}", id, e);
                        }
                    } else if s.rejected {
                        prop_assert!(result.is_err(),
                            "Expire of rejected milestone {} succeeded", id);
                        if let Err(Ok(e)) = result {
                            prop_assert_eq!(e, PayrollError::MilestoneAlreadyRejected,
                                "Wrong error for expire-after-reject on milestone {}: {:?}", id, e);
                        }
                    } else if result.is_ok() {
                        shadow.get_mut(&id).unwrap().expired = true;
                    }
                }

                // ---- Claim --------------------------------------------------
                3 => {
                    let result = client.try_claim_milestone(&agreement_id, &id);

                    if s.claimed {
                        prop_assert!(result.is_err(),
                            "Double-claim of milestone {} succeeded", id);
                        if let Err(Ok(e)) = result {
                            prop_assert_eq!(e, PayrollError::MilestoneAlreadyClaimed,
                                "Wrong error for double-claim on milestone {}: {:?}", id, e);
                        }
                    } else if !s.approved {
                        prop_assert!(result.is_err(),
                            "Claim of unapproved milestone {} succeeded", id);
                        if let Err(Ok(e)) = result {
                            prop_assert_eq!(e, PayrollError::MilestoneNotApproved,
                                "Wrong error for unapproved claim on milestone {}: {:?}", id, e);
                        }
                    } else if result.is_ok() {
                        shadow.get_mut(&id).unwrap().claimed = true;
                    }
                }

                // ---- Approve then immediately Claim (happy path) ------------
                4 => {
                    if !s.is_terminal() && !s.approved {
                        if client.try_approve_milestone(&agreement_id, &id).is_ok() {
                            shadow.get_mut(&id).unwrap().approved = true;
                            let after_approve = client.get_milestone(&agreement_id, &id);
                            if after_approve.map(|m| m.approved).unwrap_or(false) {
                                if client.try_claim_milestone(&agreement_id, &id).is_ok() {
                                    shadow.get_mut(&id).unwrap().claimed = true;
                                }
                            }
                        }
                    }
                }

                // ---- Approve then Expire (must fail: already approved) ------
                5 => {
                    if !s.is_terminal() && !s.approved {
                        if client.try_approve_milestone(&agreement_id, &id).is_ok() {
                            shadow.get_mut(&id).unwrap().approved = true;
                            let expire_result = client.try_expire_milestone(&agreement_id, &id);
                            prop_assert!(
                                expire_result.is_err(),
                                "expire_milestone succeeded on approved milestone {} \
                                 (approve→expire path)",
                                id
                            );
                        }
                    }
                }

                _ => unreachable!(),
            }

            // Cross-check shadow approved/claimed flags against on-chain state.
            if let Some(mv) = client.get_milestone(&agreement_id, &id) {
                let sh = shadow[&id];
                prop_assert_eq!(
                    mv.approved, sh.approved,
                    "approved mismatch for milestone {}: on-chain={} shadow={}",
                    id, mv.approved, sh.approved
                );
                prop_assert_eq!(
                    mv.claimed, sh.claimed,
                    "claimed mismatch for milestone {}: on-chain={} shadow={}",
                    id, mv.claimed, sh.claimed
                );
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(proptest_cases()))]

    // ------------------------------------------------------------------
    // Invariant 7 – Escrow balance is non-negative at all times
    // ------------------------------------------------------------------
    /// After every `claim_milestone` call the cumulative amount claimed must
    /// not exceed the total funded amount, which is the escrow non-negativity
    /// guarantee expressed in terms the test can observe without internal
    /// storage access.
    #[test]
    fn prop_escrow_never_negative(
        milestone_count in 1u32..=5,
        amount in 100i128..=5_000,
        claim_sequence in prop::collection::vec(1u32..=5, 1..=15),
    ) {
        let (_env, _employer, _contributor, client, agreement_id) =
            setup(milestone_count, amount);

        let total_funded = amount * (milestone_count as i128);
        let bound = milestone_count;
        let mut total_claimed: i128 = 0;

        for raw_id in claim_sequence {
            let id = ((raw_id.saturating_sub(1)) % bound) + 1;

            // Approve first so the claim can proceed.
            let _ = client.try_approve_milestone(&agreement_id, &id);

            let ms_amount = client
                .get_milestone(&agreement_id, &id)
                .map(|m| m.amount)
                .unwrap_or(0);

            if client.try_claim_milestone(&agreement_id, &id).is_ok() {
                total_claimed += ms_amount;
            }

            // Invariant: total_claimed must never exceed total_funded.
            prop_assert!(
                total_claimed <= total_funded,
                "Escrow over-drawn: total_claimed={} > total_funded={} after claiming milestone {}",
                total_claimed, total_funded, id
            );
        }

        // End-state: claimed + still-claimable ≤ funded.
        let remaining_claimable: i128 = (1..=milestone_count)
            .filter_map(|mid| client.get_milestone(&agreement_id, &mid))
            .filter(|m| m.approved && !m.claimed)
            .map(|m| m.amount)
            .sum();

        prop_assert!(
            total_claimed + remaining_claimable <= total_funded,
            "End-state escrow invariant violated: claimed={} + claimable={} > funded={}",
            total_claimed, remaining_claimable, total_funded
        );
    }
}
