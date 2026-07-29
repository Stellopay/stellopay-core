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
    /// Presence of this key means a registered rule blocks `PayrollAction`.
    /// Absence means no such rule is registered. Removal deletes the key
    /// outright rather than writing `false`, so a removed rule leaves nothing
    /// behind that a later read could mistake for an active rule.
    BlockedAction(PayrollAction),
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
    ActionBlocked,
}

/// Canonical identifiers for each evaluated rule in the compliance engine.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceRule {
    EmergencyPause,
    AuxiliaryNotAllowed,
    TerminalState,
    InvalidCurrentState,
    InvalidTargetState,
    GracePeriodRequired,
    ActionBlocked,
}

/// Trace entry for a single rule evaluation.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TraceEntry {
    pub rule: TraceRule,
    pub result: Decision,
    /// Denial reason that caused this rule to decide `Deny`, or
    /// `ReasonCode::Allowed` for allowed path evaluations.
    ///
    /// Note: this is a plain `ReasonCode` rather than `Option<ReasonCode>`
    /// because Soroban's `#[contracttype]` codegen (under the `testutils`
    /// feature) cannot derive an `ScVal` conversion for `Option<T>` where `T`
    /// is a user-defined enum/struct — only primitive-wrapped `Option<T>` is
    /// supported. `ReasonCode::Allowed` is used as the "no denial" sentinel.
    pub reason: ReasonCode,
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

    /// @notice Enables or disables emergency pause. Only the admin may call this.
    /// @dev Writes the pause flag to persistent storage. The new state is read
    ///      synchronously by `check_action` on every invocation, so it takes
    ///      effect immediately with no stale-read window. Non-admin callers are
    ///      rejected by the `require_admin` guard.
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
    /// @dev Returns false by default if the auxiliary has not been explicitly allowed.
    pub fn is_auxiliary_allowed(env: Env, auxiliary: Address) -> bool {
        env.storage()
            .persistent()
            .get(&StorageKey::AuxiliaryAllowed(auxiliary))
            .unwrap_or(false)
    }

    /// @notice Returns whether emergency pause is currently active.
    /// @dev Read-only, permissionless view intended for other contracts
    ///      (e.g. `payment_scheduler`) that want to respect this contract's
    ///      emergency pause as a shared halt signal without depending on the
    ///      heavier `check_action` rules-engine call. Mirrors the same
    ///      `StorageKey::EmergencyPause` flag read internally by
    ///      `check_action`, so the two never disagree.
    ///
    ///      Returns `false` before `initialize` has been called — there is no
    ///      privileged state to protect yet, so absence of initialization is
    ///      not itself treated as a paused signal.
    pub fn is_emergency_paused(env: Env) -> bool {
        env.storage()
            .persistent()
            .get::<_, bool>(&StorageKey::EmergencyPause)
            .unwrap_or(false)
    }

    /// @notice Validates a payroll action transition.
    /// @dev Rule precedence (highest -> lowest):
    ///      1. Emergency pause deny.
    ///      2. Registered action-blocking rule deny (when a rule is registered
    ///         for `action`).
    ///      3. Auxiliary allowlist deny (when `executor != actor`).
    ///      4. Terminal-state deny.
    ///      5. Action/current-state compatibility deny.
    ///      6. Target-state compatibility deny.
    ///      7. Grace-period requirement deny for cancelled claims.
    ///      8. Allow.
    ///
    ///      Every rule reads its inputs from persistent storage at evaluation
    ///      time; no rule state is cached across invocations or snapshotted at
    ///      transaction start. Registering or removing a rule therefore changes
    ///      the outcome of the very next evaluation.
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
        let sorted_rules = Self::get_sorted_rule_ids(&env);

        for rule in sorted_rules.iter() {
            let entry = match rule {
                TraceRule::EmergencyPause => Self::evaluate_emergency_pause(&env),
                TraceRule::AuxiliaryNotAllowed => {
                    Self::evaluate_auxiliary_not_allowed(&env, &actor, &executor)
                }
                TraceRule::TerminalState => Self::evaluate_terminal_state(&current_state),
                TraceRule::InvalidCurrentState => {
                    Self::evaluate_invalid_current_state(&action, &current_state)
                }
                TraceRule::InvalidTargetState => {
                    Self::evaluate_invalid_target_state(&action, &current_state, &target_state)
                }
                TraceRule::GracePeriodRequired => {
                    Self::evaluate_grace_period_required(&action, &current_state, grace_period_active)
                }
            };

            if let Some(entry) = entry {
                let is_deny = entry.result == Decision::Deny;
                let reason = entry.reason;
                traces.push_back(entry);
                if is_deny {
                    return Self::make_decision(Decision::Deny, reason, traces);
                }
            }
        }

        Self::make_decision(Decision::Allow, ReasonCode::Allowed, traces)
    }

    // -------------------------------------------------------------------------
    // Rule evaluation functions
    // -------------------------------------------------------------------------

    fn evaluate_emergency_pause(env: &Env) -> Option<TraceEntry> {
        let is_paused = env
            .storage()
            .persistent()
            .get::<_, bool>(&StorageKey::EmergencyPause)
            .unwrap_or(false);

        Some(TraceEntry {
            rule: TraceRule::EmergencyPause,
            result: if is_paused {
                Decision::Deny
            } else {
                Decision::Allow
            },
            reason: if is_paused {
                ReasonCode::EmergencyPaused
            } else {
                ReasonCode::Allowed
            },
        })
    }

    fn evaluate_auxiliary_not_allowed(
        env: &Env,
        actor: &Address,
        executor: &Address,
    ) -> Option<TraceEntry> {
        if executor == actor {
            return None;
        }

        // 2. Registered action-blocking rule check.
        //
        // Read fresh from persistent storage on every evaluation. A trace entry
        // is emitted only when a rule is actually registered for this action —
        // consistent with the auxiliary and grace-period rules, which likewise
        // trace only when they apply. Once `remove_rule` deletes the entry this
        // read returns false and the rule stops contributing to the decision.
        let is_blocked = env
            .storage()
            .persistent()
            .get::<_, bool>(&StorageKey::BlockedAction(action))
            .unwrap_or(false);

        if is_blocked {
            traces.push_back(TraceEntry {
                rule: TraceRule::ActionBlocked,
                result: Decision::Deny,
                reason: ReasonCode::ActionBlocked,
            });
            return Self::make_decision(Decision::Deny, ReasonCode::ActionBlocked, traces);
        }

        // 3. Auxiliary Not Allowed check
        if executor != actor {
            let is_allowed = Self::is_auxiliary_allowed(env.clone(), executor);
            let aux_result = if is_allowed {
                Decision::Allow
            } else {
                Decision::Deny
            },
            reason: if is_allowed {
                ReasonCode::Allowed
            } else {
                ReasonCode::AuxiliaryNotAllowed
            },
        })
    }

        // 4. Terminal State check
        let is_terminal = current_state == AgreementStatus::Completed;
        let terminal_result = if is_terminal {
            Decision::Deny
        } else {
            Decision::Allow
        };
        traces.push_back(TraceEntry {
            rule: TraceRule::TerminalState,
            result: if is_terminal {
                Decision::Deny
            } else {
                Decision::Allow
            },
            reason: if is_terminal {
                ReasonCode::TerminalState
            } else {
                ReasonCode::Allowed
            },
        })
    }

        // 5. Invalid Current State check
        let is_current_valid = Self::is_action_allowed_from_state(action, current_state);
        let current_valid_result = if is_current_valid {
            Decision::Allow
        } else {
            Decision::Deny
        };
        traces.push_back(TraceEntry {
            rule: TraceRule::InvalidCurrentState,
            result: if is_valid {
                Decision::Allow
            } else {
                Decision::Deny
            },
            reason: if is_valid {
                ReasonCode::Allowed
            } else {
                ReasonCode::InvalidCurrentState
            },
        })
    }

        // 6. Invalid Target State check
        let expected_target = Self::expected_target_state(action, current_state);
        let is_target_valid = target_state == expected_target;
        let target_valid_result = if is_target_valid {
            Decision::Allow
        } else {
            Decision::Deny
        };
        traces.push_back(TraceEntry {
            rule: TraceRule::InvalidTargetState,
            result: if is_valid {
                Decision::Allow
            } else {
                Decision::Deny
            },
            reason: if is_valid {
                ReasonCode::Allowed
            } else {
                ReasonCode::InvalidTargetState
            },
        })
    }

    fn evaluate_grace_period_required(
        action: &PayrollAction,
        current_state: &AgreementStatus,
        grace_period_active: bool,
    ) -> Option<TraceEntry> {
        let is_claim_action = *action == PayrollAction::ClaimPayroll
            || *action == PayrollAction::ClaimTimeBased
            || *action == PayrollAction::ClaimMilestone;

        if !is_claim_action || *current_state != AgreementStatus::Cancelled {
            return None;
        }

        // 7. Grace Period Required check
        let is_claim_action = action == PayrollAction::ClaimPayroll
            || action == PayrollAction::ClaimTimeBased
            || action == PayrollAction::ClaimMilestone;
        if is_claim_action && current_state == AgreementStatus::Cancelled {
            let grace_result = if grace_period_active {
                Decision::Allow
            } else {
                Decision::Deny
            },
            reason: if grace_period_active {
                ReasonCode::Allowed
            } else {
                ReasonCode::GracePeriodRequired
            },
        })
    }

    // -------------------------------------------------------------------------
    // Priority helpers
    // -------------------------------------------------------------------------

    fn default_rule_priority(rule: &TraceRule) -> u32 {
        match rule {
            TraceRule::EmergencyPause => 0,
            TraceRule::AuxiliaryNotAllowed => 1,
            TraceRule::TerminalState => 2,
            TraceRule::InvalidCurrentState => 3,
            TraceRule::InvalidTargetState => 4,
            TraceRule::GracePeriodRequired => 5,
        }
    }

    fn effective_rule_priority(env: &Env, rule: &TraceRule) -> u32 {
        env.storage()
            .persistent()
            .get::<_, u32>(&StorageKey::RulePriority(*rule))
            .unwrap_or_else(|| Self::default_rule_priority(rule))
    }

    /// Returns rule IDs sorted by effective priority (ascending). Uses a
    /// stable insertion sort so equal-priority rules keep their default order.
    fn get_sorted_rule_ids(env: &Env) -> soroban_sdk::Vec<TraceRule> {
        let mut sorted = [
            (
                Self::effective_rule_priority(env, &TraceRule::EmergencyPause),
                TraceRule::EmergencyPause,
            ),
            (
                Self::effective_rule_priority(env, &TraceRule::AuxiliaryNotAllowed),
                TraceRule::AuxiliaryNotAllowed,
            ),
            (
                Self::effective_rule_priority(env, &TraceRule::TerminalState),
                TraceRule::TerminalState,
            ),
            (
                Self::effective_rule_priority(env, &TraceRule::InvalidCurrentState),
                TraceRule::InvalidCurrentState,
            ),
            (
                Self::effective_rule_priority(env, &TraceRule::InvalidTargetState),
                TraceRule::InvalidTargetState,
            ),
            (
                Self::effective_rule_priority(env, &TraceRule::GracePeriodRequired),
                TraceRule::GracePeriodRequired,
            ),
        ];

        // Stable insertion sort
        let mut i: usize = 1;
        while i < 6 {
            let mut j = i;
            while j > 0 && sorted[j - 1].0 > sorted[j].0 {
                sorted.swap(j - 1, j);
                j -= 1;
            }
            i += 1;
        }

        let mut result: soroban_sdk::Vec<TraceRule> = soroban_sdk::Vec::new(env);
        for (_, rule) in sorted {
            result.push_back(rule);
        }
        result
    }

    fn make_decision(
        decision: Decision,
        reason: ReasonCode,
        traces: soroban_sdk::Vec<TraceEntry>,
    ) -> ComplianceDecision {
        ComplianceDecision {
            decision,
            reason,
            traces,
        }
    }

    fn expected_target_state(
        action: PayrollAction,
        current_state: AgreementStatus,
    ) -> AgreementStatus {
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
                current_state == AgreementStatus::Created
                    || current_state == AgreementStatus::Active
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
                current_state == AgreementStatus::Active
                    || current_state == AgreementStatus::Cancelled
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

    #[allow(dead_code)]
    fn allow(env: &Env) -> ComplianceDecision {
        ComplianceDecision {
            decision: Decision::Allow,
            reason: ReasonCode::Allowed,
            traces: soroban_sdk::Vec::new(env),
        }
    }

    #[allow(dead_code)]
    fn deny(reason: ReasonCode, traces: soroban_sdk::Vec<TraceEntry>) -> ComplianceDecision {
        ComplianceDecision {
            decision: Decision::Deny,
            reason,
            traces,
        }
    }
}
