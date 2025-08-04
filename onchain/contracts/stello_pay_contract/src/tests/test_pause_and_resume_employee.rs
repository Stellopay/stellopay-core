use soroban_sdk::{testutils::Address as _, Address, Env};
use crate::payroll::{PayrollContractClient, PayrollError};
use soroban_sdk::token::{StellarAssetClient as TokenAdmin, TokenClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo, MockAuth, MockAuthInvoke},
     IntoVal, Vec,
};

fn setup_token(env: &Env) -> (Address, TokenAdmin) {
    let token_admin = Address::generate(env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone());
    (
        token_contract_id.address(),
        TokenAdmin::new(&env, &token_contract_id.address()),
    )
}

#[test]
fn test_pause_resume_employee_payroll_by_owner() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    
    let owner = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days

    env.mock_all_auths();

    // Initialize contract and create payroll
    client.initialize(&owner);
    client.create_or_update_escrow(&owner, &employee, &token, &amount, &interval, &recurrence_frequency);

    // Default should be false
    client.resume_employee_payroll(&owner, &employee);
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.is_paused, false);

    // Test pause
    client.pause_employee_payroll(&owner, &employee);
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.is_paused, true);

    // Test resume
    client.resume_employee_payroll(&owner, &employee);
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.is_paused, false);
}

#[test]
fn test_pause_resume_employee_payroll_by_employer() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    
    let owner = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64;

    env.mock_all_auths();

    // Initialize contract and create payroll
    client.initialize(&owner);
    client.create_or_update_escrow(&owner, &employee, &token, &amount, &interval, &recurrence_frequency);

    // Transfer ownership to employer
    client.transfer_ownership(&owner, &employer);

    // Test pause by employer
    client.pause_employee_payroll(&employer, &employee);
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.is_paused, true);

    // Test resume by employer
    client.resume_employee_payroll(&employer, &employee);
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.is_paused, false);
}


#[test]
#[should_panic(expected = "HostError: Error(Contract, #1)")]
fn test_pause_employee_payroll_unauthorized() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    
    let owner = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64;

    env.mock_all_auths();

    // Initialize contract and create payroll
    client.initialize(&owner);
    client.create_or_update_escrow(&owner, &employee, &token, &amount, &interval, &recurrence_frequency);

    client.resume_employee_payroll(&unauthorized, &employee);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #1)")]
fn test_resume_employee_payroll_unauthorized() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    
    let owner = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64;

    env.mock_all_auths();

    client.initialize(&owner);
    client.create_or_update_escrow(&owner, &employee, &token, &amount, &interval, &recurrence_frequency);
    client.pause_employee_payroll(&owner, &employee);

    // Should fail when non-owner/non-employer tries to resume
    client.resume_employee_payroll(&unauthorized, &employee);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #4)")]
fn test_pause_nonexistent_employee_payroll() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    
    let owner = Address::generate(&env);
    let employee = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&owner);

    // Should fail when trying to pause non-existent payroll
    client.pause_employee_payroll(&owner, &employee);
}



#[test]
#[should_panic(expected = "HostError: Error(Contract, #7)")]
fn test_pause_disburse_salary_paused_employee() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    
    let owner = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64;

    env.mock_all_auths();

    client.initialize(&owner);
    client.create_or_update_escrow(&owner, &employee, &token, &amount, &interval, &recurrence_frequency);
    client.pause_employee_payroll(&owner, &employee);

    // Should fail when trying to disburse to paused employee
    client.disburse_salary(&owner, &employee);
}


#[test]
fn test_pause_resume_disburse_salary_success() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64; // 1 day in seconds
    let recurrence_frequency = 2592000u64; // 30 days in seconds

    env.mock_all_auths();

    // Fund the employer with tokens
    token_admin.mint(&employer, &10000);

    // Verify minting
    let token_client = TokenClient::new(&env, &token_address);
    let employer_balance = token_client.balance(&employer);
    assert_eq!(employer_balance, 10000);

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &5000i128);

    // Verify deposit
    let payroll_contract_balance = token_client.balance(&contract_id);
    assert_eq!(payroll_contract_balance, 5000);

    // Create escrow first
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &amount,
        &interval,
        &recurrence_frequency,
    );

    // Advance time beyond next payout timestamp
    let next_timestamp = env.ledger().timestamp() + recurrence_frequency + 1;
    env.ledger().set(LedgerInfo {
        timestamp: next_timestamp,
        protocol_version: 22,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6312000,
    });

    client.pause_employee_payroll(&employer, &employee);

    client.resume_employee_payroll(&employer, &employee);

    client.disburse_salary(&employer, &employee);

    // Verify employee received tokens
    let employee_balance = token_client.balance(&employee);
    assert_eq!(employee_balance, amount);

    // Verify last_payment_time was updated
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.last_payment_time, env.ledger().timestamp());
}



#[test]
#[should_panic(expected = "HostError: Error(Contract, #7)")]
fn test_pause_disburse_salary_fail() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64; // 1 day in seconds
    let recurrence_frequency = 2592000u64; // 30 days in seconds

    env.mock_all_auths();

    // Fund the employer with tokens
    token_admin.mint(&employer, &10000);

    // Verify minting
    let token_client = TokenClient::new(&env, &token_address);
    let employer_balance = token_client.balance(&employer);
    assert_eq!(employer_balance, 10000);

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &5000i128);

    // Verify deposit
    let payroll_contract_balance = token_client.balance(&contract_id);
    assert_eq!(payroll_contract_balance, 5000);

    // Create escrow first
    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &amount,
        &interval,
        &recurrence_frequency,
    );

    // Advance time beyond next payout timestamp
    let next_timestamp = env.ledger().timestamp() + recurrence_frequency + 1;
    env.ledger().set(LedgerInfo {
        timestamp: next_timestamp,
        protocol_version: 22,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6312000,
    });

    client.pause_employee_payroll(&employer, &employee);

    client.disburse_salary(&employer, &employee);

    // Verify employee received tokens
    let employee_balance = token_client.balance(&employee);
    assert_eq!(employee_balance, amount);

    // Verify last_payment_time was updated
    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.last_payment_time, env.ledger().timestamp());
}


#[test]
fn test_pause_process_recurring_disbursements() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days

    env.mock_all_auths();

    // Fund the employer with tokens
    token_admin.mint(&employer, &10000);

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &5000i128);

    // Create escrows for two employees
    client.create_or_update_escrow(&employer, &employee1, &token_address, &amount, &interval, &recurrence_frequency);
    client.create_or_update_escrow(&employer, &employee2, &token_address, &amount, &interval, &recurrence_frequency);

    // Advance time beyond next payout timestamp
    let next_timestamp = env.ledger().timestamp() + recurrence_frequency + 1;
    env.ledger().set(LedgerInfo {
        timestamp: next_timestamp,
        protocol_version: 22,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6312000,
    });

    client.pause_employee_payroll(&employer, &employee1);

    // Create vector of employees
    let mut employees = Vec::new(&env);
    employees.push_back(employee1.clone());
    employees.push_back(employee2.clone());

    // Process recurring disbursements
    let processed_employees = client.process_recurring_disbursements(&employer, &employees);
    assert_eq!(processed_employees.len(), 1);

    // Verify both employees received tokens
    let token_client = TokenClient::new(&env, &token_address);
    let employee1_balance = token_client.balance(&employee1);
    let employee2_balance = token_client.balance(&employee2);
    assert_eq!(employee1_balance, 0);
    assert_eq!(employee2_balance, amount);
}


#[test]
fn test_pause_resume_process_recurring_disbursements() {
    let env = Env::default();
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, token_admin) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);

    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days

    env.mock_all_auths();

    // Fund the employer with tokens
    token_admin.mint(&employer, &10000);

    // Initialize contract and deposit tokens
    client.initialize(&employer);
    client.deposit_tokens(&employer, &token_address, &5000i128);

    // Create escrows for two employees
    client.create_or_update_escrow(&employer, &employee1, &token_address, &amount, &interval, &recurrence_frequency);
    client.create_or_update_escrow(&employer, &employee2, &token_address, &amount, &interval, &recurrence_frequency);

    // Advance time beyond next payout timestamp
    let next_timestamp = env.ledger().timestamp() + recurrence_frequency + 1;
    env.ledger().set(LedgerInfo {
        timestamp: next_timestamp,
        protocol_version: 22,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6312000,
    });

    client.pause_employee_payroll(&employer, &employee1);

    // Create vector of employees
    let mut employees = Vec::new(&env);
    employees.push_back(employee1.clone());
    employees.push_back(employee2.clone());

    client.resume_employee_payroll(&employer, &employee1);

    // Process recurring disbursements
    let processed_employees = client.process_recurring_disbursements(&employer, &employees);
    assert_eq!(processed_employees.len(), 2);

    // Verify both employees received tokens
    let token_client = TokenClient::new(&env, &token_address);
    let employee1_balance = token_client.balance(&employee1);
    let employee2_balance = token_client.balance(&employee2);
    assert_eq!(employee1_balance, amount);
    assert_eq!(employee2_balance, amount);
}