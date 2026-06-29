//! Exhaustive negative matrix for invalid payroll transitions.

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

fn setup(env: &Env) -> ComplianceCheckerContractClient<'_> {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, ComplianceCheckerContract);
    let client = ComplianceCheckerContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    client
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
    let client = setup(&env);
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
    let client = setup(&env);
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
