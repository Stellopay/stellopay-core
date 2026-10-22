//! Boundary coverage for `PayrollContract::set_multisig_config`.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};
use stello_pay_contract::{
    storage::{PayrollError, StorageKey},
    PayrollContract, PayrollContractClient,
};

fn setup(env: &Env) -> (Address, PayrollContractClient<'static>, Address) {
    env.mock_all_auths();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (contract_id, client, owner)
}

fn stored_thresholds(env: &Env, contract_id: &Address) -> (i128, i128) {
    env.as_contract(contract_id, || {
        (
            env.storage()
                .persistent()
                .get(&StorageKey::LargePaymentThreshold)
                .unwrap(),
            env.storage()
                .persistent()
                .get(&StorageKey::DisputeResolutionThreshold)
                .unwrap(),
        )
    })
}

#[test]
fn test_zero_and_one_thresholds_are_accepted() {
    let env = Env::default();
    let (contract_id, client, owner) = setup(&env);
    let multisig = Address::generate(&env);

    client.set_multisig_config(&owner, &multisig, &0, &1);

    assert_eq!(stored_thresholds(&env, &contract_id), (0, 1));
}

#[test]
fn test_maximum_i128_thresholds_are_accepted() {
    let env = Env::default();
    let (contract_id, client, owner) = setup(&env);
    let multisig = Address::generate(&env);

    client.set_multisig_config(&owner, &multisig, &i128::MAX, &i128::MAX);

    assert_eq!(
        stored_thresholds(&env, &contract_id),
        (i128::MAX, i128::MAX)
    );
}

#[test]
fn test_one_past_maximum_is_refused_by_typed_conversion() {
    let one_past_maximum = i128::MAX as u128 + 1;
    let conversion: Result<i128, _> = i128::try_from(one_past_maximum);

    assert!(conversion.is_err());
}

#[test]
fn test_uninitialized_owner_is_refused_with_typed_error() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let multisig = Address::generate(&env);

    assert_eq!(
        client.try_set_multisig_config(&owner, &multisig, &0, &1),
        Err(Ok(PayrollError::Unauthorized))
    );
}
