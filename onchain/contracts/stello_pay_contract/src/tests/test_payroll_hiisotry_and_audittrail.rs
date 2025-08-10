use soroban_sdk::{testutils::Address as _, Address, Env, log, symbol_short};
use soroban_sdk::token::{StellarAssetClient as TokenAdmin, TokenClient};
use soroban_sdk::testutils::{Ledger, LedgerInfo};
use crate::payroll::{PayrollContract, PayrollContractClient};

fn create_test_contract() -> (Env, Address, PayrollContractClient<'static>) {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    (env, contract_id, client)
}

fn setup_token(env: &Env) -> (Address, TokenAdmin) {
    let token_admin = Address::generate(env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone());
    (
        token_contract_id.address(),
        TokenAdmin::new(&env, &token_contract_id.address()),
    )
}
#[test]
fn test_record_new_escrow() {
    let (env, contract_id, client) = create_test_contract();

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64; // 30 days in seconds

    env.mock_all_auths();

    client.initialize(&employer);

    let _created_payroll =
        client.create_or_update_escrow(
        &employer, 
        &employee,
        &token, 
        &amount, 
        &interval, 
        &recurrence_frequency
    );

    let entries = client.get_payroll_history(&employee, &None, &None, &Some(5));
    assert_eq!(entries.len(), 1); 
    assert_eq!(entries.get(0).unwrap().action, symbol_short!("created"));

}

#[test]
fn test_payroll_history_query() {
    let (env, contract_id, client) = create_test_contract();

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64;

    env.mock_all_auths();

    client.initialize(&employer);

    // Set different ledger timestamps to create history entries
    env.ledger().with_mut(|l| l.timestamp = 1000);
    client.create_or_update_escrow(
        &employer, 
        &employee,
        &token, 
        &amount, 
        &interval, 
        &recurrence_frequency
    );

    env.ledger().with_mut(|l| l.timestamp = 2000);
    client.pause_employee_payroll(&employer, &employee);

    env.ledger().with_mut(|l| l.timestamp = 3000);
    client.resume_employee_payroll(&employer, &employee);

    env.ledger().with_mut(|l| l.timestamp = 4000);
    client.create_or_update_escrow(&employer, &employee, &token, &(amount * 2), &interval, &recurrence_frequency);

    // Test 1: Query all entries (no timestamp filters, default limit)
    let entries = client.get_payroll_history(&employee, &None, &None, &Some(5));
    log!(&env, "All history entries: {:?}", entries);
    assert_eq!(entries.len(), 4);
    assert_eq!(entries.get(0).unwrap().action, symbol_short!("created"));
    assert_eq!(entries.get(1).unwrap().action, symbol_short!("paused"));
    assert_eq!(entries.get(2).unwrap().action, symbol_short!("resumed"));
    assert_eq!(entries.get(3).unwrap().action, symbol_short!("updated"));

    // Test 2: Query with start_timestamp (only entries after timestamp 1500)
    let entries = client.get_payroll_history(&employee, &Some(1500), &None, &Some(5));
    log!(&env, "History entries after 1500: {:?}", entries);
    assert_eq!(entries.len(), 3);
    assert_eq!(entries.get(0).unwrap().timestamp, 2000);
    assert_eq!(entries.get(1).unwrap().timestamp, 3000);
    assert_eq!(entries.get(2).unwrap().timestamp, 4000);

    // Test 3: Query with end_timestamp (only entries before timestamp 2500)
    let entries = client.get_payroll_history(&employee, &None, &Some(2500), &Some(5));
    log!(&env, "History entries before 2500: {:?}", entries);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries.get(0).unwrap().timestamp, 1000);
    assert_eq!(entries.get(1).unwrap().timestamp, 2000);

    // Test 4: Query with both start_timestamp and end_timestamp (between 1500 and 3500)
    let entries = client.get_payroll_history(&employee, &Some(1500), &Some(3500), &Some(5));
    log!(&env, "History entries between 1500 and 3500: {:?}", entries);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries.get(0).unwrap().timestamp, 2000);
    assert_eq!(entries.get(1).unwrap().timestamp, 3000);

    // Test 5: Query with limit (only first 2 entries)
    let entries = client.get_payroll_history(&employee, &None, &None, &Some(2));
    log!(&env, "History entries with limit 2: {:?}", entries);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries.get(0).unwrap().timestamp, 1000);
    assert_eq!(entries.get(1).unwrap().timestamp, 2000);
}

#[test]
fn test_payroll_history_edge_cases() {
    let (env, contract_id, client) = create_test_contract();

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let amount = 1000i128;
    let interval = 86400u64;
    let recurrence_frequency = 2592000u64;

    env.mock_all_auths();

    client.initialize(&employer);

    // Create some history entries
    env.ledger().with_mut(|l| l.timestamp = 1000);
    client.create_or_update_escrow(
        &employer, 
        &employee, 
        &token, 
        &amount, 
        &interval, 
        &recurrence_frequency
    );

    env.ledger().with_mut(|l| l.timestamp = 2000);
    client.pause_employee_payroll(&employer, &employee);

    // Test 1: Query for non-existent employee
    let non_existent_employee = Address::generate(&env);
    let entries = client.get_payroll_history(&non_existent_employee, &None, &None, &Some(5));
    assert_eq!(entries.len(), 0); 

    // Test 2: Invalid timestamp range (start > end)
    let entries = client.get_payroll_history(&employee, &Some(3000), &Some(1000), &Some(5));
    assert_eq!(entries.len(), 0);

    // Test 3: Timestamps outside history range
    let entries = client.get_payroll_history(&employee, &Some(5000), &Some(6000), &Some(5));
    assert_eq!(entries.len(), 0);

    // Test 4: Zero limit
    let entries = client.get_payroll_history(&employee, &None, &None, &Some(0));
    assert_eq!(entries.len(), 0);
}


#[test]
fn test_audit_trail_disburse_success() {
    let (env, contract_id, client) = create_test_contract();
    let (token_address, token_admin) = setup_token(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let amount = 1000i128;
    let interval = 86400u64;
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

    // Create escrow
    client.create_or_update_escrow(
        &employer, 
        &employee,
        &token_address,
        &amount, 
        &interval, 
        &recurrence_frequency
    );

    // Advance timestamp to allow disbursement
    let disbursement_timestamp = env.ledger().timestamp() + recurrence_frequency + 1;
    env.ledger().set(LedgerInfo {
        timestamp: disbursement_timestamp,
        protocol_version: 22,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6312000,
    });

    // Perform disbursement
    client.disburse_salary(&employer, &employee);

    // Verify employee received tokens
    let employee_balance = token_client.balance(&employee);
    assert_eq!(employee_balance, amount);

    // Query audit trail
    let entries = client.get_audit_trail(&employee, &None, &None, &Some(5));
    log!(&env, "Audit trail entries: {:?}", entries);
    assert_eq!(entries.len(), 1); // Expect 1 disbursement entry
    let entry = entries.get(0).unwrap();
    assert_eq!(entry.action, symbol_short!("disbursed"));
    assert_eq!(entry.employee, employee);
    assert_eq!(entry.employer, employer);
    assert_eq!(entry.token, token_address);
    assert_eq!(entry.amount, amount);
    assert_eq!(entry.timestamp, disbursement_timestamp);
    assert_eq!(entry.last_payment_time, disbursement_timestamp);
    assert_eq!(
        entry.next_payout_timestamp, 
        disbursement_timestamp + recurrence_frequency
    );
    assert_eq!(entry.id, 1); // First audit entry
}