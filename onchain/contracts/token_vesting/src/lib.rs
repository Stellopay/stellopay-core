#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Vec};

#[contract]
pub struct TokenVestingContract;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VestingKind {
    /// Linearly increasing vesting between start and end timestamps.
    Linear,
    /// Entire amount becomes available at `cliff_time`, nothing before.
    Cliff,
    /// Custom step schedule based on explicit checkpoints.
    Custom,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VestingStatus {
    Active,
    Revoked,
    Completed,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomCheckpoint {
    /// Absolute timestamp at which `cumulative_amount` becomes vested.
    pub time: u64,
    /// Cumulative vested amount at this timestamp.
    pub cumulative_amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VestingSchedule {
    pub id: u128,
    pub employer: Address,
    pub beneficiary: Address,
    pub token: Address,
    pub kind: VestingKind,
    pub total_amount: i128,
    pub released_amount: i128,
    pub start_time: u64,
    pub end_time: u64,
    pub cliff_time: Option<u64>,
    pub checkpoints: Vec<CustomCheckpoint>,
    pub status: VestingStatus,
    pub revocable: bool,
    pub revoked_at: Option<u64>,
}

#[contracttype]
#[derive(Clone)]
enum StorageKey {
    Initialized,
    Owner,
    NextScheduleId,
    Schedule(u128),
}

fn require_initialized(env: &Env) {
    let initialized = env
        .storage()
        .persistent()
        .get::<_, bool>(&StorageKey::Initialized)
        .unwrap_or(false);
    assert!(initialized, "Contract not initialized");
}

fn read_owner(env: &Env) -> Address {
    env.storage()
        .persistent()
        .get::<_, Address>(&StorageKey::Owner)
        .expect("Owner not set")
}

fn next_schedule_id(env: &Env) -> u128 {
    let current = env
        .storage()
        .persistent()
        .get::<_, u128>(&StorageKey::NextScheduleId)
        .unwrap_or(0);
    let next = current.checked_add(1).expect("Schedule id overflow");
    env.storage()
        .persistent()
        .set(&StorageKey::NextScheduleId, &next);
    next
}

fn read_schedule(env: &Env, id: u128) -> VestingSchedule {
    env.storage()
        .persistent()
        .get::<_, VestingSchedule>(&StorageKey::Schedule(id))
        .expect("Schedule not found")
}

fn write_schedule(env: &Env, schedule: &VestingSchedule) {
    env.storage()
        .persistent()
        .set(&StorageKey::Schedule(schedule.id), schedule);
}

fn compute_vested_amount(now: u64, schedule: &VestingSchedule) -> i128 {
    if schedule.total_amount <= 0 {
        return 0;
    }

    let effective_now = match schedule.status {
        VestingStatus::Revoked => schedule.revoked_at.unwrap_or(now),
        _ => now,
    };

    match schedule.kind {
        VestingKind::Linear => {
            if effective_now <= schedule.start_time {
                0
            } else if effective_now >= schedule.end_time {
                schedule.total_amount
            } else {
                let elapsed = effective_now - schedule.start_time;
                let duration = schedule.end_time - schedule.start_time;
                if duration == 0 {
                    schedule.total_amount
                } else {
                    // Linear interpolation: total * elapsed / duration
                    (schedule.total_amount * i128::from(elapsed as i64))
                        / i128::from(duration as i64)
                }
            }
        }
        VestingKind::Cliff => match schedule.cliff_time {
            Some(cliff) if effective_now >= cliff => schedule.total_amount,
            _ => 0,
        },
        VestingKind::Custom => {
            if schedule.checkpoints.len() == 0 {
                return 0;
            }
            let mut last_amount: i128 = 0;
            for i in 0..schedule.checkpoints.len() {
                let cp = schedule.checkpoints.get(i).unwrap();
                if effective_now >= cp.time {
                    last_amount = cp.cumulative_amount;
                } else {
                    break;
                }
            }
            if last_amount > schedule.total_amount {
                schedule.total_amount
            } else {
                last_amount
            }
        }
    }
}

fn compute_releasable(now: u64, schedule: &VestingSchedule) -> i128 {
    let vested = compute_vested_amount(now, schedule);
    let mut releasable = vested.checked_sub(schedule.released_amount).unwrap_or(0);
    if releasable < 0 {
        releasable = 0;
    }
    releasable
}

#[contractimpl]
impl TokenVestingContract {
    /// @notice Initializes the token vesting contract.
    /// @dev Must be called once by the admin/owner.
    /// @param owner Address allowed to perform admin operations such as
    ///        approving early releases.
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

    /// @notice Creates a linear vesting schedule.
    /// @dev Employer escrows the full `total_amount` at creation time.
    /// @param employer Funding address; must authenticate.
    /// @param beneficiary Employee/recipient of vested tokens.
    /// @param token Token contract address used for vesting.
    /// @param total_amount Total number of tokens to vest.
    /// @param start_time Vesting start timestamp.
    /// @param end_time Vesting end timestamp (must be > start_time).
    /// @param cliff_time Optional cliff timestamp.
    /// @param revocable Whether employer can revoke this schedule.
    pub fn create_linear_schedule(
        env: Env,
        employer: Address,
        beneficiary: Address,
        token: Address,
        total_amount: i128,
        start_time: u64,
        end_time: u64,
        cliff_time: Option<u64>,
        revocable: bool,
    ) -> u128 {
        require_initialized(&env);
        employer.require_auth();

        assert!(total_amount > 0, "Total amount must be positive");
        assert!(end_time > start_time, "End time must be after start time");

        if let Some(cliff) = cliff_time {
            assert!(
                cliff >= start_time && cliff <= end_time,
                "Cliff must be within [start, end]"
            );
        }

        // Escrow tokens in the vesting contract.
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&employer, &env.current_contract_address(), &total_amount);

        let id = next_schedule_id(&env);
        let schedule = VestingSchedule {
            id,
            employer,
            beneficiary,
            token,
            kind: VestingKind::Linear,
            total_amount,
            released_amount: 0,
            start_time,
            end_time,
            cliff_time,
            checkpoints: Vec::new(&env),
            status: VestingStatus::Active,
            revocable,
            revoked_at: None,
        };
        write_schedule(&env, &schedule);
        id
    }

    /// @notice Creates a cliff vesting schedule.
    /// @dev All tokens vest at `cliff_time`.
    pub fn create_cliff_schedule(
        env: Env,
        employer: Address,
        beneficiary: Address,
        token: Address,
        total_amount: i128,
        cliff_time: u64,
        revocable: bool,
    ) -> u128 {
        require_initialized(&env);
        employer.require_auth();

        assert!(total_amount > 0, "Total amount must be positive");

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&employer, &env.current_contract_address(), &total_amount);

        let id = next_schedule_id(&env);
        let schedule = VestingSchedule {
            id,
            employer,
            beneficiary,
            token,
            kind: VestingKind::Cliff,
            total_amount,
            released_amount: 0,
            start_time: cliff_time,
            end_time: cliff_time,
            cliff_time: Some(cliff_time),
            checkpoints: Vec::new(&env),
            status: VestingStatus::Active,
            revocable,
            revoked_at: None,
        };
        write_schedule(&env, &schedule);
        id
    }

    /// @notice Creates a custom vesting schedule with arbitrary checkpoints.
    /// @dev `checkpoints` must be sorted by `time` and end at `total_amount`.
    pub fn create_custom_schedule(
        env: Env,
        employer: Address,
        beneficiary: Address,
        token: Address,
        total_amount: i128,
        checkpoints: Vec<CustomCheckpoint>,
        revocable: bool,
    ) -> u128 {
        require_initialized(&env);
        employer.require_auth();

        assert!(total_amount > 0, "Total amount must be positive");
        assert!(checkpoints.len() > 0, "At least one checkpoint required");

        let mut last_time: u64 = 0;
        let mut last_amount: i128 = 0;
        for i in 0..checkpoints.len() {
            let cp = checkpoints.get(i).unwrap();
            assert!(cp.time >= last_time, "Checkpoints must be sorted");
            assert!(
                cp.cumulative_amount >= last_amount,
                "Checkpoint amounts must be non-decreasing"
            );
            last_time = cp.time;
            last_amount = cp.cumulative_amount;
        }
        assert!(
            last_amount == total_amount,
            "Last checkpoint must equal total_amount"
        );

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&employer, &env.current_contract_address(), &total_amount);

        let id = next_schedule_id(&env);
        let schedule = VestingSchedule {
            id,
            employer,
            beneficiary,
            token,
            kind: VestingKind::Custom,
            total_amount,
            released_amount: 0,
            start_time: checkpoints.get(0).unwrap().time,
            end_time: last_time,
            cliff_time: None,
            checkpoints,
            status: VestingStatus::Active,
            revocable,
            revoked_at: None,
        };
        write_schedule(&env, &schedule);
        id
    }

    /// @notice Claims any vested but unreleased tokens for a schedule.
    /// @param beneficiary Schedule beneficiary; must authenticate.
    /// @param schedule_id Vesting schedule identifier.
    /// @return amount Claimed token amount.
    pub fn claim(env: Env, beneficiary: Address, schedule_id: u128) -> i128 {
        require_initialized(&env);
        beneficiary.require_auth();

        let mut schedule = read_schedule(&env, schedule_id);
        assert!(
            schedule.beneficiary == beneficiary,
            "Only beneficiary can claim"
        );
        assert!(
            schedule.status != VestingStatus::Completed,
            "Schedule already completed"
        );

        let now = env.ledger().timestamp();
        let amount = compute_releasable(now, &schedule);
        assert!(amount > 0, "Nothing to claim");

        let token_client = token::Client::new(&env, &schedule.token);
        token_client.transfer(&env.current_contract_address(), &beneficiary, &amount);

        schedule.released_amount = schedule
            .released_amount
            .checked_add(amount)
            .expect("Released amount overflow");

        if schedule.released_amount >= schedule.total_amount {
            schedule.status = VestingStatus::Completed;
        }

        write_schedule(&env, &schedule);
        amount
    }

    /// @notice Approves an early release of unvested tokens.
    /// @dev Only the contract owner (admin) can approve early releases.
    /// @param admin Contract owner; must authenticate.
    /// @param schedule_id Vesting schedule identifier.
    /// @param amount Maximum early release amount requested.
    /// @return released Actual amount released (capped at remaining unvested).
    pub fn approve_early_release(
        env: Env,
        admin: Address,
        schedule_id: u128,
        amount: i128,
    ) -> i128 {
        require_initialized(&env);
        admin.require_auth();

        let owner = read_owner(&env);
        assert!(admin == owner, "Only owner can approve early release");
        assert!(amount > 0, "Amount must be positive");

        let mut schedule = read_schedule(&env, schedule_id);
        assert!(
            schedule.status == VestingStatus::Active,
            "Schedule not active"
        );

        let now = env.ledger().timestamp();
        let vested = compute_vested_amount(now, &schedule);
        let unvested_remaining = schedule.total_amount.checked_sub(vested).unwrap_or(0);
        assert!(
            unvested_remaining > 0,
            "No unvested tokens remain for early release"
        );

        let release_amount = if amount > unvested_remaining {
            unvested_remaining
        } else {
            amount
        };

        let token_client = token::Client::new(&env, &schedule.token);
        token_client.transfer(
            &env.current_contract_address(),
            &schedule.beneficiary,
            &release_amount,
        );

        schedule.released_amount = schedule
            .released_amount
            .checked_add(release_amount)
            .expect("Released amount overflow");

        if schedule.released_amount >= schedule.total_amount {
            schedule.status = VestingStatus::Completed;
        }

        write_schedule(&env, &schedule);
        release_amount
    }

    /// @notice Revokes a revocable schedule for a terminated employee.
    /// @dev Employer recovers unvested tokens; vested portion remains claimable.
    /// @param employer Employer that created the schedule; must authenticate.
    /// @param schedule_id Vesting schedule identifier.
    /// @return refunded_amount Amount of unvested tokens refunded to employer.
    pub fn revoke(env: Env, employer: Address, schedule_id: u128) -> i128 {
        require_initialized(&env);
        employer.require_auth();

        let mut schedule = read_schedule(&env, schedule_id);
        assert!(schedule.employer == employer, "Only employer can revoke");
        assert!(schedule.revocable, "Schedule is not revocable");
        assert!(
            schedule.status == VestingStatus::Active,
            "Schedule not active"
        );

        let now = env.ledger().timestamp();
        let vested = compute_vested_amount(now, &schedule);
        let unvested = schedule.total_amount.checked_sub(vested).unwrap_or(0);
        assert!(unvested >= 0, "Invalid vesting state");

        if unvested > 0 {
            let token_client = token::Client::new(&env, &schedule.token);
            token_client.transfer(&env.current_contract_address(), &employer, &unvested);
        }

        schedule.status = VestingStatus::Revoked;
        schedule.revoked_at = Some(now);
        write_schedule(&env, &schedule);

        unvested
    }

    /// @notice Reads a vesting schedule by id.
    pub fn get_schedule(env: Env, schedule_id: u128) -> Option<VestingSchedule> {
        env.storage()
            .persistent()
            .get(&StorageKey::Schedule(schedule_id))
    }

    /// @notice Returns the amount currently vested for a schedule.
    pub fn get_vested_amount(env: Env, schedule_id: u128) -> i128 {
        let schedule = read_schedule(&env, schedule_id);
        let now = env.ledger().timestamp();
        compute_vested_amount(now, &schedule)
    }

    /// @notice Returns the currently releasable (claimable) amount.
    pub fn get_releasable_amount(env: Env, schedule_id: u128) -> i128 {
        let schedule = read_schedule(&env, schedule_id);
        let now = env.ledger().timestamp();
        compute_releasable(now, &schedule)
    }

    /// @notice Returns the contract owner/admin.
    pub fn get_owner(env: Env) -> Option<Address> {
        env.storage().persistent().get(&StorageKey::Owner)
    }
}
