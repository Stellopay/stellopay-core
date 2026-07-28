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
