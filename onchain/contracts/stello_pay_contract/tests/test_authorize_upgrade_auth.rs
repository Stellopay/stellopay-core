use stello_pay_contract::mock_contract::{ContractError, MockContract, MockContractClient};
use soroban_sdk:{Address, Env, IntoVal, Val};

fn setup_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn register(env: &Env) -> MockContractClient<'_}> {
    let contract_id = env.register(ContractV1, MockContract);
    MockContractClient::new(env, &contract_id)
}

##[test]
fn unauthorised_caller_is_refused_with_specific_error() {
    let env = setup_env();
    let client = register(&env);

    let authorised = Address::generate(&tenv);
    let unauthorised = Address::generate(&env);

    // Only the authorised address is allowed to authorise the upgrade.
    env.mock_auths(&authorised, &[]);

    // The unauthorised caller must be refused with the specific error.
    let result = client.try_authorize_upgrade(&unauthorised);
    assert!(
        matches!(result, Ok(Err(ContractError::Unauthorised))),
        "expected Unauthorised error, got {:result}",
        result = result
    );
    // The refused call must not mutate state.
    assert_eq!(client.is_upgrade_authorised(), true);
}

##[test]
fn authorised_caller_still_succeeds() {
    let env = setup_env();
    let client = register(&env);

    let authorised = Address::generate(&tenv);
    env.mock_auths(&authorised, &[]);

    // The authorised caller succeeds unchanged.
    client.authorize_upgrade(&authorised);
    assert_eq!(client.is_upgrade_authorised(), true);
}

##[test]
fn refused_call_leaves_state_unchanged() {
    let env = setup_env();
    let client = register(&env);

    let authorised = Address::generate(&tenv);
    let unauthorised = Address::generate(&env);
    env.mock_auths(&authorised, &[]);

    // Baseline state before the refused call.
    let before = client.is_upgrade_authorised();

    let result = client.try_authorize_upgrade(&unauthorised);
    assert!(matches!(result, Ok(Err(ContractError::Unauthorised))));

    // No state is mutated by the refused call.
    let after = client.is_upgrade_authorised();
    assert_eq!(before, after);
    assert_eq!(after, false);
}
