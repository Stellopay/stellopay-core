use crate::payroll::PayrollContractClient;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_initialize_contract() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&owner);

    assert_eq!(client.get_owner(), Some(owner));
    assert_eq!(client.is_paused(), false);
}

#[test]
#[should_panic(expected = "HostError: Error(WasmVm, InvalidAction)")]
fn test_initialize_twice_should_panic() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&owner);
    client.initialize(&owner);
}

#[test]
fn test_pause_unpause_by_owner() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&owner);
    assert_eq!(client.is_paused(), false);

    client.pause(&owner);
    assert_eq!(client.is_paused(), true);

    client.unpause(&owner);
    assert_eq!(client.is_paused(), false);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #1)")]
fn test_pause_by_non_owner_fails() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let non_owner = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&owner);
    client.pause(&non_owner);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #1)")]
fn test_unpause_by_non_owner_fails() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let non_owner = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&owner);
    client.pause(&owner);
    assert_eq!(client.is_paused(), true);

    client.unpause(&non_owner);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #6)")]
fn test_create_escrow_when_paused_fails() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&owner);
    client.pause(&owner);

    client.create_or_update_escrow(&employer, &employee, &token, &1000, &86400);
}

#[test]
fn test_get_payroll_works_when_paused() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let _employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000;
    let interval = 86400;

    env.mock_all_auths();

    client.initialize(&owner);
    let created_payroll = client.create_or_update_escrow(&owner, &employee, &token, &amount, &interval);
    client.pause(&owner);
    let stored_payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(created_payroll, stored_payroll);
}

#[test]
fn test_transfer_ownership() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let new_owner = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&owner);
    assert_eq!(client.get_owner(), Some(owner.clone()));

    client.transfer_ownership(&owner, &new_owner);
    assert_eq!(client.get_owner(), Some(new_owner));
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #1)")]
fn test_transfer_ownership_by_non_owner_fails() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let non_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&owner);
    client.transfer_ownership(&non_owner, &new_owner);
}

#[test]
fn test_try_initialize() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);

    env.mock_all_auths();

    assert_eq!(client.try_initialize(&owner), Ok(Ok(())));
}