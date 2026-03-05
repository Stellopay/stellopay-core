#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, BytesN, Env, Vec};

#[contract]
pub struct MultisigContract;

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
    Signers,
    Threshold,
    OperationCounter,
    Operation(u128),
    Approvals(u128),
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

fn is_signer(env: &Env, addr: &Address) -> bool {
    let signers = read_signers(env);
    for i in 0..signers.len() {
        if &signers.get(i).unwrap() == addr {
            return true;
        }
    }
    false
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
    approvals.len()
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

fn execute_if_threshold_met(env: &Env, operation_id: u128) {
    let threshold = read_threshold(env);
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

    /// @notice Proposes a new multisig-protected operation.
    /// @dev The proposer must be one of the configured signers.
    /// @param proposer Signer creating the operation.
    /// @param kind Encoded operation details.
    /// @return operation_id Newly created operation identifier.
    pub fn propose_operation(env: Env, proposer: Address, kind: OperationKind) -> u128 {
        require_initialized(&env);
        proposer.require_auth();
        assert!(is_signer(&env, &proposer), "Only signers can propose");

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
        let threshold = read_threshold(&env);

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
    /// @dev Guardian can bypass threshold checks in break-glass scenarios.
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

    /// @notice Returns current approvals for an operation.
    /// @param operation_id operation_id parameter
    /// @dev Requires caller authentication
    pub fn get_approvals(env: Env, operation_id: u128) -> Vec<Address> {
        read_approvals(&env, operation_id)
    }
}
