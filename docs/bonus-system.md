# Bonus and Incentive Payment System

This document describes the bonus and incentive smart contract with comprehensive cap enforcement, clawback mechanisms, and termination integration.

## Scope

The `bonus_system` contract handles compensation flows that are separate from regular payroll:

- One-time bonuses with escrow
- Recurring incentives with vesting schedules
- Approval and rejection workflow before any claim
- Escrowed token custody to guarantee payouts after approval
- **Bonus cap system** (per-employee and per-period limits)
- **Clawback mechanism** (admin-controlled reversal of claimed bonuses)
- **Termination integration** (block post-termination bonuses, enable clawbacks)

## Contract Location

- Contract: `onchain/contracts/bonus_system/src/lib.rs`
- Tests: `onchain/contracts/bonus_system/tests/test_bonus.rs`

## Security Model

- `initialize` is one-time only.
- Employers must authenticate and escrow funds at creation time.
- Only the configured approver can approve or reject an incentive.
- Only the target employee can claim payouts.
- Approved incentives cannot be cancelled, preserving payout guarantees.
- Claims and refunds rely on token transfers from contract escrow.
- **Bonus caps are strictly enforced before creation** (no silent adjustments).
- **Clawbacks require owner authentication and immutable reason hash**.
- **Terminated employees cannot receive new bonuses**.

## Data Model

Each incentive stores:

- `kind`: `OneTime` or `Recurring`
- `status`: `Pending`, `Approved`, `Rejected`, `Cancelled`, `Completed`
- roles: `employer`, `employee`, `approver`
- payout configuration and current claim progress

### Cap System Data

- **Per-employee cap**: Maximum total bonuses an employee can receive in a period
- **Per-period cap**: Maximum total bonuses across all employees in a period (30-day periods)
- **Employee bonus total**: Tracks total bonuses issued to employee in current period
- **Period bonus total**: Tracks total bonuses issued across all employees in current period

### Clawback Data

- **Clawback total per incentive**: Tracks total amount clawed back from each incentive
- **Reason hash**: Immutable audit trail for each clawback operation

### Termination Data

- **Employee terminated flag**: Boolean flag indicating termination status

## Public API

### Core Functions

- `initialize(owner)`
- `create_one_time_bonus(employer, employee, approver, token, amount, unlock_time)`
- `create_recurring_incentive(employer, employee, approver, token, amount_per_payout, total_payouts, start_time, interval_seconds)`
- `approve_incentive(approver, incentive_id)`
- `reject_incentive(approver, incentive_id)`
- `claim_incentive(employee, incentive_id)`
- `cancel_incentive(employer, incentive_id)`
- `get_incentive(incentive_id)`
- `get_claimable_payouts(incentive_id)`
- `get_owner()`

### Cap Management

- `set_bonus_cap(admin, employee, cap_amount)` - Set per-employee or period cap
- `get_employee_cap(employee)` - Get employee's bonus cap
- `get_period_cap()` - Get current period bonus cap
- `get_employee_bonus_total(employee)` - Get employee's total bonuses in current period

### Clawback

- `execute_clawback(admin, employee, incentive_id, clawback_amount, reason_hash)` - Execute clawback
- `get_clawback_total(incentive_id)` - Get total clawed back from incentive

### Termination

- `terminate_employee(admin, employee)` - Mark employee as terminated
- `is_employee_terminated(employee)` - Check if employee is terminated

## Workflow Summary

1. **Admin configures caps** (optional): Set per-employee and/or period bonus limits
2. **Employer creates bonus**: Funds escrow, subject to cap enforcement and termination checks
3. **Approver reviews**: Approves or rejects the incentive
4. **Employee claims**: Claims vested payouts (allowed even after termination)
5. **Admin can clawback**: Reverses claimed bonuses with audit trail (allowed even after termination)
6. **Employer cancels**: Can cancel pending/rejected incentives for refund

## Bonus Cap System

### Configuration

Caps are set by the contract owner (admin) and can be configured at two levels:

1. **Per-employee cap**: Limits total bonuses for a specific employee in a period
   ```
   set_bonus_cap(admin, Some(employee_address), 1000)
   ```

2. **Period cap**: Limits total bonuses across all employees in a 30-day period
   ```
   set_bonus_cap(admin, None, 5000)
   ```

### Enforcement

- Caps are checked **before** bonus creation
- If cap would be exceeded, the transaction fails with explicit error
- `CapEnforcementEvent` is emitted when cap is hit
- Caps reset every 30 days (2,592,000 seconds)
- **No silent adjustments or partial allocations**

### Example

```
Period: 30 days
Employee cap: 1,000 tokens
Period cap: 5,000 tokens

Transaction 1: Create 400 token bonus for Employee A -> SUCCESS (400/1000 used)
Transaction 2: Create 500 token bonus for Employee A -> SUCCESS (900/1000 used)
Transaction 3: Create 200 token bonus for Employee A -> FAIL (would exceed 1000 cap)
Transaction 4: Create 100 token bonus for Employee B -> SUCCESS (1000/5000 period used)
```

## Clawback Mechanism

### Overview

The clawback mechanism allows the contract owner to reverse previously claimed bonuses under controlled conditions with full auditability.

### Requirements

- **Admin-only**: Only the contract owner can execute clawbacks
- **Reason hash required**: Immutable audit proof for why clawback occurred
- **Amount limits**: Cannot claw back more than was actually claimed by employee
- **Tracking**: Clawback amounts are tracked per incentive to prevent double-clawback

### Safety Rules

1. Cannot claw back unclaimed amounts
2. Cannot exceed `claimed_amount = claimed_payouts * amount_per_payout`
3. Cannot exceed `claimed_amount - already_clawed_amount`
4. Funds return to original employer
5. Works on terminated employees (for offboarding scenarios)

### Example Flow

```
1. Employee claims 500 tokens from incentive
2. Admin discovers overpayment or policy violation
3. Admin executes: execute_clawback(admin, employee, incentive_id, 500, reason_hash)
4. 500 tokens transferred from contract back to employer
5. ClawbackExecutedEvent emitted with reason_hash for audit
6. get_clawback_total(incentive_id) returns 500
```

## Termination Integration

### Behavior

When an employee is terminated:

1. **No new bonuses**: All attempts to create new bonuses fail immediately
2. **Existing bonuses remain claimable**: Employee can still claim vested payouts
3. **Clawback still available**: Admin can claw back previously claimed bonuses
4. **Termination is irreversible**: Once terminated, flag cannot be unset

### Use Case: Employee Offboarding

```
Scenario: Employee leaving company
1. Admin calls: terminate_employee(admin, employee)
2. All pending bonus creations fail for this employee
3. Employee claims any remaining vested bonuses
4. Admin reviews bonuses and executes clawbacks if needed (e.g., sign-on bonus repayment)
5. Offboarding complete with full audit trail
```

## Event Reference

### Existing Events

- `IncentiveCreatedEvent`: Bonus created and funded
- `IncentiveApprovedEvent`: Bonus approved by approver
- `IncentiveRejectedEvent`: Bonus rejected by approver
- `IncentiveClaimedEvent`: Employee claimed payout
- `IncentiveCancelledEvent`: Employer cancelled bonus

### New Events

#### BonusCapSetEvent

Emitted when admin sets a bonus cap.

```rust
pub struct BonusCapSetEvent {
    pub admin: Address,          // Who set the cap
    pub employee: Option<Address>, // None = period cap, Some = employee cap
    pub cap_amount: i128,         // Cap amount
}
```

#### ClawbackExecutedEvent

Emitted when admin executes a clawback.

```rust
pub struct ClawbackExecutedEvent {
    pub admin: Address,           // Who executed clawback
    pub employee: Address,        // Employee affected
    pub incentive_id: u128,       // Incentive clawed from
    pub clawback_amount: i128,    // Amount clawed back
    pub reason_hash: u128,        // Immutable audit proof
}
```

#### EmployeeTerminatedEvent

Emitted when employee is terminated.

```rust
pub struct EmployeeTerminatedEvent {
    pub admin: Address,           // Who terminated
    pub employee: Address,        // Terminated employee
    pub timestamp: u64,           // Termination time
}
```

#### CapEnforcementEvent

Emitted when a bonus creation would exceed a cap.

```rust
pub struct CapEnforcementEvent {
    pub employee: Address,        // Employee affected
    pub requested_amount: i128,   // Amount requested
    pub remaining_cap: i128,      // Remaining cap space
    pub period: u64,              // Current period
}
```

## Testing Focus

The test suite covers:

- Happy paths for one-time and recurring flows
- Approval access control
- Claim gating before approval or before vesting
- Cancellation/refund behavior
- Completion state transition for recurring incentives
- **Cap enforcement (per-employee and per-period)**
- **Cap boundary conditions (exact cap, exceeding cap)**
- **Cap reset across periods**
- **Admin-only clawback with reason hash**
- **Clawback amount limits (cannot exceed claimed)**
- **Multiple clawbacks on same incentive**
- **Clawback returns funds to employer**
- **Termination blocks new bonuses**
- **Existing bonuses claimable after termination**
- **Clawback works on terminated employees**
- **Partial claim followed by clawback**
- **Full lifecycle integration tests**