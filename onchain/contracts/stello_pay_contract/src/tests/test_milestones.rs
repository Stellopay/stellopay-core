#![cfg(test)]
use soroban_sdk::{testutils::{Address as _, Events}, Address, Env, IntoVal, symbol_short};
use crate::{StelloPayContract, StelloPayContractClient};

#[test]
fn test_approve_milestone_success() {
    let env = Env::default();
    let approver = Address::generate(&env);
    let employee = Address::generate(&env);
    let client = StelloPayContractClient::new(&env, &env.register_contract(None, StelloPayContract));

    // Setup: Create agreement and milestone...
    let milestone_id = setup_agreement_with_milestone(&env, &client, &approver, &employee);

    // Call as approver
    env.mock_all_auths();
    client.approve_milestone(&milestone_id);
    
    // Verify status changed (assuming a getter exists)
    assert_eq!(client.get_milestone(&milestone_id).status, 2); // 2 = Approved
}

#[test]
#[should_panic(expected = "Status(ContractError(1))")] // PayrollError::NotAuthorized
fn test_fail_unrelated_address_approval() {
    let env = Env::default();
    let approver = Address::generate(&env);
    let attacker = Address::generate(&env);
    let client = StelloPayContractClient::new(&env, &env.register_contract(None, StelloPayContract));

    let milestone_id = setup_agreement_with_milestone(&env, &client, &approver, &Address::generate(&env));

    // Attempt to approve as attacker
    // We mock auth for the attacker, but the contract requires auth for the 'approver'
    env.mock_auths(&[
        soroban_sdk::testutils::MockAuth {
            address: &attacker,
            invoke: &soroban_sdk::testutils::MockAuthInvoke {
                contract: &client.address,
                function: "approve_milestone",
                args: (milestone_id,).into_val(&env),
                sub_invocations: &[],
            },
        }
    ]);

    client.approve_milestone(&milestone_id);
}

#[test]
#[should_panic(expected = "Status(ContractError(1))")]
fn test_fail_employee_self_approval() {
    let env = Env::default();
    let approver = Address::generate(&env);
    let employee = Address::generate(&env);
    let client = StelloPayContractClient::new(&env, &env.register_contract(None, StelloPayContract));

    let milestone_id = setup_agreement_with_milestone(&env, &client, &approver, &employee);

    // Employee tries to sign for their own milestone approval
    env.mock_auths(&[
        soroban_sdk::testutils::MockAuth {
            address: &employee,
            invoke: &soroban_sdk::testutils::MockAuthInvoke {
                contract: &client.address,
                function: "approve_milestone",
                args: (milestone_id,).into_val(&env),
                sub_invocations: &[],
            },
        }
    ]);

    client.approve_milestone(&milestone_id);
}
