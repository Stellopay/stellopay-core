# Compliance Checker Contract

## Overview

The Compliance Checker contract implements a deterministic rules engine for validating payroll lifecycle actions. It enforces security policies and transition rules for payroll agreements, ensuring that all state transitions are authorized and compliant.

## Key Security Design

### Fail-Closed by Default

The contract implements a **fail-closed** security model for auxiliary contract authorization:

- **Auxiliary callers are denied by default**. An auxiliary contract that has never been explicitly added via `set_auxiliary_allowed` will be rejected by `check_action`.
- **Explicit allowlisting is required**. To authorize an auxiliary contract, an admin must explicitly call `set_auxiliary_allowed(auxiliary, true)`.
- **Removal restores the fail-closed state**. Setting an auxiliary to `false` immediately denies future calls.

This design follows the principle of least privilege and protects against unauthorized access by unvetted auxiliary contracts.

### Rule Precedence

When evaluating a compliance check, rules are applied in the following precedence order (highest to lowest):

1. **Emergency Pause** - If paused, all actions are denied with `EmergencyPaused`
2. **Auxiliary Allowlist** - If `executor != actor`, the executor must be explicitly allowlisted
3. **Terminal State** - Agreements in `Completed` state cannot transition
4. **Invalid Current State** - The action must be valid from the current state
5. **Invalid Target State** - The target state must be the expected transition
6. **Grace Period Required** - Claims from `Cancelled` require an active grace period
7. **Allow** - If all checks pass, the action is allowed

## Security Assumptions

### Explicit Authorization Required

The contract assumes that all auxiliary contracts must be explicitly allowlisted by the admin. This prevents:

- **Unvetted code execution**: Unknown auxiliary contracts cannot participate in payroll operations.
- **Privilege escalation**: Malicious contracts cannot bypass security checks by impersonating authorized actors.
- **Accidental authorization**: Unintended auxiliary contracts don't gain implicit trust.

### Admin Controls

The admin address has the authority to:

- Set emergency pause state
- Add or remove auxiliary contracts from the allowlist
- The admin cannot bypass the fail-closed default; they must explicitly authorize each auxiliary

## Implementation Notes

### `check_action` Authorization Flow

Require initialization

Authenticate actor (and executor if different)

Check Emergency Pause → Deny if paused

Check Auxiliary Allowlist → Deny if uninitialized

Check Terminal State → Deny if completed

Check Current State Validity → Deny if invalid

Check Target State Validity → Deny if invalid

Check Grace Period → Deny if required but not active

Allow the action


### `set_auxiliary_allowed` Interface

```rust
pub fn set_auxiliary_allowed(
    env: Env,
    caller: Address,    // Must be admin
    auxiliary: Address, // Contract to allow/deny
    allowed: bool       // true = allow, false = deny
)

Only the admin can modify the auxiliary allowlist

Setting allowed to false immediately revokes authorization

The contract stores the allowlist in persistent storage

is_auxiliary_allowed Query

pub fn is_auxiliary_allowed(env: Env, auxiliary: Address) -> bool

Returns false for auxiliaries that have never been configured

Returns the explicit state for configured auxiliaries

Used internally by check_action to enforce the fail-closed policy

Test Coverage
The contract includes comprehensive tests covering:

Fail-closed auxiliary checks: Uninitialized auxiliaries are properly denied

Explicit allowlisting: Configured auxiliaries pass checks

Allowlist removal: De-initialized auxiliaries revert to fail-closed

Multiple auxiliaries: Each auxiliary's authorization is independently tracked

Multiple action types: All payroll actions respect the authorization policy

Trace verification: Denials include proper trace entries showing the rejection reason

Example Test: Uninitialized Auxiliary

#[test]
fn test_uninitialized_auxiliary_fails_closed() {
    // Setup
    let env = create_env();
    let (_cid, client, admin) = setup(&env);
    let actor = Address::generate(&env);
    let unknown_auxiliary = Address::generate(&env);
    
    // Test: unknown auxiliary should be denied
    let decision = client.check_action(
        &actor,
        &unknown_auxiliary,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    
    assert_eq!(decision.decision, Decision::Deny);
    assert_eq!(decision.reason, ReasonCode::AuxiliaryNotAllowed);
    
    // Configure the auxiliary
    client.set_auxiliary_allowed(&admin, &unknown_auxiliary, &true);
    
    // Test: now it should be allowed
    let allowed_decision = client.check_action(
        &actor,
        &unknown_auxiliary,
        &PayrollAction::ActivateAgreement,
        &AgreementStatus::Created,
        &AgreementStatus::Active,
        &false,
    );
    
    assert_eq!(allowed_decision.decision, Decision::Allow);
}

Security Considerations
Admin keys: Compromise of the admin key would allow an attacker to allowlist malicious auxiliary contracts. Admin should be a multisig or cold wallet.

Auxiliary contract security: Allowlisted auxiliary contracts must be secured and follow the same security standards as the core contract.

Storage persistence: The allowlist state persists across upgrades. When upgrading, verify the allowlist is correctly migrated.

Fail-closed guarantee: The contract guarantees that any auxiliary not explicitly allowlisted will be denied. This is a hard security invariant.

No implicit trust: There is no fallback to "allow" for uninitialized auxiliaries. This prevents accidental authorization through default-true logic.

Migration and Upgrades
When upgrading the contract:

The storage schema for AuxiliaryAllowed must remain compatible

The fail-closed behavior must be preserved

Existing allowlist entries should be verified after upgrade

Consider reinitializing the allowlist if storage layout changes

Related Documentation
State Machines - Detailed state transition rules

Emergency Pause - Emergency pause behavior

RBAC - Role-based access control

Security Threat Model - Overall security analysis


## Commands to Apply Changes

1. Copy the test code into `onchain/contracts/compliance_checker/tests/test_compliance.rs` at the end of the file (before the final `}`).

2. Replace the entire content of `docs/compliance-checker.md` with the new content.

3. Run the tests:
```bash
cargo test -p compliance_checker


## Commands to Apply Changes

1. Copy the test code into `onchain/contracts/compliance_checker/tests/test_compliance.rs` at the end of the file (before the final `}`).

2. Replace the entire content of `docs/compliance-checker.md` with the new content.

3. Run the tests:
```bash
cargo test -p compliance_checker

