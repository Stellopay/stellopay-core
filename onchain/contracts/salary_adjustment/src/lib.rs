#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

#[contract]
pub struct SalaryAdjustmentContract;

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

#[contracttype]
#[derive(Clone)]
enum StorageKey {
    Initialized,
    Owner,
    NextAdjustmentId,
    Adjustment(u128),
}

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

#[contractimpl]
impl SalaryAdjustmentContract {
    /// @notice Initializes the salary adjustment contract.
    /// @dev Can only be executed once.
    /// @param owner Address allowed to run owner-level actions.
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

    /// @notice Creates a salary adjustment request.
    /// @dev Determines increase or decrease from salary comparison.
    /// @param employer Employer submitting the adjustment; must authenticate.
    /// @param employee Employee whose salary is being adjusted.
    /// @param approver Address that can approve or reject.
    /// @param current_salary Current salary amount.
    /// @param new_salary Proposed new salary amount.
    /// @param effective_date Timestamp when the adjustment takes effect.
    /// @return u128
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

        let kind = if new_salary > current_salary {
            AdjustmentKind::Increase
        } else {
            AdjustmentKind::Decrease
        };

        let adjustment_id = next_adjustment_id(&env);
        let created_at = env.ledger().timestamp();

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
            created_at,
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
    /// @param approver Approver address.
    /// @param adjustment_id Adjustment identifier.
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
    /// @dev Rejected adjustments can be cancelled by employer.
    /// @param approver Approver address.
    /// @param adjustment_id Adjustment identifier.
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
    /// @param employer Employer applying the adjustment.
    /// @param adjustment_id Adjustment identifier.
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
    /// @dev Approved adjustments cannot be cancelled.
    /// @param employer Employer requesting cancellation.
    /// @param adjustment_id Adjustment identifier.
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

    /// @notice Reads a stored salary adjustment by id.
    /// @param adjustment_id Adjustment identifier.
    /// @return `Option<SalaryAdjustment>`
    /// @dev Requires caller authentication
    pub fn get_adjustment(env: Env, adjustment_id: u128) -> Option<SalaryAdjustment> {
        env.storage()
            .persistent()
            .get(&StorageKey::Adjustment(adjustment_id))
    }

    /// @notice Returns contract owner.
    /// @dev Requires caller authentication
    pub fn get_owner(env: Env) -> Option<Address> {
        env.storage().persistent().get(&StorageKey::Owner)
    }
}
