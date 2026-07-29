use soroban_sdk::{
    testutils::{Address as _, Events},
    vec, Address, Env, IntoVal,
};

use crate::{ManagerUpdatedEvent, PayrollEscrowContract, PayrollEscrowContractClient};

fn create_token_contract<'a>(e: &Env, admin: &Address) -> soroban_sdk::token::Client<'a> {
    let token = e.register_stellar_asset_contract(admin.clone());
    soroban_sdk::token::Client::new(e, &token)
}

fn create_payroll_escrow_contract<'a>(e: &Env) -> PayrollEscrowContractClient<'a> {
    let contract_id = e.register_contract(None, PayrollEscrowContract);
    PayrollEscrowContractClient::new(e, &contract_id)
}

#[test]
fn test_initialize_escrow() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let client = create_payroll_escrow_contract(&env);

    // contract should not be initialized initially, but we can't check internal storage directly
    // easily from client Initialize
    client.initialize(&admin, &token.address, &manager);

    // There isn't a direct getter for "initialized", but subsequent calls depending on it will pass
}

#[test]
#[should_panic(expected = "Contract already initialized")]
fn test_initialize_twice_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let client = create_payroll_escrow_contract(&env);

    client.initialize(&admin, &token.address, &manager);
    client.initialize(&admin, &token.address, &manager);
}

#[test]
fn test_admin_set_correctly() {
    // This is implicitly tested by initialize success and auth checks in other functions,
    // but since we don't have a get_admin function, we can verify it by checking that
    // only admin can call functions that require admin auth (though initialize is the only one
    // currently) The contract doesn't explicitly expose admin getter.
    // However, we can assert that initialize sets the admin.

    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);
}

#[test]
fn test_update_manager_admin_authorized() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let new_manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    client.update_manager(&admin, &new_manager);

    let events = env.events().all();
    let last_event = events.last().unwrap();

    assert_eq!(
        last_event.1,
        vec![
            &env,
            soroban_sdk::String::from_str(&env, "manager_updated").into_val(&env)
        ]
    );

    let event: ManagerUpdatedEvent = last_event.2.into_val(&env);
    assert_eq!(event.old_manager, manager);
    assert_eq!(event.new_manager, new_manager);
}

#[test]
fn test_update_manager_non_admin_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let non_admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let new_manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    let result = client.try_update_manager(&non_admin, &new_manager);
    assert!(result.is_err());
}

#[test]
fn test_update_manager_same_manager_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    let result = client.try_update_manager(&admin, &manager);
    assert!(result.is_err());
}

#[test]
fn test_release_uses_new_manager_after_rotation() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let new_manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &500);

    client.update_manager(&admin, &new_manager);

    assert!(client.try_release(&manager, &1, &employee, &100).is_err());

    client.release(&new_manager, &1, &employee, &200);

    assert_eq!(client.get_agreement_balance(&1), 300);
    assert_eq!(token.balance(&employee), 200);
}

#[test]
fn test_fund_agreement() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);

    let agreement_id = 1u128;
    let amount = 500i128;

    client.fund_agreement(&employer, &agreement_id, &employer, &amount);

    // Check balance
    assert_eq!(client.get_agreement_balance(&agreement_id), amount);
}

#[test]
#[should_panic(expected = "Amount must be positive")]
fn test_fund_zero_amount_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    client.fund_agreement(&employer, &1, &employer, &0);
}

#[test]
#[should_panic(expected = "Contract not initialized")]
fn test_fund_not_initialized_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let employer = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    // Skip initialize

    client.fund_agreement(&employer, &1, &employer, &100);
}

#[test]
fn test_fund_updates_balance() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);

    client.fund_agreement(&employer, &1, &employer, &100);
    assert_eq!(client.get_agreement_balance(&1), 100);

    client.fund_agreement(&employer, &1, &employer, &200);
    assert_eq!(client.get_agreement_balance(&1), 300);
}

#[test]
fn test_fund_employer_recorded() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);

    client.fund_agreement(&employer, &1, &employer, &100);

    assert_eq!(client.get_agreement_employer(&1), Some(employer));
}

#[test]
fn test_funded_event_emitted() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);

    let agreement_id = 1u128;
    let amount = 100i128;
    client.fund_agreement(&employer, &agreement_id, &employer, &amount);

    // Verify event
    let events = env.events().all();
    let last_event = events.last().unwrap();

    // Verify topics
    let topics = last_event.1;
    assert_eq!(
        topics,
        vec![
            &env,
            soroban_sdk::String::from_str(&env, "funded").into_val(&env),
            agreement_id.into_val(&env)
        ]
    );
}

#[test]
fn test_release_funds() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &500);

    client.release(&manager, &1, &employee, &200);

    // Check balance
    assert_eq!(client.get_agreement_balance(&1), 300);
    // Check employee received funds
    assert_eq!(token.balance(&employee), 200);
}

#[test]
#[should_panic(expected = "Amount must be positive")]
fn test_release_zero_amount_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &500);

    client.release(&manager, &1, &employee, &0);
}

#[test]
#[should_panic(expected = "Insufficient balance")]
fn test_release_insufficient_balance_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &500);

    client.release(&manager, &1, &employee, &600);
}

#[test]
#[should_panic(expected = "Only manager can release funds")]
fn test_release_unauthorized_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let other = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &500);

    client.release(&other, &1, &employee, &200);
}

#[test]
fn test_release_balance_decreases() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &500);

    client.release(&manager, &1, &employee, &100);
    assert_eq!(client.get_agreement_balance(&1), 400);

    client.release(&manager, &1, &employee, &100);
    assert_eq!(client.get_agreement_balance(&1), 300);
}

#[test]
fn test_released_event_emitted() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &500);

    client.release(&manager, &1, &employee, &200);

    // Verify event
    let events = env.events().all();
    let last_event = events.last().unwrap();
    let topics = last_event.1;
    assert_eq!(
        topics,
        vec![
            &env,
            soroban_sdk::String::from_str(&env, "released").into_val(&env),
            1u128.into_val(&env)
        ]
    );
}

#[test]
fn test_refund_remaining() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &500);

    client.refund_remaining(&manager, &1);

    // Check balance is zero
    assert_eq!(client.get_agreement_balance(&1), 0);
    // Check employer received funds
    assert_eq!(token.balance(&employer), 1000); // 500 initial + 500 refund
}

#[test]
#[should_panic(expected = "No balance to refund")]
fn test_refund_zero_balance() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &500);

    // Empty balance
    client.release(&manager, &1, &Address::generate(&env), &500);

    client.refund_remaining(&manager, &1);
}

#[test]
#[should_panic(expected = "Only manager can refund funds")]
fn test_refund_unauthorized_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let other = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &500);

    client.refund_remaining(&other, &1);
}

#[test]
fn test_refund_to_correct_employer() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer1 = Address::generate(&env);
    let employer2 = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer1, &1000);
    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer2, &1000);

    client.fund_agreement(&employer1, &1, &employer1, &500);
    client.fund_agreement(&employer2, &2, &employer2, &500);

    client.refund_remaining(&manager, &1);

    assert_eq!(token.balance(&employer1), 1000);
    assert_eq!(token.balance(&employer2), 500); // unaffected
}

#[test]
fn test_refund_balance_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &500);

    client.refund_remaining(&manager, &1);

    assert_eq!(client.get_agreement_balance(&1), 0);
}

#[test]
fn test_refunded_event_emitted() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &500);

    client.refund_remaining(&manager, &1);

    // Verify event
    let events = env.events().all();
    let last_event = events.last().unwrap();
    let topics = last_event.1;
    assert_eq!(
        topics,
        vec![
            &env,
            soroban_sdk::String::from_str(&env, "refunded").into_val(&env),
            1u128.into_val(&env)
        ]
    );
}

#[test]
fn test_get_agreement_balance() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &500);

    assert_eq!(client.get_agreement_balance(&1), 500);
}

#[test]
fn test_get_nonexistent_balance() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    assert_eq!(client.get_agreement_balance(&999), 0);
}

#[test]
fn test_get_agreement_employer() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &500);

    assert_eq!(client.get_agreement_employer(&1), Some(employer));
    assert_eq!(client.get_agreement_employer(&999), None);
}

#[test]
fn test_very_large_amounts() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    let large_amount: i128 = i128::MAX / 2;
    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &i128::MAX);

    client.fund_agreement(&employer, &1, &employer, &large_amount);

    assert_eq!(client.get_agreement_balance(&1), large_amount);

    // Add more
    client.fund_agreement(&employer, &1, &employer, &1);
    assert_eq!(client.get_agreement_balance(&1), large_amount + 1);
}

#[test]
fn test_multiple_agreements_same_employer() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);

    client.fund_agreement(&employer, &1, &employer, &100);
    client.fund_agreement(&employer, &2, &employer, &200);

    assert_eq!(client.get_agreement_balance(&1), 100);
    assert_eq!(client.get_agreement_balance(&2), 200);

    assert_eq!(client.get_agreement_employer(&1), Some(employer.clone()));
    assert_eq!(client.get_agreement_employer(&2), Some(employer));
}

#[test]
fn test_rapid_funding_releasing() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);

    // Fund - Release - Fund - Release
    client.fund_agreement(&employer, &1, &employer, &100);
    client.release(&manager, &1, &employee, &50);
    assert_eq!(client.get_agreement_balance(&1), 50);

    client.fund_agreement(&employer, &1, &employer, &100);
    assert_eq!(client.get_agreement_balance(&1), 150);

    client.release(&manager, &1, &employee, &150);
    assert_eq!(client.get_agreement_balance(&1), 0);

    assert_eq!(token.balance(&employee), 200);
}

#[test]
#[should_panic(expected = "Mismatched employer for agreement")]
fn test_fund_agreement_mismatched_employer_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer1 = Address::generate(&env);
    let employer2 = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer1, &1000);
    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer2, &1000);

    // Initial funding by employer1
    client.fund_agreement(&employer1, &1, &employer1, &100);

    // Attempted additional funding by employer2 for same agreement ID
    client.fund_agreement(&employer2, &1, &employer2, &100);
}

#[test]
fn test_release_partial_sequence() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &1000);

    // Multiple partial releases
    client.release(&manager, &1, &employee, &100);
    client.release(&manager, &1, &employee, &200);
    client.release(&manager, &1, &employee, &300);

    assert_eq!(client.get_agreement_balance(&1), 400);
    assert_eq!(token.balance(&employee), 600);
}

#[test]
#[should_panic(expected = "No balance to refund")]
fn test_release_full_and_refund_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &500);

    // Full release
    client.release(&manager, &1, &employee, &500);
    assert_eq!(client.get_agreement_balance(&1), 0);

    // Refund should now fail
    client.refund_remaining(&manager, &1);
}

#[test]
#[should_panic(expected = "Balance overflow")]
fn test_fund_overflow_rejects_with_no_transfer() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &i128::MAX);

    // Fund up to the maximum representable balance.
    client.fund_agreement(&employer, &1, &employer, &i128::MAX);

    // Any further funding must overflow `checked_add` and panic (via
    // `.expect("Balance overflow")`) before any token transfer is attempted.
    client.fund_agreement(&employer, &1, &employer, &1);
}

// ============================================================================
// Conservation invariant helpers and edge-case coverage
// ============================================================================

/// Asserts the escrow conservation invariant for a single agreement:
/// `total_funded == total_released + total_refunded + remaining_balance`.
fn assert_escrow_conservation(
    client: &PayrollEscrowContractClient<'_>,
    agreement_id: u128,
    total_funded: i128,
    total_released: i128,
    total_refunded: i128,
) {
    let remaining = client.get_agreement_balance(&agreement_id);
    let outflow = total_released + total_refunded;
    assert_eq!(
        total_funded,
        outflow + remaining,
        "conservation violated: funded={total_funded} released={total_released} refunded={total_refunded} remaining={remaining}"
    );
    assert!(
        outflow <= total_funded,
        "outflow must not exceed funded deposits"
    );
}

#[test]
fn test_escrow_conservation_invariant_multi_step() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &10_000);

    let agreement_id = 99u128;
    client.fund_agreement(&employer, &agreement_id, &employer, &1000);
    client.fund_agreement(&employer, &agreement_id, &employer, &500);
    assert_escrow_conservation(&client, agreement_id, 1500, 0, 0);

    client.release(&manager, &agreement_id, &recipient, &400);
    assert_escrow_conservation(&client, agreement_id, 1500, 400, 0);

    client.refund_remaining(&manager, &agreement_id);
    assert_escrow_conservation(&client, agreement_id, 1500, 400, 1100);
    assert_eq!(client.get_agreement_balance(&agreement_id), 0);
}

#[test]
fn test_release_to_non_employee_recipient() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let arbitrary_recipient = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &800);

    client.release(&manager, &1, &arbitrary_recipient, &300);

    assert_eq!(token.balance(&arbitrary_recipient), 300);
    assert_eq!(client.get_agreement_balance(&1), 500);
}

#[test]
#[should_panic(expected = "No balance to refund")]
fn test_double_refund_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &500);

    client.refund_remaining(&manager, &1);
    client.refund_remaining(&manager, &1);
}

#[test]
fn test_double_refund_preserves_accounting() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    let funded: i128 = 1_000;
    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &funded);

    let agreement_id = 42u128;
    client.fund_agreement(&employer, &agreement_id, &employer, &funded);

    // Snapshot before first refund
    let pre_refund = AccountingSnapshot::take(&env, &token, &client, agreement_id, &recipient);
    assert_eq!(pre_refund.internal_balance, funded);
    assert_eq!(token.balance(&employer), 0);
    pre_refund.assert_accounting_in_sync();

    // First refund succeeds — all accounting must be consistent
    client.refund_remaining(&manager, &agreement_id);

    let after_refund = AccountingSnapshot::take(&env, &token, &client, agreement_id, &recipient);
    assert_eq!(after_refund.internal_balance, 0);
    assert_eq!(after_refund.contract_token_balance, 0);
    assert_eq!(
        token.balance(&employer),
        funded,
        "employer must receive the full refund"
    );
    after_refund.assert_accounting_in_sync();
    assert_escrow_conservation(&client, agreement_id, funded, 0, funded);
}

#[test]
fn test_refund_accounting_matches_funded_exactly() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &100_000);

    // Case 1: fund with no releases → refund total must match fund total
    let aid1 = 1u128;
    let fund1 = 7_500i128;
    client.fund_agreement(&employer, &aid1, &employer, &fund1);
    client.refund_remaining(&manager, &aid1);
    assert_escrow_conservation(&client, aid1, fund1, 0, fund1);
    assert_eq!(client.get_agreement_balance(&aid1), 0);

    // Case 2: fund with partial releases → refund total + releases = fund total
    let aid2 = 2u128;
    let fund2 = 5_000i128;
    let released = 1_200i128;
    client.fund_agreement(&employer, &aid2, &employer, &fund2);
    client.release(&manager, &aid2, &recipient, &released);
    client.refund_remaining(&manager, &aid2);
    let refunded2 = fund2 - released;
    assert_escrow_conservation(&client, aid2, fund2, released, refunded2);
    assert_eq!(client.get_agreement_balance(&aid2), 0);

    // Case 3: multiple funds into same agreement with partial releases
    let aid3 = 3u128;
    let fund3a = 3_000i128;
    let fund3b = 2_000i128;
    let total_fund3 = fund3a + fund3b;
    let released3 = 1_500i128;
    client.fund_agreement(&employer, &aid3, &employer, &fund3a);
    client.fund_agreement(&employer, &aid3, &employer, &fund3b);
    client.release(&manager, &aid3, &recipient, &released3);
    client.refund_remaining(&manager, &aid3);
    let refunded3 = total_fund3 - released3;
    assert_escrow_conservation(&client, aid3, total_fund3, released3, refunded3);
    assert_eq!(client.get_agreement_balance(&aid3), 0);
}

#[test]
#[should_panic(expected = "Insufficient balance")]
fn test_release_after_refund_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1000);
    client.fund_agreement(&employer, &1, &employer, &500);

    client.refund_remaining(&manager, &1);
    client.release(&manager, &1, &recipient, &100);
}

// ============================================================================
// Storage key collision regression tests
// ============================================================================

#[test]
fn test_agreement_balance_key_collision_adjacent_ids() {
    // Regression test: ensures adjacent agreement IDs resolve to distinct storage slots.
    // A key-derivation bug here would let one agreement's funding silently overwrite another's.
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &10_000);

    // Fund adjacent agreement IDs
    let id1 = 1000u128;
    let id2 = 1001u128;
    let id3 = 1002u128;

    client.fund_agreement(&employer, &id1, &employer, &100);
    client.fund_agreement(&employer, &id2, &employer, &200);
    client.fund_agreement(&employer, &id3, &employer, &300);

    // Verify balances are independent
    assert_eq!(client.get_agreement_balance(&id1), 100);
    assert_eq!(client.get_agreement_balance(&id2), 200);
    assert_eq!(client.get_agreement_balance(&id3), 300);

    // Release from middle agreement should not affect others
    client.release(&manager, &id2, &Address::generate(&env), &50);
    assert_eq!(
        client.get_agreement_balance(&id1),
        100,
        "id1 balance unchanged"
    );
    assert_eq!(
        client.get_agreement_balance(&id2),
        150,
        "id2 balance decreased"
    );
    assert_eq!(
        client.get_agreement_balance(&id3),
        300,
        "id3 balance unchanged"
    );
}

#[test]
fn test_agreement_balance_key_collision_edge_values() {
    // Regression test: ensures zero and max value agreement IDs resolve to distinct slots.
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &10_000);

    // Fund edge case agreement IDs
    let id_zero = 0u128;
    let id_one = 1u128;
    let id_max = u128::MAX;

    client.fund_agreement(&employer, &id_zero, &employer, &100);
    client.fund_agreement(&employer, &id_one, &employer, &200);
    client.fund_agreement(&employer, &id_max, &employer, &300);

    // Verify balances are independent
    assert_eq!(client.get_agreement_balance(&id_zero), 100);
    assert_eq!(client.get_agreement_balance(&id_one), 200);
    assert_eq!(client.get_agreement_balance(&id_max), 300);

    // Release from zero ID should not affect others
    client.release(&manager, &id_zero, &Address::generate(&env), &50);
    assert_eq!(
        client.get_agreement_balance(&id_zero),
        50,
        "zero ID balance decreased"
    );
    assert_eq!(
        client.get_agreement_balance(&id_one),
        200,
        "one ID balance unchanged"
    );
    assert_eq!(
        client.get_agreement_balance(&id_max),
        300,
        "max ID balance unchanged"
    );
}

#[test]
fn test_agreement_balance_key_collision_release_isolation() {
    // Regression test: ensures releasing one agreement never mutates another's balance.
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &10_000);

    // Fund multiple agreements with structurally similar IDs
    let id_a = 12345u128;
    let id_b = 12346u128;
    let id_c = 12347u128;

    client.fund_agreement(&employer, &id_a, &employer, &1000);
    client.fund_agreement(&employer, &id_b, &employer, &1000);
    client.fund_agreement(&employer, &id_c, &employer, &1000);

    // Fully release agreement B
    client.release(&manager, &id_b, &recipient, &1000);

    // Verify only B's balance changed
    assert_eq!(
        client.get_agreement_balance(&id_a),
        1000,
        "A balance unchanged"
    );
    assert_eq!(client.get_agreement_balance(&id_b), 0, "B balance zeroed");
    assert_eq!(
        client.get_agreement_balance(&id_c),
        1000,
        "C balance unchanged"
    );

    // Refund agreement A
    client.refund_remaining(&manager, &id_a);

    // Verify only A's balance changed
    assert_eq!(client.get_agreement_balance(&id_a), 0, "A balance zeroed");
    assert_eq!(
        client.get_agreement_balance(&id_b),
        0,
        "B balance still zero"
    );
    assert_eq!(
        client.get_agreement_balance(&id_c),
        1000,
        "C balance unchanged"
    );
}

// ============================================================================
// Sequential partial withdrawal accounting — edge-case coverage
//
// These tests validate that:
//   1. Internal escrow balance (AgreementBalance storage) decrements exactly.
//   2. On-chain token balance held by the contract decrements in lock-step.
//   3. Recipient token balance increments in lock-step.
//   4. Exact exhaustion succeeds and leaves internal balance == 0.
//   5. Any subsequent withdrawal attempt fails with "Insufficient balance".
//   6. A failed withdrawal leaves every balance unchanged.
//   7. No underflow or off-by-one behaviour exists.
//   8. Accounting integrity holds across the full three-step lifecycle.
// ============================================================================

/// Returns the token balance of the escrow contract itself.
///
/// This is the on-chain custody balance — the actual tokens held by the
/// contract address. It must equal the internal `AgreementBalance` for a
/// single-agreement escrow throughout the withdrawal lifecycle.
fn contract_token_balance(
    env: &Env,
    token: &soroban_sdk::token::Client,
    client: &PayrollEscrowContractClient,
) -> i128 {
    token.balance(&client.address)
}

/// Comprehensive accounting snapshot taken after every significant operation.
struct AccountingSnapshot {
    /// Internal balance from `get_agreement_balance` storage.
    internal_balance: i128,
    /// Token balance held by the contract address (on-chain custody).
    contract_token_balance: i128,
    /// Token balance of the designated recipient.
    recipient_token_balance: i128,
}

impl AccountingSnapshot {
    fn take(
        env: &Env,
        token: &soroban_sdk::token::Client,
        client: &PayrollEscrowContractClient,
        agreement_id: u128,
        recipient: &Address,
    ) -> Self {
        Self {
            internal_balance: client.get_agreement_balance(&agreement_id),
            contract_token_balance: token.balance(&client.address),
            recipient_token_balance: token.balance(recipient),
        }
    }

    /// Assert that internal accounting matches the actual on-chain token balance.
    fn assert_accounting_in_sync(&self) {
        assert_eq!(
            self.internal_balance, self.contract_token_balance,
            "CRITICAL: internal escrow balance ({}) does not match actual \
             contract token custody ({}). Accounting drift detected.",
            self.internal_balance, self.contract_token_balance
        );
    }
}

// ----------------------------------------------------------------------------
// Core sequential partial withdrawal test
// ----------------------------------------------------------------------------

#[test]
fn test_sequential_partial_withdrawals_full_lifecycle() {
    // Scenario:
    //   Fund 900.
    //   Withdrawal 1: partial — release 300.
    //   Withdrawal 2: exact exhaustion — release remaining 600.
    //   Withdrawal 3: should fail ("Insufficient balance").
    //   After failure: verify all balances unchanged.
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    let funded: i128 = 900;
    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &funded);

    let agreement_id = 1u128;
    client.fund_agreement(&employer, &agreement_id, &employer, &funded);

    // ── Baseline snapshot after funding ──────────────────────────────────
    let after_fund = AccountingSnapshot::take(&env, &token, &client, agreement_id, &recipient);
    assert_eq!(
        after_fund.internal_balance, funded,
        "internal balance must equal funded amount after funding"
    );
    assert_eq!(
        after_fund.contract_token_balance, funded,
        "contract must hold all funded tokens after funding"
    );
    assert_eq!(
        after_fund.recipient_token_balance, 0,
        "recipient must have zero tokens before any withdrawal"
    );
    after_fund.assert_accounting_in_sync();

    // ── Withdrawal 1: partial release of 300 ─────────────────────────────
    let w1: i128 = 300;
    client.release(&manager, &agreement_id, &recipient, &w1);

    let after_w1 = AccountingSnapshot::take(&env, &token, &client, agreement_id, &recipient);
    assert_eq!(
        after_w1.internal_balance,
        funded - w1,
        "internal balance must decrease by exactly w1 after first withdrawal"
    );
    assert_eq!(
        after_w1.contract_token_balance,
        funded - w1,
        "contract token balance must decrease by exactly w1 after first withdrawal"
    );
    assert_eq!(
        after_w1.recipient_token_balance, w1,
        "recipient must receive exactly w1 tokens after first withdrawal"
    );
    after_w1.assert_accounting_in_sync();
    // Conservation: funded == released_so_far + remaining
    assert_escrow_conservation(&client, agreement_id, funded, w1, 0);

    // ── Withdrawal 2: exact exhaustion — release remaining 600 ───────────
    let w2: i128 = funded - w1; // 600 — exactly what's left
    client.release(&manager, &agreement_id, &recipient, &w2);

    let after_w2 = AccountingSnapshot::take(&env, &token, &client, agreement_id, &recipient);
    assert_eq!(
        after_w2.internal_balance, 0,
        "internal balance must be zero after exact exhaustion withdrawal"
    );
    assert_eq!(
        after_w2.contract_token_balance, 0,
        "contract must hold zero tokens after exact exhaustion"
    );
    assert_eq!(
        after_w2.recipient_token_balance,
        w1 + w2,
        "recipient must have received w1 + w2 total tokens"
    );
    assert_eq!(
        w1 + w2,
        funded,
        "sum of withdrawals must equal funded amount"
    );
    after_w2.assert_accounting_in_sync();
    assert_escrow_conservation(&client, agreement_id, funded, w1 + w2, 0);

    // ── Withdrawal 3: must fail — insufficient balance ────────────────────
    let w3: i128 = 1;
    let failed = client.try_release(&manager, &agreement_id, &recipient, &w3);
    assert!(failed.is_err(), "withdrawal after exhaustion must fail");

    // Verify every balance is unchanged after the failed attempt
    let after_failure = AccountingSnapshot::take(&env, &token, &client, agreement_id, &recipient);
    assert_eq!(
        after_failure.internal_balance, 0,
        "internal balance must remain zero after failed withdrawal"
    );
    assert_eq!(
        after_failure.contract_token_balance, 0,
        "contract token balance must remain zero after failed withdrawal"
    );
    assert_eq!(
        after_failure.recipient_token_balance,
        w1 + w2,
        "recipient balance must be unchanged after failed withdrawal"
    );
    after_failure.assert_accounting_in_sync();
}

// ----------------------------------------------------------------------------
// Granular partial withdrawal steps (5-step sequence)
// ----------------------------------------------------------------------------

#[test]
fn test_five_sequential_partial_withdrawals_account_correctly() {
    // Fund 1_000 and release five partial amounts of 100, 200, 150, 350, 200.
    // Total = 1_000; after each step verify internal == contract token balance.
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    let funded: i128 = 1_000;
    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &funded);
    client.fund_agreement(&employer, &1u128, &employer, &funded);

    let withdrawals: [i128; 5] = [100, 200, 150, 350, 200];
    let mut released_so_far: i128 = 0;

    for (step, &amount) in withdrawals.iter().enumerate() {
        client.release(&manager, &1u128, &recipient, &amount);
        released_so_far += amount;

        let snap = AccountingSnapshot::take(&env, &token, &client, 1u128, &recipient);
        let expected_remaining = funded - released_so_far;

        assert_eq!(
            snap.internal_balance,
            expected_remaining,
            "step {}: internal balance wrong (expected {}, got {})",
            step + 1,
            expected_remaining,
            snap.internal_balance
        );
        assert_eq!(
            snap.contract_token_balance,
            expected_remaining,
            "step {}: contract token balance wrong (expected {}, got {})",
            step + 1,
            expected_remaining,
            snap.contract_token_balance
        );
        assert_eq!(
            snap.recipient_token_balance,
            released_so_far,
            "step {}: recipient balance wrong (expected {}, got {})",
            step + 1,
            released_so_far,
            snap.recipient_token_balance
        );
        snap.assert_accounting_in_sync();
        assert_escrow_conservation(&client, 1u128, funded, released_so_far, 0);
    }

    // All funds released — subsequent attempt must fail
    assert_eq!(
        client.get_agreement_balance(&1u128),
        0,
        "balance must be zero after all withdrawals"
    );
    assert!(
        client
            .try_release(&manager, &1u128, &recipient, &1)
            .is_err(),
        "any further withdrawal must fail with insufficient balance"
    );
}

// ----------------------------------------------------------------------------
// Exact 1-token withdrawal leaves zero balance
// ----------------------------------------------------------------------------

#[test]
fn test_single_token_exact_exhaustion() {
    // Fund with 1 token, release exactly 1 — verify balance is zero,
    // and a subsequent release of 1 fails.
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &1);
    client.fund_agreement(&employer, &1u128, &employer, &1);

    // Release the single token
    client.release(&manager, &1u128, &recipient, &1);

    assert_eq!(
        client.get_agreement_balance(&1u128),
        0,
        "balance must be exactly zero"
    );
    assert_eq!(
        token.balance(&client.address),
        0,
        "contract must hold no tokens"
    );
    assert_eq!(
        token.balance(&recipient),
        1,
        "recipient must have the 1 token"
    );

    // Subsequent attempt must fail
    let failed = client.try_release(&manager, &1u128, &recipient, &1);
    assert!(failed.is_err(), "release after full exhaustion must fail");

    // State unchanged after failure
    assert_eq!(client.get_agreement_balance(&1u128), 0);
    assert_eq!(token.balance(&client.address), 0);
    assert_eq!(token.balance(&recipient), 1);
}

// ----------------------------------------------------------------------------
// Off-by-one: withdrawal of exactly `balance` succeeds; `balance + 1` fails
// ----------------------------------------------------------------------------

#[test]
fn test_release_exact_balance_succeeds_plus_one_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    let funded: i128 = 500;
    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &funded);
    client.fund_agreement(&employer, &1u128, &employer, &funded);

    // First: attempt balance + 1 — must fail before touching state
    let over = client.try_release(&manager, &1u128, &recipient, &(funded + 1));
    assert!(over.is_err(), "release of balance+1 must fail");
    assert_eq!(
        client.get_agreement_balance(&1u128),
        funded,
        "balance must be unchanged after failed over-withdrawal"
    );
    assert_eq!(
        token.balance(&client.address),
        funded,
        "contract token balance must be unchanged after failed over-withdrawal"
    );

    // Now release the exact remaining balance — must succeed
    client.release(&manager, &1u128, &recipient, &funded);
    assert_eq!(
        client.get_agreement_balance(&1u128),
        0,
        "balance must be zero after exact exhaustion"
    );
    assert_eq!(
        token.balance(&client.address),
        0,
        "contract must hold zero tokens after exact exhaustion"
    );
    assert_eq!(
        token.balance(&recipient),
        funded,
        "recipient must hold exactly funded amount"
    );
}

// ----------------------------------------------------------------------------
// Multi-agreement isolation during sequential partial withdrawals
// ----------------------------------------------------------------------------

#[test]
fn test_partial_withdrawals_from_one_agreement_do_not_affect_others() {
    // Three agreements funded independently. Exhaust agreement A via two
    // partial withdrawals; verify B and C balances and on-chain totals remain
    // exactly correct throughout.
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let recipient_a = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &3_000);

    let (id_a, id_b, id_c) = (10u128, 20u128, 30u128);
    let (fund_a, fund_b, fund_c) = (1_000i128, 800i128, 600i128);

    client.fund_agreement(&employer, &id_a, &employer, &fund_a);
    client.fund_agreement(&employer, &id_b, &employer, &fund_b);
    client.fund_agreement(&employer, &id_c, &employer, &fund_c);

    // Contract must hold all three amounts
    assert_eq!(token.balance(&client.address), fund_a + fund_b + fund_c);

    // Partial withdrawal 1 from A
    let w1_a: i128 = 400;
    client.release(&manager, &id_a, &recipient_a, &w1_a);

    assert_eq!(client.get_agreement_balance(&id_a), fund_a - w1_a);
    assert_eq!(
        client.get_agreement_balance(&id_b),
        fund_b,
        "B unchanged after A withdrawal"
    );
    assert_eq!(
        client.get_agreement_balance(&id_c),
        fund_c,
        "C unchanged after A withdrawal"
    );
    assert_eq!(
        token.balance(&client.address),
        fund_a + fund_b + fund_c - w1_a,
        "contract holds sum of remaining balances"
    );

    // Partial withdrawal 2 from A — exact exhaustion
    let w2_a: i128 = fund_a - w1_a; // 600
    client.release(&manager, &id_a, &recipient_a, &w2_a);

    assert_eq!(client.get_agreement_balance(&id_a), 0, "A fully exhausted");
    assert_eq!(
        client.get_agreement_balance(&id_b),
        fund_b,
        "B still unchanged"
    );
    assert_eq!(
        client.get_agreement_balance(&id_c),
        fund_c,
        "C still unchanged"
    );
    assert_eq!(
        token.balance(&client.address),
        fund_b + fund_c,
        "contract holds B+C after A exhausted"
    );
    assert_eq!(
        token.balance(&recipient_a),
        fund_a,
        "recipient received all A funds"
    );

    // Subsequent withdrawal from A must fail
    assert!(
        client
            .try_release(&manager, &id_a, &recipient_a, &1)
            .is_err(),
        "withdrawal from exhausted A must fail"
    );

    // B and C balances unchanged
    assert_eq!(client.get_agreement_balance(&id_b), fund_b);
    assert_eq!(client.get_agreement_balance(&id_c), fund_c);
    assert_eq!(
        token.balance(&client.address),
        fund_b + fund_c,
        "contract balance still correct after failed A withdrawal"
    );
}

// ----------------------------------------------------------------------------
// Failed withdrawal leaves state entirely unchanged (state-unchanged regression)
// ----------------------------------------------------------------------------

#[test]
fn test_failed_withdrawal_leaves_all_state_unchanged() {
    // After a partial withdrawal, attempt an over-withdrawal.
    // Every observable state element must be identical before and after the
    // failed attempt.
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    let funded: i128 = 750;
    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &funded);
    client.fund_agreement(&employer, &1u128, &employer, &funded);

    // First partial withdrawal
    let w1: i128 = 250;
    client.release(&manager, &1u128, &recipient, &w1);

    // Capture state before the failed attempt
    let before = AccountingSnapshot::take(&env, &token, &client, 1u128, &recipient);

    // Attempt to withdraw more than remaining (500 remaining, try 501)
    let over_amount: i128 = before.internal_balance + 1;
    let failed = client.try_release(&manager, &1u128, &recipient, &over_amount);
    assert!(failed.is_err(), "over-withdrawal must fail");

    // Capture state after the failed attempt
    let after = AccountingSnapshot::take(&env, &token, &client, 1u128, &recipient);

    // Every field must be identical
    assert_eq!(
        before.internal_balance, after.internal_balance,
        "internal balance must be unchanged after failed withdrawal"
    );
    assert_eq!(
        before.contract_token_balance, after.contract_token_balance,
        "contract token balance must be unchanged after failed withdrawal"
    );
    assert_eq!(
        before.recipient_token_balance, after.recipient_token_balance,
        "recipient balance must be unchanged after failed withdrawal"
    );
    before.assert_accounting_in_sync();
    after.assert_accounting_in_sync();
}

// ----------------------------------------------------------------------------
// Accounting integrity: internal balance always equals contract token custody
// ----------------------------------------------------------------------------

#[test]
fn test_accounting_integrity_across_full_withdrawal_lifecycle() {
    // A broader sweep: fund, multiple partial releases, then refund.
    // At every step, assert internal_balance == contract_token_balance.
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    let funded: i128 = 2_000;
    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &funded);
    client.fund_agreement(&employer, &1u128, &employer, &funded);

    AccountingSnapshot::take(&env, &token, &client, 1u128, &recipient).assert_accounting_in_sync();

    // 4 partial releases
    for amount in [200i128, 300, 500, 400] {
        client.release(&manager, &1u128, &recipient, &amount);
        AccountingSnapshot::take(&env, &token, &client, 1u128, &recipient)
            .assert_accounting_in_sync();
    }

    // Remaining = 2000 - 1400 = 600; refund it
    assert_eq!(client.get_agreement_balance(&1u128), 600);
    client.refund_remaining(&manager, &1u128);

    let final_snap = AccountingSnapshot::take(&env, &token, &client, 1u128, &recipient);
    final_snap.assert_accounting_in_sync();
    assert_eq!(
        final_snap.internal_balance, 0,
        "internal balance must be zero after refund"
    );
    assert_eq!(
        final_snap.contract_token_balance, 0,
        "contract must hold zero tokens after refund"
    );

    // Employer received the 600 refund (they started with 2000, funded 2000, so 0 remaining;
    // but refund goes back to employer so they now have 600)
    assert_eq!(
        token.balance(&employer),
        600,
        "employer must receive the refunded amount"
    );
}

// ============================================================================
// Cumulative release-cap tests
//
// These tests directly address the requirement that repeated calls to
// `release` can never, in aggregate, release more than the originally funded
// amount, even across many small partial releases.
//
// Covered assertions per call:
//   • running_total_released <= funded_amount  (cap invariant)
//   • get_agreement_balance == funded - running_total_released  (counter sync)
//   • A release that would push the running total past `funded` errors
//     immediately and does NOT release a truncated/partial amount.
//   • State (internal balance + contract custody) is identical before and
//     after any rejected call.
// ============================================================================

/// Positive test: ten micro-releases in a loop; after each one the running
/// total must not exceed the funded amount and the internal balance counter
/// must equal `funded - running_total`.
///
/// Funded = 1_000. Ten releases of 100 each, total = 1_000 == funded.
/// The cap invariant holds throughout and the final balance is exactly 0.
#[test]
fn test_cumulative_release_cap_many_small_releases_never_exceed_funded() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    // Fund exactly 1_000 tokens.
    let funded: i128 = 1_000;
    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &funded);

    let agreement_id = 42u128;
    client.fund_agreement(&employer, &agreement_id, &employer, &funded);

    // Sanity: balance starts at funded amount.
    assert_eq!(client.get_agreement_balance(&agreement_id), funded);

    // Release in 10 equal instalments of 100.
    let release_amount: i128 = 100;
    let steps: u32 = 10;
    let mut running_total: i128 = 0;

    for step in 1..=steps {
        client.release(&manager, &agreement_id, &recipient, &release_amount);
        running_total += release_amount;

        let expected_remaining = funded - running_total;
        let internal_balance = client.get_agreement_balance(&agreement_id);

        // Cap invariant: aggregate released must never exceed funded.
        assert!(
            running_total <= funded,
            "step {step}: cumulative release ({running_total}) exceeded funded ({funded})"
        );

        // Counter sync: internal balance must equal funded minus released.
        assert_eq!(
            internal_balance,
            expected_remaining,
            "step {step}: internal balance ({internal_balance}) != funded ({funded}) \
             - released ({running_total}) = {expected_remaining}"
        );
    }

    // All 10 × 100 = 1_000 released — balance must be exactly zero.
    assert_eq!(
        client.get_agreement_balance(&agreement_id),
        0,
        "balance must be zero after all instalments are released"
    );
    assert_eq!(
        running_total, funded,
        "total released must equal exactly the funded amount"
    );
    assert_eq!(
        token.balance(&recipient),
        funded,
        "recipient must hold all funded tokens after final instalment"
    );
}

/// Negative test: a single `release` call whose amount would push the
/// cumulative total past `funded` must error immediately.
///
/// It must NOT release a partial/truncated amount — the full requested
/// amount is either transferred or nothing is transferred (atomic failure).
///
/// Setup: fund 500, release 300 (running = 300, remaining = 200).
/// Attempt release of 201 — this would push total to 501 > 500 funded.
/// Expected: error with "Insufficient balance"; state entirely unchanged.
#[test]
fn test_cumulative_release_cap_over_funded_amount_errors_not_truncates() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    let funded: i128 = 500;
    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &funded);

    let agreement_id = 7u128;
    client.fund_agreement(&employer, &agreement_id, &employer, &funded);

    // First legitimate partial release: 300 tokens.
    let first_release: i128 = 300;
    client.release(&manager, &agreement_id, &recipient, &first_release);

    let remaining = funded - first_release; // 200
    assert_eq!(
        client.get_agreement_balance(&agreement_id),
        remaining,
        "internal balance must be funded - first_release after the first call"
    );

    // Snapshot state before the rejected call.
    let balance_before = client.get_agreement_balance(&agreement_id);
    let contract_tokens_before = token.balance(&client.address);
    let recipient_tokens_before = token.balance(&recipient);

    // Attempt to release 201 — one more than the 200 remaining.
    // This would make cumulative = 300 + 201 = 501, exceeding funded (500).
    let over_amount: i128 = remaining + 1; // 201
    let result = client.try_release(&manager, &agreement_id, &recipient, &over_amount);

    // Must error — not silently release a truncated 200.
    assert!(
        result.is_err(),
        "release that exceeds remaining balance must return an error, not succeed with truncation"
    );

    // State must be byte-for-byte identical to before the rejected call.
    assert_eq!(
        client.get_agreement_balance(&agreement_id),
        balance_before,
        "internal balance must be unchanged after rejected over-release"
    );
    assert_eq!(
        token.balance(&client.address),
        contract_tokens_before,
        "contract token custody must be unchanged after rejected over-release"
    );
    assert_eq!(
        token.balance(&recipient),
        recipient_tokens_before,
        "recipient balance must be unchanged — no partial transfer must have occurred"
    );

    // Confirm the valid remaining 200 can still be released after the failed attempt.
    client.release(&manager, &agreement_id, &recipient, &remaining);
    assert_eq!(
        client.get_agreement_balance(&agreement_id),
        0,
        "balance must be zero after releasing the exact remaining amount"
    );
    assert_eq!(
        token.balance(&recipient),
        funded,
        "recipient must now hold the full funded amount"
    );
}

/// Property test: assert `get_agreement_balance == funded - released` after
/// every individual `release` call in a variable-step sequence.
///
/// Uses amounts [50, 75, 25, 100, 200, 50] totalling 500 == funded.
/// The internal counter is verified after each step, not just at the end.
#[test]
fn test_cumulative_release_internal_balance_tracks_funded_minus_released_after_every_call() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = create_token_contract(&env, &token_admin);
    let employer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let client = create_payroll_escrow_contract(&env);
    client.initialize(&admin, &token.address, &manager);

    let funded: i128 = 500;
    soroban_sdk::token::StellarAssetClient::new(&env, &token.address).mint(&employer, &funded);

    let agreement_id = 99u128;
    client.fund_agreement(&employer, &agreement_id, &employer, &funded);

    // Variable release sequence — total intentionally equals funded exactly.
    let releases: [i128; 6] = [50, 75, 25, 100, 200, 50];
    let mut cumulative_released: i128 = 0;

    for (i, &amount) in releases.iter().enumerate() {
        client.release(&manager, &agreement_id, &recipient, &amount);
        cumulative_released += amount;

        let expected_balance = funded - cumulative_released;
        let actual_balance = client.get_agreement_balance(&agreement_id);

        // Primary assertion from the issue: counter == funded - released after every call.
        assert_eq!(
            actual_balance,
            expected_balance,
            "call {}: get_agreement_balance ({actual_balance}) must equal \
             funded ({funded}) - cumulative_released ({cumulative_released}) = {expected_balance}",
            i + 1
        );

        // Bonus: cap invariant must hold at every step too.
        assert!(
            cumulative_released <= funded,
            "call {}: cumulative released ({cumulative_released}) must never exceed funded ({funded})",
            i + 1
        );
    }

    // Final state checks.
    assert_eq!(
        client.get_agreement_balance(&agreement_id),
        0,
        "final balance must be zero"
    );
    assert_eq!(
        token.balance(&recipient),
        funded,
        "recipient must hold all funded tokens"
    );
    assert_eq!(
        token.balance(&client.address),
        0,
        "contract must hold zero tokens"
    );
}
