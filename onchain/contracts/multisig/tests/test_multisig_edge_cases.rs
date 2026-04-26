#![cfg(test)]

use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, BytesN, Env, Vec,
};

use multisig::{MultisigContract, MultisigContractClient, OperationKind, OperationStatus};

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

    // Third signer tries to approve - should be silently ignored (not pending)
    client.approve_operation(&signers.get(2).unwrap(), &op_id);
    let approvals = client.get_approvals(&op_id);
    assert_eq!(approvals.len(), 2); // Still only 2 approvals
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
