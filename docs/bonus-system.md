# Bonus and Incentive Payment System

This document describes the bonus and incentive smart contract added for issue `#210`.

## Scope

The `bonus_system` contract handles compensation flows that are separate from regular payroll:

- One-time bonuses
- Recurring incentives
- Approval and rejection workflow before any claim
- Escrowed token custody to guarantee payouts after approval

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

## Data Model

Each incentive stores:

- `kind`: `OneTime` or `Recurring`
- `status`: `Pending`, `Approved`, `Rejected`, `Cancelled`, `Completed`
- roles: `employer`, `employee`, `approver`
- payout configuration and current claim progress

## Public API

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

## Workflow Summary

1. Employer creates a one-time bonus or recurring incentive and funds it in full.
2. Approver reviews and either approves or rejects.
3. Employee claims once approved and when payouts become available.
4. Employer can cancel and recover funds only while status is `Pending` or `Rejected`.

## Testing Focus

The test suite covers:

- happy paths for one-time and recurring flows
- approval access control
- claim gating before approval or before vesting
- cancellation/refund behavior
- completion state transition for recurring incentives