//! Exhaustive negative matrix for invalid payroll transitions.

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

fn setup(env: &Env) -> (ComplianceCheckerContractClient<'_>, Address) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, ComplianceCheckerContract);
    let client = ComplianceCheckerContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (client, admin)
}

fn actions() -> [PayrollAction; 11] {
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

fn states() -> [AgreementStatus; 6] {
    [
        AgreementStatus::Created,
        AgreementStatus::Active,
        AgreementStatus::Paused,
        AgreementStatus::Cancelled,
        AgreementStatus::Completed,
        AgreementStatus::Disputed,
    ]
}

fn is_valid_current(action: PayrollAction, current: AgreementStatus) -> bool {
    match action {
        PayrollAction::AddEmployee => current == AgreementStatus::Created,
        PayrollAction::ActivateAgreement => current == AgreementStatus::Created,
        PayrollAction::PauseAgreement => current == AgreementStatus::Active,
        PayrollAction::ResumeAgreement => current == AgreementStatus::Paused,
        PayrollAction::CancelAgreement => {
            current == AgreementStatus::Created || current == AgreementStatus::Active
        }
        PayrollAction::FinalizeGracePeriod => current == AgreementStatus::Cancelled,
        PayrollAction::RaiseDispute => {
            current == AgreementStatus::Created
                || current == AgreementStatus::Active
                || current == AgreementStatus::Cancelled
        }
        PayrollAction::ResolveDispute => current == AgreementStatus::Disputed,
        PayrollAction::ClaimPayroll
        | PayrollAction::ClaimTimeBased
        | PayrollAction::ClaimMilestone => {
            current == AgreementStatus::Active || current == AgreementStatus::Cancelled
        }
    }
}

fn expected_target(action: PayrollAction, current: AgreementStatus) -> AgreementStatus {
    match action {
        PayrollAction::AddEmployee => AgreementStatus::Created,
        PayrollAction::ActivateAgreement => AgreementStatus::Active,
        PayrollAction::PauseAgreement => AgreementStatus::Paused,
        PayrollAction::ResumeAgreement => AgreementStatus::Active,
        PayrollAction::CancelAgreement => AgreementStatus::Cancelled,
        PayrollAction::FinalizeGracePeriod => AgreementStatus::Cancelled,
        PayrollAction::RaiseDispute => AgreementStatus::Disputed,
        PayrollAction::ResolveDispute => AgreementStatus::Completed,
        PayrollAction::ClaimPayroll
        | PayrollAction::ClaimTimeBased
        | PayrollAction::ClaimMilestone => current,
    }
}

#[test]
fn exhaustive_invalid_current_state_denies() {
    let env = create_env();
    let (client, _admin) = setup(&env);
    let actor = Address::generate(&env);

    for action in actions() {
        for current in states() {
            let target = expected_target(action, current);
            let decision = client.check_action(&actor, &actor, &action, &current, &target, &true);

            if current == AgreementStatus::Completed {
                assert_eq!(decision.decision, Decision::Deny);
                assert_eq!(decision.reason, ReasonCode::TerminalState);
                continue;
            }

            if !is_valid_current(action, current) {
                assert_eq!(decision.decision, Decision::Deny);
                assert_eq!(decision.reason, ReasonCode::InvalidCurrentState);
            }
        }
    }
}

#[test]
fn exhaustive_invalid_target_state_denies() {
    let env = create_env();
    let (client, _admin) = setup(&env);
    let actor = Address::generate(&env);

    for action in actions() {
        for current in states() {
            if current == AgreementStatus::Completed || !is_valid_current(action, current) {
                continue;
            }

            let good_target = expected_target(action, current);
            for bad_target in states() {
                if bad_target == good_target {
                    continue;
                }

                let decision =
                    client.check_action(&actor, &actor, &action, &current, &bad_target, &true);
                assert_eq!(decision.decision, Decision::Deny);
                assert_eq!(decision.reason, ReasonCode::InvalidTargetState);
            }
        }
    }
}

/// Adversarial matrix test for auxiliary-allowed and emergency-paused combinations.
///
/// This test exhaustively verifies that emergency pause always overrides auxiliary
/// allowlist status, as documented in docs/compliance-checker.md. It tests all
/// combinations of these two independent flags across all payroll actions.
///
/// Rule table:
/// | Emergency Paused | Auxiliary Allowed | Executor == Actor | Expected Decision | Expected Reason |
/// |-----------------|-------------------|-------------------|-------------------|-----------------|
/// | false           | N/A               | true              | Allow             | Allowed         |
/// | false           | true              | false             | Allow             | Allowed         |
/// | false           | false             | false             | Deny              | AuxiliaryNotAllowed |
/// | true            | N/A               | true              | Deny              | EmergencyPaused |
/// | true            | true              | false             | Deny              | EmergencyPaused |
/// | true            | false             | false             | Deny              | EmergencyPaused |
#[test]
fn adversarial_auxiliary_pause_matrix() {
    let env = create_env();
    let (client, admin) = setup(&env);
    let actor = Address::generate(&env);
    let auxiliary = Address::generate(&env);

    // Use a valid action/state combination for testing (ActivateAgreement from Created)
    let action = PayrollAction::ActivateAgreement;
    let current = AgreementStatus::Created;
    let target = AgreementStatus::Active;

    // Test 1: Emergency paused = false, executor == actor (direct call)
    // Expected: Allow, ReasonCode::Allowed
    client.set_emergency_pause(&admin, &false);
    let decision = client.check_action(&actor, &actor, &action, &current, &target, &false);
    assert_eq!(decision.decision, Decision::Allow);
    assert_eq!(decision.reason, ReasonCode::Allowed);

    // Test 2: Emergency paused = false, auxiliary allowed, executor != actor
    // Expected: Allow, ReasonCode::Allowed
    client.set_emergency_pause(&admin, &false);
    client.set_auxiliary_allowed(&admin, &auxiliary, &true);
    let decision = client.check_action(&actor, &auxiliary, &action, &current, &target, &false);
    assert_eq!(decision.decision, Decision::Allow);
    assert_eq!(decision.reason, ReasonCode::Allowed);

    // Test 3: Emergency paused = false, auxiliary not allowed, executor != actor
    // Expected: Deny, ReasonCode::AuxiliaryNotAllowed
    client.set_emergency_pause(&admin, &false);
    client.set_auxiliary_allowed(&admin, &auxiliary, &false);
    let decision = client.check_action(&actor, &auxiliary, &action, &current, &target, &false);
    assert_eq!(decision.decision, Decision::Deny);
    assert_eq!(decision.reason, ReasonCode::AuxiliaryNotAllowed);

    // Test 4: Emergency paused = true, executor == actor (direct call)
    // Expected: Deny, ReasonCode::EmergencyPaused
    client.set_emergency_pause(&admin, &true);
    let decision = client.check_action(&actor, &actor, &action, &current, &target, &false);
    assert_eq!(decision.decision, Decision::Deny);
    assert_eq!(decision.reason, ReasonCode::EmergencyPaused);

    // Test 5: Emergency paused = true, auxiliary allowed, executor != actor
    // Expected: Deny, ReasonCode::EmergencyPaused (emergency pause overrides)
    client.set_emergency_pause(&admin, &true);
    client.set_auxiliary_allowed(&admin, &auxiliary, &true);
    let decision = client.check_action(&actor, &auxiliary, &action, &current, &target, &false);
    assert_eq!(decision.decision, Decision::Deny);
    assert_eq!(decision.reason, ReasonCode::EmergencyPaused);

    // Test 6: Emergency paused = true, auxiliary not allowed, executor != actor
    // Expected: Deny, ReasonCode::EmergencyPaused (emergency pause overrides)
    client.set_emergency_pause(&admin, &true);
    client.set_auxiliary_allowed(&admin, &auxiliary, &false);
    let decision = client.check_action(&actor, &auxiliary, &action, &current, &target, &false);
    assert_eq!(decision.decision, Decision::Deny);
    assert_eq!(decision.reason, ReasonCode::EmergencyPaused);
}

/// Test that emergency pause overrides auxiliary allowlist across all actions.
///
/// This is a focused security test confirming the critical invariant:
/// emergency pause always denies, even for allowlisted auxiliary contracts.
#[test]
fn emergency_pause_overrides_auxiliary_allowlist() {
    let env = create_env();
    let (client, admin) = setup(&env);
    let actor = Address::generate(&env);
    let auxiliary = Address::generate(&env);

    // Allowlist the auxiliary contract
    client.set_auxiliary_allowed(&admin, &auxiliary, &true);

    // Enable emergency pause
    client.set_emergency_pause(&admin, &true);

    // Test all actions - all should be denied with EmergencyPaused reason
    for action in actions() {
        // Use a valid current state for each action
        let current = match action {
            PayrollAction::AddEmployee => AgreementStatus::Created,
            PayrollAction::ActivateAgreement => AgreementStatus::Created,
            PayrollAction::PauseAgreement => AgreementStatus::Active,
            PayrollAction::ResumeAgreement => AgreementStatus::Paused,
            PayrollAction::CancelAgreement => AgreementStatus::Created,
            PayrollAction::FinalizeGracePeriod => AgreementStatus::Cancelled,
            PayrollAction::RaiseDispute => AgreementStatus::Created,
            PayrollAction::ResolveDispute => AgreementStatus::Disputed,
            PayrollAction::ClaimPayroll
            | PayrollAction::ClaimTimeBased
            | PayrollAction::ClaimMilestone => AgreementStatus::Active,
        };

        let target = expected_target(action, current);

        // Call through allowlisted auxiliary while paused
        let decision = client.check_action(&actor, &auxiliary, &action, &current, &target, &false);

        assert_eq!(
            decision.decision,
            Decision::Deny,
            "Action {:?} should be denied when emergency paused",
            action
        );
        assert_eq!(
            decision.reason,
            ReasonCode::EmergencyPaused,
            "Action {:?} should have EmergencyPaused reason even through allowlisted auxiliary",
            action
        );
    }
}

/// Test that custom rule priorities change evaluation order.
///
/// With default priorities, EmergencyPause (priority 0) is evaluated before
/// TerminalState (priority 2). After promoting TerminalState to priority 0,
/// it should be evaluated first.
#[test]
fn test_set_rule_priority_changes_evaluation_order() {
    let env = create_env();
    let (client, admin) = setup(&env);
    let actor = Address::generate(&env);

    // Default order: EmergencyPause first, then TerminalState
    // Set EmergencyPause = false (Allow) and state = Completed (Deny)
    client.set_emergency_pause(&admin, &false);
    let decision = client.check_action(
        &actor,
        &actor,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Completed,
        &AgreementStatus::Completed,
        &false,
    );

    // With default priorities, EmergencyPause (0) at trace[0], TerminalState (2) at trace[1]
    assert_eq!(decision.traces.len(), 2);
    assert_eq!(decision.traces.get(0).unwrap().rule, TraceRule::EmergencyPause);
    assert_eq!(decision.traces.get(0).unwrap().result, Decision::Allow);
    assert_eq!(decision.traces.get(1).unwrap().rule, TraceRule::TerminalState);
    assert_eq!(decision.traces.get(1).unwrap().result, Decision::Deny);
    assert_eq!(decision.decision, Decision::Deny);
    assert_eq!(decision.reason, ReasonCode::TerminalState);

    // Promote TerminalState to priority 0 (highest)
    client.set_rule_priority(&admin, &TraceRule::TerminalState, &0);
    // Demote EmergencyPause to priority 10 (lower)
    client.set_rule_priority(&admin, &TraceRule::EmergencyPause, &10);

    // Now TerminalState should be evaluated before EmergencyPause
    let decision_reordered = client.check_action(
        &actor,
        &actor,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Completed,
        &AgreementStatus::Completed,
        &false,
    );

    assert_eq!(decision_reordered.traces.len(), 1);
    assert_eq!(
        decision_reordered.traces.get(0).unwrap().rule,
        TraceRule::TerminalState
    );
    assert_eq!(
        decision_reordered.traces.get(0).unwrap().result,
        Decision::Deny
    );
    assert_eq!(decision_reordered.decision, Decision::Deny);
    assert_eq!(decision_reordered.reason, ReasonCode::TerminalState);
}

/// Test that a higher-priority rule's Deny short-circuits lower-priority rules.
///
/// When AuxiliaryNotAllowed is promoted to priority 0 (higher than
/// EmergencyPause at its default 0, but with priority tie-breaking preserved),
/// it should be evaluated first. If it denies, EmergencyPause is never traced.
#[test]
fn test_higher_priority_rule_short_circuits_lower() {
    let env = create_env();
    let (client, admin) = setup(&env);
    let actor = Address::generate(&env);
    let auxiliary = Address::generate(&env);

    // Enable emergency pause and ensure auxiliary is NOT allowlisted
    client.set_emergency_pause(&admin, &true);
    client.set_auxiliary_allowed(&admin, &auxiliary, &false);

    // By default, EmergencyPause (0) evaluates first and short-circuits.
    let decision_default = client.check_action(
        &actor,
        &auxiliary,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(decision_default.traces.len(), 1);
    assert_eq!(
        decision_default.traces.get(0).unwrap().rule,
        TraceRule::EmergencyPause
    );
    assert_eq!(decision_default.decision, Decision::Deny);
    assert_eq!(decision_default.reason, ReasonCode::EmergencyPaused);

    // Promote AuxiliaryNotAllowed to priority 0 (highest) for the scenario
    // where both rules would deny.
    client.set_rule_priority(&admin, &TraceRule::AuxiliaryNotAllowed, &0);
    client.set_rule_priority(&admin, &TraceRule::EmergencyPause, &10);

    // Now AuxiliaryNotAllowed should evaluate first and short-circuit
    let decision_short = client.check_action(
        &actor,
        &auxiliary,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    assert_eq!(decision_short.traces.len(), 1);
    assert_eq!(
        decision_short.traces.get(0).unwrap().rule,
        TraceRule::AuxiliaryNotAllowed
    );
    assert_eq!(
        decision_short.traces.get(0).unwrap().result,
        Decision::Deny
    );
    assert_eq!(decision_short.decision, Decision::Deny);
    assert_eq!(decision_short.reason, ReasonCode::AuxiliaryNotAllowed);
}

/// Test that removing a custom priority restores the default ordering.
#[test]
fn test_remove_rule_priority_restores_default() {
    let env = create_env();
    let (client, admin) = setup(&env);

    // Verify default priority for TerminalState
    let default_priority = client.get_rule_priority(&TraceRule::TerminalState);
    assert_eq!(default_priority, 2);

    // Set custom priority
    client.set_rule_priority(&admin, &TraceRule::TerminalState, &0);
    let custom_priority = client.get_rule_priority(&TraceRule::TerminalState);
    assert_eq!(custom_priority, 0);

    // Remove the override
    client.remove_rule_priority(&admin, &TraceRule::TerminalState);
    let restored_priority = client.get_rule_priority(&TraceRule::TerminalState);
    assert_eq!(restored_priority, 2);
}

/// Test that non-admin callers cannot set rule priorities.
#[test]
fn test_set_rule_priority_rejects_non_admin() {
    let env = create_env();
    let (client, _admin) = setup(&env);
    let non_admin = Address::generate(&env);

    let result = client.try_set_rule_priority(
        &non_admin,
        &TraceRule::TerminalState,
        &0,
    );
    assert!(result.is_err(), "non-admin caller must be rejected");
}

/// Test that non-admin callers cannot remove rule priorities.
#[test]
fn test_remove_rule_priority_rejects_non_admin() {
    let env = create_env();
    let (client, _admin) = setup(&env);
    let non_admin = Address::generate(&env);

    let result = client.try_remove_rule_priority(
        &non_admin,
        &TraceRule::TerminalState,
    );
    assert!(result.is_err(), "non-admin caller must be rejected");
}
