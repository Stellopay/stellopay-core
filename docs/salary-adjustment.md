Salary Adjustment System
Contract path: onchain/contracts/salary_adjustment/src/lib.rs
Test path: onchain/contracts/salary_adjustment/tests/test_adjustment.rs

This document covers the salary_adjustment contract originally implemented for issue #337, including the terminal rejection guarantees added for issue #880.

Overview
The salary_adjustment contract manages employer-driven salary change requests with structured approval workflows, effective date enforcement, salary cap controls, payroll-visible salary tracking, and an append-only audit stream for compliance review.

Workflow
text

Employer creates adjustment (Pending)
         │
   Approver reviews ───────── Employer cancels
    ┌─────┴─────┐                     │
  Approve     Reject               Cancelled
    │             │                (terminal)
  Approved    Rejected
    │          (terminal + reason stored)
  Employer applies after effective_date
    │
  Applied  ──► employee salary updated for payroll
(terminal)
Lifecycle states
State	Meaning	Allowed next state
Pending	Awaiting the configured approver's decision	Approved, Rejected, or Cancelled
Approved	Accepted and waiting for its effective date	Applied
Rejected	Permanently declined; rejection_reason is stored on the adjustment	None (terminal)
Applied	Finalized and reflected in EmployeeSalary	None (terminal)
Cancelled	Withdrawn by the employer while pending	None (terminal)
A rejected id cannot be approved, applied, cancelled, or rejected again. Creation APIs assign ids from a monotonic counter and never accept an id from a caller, so they cannot overwrite or re-submit the rejected record. The employee/effective-date reservation also remains occupied after rejection, preventing the same slot from being submitted as a new adjustment.

Security Model
Concern	Enforcement
Only employer creates/applies/cancels	employer.require_auth() + identity check
Only designated approver approves/rejects	approver.require_auth() + adjustment.approver == approver
Retroactive abuse	create_adjustment / propose_adjustment are forward-only; retroactive edits require owner + employer authorization and a reason hash
Typed proposal bounds	Percentage and fixed-amount values must be > 0; decreases that would yield new_salary <= 0 are rejected
Salary cap enforcement	new_salary <= effective_salary_cap() at creation
One-time initialization	Persistent flag; second call panics
Terminal lifecycle enforcement	Rejected, Applied, and Cancelled have no outgoing transition; cancel is limited to Pending
Rejection accountability	A non-blank reason of at most 256 UTF-8 bytes is stored atomically with Pending → Rejected
Conflicting edits	Same employee + same effective timestamp can only have one stored adjustment
Compliance auditability	Every mutating action appends a queryable audit record and emits an audit event
Constants
Constant	Value	Meaning
DEFAULT_MAX_SALARY	1_000_000_000_000	Default cap (1 trillion stroops) when none is set
BPS_DENOMINATOR	10_000	Basis-points denominator for propose_adjustment percentage mode
MAX_REJECTION_REASON_LENGTH	256	Maximum UTF-8 byte length of a rejection reason
Storage Layout
Key	Type	Description
Initialized	bool	One-time init guard
Owner	Address	Admin who can set salary cap
NextAdjustmentId	u128	Monotonic counter
Adjustment(u128)	SalaryAdjustment	Adjustment record by id
SalaryCap	i128	Optional salary ceiling set by owner
EmployeeSalary(Address)	i128	Last applied salary per employee
NextAuditLogId	u128	Monotonic audit log counter
AuditLog(u128)	SalaryAdjustmentAuditEntry	Append-only audit entry
EmployeeEffectiveAdjustment(Address, u64)	u128	Conflict sentinel for employee/effective-date pairs
Data Model
Rust

pub struct SalaryAdjustment {
    pub id: u128,
    pub employer: Address,
    pub employee: Address,
    pub approver: Address,
    pub kind: AdjustmentKind,       // Increase | Decrease
    pub status: AdjustmentStatus,   // Pending | Approved | Rejected | Applied | Cancelled
    pub current_salary: i128,
    pub new_salary: i128,
    pub effective_date: u64,        // Unix timestamp; must be >= created_at
    pub created_at: u64,
    pub retroactive: bool,
    pub retroactive_approved_by: Option<Address>,
    pub reason_hash: Option<BytesN<32>>, // Retroactive-authorization commitment
    pub rejection_reason: Option<String>, // Set only by terminal rejection
}
reason_hash and rejection_reason serve different purposes. Retroactive records store a contract-computed reason commitment rather than the raw caller-provided hash. The stored value is:

text

sha256(
  "salary_adjustment:retroactive:v1" ||
  owner || employer || employee ||
  current_salary || new_salary || effective_date ||
  caller_supplied_reason_hash
)
This domain separates salary-adjustment reasons from hashes used by other contracts, binds the reason to the immutable adjustment parameters, and avoids storing the plaintext retroactive-authorization rationale on-chain.

A rejection is different: its mandatory human-readable rejection_reason is persisted directly on the adjustment so get_adjustment(id) can return the approver's explanation. Rejection reasons are public ledger data; callers must not include secrets or sensitive personal information.

Contract API
initialize(owner)
One-time setup. Panics if called twice.

set_salary_cap(owner, cap)
Owner-only. Sets a global ceiling enforced on all future create_adjustment calls.

Panics: "Only owner can set salary cap", "Salary cap must be positive"

create_adjustment(employer, employee, approver, current_salary, new_salary, effective_date) -> u128
Creates a new forward-only adjustment in Pending state. effective_date must be at or after the current ledger timestamp.

Panics:

"Current salary must be positive"
"New salary must be positive"
"New salary must differ from current salary"
"New salary exceeds salary cap"
"Effective date cannot be in the past"
"Conflicting adjustment exists"
propose_adjustment(employer, employee, approver, current_salary, adjustment_type, value, kind, effective_date) -> u128
Creates a forward-only adjustment from a percentage or fixed-amount delta instead of an absolute new_salary. The resulting absolute salary is computed on-chain, then the same pending → approve → apply workflow is used.

adjustment_type	value meaning	Formula
Percentage	Basis points (10_000 = 100%)	new = current ± floor(current × value / 10_000)
FixedAmount	Absolute stroop delta	new = current ± value
kind selects increase vs decrease. value must always be positive; direction comes only from kind.

Examples:

Percentage increase: current=10_000, value=1_000 (10%), kind=Increase → 11_000
Fixed decrease: current=10_000, value=1_500, kind=Decrease → 8_500
Panics (type-specific):

"Percentage must be positive" — rejects value <= 0 (including negative percentages)
"Fixed amount must be positive" — rejects value <= 0 (fixed amount below zero)
"Adjustment would result in non-positive salary" — decrease would drive salary to ≤ 0
"Increase kind requires a higher resulting salary" / "Decrease kind requires a lower resulting salary"
All standard create_adjustment validation panics after the absolute salary is resolved
create_retroactive_adjustment(owner, employer, employee, approver, current_salary, new_salary, effective_date, reason_hash) -> u128
Creates a retroactive adjustment in Pending state using the dedicated authorization path.

Requirements:

owner must match the initialized contract owner and authenticate
employer must authenticate
effective_date must be before the current ledger timestamp
reason_hash must be non-zero
the stored reason hash is domain-separated and bound to the adjustment parameters
Panics:

"Only owner can authorize retroactive adjustment"
"Use create_adjustment for forward adjustments"
"Retroactive reason hash required"
all standard create_adjustment validation panics except the forward-only date check
approve_adjustment(approver, adjustment_id)
Moves status Pending → Approved. Only the configured approver may call.

Panics: "Only approver can approve", "Adjustment is not pending"

reject_adjustment(approver, adjustment_id, rejection_reason)
Atomically stores rejection_reason and moves status Pending → Rejected. The reason must contain at least one non-ASCII-whitespace byte and must not exceed MAX_REJECTION_REASON_LENGTH (256) UTF-8 bytes. Rejected is terminal: no later approve, apply, cancel, or second rejection can mutate the id.

Panics: "Only approver can reject", "Adjustment is not pending", "Rejection reason is required", "Rejection reason too long"

apply_adjustment(employer, adjustment_id)
Moves status Approved → Applied. Requires ledger.timestamp() >= effective_date.
Updates EmployeeSalary(employee) for payroll visibility.

Panics: "Only employer can apply", "Adjustment is not approved", "Effective date not reached"

cancel_adjustment(employer, adjustment_id)
Moves status Pending → Cancelled. Approved, Rejected, Applied, and already-Cancelled records are immutable.

Panics: "Only employer can cancel", "Adjustment cannot be cancelled"

get_adjustment(adjustment_id) -> Option<SalaryAdjustment>
Read-only lookup by id. For a rejected adjustment, the returned record has status == Rejected and rejection_reason == Some(...).

get_owner() -> Option<Address>
Returns the contract owner.

get_salary_cap() -> i128
Returns configured cap or DEFAULT_MAX_SALARY if none set.

get_employee_salary(employee) -> Option<i128>
Returns the last applied salary for payroll claiming logic. None until first adjustment is applied.

get_audit_log(audit_id) -> Option<SalaryAdjustmentAuditEntry>
Returns a stored append-only audit entry by id.

get_audit_log_count() -> u128
Returns the number of audit entries written.

Events
Topic	Payload
("adjustment_created", id)	AdjustmentCreatedEvent
("adjustment_approved", id)	AdjustmentApprovedEvent
("adjustment_rejected", id)	AdjustmentRejectedEvent (includes the rejection reason)
("adjustment_applied", id)	AdjustmentAppliedEvent
("adjustment_cancelled", id)	AdjustmentCancelledEvent
("salary_cap_set", cap)	SalaryCapSetEvent
("salary_adjustment_audit", audit_id)	AdjustmentAuditEvent
Audit Stream
Every successful mutating action appends a SalaryAdjustmentAuditEntry and emits a matching audit event:

adjustment_created
adjustment_approved
adjustment_rejected
adjustment_applied
adjustment_cancelled
salary_cap_set
Audit records include the actor, action, optional adjustment id, optional employee, optional amount, optional reason hash, and ledger timestamp. There is no update or delete entrypoint for audit records.

Concurrent Pending Proposals
When an employer submits a second proposal for the same employee before the first is approved, the behavior depends on the effective dates:

Scenario	Result
Same effective date, first still pending	Rejected — "Conflicting adjustment exists"
Different effective dates	Allowed — proposals coexist independently
Each proposal is managed by its unique adjustment_id. All operations (approve_adjustment, apply_adjustment, cancel_adjustment) target the specific id — there is no ambiguous "latest proposal" pointer.

Note: Cancelling or rejecting a proposal does not free its effective date slot. A new proposal with the same effective date will still be rejected. This prevents accidental reuse and preserves a complete audit record for the slot.

Test Coverage
Tests covering:

Initialization (one-time guard, owner stored)
Double-init and pre-init panics
Create: increase, decrease, timestamps, id increment
Create validations: zero salary, same salary, retroactive date, cap exceeded, conflicting effective dates
propose_adjustment percentage vs fixed-amount: correct math through apply_adjustment, negative percentage rejected, fixed amount ≤ 0 rejected, decrease that would drive salary ≤ 0 rejected, failed proposals leave no state
Concurrent proposals: same effective date rejected, different effective dates allowed, cancel/reject independence
Proposal targeting: approve/apply/cancel act on specific id, not "latest"
Retroactive authorization: default block, owner authorization, non-owner rejection, non-zero reason hash, domain-separated immutable reason storage
Cap: default, set, boundary (new_salary == cap), cap tightened
Approve: status change, wrong approver, double-approve, approve-after-reject
Reject: durable reason returned by get_adjustment, mandatory/non-whitespace reason, 256-byte boundary, wrong approver, reject-after-approve
Terminal rejection: apply, approve, cancel, second rejection, and same-slot re-submission all fail; failed calls leave the original status and reason unchanged and a later proposal receives a fresh id
Apply: happy path, exact effective date, before effective date, unapproved, wrong employer
Cancel: pending succeeds; rejected, approved, and applied states are blocked; wrong employer is blocked
Payroll visibility: None before apply, correct value after apply, tracks latest, independent per employee
Audit visibility: audit count, audit entry fields, audit reason linkage
Query: nonexistent adjustment, get_owner
Invariants
An adjustment id is generated by the contract and never reused.
effective_date >= created_at for standard adjustments.
Retroactive adjustments must store retroactive = true, owner approval, and a domain-separated reason_hash.
new_salary <= salary_cap for all stored adjustments.
EmployeeSalary(employee) reflects the new_salary of the most recently applied adjustment.
Only Pending adjustments can be cancelled.
Only Approved adjustments can be applied.
Rejected is terminal and always has a non-blank rejection_reason of at most 256 UTF-8 bytes.
Status transitions are one-way and irreversible (no rollback).
Audit log IDs are monotonic and records are append-only.
One employee/effective-date pair cannot be reused for a conflicting adjustment.
propose_adjustment percentage and fixed-amount values must be strictly positive; direction comes only from kind.
Typed decreases that would produce new_salary <= 0 are rejected before any storage write.
Security Considerations
Retroactive abuse: create_adjustment rejects effective_date < now, preventing accidental or unauthorized backdated salary changes. Retroactive changes must use create_retroactive_adjustment, which requires both owner and employer auth plus a non-zero reason hash.
Typed proposal bounds: propose_adjustment rejects negative/zero percentages, negative/zero fixed amounts, and any decrease that would drive salary to zero or below, so callers cannot smuggle invalid deltas through percentage or fixed modes.
Retroactive-reason privacy and integrity: The raw retroactive-authorization rationale is not stored. The contract stores a domain-separated SHA-256 commitment bound to the owner, employer, employee, salaries, and effective date, so that commitment cannot be replayed across unrelated adjustments without changing the stored hash.
Rejection accountability and privacy: A rejection reason is deliberately stored in plaintext and returned by get_adjustment; it is also emitted in AdjustmentRejectedEvent. The contract rejects empty/ASCII-whitespace-only input and caps the value at 256 UTF-8 bytes to prevent unbounded storage growth. Integrators must treat it as public and exclude secrets and personal data.
Terminal rejection: approve_adjustment accepts only Pending, apply_adjustment accepts only Approved, and cancel_adjustment accepts only Pending. Because rejection atomically changes the record to Rejected, no public transition can move that id again. Monotonic contract-assigned ids and the retained employee/effective-date reservation prevent same-id or same-slot re-submission.
Auditability: All successful state changes write append-only audit entries that can be queried by id and correlated with events.
Conflicts: Duplicate adjustments for the same employee and effective timestamp are rejected to prevent ambiguous payroll interpretation.
Cap bypass: Cap is read fresh on each create_adjustment / propose_adjustment call, so lowering the cap immediately restricts new requests.
Approver identity: The approver is stored per-adjustment at creation. A global admin change does not affect outstanding adjustments.
Auth checks: All state-mutating methods call require_auth() on the acting address before any reads or writes.