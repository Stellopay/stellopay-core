#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env};

#[contract]
pub struct BonusSystemContract;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IncentiveKind {
    OneTime,
    Recurring,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Cancelled,
    Completed,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Incentive {
    pub id: u128,
    pub employer: Address,
    pub employee: Address,
    pub approver: Address,
    pub token: Address,
    pub kind: IncentiveKind,
    pub status: ApprovalStatus,
    pub amount_per_payout: i128,
    pub total_payouts: u32,
    pub claimed_payouts: u32,
    pub start_time: u64,
    pub interval_seconds: u64,
}

#[contracttype]
#[derive(Clone)]
enum StorageKey {
    Initialized,
    Owner,
    NextIncentiveId,
    Incentive(u128),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncentiveCreatedEvent {
    pub incentive_id: u128,
    pub employer: Address,
    pub employee: Address,
    pub approver: Address,
    pub token: Address,
    pub kind: IncentiveKind,
    pub escrowed_amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncentiveApprovedEvent {
    pub incentive_id: u128,
    pub approver: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncentiveRejectedEvent {
    pub incentive_id: u128,
    pub approver: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncentiveClaimedEvent {
    pub incentive_id: u128,
    pub employee: Address,
    pub payouts_claimed: u32,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncentiveCancelledEvent {
    pub incentive_id: u128,
    pub employer: Address,
    pub refunded_amount: i128,
}

fn require_initialized(env: &Env) {
    let initialized = env
        .storage()
        .persistent()
        .get::<_, bool>(&StorageKey::Initialized)
        .unwrap_or(false);
    assert!(initialized, "Contract not initialized");
}

fn read_incentive(env: &Env, incentive_id: u128) -> Incentive {
    env.storage()
        .persistent()
        .get::<_, Incentive>(&StorageKey::Incentive(incentive_id))
        .expect("Incentive not found")
}

fn write_incentive(env: &Env, incentive: &Incentive) {
    env.storage()
        .persistent()
        .set(&StorageKey::Incentive(incentive.id), incentive);
}

fn next_incentive_id(env: &Env) -> u128 {
    let current = env
        .storage()
        .persistent()
        .get::<_, u128>(&StorageKey::NextIncentiveId)
        .unwrap_or(0);
    let next = current.checked_add(1).expect("Incentive id overflow");
    env.storage()
        .persistent()
        .set(&StorageKey::NextIncentiveId, &next);
    next
}

fn checked_mul_amount(amount_per_payout: i128, payouts: u32) -> i128 {
    amount_per_payout
        .checked_mul(i128::from(payouts))
        .expect("Amount overflow")
}

fn vested_payouts(now: u64, start_time: u64, interval_seconds: u64, total_payouts: u32) -> u32 {
    if now < start_time {
        return 0;
    }

    let elapsed = now - start_time;
    let raw = elapsed
        .checked_div(interval_seconds)
        .and_then(|cycles| cycles.checked_add(1))
        .unwrap_or(u64::MAX);

    if raw > u64::from(total_payouts) {
        total_payouts
    } else {
        raw as u32
    }
}

#[contractimpl]
impl BonusSystemContract {
    /// @notice Initializes the bonus and incentive contract.
    /// @dev Can only be executed once and stores the owner for future admin operations.
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
        env.storage().persistent().set(&StorageKey::Initialized, &true);
    }

    /// @notice Creates a one-time bonus with escrowed funds.
    /// @dev Employer funds the contract immediately and approver must approve before claim.
    /// @param employer Employer funding the bonus.
    /// @param employee Employee allowed to claim.
    /// @param approver Address that can approve or reject.
    /// @param token Token contract used for payout.
    /// @param amount Bonus amount.
    /// @param unlock_time Earliest claim timestamp.
    pub fn create_one_time_bonus(
        env: Env,
        employer: Address,
        employee: Address,
        approver: Address,
        token: Address,
        amount: i128,
        unlock_time: u64,
    ) -> u128 {
        require_initialized(&env);
        employer.require_auth();
        assert!(amount > 0, "Amount must be positive");

        let incentive_id = next_incentive_id(&env);
        let incentive = Incentive {
            id: incentive_id,
            employer: employer.clone(),
            employee: employee.clone(),
            approver: approver.clone(),
            token: token.clone(),
            kind: IncentiveKind::OneTime,
            status: ApprovalStatus::Pending,
            amount_per_payout: amount,
            total_payouts: 1,
            claimed_payouts: 0,
            start_time: unlock_time,
            interval_seconds: 0,
        };

        token::Client::new(&env, &token).transfer(&employer, &env.current_contract_address(), &amount);

        write_incentive(&env, &incentive);
        env.events().publish(
            ("incentive_created", incentive_id),
            IncentiveCreatedEvent {
                incentive_id,
                employer,
                employee,
                approver,
                token,
                kind: IncentiveKind::OneTime,
                escrowed_amount: amount,
            },
        );

        incentive_id
    }

    /// @notice Creates a recurring incentive with escrowed funds for all scheduled payouts.
    /// @dev Total escrow = amount_per_payout * total_payouts.
    /// @param employer Employer funding the incentive.
    /// @param employee Employee allowed to claim accrued payouts.
    /// @param approver Address that can approve or reject.
    /// @param token Token contract used for payouts.
    /// @param amount_per_payout Amount per interval.
    /// @param total_payouts Number of payout intervals.
    /// @param start_time Timestamp when first payout becomes claimable.
    /// @param interval_seconds Number of seconds between payouts.
    pub fn create_recurring_incentive(
        env: Env,
        employer: Address,
        employee: Address,
        approver: Address,
        token: Address,
        amount_per_payout: i128,
        total_payouts: u32,
        start_time: u64,
        interval_seconds: u64,
    ) -> u128 {
        require_initialized(&env);
        employer.require_auth();
        assert!(amount_per_payout > 0, "Amount must be positive");
        assert!(total_payouts > 0, "Total payouts must be positive");
        assert!(interval_seconds > 0, "Interval must be positive");

        let escrowed_amount = checked_mul_amount(amount_per_payout, total_payouts);
        let incentive_id = next_incentive_id(&env);
        let incentive = Incentive {
            id: incentive_id,
            employer: employer.clone(),
            employee: employee.clone(),
            approver: approver.clone(),
            token: token.clone(),
            kind: IncentiveKind::Recurring,
            status: ApprovalStatus::Pending,
            amount_per_payout,
            total_payouts,
            claimed_payouts: 0,
            start_time,
            interval_seconds,
        };

        token::Client::new(&env, &token).transfer(&employer, &env.current_contract_address(), &escrowed_amount);

        write_incentive(&env, &incentive);
        env.events().publish(
            ("incentive_created", incentive_id),
            IncentiveCreatedEvent {
                incentive_id,
                employer,
                employee,
                approver,
                token,
                kind: IncentiveKind::Recurring,
                escrowed_amount,
            },
        );

        incentive_id
    }

    /// @notice Approves a pending incentive.
    /// @dev Only the configured approver can move status from Pending to Approved.
    /// @param approver Approver address.
    /// @param incentive_id Incentive identifier.
    pub fn approve_incentive(env: Env, approver: Address, incentive_id: u128) {
        require_initialized(&env);
        approver.require_auth();

        let mut incentive = read_incentive(&env, incentive_id);
        assert!(incentive.approver == approver, "Only approver can approve");
        assert!(incentive.status == ApprovalStatus::Pending, "Incentive is not pending");

        incentive.status = ApprovalStatus::Approved;
        write_incentive(&env, &incentive);

        env.events().publish(
            ("incentive_approved", incentive_id),
            IncentiveApprovedEvent {
                incentive_id,
                approver,
            },
        );
    }

    /// @notice Rejects a pending incentive.
    /// @dev Rejected incentives can be cancelled by employer for full refund.
    /// @param approver Approver address.
    /// @param incentive_id Incentive identifier.
    pub fn reject_incentive(env: Env, approver: Address, incentive_id: u128) {
        require_initialized(&env);
        approver.require_auth();

        let mut incentive = read_incentive(&env, incentive_id);
        assert!(incentive.approver == approver, "Only approver can reject");
        assert!(incentive.status == ApprovalStatus::Pending, "Incentive is not pending");

        incentive.status = ApprovalStatus::Rejected;
        write_incentive(&env, &incentive);

        env.events().publish(
            ("incentive_rejected", incentive_id),
            IncentiveRejectedEvent {
                incentive_id,
                approver,
            },
        );
    }

    /// @notice Claims currently available payouts for an approved incentive.
    /// @dev One-time bonus claims exactly one payout after unlock. Recurring claims all accrued payouts.
    /// @param employee Employee claiming funds.
    /// @param incentive_id Incentive identifier.
    /// @return amount Claimed token amount.
    pub fn claim_incentive(env: Env, employee: Address, incentive_id: u128) -> i128 {
        require_initialized(&env);
        employee.require_auth();

        let mut incentive = read_incentive(&env, incentive_id);
        assert!(incentive.employee == employee, "Only employee can claim");
        assert!(incentive.status == ApprovalStatus::Approved, "Incentive is not approved");

        let now = env.ledger().timestamp();
        let payouts_to_claim = match incentive.kind {
            IncentiveKind::OneTime => {
                assert!(now >= incentive.start_time, "Bonus is still locked");
                assert!(incentive.claimed_payouts == 0, "Bonus already claimed");
                1
            }
            IncentiveKind::Recurring => {
                let vested = vested_payouts(
                    now,
                    incentive.start_time,
                    incentive.interval_seconds,
                    incentive.total_payouts,
                );
                let claimable = vested.saturating_sub(incentive.claimed_payouts);
                assert!(claimable > 0, "No payouts available");
                claimable
            }
        };

        let amount = checked_mul_amount(incentive.amount_per_payout, payouts_to_claim);
        token::Client::new(&env, &incentive.token).transfer(&env.current_contract_address(), &employee, &amount);

        incentive.claimed_payouts = incentive
            .claimed_payouts
            .checked_add(payouts_to_claim)
            .expect("Payout counter overflow");

        if incentive.claimed_payouts == incentive.total_payouts {
            incentive.status = ApprovalStatus::Completed;
        }

        write_incentive(&env, &incentive);
        env.events().publish(
            ("incentive_claimed", incentive_id),
            IncentiveClaimedEvent {
                incentive_id,
                employee,
                payouts_claimed: payouts_to_claim,
                amount,
            },
        );

        amount
    }

    /// @notice Cancels a pending or rejected incentive and refunds remaining escrow.
    /// @dev Approved incentives cannot be cancelled to preserve payout guarantees.
    /// @param employer Employer requesting cancellation.
    /// @param incentive_id Incentive identifier.
    /// @return refunded_amount Refunded token amount.
    pub fn cancel_incentive(env: Env, employer: Address, incentive_id: u128) -> i128 {
        require_initialized(&env);
        employer.require_auth();

        let mut incentive = read_incentive(&env, incentive_id);
        assert!(incentive.employer == employer, "Only employer can cancel");
        assert!(
            incentive.status == ApprovalStatus::Pending || incentive.status == ApprovalStatus::Rejected,
            "Incentive cannot be cancelled"
        );

        let remaining_payouts = incentive
            .total_payouts
            .checked_sub(incentive.claimed_payouts)
            .expect("Invalid payout state");
        let refunded_amount = checked_mul_amount(incentive.amount_per_payout, remaining_payouts);

        incentive.status = ApprovalStatus::Cancelled;
        write_incentive(&env, &incentive);

        token::Client::new(&env, &incentive.token)
            .transfer(&env.current_contract_address(), &employer, &refunded_amount);

        env.events().publish(
            ("incentive_cancelled", incentive_id),
            IncentiveCancelledEvent {
                incentive_id,
                employer,
                refunded_amount,
            },
        );

        refunded_amount
    }

    /// @notice Reads a stored incentive by id.
    /// @param incentive_id Incentive identifier.
    /// @return incentive Optional incentive object.
    pub fn get_incentive(env: Env, incentive_id: u128) -> Option<Incentive> {
        env.storage()
            .persistent()
            .get(&StorageKey::Incentive(incentive_id))
    }

    /// @notice Returns claimable payout count at the current ledger timestamp.
    /// @dev Returns zero unless incentive is approved.
    /// @param incentive_id Incentive identifier.
    /// @return payouts Number of payouts currently claimable.
    pub fn get_claimable_payouts(env: Env, incentive_id: u128) -> u32 {
        let incentive = match env
            .storage()
            .persistent()
            .get::<_, Incentive>(&StorageKey::Incentive(incentive_id))
        {
            Some(value) => value,
            None => return 0,
        };

        if incentive.status != ApprovalStatus::Approved {
            return 0;
        }

        let now = env.ledger().timestamp();
        match incentive.kind {
            IncentiveKind::OneTime => {
                if now >= incentive.start_time && incentive.claimed_payouts == 0 {
                    1
                } else {
                    0
                }
            }
            IncentiveKind::Recurring => {
                let vested = vested_payouts(
                    now,
                    incentive.start_time,
                    incentive.interval_seconds,
                    incentive.total_payouts,
                );
                vested.saturating_sub(incentive.claimed_payouts)
            }
        }
    }

    /// @notice Returns contract owner.
    pub fn get_owner(env: Env) -> Option<Address> {
        env.storage().persistent().get(&StorageKey::Owner)
    }
}