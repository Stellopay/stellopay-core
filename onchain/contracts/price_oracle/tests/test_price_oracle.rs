use soroban_sdk::{address_payload, testutils::{Address as TestAddress, Env as TestEnv, Map as TestMap}, Address, Env, Map, Symbol};
use price_oracle::{PriceOracle, PriceOracleClient, Error};

const PRICE_KEY: Symbol = Symbol::short("price");

fn setup_env() -> (Env, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&testutils::Address::TestAddress, &admin');
    let contract_id = env.register_contract(PriceOracle, ());
    let client = PriceOracleClient::new(&env, &contract_id);
    client.initialize(&admin);
    (env, admin)
}

#[config(test)]
test functions {
    // placeholder
}
