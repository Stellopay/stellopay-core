## Integration Examples

This document provides **minimal, working‑style examples** for interacting with Stellopay contracts from different environments.

It focuses on the payroll contract, but the patterns apply to other contracts as well.

---

### General Patterns

Across languages and frameworks, integration typically follows the same steps:

1. **Obtain the contract ID** (from deployment or config).
2. **Build a transaction** that invokes a contract function with typed arguments.
3. **Simulate** the transaction (optional but recommended) to estimate fees and validate parameters.
4. **Sign and submit** the transaction to the Soroban RPC endpoint.
5. **Decode results and events** to update off‑chain state.

The examples below sketch this flow in JavaScript/TypeScript and Rust.

---

### Example: JavaScript / TypeScript (Node.js)

This example uses the modern `@stellar/stellar-sdk` with Soroban support to call `create_payroll_agreement` on the payroll contract.

```ts
import {
  Contract,
  Networks,
  SorobanRpc,
  TransactionBuilder,
  xdr,
} from '@stellar/stellar-sdk';

const rpcUrl = 'https://rpc-futurenet.stellar.org';
const server = new SorobanRpc.Server(rpcUrl, { allowHttp: true });

const networkPassphrase = Networks.TESTNET; // or Futurenet/Mainnet
const contractId = '<PAYROLL_CONTRACT_ID>';

async function createPayrollAgreement(employerKeypair, tokenAddress: string) {
  const account = await server.getAccount(employerKeypair.publicKey());
  const contract = new Contract(contractId);

  const tx = new TransactionBuilder(account, {
    fee: '100000',
    networkPassphrase,
  })
    .addOperation(
      contract.call(
        'create_payroll_agreement',
        xdr.ScVal.scvAddress(xdr.ScAddress.scAddressTypeAccount(
          xdr.PublicKey.publicKeyTypeEd25519(employerKeypair.rawPublicKey())
        )),
        xdr.ScVal.scvAddress(Contract.fromContractId(tokenAddress).toScAddress()),
        xdr.ScVal.scvU64(604800n) // grace_period_seconds
      )
    )
    .setTimeout(60)
    .build();

  // Optional: simulate for fee and result preview
  const sim = await server.simulateTransaction(tx);
  if (sim.error) throw new Error(sim.error);

  tx.sign(employerKeypair);
  const sendResp = await server.sendTransaction(tx);
  console.log('Submitted:', sendResp.hash);
}
```

Key points:

- use the generated `Contract` helper for method encoding
- use `simulateTransaction` before sending for validation
- encode arguments as the correct Soroban XDR types

---

### Example: Rust Off‑Chain Service

Rust services can reuse the Soroban client libraries and the **generated contract client types**.

Below is a sketch of invoking `create_escrow_agreement` from a background worker using the auto‑generated `PayrollContractClient`:

```rust
use soroban_sdk::{Address, Env};
use stello_pay_contract::{PayrollContractClient};

fn create_escrow_example(env: &Env, contract_id: &Address) {
    let client = PayrollContractClient::new(env, contract_id);

    let employer = Address::from_string("GEMPLOYER...");
    let contributor = Address::from_string("GCONTRIB...");
    let token = Address::from_string("GTOKEN...");

    let amount_per_period: i128 = 1_000;
    let period_seconds: u64 = 86_400;
    let num_periods: u32 = 12;

    // In an off‑chain context this `Env` would be coming from the host,
    // but the call pattern is identical to the test clients.
    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &amount_per_period,
        &period_seconds,
        &num_periods,
    );

    // Store or index `agreement_id` in your service for later use.
    let _ = agreement_id;
}
```

This mirrors how the test suite exercises the contract and is a good starting point for any Rust‑based orchestration or command‑line tooling.

---

### Example: Using `stellar` CLI for Quick Integrations

For scripting and manual testing, the `stellar` CLI is often the simplest “integration client”.

```bash
# Invoke initialize(owner) on the payroll contract
stellar contract invoke \
  --id <PAYROLL_CONTRACT_ID> \
  --source <OWNER_KEYNAME> \
  --network futurenet \
  --func initialize \
  --arg address:<OWNER_ACCOUNT_ID>

# Call a simple getter, e.g., get_agreement
stellar contract invoke \
  --id <PAYROLL_CONTRACT_ID> \
  --source <ANY_KEYNAME> \
  --network futurenet \
  --func get_agreement \
  --arg u128:1
```

These patterns can be wrapped in shell scripts, CI jobs, or higher‑level deployment tooling to provide reliable, repeatable interactions without writing additional application code.

---

### Dispute Escalation + Audit Logger Integration

The `dispute_escalation` contract optionally records every state transition in
the shared `audit_logger` contract for compliance and forensic audit trails.

#### Wiring

```rust
use audit_logger::{AuditLoggerContract, AuditLoggerContractClient};
use dispute_escalation::{
    DisputeEscalationContract, DisputeEscalationContractClient,
    types::{DisputeOutcome, DisputeStatus, EscalationLevel},
};

// Deploy both contracts
let dispute_id = env.register(DisputeEscalationContract, ());
let dispute_client = DisputeEscalationContractClient::new(&env, &dispute_id);

let audit_id = env.register(AuditLoggerContract, ());
let audit_client = AuditLoggerContractClient::new(&env, &audit_id);

// Initialize
dispute_client.initialize(&owner, &admin);
audit_client.initialize(&owner, &100u32);

// Wire dispute_escalation to audit_logger
dispute_client.set_audit_logger(&admin, &audit_id);
```

#### Lifecycle

After wiring, every transition emits an entry into the audit logger:

| Transition | Action logged | Actor |
|---|---|---|
| `file_dispute` | `dispute_filed` | Caller |
| `escalate_dispute` | `dispute_escalated` | Caller |
| `keeper_advance_stage` | `dispute_sla_breached` | Keeper |
| `resolve_dispute` (L1/L2) | `dispute_resolved` | Admin |
| `resolve_dispute` (L3) | `dispute_finalised` | Admin |
| `appeal_ruling` | `dispute_appealed` | Appellant |
| `expire_dispute` | `dispute_expired` | Caller |

Each entry stores the authenticated `actor`, a `subject` matching the caller,
and a ledger `timestamp`. The entries are strictly ordered and non-duplicable
— a failed transition attempt never creates an audit record.

#### Verification

```rust
// After driving the dispute through its lifecycle, collect all entries
let count = audit_client.get_log_count();
let page = audit_client.get_logs(&0u32, &(count as u32)).unwrap();
for entry in page.entries.iter() {
    assert!(entry.id > 0);
    assert!(entry.timestamp > 0);
    // entry.action is one of the actions listed above
}
```

When no audit logger is configured, the dispute contract operates normally
and no external audit entries are recorded.

---

### Payroll + Token Vesting Integration Assumptions

The payroll and token vesting contracts are integrated by orchestration rather than by direct contract-to-contract calls. A hiring workflow should bind the same `employer`, `employee`, and `token` to:

- a `stello_pay_contract` payroll agreement for recurring salary claims
- a `token_vesting` schedule for grant, bonus, or equity-like vesting claims

The integration tests in `onchain/integration_tests/tests/test_token_vesting_payroll_integration.rs` cover the expected lifecycle:

- hire: employer creates and activates a payroll agreement, then creates a revocable vesting schedule for the same employee
- claims: employee claims payroll periods and vested tokens at multiple ledger timestamps
- termination: payroll cancellation starts the grace period, while vesting revocation refunds only unvested tokens and leaves vested-but-unclaimed tokens claimable
- dispute/grace alignment: an admin early release may be used after a payroll dispute or grace-period decision, but only the vesting owner can approve it
- security boundaries: mismatched employees cannot claim another employee's payroll or vesting, and mismatched employers cannot revoke a schedule

Security notes:

- Payroll escrow accounting remains independent from vesting escrow accounting; both contracts transfer the same SAC token but hold separate balances.
- Revocation freezes vesting at `revoked_at`; later ledger movement must not increase `releasable_amount`.
- Repeated same-ledger claims are rejected once no additional payroll period or vested amount is available.
- Off-chain services should persist the payroll `agreement_id` to vesting `schedule_id` mapping and verify the employer, beneficiary, and token match before presenting combined lifecycle actions.

---

### Payroll Badge Minting on Agreement Activation

The `stello_pay_contract` and `nft_payroll_badge` contracts are composed via **orchestration** (not direct on-chain cross-contract calls). An off-chain orchestrator activates a payroll agreement and, as a follow-up, mints a badge for the employee that references the agreement in its metadata.

#### Orchestrated Pattern (current)

1. Employer calls `activate_agreement(agreement_id)` on the payroll contract.
2. The orchestrator reads the resulting `agreement_id` and the employee list.
3. The orchestrator (or badge contract owner) calls `mint(caller, recipient, name, metadata_uri)` on the badge contract, embedding the agreement reference in `metadata_uri` (e.g. `ipfs://stellopay/badge/{agreement_id}/employee/{index}`).

```rust
// Pseudocode for an off-chain Rust orchestrator
use nft_payroll_badge::NftPayrollBadgeContractClient;
use stello_pay_contract::PayrollContractClient;

fn on_agreement_activated(
    payroll: &PayrollContractClient,
    badge: &NftPayrollBadgeContractClient,
    badge_owner: &Address,
    agreement_id: u128,
) {
    // Activate the agreement
    payroll.activate_agreement(&agreement_id);

    // Determine employee(s) from the agreement
    let employees = payroll.get_agreement_employees(&agreement_id);

    // Mint a badge for each employee, referencing the agreement
    for (i, employee) in employees.iter().enumerate() {
        let metadata_uri = format!("ipfs://stellopay/badge/{}/employee/{}", agreement_id, i);
        badge.mint(
            badge_owner,
            &employee,
            &"Active Payroll Badge".into(),
            &metadata_uri.into(),
        );
    }
}
```

#### Future Direct On-Chain Integration

A future enhancement would have the payroll contract call the badge contract directly during `activate_agreement`, eliminating the need for an off-chain orchestrator. This requires:

- The payroll contract to store the badge contract address (similar to `set_salary_adjustment_contract` / `set_rate_limiter_contract`).
- A new `set_badge_contract` entrypoint on the payroll contract, owner-gated.
- An inline mint call in `activate_agreement` when the badge contract is configured.

#### Security Assumptions

- **Ownership boundary**: Only the badge contract owner may mint badges. The orchestrator must either be the badge contract owner or sign a transaction for the badge owner.
- **Metadata integrity**: The `metadata_uri` must faithfully reference the agreement. Off-chain indexers should verify the agreement exists and is active before accepting a badge as valid.
- **Idempotency**: The orchestrator must guard against double-minting (e.g. track which agreements have already had badges minted). The badge contract itself does not enforce 1:1 agreement-to-badge uniqueness.
- **Cancellation**: Badges minted against an active agreement are not invalidated if the agreement is later cancelled. The badge remains a historical record; off-chain consumers should check current agreement status.

#### Integration Tests

See `onchain/integration_tests/tests/test_badge_activation_integration.rs` for the full test suite covering:

- End-to-end happy path: activate agreement → mint badge → verify metadata
- Multiple employee badges after a single activation
- Non-owner badge mint rejection
- Badge persistence after agreement cancellation
- Paginated badge queries across multiple agreements

---

### Department Management + Payroll Integration

#### Design intent: explicit decoupling

`department_manager` and `stello_pay_contract` are **independent contracts** that share no on-chain state. This is a deliberate design decision:

- `department_manager` is an _organisational_ contract. It tracks which employees belong to which department inside an organisation. It has no knowledge of payroll agreements, escrow balances, or salary schedules.
- `stello_pay_contract` is a _financial_ contract. It tracks payroll agreements, escrow, salary-per-period, and claimed-period counts. It has no knowledge of organisations or department membership.

Neither contract calls the other. There is no hook, callback, or cross-contract read between them.

#### What `remove_employee_from_department` does (and does not do)

When an org owner calls `remove_employee_from_department`:

| Contract | Side-effects |
|---|---|
| `department_manager` | Removes `EmployeeInDepartment` and `EmployeeDepartment` storage keys; updates `DepartmentEmployees` list; emits `emp_rmvd` event. |
| `stello_pay_contract` | **Nothing.** The agreement, escrow balance, employee address at index, and claimed-period count are all unchanged. |

#### Payroll eligibility after offboarding

An employee removed from every department retains full `claim_payroll` eligibility as long as:

1. The payroll agreement is `Active`, _or_ the agreement is `Cancelled` and the grace window has not yet expired.
2. One or more unclaimed periods have elapsed since the last claim.
3. The caller is the address stored at `employee_index` in the agreement (set at `add_employee_to_agreement` time — immutable after that).

Revoking payroll access requires explicitly cancelling or pausing the agreement in `stello_pay_contract`. Department removal alone is insufficient.

#### Recommended offboarding workflow

```
1. Call department_manager::remove_employee_from_department
      → Removes organisational visibility; fires emp_rmvd event.

2. Call stello_pay_contract::cancel_agreement   (employer only)
      → Starts grace period; employee may claim outstanding periods.

3. Wait for grace period to expire (or call finalize_grace_period).
      → Remaining escrow is refunded to employer.
      → All further claim_payroll calls are rejected.
```

If the employee should receive a final pay-out for accrued periods before the agreement is cancelled, let them claim first (step 2a):

```
2a. Employee calls stello_pay_contract::claim_payroll
       → Claims all elapsed but unclaimed periods.

2b. Employer calls stello_pay_contract::cancel_agreement.
```

#### Integration test coverage

The full lifecycle described above is exercised in:

`onchain/integration_tests/tests/test_department_payroll_integration.rs`

| Test | Scenario |
|---|---|
| `claim_succeeds_before_any_department_assignment` | Payroll is independent of dept membership from the start |
| `claim_succeeds_after_department_assignment` | Assigning to a dept does not gate or alter payroll |
| `claim_still_succeeds_after_department_removal` | **Core**: removal does not revoke claim eligibility |
| `sequential_claim_after_removal_accumulates_correctly` | Period accounting is cumulative across the removal boundary |
| `multiple_employees_removal_of_one_does_not_affect_other` | Removal is scoped to one employee; co-workers unaffected |
| `stranger_cannot_claim_regardless_of_dept_membership` | Dept membership ≠ payroll auth; wrong caller is always rejected |
| `claim_during_grace_period_after_dept_removal` | Cancelled-agreement grace path is unaffected by dept state |
| `employee_reassigned_to_new_dept_can_still_claim` | Dept re-assignment has no payroll side-effect |
| `dept_removal_event_is_emitted_payroll_state_unchanged` | `emp_rmvd` fires; all payroll storage is byte-identical before and after |
| `fully_removed_employee_loses_dept_membership_only` | `get_employee_department` → None; `claim_payroll` → Ok simultaneously |

#### Security notes

- **No implicit payroll gate.** Do not rely on `remove_employee_from_department` to stop salary payments. Always cancel the agreement explicitly.
- **Auth is index-based in stello_pay_contract.** The employee address is fixed at `add_employee_to_agreement` time. Department membership of any address — including the employee or a stranger — has no influence on `require_auth` checks inside `claim_payroll`.
- **Event-driven offboarding.** Off-chain systems listening to `emp_rmvd` events should trigger a payroll-cancellation workflow rather than assuming payroll access was revoked automatically.
- **Grace period as a buffer.** The grace period in `stello_pay_contract` is intentionally sized to give employees time to claim outstanding periods after a cancellation. Factor this into offboarding timelines.

---

### Compliance Checker Emergency Pause Halting the Payment Scheduler

#### Design intent: opt-in, loosely-coupled cross-contract gate

`compliance_checker` and `payment_scheduler` are normally independent contracts,
but `payment_scheduler` can optionally be pointed at a deployed
`compliance_checker` instance so that a single, shared emergency-pause
authority can halt scheduled disbursements without the scheduler needing a
separate pause mechanism of its own.

The link is:

- **Opt-in**: a scheduler that never calls `set_compliance_checker` behaves
  exactly as it always has — no compliance check is performed, and an
  unrelated `compliance_checker` deployment being paused has zero effect on
  it.
- **Owner-gated**: only the scheduler's own `owner` (set at `initialize`) may
  call `set_compliance_checker`; a mismatched caller address is rejected with
  `SchedulerError::Unauthorized`.
- **Loosely coupled**: `payment_scheduler` does not take a compile-time Cargo
  dependency on the `compliance_checker` crate. It calls
  `compliance_checker::is_emergency_paused() -> bool` dynamically via
  `env.invoke_contract`, the same pattern already used for the
  `payment_retry` integration (`RetryContractClient`). The two contracts only
  need to agree on a function name and return type.

#### What `process_due_payments` does when paused

`compliance_checker::is_emergency_paused()` is a permissionless, read-only
view mirroring the same `EmergencyPause` flag consulted internally by
`check_action`. When a `compliance_checker` is configured and reports an
active pause, `process_due_payments` returns immediately, before evaluating,
transferring for, or advancing **any** job:

| State | Effect |
|---|---|
| Jobs already settled by an earlier, unpaused call | Untouched — remain `Completed`/paid. |
| Jobs not yet reached when the pause takes effect | Untouched — remain exactly as they were (`Active`, unpaid, same `next_scheduled_time`, `executions`, `retry_count`). |
| Return value of a paused call | `0` — no job was evaluated. |

Because a real keeper drains due jobs by calling `process_due_payments`
repeatedly (see the contract's own module docs), pausing `compliance_checker`
*between* two such calls is sufficient to halt a batch mid-flight: the next
call is a complete no-op, and once the pause is lifted, the following call
resumes and correctly settles exactly the jobs that were left pending.

#### Recommended workflow

```
1. employer/owner: payment_scheduler::set_compliance_checker(owner, compliance_checker_id)
      → One-time wiring; owner-gated.

2. keeper (any address, repeatedly): payment_scheduler::process_due_payments(max_jobs)
      → Normal operation: due jobs settle or get delegated to payment_retry.

3. compliance admin: compliance_checker::set_emergency_pause(admin, true)
      → Shared halt signal flips on.

4. keeper: payment_scheduler::process_due_payments(max_jobs)
      → Returns 0; every remaining due job is left untouched.

5. compliance admin: compliance_checker::set_emergency_pause(admin, false)
      → Halt signal flips off.

6. keeper: payment_scheduler::process_due_payments(max_jobs)
      → Resumes exactly where it left off.
```

#### Integration test coverage

`onchain/integration_tests/tests/test_compliance_pause_scheduler_integration.rs`:

| Test | Scenario |
|---|---|
| `test_emergency_pause_halts_remaining_jobs_mid_batch` | **Core**: process 2 of 5 due jobs, pause mid-batch, verify 0 jobs evaluated and the remaining 3 are byte-for-byte untouched, unpause, verify all 3 settle correctly and no job is ever double-paid |
| `test_pause_before_any_processing_evaluates_zero_jobs` | Pause active before the very first call — no partial progress is possible |
| `test_repeated_pause_unpause_cycles_do_not_corrupt_state` | Multiple pause/unpause cycles interleaved with partial processing do not lose progress or double-process a job |
| `test_scheduler_without_compliance_checker_configured_is_unaffected_by_pause` | Backward compatibility: an unconfigured scheduler ignores an unrelated, paused `compliance_checker` |
| `test_set_compliance_checker_rejects_non_owner` | Only the scheduler's owner may wire up (or repoint) the compliance checker link |

#### Security notes

- **No auth needed for the read.** `is_emergency_paused` requires no
  `require_auth`, by design — any caller of the permissionless
  `process_due_payments` entrypoint must observe the same halt behavior, not
  just the scheduler's owner.
- **Only the compliance admin can flip the flag.** `set_emergency_pause`
  requires the `compliance_checker` admin's signature; a scheduler consulting
  it inherits that same trust boundary rather than introducing a new one.
- **Only the scheduler owner can (re)point the link.** `set_compliance_checker`
  requires `owner.require_auth()` plus an exact match against the stored
  owner, preventing any other address from redirecting the scheduler's pause
  signal to a malicious contract that always reports "not paused".
- **Fully backward compatible.** The `ComplianceChecker` storage key is
  optional and absent by default; existing deployments and existing tests are
  unaffected unless `set_compliance_checker` is explicitly called.

---

### Salary Adjustment + Payroll Claim Integration

`salary_adjustment` and `stello_pay_contract` are linked at the contract level via `set_salary_adjustment_contract`. When configured, `claim_payroll` reads the employee's current salary from the salary adjustment contract and uses it as an override.

#### Integration Contract Binding

The payroll contract stores the salary adjustment contract address in its persistent storage under `SalaryAdjustmentContract`. The owner sets it once:

```rust
payroll_client.set_salary_adjustment_contract(&employer, &salary_adjustment_id);
```

#### How Claim Payroll Consumes the Adjustment

Inside `claim_payroll_inner` (payroll.rs:2681-2690):

```
1. Read salary_per_period from payroll agreement storage.
2. If SalaryAdjustmentContract is configured, call get_employee_salary(employee).
3. If get_employee_salary returns Some(adjusted_salary), override salary_per_period.
```

The override only activates **after** `apply_adjustment` is called on the salary adjustment contract. A `Pending` or `Approved` (but not `Applied`) adjustment has no effect on payouts.

#### Lifecycle

```
1. Employer calls salary_adjustment::create_adjustment        → Pending
2. Approver calls salary_adjustment::approve_adjustment       → Approved
3. Employer calls salary_adjustment::apply_adjustment         → Applied; EmployeeSalary updated
4. Employee calls stello_pay_contract::claim_payroll          → Payout uses new salary
```

At step 3, `apply_adjustment` writes `EmployeeSalary(employee) = new_salary`. At step 4, the `SalaryAdjustmentClient::get_employee_salary` cross-contract call reads that stored value.

#### Security Assumptions

| Concern | Enforcement |
|---------|-------------|
| Only applied adjustments affect payroll | `get_employee_salary` reads `EmployeeSalary` storage, which is only written by `apply_adjustment` |
| Employer controls adjustment lifecycle | `create_adjustment`, `apply_adjustment` require `employer.require_auth()` |
| Approver cannot apply | Approve/Reject are the only actions available to the approver address |
| Payroll contract cannot be tricked into stale salaries | The override is an additive read — if `get_employee_salary` returns `None`, the original payroll-stored salary is used unchanged |
| Adjustment scope is per-employee | `EmployeeSalary` is keyed by `Address`; one employee's adjustment never leaks to another |

#### Integration Tests

See `onchain/integration_tests/tests/test_salary_adjustment_payroll_integration.rs` for the full test suite covering:

| Test | Scenario |
|------|----------|
| `test_salary_adjustment_apply_updates_payroll_salary` | Full happy path: apply increase, claim reflects new rate |
| `test_salary_decrease_affects_payroll_claim` | Salary decrease is correctly reflected in payout |
| `test_applied_adjustment_reflected_in_next_claim` | Very next claim after `apply_adjustment` uses new salary, not stale figure |
| `test_pending_adjustment_does_not_affect_claim_amount` | Approved-but-unapplied adjustment is invisible to `claim_payroll` |
| `test_second_pending_adjustment_ignored_after_first_applied` | Sequential adjustments: applied one is active, pending one is ignored |
| `test_no_adjustment_yet_returns_none` | `get_employee_salary` returns `None` before first `apply_adjustment` |

#### Security Notes

- **Only `Applied` status overrides payroll.** A `Pending` or `Approved` adjustment must never reach the payroll claim flow. The integration test `test_pending_adjustment_does_not_affect_claim_amount` locks in this invariant.
- **Cross-contract reads are additive.** The payroll contract uses the adjustment contract's salary as an override, never as a replacement of its own stored salary. If the salary adjustment contract is removed or returns `None`, the payroll contract falls back to its own `EmployeeSalary` value.
- **One adjustment contract per payroll contract.** The binding is a single address set by the owner. There is no multi-contract aggregation; a single source of truth avoids ambiguity.
- **No retroactive payout recalculation.** The adjustment affects only future claims. Past claims at the old salary are not retroactively adjusted.
- **Effective date gating.** `apply_adjustment` enforces `now >= effective_date`. The payroll contract does not re-check the effective date — it trusts the salary adjustment contract's state machine. This is safe because `apply_adjustment` is the only path that writes `EmployeeSalary`.

---


### Slashing Penalty + Payroll Escrow Integration

The slashing_penalty and payroll_escrow contracts are designed to interoperate via an orchestrated pattern. When a participant is penalized, an orchestrator applies the slash and reflects the penalty against their escrowed payroll funds.

#### Orchestrated Pattern

1. A participant commits a slashable offense.
2. An authorized slasher initiates and eventually finalizes the slash via slashing_penalty::execute_slash.
3. An orchestrator (which is configured as the Manager of the payroll_escrow contract) verifies the slash status.
4. The orchestrator calls payroll_escrow::release to deduct the penalized amount from the available escrow balance, redirecting it to a treasury or burn address.

#### Security Assumptions

- **Manager Authority**: Only the address stored as the Manager in payroll_escrow can authorize the release of funds to apply the penalty.
- **Slash Execution Verification**: The orchestrator must securely query get_slash_record to verify that the slash reached the Executed status before deducting funds.
- **Isolated Balances**: A penalty executed against one participant must strictly only reduce the escrow balance tied to that specific participant's agreements. Unrelated parties remain unaffected.

#### Integration Tests

See onchain/integration_tests/tests/test_slashing_escrow_integration.rs for the full test suite covering:
- Simulating a slashing orchestrator
- Verifying execute_slash correctly reduces the balance payroll_escrow reports as available
- Ensuring unrelated party escrow balances are unaffected by another party's slash

### Rate Limiter + Payment Retry Integration

#### Design Intent: Preventing Double-Counting of Throttled Attempts

The `rate_limiter` and `payment_retry` contracts are designed to operate **independently** while maintaining consistent state. The key invariant is:

> A single throttled payment attempt must increment `payment_retry.retry_count` by **exactly one**, regardless of how many times `rate_limiter` enforces its quota.

#### How the Contracts Interact

The integration is **orchestration-based** (not direct cross-contract calls):

1. An off-chain keeper or service calls `rate_limiter.check_and_consume()` before attempting a payment
2. If rate limiting occurs, the call panics with "rate limit exceeded" — this is NOT counted as a payment retry attempt
3. The keeper then calls `payment_retry.process_retry()` for actual payment processing
4. `payment_retry` increments `retry_count` only when escrow balance is insufficient (`escrowed < amount`)

#### Separation of Concerns

| Contract | What it tracks | When it updates |
|----------|---------------|-----------------|
| `rate_limiter` | Token bucket per address | On every `check_and_consume` call |
| `payment_retry` | Failed payment attempts per `payment_id` | Only when `escrowed < amount` in `process_payment_if_due` |

#### Security Invariants

| Invariant | Enforcement |
|-----------|-------------|
| Single-counting | `payment_retry` only increments `retry_count` when escrow is insufficient, not when rate limiting occurs |
| No double-counting | Rate limiter and payment retry have **separate** storage key namespaces |
| Idempotency | `process_payment_if_due` checks `next_retry_at` before processing; calls during backoff are no-ops |
| Terminal state isolation | Once `state ∈ {Success, Failed, Cancelled}`, counter never changes |

#### Integration Test Coverage

`onchain/integration_tests/tests/test_rate_limiter_payment_retry_integration.rs`:

| Test | Scenario | Invariant Verified |
|------|----------|-------------------|
| `test_throttled_attempt_counts_as_one` | Basic throttling → retry_count = 1 | Single-counting |
| `test_successful_retry_after_throttle_increments_by_one` | Success after throttle → counter stays at failed attempts | No double-increment on success |
| `test_multiple_throttles_before_funding` | Multiple throttles → counter increments correctly | Counter accuracy under load |
| `test_rate_limiter_exhaustion_then_refill` | Exhaust + refill → counter continues correctly | State consistency |
| `test_full_lifecycle_throttle_to_success` | E2E: throttle → partial → success | Complete lifecycle correctness |
| `test_throttle_during_retry_backoff` | Throttle during backoff window | No early increment |
| `test_rate_limiter_external_exhaustion_does_not_affect_payment_counter` | External rate limiting | Independent tracking |
| `test_integrated_rate_limiter_and_payment_retry_flow` | E2E integration flow | End-to-end correctness |
| `test_batch_process_due_payments_counter_integrity` | Batch processing | Per-payment isolation |
| `test_zero_max_retries_counter_behavior` | Edge case: max_retries = 0 | Zero max_retries |
| `test_rapid_successive_calls_counter_integrity` | Rapid calls during backoff | Idempotency |
| `test_retry_failed_events_emitted_during_throttle` | Event emission | Event correctness |

#### Security Notes

- **Rate limiting does NOT cause retry counting.** A `rate_limiter` panic is separate from `payment_retry` state. Only `process_payment_if_due` increments the counter.
- **Backoff window protection.** Calls to `process_retry` during `next_retry_at - now > 0` are no-ops and do NOT increment the counter.
- **Terminal state is absolute.** Once a payment reaches `Success`, `Failed`, or `Cancelled`, the counter is frozen regardless of subsequent calls.
- **Counter is per-payment, not per-address.** Each `payment_id` has its own `retry_count`, preventing one payment's failures from affecting another.

#### Recommended Integration Pattern

```rust
// Off-chain keeper pseudocode
async fn process_payment_with_rate_limiting(
    rate_limiter: &RateLimiterClient,
    payment_retry: &PaymentRetryContractClient,
    payment_id: BytesN<32>,
    caller: &Address,
) -> Result<PaymentState, Error> {
    // Step 1: Check rate limit (non-mutating read)
    let usage = rate_limiter.get_usage(caller)?;
    if usage.map(|u| u.tokens).unwrap_or(1) == 0 {
        return Err(RateLimitExceeded);
    }

    // Step 2: Consume rate limit token
    match rate_limiter.try_check_and_consume(caller) {
        Ok(_) => { /* proceed */ }
        Err(_) => return Err(RateLimitExceeded),
    }

    // Step 3: Process the payment
    payment_retry.process_retry(&payment_id);

    // Step 4: Return current state
    Ok(payment_retry.get_payment(&payment_id)?.state)
}
```

#### Running the Tests

```bash
# Run all rate limiter + payment retry integration tests
cargo test -p integration_tests test_rate_limiter_payment_retry

# Run a specific test
cargo test -p integration_tests test_throttled_attempt_counts_as_one

# Run with verbose output
cargo test -p integration_tests test_rate_limiter_payment_retry -- --nocapture
```

#### Test Execution Evidence

Each test in the integration suite verifies:
1. The counter starts at 0 for new payments
2. Each failed escrow attempt increments the counter by exactly 1
3. Successful payments do NOT increment the counter
4. Terminal states (`Success`, `Failed`) preserve the counter
5. Rate limiter state changes do not affect the counter
6. Batch processing maintains per-payment counter isolation

