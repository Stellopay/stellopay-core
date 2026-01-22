#![cfg(test)]

use soroban_sdk::{testutils::{Address as _, Events}, vec, Address, Env, IntoVal, Symbol};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn create_setup() -> (Env, PayrollContractClient<'static>, Address, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    client.initialize(&owner);

    (env, client, contract_id, owner, employer, employee, token)
}

fn create_agreement(client: &PayrollContractClient, employer: &Address, employee: &Address, token: &Address) {
    client.create_or_update_escrow(
        employer,
        employee,
        token,
        &1000_i128,
        &86400_u32,
        &2592000_u32,
    );
}

// --- Pause Tests ---

#[test]
fn test_pause_active_agreement() {
    let (_, client, _, _, employer, employee, token) = create_setup();
    create_agreement(&client, &employer, &employee, &token);

    // Initial state: Not paused
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.is_paused, false);

    // Action: Pause
    client.pause_agreement(&employee);

    // Verify: Paused
    let payroll_paused = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll_paused.is_paused, true);
}

#[test]
#[should_panic(expected = "Agreement not found")]
fn test_pause_non_active_fails() {
    let (_, client, _, _, _, employee, _) = create_setup();
    // No agreement created
    client.pause_agreement(&employee);
}

#[test]
#[should_panic(expected = "Agreement already paused")]
fn test_pause_already_paused_fails() {
    let (_, client, _, _, employer, employee, token) = create_setup();
    create_agreement(&client, &employer, &employee, &token);

    client.pause_agreement(&employee);
    client.pause_agreement(&employee);
}



#[test]
fn test_verify_pause_requires_employer_auth() {
     let (env, client, _, _, employer, employee, token) = create_setup();
    create_agreement(&client, &employer, &employee, &token);

    client.pause_agreement(&employee);
    
    let auths = env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, employer);
}

#[test]
fn test_agreement_paused_event() {
    let (env, client, contract_address, _, employer, employee, token) = create_setup();
    create_agreement(&client, &employer, &employee, &token);

    client.pause_agreement(&employee);

    let events = env.events().all();
    let event = events.last().unwrap();
    
    // valid event tuple: (contract_address, topics, data)
    assert_eq!(event.0, contract_address);
    assert_eq!(event.1, vec![&env, Symbol::new(&env, "agreement_paused").into_val(&env)]);
}

#[test]
fn test_agreement_status_changes_to_paused() {
    let (_, client, _, _, employer, employee, token) = create_setup();
    create_agreement(&client, &employer, &employee, &token);

    // Verify initial state
    let payroll_before = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll_before.is_paused, false);

    // Pause agreement
    client.pause_agreement(&employee);

    // Verify status changed
    let payroll_after = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll_after.is_paused, true);
    
    // Verify other fields remain unchanged
    assert_eq!(payroll_after.amount, payroll_before.amount);
    assert_eq!(payroll_after.employer, payroll_before.employer);
    assert_eq!(payroll_after.token, payroll_before.token);
}

#[test]
fn test_pause_resume_preserves_agreement_data() {
    let (_, client, _, _, employer, employee, token) = create_setup();
    create_agreement(&client, &employer, &employee, &token);

    let original = client.get_payroll(&employee).unwrap();
    
    // Pause and resume
    client.pause_agreement(&employee);
    client.resume_agreement(&employee);
    
    let after = client.get_payroll(&employee).unwrap();
    
    // Verify all data preserved except pause state
    assert_eq!(after.amount, original.amount);
    assert_eq!(after.employer, original.employer);
    assert_eq!(after.interval, original.interval);
    assert_eq!(after.recurrence_frequency, original.recurrence_frequency);
    assert_eq!(after.token, original.token);
    assert_eq!(after.is_paused, false);
}

#[test]
fn test_multiple_employees_independent_pause_states() {
    let (_, client, _, _, employer, employee1, token) = create_setup();
    let env = Env::default();
    env.mock_all_auths();
    let employee2 = Address::generate(&env);
    
    // Create agreements for both employees
    create_agreement(&client, &employer, &employee1, &token);
    create_agreement(&client, &employer, &employee2, &token);
    
    // Pause only employee1's agreement
    client.pause_agreement(&employee1);
    
    // Verify employee1 is paused
    assert!(client.get_payroll(&employee1).unwrap().is_paused);
    
    // Verify employee2 is NOT paused
    assert!(!client.get_payroll(&employee2).unwrap().is_paused);
}

// --- Resume Tests ---

#[test]
fn test_resume_paused_agreement() {
    let (_, client, _, _, employer, employee, token) = create_setup();
    create_agreement(&client, &employer, &employee, &token);

    client.pause_agreement(&employee);
    // Verify it's paused
    assert!(client.get_payroll(&employee).unwrap().is_paused);

    client.resume_agreement(&employee);
    // Verify it's active
    assert!(!client.get_payroll(&employee).unwrap().is_paused);
}

#[test]
#[should_panic(expected = "Agreement not paused")]
fn test_resume_non_paused_fails() {
    let (_, client, _, _, employer, employee, token) = create_setup();
    create_agreement(&client, &employer, &employee, &token);
    // It's active by default
    client.resume_agreement(&employee);
}

#[test]
fn test_verify_resume_requires_employer_auth() {
     let (env, client, _, _, employer, employee, token) = create_setup();
    create_agreement(&client, &employer, &employee, &token);
    client.pause_agreement(&employee);

    client.resume_agreement(&employee);
    
    let auths = env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, employer);
}

#[test]
fn test_agreement_resumed_event() {
    let (env, client, contract_address, _, employer, employee, token) = create_setup();
    create_agreement(&client, &employer, &employee, &token);
    client.pause_agreement(&employee);

    client.resume_agreement(&employee);

    let events = env.events().all();
    let event = events.last().unwrap();
    
    assert_eq!(event.0, contract_address);
    assert_eq!(event.1, vec![&env, Symbol::new(&env, "agreement_resumed").into_val(&env)]);
}

#[test]
fn test_agreement_status_changes_to_active() {
    let (_, client, _, _, employer, employee, token) = create_setup();
    create_agreement(&client, &employer, &employee, &token);
    
    // First pause
    client.pause_agreement(&employee);
    let payroll_paused = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll_paused.is_paused, true);
    
    // Then resume
    client.resume_agreement(&employee);
    
    // Verify status changed to active
    let payroll_active = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll_active.is_paused, false);
    
    // Verify other fields remain unchanged
    assert_eq!(payroll_active.amount, payroll_paused.amount);
    assert_eq!(payroll_active.employer, payroll_paused.employer);
    assert_eq!(payroll_active.token, payroll_paused.token);
}

// --- Claiming Tests ---

#[test]
#[should_panic(expected = "Agreement is paused")]
fn test_cannot_claim_when_paused() {
    let (_, client, _, _, employer, employee, token) = create_setup();
    create_agreement(&client, &employer, &employee, &token);
    client.pause_agreement(&employee);

    client.claim_payroll(&employee);
}

#[test]
fn test_can_claim_after_resuming() {
    let (_, client, _, _, employer, employee, token) = create_setup();
    create_agreement(&client, &employer, &employee, &token);
    client.pause_agreement(&employee);
    
    // Verify claim fails
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.claim_payroll(&employee);
    }));
    assert!(result.is_err());

    client.resume_agreement(&employee);
    
    // Verify claim succeeds
    client.claim_payroll(&employee);
}

#[test]
fn test_pause_resume_cycle_multiple_times() {
    let (_, client, _, _, employer, employee, token) = create_setup();
    create_agreement(&client, &employer, &employee, &token);

    for _ in 0..3 {
        client.pause_agreement(&employee);
        assert!(client.get_payroll(&employee).unwrap().is_paused);
        
        client.resume_agreement(&employee);
        assert!(!client.get_payroll(&employee).unwrap().is_paused);
    }
}
