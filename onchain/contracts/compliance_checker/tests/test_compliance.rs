//! Compliance checker rules-engine tests.

#![cfg(test)]
#![allow(deprecated)]

use compliance_checker::{
    AgreementStatus, ComplianceCheckerContract, ComplianceCheckerContractClient, Decision,
    PayrollAction, ReasonCode,
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
        (PayrollAction::AddEmployee, AgreementStatus::Active, AgreementStatus::Created),
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
        let decision =
            client.check_action(&actor, &actor, &action, &current, &bad_target, &false);
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
