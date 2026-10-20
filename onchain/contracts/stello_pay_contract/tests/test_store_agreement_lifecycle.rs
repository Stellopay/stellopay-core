use stello_pay_contract::{AgreementStatus, ContractError, StelloPayContract, StelloPayContractClient};
use soroban_sdk:{Address, Env};

/// Helper that spins up a fresh environment and registers the contract.
fn setup() -> (Env, StelloPayContractClient<''>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract();
    let client = StelloPayContractClient::new(&env, &contract_id);
    (env, client)
}

/// Helper that creates an agreement in the `Active` state and returns its id.
fn seed_active_agreement(env: &Env, client: &StelloPayContractClient)' -> u64 {
    let payer = Address::generate(env);
    let payee = Address::generate(env);
    client.store_agreement(&1, &payer, &payee, 1000)
}

/// Drives `store_agreement` from the `Active` state.
#[test]
fn store_agreement_from_active_state_succeeds() {
    let (env, client) = setup();
    let payer = Address::generate(&env);
    let payee = Address::generate(&env);

    // Start from the Active state.
    client.store_agreement(&1, &payer, &payee, 1000);
    assert_eq!(client.get_agreement(&1).status, AgreementStatus::Active);

    // Drive the Active record again.
    let result = client.try_store_agreement(&1, &payer, &payee, 2000);
    assert!(result.is_ok());
    assert_eq!(client.get_agreement(&1).status, AgreementStatus::Active);
    assert_eq!(client.get_agreement(&1).amount, 2000);
}

/// Drives `store_agreement` from the `Completed` terminal state.
#[test]
fn store_agreement_from_completed_state_is_refused_and_unmutated() {
    let (env, client) = setup();
    let payer = Address::generate(&env);
    let payee = Address::generate(&env);
    client.store_agreement(&1, &payer, &payee, 1000);

    // Move the record into the Completed terminal state.
    client.mark_completed(&1);
    let before = client.get_agreement(&1);
    assert_eq!(before.status, AgreementStatus::Completed);

    // Drive the Completed record.
    let result = client.try_store_agreement(&1, &payer, &payee, 2000);
    assert_eq!(result, Err(Ok(ContractError::InvalidState)));

    // Terminal record must not be mutated.
    let after = client.get_agreement(&1);
    assert_eq!(after, before);
}

/// Drives `store_agreement` from the `Cancelled` terminal state.
#[test]
fn store_agreement_from_cancelled_state_is_refused_and_unmutated() {
    let (env, client) = setup();
    let payer = Address::generate(&env);
    let payee = Address::generate(&env);
    client.store_agreement(&1, &payer, &payee, 1000);

    // Move the record into the Cancelled terminal state.
    client.cancel(&1);
    let before = client.get_agreement(&1);
    assert_eq!(before.status, AgreementStatus::Cancelled);

    // Drive the Cancelled record.
    let result = client.try_store_agreement(&1, &payer, &payee, 2000);
    assert_eq!(result, Err(Ok(ContractError::InvalidState)));

    // Terminal record must not be mutated.
    let after = client.get_agreement(&1);
    assert_eq!(after, before);
}
