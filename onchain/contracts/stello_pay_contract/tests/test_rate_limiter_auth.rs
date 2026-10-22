use soroban_sdk::{testutils::Address as _, Address, Env};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn setup() -> (Env, PayrollContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

#[test]
fn test_set_rate_limiter_contract_requires_auth() {
    let (env, client, admin) = setup();
    let new_contract = Address::generate(&env);

    // Record the current state so we can prove the refused call mutates nothing.
    let before = client.get_rate_limiter_contract();

    // No auths are mocked here, so the admin's auth is missing.
    env.mock_auths(&[]);
    let result = client.try_set_rate_limiter_contract(&admin, &new_contract);
    assert!(result.is_err());

    // State is unchanged by the refused call.
    assert_eq!(client.get_rate_limiter_contract(), before);

    // The authorised caller still succeeds unchanged.
    env.mock_all_auths();
    client.set_rate_limiter_contract(&admin, &new_contract);
    assert_eq!(client.get_rate_limiter_contract(), Some(new_contract));
}
