#![no_std]

//! Payroll compliance transition rules engine.
//!
//! This contract encodes allow/deny checks for payroll lifecycle actions and
//! emits deterministic reason codes for each decision.

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

#[contract]
pub struct ComplianceCheckerContract;

#[contracttype]
#[derive(Clone)]
enum StorageKey {
    Initialized,
    Admin,
    EmergencyPause,
    AuxiliaryAllowed(Address),
}

/// Payroll agreement lifecycle statuses mirrored from main payroll flows.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgreementStatus {
    Created,
    Active,
    Paused,
    Cancelled,
    Completed,
    Disputed,
}

/// Validated payroll actions.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PayrollAction {
    AddEmployee,
    ActivateAgreement,
    PauseAgreement,
    ResumeAgreement,
    CancelAgreement,
    FinalizeGracePeriod,
    RaiseDispute,
    ResolveDispute,
    ClaimPayroll,
    ClaimTimeBased,
    ClaimMilestone,
}

/// Binary decision for a compliance check.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decision {
    Allow,
    Deny,
}

/// Deterministic reason codes returned by the rules engine.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReasonCode {
    Allowed,
    AuxiliaryNotAllowed,
    EmergencyPaused,
    TerminalState,
    InvalidCurrentState,
    InvalidTargetState,
    GracePeriodRequired,
}

/// Trace entry for a single rule evaluation.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TraceEntry {
    pub rule: ReasonCode,
    pub result: Decision,
}

/// Result payload returned by rule evaluation.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComplianceDecision {
    pub decision: Decision,
    pub reason: ReasonCode,
    pub traces: soroban_sdk::Vec<TraceEntry>,
}

#[contractimpl]
impl ComplianceCheckerContract {
    /// @notice Initializes the compliance checker.
    /// @dev One-time setup. `admin` is the only principal allowed to mutate
    ///      security settings (pause state and auxiliary allowlist).
    pub fn initialize(env: Env, admin: Address) {
        if env
            .storage()
            .persistent()
            .get::<_, bool>(&StorageKey::Initialized)
            .unwrap_or(false)
        {
            panic!("Already initialized");
        }

        admin.require_auth();
        env.storage().persistent().set(&StorageKey::Admin, &admin);
        env.storage()
            .persistent()
            .set(&StorageKey::EmergencyPause, &false);
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);
    }

    /// @notice Enables or disables emergency pause checks.
    /// @dev Highest precedence deny: when paused, all payroll action checks
    ///      return `Deny/EmergencyPaused`.
    pub fn set_emergency_pause(env: Env, caller: Address, is_paused: bool) {
        Self::require_initialized(&env);
        Self::require_admin(&env, &caller);
        env.storage()
            .persistent()
            .set(&StorageKey::EmergencyPause, &is_paused);
    }

    /// @notice Allowlists or removes an auxiliary contract.
    /// @dev Auxiliary callers are denied by default and must be explicitly
    ///      enabled. This protects against indirect bypass by helper contracts.
    pub fn set_auxiliary_allowed(env: Env, caller: Address, auxiliary: Address, allowed: bool) {
        Self::require_initialized(&env);
        Self::require_admin(&env, &caller);
        env.storage()
            .persistent()
            .set(&StorageKey::AuxiliaryAllowed(auxiliary), &allowed);
    }

    /// @notice Returns whether an auxiliary contract is explicitly allowlisted.
    pub fn is_auxiliary_allowed(env: Env, auxiliary: Address) -> bool {
        env.storage()
            .persistent()
            .get(&StorageKey::AuxiliaryAllowed(auxiliary))
            .unwrap_or(false)
    }

    /// @notice Validates a payroll action transition.
    /// @dev Rule precedence (highest -> lowest):
    ///      1. Emergency pause deny.
    ///      2. Auxiliary allowlist deny (when `executor != actor`).
    ///      3. Terminal-state deny.
    ///      4. Action/current-state compatibility deny.
    ///      5. Target-state compatibility deny.
    ///      6. Grace-period requirement deny for cancelled claims.
    ///      7. Allow.
    ///
    ///      Security assumption: callers must pass the real execution context:
    ///      `actor` is the principal authorizing the action, and `executor` is
    ///      the immediate executor. If `executor != actor`, executor is treated
    ///      as an auxiliary contract and must be allowlisted.
    pub fn check_action(
        env: Env,
        actor: Address,
        executor: Address,
        action: PayrollAction,
        current_state: AgreementStatus,
        target_state: AgreementStatus,
        grace_period_active: bool,
    ) -> ComplianceDecision {
        Self::require_initialized(&env);

        actor.require_auth();
        if executor != actor {
            executor.require_auth();
        }

        let mut traces = soroban_sdk::Vec::new(&env);

        // 1. Emergency Pause check
        let is_paused = env
            .storage()
            .persistent()
            .get::<_, bool>(&StorageKey::EmergencyPause)
            .unwrap_or(false);
        
        let pause_result = if is_paused { Decision::Deny } else { Decision::Allow };
        traces.push_back(TraceEntry { rule: ReasonCode::EmergencyPaused, result: pause_result });
        
        if is_paused {
            return Self::make_decision(Decision::Deny, ReasonCode::EmergencyPaused, traces);
        }

        // 2. Auxiliary Not Allowed check
        if executor != actor {
            let is_allowed = Self::is_auxiliary_allowed(env.clone(), executor);
            let aux_result = if is_allowed { Decision::Allow } else { Decision::Deny };
            traces.push_back(TraceEntry { rule: ReasonCode::AuxiliaryNotAllowed, result: aux_result });
            if !is_allowed {
                return Self::make_decision(Decision::Deny, ReasonCode::AuxiliaryNotAllowed, traces);
            }
        }

        // 3. Terminal State check
        let is_terminal = current_state == AgreementStatus::Completed;
        let terminal_result = if is_terminal { Decision::Deny } else { Decision::Allow };
        traces.push_back(TraceEntry { rule: ReasonCode::TerminalState, result: terminal_result });
        if is_terminal {
            return Self::make_decision(Decision::Deny, ReasonCode::TerminalState, traces);
        }

        // 4. Invalid Current State check
        let is_current_valid = Self::is_action_allowed_from_state(action, current_state);
        let current_valid_result = if is_current_valid { Decision::Allow } else { Decision::Deny };
        traces.push_back(TraceEntry { rule: ReasonCode::InvalidCurrentState, result: current_valid_result });
        if !is_current_valid {
            return Self::make_decision(Decision::Deny, ReasonCode::InvalidCurrentState, traces);
        }

        // 5. Invalid Target State check
        let expected_target = Self::expected_target_state(action, current_state);
        let is_target_valid = target_state == expected_target;
        let target_valid_result = if is_target_valid { Decision::Allow } else { Decision::Deny };
        traces.push_back(TraceEntry { rule: ReasonCode::InvalidTargetState, result: target_valid_result });
        if !is_target_valid {
            return Self::make_decision(Decision::Deny, ReasonCode::InvalidTargetState, traces);
        }

        // 6. Grace Period Required check
        let is_claim_action = action == PayrollAction::ClaimPayroll
            || action == PayrollAction::ClaimTimeBased
            || action == PayrollAction::ClaimMilestone;
        if is_claim_action && current_state == AgreementStatus::Cancelled {
            let grace_result = if grace_period_active { Decision::Allow } else { Decision::Deny };
            traces.push_back(TraceEntry { rule: ReasonCode::GracePeriodRequired, result: grace_result });
            if !grace_period_active {
                return Self::make_decision(Decision::Deny, ReasonCode::GracePeriodRequired, traces);
            }
        }

        Self::make_decision(Decision::Allow, ReasonCode::Allowed, traces)
    }

    fn make_decision(decision: Decision, reason: ReasonCode, traces: soroban_sdk::Vec<TraceEntry>) -> ComplianceDecision {
        ComplianceDecision {
            decision,
            reason,
            traces,
        }
    }

    fn expected_target_state(action: PayrollAction, current_state: AgreementStatus) -> AgreementStatus {
        match action {
            PayrollAction::AddEmployee => AgreementStatus::Created,
            PayrollAction::ActivateAgreement => AgreementStatus::Active,
            PayrollAction::PauseAgreement => AgreementStatus::Paused,
            PayrollAction::ResumeAgreement => AgreementStatus::Active,
            PayrollAction::CancelAgreement => AgreementStatus::Cancelled,
            PayrollAction::FinalizeGracePeriod => AgreementStatus::Cancelled,
            PayrollAction::RaiseDispute => AgreementStatus::Disputed,
            PayrollAction::ResolveDispute => AgreementStatus::Completed,
            PayrollAction::ClaimPayroll => current_state,
            PayrollAction::ClaimTimeBased => current_state,
            PayrollAction::ClaimMilestone => current_state,
        }
    }

    fn is_action_allowed_from_state(action: PayrollAction, current_state: AgreementStatus) -> bool {
        match action {
            PayrollAction::AddEmployee => current_state == AgreementStatus::Created,
            PayrollAction::ActivateAgreement => current_state == AgreementStatus::Created,
            PayrollAction::PauseAgreement => current_state == AgreementStatus::Active,
            PayrollAction::ResumeAgreement => current_state == AgreementStatus::Paused,
            PayrollAction::CancelAgreement => {
                current_state == AgreementStatus::Created || current_state == AgreementStatus::Active
            }
            PayrollAction::FinalizeGracePeriod => current_state == AgreementStatus::Cancelled,
            PayrollAction::RaiseDispute => {
                current_state == AgreementStatus::Created
                    || current_state == AgreementStatus::Active
                    || current_state == AgreementStatus::Cancelled
            }
            PayrollAction::ResolveDispute => current_state == AgreementStatus::Disputed,
            PayrollAction::ClaimPayroll
            | PayrollAction::ClaimTimeBased
            | PayrollAction::ClaimMilestone => {
                current_state == AgreementStatus::Active || current_state == AgreementStatus::Cancelled
            }
        }
    }

    fn require_initialized(env: &Env) {
        let initialized: bool = env
            .storage()
            .persistent()
            .get(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(initialized, "Contract not initialized");
    }

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::Admin)
            .expect("Admin not set");
        caller.require_auth();
        assert!(*caller == admin, "Not admin");
    }

    fn allow(env: &Env) -> ComplianceDecision {
        ComplianceDecision {
            decision: Decision::Allow,
            reason: ReasonCode::Allowed,
            traces: soroban_sdk::Vec::new(env),
        }
    }

    fn deny(reason: ReasonCode, traces: soroban_sdk::Vec<TraceEntry>) -> ComplianceDecision {
        ComplianceDecision {
            decision: Decision::Deny,
            reason,
            traces,
        }
    }
}
