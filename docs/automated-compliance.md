# Automated Compliance Checks

This document describes the automated compliance checker contract (issue #233): configurable rules, automatic checks, reporting, and alerts.

## Overview

The `compliance_checker` contract provides:

- **Configurable rules** – Threshold and presence rules keyed by symbolic attributes.
- **Automated checks** – Evaluate a subject's attributes against all active rules.
- **Compliance reporting** – Structured reports with pass/fail and violation details.
- **Violation alerts** – Contract events emitted for each violation.

## Contract Location

- **Contract**: `onchain/contracts/compliance_checker/src/lib.rs`
- **Tests**: `onchain/contracts/compliance_checker/tests/test_compliance.rs`

## Data Model

### RuleKind

```rust
pub enum RuleKind {
    MaxValue { key: Symbol, max: i128 },
    MinValue { key: Symbol, min: i128 },
    RequiredFlag { key: Symbol },
}
```

### Rule

```rust
pub struct Rule {
    pub id: u32,
    pub active: bool,
    pub kind: RuleKind,
    pub description: Symbol,
    pub severity: u8,
}
```

### ComplianceViolation & ComplianceReport

```rust
pub struct ComplianceViolation {
    pub rule_id: u32,
    pub message: Symbol,
    pub severity: u8,
}

pub struct ComplianceReport {
    pub subject_id: u128,
    pub passed: bool,
    pub violations: Vec<ComplianceViolation>,
}
```

## API

### Initialization

- `initialize(admin)` – One-time initialization. Sets the admin that manages rules.

### Rule Management

- `add_rule(caller, kind, description, severity) -> rule_id` – Adds a new active rule. `caller` must be admin.
- `set_rule_active(caller, rule_id, active)` – Activates or deactivates a rule.
- `get_rule(rule_id) -> Rule` – Returns a single rule.
- `list_rules() -> Vec<Rule>` – Returns all rules in insertion order.

### Compliance Checking

- `check_compliance(subject_id, attributes) -> ComplianceReport`
  - `attributes` is a `Vec<(Symbol, i128)>` of key/value pairs.
  - Evaluates all active rules and returns a report with violations.
  - Stores the last report per `subject_id` in contract storage.
- `get_last_report(subject_id) -> Option<ComplianceReport>` – Returns the stored report, if any.

### Events

- `ComplianceViolationEvent { subject_id, rule_id, severity }` – Emitted for each violation.

## Usage Pattern

1. Admin deploys and calls `initialize(admin)`.
2. Admin adds rules, for example:
   - `MaxValue { key: "amount", max: 1_000 }`
   - `RequiredFlag { key: "kyc" }`
3. A caller collects attributes for a subject (e.g., `amount`, `kyc_flag`) and calls
   `check_compliance(subject_id, attributes)`.
4. The caller inspects the returned `ComplianceReport` and/or subscribes to
   `ComplianceViolationEvent` logs for real-time alerts.

## Security Considerations

- Only the admin can add or modify rules; this is enforced via `require_auth` and stored admin address.
- The contract does not perform token transfers; it only evaluates attributes and stores reports.
- Severity values are advisory and can be mapped by off-chain systems to alerting or escalation policies.
