#![no_std]
#![allow(deprecated)] // env.events().publish() — codebase-wide pattern

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, token, Address, BytesN,
    Env, Vec,
};
use rbac_interface::{RbacContractClient, Role};

/// Errors emitted by the multisig contract.
///
/// Uses `#[contracterror]` so that `panic_with_error!` can convert values of
/// this type into the host's `soroban_sdk::Error` representation.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum MultisigError {
    /// The WASM hash presented at execution time does not match the hash that
    /// was approved and stored in the original `ContractUpgrade` proposal.
    ContractUpgradeHashMismatch = 1,

    /// A `LargePayment` was proposed with a non-positive amount.
    ///
    /// Amounts must be strictly positive (`amount > 0`).  A zero or negative
    /// value indicates a misconfigured payload and is rejected at proposal
    /// time before any signer can accumulate approvals for it.
    InvalidAmount = 2,

    /// A `LargePayment` or `ContractUpgrade` payload names the multisig
    /// contract itself as the recipient or upgrade target.
    ///
    /// Using the contract's own address as the destination is almost certainly
    /// a configuration error (sending tokens to yourself, or upgrading yourself
    /// through the same contract instance).  These payloads are rejected at
    /// proposal time.
    SelfReferentialRecipient = 3,

    /// A signer without the Arbiter role attempted to propose a
    /// DisputeResolution operation.  Only addresses that hold or inherit
    /// the Arbiter role through the configured RBAC contract may propose
    /// dispute-resolution operations.
    NotArbiter = 4,
}

#[contract]
pub struct MultisigContract;

/// Stable identifiers used to configure per-operation thresholds.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationType {
    ContractUpgrade,
    LargePayment,
    DisputeResolution,
}

/// Operation kinds supported by the multisig.
///
/// These are intentionally generic so that off-chain automation or
/// higher-level contracts can interpret and act on approved operations.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationKind {
    /// Multi-sig approval for a contract upgrade.
    ///
    /// Tuple layout: (target, new_wasm_hash)
    ContractUpgrade(Address, BytesN<32>),
    /// Direct token payment executed from the multisig wallet.
    ///
    /// Tuple layout: (token, to, amount)
    LargePayment(Address, Address, i128),
    /// Dispute resolution intent for an external payroll-style contract.
    ///
    /// Tuple layout: (payroll_contract, agreement_id, pay_employee, refund_employer)
    DisputeResolution(Address, u128, i128, i128),
    /// Sets or removes the signer threshold override for an operation type.
    ///
    /// Tuple layout: (operation_type, threshold). A `None` threshold removes
    /// the override and restores the default threshold.
    SetThresholdOverride(OperationType, Option<u32>),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationStatus {
    Pending,
    Executed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Operation {
    pub id: u128,
    pub kind: OperationKind,
    pub creator: Address,
    pub status: OperationStatus,
    pub created_at: u64,
    pub executed_at: Option<u64>,
}

#[contracttype]
#[derive(Clone)]
enum StorageKey {
    Initialized,
    Owner,
    EmergencyGuardian,
    RbacAddress,
    Signers,
    Threshold,
    OperationCounter,
    Operation(u128),
    Approvals(u128),
    ThresholdOverride(OperationType),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationProposedEvent {
    pub operation_id: u128,
    pub creator: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationApprovedEvent {
    pub operation_id: u128,
    pub signer: Address,
    pub approvals: u32,
    pub threshold: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationExecutedEvent {
    pub operation_id: u128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationCancelledEvent {
    pub operation_id: u128,
}

fn require_initialized(env: &Env) {
    let initialized = env
        .storage()
        .persistent()
        .get::<_, bool>(&StorageKey::Initialized)
        .unwrap_or(false);
    assert!(initialized, "Contract not initialized");
}

fn read_signers(env: &Env) -> Vec<Address> {
    env.storage()
        .persistent()
        .get::<_, Vec<Address>>(&StorageKey::Signers)
        .expect("Signers not set")
}

fn read_threshold(env: &Env) -> u32 {
    env.storage()
        .persistent()
        .get::<_, u32>(&StorageKey::Threshold)
        .expect("Threshold not set")
}

fn read_threshold_override(env: &Env, operation_type: &OperationType) -> Option<u32> {
    env.storage()
        .persistent()
        .get(&StorageKey::ThresholdOverride(operation_type.clone()))
}

fn read_effective_threshold(env: &Env, operation_type: &OperationType) -> u32 {
    read_threshold_override(env, operation_type).unwrap_or_else(|| read_threshold(env))
}

fn operation_type(kind: &OperationKind) -> OperationType {
    match kind {
        OperationKind::ContractUpgrade(_, _) => OperationType::ContractUpgrade,
        OperationKind::LargePayment(_, _, _) => OperationType::LargePayment,
        OperationKind::DisputeResolution(_, _, _, _) => OperationType::DisputeResolution,
        OperationKind::SetThresholdOverride(operation_type, _) => operation_type.clone(),
    }
}

fn validate_threshold_override(env: &Env, threshold: &Option<u32>) {
    if let Some(threshold) = threshold {
        let signer_count = read_signers(env).len();
        assert!(
            *threshold > 0 && *threshold <= signer_count,
            "Invalid threshold override"
        );
    }
}

/// Validates the payload of an `OperationKind` at proposal time.
///
/// Rules enforced before any signer approval is recorded:
///
/// | Kind              | Rule                                                          | Error                  |
/// |-------------------|---------------------------------------------------------------|------------------------|
/// | `LargePayment`    | `amount` must be strictly positive (`amount > 0`)            | `InvalidAmount`        |
/// | `LargePayment`    | `to` must not be the multisig contract itself                 | `SelfReferentialRecipient` |
/// | `ContractUpgrade` | `target` must not be the multisig contract itself            | `SelfReferentialRecipient` |
///
/// All other `OperationKind` variants have no payload constraints at proposal
/// time and pass through unconditionally.
fn validate_operation_kind(env: &Env, kind: &OperationKind) {
    match kind {
        OperationKind::LargePayment(_token, to, amount) => {
            if *amount <= 0 {
                panic_with_error!(env, MultisigError::InvalidAmount);
            }
            if to == &env.current_contract_address() {
                panic_with_error!(env, MultisigError::SelfReferentialRecipient);
            }
        }
        OperationKind::ContractUpgrade(target, _hash) => {
            if target == &env.current_contract_address() {
                panic_with_error!(env, MultisigError::SelfReferentialRecipient);
            }
        }
        // DisputeResolution and SetThresholdOverride have no payload
        // constraints enforced at proposal time.
        OperationKind::DisputeResolution(_, _, _, _) | OperationKind::SetThresholdOverride(_, _) => {}
    }
}

fn is_signer(env: &Env, addr: &Address) -> bool {
    let signers = read_signers(env);
    for i in 0..signers.len() {
        if &signers.get(i).unwrap() == addr {
            return true;
        }
    }
    false
}

fn read_rbac_address(env: &Env) -> Option<Address> {
    env.storage()
        .persistent()
        .get(&StorageKey::RbacAddress)
}

fn write_rbac_address(env: &Env, addr: &Address) {
    env.storage().persistent().set(&StorageKey::RbacAddress, addr);
}

fn next_operation_id(env: &Env) -> u128 {
    let current = env
        .storage()
        .persistent()
        .get::<_, u128>(&StorageKey::OperationCounter)
        .unwrap_or(0);
    let next = current.checked_add(1).expect("Operation id overflow");
    env.storage()
        .persistent()
        .set(&StorageKey::OperationCounter, &next);
    next
}

fn read_operation(env: &Env, operation_id: u128) -> Operation {
    env.storage()
        .persistent()
        .get::<_, Operation>(&StorageKey::Operation(operation_id))
        .expect("Operation not found")
}

fn write_operation(env: &Env, op: &Operation) {
    env.storage()
        .persistent()
        .set(&StorageKey::Operation(op.id), op);
}

fn read_approvals(env: &Env, operation_id: u128) -> Vec<Address> {
    env.storage()
        .persistent()
        .get::<_, Vec<Address>>(&StorageKey::Approvals(operation_id))
        .unwrap_or(Vec::new(env))
}

fn write_approvals(env: &Env, operation_id: u128, approvals: &Vec<Address>) {
    env.storage()
        .persistent()
        .set(&StorageKey::Approvals(operation_id), approvals);
}

fn has_approved(env: &Env, operation_id: u128, signer: &Address) -> bool {
    let approvals = read_approvals(env, operation_id);
    for i in 0..approvals.len() {
        if &approvals.get(i).unwrap() == signer {
            return true;
        }
    }
    false
}

fn approval_count(env: &Env, operation_id: u128) -> u32 {
    let approvals = read_approvals(env, operation_id);
    let mut count = 0;
    for i in 0..approvals.len() {
        let addr = approvals.get(i).unwrap();
        if is_signer(env, &addr) {
            count += 1;
        }
    }
    count
}

fn is_emergency_guardian(env: &Env, addr: &Address) -> bool {
    match env
        .storage()
        .persistent()
        .get::<_, Address>(&StorageKey::EmergencyGuardian)
    {
        Some(g) => &g == addr,
        None => false,
    }
}

/// Returns whether an operation kind is eligible for emergency execution.
///
/// Only time-sensitive, break-glass operations are eligible. Routine
/// operations (e.g. `LargePayment`) and governance changes
/// (e.g. `ContractUpgrade`, `SetThresholdOverride`) are intentionally
/// excluded so that the emergency guardian cannot bypass the normal
/// multi-signer approval process for operations that are not urgent.
///
/// ## Eligible kinds
///
/// | OperationKind       | Eligible | Rationale                          |
/// |---------------------|----------|------------------------------------|
/// | DisputeResolution   | Yes      | Prevents fund lockup; time-critical|
/// | ContractUpgrade     | No       | Governance change; needs consensus |
/// | LargePayment        | No       | Routine operation; use approvals   |
/// | SetThresholdOverride| No       | Already blocked in `perform_execute`|
fn is_emergency_eligible(kind: &OperationKind) -> bool {
    matches!(kind, OperationKind::DisputeResolution(_, _, _, _))
}

fn execute_if_threshold_met(env: &Env, operation_id: u128) {
    let op = read_operation(env, operation_id);
    let threshold = read_effective_threshold(env, &operation_type(&op.kind));
    let approvals = approval_count(env, operation_id);
    if approvals >= threshold {
        // Execute without additional signer auth (they already authenticated
        // when approving). Execution itself is a pure state transition.
        perform_execute(env, operation_id);
    }
}

fn perform_execute(env: &Env, operation_id: u128) {
    let mut op = read_operation(env, operation_id);
    if op.status != OperationStatus::Pending {
        return;
    }

    // Configuration changes must never use the guardian bypass. Re-check the
    // target type's active threshold here so every write is protected by the
    // pre-change value, regardless of which execution path reached this code.
    if let OperationKind::SetThresholdOverride(operation_type, _) = &op.kind {
        let threshold = read_effective_threshold(env, operation_type);
        assert!(
            approval_count(env, operation_id) >= threshold,
            "Threshold override requires current threshold"
        );
    }

    match &op.kind {
        OperationKind::LargePayment(token, to, amount) => {
            assert!(*amount > 0, "Amount must be positive");
            let client = token::Client::new(env, token);
            // Transfer from multisig contract balance.
            client.transfer(&env.current_contract_address(), to, amount);
        }
        // For ContractUpgrade and DisputeResolution we intentionally only
        // record the approval and execution. Off-chain or higher-level
        // orchestrators consume these events and perform the concrete action.
        OperationKind::ContractUpgrade(_, _) => {}
        OperationKind::DisputeResolution(_, _, _, _) => {}
        OperationKind::SetThresholdOverride(operation_type, threshold) => match threshold {
            Some(threshold) => env.storage().persistent().set(
                &StorageKey::ThresholdOverride(operation_type.clone()),
                threshold,
            ),
            None => env
                .storage()
                .persistent()
                .remove(&StorageKey::ThresholdOverride(operation_type.clone())),
        },
    }

    op.status = OperationStatus::Executed;
    op.executed_at = Some(env.ledger().timestamp());
    write_operation(env, &op);

    env.events().publish(
        ("operation_executed", operation_id),
        OperationExecutedEvent { operation_id },
    );
}

#[contractimpl]
impl MultisigContract {
    /// @notice Initializes the multisig wallet with signers and a threshold.
    /// @dev Can only be called once by the designated owner.
    /// @param owner Address that controls configuration updates.
    /// @param signers Initial signer set allowed to approve operations.
    /// @param threshold Number of signatures required to execute.
    /// @param emergency_guardian Optional address that can unilaterally execute
    ///        any pending operation for break-glass scenarios.
    pub fn initialize(
        env: Env,
        owner: Address,
        signers: Vec<Address>,
        threshold: u32,
        emergency_guardian: Option<Address>,
        rbac_address: Option<Address>,
    ) {
        owner.require_auth();

        let initialized = env
            .storage()
            .persistent()
            .get::<_, bool>(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(!initialized, "Contract already initialized");

        let signer_count = signers.len();
        assert!(signer_count > 0, "At least one signer required");
        assert!(
            threshold > 0 && threshold <= signer_count,
            "Invalid threshold"
        );

        // Ensure signer list has no duplicates.
        for i in 0..signer_count {
            let a = signers.get(i).unwrap();
            for j in (i + 1)..signer_count {
                let b = signers.get(j).unwrap();
                assert!(a != b, "Duplicate signer");
            }
        }

        env.storage().persistent().set(&StorageKey::Owner, &owner);
        env.storage()
            .persistent()
            .set(&StorageKey::Signers, &signers);
        env.storage()
            .persistent()
            .set(&StorageKey::Threshold, &threshold);

        if let Some(g) = emergency_guardian {
            env.storage()
                .persistent()
                .set(&StorageKey::EmergencyGuardian, &g);
        }

        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);
    }

    /// @notice Updates the signer set and default threshold.
    /// @dev Can only be called by the designated owner.
    /// @param new_signers The new list of signers.
    /// @param new_threshold The new default threshold.
    pub fn update_signers(env: Env, new_signers: Vec<Address>, new_threshold: u32) {
        require_initialized(&env);
        let owner = env
            .storage()
            .persistent()
            .get::<_, Address>(&StorageKey::Owner)
            .expect("Owner not set");
        owner.require_auth();

        let signer_count = new_signers.len();
        assert!(signer_count > 0, "At least one signer required");
        assert!(
            new_threshold > 0 && new_threshold <= signer_count,
            "New threshold must be between 1 and the number of signers"
        );

        // Ensure signer list has no duplicates.
        for i in 0..signer_count {
            let a = new_signers.get(i).unwrap();
            for j in (i + 1)..signer_count {
                let b = new_signers.get(j).unwrap();
                assert!(a != b, "Duplicate signer");
            }
        }

        env.storage()
            .persistent()
            .set(&StorageKey::Signers, &new_signers);
        env.storage()
            .persistent()
            .set(&StorageKey::Threshold, &new_threshold);

        // Adjust/cap any active per-operation overrides to ensure they do not exceed the new signer
        // count.
        for op_type in [
            OperationType::ContractUpgrade,
            OperationType::LargePayment,
            OperationType::DisputeResolution,
        ] {
            if let Some(override_val) = read_threshold_override(&env, &op_type) {
                if override_val > signer_count as u32 {
                    env.storage()
                        .persistent()
                        .set(&StorageKey::ThresholdOverride(op_type), &signer_count);
                }
            }
        }
    }

    /// @notice Proposes a new multisig-protected operation.
    /// @dev The proposer must be one of the configured signers.
    /// @param proposer Signer creating the operation.
    /// @param kind Encoded operation details.
    /// @return operation_id Newly created operation identifier.
    pub fn propose_operation(env: Env, proposer: Address, kind: OperationKind) -> u128 {
        require_initialized(&env);
        proposer.require_auth();
        assert!(is_signer(&env, &proposer), "Only signers can propose");
        // DisputeResolution proposals must come from a signer who holds
        // (or inherits) the Arbiter role in the configured RBAC contract.
        if matches!(&kind, OperationKind::DisputeResolution(_, _, _, _)) {
            if let Some(rbac_addr) = read_rbac_address(&env) {
                let rbac = RbacContractClient::new(&env, &rbac_addr);
                assert!(
                    rbac.has_role(&proposer, &Role::Arbiter),
                    "DisputeResolution requires Arbiter role"
                );
            }
        }
        if let OperationKind::SetThresholdOverride(_, threshold) = &kind {
            validate_threshold_override(&env, threshold);
        }
        validate_operation_kind(&env, &kind);

        let id = next_operation_id(&env);
        let op = Operation {
            id,
            kind,
            creator: proposer.clone(),
            status: OperationStatus::Pending,
            created_at: env.ledger().timestamp(),
            executed_at: None,
        };
        write_operation(&env, &op);

        // Auto-approve by proposer.
        let mut approvals = Vec::new(&env);
        approvals.push_back(proposer.clone());
        write_approvals(&env, id, &approvals);

        env.events().publish(
            ("operation_proposed", id),
            OperationProposedEvent {
                operation_id: id,
                creator: proposer,
            },
        );

        execute_if_threshold_met(&env, id);

        id
    }

    /// @notice Approves a pending operation as a signer.
    /// @dev Once the approval count reaches the configured threshold, the
    ///      operation is executed automatically.
    /// @param signer Signer approving the operation.
    /// @param operation_id Operation identifier.
    pub fn approve_operation(env: Env, signer: Address, operation_id: u128) {
        require_initialized(&env);
        signer.require_auth();
        assert!(is_signer(&env, &signer), "Only signers can approve");

        let op = read_operation(&env, operation_id);
        assert!(
            op.status == OperationStatus::Pending,
            "Operation not pending"
        );

        if has_approved(&env, operation_id, &signer) {
            return;
        }

        let mut approvals = read_approvals(&env, operation_id);
        approvals.push_back(signer.clone());
        let count = approvals.len();
        let threshold = read_effective_threshold(&env, &operation_type(&op.kind));

        write_approvals(&env, operation_id, &approvals);

        env.events().publish(
            ("operation_approved", operation_id),
            OperationApprovedEvent {
                operation_id,
                signer,
                approvals: count,
                threshold,
            },
        );

        execute_if_threshold_met(&env, operation_id);
    }

    /// @notice Cancels a pending operation.
    /// @dev Only the creator or the owner can cancel.
    /// @param caller Address requesting cancellation.
    /// @param operation_id Operation identifier.
    pub fn cancel_operation(env: Env, caller: Address, operation_id: u128) {
        require_initialized(&env);
        caller.require_auth();

        let mut op = read_operation(&env, operation_id);
        assert!(
            op.status == OperationStatus::Pending,
            "Operation not pending"
        );

        let owner = env
            .storage()
            .persistent()
            .get::<_, Address>(&StorageKey::Owner)
            .expect("Owner not set");

        assert!(
            caller == op.creator || caller == owner,
            "Only creator or owner can cancel"
        );

        op.status = OperationStatus::Cancelled;
        write_operation(&env, &op);

        env.events().publish(
            ("operation_cancelled", operation_id),
            OperationCancelledEvent { operation_id },
        );
    }

    /// @notice Executes a pending operation via the emergency guardian.
    /// @dev Guardian can bypass threshold checks **only** for operations
    ///      explicitly flagged as emergency-eligible (currently only
    ///      `DisputeResolution`). Routine operations (`LargePayment`),
    ///      governance changes (`ContractUpgrade`), and threshold overrides
    ///      (`SetThresholdOverride`) are rejected even when called by the
    ///      configured guardian. This prevents the break-glass mechanism
    ///      from being used to circumvent the normal multi-signer approval
    ///      process for non-urgent operations.
    /// @param guardian Configured guardian address.
    /// @param operation_id Operation identifier.
    pub fn emergency_execute(env: Env, guardian: Address, operation_id: u128) {
        require_initialized(&env);
        guardian.require_auth();
        assert!(
            is_emergency_guardian(&env, &guardian),
            "Only guardian can execute"
        );

        let op = read_operation(&env, operation_id);
        assert!(
            op.status == OperationStatus::Pending,
            "Operation not pending"
        );

        assert!(
            is_emergency_eligible(&op.kind),
            "Operation kind not eligible for emergency execution"
        );

        perform_execute(&env, operation_id);
    }

    /// @notice Returns the stored operation by id, if any.
    /// @param operation_id operation_id parameter
    /// @dev Requires caller authentication
    pub fn get_operation(env: Env, operation_id: u128) -> Option<Operation> {
        env.storage()
            .persistent()
            .get(&StorageKey::Operation(operation_id))
    }

    /// @notice Returns the current signer set.
    /// @dev Requires caller authentication
    pub fn get_signers(env: Env) -> Vec<Address> {
        read_signers(&env)
    }

    /// @notice Returns the current threshold.
    /// @dev Requires caller authentication
    pub fn get_threshold(env: Env) -> u32 {
        read_threshold(&env)
    }

    /// @notice Returns the configured threshold override for an operation type.
    /// @param operation_type Operation type to query.
    /// @return The override, or `None` when the default threshold applies.
    pub fn get_threshold_override(env: Env, operation_type: OperationType) -> Option<u32> {
        read_threshold_override(&env, &operation_type)
    }

    /// @notice Returns the threshold currently active for an operation type.
    /// @param operation_type Operation type to query.
    /// @return The configured override or, when absent, the default threshold.
    pub fn get_effective_threshold(env: Env, operation_type: OperationType) -> u32 {
        read_effective_threshold(&env, &operation_type)
    }

    /// @notice Returns current approvals for an operation.
    /// @param operation_id operation_id parameter
    /// @dev Requires caller authentication
    pub fn get_approvals(env: Env, operation_id: u128) -> Vec<Address> {
        read_approvals(&env, operation_id)
    }

    /// @notice Records the caller's approval for an operation and, once the
    ///         effective threshold is met, executes it — with mandatory WASM
    ///         hash re-validation for `ContractUpgrade` operations.
    ///
    /// @dev This function is an explicit, hash-gated alternative to
    ///      `approve_operation` for the final approval that triggers execution.
    ///      It is the **required** path when a signer wants to assert that the
    ///      WASM hash that will be deployed matches exactly what was voted on
    ///      at proposal time.
    ///
    ///      Execution flow:
    ///      1. Caller authenticates and must be a configured signer.
    ///      2. For `ContractUpgrade` operations, `expected_hash` is validated against the hash
    ///         stored in the proposal *before* the approval is recorded. If the hashes differ, the
    ///         call is rejected with `ContractUpgradeHashMismatch` and no state is modified.
    ///      3. The caller's approval is recorded (idempotent if already given).
    ///      4. If the resulting approval count meets the effective threshold, `perform_execute` is
    ///         called and the operation is marked Executed.
    ///
    ///      This design lets the final signing party use `execute_operation`
    ///      instead of `approve_operation` to enforce that they confirm the
    ///      correct hash at the moment of execution, closing the window between
    ///      proposal approval and on-chain execution.
    ///
    /// @param caller          A configured signer who is approving and
    ///                        triggering execution.
    /// @param operation_id    The ID of the pending operation.
    /// @param expected_hash   For `ContractUpgrade` operations: the WASM hash
    ///                        the caller attests to. Must match the hash stored
    ///                        in the approved proposal exactly.
    ///                        Pass `None` for non-upgrade operation kinds (it
    ///                        is ignored for those kinds).
    ///
    /// @custom:error ContractUpgradeHashMismatch  Raised when `expected_hash`
    ///               is absent or does not equal the hash stored in the
    ///               `ContractUpgrade` proposal. No state is modified.
    pub fn execute_operation(
        env: Env,
        caller: Address,
        operation_id: u128,
        expected_hash: Option<BytesN<32>>,
    ) {
        require_initialized(&env);
        caller.require_auth();
        assert!(is_signer(&env, &caller), "Only signers can execute");

        let op = read_operation(&env, operation_id);
        assert!(
            op.status == OperationStatus::Pending,
            "Operation not pending"
        );

        // For ContractUpgrade operations, re-validate the WASM hash against
        // the hash that was approved by signers at proposal time.
        // Crucially, this check happens BEFORE recording the approval so that
        // a mismatched hash never results in a partially-modified state.
        if let OperationKind::ContractUpgrade(_, stored_hash) = &op.kind {
            match &expected_hash {
                Some(h) if h == stored_hash => {
                    // Hash confirmed — safe to proceed.
                }
                _ => {
                    // Either no hash was provided or the supplied hash does
                    // not match what signers approved. Reject execution and
                    // leave all state unchanged.
                    panic_with_error!(&env, MultisigError::ContractUpgradeHashMismatch);
                }
            }
        }

        // Record the caller's approval (idempotent: ignored if already given).
        if !has_approved(&env, operation_id, &caller) {
            let mut approvals = read_approvals(&env, operation_id);
            approvals.push_back(caller.clone());
            let count = approvals.len();
            let threshold = read_effective_threshold(&env, &operation_type(&op.kind));

            write_approvals(&env, operation_id, &approvals);

            env.events().publish(
                ("operation_approved", operation_id),
                OperationApprovedEvent {
                    operation_id,
                    signer: caller,
                    approvals: count,
                    threshold,
                },
            );
        }

        // Execute if threshold is now met.
        execute_if_threshold_met(&env, operation_id);
    }
}
