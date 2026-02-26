#![no_std]

//! Automated Compliance Checker Contract (#233)
//!
//! Provides configurable compliance rules, automatic checks, reporting, and
//! violation alerts for on-chain business logic.

use soroban_sdk::{
    contract, contractevent, contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec,
};

#[contract]
pub struct ComplianceCheckerContract;

/// Storage keys for the compliance checker
#[contracttype]
#[derive(Clone)]
enum StorageKey {
    Initialized,
    Admin,
    NextRuleId,
    /// Rule definition: rule_id -> Rule
    Rule(u32),
    /// Ordered list of rule IDs
    RulesIndex,
    /// Last compliance report for a subject: subject_id -> ComplianceReport
    LastReport(u128),
}

/// Kinds of compliance rules supported by the contract.
///
/// Note: Soroban contract types require tuple-style payloads.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuleKind {
    /// Attribute must not exceed a maximum value, e.g. `amount <= max`.
    MaxValue(Symbol, i128),
    /// Attribute must be at least a minimum value, e.g. `amount >= min`.
    MinValue(Symbol, i128),
    /// Attribute must be present and non-zero, e.g. a required flag.
    RequiredFlag(Symbol),
}

/// A single compliance rule.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rule {
    pub id: u32,
    pub active: bool,
    pub kind: RuleKind,
    pub description: Symbol,
    pub severity: u32,
}

/// A single violation entry in a compliance report.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComplianceViolation {
    pub rule_id: u32,
    pub message: Symbol,
    pub severity: u32,
}

/// Result of a compliance check for a subject.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComplianceReport {
    pub subject_id: u128,
    pub passed: bool,
    pub violations: Vec<ComplianceViolation>,
}

/// Emitted when a compliance violation is detected.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComplianceViolationEvent {
    pub subject_id: u128,
    pub rule_id: u32,
    pub severity: u32,
}

#[contractimpl]
impl ComplianceCheckerContract {
    /// Initializes the contract.
    ///
    /// # Arguments
    /// * `admin` - Address that will manage rules (must authenticate).
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
        env.storage()
            .persistent()
            .set(&StorageKey::Admin, &admin);
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);
        env.storage()
            .persistent()
            .set(&StorageKey::NextRuleId, &1u32);
        let empty: Vec<u32> = Vec::new(&env);
        env.storage()
            .persistent()
            .set(&StorageKey::RulesIndex, &empty);
    }

    /// Adds a new compliance rule.
    ///
    /// # Arguments
    /// * `caller` - Must be the admin (must authenticate).
    /// * `kind` - Rule kind.
    /// * `description` - Short description of the rule.
    /// * `severity` - Severity level.
    pub fn add_rule(
        env: Env,
        caller: Address,
        kind: RuleKind,
        description: Symbol,
        severity: u32,
    ) -> u32 {
        Self::require_initialized(&env);
        Self::require_admin(&env, &caller);

        let next_id: u32 = env
            .storage()
            .persistent()
            .get(&StorageKey::NextRuleId)
            .unwrap_or(1);
        let new_next = next_id
            .checked_add(1)
            .expect("Rule id overflow");
        env.storage()
            .persistent()
            .set(&StorageKey::NextRuleId, &new_next);

        let rule = Rule {
            id: next_id,
            active: true,
            kind,
            description,
            severity,
        };
        env.storage()
            .persistent()
            .set(&StorageKey::Rule(next_id), &rule);

        let mut index: Vec<u32> = env
            .storage()
            .persistent()
            .get(&StorageKey::RulesIndex)
            .unwrap_or_else(|| Vec::new(&env));
        index.push_back(next_id);
        env.storage()
            .persistent()
            .set(&StorageKey::RulesIndex, &index);

        next_id
    }

    /// Activates or deactivates an existing rule.
    pub fn set_rule_active(env: Env, caller: Address, rule_id: u32, active: bool) {
        Self::require_initialized(&env);
        Self::require_admin(&env, &caller);
        let mut rule: Rule = env
            .storage()
            .persistent()
            .get(&StorageKey::Rule(rule_id))
            .expect("Rule not found");
        rule.active = active;
        env.storage()
            .persistent()
            .set(&StorageKey::Rule(rule_id), &rule);
    }

    /// Returns a single rule.
    pub fn get_rule(env: Env, rule_id: u32) -> Rule {
        env.storage()
            .persistent()
            .get(&StorageKey::Rule(rule_id))
            .expect("Rule not found")
    }

    /// Lists all rules in insertion order.
    pub fn list_rules(env: Env) -> Vec<Rule> {
        let index: Vec<u32> = env
            .storage()
            .persistent()
            .get(&StorageKey::RulesIndex)
            .unwrap_or_else(|| Vec::new(&env));
        let mut out: Vec<Rule> = Vec::new(&env);
        let mut i = 0u32;
        while i < index.len() {
            let id = index.get(i).unwrap();
            if let Some(rule) = env
                .storage()
                .persistent()
                .get::<_, Rule>(&StorageKey::Rule(id))
            {
                out.push_back(rule);
            }
            i += 1;
        }
        out
    }

    /// Checks compliance for a subject with a set of key/value attributes.
    ///
    /// Attributes are represented as `(key, value)` pairs, where the
    /// interpretation of `value` depends on the specific `RuleKind`.
    pub fn check_compliance(
        env: Env,
        subject_id: u128,
        attributes: Vec<(Symbol, i128)>,
    ) -> ComplianceReport {
        Self::require_initialized(&env);
        let rules = Self::list_rules(env.clone());
        let mut violations: Vec<ComplianceViolation> = Vec::new(&env);

        let mut i = 0u32;
        while i < rules.len() {
            let rule = rules.get(i).unwrap();
            if rule.active {
                if let Some(v) = Self::evaluate_rule(&env, &rule, subject_id, &attributes) {
                    violations.push_back(v);
                }
            }
            i += 1;
        }

        let passed = violations.len() == 0;
        let report = ComplianceReport {
            subject_id,
            passed,
            violations,
        };
        env.storage()
            .persistent()
            .set(&StorageKey::LastReport(subject_id), &report);
        report
    }

    /// Returns the last stored report for a subject, if any.
    pub fn get_last_report(env: Env, subject_id: u128) -> Option<ComplianceReport> {
        env.storage()
            .persistent()
            .get(&StorageKey::LastReport(subject_id))
    }

    fn evaluate_rule(
        env: &Env,
        rule: &Rule,
        subject_id: u128,
        attributes: &Vec<(Symbol, i128)>,
    ) -> Option<ComplianceViolation> {
        use RuleKind::*;

        match &rule.kind {
            MaxValue(key, max) => {
                if let Some(value) = Self::get_attr(attributes, key) {
                    if value > *max {
                        let msg = symbol_short!("max_vio");
                        ComplianceViolationEvent {
                            subject_id,
                            rule_id: rule.id,
                            severity: rule.severity,
                        }
                        .publish(env);
                        return Some(ComplianceViolation {
                            rule_id: rule.id,
                            message: msg,
                            severity: rule.severity,
                        });
                    }
                }
            }
            MinValue(key, min) => {
                if let Some(value) = Self::get_attr(attributes, key) {
                    if value < *min {
                        let msg = symbol_short!("min_vio");
                        ComplianceViolationEvent {
                            subject_id,
                            rule_id: rule.id,
                            severity: rule.severity,
                        }
                        .publish(env);
                        return Some(ComplianceViolation {
                            rule_id: rule.id,
                            message: msg,
                            severity: rule.severity,
                        });
                    }
                }
            }
            RequiredFlag(key) => {
                if let Some(value) = Self::get_attr(attributes, key) {
                    if value == 0 {
                        let msg = symbol_short!("flag_mis");
                        ComplianceViolationEvent {
                            subject_id,
                            rule_id: rule.id,
                            severity: rule.severity,
                        }
                        .publish(env);
                        return Some(ComplianceViolation {
                            rule_id: rule.id,
                            message: msg,
                            severity: rule.severity,
                        });
                    }
                } else {
                    let msg = symbol_short!("flag_mis");
                    ComplianceViolationEvent {
                        subject_id,
                        rule_id: rule.id,
                        severity: rule.severity,
                    }
                    .publish(env);
                    return Some(ComplianceViolation {
                        rule_id: rule.id,
                        message: msg,
                        severity: rule.severity,
                    });
                }
            }
        }
        None
    }

    fn get_attr(attrs: &Vec<(Symbol, i128)>, key: &Symbol) -> Option<i128> {
        let mut i = 0u32;
        while i < attrs.len() {
            let pair = attrs.get(i).unwrap();
            if &pair.0 == key {
                return Some(pair.1);
            }
            i += 1;
        }
        None
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
}
