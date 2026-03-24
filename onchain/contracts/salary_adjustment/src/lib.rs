#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

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

// ============================================================================
// CONSTANTS
// ============================================================================

/// Default maximum allowable salary (1 trillion stroops) used when no
/// explicit cap has been configured by the owner.
pub const DEFAULT_MAX_SALARY: i128 = 1_000_000_000_000;

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

/// Returns the configured salary cap, falling back to DEFAULT_MAX_SALARY.
fn effective_salary_cap(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get::<_, i128>(&StorageKey::SalaryCap)
        .unwrap_or(DEFAULT_MAX_SALARY)
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

        env.storage()
            .persistent()
            .set(&StorageKey::SalaryCap, &cap);

        env.events()
            .publish(("salary_cap_set", cap), SalaryCapSetEvent { owner, cap });
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
        assert!(current_salary > 0, "Current salary must be positive");
        assert!(new_salary > 0, "New salary must be positive");
        assert!(
            new_salary != current_salary,
            "New salary must differ from current salary"
        );

        let cap = effective_salary_cap(&env);
        assert!(new_salary <= cap, "New salary exceeds salary cap");

        let now = env.ledger().timestamp();
        assert!(
            effective_date >= now,
            "Effective date cannot be in the past"
        );

        let kind = if new_salary > current_salary {
            AdjustmentKind::Increase
        } else {
            AdjustmentKind::Decrease
        };

        let adjustment_id = next_adjustment_id(&env);

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
            created_at: now,
        };

        write_adjustment(&env, &adjustment);
        env.events().publish(
            ("adjustment_created", adjustment_id),
            AdjustmentCreatedEvent {
                adjustment_id,
                employer,
                employee,
                kind,
                current_salary,
                new_salary,
                effective_date,
            },
        );

        adjustment_id
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
                approver,
            },
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
                approver,
            },
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
        env.storage()
            .persistent()
            .set(&StorageKey::EmployeeSalary(adjustment.employee.clone()), &adjustment.new_salary);

        env.events().publish(
            ("adjustment_applied", adjustment_id),
            AdjustmentAppliedEvent {
                adjustment_id,
                employee: adjustment.employee.clone(),
                new_salary: adjustment.new_salary,
            },
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
                employer,
            },
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
}
