#![cfg(test)]

use multisig::{
    MultisigContract, MultisigContractClient, OperationKind, OperationStatus, OperationType,
};
use rbac::{RbacContract, RbacContractClient as RbacClient, Role};
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

fn setup_initialized(
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

#[test]
fn initialize_rejects_invalid_threshold() {
    let env = create_env();
    let (id, client) = register_contract(&env);
    let owner = Address::generate(&env);
    let s1 = Address::generate(&env);

    let mut signers = Vec::new(&env);
    signers.push_back(s1);

    // threshold 0 is invalid
    let res = client.try_initialize(&owner, &signers, &0u32, &None);
    assert!(res.is_err());

    // threshold > len(signers) is invalid
    let res = client.try_initialize(&owner, &signers, &2u32, &None);
    assert!(res.is_err());

    // Sanity: valid config succeeds
    client.initialize(&owner, &signers, &1u32, &None);

    // second initialize should fail
    let res = client.try_initialize(&owner, &signers, &1u32, &None);
    assert!(res.is_err());

    // avoid unused warning
    let _ = id;
}

#[test]
fn propose_and_auto_approve_by_creator() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_initialized(&env);

    let proposer = signers.get(0).unwrap();
    let target = Address::generate(&env);
    let hash: BytesN<32> = BytesN::from_array(&env, &[1u8; 32]);

    let op_id = client.propose_operation(
        &proposer,
        &OperationKind::ContractUpgrade(target.clone(), hash),
    );

    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.id, op_id);
    assert_eq!(op.status, OperationStatus::Pending);

    let approvals = client.get_approvals(&op_id);
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals.get(0).unwrap(), proposer);
}

#[test]
fn threshold_execution_for_large_payment() {
    let env = create_env();
    let (multisig_id, client, _owner, signers, _guardian) = setup_initialized(&env);

    // set up token contract and fund multisig
    let admin = Address::generate(&env);
    let token = create_token_contract(&env, &admin);
    let token_admin_client = StellarAssetClient::new(&env, &token.address);

    // mint to multisig contract so it can pay out
    token_admin_client.mint(&multisig_id, &1_000i128);

    let recipient = Address::generate(&env);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 500i128),
    );

    // One approval (from proposer) is not enough yet (threshold = 2)
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Pending);
    assert_eq!(token.balance(&recipient), 0);

    // Second signer approves, reaching threshold and triggering transfer
    client.approve_operation(&signers.get(1).unwrap(), &op_id);

    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Executed);
    assert_eq!(token.balance(&recipient), 500i128);
}

#[test]
fn operation_type_override_takes_effect_without_changing_default() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_initialized(&env);

    let set_override = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::SetThresholdOverride(OperationType::ContractUpgrade, Some(3)),
    );
    client.approve_operation(&signers.get(1).unwrap(), &set_override);

    assert_eq!(
        client.get_threshold_override(&OperationType::ContractUpgrade),
        Some(3)
    );
    assert_eq!(
        client.get_effective_threshold(&OperationType::ContractUpgrade),
        3
    );
    assert_eq!(
        client.get_effective_threshold(&OperationType::DisputeResolution),
        2
    );

    let upgrade = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::ContractUpgrade(
            Address::generate(&env),
            BytesN::from_array(&env, &[2u8; 32]),
        ),
    );
    client.approve_operation(&signers.get(1).unwrap(), &upgrade);
    assert_eq!(
        client.get_operation(&upgrade).unwrap().status,
        OperationStatus::Pending
    );
    client.approve_operation(&signers.get(2).unwrap(), &upgrade);
    assert_eq!(
        client.get_operation(&upgrade).unwrap().status,
        OperationStatus::Executed
    );

    let dispute = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(Address::generate(&env), 7, 10, 0),
    );
    client.approve_operation(&signers.get(1).unwrap(), &dispute);
    assert_eq!(
        client.get_operation(&dispute).unwrap().status,
        OperationStatus::Executed
    );
}

#[test]
fn emergency_guardian_can_execute_dispute_resolution_without_threshold() {
    let env = create_env();
    let (_id, client, _owner, signers, guardian) = setup_initialized(&env);

    // DisputeResolution is emergency-eligible
    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(Address::generate(&env), 1u128, 10, 0),
    );

    // Guardian executes directly (skipping the second approval)
    client.emergency_execute(&guardian, &op_id);

    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Executed);
}

#[test]
fn cancel_operation_by_creator_or_owner() {
    let env = create_env();
    let (_id, client, owner, signers, _guardian) = setup_initialized(&env);

    let proposer = signers.get(0).unwrap();
    let other = Address::generate(&env);

    let op_id = client.propose_operation(
        &proposer,
        &OperationKind::DisputeResolution(Address::generate(&env), 1u128, 10, 0),
    );

    // non-creator, non-owner cannot cancel
    let res = client.try_cancel_operation(&other, &op_id);
    assert!(res.is_err());

    // creator can cancel
    client.cancel_operation(&proposer, &op_id);
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Cancelled);

    // owner can no longer cancel an already-cancelled op
    let res = client.try_cancel_operation(&owner, &op_id);
    assert!(res.is_err());
}


// --- Duplicate-signature rejection tests (#1084) ---

#[test]
fn test_duplicate_approval_does_not_inflate_approval_count() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_initialized(&env);

    let proposer = signers.get(0).unwrap();
    let op_id = client.propose_operation(
        &proposer,
        &OperationKind::DisputeResolution(Address::generate(&env), 1u128, 10, 0),
    );

    // After proposal, creator has auto-approved (count = 1)
    let approvals = client.get_approvals(&op_id);
    assert_eq!(approvals.len(), 1);

    // The same signer calling approve_operation again on the same operation
    // must not inflate the approval count.
    client.approve_operation(&proposer, &op_id);

    let approvals_after = client.get_approvals(&op_id);
    assert_eq!(
        approvals_after.len(),
        1,
        "Duplicate approval from the same signer must not increase approval count"
    );

    // Operation must stay Pending because threshold (2) is not met with only 1 distinct signer.
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Pending);
}

#[test]
fn test_operation_only_executes_with_distinct_signers_reaching_threshold() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian) = setup_initialized(&env);

    let s1 = signers.get(0).unwrap();
    let s2 = signers.get(1).unwrap();
    let s3 = signers.get(2).unwrap();

    let op_id = client.propose_operation(
        &s1,
        &OperationKind::DisputeResolution(Address::generate(&env), 2u128, 20, 0),
    );

    // s1 is auto-approved. A duplicate approval from s1 must not trigger execution.
    client.approve_operation(&s1, &op_id);
    let op_mid = client.get_operation(&op_id).unwrap();
    assert_eq!(
        op_mid.status,
        OperationStatus::Pending,
        "Operation must remain Pending after duplicate approval from the same signer"
    );

    // s2 is a distinct signer. Threshold (2) is now met → execution fires.
    client.approve_operation(&s2, &op_id);
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(
        op.status,
        OperationStatus::Executed,
        "Operation must execute only when distinct signers reach the threshold"
    );

    // After execution, any further approval from a remaining signer must be rejected.
    let res = client.try_approve_operation(&s3, &op_id);
    assert!(
        res.is_err(),
        "Approving an already-executed operation must fail"
    );

    // The approval list must contain exactly the 2 distinct signers (s1 and s2),
    // not inflated by the duplicate approval or the post-execution attempt.
    let approvals = client.get_approvals(&op_id);
    assert_eq!(
        approvals.len(),
        2,
        "Approval list must contain exactly 2 distinct signers, not duplicate entries"
    );
}


fn setup_with_rbac(
    env: &Env,
) -> (
    Address,
    MultisigContractClient<'static>,
    Address,
    Vec<Address>,
    Address,
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

    // Deploy RBAC contract and grant Arbiter role to s1
    let rbac_id = env.register_contract(None, RbacContract);
    let rbac_client = RbacClient::new(env, &rbac_id);
    rbac_client.initialize(&owner);
    rbac_client.grant_role(&owner, &s1, &Role::Arbiter);

    client.initialize(&owner, &signers, &2u32, &Some(guardian.clone()), &Some(rbac_id.clone()));

    (id, client, owner, signers, guardian, rbac_id)
}

#[test]
fn dispute_resolution_rejected_for_non_arbiter_signer() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian, _rbac_id) = setup_with_rbac(&env);

    // s2 does not have Arbiter role
    let non_arbiter = signers.get(1).unwrap();
    let result = client.try_propose_operation(
        &non_arbiter,
        &OperationKind::DisputeResolution(Address::generate(&env), 1u128, 10, 0),
    );
    assert!(result.is_err(), "Non-arbiter signer must not propose DisputeResolution");
}

#[test]
fn dispute_resolution_accepted_for_arbiter_signer() {
    let env = create_env();
    let (_id, client, _owner, signers, _guardian, _rbac_id) = setup_with_rbac(&env);

    // s1 has Arbiter role (granted in setup_with_rbac)
    let arbiter = signers.get(0).unwrap();
    let op_id = client.propose_operation(
        &arbiter,
        &OperationKind::DisputeResolution(Address::generate(&env), 1u128, 10, 0),
    );

    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Pending);
    let approvals = client.get_approvals(&op_id);
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals.get(0).unwrap(), arbiter);
}
