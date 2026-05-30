#![cfg(test)]

use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env,
};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

use rbac::{RbacContract, RbacContractClient, Role};

const NEW_CONTRACT_WASM: &[u8] = include_bytes!("./stello_pay_contract.wasm");

// ── Setup Helpers ───────────────────────────────────────────────────────────

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

// ── Unit Tests ──────────────────────────────────────────────────────────────

#[test]
fn test_unit_upgrade_success_owner_no_rbac() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, owner) = setup(&env);
    let new_wasm_hash: BytesN<32> = env.deployer().upload_contract_wasm(NEW_CONTRACT_WASM);

    // Call upgrade as owner - should succeed
    client.upgrade(&new_wasm_hash, &owner);
}

#[test]
#[should_panic(expected = "Unauthorized")]
fn test_unit_upgrade_rejects_non_owner_no_rbac() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner) = setup(&env);
    let new_wasm_hash: BytesN<32> = env.deployer().upload_contract_wasm(NEW_CONTRACT_WASM);
    let intruder = Address::generate(&env);

    // Call upgrade as intruder - should panic
    client.upgrade(&new_wasm_hash, &intruder);
}

#[test]
fn test_unit_upgrade_success_rbac_admin() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, owner) = setup(&env);
    let (rbac, rbac_owner) = deploy_rbac(&env);
    client.set_rbac_contract(&owner, &rbac.address);

    let admin = Address::generate(&env);
    rbac.grant_role(&rbac_owner, &admin, &Role::Admin);

    let new_wasm_hash: BytesN<32> = env.deployer().upload_contract_wasm(NEW_CONTRACT_WASM);

    // Call upgrade as rbac admin - should succeed
    client.upgrade(&new_wasm_hash, &admin);
}

#[test]
#[should_panic(expected = "Missing required role")]
fn test_unit_upgrade_rejects_rbac_non_admin() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, owner) = setup(&env);
    let (rbac, rbac_owner) = deploy_rbac(&env);
    client.set_rbac_contract(&owner, &rbac.address);

    let non_admin = Address::generate(&env);
    // Grant standard Employer role, not Admin
    rbac.grant_role(&rbac_owner, &non_admin, &Role::Employer);

    let new_wasm_hash: BytesN<32> = env.deployer().upload_contract_wasm(NEW_CONTRACT_WASM);

    // Call upgrade as non-admin - should panic
    client.upgrade(&new_wasm_hash, &non_admin);
}

// ── Property Tests ──────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Property: Any non-owner caller always gets rejected with an authorization error
    /// when RBAC is unset, regardless of the WASM hash provided.
    #[test]
    fn prop_non_owner_upgrade_always_fails(
        seed_hash in prop::array::uniform32(0u8..255),
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let (client, owner) = setup(&env);
        let intruder = Address::generate(&env);
        // Avoid collision
        if intruder != owner {
            let arbitrary_hash = BytesN::from_array(&env, &seed_hash);
            let result = client.try_upgrade(&arbitrary_hash, &intruder);
            
            // Should fail with an auth error
            prop_assert!(result.is_err());
        }
    }

    /// Property: Any non-admin caller always gets rejected with an authorization error
    /// when RBAC is configured, regardless of the WASM hash provided.
    #[test]
    fn prop_non_admin_rbac_upgrade_always_fails(
        seed_hash in prop::array::uniform32(0u8..255),
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let (client, owner) = setup(&env);
        let (rbac, rbac_owner) = deploy_rbac(&env);
        client.set_rbac_contract(&owner, &rbac.address);

        let non_admin = Address::generate(&env);
        // Grant standard Employer role, not Admin
        rbac.grant_role(&rbac_owner, &non_admin, &Role::Employer);

        let arbitrary_hash = BytesN::from_array(&env, &seed_hash);
        let result = client.try_upgrade(&arbitrary_hash, &non_admin);

        // Should fail with role error
        prop_assert!(result.is_err());
    }

    /// Property: Upgrading preserves all existing state invariants (like owner, rbac link)
    /// under a successful admin upgrade.
    #[test]
    fn prop_admin_upgrade_preserves_invariants(
        grace_period in 3600u64..100_000u64,
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let (client, owner) = setup(&env);
        let (rbac, rbac_owner) = deploy_rbac(&env);
        client.set_rbac_contract(&owner, &rbac.address);

        let admin = Address::generate(&env);
        rbac.grant_role(&rbac_owner, &admin, &Role::Admin);

        // Create an agreement to test state retention
        let employer = Address::generate(&env);
        let token = Address::generate(&env);
        let agreement_id = client.create_payroll_agreement(&employer, &token, &grace_period);
        let agreement_before = client.get_agreement(&agreement_id).unwrap();

        // Perform the upgrade
        let new_wasm_hash: BytesN<32> = env.deployer().upload_contract_wasm(NEW_CONTRACT_WASM);
        client.upgrade(&new_wasm_hash, &admin);

        // Retrieve agreement again to verify state is preserved after the upgrade boundary
        let agreement_after = client.get_agreement(&agreement_id).unwrap();
        prop_assert_eq!(agreement_before.id, agreement_after.id);
        prop_assert_eq!(agreement_before.employer, agreement_after.employer);
        prop_assert_eq!(agreement_before.token, agreement_after.token);
        prop_assert_eq!(agreement_before.grace_period_seconds, agreement_after.grace_period_seconds);
    }
}
