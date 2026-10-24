#![cfg(test)]

use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn setup() -> (Env, PayrollContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let rate_limiter = Address::generate(&env);
    // `set_rate_limiter_contract` is owner-gated, so the contract must be
    // initialised with `admin` as owner before the entrypoint will accept it.
    client.initialize(&admin);
    (env, client, admin, rate_limiter)
}

#[test]
fn test_set_rate_limiter_contract_twice_idempotent() {
    let (_env, client, admin, rate_limiter) = setup();

    // First call
    client.set_rate_limiter_contract(&admin, &rate_limiter);
    let after_first = client.get_rate_limiter_contract();
    assert_eq!(after_first, Some(rate_limiter.clone()));

    // Second call with the same address
    client.set_rate_limiter_contract(&admin, &rate_limiter);
    let after_second = client.get_rate_limiter_contract();
    assert_eq!(after_second, Some(rate_limiter.clone()));

    // State is identical to a single call
    assert_eq!(after_first, after_second);
}
