//! Compliance checker rules-engine tests.

#![cfg(test)]
#![allow(deprecated)]

use compliance_checker::{
    AgreementStatus, ComplianceCheckerContract, ComplianceCheckerContractClient,
    ComplianceDecision, Decision, PayrollAction, ReasonCode, TraceRule,
};
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup(env: &Env) -> (Address, ComplianceCheckerContractClient<'_>, Address) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, ComplianceCheckerContract);
    let client = ComplianceCheckerContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (contract_id, client, admin)
}

#[test]
fn test_valid_transition_allows_activate_created_to_active() {
    let env = create_env();
    let (_cid, client, _admin) = setup(&env);
    let actor = Address::generate(&env);
    let decision = client.check_action(
        &actor,
        &actor,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );

    assert_eq!(decision.decision, Decision::Allow);
    assert_eq!(decision.reason, ReasonCode::Allowed);

    // Verify traces:
    // 1. EmergencyPause: Allow
    // 2. TerminalState: Allow
    // 3. InvalidCurrentState: Allow
    // 4. InvalidTargetState: Allow
    // (GracePeriodRequired skipped for ActivateAgreement)
    assert_eq!(decision.traces.len(), 4);
}

#[test]
fn test_non_allowlisted_auxiliary_is_denied() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);
    let actor = Address::generate(&env);
    let auxiliary = Address::generate(&env);

    let decision = client.check_action(
        &actor,
        &auxiliary,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(decision.decision, Decision::Deny);
    assert_eq!(decision.reason, ReasonCode::AuxiliaryNotAllowed);

    client.set_auxiliary_allowed(&admin, &auxiliary, &true);

    let decision_after_allow = client.check_action(
        &actor,
        &auxiliary,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(decision_after_allow.decision, Decision::Allow);
    assert_eq!(decision_after_allow.reason, ReasonCode::Allowed);
}

#[test]
fn test_emergency_pause_has_highest_precedence() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);
    let actor = Address::generate(&env);
    let auxiliary = Address::generate(&env);
    client.set_emergency_pause(&admin, &true);

    let decision = client.check_action(
        &actor,
        &auxiliary,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(decision.decision, Decision::Deny);
    assert_eq!(decision.reason, ReasonCode::EmergencyPaused);
}

#[test]
fn test_completed_state_denies_all_actions() {
    let env = create_env();
    let (_cid, client, _admin) = setup(&env);
    let actor = Address::generate(&env);

    let actions = [
        PayrollAction::AddEmployee,
        PayrollAction::ActivateAgreement,
        PayrollAction::PauseAgreement,
        PayrollAction::ResumeAgreement,
        PayrollAction::CancelAgreement,
        PayrollAction::FinalizeGracePeriod,
        PayrollAction::RaiseDispute,
        PayrollAction::ResolveDispute,
        PayrollAction::ClaimPayroll,
        PayrollAction::ClaimTimeBased,
        PayrollAction::ClaimMilestone,
    ];

    for action in actions {
        let decision = client.check_action(
            &actor,
            &actor,
            &action,
            &AgreementStatus::Completed,
            &AgreementStatus::Completed,
            &false,
        );
        assert_eq!(decision.decision, Decision::Deny);
        assert_eq!(decision.reason, ReasonCode::TerminalState);
    }
}

#[test]
fn test_invalid_current_state_matrix_is_denied() {
    let env = create_env();
    let (_cid, client, _admin) = setup(&env);
    let actor = Address::generate(&env);

    let deny_cases = [
        (
            PayrollAction::AddEmployee,
            AgreementStatus::Active,
            AgreementStatus::Created,
        ),
        (
            PayrollAction::ActivateAgreement,
            AgreementStatus::Paused,
            AgreementStatus::Active,
        ),
        (
            PayrollAction::PauseAgreement,
            AgreementStatus::Created,
            AgreementStatus::Paused,
        ),
        (
            PayrollAction::ResumeAgreement,
            AgreementStatus::Active,
            AgreementStatus::Active,
        ),
        (
            PayrollAction::CancelAgreement,
            AgreementStatus::Paused,
            AgreementStatus::Cancelled,
        ),
        (
            PayrollAction::FinalizeGracePeriod,
            AgreementStatus::Active,
            AgreementStatus::Cancelled,
        ),
        (
            PayrollAction::RaiseDispute,
            AgreementStatus::Disputed,
            AgreementStatus::Disputed,
        ),
        (
            PayrollAction::ResolveDispute,
            AgreementStatus::Active,
            AgreementStatus::Completed,
        ),
        (
            PayrollAction::ClaimPayroll,
            AgreementStatus::Paused,
            AgreementStatus::Paused,
        ),
        (
            PayrollAction::ClaimTimeBased,
            AgreementStatus::Created,
            AgreementStatus::Created,
        ),
        (
            PayrollAction::ClaimMilestone,
            AgreementStatus::Disputed,
            AgreementStatus::Disputed,
        ),
    ];

    for (action, current, target) in deny_cases {
        let decision = client.check_action(&actor, &actor, &action, &current, &target, &false);
        assert_eq!(decision.decision, Decision::Deny);
        assert_eq!(decision.reason, ReasonCode::InvalidCurrentState);
    }
}

#[test]
fn test_invalid_target_state_is_denied() {
    let env = create_env();
    let (_cid, client, _admin) = setup(&env);
    let actor = Address::generate(&env);

    let cases = [
        (
            PayrollAction::ActivateAgreement,
            AgreementStatus::Created,
            AgreementStatus::Paused,
        ),
        (
            PayrollAction::PauseAgreement,
            AgreementStatus::Active,
            AgreementStatus::Active,
        ),
        (
            PayrollAction::ResumeAgreement,
            AgreementStatus::Paused,
            AgreementStatus::Paused,
        ),
        (
            PayrollAction::CancelAgreement,
            AgreementStatus::Created,
            AgreementStatus::Active,
        ),
        (
            PayrollAction::FinalizeGracePeriod,
            AgreementStatus::Cancelled,
            AgreementStatus::Completed,
        ),
        (
            PayrollAction::RaiseDispute,
            AgreementStatus::Active,
            AgreementStatus::Active,
        ),
        (
            PayrollAction::ResolveDispute,
            AgreementStatus::Disputed,
            AgreementStatus::Disputed,
        ),
    ];

    for (action, current, bad_target) in cases {
        let decision = client.check_action(&actor, &actor, &action, &current, &bad_target, &false);
        assert_eq!(decision.decision, Decision::Deny);
        assert_eq!(decision.reason, ReasonCode::InvalidTargetState);
    }
}

#[test]
fn test_claims_from_cancelled_require_grace_period() {
    let env = create_env();
    let (_cid, client, _admin) = setup(&env);
    let actor = Address::generate(&env);

    let claim_actions = [
        PayrollAction::ClaimPayroll,
        PayrollAction::ClaimTimeBased,
        PayrollAction::ClaimMilestone,
    ];
    for action in claim_actions {
        let denied = client.check_action(
            &actor,
            &actor,
            &action,
            &AgreementStatus::Cancelled,
            &AgreementStatus::Cancelled,
            &false,
        );
        assert_eq!(denied.decision, Decision::Deny);
        assert_eq!(denied.reason, ReasonCode::GracePeriodRequired);

        let allowed = client.check_action(
            &actor,
            &actor,
            &action,
            &AgreementStatus::Cancelled,
            &AgreementStatus::Cancelled,
            &true,
        );
        assert_eq!(allowed.decision, Decision::Allow);
        assert_eq!(allowed.reason, ReasonCode::Allowed);
    }
}

#[test]
fn test_deny_traces_include_rule_and_reason() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);
    let actor = Address::generate(&env);
    let auxiliary = Address::generate(&env);

    let emergency = client.check_action(
        &actor,
        &auxiliary,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(emergency.decision, Decision::Deny);
    assert_eq!(emergency.reason, ReasonCode::AuxiliaryNotAllowed);
    let trace = emergency.traces.get(1).unwrap();
    assert_eq!(trace.rule, TraceRule::AuxiliaryNotAllowed);
    assert_eq!(trace.result, Decision::Deny);
    assert_eq!(trace.reason, ReasonCode::AuxiliaryNotAllowed);

    client.set_emergency_pause(&admin, &true);
    let emergency = client.check_action(
        &actor,
        &auxiliary,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(emergency.decision, Decision::Deny);
    assert_eq!(emergency.reason, ReasonCode::EmergencyPaused);
    let trace = emergency.traces.get(0).unwrap();
    assert_eq!(trace.rule, TraceRule::EmergencyPause);
    assert_eq!(trace.result, Decision::Deny);
    assert_eq!(trace.reason, ReasonCode::EmergencyPaused);
}

#[test]
fn test_ruleset_upgrade_emergency_pause_affects_new_evaluations_only() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);
    let actor = Address::generate(&env);

    // 1. Evaluation under initial rules (emergency pause OFF)
    let decision_before = client.check_action(
        &actor,
        &actor,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(decision_before.decision, Decision::Allow);
    assert_eq!(decision_before.reason, ReasonCode::Allowed);

    // 2. Admin upgrades rule set: enable emergency pause
    client.set_emergency_pause(&admin, &true);

    // 3. New evaluation must reflect the updated rules
    let decision_after = client.check_action(
        &actor,
        &actor,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(decision_after.decision, Decision::Deny);
    assert_eq!(decision_after.reason, ReasonCode::EmergencyPaused);

    // 4. The original decision struct is a plain value type and is unchanged
    assert_eq!(decision_before.decision, Decision::Allow);
    assert_eq!(decision_before.reason, ReasonCode::Allowed);

    // 5. Admin downgrades: disable emergency pause
    client.set_emergency_pause(&admin, &false);

    // 6. New evaluation returns to Allow
    let decision_restored = client.check_action(
        &actor,
        &actor,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(decision_restored.decision, Decision::Allow);
    assert_eq!(decision_restored.reason, ReasonCode::Allowed);

    // 7. Previously captured deny decision remains unchanged
    assert_eq!(decision_after.decision, Decision::Deny);
    assert_eq!(decision_after.reason, ReasonCode::EmergencyPaused);
}

#[test]
fn test_allow_path_traces_have_none_reasons() {
    let env = create_env();
    let (_cid, client, _admin) = setup(&env);
    let actor = Address::generate(&env);

    let decision = client.check_action(
        &actor,
        &actor,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );

    assert_eq!(decision.decision, Decision::Allow);
    assert_eq!(decision.reason, ReasonCode::Allowed);
    assert_eq!(decision.traces.len(), 4);
    for i in 0..decision.traces.len() {
        assert_eq!(decision.traces.get(i).unwrap().reason, ReasonCode::Allowed);
    }
}

#[test]
fn test_uninitialized_auxiliary_fails_closed() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);
    let actor = Address::generate(&env);
    let unknown_auxiliary = Address::generate(&env);

    // Test: check_action for an auxiliary that was NEVER initialized
    // via set_auxiliary_allowed should fail closed (deny)
    let decision = client.check_action(
        &actor,
        &unknown_auxiliary,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );

    // Assert it fails closed with auxiliary not allowed
    assert_eq!(decision.decision, Decision::Deny);
    assert_eq!(decision.reason, ReasonCode::AuxiliaryNotAllowed);

    // Verify the trace shows the auxiliary check was the denial reason
    let trace = decision.traces.get(1).unwrap();
    assert_eq!(trace.rule, TraceRule::AuxiliaryNotAllowed);
    assert_eq!(trace.result, Decision::Deny);
    assert_eq!(trace.reason, ReasonCode::AuxiliaryNotAllowed);

    // Now explicitly configure the auxiliary
    client.set_auxiliary_allowed(&admin, &unknown_auxiliary, &true);

    // Test: same action should now pass with explicit configuration
    let allowed_decision = client.check_action(
        &actor,
        &unknown_auxiliary,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );

    // Assert it now allows the action
    assert_eq!(allowed_decision.decision, Decision::Allow);
    assert_eq!(allowed_decision.reason, ReasonCode::Allowed);
}

#[test]
fn test_multiple_uninitialized_auxiliaries_fail_closed() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);
    let actor = Address::generate(&env);

    // Generate multiple uninitialized auxiliaries
    let aux1 = Address::generate(&env);
    let aux2 = Address::generate(&env);
    let aux3 = Address::generate(&env);

    let actions = [
        PayrollAction::ActivateAgreement,
        PayrollAction::PauseAgreement,
        PayrollAction::ClaimPayroll,
    ];

    // Each action is paired with a transition that is valid for it, so a Deny
    // can only come from the auxiliary rule and not from the state rules.
    // Test: all uninitialized auxiliaries should fail closed for all actions
    for aux in [aux1.clone(), aux2.clone(), aux3.clone()] {
        for action in actions {
            let (current, target) = valid_transition(action);
            let decision = client.check_action(&actor, &aux, &action, &current, &target, &false);

            // Should always deny for uninitialized auxiliaries
            assert_eq!(decision.decision, Decision::Deny);
            assert_eq!(decision.reason, ReasonCode::AuxiliaryNotAllowed);

            // Verify it's the auxiliary check that denied, not another rule
            let trace = decision.traces.get(1).unwrap();
            assert_eq!(trace.rule, TraceRule::AuxiliaryNotAllowed);
            assert_eq!(trace.result, Decision::Deny);
        }
    }

    // Explicitly configure aux1 only
    client.set_auxiliary_allowed(&admin, &aux1, &true);

    // aux1 should now pass, aux2 and aux3 should still fail
    for action in actions {
        let (current, target) = valid_transition(action);

        // aux1 should pass
        let decision_aux1 = client.check_action(&actor, &aux1, &action, &current, &target, &false);
        assert_eq!(decision_aux1.decision, Decision::Allow);
        assert_eq!(decision_aux1.reason, ReasonCode::Allowed);

        // aux2 should still fail (never initialized)
        let decision_aux2 = client.check_action(&actor, &aux2, &action, &current, &target, &false);
        assert_eq!(decision_aux2.decision, Decision::Deny);
        assert_eq!(decision_aux2.reason, ReasonCode::AuxiliaryNotAllowed);

        // aux3 should still fail (never initialized)
        let decision_aux3 = client.check_action(&actor, &aux3, &action, &current, &target, &false);
        assert_eq!(decision_aux3.decision, Decision::Deny);
        assert_eq!(decision_aux3.reason, ReasonCode::AuxiliaryNotAllowed);
    }
}

#[test]
fn test_auxiliary_allowlist_removal_restores_fail_closed() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);
    let actor = Address::generate(&env);
    let auxiliary = Address::generate(&env);

    // Explicitly configure and verify it works
    client.set_auxiliary_allowed(&admin, &auxiliary, &true);

    let decision = client.check_action(
        &actor,
        &auxiliary,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(decision.decision, Decision::Allow);

    // Remove from allowlist (de-initialize)
    client.set_auxiliary_allowed(&admin, &auxiliary, &false);

    // Should fail closed again
    let decision_removed = client.check_action(
        &actor,
        &auxiliary,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(decision_removed.decision, Decision::Deny);
    assert_eq!(decision_removed.reason, ReasonCode::AuxiliaryNotAllowed);
}

#[test]
fn test_set_emergency_pause_rejects_non_admin() {
    let env = create_env();
    let (_cid, client, _admin) = setup(&env);
    let non_admin = Address::generate(&env);

    // An address that is not the admin must not be able to toggle the
    // emergency-pause flag.  With `mock_all_auths` the authentication inside
    // `require_admin` passes for any caller, but the `*caller == admin` check
    // panics with "Not admin".
    let result = client.try_set_emergency_pause(&non_admin, &true);
    assert!(result.is_err(), "non-admin caller must be rejected");
}

#[test]
fn test_set_emergency_pause_immediate_effect() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);
    let actor = Address::generate(&env);

    // 1. Before pausing, a valid transition is Allowed.
    let before = client.check_action(
        &actor,
        &actor,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(
        before.decision,
        Decision::Allow,
        "action must be allowed before emergency pause"
    );

    // 2. Pause with immediate effect.
    client.set_emergency_pause(&admin, &true);

    // 3. The very next check_action call must see the paused state — no stale-read window or
    //    delayed propagation.
    let after = client.check_action(
        &actor,
        &actor,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(
        after.decision,
        Decision::Deny,
        "action must be denied immediately after emergency pause"
    );
    assert_eq!(
        after.reason,
        ReasonCode::EmergencyPaused,
        "denial reason must be EmergencyPaused"
    );
    // The first trace entry must be the emergency-pause rule with Deny.
    assert!(!after.traces.is_empty());
    let first_trace = after.traces.get(0).unwrap();
    assert_eq!(first_trace.rule, TraceRule::EmergencyPause);
    assert_eq!(first_trace.result, Decision::Deny);
    assert_eq!(first_trace.reason, ReasonCode::EmergencyPaused);
}

// ---------------------------------------------------------------------------
// Rule-removal enforcement-stop tests
//
// These tests pin down the guarantee that `check_action` enforces the rule set
// as it exists *at evaluation time*. A rule that has been removed must stop
// being applied on the very next evaluation, and no stale or snapshotted copy
// of it may survive the removal.
// ---------------------------------------------------------------------------

/// Every payroll action, used to prove rule registration and removal are
/// scoped to exactly one action.
fn all_actions() -> [PayrollAction; 11] {
    [
        PayrollAction::AddEmployee,
        PayrollAction::ActivateAgreement,
        PayrollAction::PauseAgreement,
        PayrollAction::ResumeAgreement,
        PayrollAction::CancelAgreement,
        PayrollAction::FinalizeGracePeriod,
        PayrollAction::RaiseDispute,
        PayrollAction::ResolveDispute,
        PayrollAction::ClaimPayroll,
        PayrollAction::ClaimTimeBased,
        PayrollAction::ClaimMilestone,
    ]
}

/// A `(action, current, target)` triple that is compliant once no blocking rule
/// applies, so a `Deny` can only come from the registered rule under test.
fn valid_transition(action: PayrollAction) -> (AgreementStatus, AgreementStatus) {
    match action {
        PayrollAction::AddEmployee => (AgreementStatus::Created, AgreementStatus::Created),
        PayrollAction::ActivateAgreement => (AgreementStatus::Created, AgreementStatus::Active),
        PayrollAction::PauseAgreement => (AgreementStatus::Active, AgreementStatus::Paused),
        PayrollAction::ResumeAgreement => (AgreementStatus::Paused, AgreementStatus::Active),
        PayrollAction::CancelAgreement => (AgreementStatus::Active, AgreementStatus::Cancelled),
        PayrollAction::FinalizeGracePeriod => {
            (AgreementStatus::Cancelled, AgreementStatus::Cancelled)
        }
        PayrollAction::RaiseDispute => (AgreementStatus::Active, AgreementStatus::Disputed),
        PayrollAction::ResolveDispute => (AgreementStatus::Disputed, AgreementStatus::Completed),
        PayrollAction::ClaimPayroll
        | PayrollAction::ClaimTimeBased
        | PayrollAction::ClaimMilestone => (AgreementStatus::Active, AgreementStatus::Active),
    }
}

#[test]
fn test_registered_rule_blocks_action_then_removal_stops_enforcement() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);
    let actor = Address::generate(&env);

    // 1. Baseline: the transition is compliant with no rule registered.
    let before = client.check_action(
        &actor,
        &actor,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(before.decision, Decision::Allow);
    assert!(!client.is_rule_registered(&PayrollAction::ActivateAgreement));

    // 2. Register a rule blocking that specific action.
    client.register_rule(&admin, &PayrollAction::ActivateAgreement);
    assert!(client.is_rule_registered(&PayrollAction::ActivateAgreement));

    let blocked = client.check_action(
        &actor,
        &actor,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(
        blocked.decision,
        Decision::Deny,
        "registered rule must block the action"
    );
    assert_eq!(blocked.reason, ReasonCode::ActionBlocked);

    // The denial must be attributed to the rule itself, not to an unrelated
    // check that happens to also fail.
    let trace = blocked.traces.get(1).unwrap();
    assert_eq!(trace.rule, TraceRule::ActionBlocked);
    assert_eq!(trace.result, Decision::Deny);
    assert_eq!(trace.reason, ReasonCode::ActionBlocked);

    // 3. Remove the rule.
    client.remove_rule(&admin, &PayrollAction::ActivateAgreement);
    assert!(
        !client.is_rule_registered(&PayrollAction::ActivateAgreement),
        "removed rule must not remain registered"
    );

    // 4. The very next evaluation must allow the previously-blocked action.
    let after = client.check_action(
        &actor,
        &actor,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(
        after.decision,
        Decision::Allow,
        "removed rule must stop being enforced immediately"
    );
    assert_eq!(after.reason, ReasonCode::Allowed);

    // No ActionBlocked trace may survive the removal — a stale cached rule
    // would still record an evaluation here even if it decided Allow.
    for i in 0..after.traces.len() {
        assert_ne!(after.traces.get(i).unwrap().rule, TraceRule::ActionBlocked);
    }
}

#[test]
fn test_rule_removal_is_scoped_to_the_removed_action() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);
    let actor = Address::generate(&env);

    // Register a blocking rule for every action.
    for action in all_actions() {
        client.register_rule(&admin, &action);
    }

    // Remove them one at a time; after each removal exactly the removed actions
    // are allowed and every still-registered action stays blocked.
    let actions = all_actions();
    for (removed_count, removed) in actions.iter().enumerate() {
        client.remove_rule(&admin, removed);

        for (i, action) in actions.iter().enumerate() {
            let (current, target) = valid_transition(*action);
            let decision = client.check_action(&actor, &actor, action, &current, &target, &false);

            if i <= removed_count {
                assert_eq!(
                    decision.decision,
                    Decision::Allow,
                    "action with a removed rule must no longer be blocked"
                );
                assert!(!client.is_rule_registered(action));
            } else {
                assert_eq!(
                    decision.decision,
                    Decision::Deny,
                    "unrelated rules must survive an adjacent removal"
                );
                assert_eq!(decision.reason, ReasonCode::ActionBlocked);
                assert!(client.is_rule_registered(action));
            }
        }
    }
}

#[test]
fn test_rule_removal_does_not_blanket_allow_the_action() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);
    let actor = Address::generate(&env);

    client.register_rule(&admin, &PayrollAction::PauseAgreement);
    client.remove_rule(&admin, &PayrollAction::PauseAgreement);

    // Removing the rule must return the action to normal evaluation, not
    // exempt it from the remaining rules. PauseAgreement is invalid from
    // Created, so the state rules must still deny it.
    let decision = client.check_action(
        &actor,
        &actor,
        &PayrollAction::PauseAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Paused,
        &false,
    );
    assert_eq!(decision.decision, Decision::Deny);
    assert_eq!(
        decision.reason,
        ReasonCode::InvalidCurrentState,
        "removal must restore normal evaluation, not bypass the other rules"
    );
}

#[test]
fn test_removed_rule_can_be_registered_again() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);
    let actor = Address::generate(&env);

    let check = |client: &ComplianceCheckerContractClient| {
        client.check_action(
            &actor,
            &actor,
            &PayrollAction::CancelAgreement,
            &AgreementStatus::Active,
            &AgreementStatus::Cancelled,
            &false,
        )
    };

    for _ in 0..3 {
        client.register_rule(&admin, &PayrollAction::CancelAgreement);
        assert_eq!(check(&client).decision, Decision::Deny);

        client.remove_rule(&admin, &PayrollAction::CancelAgreement);
        assert_eq!(check(&client).decision, Decision::Allow);
    }
}

#[test]
fn test_register_and_remove_rule_are_idempotent() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);
    let actor = Address::generate(&env);

    // Removing a rule that was never registered is a no-op, not an error.
    client.remove_rule(&admin, &PayrollAction::RaiseDispute);
    assert!(!client.is_rule_registered(&PayrollAction::RaiseDispute));

    client.register_rule(&admin, &PayrollAction::RaiseDispute);
    client.register_rule(&admin, &PayrollAction::RaiseDispute);
    assert!(client.is_rule_registered(&PayrollAction::RaiseDispute));

    // A double removal must not leave the rule half-enforced.
    client.remove_rule(&admin, &PayrollAction::RaiseDispute);
    client.remove_rule(&admin, &PayrollAction::RaiseDispute);
    assert!(!client.is_rule_registered(&PayrollAction::RaiseDispute));

    let decision = client.check_action(
        &actor,
        &actor,
        &PayrollAction::RaiseDispute,
        &AgreementStatus::Active,
        &AgreementStatus::Disputed,
        &false,
    );
    assert_eq!(decision.decision, Decision::Allow);
}

#[test]
fn test_register_rule_rejects_non_admin() {
    let env = create_env();
    let (_cid, client, _admin) = setup(&env);
    let non_admin = Address::generate(&env);

    let result = client.try_register_rule(&non_admin, &PayrollAction::ClaimPayroll);
    assert!(result.is_err(), "non-admin caller must not register rules");
    assert!(
        !client.is_rule_registered(&PayrollAction::ClaimPayroll),
        "rejected registration must not mutate the rule set"
    );
}

#[test]
fn test_remove_rule_rejects_non_admin() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);
    let actor = Address::generate(&env);
    let non_admin = Address::generate(&env);

    client.register_rule(&admin, &PayrollAction::ClaimPayroll);

    let result = client.try_remove_rule(&non_admin, &PayrollAction::ClaimPayroll);
    assert!(result.is_err(), "non-admin caller must not remove rules");

    // The rule must still be enforced after the rejected removal — a guard that
    // rejected the caller but still deleted the entry would be caught here.
    assert!(client.is_rule_registered(&PayrollAction::ClaimPayroll));
    let decision = client.check_action(
        &actor,
        &actor,
        &PayrollAction::ClaimPayroll,
        &AgreementStatus::Active,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(decision.decision, Decision::Deny);
    assert_eq!(decision.reason, ReasonCode::ActionBlocked);

    // Only the admin can lift it.
    client.remove_rule(&admin, &PayrollAction::ClaimPayroll);
    let decision = client.check_action(
        &actor,
        &actor,
        &PayrollAction::ClaimPayroll,
        &AgreementStatus::Active,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(decision.decision, Decision::Allow);
}

#[test]
fn test_emergency_pause_outranks_registered_rule() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);
    let actor = Address::generate(&env);

    client.register_rule(&admin, &PayrollAction::ActivateAgreement);
    client.set_emergency_pause(&admin, &true);

    // Precedence is fixed: the pause reason wins so operators get a stable
    // reason code regardless of which rules also happen to be registered.
    let decision = client.check_action(
        &actor,
        &actor,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(decision.decision, Decision::Deny);
    assert_eq!(decision.reason, ReasonCode::EmergencyPaused);

    // Lifting the pause exposes the still-registered rule underneath it.
    client.set_emergency_pause(&admin, &false);
    let decision = client.check_action(
        &actor,
        &actor,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(decision.decision, Decision::Deny);
    assert_eq!(decision.reason, ReasonCode::ActionBlocked);
}

#[test]
fn test_registered_rule_blocks_allowlisted_auxiliary_executor() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);
    let actor = Address::generate(&env);
    let auxiliary = Address::generate(&env);

    client.set_auxiliary_allowed(&admin, &auxiliary, &true);
    client.register_rule(&admin, &PayrollAction::ActivateAgreement);

    // A rule blocks the action itself, so an allowlisted executor cannot route
    // around it.
    let decision = client.check_action(
        &actor,
        &auxiliary,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(decision.decision, Decision::Deny);
    assert_eq!(decision.reason, ReasonCode::ActionBlocked);

    client.remove_rule(&admin, &PayrollAction::ActivateAgreement);
    let decision = client.check_action(
        &actor,
        &auxiliary,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(decision.decision, Decision::Allow);
}

// ---------------------------------------------------------------------------
// In-flight evaluation
//
// `RuleSequencer` interleaves evaluations and rule mutations inside a *single*
// transaction, driving `check_action` through cross-contract calls. All calls
// in one transaction share the same ledger state, so this is the strongest
// available reproduction of an action that is in flight while the rule set
// changes underneath it. Each returned decision must reflect the rule set as of
// its own evaluation, never a snapshot taken when the transaction started.
// ---------------------------------------------------------------------------

#[contract]
pub struct RuleSequencer;

#[contractimpl]
impl RuleSequencer {
    /// Evaluates, removes the rule, then evaluates again — all in one
    /// transaction. Returns both decisions in evaluation order.
    pub fn check_remove_check(
        env: Env,
        checker: Address,
        admin: Address,
        actor: Address,
        action: PayrollAction,
        current: AgreementStatus,
        target: AgreementStatus,
    ) -> (ComplianceDecision, ComplianceDecision) {
        let client = ComplianceCheckerContractClient::new(&env, &checker);

        let before = client.check_action(&actor, &actor, &action, &current, &target, &false);
        client.remove_rule(&admin, &action);
        let after = client.check_action(&actor, &actor, &action, &current, &target, &false);

        (before, after)
    }

    /// The mirror case: evaluate, register a rule, then evaluate again within
    /// one transaction.
    pub fn check_register_check(
        env: Env,
        checker: Address,
        admin: Address,
        actor: Address,
        action: PayrollAction,
        current: AgreementStatus,
        target: AgreementStatus,
    ) -> (ComplianceDecision, ComplianceDecision) {
        let client = ComplianceCheckerContractClient::new(&env, &checker);

        let before = client.check_action(&actor, &actor, &action, &current, &target, &false);
        client.register_rule(&admin, &action);
        let after = client.check_action(&actor, &actor, &action, &current, &target, &false);

        (before, after)
    }
}

/// `check_action` authenticates the actor from inside a sub-invocation, which
/// recording auth rejects unless non-root authorization is permitted.
fn create_env_for_sequencer() -> Env {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();
    env
}

fn setup_sequencer(env: &Env) -> RuleSequencerClient<'_> {
    #[allow(deprecated)]
    let sequencer_id = env.register_contract(None, RuleSequencer);
    RuleSequencerClient::new(env, &sequencer_id)
}

#[test]
fn test_in_flight_evaluation_reflects_rule_set_at_evaluation_time() {
    let env = create_env_for_sequencer();
    let (checker_id, client, admin) = setup(&env);
    let sequencer = setup_sequencer(&env);
    let actor = Address::generate(&env);

    client.register_rule(&admin, &PayrollAction::ActivateAgreement);

    let (before, after) = sequencer.check_remove_check(
        &checker_id,
        &admin,
        &actor,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
    );

    // The evaluation that ran before the removal is bound to the rule set at
    // its own evaluation time and is not retroactively changed by the removal.
    assert_eq!(
        before.decision,
        Decision::Deny,
        "evaluation preceding the removal must still see the rule"
    );
    assert_eq!(before.reason, ReasonCode::ActionBlocked);

    // The evaluation that ran after the removal — in the same transaction —
    // must already see the new rule set, not the transaction-start snapshot.
    assert_eq!(
        after.decision,
        Decision::Allow,
        "evaluation following the removal must not apply the removed rule"
    );
    assert_eq!(after.reason, ReasonCode::Allowed);
    for i in 0..after.traces.len() {
        assert_ne!(after.traces.get(i).unwrap().rule, TraceRule::ActionBlocked);
    }

    // The removal committed for observers outside the transaction too.
    assert!(!client.is_rule_registered(&PayrollAction::ActivateAgreement));
}

#[test]
fn test_in_flight_evaluation_reflects_rule_registered_mid_transaction() {
    let env = create_env_for_sequencer();
    let (checker_id, client, admin) = setup(&env);
    let sequencer = setup_sequencer(&env);
    let actor = Address::generate(&env);

    let (before, after) = sequencer.check_register_check(
        &checker_id,
        &admin,
        &actor,
        &PayrollAction::CancelAgreement,
        &AgreementStatus::Active,
        &AgreementStatus::Cancelled,
    );

    assert_eq!(
        before.decision,
        Decision::Allow,
        "evaluation preceding the registration must not see the new rule"
    );
    assert_eq!(
        after.decision,
        Decision::Deny,
        "evaluation following the registration must apply the new rule"
    );
    assert_eq!(after.reason, ReasonCode::ActionBlocked);
    assert!(client.is_rule_registered(&PayrollAction::CancelAgreement));
}

#[test]
fn test_in_flight_removal_holds_for_every_action() {
    let env = create_env_for_sequencer();
    let (checker_id, client, admin) = setup(&env);
    let sequencer = setup_sequencer(&env);
    let actor = Address::generate(&env);

    for action in all_actions() {
        let (current, target) = valid_transition(action);
        client.register_rule(&admin, &action);

        let (before, after) =
            sequencer.check_remove_check(&checker_id, &admin, &actor, &action, &current, &target);

        assert_eq!(before.decision, Decision::Deny);
        assert_eq!(before.reason, ReasonCode::ActionBlocked);
        assert_eq!(after.decision, Decision::Allow);
        assert_eq!(after.reason, ReasonCode::Allowed);
        assert!(!client.is_rule_registered(&action));
    }
}

#[test]
fn test_rule_mutation_requires_initialization() {
    let env = create_env();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, ComplianceCheckerContract);
    let client = ComplianceCheckerContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    // Before `initialize`, there is no admin to authorize against, so rule
    // mutation must fail rather than fall through to an unguarded write.
    assert!(client
        .try_register_rule(&admin, &PayrollAction::ActivateAgreement)
        .is_err());
    assert!(client
        .try_remove_rule(&admin, &PayrollAction::ActivateAgreement)
        .is_err());
    assert!(!client.is_rule_registered(&PayrollAction::ActivateAgreement));

    // After initialization the same calls succeed for the admin.
    client.initialize(&admin);
    client.register_rule(&admin, &PayrollAction::ActivateAgreement);
    assert!(client.is_rule_registered(&PayrollAction::ActivateAgreement));
}

#[test]
fn test_initialize_is_single_shot() {
    let env = create_env();
    let (_cid, client, admin) = setup(&env);
    let attacker = Address::generate(&env);

    // Re-initialization must not be able to install a new admin, which would
    // otherwise hand rule-removal rights to an arbitrary caller.
    assert!(client.try_initialize(&attacker).is_err());

    client.register_rule(&admin, &PayrollAction::ClaimMilestone);
    assert!(client
        .try_remove_rule(&attacker, &PayrollAction::ClaimMilestone)
        .is_err());
    assert!(client.is_rule_registered(&PayrollAction::ClaimMilestone));
}
