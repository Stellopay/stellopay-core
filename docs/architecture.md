# Stellopay Core – Architecture

This document describes the system architecture, main components, design decisions, and data flow for the Stellopay Core project (issue #242).

## System Overview

Stellopay Core is a Soroban-based payroll and agreement system. It consists of:

- **On-chain contracts** (Rust, compiled to WASM): payroll, escrow, disputes, departments, payment splitting, and supporting contracts.
- **CLI / tooling**: local and network operations for deploying and interacting with contracts.
- **Documentation and tests**: per-contract tests and integration tests.

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Stellopay Core                             │
├─────────────────────────────────────────────────────────────────┤
│  Contracts (Soroban / Stellar)                                    │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────────────────┐  │
│  │ stello_pay   │ │ payroll_     │ │ department_manager       │  │
│  │ _contract    │ │ escrow       │ │ payment_splitter          │  │
│  │ (payroll,    │ │ (funds)      │ │ token_vesting, etc.       │  │
│  │  escrow,     │ │              │ │                           │  │
│  │  disputes)   │ │              │ │                           │  │
│  └──────┬───────┘ └──────┬───────┘ └──────────────────────────┘  │
│         │                 │                                        │
│         └─────────────────┴────────────────────────────────────   │
│                           │                                         │
│                    Token (SAC / Stellar Asset)                      │
└─────────────────────────────────────────────────────────────────┘
```

## Main Components

### 1. Stello Pay Contract (`stello_pay_contract`)

- **Role**: Central payroll and escrow logic.
- **Modes**:
  - **Payroll**: Multiple employees per agreement, period-based claiming, escrow-backed.
  - **Escrow**: Single contributor, time-based or milestone-based payments.
- **Features**: Agreement lifecycle (create, activate, pause, resume, cancel), grace period, disputes (arbiter), payroll and time-based claiming, milestone claiming.
- **Storage**: Agreements, employees, escrow balances, dispute status, milestone state (instance and persistent storage as appropriate).

### 2. Payroll Escrow Contract (`payroll_escrow`)

- **Role**: Holds tokens per agreement; only the designated manager contract can release or refund.
- **Used by**: Stello pay contract (or other manager) to hold funds until release/refund conditions are met.

### 3. Department Manager (`department_manager`)

- **Role**: Organizations and departments (hierarchical), employee-to-department assignment.
- **Used for**: Organizing payees into departments and org-level reporting.

### 4. Payment Splitter (`payment_splitter`)

- **Role**: Defines splits (percent or fixed amounts) and computes per-recipient amounts.
- **Used for**: Splitting a single payment across multiple recipients; no token movement in-contract.

### 5. Other Contracts

- **Token vesting**, **payment scheduler**, **payment history**, **bonus system**, **multisig**: each provides a focused capability (vesting, scheduling, history, bonuses, multisig) and can integrate with the main payroll/escrow flow where designed.

## Design Decisions

- **Upgradeability**: Main payroll contract uses an upgradeable pattern (e.g. `UpgradeableInternal`) so logic can be improved without migrating state.
- **Auth**: Critical actions (initialize, cancel, resolve dispute, create org/department, etc.) require the appropriate address to authenticate (`require_auth`).
- **Events**: Contract events (e.g. `#[contractevent]`) are used for agreement lifecycle, claims, and disputes to support indexing and off-chain recovery/backup.
- **Two agreement families**: Payroll (multi-employee, period-based) and escrow (single contributor, time or milestone) share the same contract but different storage paths and entrypoints to keep one deployment and clear separation of behavior.

## Data Flow (Conceptual)

1. **Agreement creation**: Employer creates payroll or escrow agreement; for payroll, employees are added then agreement is activated.
2. **Funding**: Tokens are transferred to the contract (or to the escrow contract) and tracked per agreement.
3. **Claims**: Employees/contributors call `claim_payroll` or `claim_time_based` / `claim_milestone`; contract checks state, then transfers from escrow and updates claimed state.
4. **Disputes**: Either party can raise a dispute during grace period; arbiter resolves with split amounts; contract transfers accordingly.
5. **Grace period / refund**: After cancel, when grace period ends, employer can finalize and receive remaining escrow (refund).

## Component Interactions

- **Stello pay ↔ Token**: Transfers (and possibly approvals) for funding, claims, and refunds.
- **Stello pay ↔ Payroll escrow** (if used): Escrow holds funds; stello pay instructs release/refund.
- **Department manager**: Standalone; no direct token or escrow dependency; can be used by off-chain or other contracts to resolve “which department” for an address.
- **Payment splitter**: Standalone; callers use `compute_split` / `validate_split_for_amount` and perform actual transfers elsewhere.

## Security Assumptions

- Only authorized roles (owner, employer, arbiter, org owner) can perform sensitive actions.
- Reentrancy is mitigated by Soroban’s execution model and by updating state where appropriate; tests (e.g. reentrancy tests) assert that double-claim and state consistency hold.
- Escrow balances and agreement state are the source of truth for what can be claimed or refunded.

## Repository Layout (Relevant Parts)

- `onchain/contracts/` – Individual Soroban contracts.
- `onchain/integration_tests/` – Cross-contract or workflow tests.
- `docs/` – Architecture (this file), department-management, payment-splitting, and other feature docs.
- `tools/cli/` – CLI for deployment and interaction.

This architecture is intended to stay aligned with the codebase; for implementation details, refer to the contract sources and their NatSpec comments.
