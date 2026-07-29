//! Compliance checker rules-engine tests.

#![cfg(test)]
#![allow(deprecated)]

use compliance_checker::{
    AgreementStatus, ComplianceCheckerContract, ComplianceCheckerContractClient, Decision,
    PayrollAction, ReasonCode, TraceRule,
};
use soroban_sdk::{testutils::Address as _, Address, Env};

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

    #[test]
    fn test_uninitialized_auxiliary_fails_closed() {
        let env = create_env();
        let (_cid, client, _admin) = setup(&env);
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
        client.set_auxiliary_allowed(&_admin, &unknown_auxiliary, &true);

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
        let (_cid, client, _admin) = setup(&env);
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

        // Test: all uninitialized auxiliaries should fail closed for all actions
        for aux in [aux1.clone(), aux2.clone(), aux3.clone()] {
            for action in actions {
                let decision = client.check_action(
                    &actor,
                    &aux,
                    &action,
                    &AgreementStatus::Created,
                    &AgreementStatus::Active,
                    &false,
                );

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
        client.set_auxiliary_allowed(&_admin, &aux1, &true);

        // aux1 should now pass, aux2 and aux3 should still fail
        for action in actions {
            // aux1 should pass
            let decision_aux1 = client.check_action(
                &actor,
                &aux1,
                &action,
                &AgreementStatus::Created,
                &AgreementStatus::Active,
                &false,
            );
            assert_eq!(decision_aux1.decision, Decision::Allow);
            assert_eq!(decision_aux1.reason, ReasonCode::Allowed);

            // aux2 should still fail (never initialized)
            let decision_aux2 = client.check_action(
                &actor,
                &aux2,
                &action,
                &AgreementStatus::Created,
                &AgreementStatus::Active,
                &false,
            );
            assert_eq!(decision_aux2.decision, Decision::Deny);
            assert_eq!(decision_aux2.reason, ReasonCode::AuxiliaryNotAllowed);

            // aux3 should still fail (never initialized)
            let decision_aux3 = client.check_action(
                &actor,
                &aux3,
                &action,
                &AgreementStatus::Created,
                &AgreementStatus::Active,
                &false,
            );
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

    // 3. The very next check_action call must see the paused state —
    //    no stale-read window or delayed propagation.
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
    assert!(after.traces.len() >= 1);
    let first_trace = after.traces.get(0).unwrap();
    assert_eq!(first_trace.rule, TraceRule::EmergencyPause);
    assert_eq!(first_trace.result, Decision::Deny);
    assert_eq!(first_trace.reason, ReasonCode::EmergencyPaused);
}
