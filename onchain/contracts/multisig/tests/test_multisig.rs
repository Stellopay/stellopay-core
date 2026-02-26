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

    client.initialize(&owner, &signers, &2u32, &Some(guardian.clone()));

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
fn emergency_guardian_can_execute_without_threshold() {
    let env = create_env();
    let (multisig_id, client, _owner, signers, guardian) = setup_initialized(&env);

    // fund multisig
    let admin = Address::generate(&env);
    let token = create_token_contract(&env, &admin);
    let token_admin_client = StellarAssetClient::new(&env, &token.address);
    token_admin_client.mint(&multisig_id, &1_000i128);

    let recipient = Address::generate(&env);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 200i128),
    );

    // Guardian executes directly
    client.emergency_execute(&guardian, &op_id);

    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Executed);
    assert_eq!(token.balance(&recipient), 200i128);
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
