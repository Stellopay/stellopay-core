use super::{PayrollContract, PayrollContractClient};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

const NEW_CONTRACT_WASM: &[u8] =
    include_bytes!("../../../target/wasm32-unknown-unknown/release/stello_pay_contract.wasm");

#[test]
fn test_owner_initialization() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);

    client.initialize(&owner);

    assert_eq!(client.get_owner(), Some(owner));
}

#[test]
fn test_upgrade_flow() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);

    client.initialize(&owner);

    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    let new_wasm_hash: BytesN<32> = env.deployer().upload_contract_wasm(NEW_CONTRACT_WASM);

    client.upgrade(&new_wasm_hash, &owner);
}
