#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, xdr::ToXdr, Address, Bytes, BytesN, Env, Symbol,
};

// ============================================================================
// CONTRACT STRUCT
// ============================================================================

#[contract]
pub struct SalaryAdjustmentContract;

// ============================================================================
// DOMAIN TYPES
// ============================================================================

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdjustmentKind {
    Increase,
    Decrease,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdjustmentStatus {
    Pending,
    Approved,
    Rejected,
    Applied,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SalaryAdjustment {
    pub id: u128,
    pub employer: Address,
    pub employee: Address,
    pub approver: Address,
    pub kind: AdjustmentKind,
    pub status: AdjustmentStatus,
    pub current_salary: i128,
    pub new_salary: i128,
    pub effective_date: u64,
    pub created_at: u64,
    pub retroactive: bool,
    pub retroactive_approved_by: Option<Address>,
    pub reason_hash: Option<BytesN<32>>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SalaryAdjustmentAuditEntry {
    pub id: u128,
    pub adjustment_id: Option<u128>,
    pub actor: Address,
    pub action: Symbol,
    pub employee: Option<Address>,
    pub amount: Option<i128>,
    pub reason_hash: Option<BytesN<32>>,
    pub timestamp: u64,
}

// ============================================================================
// STORAGE KEYS
// ============================================================================

#[contracttype]
#[derive(Clone)]
enum StorageKey {
    Initialized,
    Owner,
    NextAdjustmentId,
    Adjustment(u128),
    /// Global salary cap enforced on all new adjustments.
    SalaryCap,
    /// Tracks the last applied salary per employee for payroll visibility.
    EmployeeSalary(Address),
    /// Monotonic audit id for append-only salary adjustment audit records.
    NextAuditLogId,
    /// Append-only audit record keyed by id.
    AuditLog(u128),
    /// Prevents conflicting unresolved adjustments for the same employee/effective timestamp.
    EmployeeEffectiveAdjustment(Address, u64),
}

// ============================================================================
// EVENTS
// ============================================================================

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdjustmentCreatedEvent {
    pub adjustment_id: u128,
    pub employer: Address,
    pub employee: Address,
    pub kind: AdjustmentKind,
    pub current_salary: i128,
    pub new_salary: i128,
    pub effective_date: u64,
    pub retroactive: bool,
    pub reason_hash: Option<BytesN<32>>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdjustmentApprovedEvent {
    pub adjustment_id: u128,
    pub approver: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdjustmentRejectedEvent {
    pub adjustment_id: u128,
    pub approver: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdjustmentAppliedEvent {
    pub adjustment_id: u128,
    pub employee: Address,
    pub new_salary: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdjustmentCancelledEvent {
    pub adjustment_id: u128,
    pub employer: Address,
}

/// Emitted when the owner sets or updates the global salary cap.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SalaryCapSetEvent {
    pub owner: Address,
    pub cap: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdjustmentAuditEvent {
    pub audit_id: u128,
    pub adjustment_id: Option<u128>,
    pub actor: Address,
    pub action: Symbol,
    pub employee: Option<Address>,
    pub amount: Option<i128>,
    pub reason_hash: Option<BytesN<32>>,
}

// ============================================================================
// CONSTANTS
// ============================================================================

/// Default maximum allowable salary (1 trillion stroops) used when no
/// explicit cap has been configured by the owner.
pub const DEFAULT_MAX_SALARY: i128 = 1_000_000_000_000;

const RETRO_REASON_DOMAIN: &[u8] = b"salary_adjustment:retroactive:v1";

// ============================================================================
// INTERNAL HELPERS
// ============================================================================

fn require_initialized(env: &Env) {
    let initialized = env
        .storage()
        .persistent()
        .get::<_, bool>(&StorageKey::Initialized)
        .unwrap_or(false);
    assert!(initialized, "Contract not initialized");
}

fn read_adjustment(env: &Env, adjustment_id: u128) -> SalaryAdjustment {
    env.storage()
        .persistent()
        .get::<_, SalaryAdjustment>(&StorageKey::Adjustment(adjustment_id))
        .expect("Adjustment not found")
}

fn write_adjustment(env: &Env, adjustment: &SalaryAdjustment) {
    env.storage()
        .persistent()
        .set(&StorageKey::Adjustment(adjustment.id), adjustment);
}

fn next_adjustment_id(env: &Env) -> u128 {
    let current = env
        .storage()
        .persistent()
        .get::<_, u128>(&StorageKey::NextAdjustmentId)
        .unwrap_or(0);
    let next = current.checked_add(1).expect("Adjustment id overflow");
    env.storage()
        .persistent()
        .set(&StorageKey::NextAdjustmentId, &next);
    next
}

fn next_audit_id(env: &Env) -> u128 {
    let current = env
        .storage()
        .persistent()
        .get::<_, u128>(&StorageKey::NextAuditLogId)
        .unwrap_or(0);
    let next = current.checked_add(1).expect("Audit id overflow");
    env.storage()
        .persistent()
        .set(&StorageKey::NextAuditLogId, &next);
    next
}

/// Returns the configured salary cap, falling back to DEFAULT_MAX_SALARY.
fn effective_salary_cap(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get::<_, i128>(&StorageKey::SalaryCap)
        .unwrap_or(DEFAULT_MAX_SALARY)
}

fn domain_separated_reason_hash(
    env: &Env,
    admin: &Address,
    employer: &Address,
    employee: &Address,
    current_salary: i128,
    new_salary: i128,
    effective_date: u64,
    reason_hash: &BytesN<32>,
) -> BytesN<32> {
    let mut bytes = Bytes::new(env);
    for byte in RETRO_REASON_DOMAIN.iter() {
        bytes.push_back(*byte);
    }
    bytes.append(&admin.clone().to_xdr(env));
    bytes.append(&employer.clone().to_xdr(env));
    bytes.append(&employee.clone().to_xdr(env));
    for byte in current_salary.to_le_bytes().iter() {
        bytes.push_back(*byte);
    }
    for byte in new_salary.to_le_bytes().iter() {
        bytes.push_back(*byte);
    }
    for byte in effective_date.to_le_bytes().iter() {
        bytes.push_back(*byte);
    }
    bytes.append(&reason_hash.clone().to_xdr(env));
    env.crypto().sha256(&bytes).into()
}

fn assert_adjustment_inputs(
    env: &Env,
    current_salary: i128,
    new_salary: i128,
    effective_date: u64,
    allow_retroactive: bool,
) {
    assert!(current_salary > 0, "Current salary must be positive");
    assert!(new_salary > 0, "New salary must be positive");
    assert!(
        new_salary != current_salary,
        "New salary must differ from current salary"
    );

    let cap = effective_salary_cap(env);
    assert!(new_salary <= cap, "New salary exceeds salary cap");

    let now = env.ledger().timestamp();
    if !allow_retroactive {
        assert!(
            effective_date >= now,
            "Effective date cannot be in the past"
        );
    }
}

fn assert_no_conflicting_adjustment(env: &Env, employee: &Address, effective_date: u64) {
    let existing =
        env.storage()
            .persistent()
            .get::<_, u128>(&StorageKey::EmployeeEffectiveAdjustment(
                employee.clone(),
                effective_date,
            ));
    assert!(existing.is_none(), "Conflicting adjustment exists");
}

fn reserve_adjustment_slot(
    env: &Env,
    employee: &Address,
    effective_date: u64,
    adjustment_id: u128,
) {
    env.storage().persistent().set(
        &StorageKey::EmployeeEffectiveAdjustment(employee.clone(), effective_date),
        &adjustment_id,
    );
}

fn append_audit(
    env: &Env,
    actor: Address,
    action: Symbol,
    adjustment_id: Option<u128>,
    employee: Option<Address>,
    amount: Option<i128>,
    reason_hash: Option<BytesN<32>>,
) -> u128 {
    let audit_id = next_audit_id(env);
    let entry = SalaryAdjustmentAuditEntry {
        id: audit_id,
        adjustment_id,
        actor: actor.clone(),
        action: action.clone(),
        employee: employee.clone(),
        amount,
        reason_hash: reason_hash.clone(),
        timestamp: env.ledger().timestamp(),
    };

    env.storage()
        .persistent()
        .set(&StorageKey::AuditLog(audit_id), &entry);
    env.events().publish(
        ("salary_adjustment_audit", audit_id),
        AdjustmentAuditEvent {
            audit_id,
            adjustment_id,
            actor,
            action,
            employee,
            amount,
            reason_hash,
        },
    );
    audit_id
}

fn create_adjustment_internal(
    env: &Env,
    employer: Address,
    employee: Address,
    approver: Address,
    current_salary: i128,
    new_salary: i128,
    effective_date: u64,
    retroactive_approved_by: Option<Address>,
    reason_hash: Option<BytesN<32>>,
) -> u128 {
    let retroactive = effective_date < env.ledger().timestamp();
    assert_adjustment_inputs(
        env,
        current_salary,
        new_salary,
        effective_date,
        retroactive_approved_by.is_some(),
    );
    assert_no_conflicting_adjustment(env, &employee, effective_date);

    let kind = if new_salary > current_salary {
        AdjustmentKind::Increase
    } else {
        AdjustmentKind::Decrease
    };

    let adjustment_id = next_adjustment_id(env);

    let adjustment = SalaryAdjustment {
        id: adjustment_id,
        employer: employer.clone(),
        employee: employee.clone(),
        approver: approver.clone(),
        kind: kind.clone(),
        status: AdjustmentStatus::Pending,
        current_salary,
        new_salary,
        effective_date,
        created_at: env.ledger().timestamp(),
        retroactive,
        retroactive_approved_by,
        reason_hash: reason_hash.clone(),
    };

    write_adjustment(env, &adjustment);
    reserve_adjustment_slot(env, &employee, effective_date, adjustment_id);
    env.events().publish(
        ("adjustment_created", adjustment_id),
        AdjustmentCreatedEvent {
            adjustment_id,
            employer: employer.clone(),
            employee: employee.clone(),
            kind,
            current_salary,
            new_salary,
            effective_date,
            retroactive,
            reason_hash: reason_hash.clone(),
        },
    );
    append_audit(
        env,
        employer,
        Symbol::new(env, "adjustment_created"),
        Some(adjustment_id),
        Some(employee),
        Some(new_salary),
        reason_hash,
    );

    adjustment_id
}

// ============================================================================
// CONTRACT IMPLEMENTATION
// ============================================================================

#[contractimpl]
impl SalaryAdjustmentContract {
    /// @notice Initializes the salary adjustment contract.
    /// @dev Can only be executed once. Subsequent calls panic.
    /// @param owner Address allowed to run owner-level actions (e.g. set_salary_cap).
    pub fn initialize(env: Env, owner: Address) {
        owner.require_auth();

        let initialized = env
            .storage()
            .persistent()
            .get::<_, bool>(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(!initialized, "Contract already initialized");

        env.storage().persistent().set(&StorageKey::Owner, &owner);
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);
    }

    /// @notice Sets a global salary cap enforced on all future adjustments.
    /// @dev Only the contract owner can call this. `cap` must be positive.
    ///
    /// # Panics
    /// * `"Contract not initialized"`
    /// * `"Only owner can set salary cap"`
    /// * `"Salary cap must be positive"`
    ///
    /// # Events
    /// Emits `("salary_cap_set", cap)` with a `SalaryCapSetEvent`.
    pub fn set_salary_cap(env: Env, owner: Address, cap: i128) {
        require_initialized(&env);
        owner.require_auth();

        let stored_owner: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::Owner)
            .expect("Owner not set");
        assert!(owner == stored_owner, "Only owner can set salary cap");
        assert!(cap > 0, "Salary cap must be positive");

        env.storage().persistent().set(&StorageKey::SalaryCap, &cap);

        env.events().publish(
            ("salary_cap_set", cap),
            SalaryCapSetEvent {
                owner: owner.clone(),
                cap,
            },
        );
        append_audit(
            &env,
            owner,
            Symbol::new(&env, "salary_cap_set"),
            None,
            None,
            Some(cap),
            None,
        );
    }

    /// @notice Creates a salary adjustment request.
    /// @dev Determines increase or decrease from salary comparison.
    ///      Rejects retroactive effective dates (before current ledger time).
    ///      Rejects new_salary exceeding the configured salary cap.
    ///
    /// @param employer Employer submitting the adjustment; must authenticate.
    /// @param employee Employee whose salary is being adjusted.
    /// @param approver Address that can approve or reject.
    /// @param current_salary Current salary amount (must be positive).
    /// @param new_salary Proposed new salary amount (must differ from current and not exceed cap).
    /// @param effective_date Timestamp when the adjustment takes effect; must be >= now.
    /// @return u128 The newly assigned adjustment id.
    ///
    /// # Panics
    /// * `"Contract not initialized"`
    /// * `"Current salary must be positive"`
    /// * `"New salary must be positive"`
    /// * `"New salary must differ from current salary"`
    /// * `"New salary exceeds salary cap"`
    /// * `"Effective date cannot be in the past"`
    ///
    /// # Events
    /// Emits `("adjustment_created", adjustment_id)` with an `AdjustmentCreatedEvent`.
    pub fn create_adjustment(
        env: Env,
        employer: Address,
        employee: Address,
        approver: Address,
        current_salary: i128,
        new_salary: i128,
        effective_date: u64,
    ) -> u128 {
        require_initialized(&env);
        employer.require_auth();
        create_adjustment_internal(
            &env,
            employer,
            employee,
            approver,
            current_salary,
            new_salary,
            effective_date,
            None,
            None,
        )
    }

    /// @notice Creates a retroactive salary adjustment with explicit owner approval.
    /// @dev Requires both the employer and contract owner to authenticate. The provided
    ///      reason hash is domain-separated with the adjustment details before storage.
    ///
    /// # Panics
    /// * `"Only owner can authorize retroactive adjustment"`
    /// * `"Retroactive reason hash required"`
    /// * Standard create validation panics.
    pub fn create_retroactive_adjustment(
        env: Env,
        owner: Address,
        employer: Address,
        employee: Address,
        approver: Address,
        current_salary: i128,
        new_salary: i128,
        effective_date: u64,
        reason_hash: BytesN<32>,
    ) -> u128 {
        require_initialized(&env);
        owner.require_auth();
        employer.require_auth();

        let stored_owner: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::Owner)
            .expect("Owner not set");
        assert!(
            owner == stored_owner,
            "Only owner can authorize retroactive adjustment"
        );
        assert!(
            effective_date < env.ledger().timestamp(),
            "Use create_adjustment for forward adjustments"
        );

        let zero = BytesN::from_array(&env, &[0; 32]);
        assert!(reason_hash != zero, "Retroactive reason hash required");

        let stored_reason_hash = domain_separated_reason_hash(
            &env,
            &owner,
            &employer,
            &employee,
            current_salary,
            new_salary,
            effective_date,
            &reason_hash,
        );

        create_adjustment_internal(
            &env,
            employer,
            employee,
            approver,
            current_salary,
            new_salary,
            effective_date,
            Some(owner),
            Some(stored_reason_hash),
        )
    }

    /// @notice Approves a pending salary adjustment.
    /// @dev Only the configured approver can move status from Pending to Approved.
    /// @param approver Approver address; must authenticate.
    /// @param adjustment_id Adjustment identifier.
    ///
    /// # Panics
    /// * `"Contract not initialized"`
    /// * `"Only approver can approve"`
    /// * `"Adjustment is not pending"`
    ///
    /// # Events
    /// Emits `("adjustment_approved", adjustment_id)` with an `AdjustmentApprovedEvent`.
    pub fn approve_adjustment(env: Env, approver: Address, adjustment_id: u128) {
        require_initialized(&env);
        approver.require_auth();

        let mut adjustment = read_adjustment(&env, adjustment_id);
        assert!(adjustment.approver == approver, "Only approver can approve");
        assert!(
            adjustment.status == AdjustmentStatus::Pending,
            "Adjustment is not pending"
        );

        adjustment.status = AdjustmentStatus::Approved;
        write_adjustment(&env, &adjustment);

        env.events().publish(
            ("adjustment_approved", adjustment_id),
            AdjustmentApprovedEvent {
                adjustment_id,
                approver: approver.clone(),
            },
        );
        append_audit(
            &env,
            approver,
            Symbol::new(&env, "adjustment_approved"),
            Some(adjustment_id),
            Some(adjustment.employee),
            Some(adjustment.new_salary),
            adjustment.reason_hash,
        );
    }

    /// @notice Rejects a pending salary adjustment.
    /// @dev Rejected adjustments can subsequently be cancelled by the employer.
    /// @param approver Approver address; must authenticate.
    /// @param adjustment_id Adjustment identifier.
    ///
    /// # Panics
    /// * `"Contract not initialized"`
    /// * `"Only approver can reject"`
    /// * `"Adjustment is not pending"`
    ///
    /// # Events
    /// Emits `("adjustment_rejected", adjustment_id)` with an `AdjustmentRejectedEvent`.
    pub fn reject_adjustment(env: Env, approver: Address, adjustment_id: u128) {
        require_initialized(&env);
        approver.require_auth();

        let mut adjustment = read_adjustment(&env, adjustment_id);
        assert!(adjustment.approver == approver, "Only approver can reject");
        assert!(
            adjustment.status == AdjustmentStatus::Pending,
            "Adjustment is not pending"
        );

        adjustment.status = AdjustmentStatus::Rejected;
        write_adjustment(&env, &adjustment);

        env.events().publish(
            ("adjustment_rejected", adjustment_id),
            AdjustmentRejectedEvent {
                adjustment_id,
                approver: approver.clone(),
            },
        );
        append_audit(
            &env,
            approver,
            Symbol::new(&env, "adjustment_rejected"),
            Some(adjustment_id),
            Some(adjustment.employee),
            Some(adjustment.new_salary),
            adjustment.reason_hash,
        );
    }

    /// @notice Applies an approved salary adjustment after the effective date.
    /// @dev Only the employer can apply. Ledger timestamp must be at or past effective_date.
    ///      Updates the employee's tracked salary for payroll claiming visibility.
    ///
    /// @param employer Employer applying the adjustment; must authenticate.
    /// @param adjustment_id Adjustment identifier.
    ///
    /// # Panics
    /// * `"Contract not initialized"`
    /// * `"Only employer can apply"`
    /// * `"Adjustment is not approved"`
    /// * `"Effective date not reached"`
    ///
    /// # Events
    /// Emits `("adjustment_applied", adjustment_id)` with an `AdjustmentAppliedEvent`.
    pub fn apply_adjustment(env: Env, employer: Address, adjustment_id: u128) {
        require_initialized(&env);
        employer.require_auth();

        let mut adjustment = read_adjustment(&env, adjustment_id);
        assert!(adjustment.employer == employer, "Only employer can apply");
        assert!(
            adjustment.status == AdjustmentStatus::Approved,
            "Adjustment is not approved"
        );

        let now = env.ledger().timestamp();
        assert!(
            now >= adjustment.effective_date,
            "Effective date not reached"
        );

        adjustment.status = AdjustmentStatus::Applied;
        write_adjustment(&env, &adjustment);

        // Update tracked salary so payroll claiming logic can read the latest effective salary.
        env.storage().persistent().set(
            &StorageKey::EmployeeSalary(adjustment.employee.clone()),
            &adjustment.new_salary,
        );

        env.events().publish(
            ("adjustment_applied", adjustment_id),
            AdjustmentAppliedEvent {
                adjustment_id,
                employee: adjustment.employee.clone(),
                new_salary: adjustment.new_salary,
            },
        );
        append_audit(
            &env,
            employer,
            Symbol::new(&env, "adjustment_applied"),
            Some(adjustment_id),
            Some(adjustment.employee),
            Some(adjustment.new_salary),
            adjustment.reason_hash,
        );
    }

    /// @notice Cancels a pending or rejected salary adjustment.
    /// @dev Approved adjustments cannot be cancelled to preserve scheduling guarantees.
    /// @param employer Employer requesting cancellation; must authenticate.
    /// @param adjustment_id Adjustment identifier.
    ///
    /// # Panics
    /// * `"Contract not initialized"`
    /// * `"Only employer can cancel"`
    /// * `"Adjustment cannot be cancelled"`
    ///
    /// # Events
    /// Emits `("adjustment_cancelled", adjustment_id)` with an `AdjustmentCancelledEvent`.
    pub fn cancel_adjustment(env: Env, employer: Address, adjustment_id: u128) {
        require_initialized(&env);
        employer.require_auth();

        let mut adjustment = read_adjustment(&env, adjustment_id);
        assert!(adjustment.employer == employer, "Only employer can cancel");
        assert!(
            adjustment.status == AdjustmentStatus::Pending
                || adjustment.status == AdjustmentStatus::Rejected,
            "Adjustment cannot be cancelled"
        );

        adjustment.status = AdjustmentStatus::Cancelled;
        write_adjustment(&env, &adjustment);

        env.events().publish(
            ("adjustment_cancelled", adjustment_id),
            AdjustmentCancelledEvent {
                adjustment_id,
                employer: employer.clone(),
            },
        );
        append_audit(
            &env,
            employer,
            Symbol::new(&env, "adjustment_cancelled"),
            Some(adjustment_id),
            Some(adjustment.employee),
            Some(adjustment.new_salary),
            adjustment.reason_hash,
        );
    }

    /// @notice Returns a stored salary adjustment by id.
    /// @param adjustment_id Adjustment identifier.
    /// @return `Option<SalaryAdjustment>` — `None` if not found.
    pub fn get_adjustment(env: Env, adjustment_id: u128) -> Option<SalaryAdjustment> {
        env.storage()
            .persistent()
            .get(&StorageKey::Adjustment(adjustment_id))
    }

    /// @notice Returns the contract owner address.
    /// @return `Option<Address>` — `None` before initialization.
    pub fn get_owner(env: Env) -> Option<Address> {
        env.storage().persistent().get(&StorageKey::Owner)
    }

    /// @notice Returns the active salary cap.
    /// @dev Returns `DEFAULT_MAX_SALARY` if no explicit cap has been set.
    /// @return i128 The salary cap in stroops.
    pub fn get_salary_cap(env: Env) -> i128 {
        effective_salary_cap(&env)
    }

    /// @notice Returns the last applied salary for an employee.
    /// @dev Intended for payroll claiming logic to determine the current effective salary.
    ///      Returns `None` until at least one adjustment has been applied for the employee.
    ///
    /// @param employee Employee address to query.
    /// @return `Option<i128>` — `None` if no adjustment has been applied yet.
    pub fn get_employee_salary(env: Env, employee: Address) -> Option<i128> {
        env.storage()
            .persistent()
            .get(&StorageKey::EmployeeSalary(employee))
    }

    /// @notice Returns an append-only audit record by id.
    /// @param audit_id Audit record identifier.
    pub fn get_audit_log(env: Env, audit_id: u128) -> Option<SalaryAdjustmentAuditEntry> {
        env.storage()
            .persistent()
            .get(&StorageKey::AuditLog(audit_id))
    }

    /// @notice Returns the number of audit records written so far.
    pub fn get_audit_log_count(env: Env) -> u128 {
        env.storage()
            .persistent()
            .get::<_, u128>(&StorageKey::NextAuditLogId)
            .unwrap_or(0)
    }
}
