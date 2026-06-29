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

## Testing Strategy

Negative coverage is concentrated in `onchain/contracts/compliance_checker/tests/test_compliance.rs` and includes:

- non-allowlisted auxiliary deny paths;
- emergency-pause precedence;
- terminal-state denial across all actions;
- invalid current-state matrix for each action;
- invalid target-state denial;
- grace-period denial for cancelled claims.
