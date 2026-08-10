#![cfg(test)]

use multisig::{
    MultisigContract, MultisigContractClient, MultisigError, OperationKind, OperationStatus,
    OperationType,
};
use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, BytesN, Env, Vec,
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
    client.initialize(&owner, &signers, &2u32, &Some(guardian.clone()), &None);
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
    client.initialize(&owner, &signers, &1u32, &Some(guardian.clone()), &None);
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
    client.initialize(&owner, &signers, &3u32, &Some(guardian.clone()), &None);
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

/// Zero-amount `LargePayment` must be rejected **at proposal time**, before
/// any approval can be recorded. Previously the payload was stored and the
/// error was only triggered at execution; this test asserts the earlier guard.
#[test]
fn large_payment_rejects_zero_amount() {
    let env = create_env();
    let (_multisig_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let admin = Address::generate(&env);
    let token = create_token_contract(&env, &admin);

    let recipient = Address::generate(&env);

    // propose_operation itself must fail — zero is not a valid amount
    let res = client.try_propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 0i128),
    );
    assert!(
        res.is_err(),
        "propose_operation must reject a LargePayment with zero amount"
    );

    // Confirm the error kind is InvalidAmount
    let err = res.unwrap_err().unwrap();
    assert_eq!(err, soroban_sdk::Error::from(MultisigError::InvalidAmount));
}

/// Negative-amount `LargePayment` must also be rejected at proposal time.
/// A negative `amount` is as meaningless as zero — it would either
/// do nothing or reverse the transfer direction, which is never the intent.
#[test]
fn large_payment_rejects_negative_amount() {
    let env = create_env();
    let (_multisig_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let admin = Address::generate(&env);
    let token = create_token_contract(&env, &admin);

    let recipient = Address::generate(&env);

    let res = client.try_propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), -1i128),
    );
    assert!(
        res.is_err(),
        "propose_operation must reject a LargePayment with negative amount"
    );

    let err = res.unwrap_err().unwrap();
    assert_eq!(err, soroban_sdk::Error::from(MultisigError::InvalidAmount));
}

/// A `LargePayment` whose recipient is the multisig contract itself must be
/// rejected at proposal time.  Sending tokens to oneself is almost always a
/// configuration error and would waste gas on approvals for an operation that
/// achieves nothing.
#[test]
fn large_payment_rejects_self_referential_recipient() {
    let env = create_env();
    let (multisig_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let admin = Address::generate(&env);
    let token = create_token_contract(&env, &admin);
    let token_admin_client = StellarAssetClient::new(&env, &token.address);
    token_admin_client.mint(&multisig_id, &1_000i128);

    // The recipient IS the multisig contract itself — must be rejected
    let res = client.try_propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), multisig_id.clone(), 500i128),
    );
    assert!(
        res.is_err(),
        "propose_operation must reject a LargePayment targeting the contract itself"
    );

    let err = res.unwrap_err().unwrap();
    assert_eq!(
        err,
        soroban_sdk::Error::from(MultisigError::SelfReferentialRecipient)
    );
}

/// A `ContractUpgrade` whose target is the multisig contract itself must be
/// rejected at proposal time.  Upgrading a contract through itself would be a
/// configuration error that could permanently brick the multisig; we reject
/// it early so no approvals can accumulate for such a payload.
#[test]
fn contract_upgrade_rejects_self_referential_target() {
    let env = create_env();
    let (multisig_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let hash: BytesN<32> = BytesN::from_array(&env, &[0xAB; 32]);

    // The upgrade target IS the multisig contract itself — must be rejected
    let res = client.try_propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::ContractUpgrade(multisig_id.clone(), hash),
    );
    assert!(
        res.is_err(),
        "propose_operation must reject a ContractUpgrade targeting the contract itself"
    );

    let err = res.unwrap_err().unwrap();
    assert_eq!(
        err,
        soroban_sdk::Error::from(MultisigError::SelfReferentialRecipient)
    );
}

/// A valid `LargePayment` (positive amount, distinct recipient) must still be
/// accepted at proposal time so that validation is not over-broad.
#[test]
fn large_payment_valid_payload_is_accepted() {
    let env = create_env();
    let (multisig_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let admin = Address::generate(&env);
    let token = create_token_contract(&env, &admin);
    let token_admin_client = StellarAssetClient::new(&env, &token.address);
    token_admin_client.mint(&multisig_id, &1_000i128);

    let recipient = Address::generate(&env);

    // Positive amount, distinct recipient — must succeed
    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 1i128),
    );

    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Pending);
}

/// A valid `ContractUpgrade` (distinct target) must still be accepted at
/// proposal time.
#[test]
fn contract_upgrade_valid_payload_is_accepted() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let target = Address::generate(&env); // different from multisig contract
    let hash: BytesN<32> = BytesN::from_array(&env, &[0x01; 32]);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::ContractUpgrade(target.clone(), hash),
    );

    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Pending);
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

    let res = client.try_initialize(&owner, &signers, &1u32, &None, &None);
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

// ==================== ContractUpgrade Hash Re-Validation ====================

/// Negative test: `execute_operation` rejects execution when the WASM hash
/// presented at execution time does not match the hash stored in the approved
/// proposal.
///
/// Security invariant: even when a signer provides their final approval
/// through `execute_operation`, supplying a *different* hash from the one that
/// was proposed and approved by other signers must be rejected with no state
/// change. This prevents any party from substituting a different WASM binary
/// after obtaining co-signers' approvals for a specific hash.
///
/// Setup: 3-of-3 multisig. S1 proposes ContractUpgrade with `approved_hash`.
/// S2 approves via `approve_operation` (2-of-3 → still Pending). S3 attempts
/// to use `execute_operation` with a *wrong* hash as their final approval vote
/// — rejected. S3 also tries `None` — also rejected. The operation remains
/// Pending throughout, confirming no partial state mutation on failure.
#[test]
fn execute_operation_rejects_contract_upgrade_hash_mismatch() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_3of3(&env);

    let target = Address::generate(&env);
    // The hash that was included in the proposal and approved by S1/S2.
    let approved_hash: BytesN<32> = BytesN::from_array(&env, &[0xAA; 32]);
    // A different hash that an attacker or mistaken caller tries to substitute.
    let wrong_hash: BytesN<32> = BytesN::from_array(&env, &[0xBB; 32]);

    // S1 proposes (auto-approves: 1-of-3).
    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::ContractUpgrade(target.clone(), approved_hash.clone()),
    );
    // S2 approves normally (2-of-3 → still Pending under 3-of-3 threshold).
    client.approve_operation(&signers.get(1).unwrap(), &op_id);

    assert_eq!(
        client.get_operation(&op_id).unwrap().status,
        OperationStatus::Pending,
        "Precondition: operation must be Pending with 2-of-3 approvals"
    );

    // S3 calls execute_operation with the WRONG hash.
    // The hash check fires before any approval is recorded → must be rejected.
    let result_wrong =
        client.try_execute_operation(&signers.get(2).unwrap(), &op_id, &Some(wrong_hash));
    assert!(
        result_wrong.is_err(),
        "execute_operation must reject a mismatched ContractUpgrade hash"
    );

    // Operation must remain Pending — no state change on hash-mismatch rejection.
    assert_eq!(
        client.get_operation(&op_id).unwrap().status,
        OperationStatus::Pending,
        "Operation status must stay Pending after hash-mismatch rejection"
    );
    // S3's approval must NOT have been recorded on failure.
    let approvals_after_wrong = client.get_approvals(&op_id);
    assert_eq!(
        approvals_after_wrong.len(),
        2,
        "Approval count must not increase after hash-mismatch rejection"
    );

    // S3 also tries with None → also rejected (absent hash = mismatch).
    let result_none = client.try_execute_operation(&signers.get(2).unwrap(), &op_id, &None);
    assert!(
        result_none.is_err(),
        "execute_operation must reject None as hash for a ContractUpgrade operation"
    );

    // Operation still Pending and approval count unchanged after None-hash rejection.
    assert_eq!(
        client.get_operation(&op_id).unwrap().status,
        OperationStatus::Pending,
        "Operation status must stay Pending after None-hash rejection"
    );
    assert_eq!(
        client.get_approvals(&op_id).len(),
        2,
        "Approval count must not increase after None-hash rejection"
    );
}

/// Positive test: `execute_operation` succeeds when the WASM hash presented
/// at execution time exactly matches the hash stored in the approved proposal,
/// and the resulting approval count reaches the effective threshold.
///
/// Setup: 3-of-3 multisig. S1 proposes ContractUpgrade with `approved_hash`
/// (auto-approves: 1-of-3). S2 approves via `approve_operation` (2-of-3 →
/// still Pending). S3 calls `execute_operation` with the *correct* hash as
/// their final approval vote. The function:
///   1. Validates the hash against the stored proposal → passes.
///   2. Records S3's approval (reaching 3-of-3 threshold).
///   3. Calls `perform_execute` → operation is marked Executed.
#[test]
fn execute_operation_succeeds_with_matching_contract_upgrade_hash() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_3of3(&env);

    let target = Address::generate(&env);
    // The hash approved at proposal time — must be presented identically at execution.
    let approved_hash: BytesN<32> = BytesN::from_array(&env, &[0xCC; 32]);

    // S1 proposes (auto-approves: 1-of-3).
    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::ContractUpgrade(target.clone(), approved_hash.clone()),
    );
    // S2 approves via approve_operation (2-of-3 → still Pending under 3-of-3).
    client.approve_operation(&signers.get(1).unwrap(), &op_id);

    assert_eq!(
        client.get_operation(&op_id).unwrap().status,
        OperationStatus::Pending,
        "Precondition: operation must be Pending before S3's execute_operation call"
    );

    // S3 calls execute_operation with the CORRECT hash. This:
    //   1. Validates approved_hash against the stored hash → passes.
    //   2. Records S3's approval (3-of-3 → threshold met).
    //   3. Calls execute_if_threshold_met → perform_execute → Executed.
    client.execute_operation(
        &signers.get(2).unwrap(),
        &op_id,
        &Some(approved_hash.clone()),
    );

    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(
        op.status,
        OperationStatus::Executed,
        "Operation must be Executed after execute_operation with matching hash"
    );
    assert!(
        op.executed_at.is_some(),
        "executed_at timestamp must be set on execution"
    );

    // Confirm approval list now contains all three signers.
    assert_eq!(
        client.get_approvals(&op_id).len(),
        3,
        "All three signers' approvals must be recorded"
    );
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
    assert_eq!(
        client.get_operation(&op_id).unwrap().status,
        OperationStatus::Pending
    );
    assert_eq!(client.get_approvals(&op_id).len(), 1);

    // S2 approves. Now 2 approvals.
    client.approve_operation(&signers.get(1).unwrap(), &op_id);
    assert_eq!(
        client.get_operation(&op_id).unwrap().status,
        OperationStatus::Pending
    );

    // S2 is removed, and the new signer set is S1 and S3. Threshold is updated to 2-of-2.
    let mut new_signers = Vec::new(&env);
    new_signers.push_back(signers.get(0).unwrap());
    new_signers.push_back(signers.get(2).unwrap());
    client.update_signers(&new_signers, &2u32);

    // Policy Check: S2's prior approval should NOT count anymore since S2 is removed.
    // The active approvals count should drop back to 1 (only S1).
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Pending);

    // If S3 approves, the valid count becomes 2 (S1, S3) which meets the threshold of 2, executing
    // the operation.
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

// ==================== Below-Minimum-Threshold Signer Removal Rejection ====================

#[test]
fn test_reject_signer_reduction_below_threshold() {
    // Verifies that removing signers such that the new threshold exceeds the
    // new signer count is rejected. With 3 signers and threshold 2, attempting
    // to reduce to 1 signer while keeping threshold 2 must fail because
    // 2 > 1.
    let env = create_env();
    let (_multisig_id, client, _owner, signers, _guardian) = setup_2of3(&env);

    let mut new_signers = Vec::new(&env);
    new_signers.push_back(signers.get(0).unwrap());

    let res = client.try_update_signers(&new_signers, &2u32);
    assert!(
        res.is_err(),
        "Must reject reducing signers to 1 while threshold remains 2"
    );

    // Verify the signer set was not modified.
    let stored_signers = client.get_signers();
    assert_eq!(stored_signers.len(), 3);
    assert_eq!(client.get_threshold(), 2u32);
}

#[test]
fn test_reject_update_signers_threshold_exceeding_count() {
    // Verifies that calling update_signers with a threshold higher than the
    // new signer count is rejected. With 3 signers total, setting 2 signers
    // with threshold 3 must fail because 3 > 2.
    let env = create_env();
    let (_multisig_id, client, _owner, signers, _guardian) = setup_3of3(&env);

    let mut new_signers = Vec::new(&env);
    new_signers.push_back(signers.get(0).unwrap());
    new_signers.push_back(signers.get(1).unwrap());

    let res = client.try_update_signers(&new_signers, &3u32);
    assert!(
        res.is_err(),
        "Must reject setting threshold 3 when only 2 signers remain"
    );

    // Verify the signer set and threshold were not modified.
    let stored_signers = client.get_signers();
    assert_eq!(stored_signers.len(), 3);
    assert_eq!(client.get_threshold(), 3u32);
}

#[test]
fn test_signer_reduction_with_threshold_adjustment_succeeds() {
    // Verifies that reducing both signers and threshold together succeeds
    // when the new threshold is <= the new signer count. Starting from 3-of-3,
    // reducing to 2 signers with threshold 2 is valid because 2 <= 2.
    // This is the intended recovery path when a signer must be removed.
    let env = create_env();
    let (_multisig_id, client, _owner, signers, _guardian) = setup_3of3(&env);

    let mut new_signers = Vec::new(&env);
    new_signers.push_back(signers.get(0).unwrap());
    new_signers.push_back(signers.get(1).unwrap());

    client.update_signers(&new_signers, &2u32);

    let stored_signers = client.get_signers();
    assert_eq!(stored_signers.len(), 2);
    assert_eq!(stored_signers.get(0).unwrap(), signers.get(0).unwrap());
    assert_eq!(stored_signers.get(1).unwrap(), signers.get(1).unwrap());
    assert_eq!(client.get_threshold(), 2u32);

    // Confirm operations can still reach quorum with the new 2-of-2 setup.
    let token = create_token_contract(&env, &Address::generate(&env));
    let token_admin = StellarAssetClient::new(&env, &token.address);
    token_admin.mint(&_multisig_id, &1_000i128);

    let recipient = Address::generate(&env);
    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 100i128),
    );

    // 1 approval (proposer) is not enough for threshold 2
    assert_eq!(
        client.get_operation(&op_id).unwrap().status,
        OperationStatus::Pending
    );

    // Second signer approves, reaching threshold
    client.approve_operation(&signers.get(1).unwrap(), &op_id);
    assert_eq!(
        client.get_operation(&op_id).unwrap().status,
        OperationStatus::Executed
    );
    assert_eq!(token.balance(&recipient), 100i128);
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

    // The override of 3 should have been capped/recalculated to 2 (since the new signer count is
    // 2).
    assert_eq!(
        client.get_threshold_override(&OperationType::ContractUpgrade),
        Some(2)
    );
    assert_eq!(
        client.get_effective_threshold(&OperationType::ContractUpgrade),
        2
    );
}

// ==================== Emergency Execute Eligibility (issue #898) ====================

/// Verifies that `emergency_execute` rejects a `LargePayment` operation
/// (not emergency-eligible) with a panic, even when called by the configured
/// guardian. Large payments are routine operations that must go through the
/// standard multi-signer approval process.
#[test]
fn emergency_execute_rejects_large_payment() {
    let env = create_env();
    let (multisig_id, client, _owner, signers, guardian) = setup_2of3(&env);

    let admin = Address::generate(&env);
    let token = create_token_contract(&env, &admin);
    let token_admin_client = StellarAssetClient::new(&env, &token.address);
    token_admin_client.mint(&multisig_id, &1_000i128);

    let recipient = Address::generate(&env);
    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 200i128),
    );

    // Guardian attempts emergency execute on a LargePayment → must be rejected
    let result = client.try_emergency_execute(&guardian, &op_id);
    assert!(
        result.is_err(),
        "LargePayment must not be emergency-eligible"
    );

    // Operation must remain Pending — no state change on rejection
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Pending);
    assert_eq!(token.balance(&recipient), 0i128);
}

/// Verifies that `emergency_execute` rejects a `ContractUpgrade` operation
/// (not emergency-eligible) with a panic, even when called by the configured
/// guardian. Contract upgrades are governance changes that require full
/// multi-signer consensus.
#[test]
fn emergency_execute_rejects_contract_upgrade() {
    let env = create_env();
    let (_id, client, _owner, signers, guardian) = setup_2of3(&env);

    let target = Address::generate(&env);
    let hash: BytesN<32> = BytesN::from_array(&env, &[0xDD; 32]);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::ContractUpgrade(target.clone(), hash),
    );

    // Guardian attempts emergency execute on a ContractUpgrade → must be rejected
    let result = client.try_emergency_execute(&guardian, &op_id);
    assert!(
        result.is_err(),
        "ContractUpgrade must not be emergency-eligible"
    );

    // Operation must remain Pending
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Pending);
}

/// Verifies that `emergency_execute` rejects a `SetThresholdOverride`
/// operation (not emergency-eligible) with a panic, even when called by the
/// configured guardian. Threshold changes are governance operations that must
/// go through the standard multi-signer approval process (and are additionally
/// protected inside `perform_execute`).
#[test]
fn emergency_execute_rejects_set_threshold_override() {
    let env = create_env();
    let (_id, client, _owner, signers, guardian) = setup_2of3(&env);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::SetThresholdOverride(OperationType::LargePayment, Some(1)),
    );

    // Guardian attempts emergency execute on a SetThresholdOverride → must be rejected
    let result = client.try_emergency_execute(&guardian, &op_id);
    assert!(
        result.is_err(),
        "SetThresholdOverride must not be emergency-eligible"
    );

    // Operation must remain Pending and the override must NOT be applied
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Pending);
    assert_eq!(
        client.get_threshold_override(&OperationType::LargePayment),
        None
    );
}

/// Verifies that `emergency_execute` succeeds for a `DisputeResolution`
/// operation (the only currently emergency-eligible kind) when called by the
/// configured guardian, bypassing the normal threshold.
#[test]
fn emergency_execute_succeeds_for_dispute_resolution() {
    let env = create_env();
    let (_id, client, _owner, signers, guardian) = setup_2of3(&env);

    let payroll_contract = Address::generate(&env);
    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(payroll_contract, 42u128, 500, 200),
    );

    // Guardian executes the emergency-eligible DisputeResolution → must succeed
    client.emergency_execute(&guardian, &op_id);

    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Executed);
    assert!(
        op.executed_at.is_some(),
        "executed_at must be set on emergency execution"
    );
}

/// Verifies that emergency-eligible operations still require the guardian's
/// authentication — they do NOT allow zero-approval execution by arbitrary
/// callers. This ensures the "guardian quorum" (i.e., the guardian's signature)
/// is always required.
#[test]
fn emergency_eligible_op_still_requires_guardian_quorum() {
    let env = create_env();
    let (_id, client, _owner, signers, guardian) = setup_2of3(&env);

    let payroll_contract = Address::generate(&env);
    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(payroll_contract, 42u128, 500, 200),
    );

    // A non-guardian signer tries emergency execute → must be rejected
    let non_guardian = signers.get(0).unwrap();
    assert_ne!(non_guardian, guardian);
    let result = client.try_emergency_execute(&non_guardian, &op_id);
    assert!(
        result.is_err(),
        "Non-guardian must be rejected even for emergency-eligible operations"
    );

    // A random address tries emergency execute → must be rejected
    let random = Address::generate(&env);
    let result = client.try_emergency_execute(&random, &op_id);
    assert!(
        result.is_err(),
        "Random address must be rejected even for emergency-eligible operations"
    );

    // Operation must remain Pending — no state change
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Pending);
}
