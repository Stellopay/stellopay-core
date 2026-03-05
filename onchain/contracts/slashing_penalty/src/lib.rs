#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env, String,
};

// ============================================================================
// Errors
// ============================================================================

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PenaltyError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
    InvalidConfig = 4,
    PolicyNotFound = 5,
    PolicyInactive = 6,
    CapExceeded = 7,
    InvalidAmount = 8,
}

// ============================================================================
// Types
// ============================================================================

/// High-level reason for a slashing or penalty event.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PenaltyReason {
    LatePayment,
    BreachOfAgreement,
    PolicyViolation,
    Custom(u32),
}

/// Configuration for a penalty policy.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PenaltyPolicy {
    /// Stable policy identifier.
    pub id: u32,
    /// Optional agreement id this policy is scoped to (e.g. payroll agreement id).
    pub agreement_id: Option<u128>,
    /// Maximum penalty in basis points of the current locked value (10000 = 100%).
    pub max_penalty_bps: u32,
    /// Optional absolute cap in token units.
    pub absolute_cap: Option<i128>,
    /// Human-readable description (e.g. "L1 late payment policy").
    pub description: String,
    /// Whether the policy is currently active.
    pub is_active: bool,
}

/// Recorded slashing action for auditability.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlashingRecord {
    pub policy_id: u32,
    pub agreement_id: Option<u128>,
    pub offender: Address,
    pub beneficiary: Address,
    pub token: Address,
    pub amount: i128,
    pub reason: PenaltyReason,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Initialized,
    Owner,
    /// Optional operator allowed to execute slashing on behalf of owner.
    Operator,
    NextPolicyId,
    Policy(u32),
    /// Monotonic index for slashing records.
    NextRecordId,
    /// Record storage: record_id -> SlashingRecord.
    Record(u64),
}

#[contract]
pub struct SlashingPenaltyContract;

fn require_initialized(env: &Env) -> Result<(), PenaltyError> {
    let initialized = env
        .storage()
        .instance()
        .get::<_, bool>(&DataKey::Initialized)
        .unwrap_or(false);
    if !initialized {
        return Err(PenaltyError::NotInitialized);
    }
    Ok(())
}

fn read_owner(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Owner)
        .expect("owner not set")
}

fn is_operator(env: &Env, addr: &Address) -> bool {
    if let Some(op) = env
        .storage()
        .instance()
        .get::<_, Address>(&DataKey::Operator)
    {
        &op == addr
    } else {
        false
    }
}

fn require_admin(env: &Env, caller: &Address) -> Result<(), PenaltyError> {
    require_initialized(env)?;
    caller.require_auth();
    let owner = read_owner(env);
    if &owner == caller || is_operator(env, caller) {
        Ok(())
    } else {
        Err(PenaltyError::NotAuthorized)
    }
}

fn next_policy_id(env: &Env) -> u32 {
    let current = env
        .storage()
        .instance()
        .get::<_, u32>(&DataKey::NextPolicyId)
        .unwrap_or(1);
    let next = current.checked_add(1).expect("policy id overflow");
    env.storage().instance().set(&DataKey::NextPolicyId, &next);
    current
}

fn next_record_id(env: &Env) -> u64 {
    let current = env
        .storage()
        .instance()
        .get::<_, u64>(&DataKey::NextRecordId)
        .unwrap_or(1);
    let next = current.checked_add(1).expect("record id overflow");
    env.storage().instance().set(&DataKey::NextRecordId, &next);
    current
}

#[contractimpl]
impl SlashingPenaltyContract {
    /// @notice Initializes the slashing/penalty contract.
    /// @dev Must be called exactly once by the protocol owner.
    /// @param owner Administrative owner address for managing policies.
    /// @return Result<(), PenaltyError>
    /// @notice Returns an error on failure.
    pub fn initialize(env: Env, owner: Address) -> Result<(), PenaltyError> {
        if env
            .storage()
            .instance()
            .get::<_, bool>(&DataKey::Initialized)
            .unwrap_or(false)
        {
            return Err(PenaltyError::AlreadyInitialized);
        }

        owner.require_auth();
        env.storage().instance().set(&DataKey::Owner, &owner);
        env.storage().instance().set(&DataKey::Initialized, &true);
        env.storage().instance().set(&DataKey::NextPolicyId, &1u32);
        env.storage().instance().set(&DataKey::NextRecordId, &1u64);
        Ok(())
    }

    /// @notice Sets an optional operator that can execute slashing.
    /// @dev Useful for wiring this contract into dispute/escrow flows
    ///      where an arbiter or automation contract is delegated authority.
    /// @param caller Owner address authorizing the change.
    /// @param operator New operator address.
    /// @return Result<(), PenaltyError>
    /// @notice Returns an error on failure.
    pub fn set_operator(env: Env, caller: Address, operator: Address) -> Result<(), PenaltyError> {
        require_initialized(&env)?;
        caller.require_auth();
        let owner = read_owner(&env);
        if owner != caller {
            return Err(PenaltyError::NotAuthorized);
        }
        env.storage().instance().set(&DataKey::Operator, &operator);
        Ok(())
    }

    /// @notice Creates a new penalty policy with configurable caps.
    /// @dev Caps are expressed in basis points of the caller-supplied
    ///      `current_locked_amount` at slashing time, and/or as a hard
    ///      absolute cap in token units.
    /// @param caller Owner or operator configuring the policy.
    /// @param agreement_id Optional scoped agreement identifier.
    /// @param max_penalty_bps Maximum penalty in basis points (<= 10000).
    /// @param absolute_cap Optional absolute cap in token units.
    /// @param description Human-readable description / policy name.
    /// @return policy_id Newly created policy id.
    /// @notice Returns an error on failure.
    pub fn create_policy(
        env: Env,
        caller: Address,
        agreement_id: Option<u128>,
        max_penalty_bps: u32,
        absolute_cap: Option<i128>,
        description: String,
    ) -> Result<u32, PenaltyError> {
        require_admin(&env, &caller)?;

        if max_penalty_bps == 0 || max_penalty_bps > 10_000 {
            return Err(PenaltyError::InvalidConfig);
        }

        if let Some(cap) = absolute_cap {
            if cap <= 0 {
                return Err(PenaltyError::InvalidConfig);
            }
        }

        let id = next_policy_id(&env);
        let policy = PenaltyPolicy {
            id,
            agreement_id,
            max_penalty_bps,
            absolute_cap,
            description,
            is_active: true,
        };

        env.storage().instance().set(&DataKey::Policy(id), &policy);

        Ok(id)
    }

    /// @notice Activates or deactivates an existing policy.
    /// @param caller Owner or operator.
    /// @param policy_id Policy identifier.
    /// @param is_active New active flag value.
    /// @return Result<(), PenaltyError>
    /// @notice Returns an error on failure.
    /// @dev Requires caller authentication
    pub fn set_policy_active(
        env: Env,
        caller: Address,
        policy_id: u32,
        is_active: bool,
    ) -> Result<(), PenaltyError> {
        require_admin(&env, &caller)?;

        let mut policy: PenaltyPolicy = env
            .storage()
            .instance()
            .get(&DataKey::Policy(policy_id))
            .ok_or(PenaltyError::PolicyNotFound)?;

        policy.is_active = is_active;
        env.storage()
            .instance()
            .set(&DataKey::Policy(policy_id), &policy);
        Ok(())
    }

    /// @notice Reads a policy by id.
    /// @param policy_id policy_id parameter
    /// @return `Option<PenaltyPolicy>`
    /// @dev Requires caller authentication
    pub fn get_policy(env: Env, policy_id: u32) -> Option<PenaltyPolicy> {
        env.storage().instance().get(&DataKey::Policy(policy_id))
    }

    /// @notice Executes a slashing action under a configured policy.
    /// @dev This is designed to integrate with dispute and escrow flows.
    ///      Callers must provide the current locked amount for the agreement;
    ///      the contract enforces that the slash amount is within the
    ///      configured percentage and absolute caps, and optionally transfers
    ///      tokens if this contract holds escrow funds.
    /// @param caller Owner or delegated operator (e.g. arbiter contract).
    /// @param policy_id Policy identifier to use.
    /// @param offender Address being penalized (for audit purposes).
    /// @param beneficiary Address receiving the slashed funds.
    /// @param token Token contract for settlement.
    /// @param agreement_id Optional agreement id in the core payroll contract.
    /// @param amount Slashing amount in token units.
    /// @param current_locked_amount Locked amount against which caps are evaluated.
    /// @param reason High-level reason for the penalty.
    /// @return Result<(), PenaltyError>
    /// @notice Returns an error on failure.
    pub fn slash(
        env: Env,
        caller: Address,
        policy_id: u32,
        offender: Address,
        beneficiary: Address,
        token: Address,
        agreement_id: Option<u128>,
        amount: i128,
        current_locked_amount: i128,
        reason: PenaltyReason,
    ) -> Result<(), PenaltyError> {
        require_admin(&env, &caller)?;

        if amount <= 0 || current_locked_amount <= 0 {
            return Err(PenaltyError::InvalidAmount);
        }
        if amount > current_locked_amount {
            return Err(PenaltyError::CapExceeded);
        }

        let policy: PenaltyPolicy = env
            .storage()
            .instance()
            .get(&DataKey::Policy(policy_id))
            .ok_or(PenaltyError::PolicyNotFound)?;

        if !policy.is_active {
            return Err(PenaltyError::PolicyInactive);
        }

        if let Some(expected_agreement) = policy.agreement_id {
            if Some(expected_agreement) != agreement_id {
                return Err(PenaltyError::InvalidConfig);
            }
        }

        // Percentage cap: amount <= max_penalty_bps / 10000 * current_locked_amount
        let max_from_bps =
            (i128::from(policy.max_penalty_bps) * current_locked_amount) / 10_000i128;
        if amount > max_from_bps {
            return Err(PenaltyError::CapExceeded);
        }

        // Optional absolute cap.
        if let Some(cap) = policy.absolute_cap {
            if amount > cap {
                return Err(PenaltyError::CapExceeded);
            }
        }

        // Attempt settlement from this contract's escrow, if funded.
        let client = token::Client::new(&env, &token);
        let contract_balance = client.balance(&env.current_contract_address());
        if contract_balance >= amount {
            client.transfer(&env.current_contract_address(), &beneficiary, &amount);
        }

        // Persist slashing record for auditability.
        let record_id = next_record_id(&env);
        let record = SlashingRecord {
            policy_id,
            agreement_id,
            offender: offender.clone(),
            beneficiary: beneficiary.clone(),
            token: token.clone(),
            amount,
            reason: reason.clone(),
            timestamp: env.ledger().timestamp(),
        };

        env.storage()
            .instance()
            .set(&DataKey::Record(record_id), &record);

        // Emit an indexable event for off-chain consumers.
        env.events().publish(
            (symbol_short!("slash"), policy_id),
            (
                record_id,
                agreement_id,
                amount,
                offender,
                beneficiary,
                token,
                reason,
            ),
        );

        Ok(())
    }

    /// @notice Reads a slashing record by id.
    /// @param record_id record_id parameter
    /// @return `Option<SlashingRecord>`
    /// @dev Requires caller authentication
    pub fn get_record(env: Env, record_id: u64) -> Option<SlashingRecord> {
        env.storage().instance().get(&DataKey::Record(record_id))
    }

    /// @notice Returns the configured owner.
    /// @dev Requires caller authentication
    pub fn get_owner(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Owner)
    }

    /// @notice Returns the configured operator, if any.
    /// @dev Requires caller authentication
    pub fn get_operator(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Operator)
    }
}
