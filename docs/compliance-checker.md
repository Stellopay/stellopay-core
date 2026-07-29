# Compliance Checker Rules Engine

This document describes the payroll compliance rules engine implemented in `onchain/contracts/compliance_checker/src/lib.rs`.

## Purpose

The contract validates payroll lifecycle actions as deterministic allow/deny decisions with explicit reason codes, so invalid state transitions cannot silently pass.

## Contract Scope

- Validates payroll lifecycle transitions only.
- Returns structured decisions (`Allow` or `Deny`) and deterministic `ReasonCode`.
- Applies auxiliary-caller restrictions to prevent indirect bypass through non-allowlisted helper contracts.

## Core Types

- `AgreementStatus`: `Created`, `Active`, `Paused`, `Cancelled`, `Completed`, `Disputed`.
- `PayrollAction`:
  - `AddEmployee`
  - `ActivateAgreement`
  - `PauseAgreement`
  - `ResumeAgreement`
  - `CancelAgreement`
  - `FinalizeGracePeriod`
  - `RaiseDispute`
  - `ResolveDispute`
  - `ClaimPayroll`
  - `ClaimTimeBased`
  - `ClaimMilestone`
- `ComplianceDecision`:
  - `decision`: `Allow` or `Deny`
  - `reason`: `ReasonCode`

## Rule Precedence (NatSpec-style)

The `check_action` entrypoint uses the following precedence from highest to lowest:

1. `EmergencyPaused` deny.
2. `AuxiliaryNotAllowed` deny when `executor != actor` and executor is not allowlisted.
3. `TerminalState` deny when current status is `Completed`.
4. `InvalidCurrentState` deny when action is not legal from current state.
5. `InvalidTargetState` deny when requested target does not match the action's expected target.
6. `GracePeriodRequired` deny for claim actions in `Cancelled` state when grace period is not active.
7. `Allowed`.

These steps are encoded in the contract comments using NatSpec-like `@notice` and `@dev` annotations for audit readability.

## Transition Rules

- `AddEmployee`: `Created -> Created`
- `ActivateAgreement`: `Created -> Active`
- `PauseAgreement`: `Active -> Paused`
- `ResumeAgreement`: `Paused -> Active`
- `CancelAgreement`: `Created|Active -> Cancelled`
- `FinalizeGracePeriod`: `Cancelled -> Cancelled` (finalization event, status remains cancelled)
- `RaiseDispute`: `Created|Active|Cancelled -> Disputed`
- `ResolveDispute`: `Disputed -> Completed`
- `ClaimPayroll|ClaimTimeBased|ClaimMilestone`: `Active|Cancelled -> same state`
  - For `Cancelled`, grace period must be active.

## Security Assumptions and Bypass Controls

- Both `actor` and `executor` must authenticate (`require_auth`).
- If `executor != actor`, the call is treated as an auxiliary path.
- Auxiliary path is denied by default and only enabled by admin allowlist (`set_auxiliary_allowed`).
- Admin-only controls:
  - `set_emergency_pause`
  - `set_auxiliary_allowed`

### Operational Assumption

Integrators must provide real execution context:

- `actor`: the principal authorizing the payroll action.
- `executor`: the immediate execution address (direct caller or helper contract identity).

Under this model, non-allowlisted auxiliary contracts cannot bypass transition checks.

## Ruleset Versioning and Mid-flight Upgrades

The compliance checker is a **stateless rules engine** with respect to compliance decisions. It does not persist evaluation results, track in-progress checks, or maintain a history of previous decisions. Every `check_action` call reads the current admin-configurable settings from storage and produces a fresh, deterministic decision.

### What changes immediately

Admin-configurable settings take effect on the very next `check_action` call:

- `set_emergency_pause` — toggling pause state changes the result of the highest-precedence rule for subsequent evaluations.
- `set_auxiliary_allowed` — adding or removing an allowlisted auxiliary address immediately affects auxiliary-path checks.

### What is NOT stored

- **No decision history**: The contract does not store any `ComplianceDecision` values. Historical decisions must be persisted by callers (e.g., the payroll contract).
- **No in-progress evaluation state**: Each `check_action` invocation is self-contained; there is no concept of a multi-step evaluation lifecycle.
- **No rule-set version identifier**: There is no on-chain version counter. Integrators who need to detect rule-set changes should track the admin settings they rely on and re-evaluate when those settings change.

### Security guarantee: no silent reinterpretation

Because this contract never stores decisions, it **cannot retroactively reinterpret** past evaluations under new rules. A `ComplianceDecision` returned from `check_action` is a plain value type; once returned, the contract has no reference to it. The caller alone decides whether to trust a cached decision or re-query after a policy change.

**Important for integrators**: Callers that cache decisions (e.g., to authorize a multi-step payroll workflow) **must** re-query the compliance checker after any admin-configurable setting change. This contract provides no caching or versioning — that responsibility belongs to the integration layer.

### Recommended pattern

```text
1. Caller performs check_action(...) → receives ComplianceDecision{Allow, ...}
2. Caller proceeds with the authorized action (e.g., state transition).
3. If the compliance admin changes settings mid-flight:
   a. The caller SHOULD re-query check_action for any subsequent action.
   b. Decisions obtained before the change remain valid only at the caller's discretion.
```

## Testing Strategy

Negative coverage is concentrated in `onchain/contracts/compliance_checker/tests/test_compliance.rs` and includes:

- non-allowlisted auxiliary deny paths;
- emergency-pause precedence;
- terminal-state denial across all actions;
- invalid current-state matrix for each action;
- invalid target-state denial;
- grace-period denial for cancelled claims;
- ruleset upgrade semantics (mid-flight admin setting changes affect only new evaluations).
