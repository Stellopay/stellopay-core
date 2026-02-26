#![cfg(test)]

use soroban_sdk::{
    testutils::Address as _,
    Address, Env, Symbol, Vec,
};

use tax_withholding::{TaxComputation, TaxError, TaxWithholdingContract, TaxWithholdingContractClient};

fn setup() -> (Env, Address, TaxWithholdingContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, TaxWithholdingContract);
    let client = TaxWithholdingContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    (env, owner, client)
}

#[test]
fn test_single_jurisdiction_withholding() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_CA");

    client.set_jurisdiction_rate(&owner, &jurisdiction, &1500u32); // 15%

    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    let gross: i128 = 10_000;
    let result: TaxComputation = client.calculate_withholding(&employee, &gross);

    assert_eq!(result.gross_amount, gross);
    assert_eq!(result.total_tax, 1_500);
    assert_eq!(result.net_amount, 8_500);
    assert_eq!(result.shares.len(), 1);

    let share = result.shares.get(0).unwrap();
    assert_eq!(share.jurisdiction, jurisdiction);
    assert_eq!(share.amount, 1_500);
}

#[test]
fn test_multi_jurisdiction_withholding() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let j1 = Symbol::new(&env, "US_FED");
    let j2 = Symbol::new(&env, "US_STATE");

    client.set_jurisdiction_rate(&owner, &j1, &1000u32); // 10%
    client.set_jurisdiction_rate(&owner, &j2, &500u32);  // 5%

    let jurisdictions = Vec::from_array(&env, [j1.clone(), j2.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    let gross: i128 = 20_000;
    let result: TaxComputation = client.calculate_withholding(&employee, &gross);

    assert_eq!(result.total_tax, 3_000); // 10% + 5%
    assert_eq!(result.net_amount, 17_000);
    assert_eq!(result.shares.len(), 2);
}

#[test]
fn test_not_configured_employee() {
    let (env, _owner, client) = setup();

    let employee = Address::generate(&env);
    let gross: i128 = 5_000;

    let res = client.try_calculate_withholding(&employee, &gross);
    assert_eq!(res, Err(Ok(TaxError::NotConfigured)));
}

