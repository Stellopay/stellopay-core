#![cfg(test)]

use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, BytesN, Env, Vec,
};

use multisig::{
    MultisigContract, MultisigContractClient, OperationKind, OperationStatus, OperationType,
};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn register_contract(env: &Env) -> (Address, MultisigContractClient<'static>) {
    #[allow(deprecated)]
    let id = env.register_contract(None, MultisigContract);
    let client = MultisigContractClient::new(env, &id);
    (id, client)
}

fn create_token_contract<'a>(env: &Env, admin: &Address) -> TokenClient<'a> {
    let token_addr = env.register_stellar_asset_contract(admin.clone());
    TokenClient::new(env, &token_addr)
}

fn setup_2of3(
    env: &Env,
) -> (
    Address,
    MultisigContractClient<'static>,
    Address,
    Vec<Address>,
    Address,
) {
    let (id, client) = register_contract(env);
    let owner = Address::generate(env);
    let s1 = Address::generate(env);
    let s2 = Address::generate(env);
    let s3 = Address::generate(env);

    let mut signers = Vec::new(env);
    signers.push_back(s1.clone());
    signers.push_back(s2.clone());
    signers.push_back(s3.clone());

    let guardian = Address::generate(env);
    client.initialize(&owner, &signers, &2u32, &Some(guardian.clone()));
    (id, client, owner, signers, guardian)
}

fn setup_1of1(
    env: &Env,
) -> (
    Address,
    MultisigContractClient<'static>,
    Address,
    Vec<Address>,
    Address,
) {
    let (id, client) = register_contract(env);
    let owner = Address::generate(env);
    let s1 = Address::generate(env);

    let mut signers = Vec::new(env);
    signers.push_back(s1.clone());

    let guardian = Address::generate(env);
    client.initialize(&owner, &signers, &1u32, &Some(guardian.clone()));
    (id, client, owner, signers, guardian)
}

fn setup_3of3(
    env: &Env,
) -> (
    Address,
    MultisigContractClient<'static>,
    Address,
    Vec<Address>,
    Address,
) {
    let (id, client) = register_contract(env);
    let owner = Address::generate(env);
    let s1 = Address::generate(env);
    let s2 = Address::generate(env);
    let s3 = Address::generate(env);

    let mut signers = Vec::new(env);
    signers.push_back(s1.clone());
    signers.push_back(s2.clone());
    signers.push_back(s3.clone());

    let guardian = Address::generate(env);
    client.initialize(&owner, &signers, &3u32, &Some(guardian.clone()));
    (id, client, owner, signers, guardian)
}

// ==================== 1-of-N Edge Cases ====================

#[test]
fn one_of_one_auto_executes_on_propose() {
    let env = create_env();
    let (multisig_id, client, _owner, signers, _guardian) = setup_1of1(&env);

    let admin = Address::generate(&env);
    let token = create_token_contract(&env, &admin);
    let token_admin_client = StellarAssetClient::new(&env, &token.address);
    token_admin_client.mint(&multisig_id, &1_000i128);

    let recipient = Address::generate(&env);
    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 100i128),
    );

    // Should auto-execute since threshold is 1 and proposer auto-approves
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Executed);
    assert_eq!(token.balance(&recipient), 100i128);
}

// ==================== N-of-N Edge Cases ====================

#[test]
fn three_of_three_requires_all_approvals() {
    let env = create_env();
    let (multisig_id, client, _owner, signers, _guardian) = setup_3of3(&env);

    let admin = Address::generate(&env);
    let token = create_token_contract(&env, &admin);
    let token_admin_client = StellarAssetClient::new(&env, &token.address);
    token_admin_client.mint(&multisig_id, &1_000i128);

    let recipient = Address::generate(&env);
    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 100i128),
    );

    // 1 approval (proposer) - not enough
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Pending);

    // 2 approvals - still not enough
    client.approve_operation(&signers.get(1).unwrap(), &op_id);
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Pending);

    // 3 approvals - now executes
    client.approve_operation(&signers.get(2).unwrap(), &op_id);
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Executed);
    assert_eq!(token.balance(&recipient), 100i128);
}

// ==================== Duplicate Approval Prevention ====================

#[test]
fn duplicate_approval_is_ignored() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(Address::generate(&env), 1u128, 10, 0),
    );

    // Same signer approves twice
    client.approve_operation(&signers.get(0).unwrap(), &op_id);
    client.approve_operation(&signers.get(0).unwrap(), &op_id);

    // Should still only have 1 approval
    let approvals = client.get_approvals(&op_id);
    assert_eq!(approvals.len(), 1);

    // Operation should still be pending (threshold is 2)
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Pending);
}

// ==================== Non-Signer Rejection ====================

#[test]
fn non_signer_cannot_propose() {
    let env = create_env();
    let (_id, client, _owner, _signers, _guardian) = setup_2of3(&env);

    let non_signer = Address::generate(&env);
    let res = client.try_propose_operation(
        &non_signer,
        &OperationKind::DisputeResolution(Address::generate(&env), 1u128, 10, 0),
    );
    assert!(res.is_err());
}

#[test]
fn non_signer_cannot_approve() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(Address::generate(&env), 1u128, 10, 0),
    );

    let non_signer = Address::generate(&env);
    let res = client.try_approve_operation(&non_signer, &op_id);
    assert!(res.is_err());
}

// ==================== Already-Executed Rejection ====================

#[test]
#[should_panic(expected = "Operation not pending")]
fn cannot_approve_already_executed_operation() {
    let env = create_env();
    let (multisig_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let admin = Address::generate(&env);
    let token = create_token_contract(&env, &admin);
    let token_admin_client = StellarAssetClient::new(&env, &token.address);
    token_admin_client.mint(&multisig_id, &1_000i128);

    let recipient = Address::generate(&env);
    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 100i128),
    );

    // Execute by reaching threshold
    client.approve_operation(&signers.get(1).unwrap(), &op_id);
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Executed);

    // Third signer tries to approve an already-executed (not pending)
    // operation. Unlike a duplicate approval from the same signer on a still
    // *pending* operation (which is silently ignored, see
    // `duplicate_approval_is_ignored`), this is rejected with a hard panic —
    // the contract only special-cases re-approval while `Pending`, not
    // approval attempts after execution.
    client.approve_operation(&signers.get(2).unwrap(), &op_id);
}

#[test]
fn cannot_cancel_already_executed_operation() {
    let env = create_env();
    let (multisig_id, client, owner, signers, _guardian) = setup_2of3(&env);

    let admin = Address::generate(&env);
    let token = create_token_contract(&env, &admin);
    let token_admin_client = StellarAssetClient::new(&env, &token.address);
    token_admin_client.mint(&multisig_id, &1_000i128);

    let recipient = Address::generate(&env);
    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 100i128),
    );

    client.approve_operation(&signers.get(1).unwrap(), &op_id);

    // Owner tries to cancel executed operation
    let res = client.try_cancel_operation(&owner, &op_id);
    assert!(res.is_err());
}

// ==================== Guardian-Only Rescue ====================

#[test]
fn non_guardian_cannot_emergency_execute() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(Address::generate(&env), 1u128, 10, 0),
    );

    let fake_guardian = Address::generate(&env);
    let res = client.try_emergency_execute(&fake_guardian, &op_id);
    assert!(res.is_err());
}

#[test]
fn guardian_cannot_execute_already_executed() {
    let env = create_env();
    let (multisig_id, client, _owner, signers, guardian) = setup_2of3(&env);

    let admin = Address::generate(&env);
    let token = create_token_contract(&env, &admin);
    let token_admin_client = StellarAssetClient::new(&env, &token.address);
    token_admin_client.mint(&multisig_id, &1_000i128);

    let recipient = Address::generate(&env);
    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 100i128),
    );

    // Execute normally
    client.approve_operation(&signers.get(1).unwrap(), &op_id);

    // Guardian tries emergency execute on already executed op
    let res = client.try_emergency_execute(&guardian, &op_id);
    assert!(res.is_err());
}

#[test]
fn guardian_cannot_execute_cancelled_operation() {
    let env = create_env();
    let (_id, client, _owner, signers, guardian) = setup_2of3(&env);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(Address::generate(&env), 1u128, 10, 0),
    );

    // Cancel the operation
    client.cancel_operation(&signers.get(0).unwrap(), &op_id);

    // Guardian tries emergency execute
    let res = client.try_emergency_execute(&guardian, &op_id);
    assert!(res.is_err());
}

// ==================== Security: Threshold Changes Mid-Flight ====================

#[test]
fn lowering_override_requires_current_override_threshold() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let raise = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::SetThresholdOverride(OperationType::ContractUpgrade, Some(3)),
    );
    client.approve_operation(&signers.get(1).unwrap(), &raise);

    let lower = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::SetThresholdOverride(OperationType::ContractUpgrade, Some(1)),
    );
    client.approve_operation(&signers.get(1).unwrap(), &lower);

    assert_eq!(
        client.get_operation(&lower).unwrap().status,
        OperationStatus::Pending
    );
    assert_eq!(
        client.get_threshold_override(&OperationType::ContractUpgrade),
        Some(3)
    );

    client.approve_operation(&signers.get(2).unwrap(), &lower);
    assert_eq!(
        client.get_operation(&lower).unwrap().status,
        OperationStatus::Executed
    );
    assert_eq!(
        client.get_effective_threshold(&OperationType::ContractUpgrade),
        1
    );
}

#[test]
fn guardian_cannot_bypass_threshold_for_override_change() {
    let env = create_env();
    let (_id, client, _owner, signers, guardian) = setup_2of3(&env);

    let change = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::SetThresholdOverride(OperationType::LargePayment, Some(1)),
    );
    let result = client.try_emergency_execute(&guardian, &change);

    assert!(result.is_err());
    assert_eq!(
        client.get_operation(&change).unwrap().status,
        OperationStatus::Pending
    );
    assert_eq!(
        client.get_threshold_override(&OperationType::LargePayment),
        None
    );
}

#[test]
fn invalid_threshold_overrides_are_rejected() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    for threshold in [0, 4] {
        let result = client.try_propose_operation(
            &signers.get(0).unwrap(),
            &OperationKind::SetThresholdOverride(OperationType::DisputeResolution, Some(threshold)),
        );
        assert!(result.is_err());
    }
}

#[test]
fn removing_override_requires_override_then_restores_default() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let raise = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::SetThresholdOverride(OperationType::DisputeResolution, Some(3)),
    );
    client.approve_operation(&signers.get(1).unwrap(), &raise);

    let remove = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::SetThresholdOverride(OperationType::DisputeResolution, None),
    );
    client.approve_operation(&signers.get(1).unwrap(), &remove);
    assert_eq!(
        client.get_operation(&remove).unwrap().status,
        OperationStatus::Pending
    );

    client.approve_operation(&signers.get(2).unwrap(), &remove);
    assert_eq!(
        client.get_threshold_override(&OperationType::DisputeResolution),
        None
    );
    assert_eq!(
        client.get_effective_threshold(&OperationType::DisputeResolution),
        2
    );
}

#[test]
fn approve_with_threshold_higher_than_current_still_counts() {
    // Verify that approvals from before threshold change still count
    let env = create_env();
    let (multisig_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let admin = Address::generate(&env);
    let token = create_token_contract(&env, &admin);
    let token_admin_client = StellarAssetClient::new(&env, &token.address);
    token_admin_client.mint(&multisig_id, &1_000i128);

    let recipient = Address::generate(&env);
    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 100i128),
    );

    // Approve (1 of 2)
    client.approve_operation(&signers.get(1).unwrap(), &op_id);
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Executed);
}

// ==================== Multiple Operations ====================

#[test]
fn multiple_operations_independent() {
    let env = create_env();
    let (multisig_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let admin = Address::generate(&env);
    let token = create_token_contract(&env, &admin);
    let token_admin_client = StellarAssetClient::new(&env, &token.address);
    token_admin_client.mint(&multisig_id, &10_000i128);

    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    let op1 = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), r1.clone(), 100i128),
    );

    let op2 = client.propose_operation(
        &signers.get(1).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), r2.clone(), 200i128),
    );

    // Only approve op1 (threshold reached)
    client.approve_operation(&signers.get(1).unwrap(), &op1);

    let o1 = client.get_operation(&op1).unwrap();
    let o2 = client.get_operation(&op2).unwrap();
    assert_eq!(o1.status, OperationStatus::Executed);
    assert_eq!(o2.status, OperationStatus::Pending);

    assert_eq!(token.balance(&r1), 100i128);
    assert_eq!(token.balance(&r2), 0i128);
}

// ==================== Replay Protection: Cancelled Operations Are Terminal ====================

#[test]
#[should_panic(expected = "Operation not pending")]
fn cannot_approve_cancelled_operation() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(Address::generate(&env), 1u128, 10, 0),
    );

    // Cancel the pending operation
    client.cancel_operation(&signers.get(0).unwrap(), &op_id);
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Cancelled);

    // Attempt to approve the cancelled operation — must panic.
    // A cancelled operation is terminal; it cannot be resurrected.
    client.approve_operation(&signers.get(1).unwrap(), &op_id);
}

#[test]
fn cannot_cancel_already_cancelled_operation() {
    let env = create_env();
    let (_id, client, owner, signers, _guardian) = setup_2of3(&env);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(Address::generate(&env), 1u128, 10, 0),
    );

    // Cancel once — succeeds
    client.cancel_operation(&signers.get(0).unwrap(), &op_id);
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Cancelled);

    // Creator tries to cancel again — rejected
    let res = client.try_cancel_operation(&signers.get(0).unwrap(), &op_id);
    assert!(res.is_err());

    // Owner tries to cancel the already-cancelled operation — also rejected
    let res = client.try_cancel_operation(&owner, &op_id);
    assert!(res.is_err());
}

#[test]
fn cancelled_operation_id_is_not_reused() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    // Propose and cancel first operation
    let op1_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(Address::generate(&env), 1u128, 10, 0),
    );
    client.cancel_operation(&signers.get(0).unwrap(), &op1_id);

    // Propose a second operation — must get a strictly higher id
    let op2_id = client.propose_operation(
        &signers.get(1).unwrap(),
        &OperationKind::DisputeResolution(Address::generate(&env), 2u128, 20, 0),
    );
    assert!(
        op2_id > op1_id,
        "Operation ids must be strictly increasing; cancelled id {} was reused as {}",
        op1_id,
        op2_id
    );

    // The second operation is a completely fresh independent operation
    let op2 = client.get_operation(&op2_id).unwrap();
    assert_eq!(op2.status, OperationStatus::Pending);
    assert!(op2.id > op1_id);
}

// ==================== Cancel Operation Authorization ====================

#[test]
fn signer_not_proposer_cannot_cancel() {
    // Verifies that a signer who is neither the proposer nor the admin
    // (owner) is rejected from cancelling a pending operation.
    let env = create_env();
    let (_id, client, owner, signers, _guardian) = setup_2of3(&env);

    // S1 proposes an operation
    let proposer = signers.get(0).unwrap();
    let op_id = client.propose_operation(
        &proposer,
        &OperationKind::DisputeResolution(Address::generate(&env), 1u128, 10, 0),
    );

    // S2 is a valid signer but NOT the proposer (S1) and NOT the owner
    let other_signer = signers.get(1).unwrap();
    assert_ne!(other_signer, proposer);
    assert_ne!(other_signer, owner);

    let res = client.try_cancel_operation(&other_signer, &op_id);
    assert!(
        res.is_err(),
        "A signer who is not the proposer and not the owner must be rejected from cancelling"
    );

    // Operation remains pending
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Pending);
}

#[test]
fn proposer_can_cancel_own_operation() {
    // Verifies that the original proposer can cancel their own pending
    // operation.
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let proposer = signers.get(0).unwrap();
    let op_id = client.propose_operation(
        &proposer,
        &OperationKind::DisputeResolution(Address::generate(&env), 1u128, 10, 0),
    );

    // Proposer cancels their own operation — must succeed
    client.cancel_operation(&proposer, &op_id);
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Cancelled);
}

#[test]
fn owner_can_cancel_any_pending_operation() {
    // Verifies that the admin (owner) can cancel any pending operation,
    // not just operations they proposed.
    let env = create_env();
    let (_id, client, owner, signers, _guardian) = setup_2of3(&env);

    let proposer = signers.get(0).unwrap();
    let op_id = client.propose_operation(
        &proposer,
        &OperationKind::DisputeResolution(Address::generate(&env), 1u128, 10, 0),
    );

    // Owner (admin) cancels — must succeed even though owner did not propose
    client.cancel_operation(&owner, &op_id);
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Cancelled);
}

// ==================== Large Payment Validation ====================

#[test]
fn large_payment_rejects_zero_amount() {
    let env = create_env();
    let (multisig_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let admin = Address::generate(&env);
    let token = create_token_contract(&env, &admin);
    let token_admin_client = StellarAssetClient::new(&env, &token.address);
    token_admin_client.mint(&multisig_id, &1_000i128);

    let recipient = Address::generate(&env);
    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 0i128),
    );

    // Second approval triggers execution which should fail
    let res = client.try_approve_operation(&signers.get(1).unwrap(), &op_id);
    assert!(res.is_err());
}

// ==================== Duplicate Signer Rejection ====================

#[test]
fn initialize_rejects_duplicate_signers() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    let s1 = Address::generate(&env);

    let mut signers = Vec::new(&env);
    signers.push_back(s1.clone());
    signers.push_back(s1.clone()); // duplicate

    let res = client.try_initialize(&owner, &signers, &1u32, &None);
    assert!(res.is_err());
}

// ==================== ContractUpgrade Flow ====================

#[test]
fn contract_upgrade_proposal_and_execute() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let target = Address::generate(&env);
    let hash: BytesN<32> = BytesN::from_array(&env, &[0xAB; 32]);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::ContractUpgrade(target.clone(), hash.clone()),
    );

    client.approve_operation(&signers.get(1).unwrap(), &op_id);

    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Executed);
}

// ==================== DisputeResolution Flow ====================

#[test]
fn dispute_resolution_proposal_and_execute() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let payroll_contract = Address::generate(&env);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(payroll_contract, 42u128, 500, 200),
    );

    client.approve_operation(&signers.get(1).unwrap(), &op_id);

    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Executed);
}

// ==================== Query Functions ====================

#[test]
fn query_functions_return_correct_data() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let stored_signers = client.get_signers();
    assert_eq!(stored_signers.len(), 3);

    let threshold = client.get_threshold();
    assert_eq!(threshold, 2u32);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(Address::generate(&env), 1u128, 10, 0),
    );

    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.id, op_id);
    assert_eq!(op.status, OperationStatus::Pending);

    let approvals = client.get_approvals(&op_id);
    assert_eq!(approvals.len(), 1);
}

#[test]
fn get_nonexistent_operation_returns_none() {
    let env = create_env();
    let (_id, client, _owner, _signers, _guardian) = setup_2of3(&env);

    let op = client.get_operation(&999u128);
    assert!(op.is_none());
}

// ==================== Signer Set Updates & Quorum Policy ====================

#[test]
fn test_signer_removal_prior_confirmation_policy() {
    let env = create_env();
    let (multisig_id, client, _owner, signers, _guardian) = setup_3of3(&env);

    let admin = Address::generate(&env);
    let token = create_token_contract(&env, &admin);
    let token_admin_client = StellarAssetClient::new(&env, &token.address);
    token_admin_client.mint(&multisig_id, &1_000i128);

    let recipient = Address::generate(&env);
    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 100i128),
    );

    // Initial state: threshold is 3. S1 proposed (which auto-approves), so 1 approval.
    assert_eq!(client.get_operation(&op_id).unwrap().status, OperationStatus::Pending);
    assert_eq!(client.get_approvals(&op_id).len(), 1);

    // S2 approves. Now 2 approvals.
    client.approve_operation(&signers.get(1).unwrap(), &op_id);
    assert_eq!(client.get_operation(&op_id).unwrap().status, OperationStatus::Pending);

    // S2 is removed, and the new signer set is S1 and S3. Threshold is updated to 2-of-2.
    let mut new_signers = Vec::new(&env);
    new_signers.push_back(signers.get(0).unwrap());
    new_signers.push_back(signers.get(2).unwrap());
    client.update_signers(&new_signers, &2u32);

    // Policy Check: S2's prior approval should NOT count anymore since S2 is removed.
    // The active approvals count should drop back to 1 (only S1).
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Pending);

    // If S3 approves, the valid count becomes 2 (S1, S3) which meets the threshold of 2, executing the operation.
    client.approve_operation(&signers.get(2).unwrap(), &op_id);
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Executed);
    assert_eq!(token.balance(&recipient), 100i128);
}

#[test]
fn test_removed_signer_cannot_newly_confirm() {
    let env = create_env();
    let (_multisig_id, client, _owner, signers, _guardian) = setup_3of3(&env);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(Address::generate(&env), 1u128, 10, 0),
    );

    // Remove S2 (index 1)
    let mut new_signers = Vec::new(&env);
    new_signers.push_back(signers.get(0).unwrap());
    new_signers.push_back(signers.get(2).unwrap());
    client.update_signers(&new_signers, &2u32);

    // S2 attempts to approve, which must fail
    let res = client.try_approve_operation(&signers.get(1).unwrap(), &op_id);
    assert!(res.is_err());

    // S2 attempts to propose a new operation, which must fail
    let res = client.try_propose_operation(
        &signers.get(1).unwrap(),
        &OperationKind::DisputeResolution(Address::generate(&env), 2u128, 10, 0),
    );
    assert!(res.is_err());
}

#[test]
fn test_quorum_override_recalculation_after_signer_removal() {
    let env = create_env();
    let (_multisig_id, client, _owner, signers, _guardian) = setup_3of3(&env);

    // S1 proposes a threshold override of 3 for ContractUpgrade.
    let override_op = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::SetThresholdOverride(OperationType::ContractUpgrade, Some(3)),
    );
    client.approve_operation(&signers.get(1).unwrap(), &override_op);
    client.approve_operation(&signers.get(2).unwrap(), &override_op);

    // Verification: override is set to 3.
    assert_eq!(
        client.get_threshold_override(&OperationType::ContractUpgrade),
        Some(3)
    );

    // Remove S3. New signer set is S1 and S2. Threshold is 2.
    let mut new_signers = Vec::new(&env);
    new_signers.push_back(signers.get(0).unwrap());
    new_signers.push_back(signers.get(1).unwrap());
    client.update_signers(&new_signers, &2u32);

    // The override of 3 should have been capped/recalculated to 2 (since the new signer count is 2).
    assert_eq!(
        client.get_threshold_override(&OperationType::ContractUpgrade),
        Some(2)
    );
    assert_eq!(
        client.get_effective_threshold(&OperationType::ContractUpgrade),
        2
    );
}
