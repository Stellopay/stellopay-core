#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

use rbac::{RbacContract, RbacContractClient, Role};

const NEW_CONTRACT_WASM: &[u8] = include_bytes!("./stello_pay_contract.wasm");

fn setup(env: &Env) -> (PayrollContractClient<'_>, Address) {
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (client, owner)
}

fn deploy_rbac(env: &Env) -> (RbacContractClient<'_>, Address) {
    let id = env.register_contract(None, RbacContract);
    let client = RbacContractClient::new(env, &id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (client, owner)
}

#[test]
fn test_upgrade_owner_fallback_when_rbac_unset() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, owner) = setup(&env);
    let new_wasm_hash: BytesN<32> = env.deployer().upload_contract_wasm(NEW_CONTRACT_WASM);

    client.upgrade(&new_wasm_hash, &owner);
}

#[test]
#[should_panic]
fn test_upgrade_rejects_non_owner_when_rbac_unset() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, owner) = setup(&env);
    let new_wasm_hash: BytesN<32> = env.deployer().upload_contract_wasm(NEW_CONTRACT_WASM);

    let other = Address::generate(&env);
    assert_ne!(other, owner);
    client.upgrade(&new_wasm_hash, &other);
}

#[test]
fn test_upgrade_requires_rbac_admin_when_configured() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, owner) = setup(&env);
    let (rbac, rbac_owner) = deploy_rbac(&env);
    client.set_rbac_contract(&owner, &rbac.address);

    let admin = Address::generate(&env);
    rbac.grant_role(&rbac_owner, &admin, &Role::Admin);

    let new_wasm_hash: BytesN<32> = env.deployer().upload_contract_wasm(NEW_CONTRACT_WASM);
    client.upgrade(&new_wasm_hash, &admin);
}

#[test]
#[should_panic]
fn test_upgrade_rejects_non_admin_when_rbac_configured() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, owner) = setup(&env);
    let (rbac, rbac_owner) = deploy_rbac(&env);
    client.set_rbac_contract(&owner, &rbac.address);

    let employer = Address::generate(&env);
    assert_ne!(employer, owner);
    rbac.grant_role(&rbac_owner, &employer, &Role::Employer);
    assert!(!rbac.has_role(&employer, &Role::Admin));

    let new_wasm_hash: BytesN<32> = env.deployer().upload_contract_wasm(NEW_CONTRACT_WASM);
    client.upgrade(&new_wasm_hash, &employer);
}

#[test]
fn test_migrate_state_versioning_and_preserves_agreement_reads() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, owner) = setup(&env);
    let (rbac, rbac_owner) = deploy_rbac(&env);
    client.set_rbac_contract(&owner, &rbac.address);

    let admin = Address::generate(&env);
    rbac.grant_role(&rbac_owner, &admin, &Role::Admin);

    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let agreement_id = client.create_payroll_agreement(&employer, &token, &86400);
    let pre = client.get_agreement(&agreement_id).unwrap();

    client.migrate_state(&admin, &0);

    let post = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(pre.id, post.id);
    assert_eq!(pre.employer, post.employer);
    assert_eq!(pre.token, post.token);
}

#[test]
fn test_migrate_state_rejects_wrong_from_version() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, owner) = setup(&env);
    let (rbac, rbac_owner) = deploy_rbac(&env);
    client.set_rbac_contract(&owner, &rbac.address);

    let admin = Address::generate(&env);
    rbac.grant_role(&rbac_owner, &admin, &Role::Admin);

    assert!(client.try_migrate_state(&admin, &1).is_err());

    client.migrate_state(&admin, &0);

    assert!(client.try_migrate_state(&admin, &0).is_err());
}
