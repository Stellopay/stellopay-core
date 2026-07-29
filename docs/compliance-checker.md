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
2. **Registered Action Rule** - If a blocking rule is registered for the action, it is denied with `ActionBlocked`
3. **Auxiliary Allowlist** - If `executor != actor`, the executor must be explicitly allowlisted
4. **Terminal State** - Agreements in `Completed` state cannot transition
5. **Invalid Current State** - The action must be valid from the current state
6. **Invalid Target State** - The target state must be the expected transition
7. **Grace Period Required** - Claims from `Cancelled` require an active grace period
8. **Allow** - If all checks pass, the action is allowed

Every rule reads its inputs from persistent storage at evaluation time; see
[Registered Action-Blocking Rules](#registered-action-blocking-rules) for the
guarantees this provides on rule removal.

## Security Assumptions

### Explicit Authorization Required

The contract assumes that all auxiliary contracts must be explicitly allowlisted by the admin. This prevents:

- **Unvetted code execution**: Unknown auxiliary contracts cannot participate in payroll operations.
- **Privilege escalation**: Malicious contracts cannot bypass security checks by impersonating authorized actors.
- **Accidental authorization**: Unintended auxiliary contracts don't gain implicit trust.

### Admin Controls

### Admin-only Guard on `set_emergency_pause`

The `set_emergency_pause` function enforces an admin-only guard via the internal
`require_admin` helper. The guard performs two checks:

1. `caller.require_auth()` — the caller must authenticate via Soroban's host
   authentication framework.
2. `assert!(*caller == admin, "Not admin")` — the authenticated caller must
   match the admin address stored during `initialize`.

This guarantees that a non-admin principal cannot toggle the emergency pause
flag, even if they have been granted some other access to the contract.

### Immediacy Guarantee

The `set_emergency_pause` function writes the `EmergencyPause` storage key using
the Soroban persistent storage layer. Because the `check_action` function reads
this same key on every invocation, the new pause state takes effect
**immediately** — there is no stale-read window, block delay, or asynchronous
propagation.

The read path is:

```rust
let is_paused = env.storage().persistent().get::<_, bool>(&StorageKey::EmergencyPause).unwrap_or(false);
```

This read occurs **before any other rule evaluation** in `check_action` (rule
precedence #1), so the pause state is latched at the very start of every
compliance check. A successful `set_emergency_pause(true)` followed by
`check_action` in the same transaction (or any subsequent transaction) will
correctly see the paused state.

### `is_emergency_paused` — Cross-Contract Pause Signal

`pub fn is_emergency_paused(env: Env) -> bool` is a permissionless, read-only
view exposing the same `EmergencyPause` flag consulted internally by
`check_action`, without requiring the heavier `check_action` call (which
needs an `actor`/`executor`/action/state context that a non-payroll contract
may not have). It is intended for other contracts that want to treat this
contract's emergency pause as a shared halt signal.

`payment_scheduler` is the first such consumer: when wired via
`set_compliance_checker`, its `process_due_payments` entrypoint calls
`is_emergency_paused` before evaluating any due job, and halts the entire
call (evaluating zero jobs) if it returns `true`. See
[docs/integration-examples.md](integration-examples.md#compliance-checker-emergency-pause-halting-the-payment-scheduler)
for the full integration and its test coverage.

### Operational Assumption

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

3. Run the tests:
```bash
cargo test -p compliance_checker

- non-allowlisted auxiliary deny paths;
- emergency-pause precedence;
- terminal-state denial across all actions;
- invalid current-state matrix for each action;
- invalid target-state denial;
- grace-period denial for cancelled claims;
- ruleset upgrade semantics (mid-flight admin setting changes affect only new evaluations).
- **`test_set_emergency_pause_rejects_non_admin`** — verifies that a non-admin
  caller is rejected before the pause flag is toggled;
- **`test_set_emergency_pause_immediate_effect`** — verifies that the very next
  `check_action` call after `set_emergency_pause(true)` returns
  `Deny/EmergencyPaused` with the correct trace entry, confirming zero-latency
  propagation.

## Registered Action-Blocking Rules

In addition to the emergency pause and the auxiliary allowlist, the admin can
register a rule that blocks one specific `PayrollAction`. This is the mechanism
used to take a single action out of service without halting the whole protocol.

### Interface

```rust
/// Registers a rule blocking `action`. Admin only. Idempotent.
pub fn register_rule(env: Env, caller: Address, action: PayrollAction)

/// Removes the blocking rule for `action`. Admin only. Idempotent —
/// removing a rule that was never registered is a no-op.
pub fn remove_rule(env: Env, caller: Address, action: PayrollAction)

/// Returns whether a blocking rule is currently registered for `action`.
pub fn is_rule_registered(env: Env, action: PayrollAction) -> bool
```

A blocked action is denied with `ReasonCode::ActionBlocked` and carries a
`TraceRule::ActionBlocked` entry in the decision trace. The rule sits at
precedence #2 — below the emergency pause, above the auxiliary allowlist and all
state rules — so:

- while the protocol is paused, `EmergencyPaused` remains the reported reason
  regardless of which rules are registered, keeping the reason code stable for
  operators;
- an allowlisted auxiliary executor cannot route around a registered rule,
  because the rule blocks the action itself rather than the caller.

The trace entry is emitted only when a rule is actually registered for the
action, matching the existing behaviour of the auxiliary and grace-period rules,
which likewise trace only when they apply.

### Enforcement Stops Immediately on Removal

`remove_rule` **deletes** the storage entry rather than writing `false`, so a
removed rule leaves nothing behind that a later read could mistake for an active
rule.

`check_action` reads the rule set from persistent storage on every invocation:

```rust
let is_blocked = env
    .storage()
    .persistent()
    .get::<_, bool>(&StorageKey::BlockedAction(action))
    .unwrap_or(false);
```

No rule state is cached in the contract, memoised across invocations, or
snapshotted at transaction start. Every decision is therefore bound to the rule
set as it exists **at that evaluation's own moment**, which gives two guarantees:

1. **Removal takes effect on the next evaluation.** The first `check_action`
   after `remove_rule` already allows the previously-blocked action. There is no
   stale-read window and no ledger-close delay.
2. **An in-flight action is never judged against a stale snapshot.** All
   invocations in a Soroban transaction share the same ledger state, so a
   `check_action` sub-invocation that runs after a `remove_rule` sub-invocation
   in the *same* transaction already sees the removal. Conversely, an evaluation
   that completed before the removal keeps its decision — decisions are values
   bound to their evaluation time and are not retroactively rewritten.

Removal restores an action to **normal evaluation** — it does not exempt it from
the remaining rules. An action whose blocking rule was removed is still subject
to the emergency pause, the auxiliary allowlist, the terminal-state rule, and
the state-transition rules.

### Security Assumptions

- **Admin-only mutation.** `register_rule` and `remove_rule` both go through the
  `require_admin` guard: the caller must authenticate via `require_auth()` *and*
  match the admin stored at `initialize`. A rejected call mutates nothing — the
  guard runs before the storage write, so a non-admin cannot flip a rule off
  even transiently.
- **Initialization required.** Both entrypoints call `require_initialized`, so
  rules cannot be mutated before an admin exists to authorize against.
- **Rule scope is per-action.** Registration and removal are keyed on a single
  `PayrollAction`. Removing one rule never disturbs another.
- **Admin key compromise.** An attacker holding the admin key could remove rules
  and re-open a blocked action. As with the auxiliary allowlist, the admin
  should be a multisig or cold wallet.
- **Storage schema.** `StorageKey::BlockedAction(PayrollAction)` must remain
  compatible across upgrades. Because absence of the key means "no rule", a
  migration that drops these entries silently unblocks every action — verify the
  registered rule set after any upgrade.

### Test Coverage

`onchain/contracts/compliance_checker/tests/test_compliance.rs`:

| Test | Asserts |
| --- | --- |
| `test_registered_rule_blocks_action_then_removal_stops_enforcement` | The full lifecycle: allowed → registered and blocked with the correct trace → removed → allowed again, with no `ActionBlocked` trace surviving the removal |
| `test_rule_removal_is_scoped_to_the_removed_action` | Removing rules one at a time leaves every still-registered rule enforced |
| `test_rule_removal_does_not_blanket_allow_the_action` | After removal the action returns to normal evaluation and is still denied by the state rules when the transition is invalid |
| `test_removed_rule_can_be_registered_again` | Register/remove cycles are stable across repetitions |
| `test_register_and_remove_rule_are_idempotent` | Double registration and double removal leave no half-enforced state |
| `test_register_rule_rejects_non_admin` | A rejected registration does not mutate the rule set |
| `test_remove_rule_rejects_non_admin` | A rejected removal leaves the rule enforced, and only the admin can lift it |
| `test_emergency_pause_outranks_registered_rule` | Pause wins the reason code; lifting it exposes the still-registered rule |
| `test_registered_rule_blocks_allowlisted_auxiliary_executor` | An allowlisted executor cannot bypass a registered rule |
| `test_in_flight_evaluation_reflects_rule_set_at_evaluation_time` | Within one transaction, the evaluation before a removal still sees the rule and the one after does not |
| `test_in_flight_evaluation_reflects_rule_registered_mid_transaction` | The mirror case for registration |
| `test_in_flight_removal_holds_for_every_action` | The in-flight guarantee holds for all 11 payroll actions |
| `test_rule_mutation_requires_initialization` | Rule mutation fails before `initialize` |
| `test_initialize_is_single_shot` | Re-initialization cannot install a new admin with rule-removal rights |

The in-flight tests drive `check_action` through a test-only `RuleSequencer`
contract that interleaves evaluations and rule mutations inside a single
transaction. Because all invocations in a transaction share the same ledger
state, this is the strongest available reproduction of an action that is in
flight while the rule set changes underneath it.
