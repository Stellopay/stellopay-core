use crate::{PayrollEscrowContract, PayrollEscrowContractClient};
use soroban_sdk::{testutils::{Address as _, Events, Ledger}, Address, Env, IntoVal, Symbol, vec};

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
    
    // contract should not be initialized initially, but we can't check internal storage directly easily from client
    // Initialize
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
     // only admin can call functions that require admin auth (though initialize is the only one currently)
     // The contract doesn't explicitly expose admin getter.
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
