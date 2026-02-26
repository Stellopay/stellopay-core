#![cfg(test)]

use soroban_sdk::{testutils::{Address as _, Ledger}, Address, Bytes, Env,};
use compliance_reporting::{
    ComplianceError, ComplianceReportingContract, ComplianceReportingContractClient, ReportType,
};

fn setup_test() -> (Env, ComplianceReportingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, ComplianceReportingContract);
    let client = ComplianceReportingContractClient::new(&env, &contract_id);
    
    client.initialize(&admin);
    
    (env, client, admin)
}

#[test]
fn test_initialize() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ComplianceReportingContract);
    let client = ComplianceReportingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    client.initialize(&admin);
    
    let result = client.try_initialize(&admin);
    assert_eq!(result, Err(Ok(ComplianceError::AlreadyInitialized)));
}

#[test]
fn test_log_record() {
    let (env, client, _) = setup_test();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    
    env.ledger().set_timestamp(1000);
    
    let metadata = Bytes::new(&env);
    
    let id = client.log_record(
        &employer, 
        &employee, 
        &token, 
        &5000i128, 
        &ReportType::Payroll, 
        &metadata
    );
    
    assert_eq!(id, 1);
    assert_eq!(client.get_record_count(&employer), 1);
}

#[test]
fn test_generate_report_date_filtering() {
    let (env, client, _) = setup_test();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let metadata = Bytes::new(&env);

    // Record 1 (t=1000)
    env.ledger().set_timestamp(1000);
    client.log_record(&employer, &employee, &token, &100, &ReportType::Payroll, &metadata);
    
    // Record 2 (t=2000)
    env.ledger().set_timestamp(2000);
    client.log_record(&employer, &employee, &token, &200, &ReportType::Tax, &metadata);
    
    // Record 3 (t=3000)
    env.ledger().set_timestamp(3000);
    client.log_record(&employer, &employee, &token, &300, &ReportType::Regulatory, &metadata);

    // Query range 1500 to 2500 (Should only catch Record 2)
    let report = client.generate_report(&employer, &1500, &2500, &None, &50);
    
    assert_eq!(report.record_count, 1);
    assert_eq!(report.total_amount, 200);
    assert_eq!(report.records.get(0).unwrap().report_type, ReportType::Tax);
}

#[test]
fn test_generate_report_type_filtering() {
    let (env, client, _) = setup_test();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);
    let metadata = Bytes::new(&env);

    env.ledger().set_timestamp(1000);
    
    // Log mixed types
    client.log_record(&employer, &employee, &token, &100, &ReportType::Tax, &metadata);
    client.log_record(&employer, &employee, &token, &500, &ReportType::Payroll, &metadata);
    client.log_record(&employer, &employee, &token, &200, &ReportType::Tax, &metadata);

    // Query ONLY Tax
    let report = client.generate_report(&employer, &0, &2000, &Some(ReportType::Tax), &50);
    
    assert_eq!(report.record_count, 2);
    assert_eq!(report.total_amount, 300); // 100 + 200
    
    for record in report.records.into_iter() {
        assert_eq!(record.report_type, ReportType::Tax);
    }
}

#[test]
fn test_edge_cases_and_errors() {
    let (env, client, _) = setup_test();
    let employer = Address::generate(&env);
    
    // Invalid Date Range
    let err = client.try_generate_report(&employer, &2000, &1000, &None, &50)
        .unwrap_err().unwrap();
    assert_eq!(err, ComplianceError::InvalidDateRange);
    
    // Limit Exceeded
    let err = client.try_generate_report(&employer, &0, &2000, &None, &150)
        .unwrap_err().unwrap();
    assert_eq!(err, ComplianceError::QueryLimitExceeded);
}
