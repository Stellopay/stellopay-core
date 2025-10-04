#![cfg(test)]

use crate::payroll::{PayrollContract, PayrollContractClient};
use crate::storage::{
    ActionType, ConditionOperator, LogicalOperator, RuleAction, RuleCondition, 
    ScheduleFrequency, ScheduleType, WeekendHandling,
};
use soroban_sdk::token::{StellarAssetClient as TokenAdmin, TokenClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    vec, Address, Env, String,
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
fn test_create_flexible_schedule_success() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let name = String::from_str(&env, "Monthly Payroll");
    let description = String::from_str(&env, "Regular monthly payments");
    
    env.mock_all_auths();
    client.initialize(&employer);

    let start_date = env.ledger().timestamp() + 86400;
    let holidays = vec![&env];
    
    let schedule_id = client.create_flexible_schedule(
        &employer,
        &name,
        &description,
        &ScheduleType::Recurring,
        &ScheduleFrequency::Monthly,
        &start_date,
        &None,
        &true,
        &holidays,
        &WeekendHandling::Skip,
    );

    assert!(schedule_id > 0);

    let schedule = client.get_schedule(&schedule_id);
    assert_eq!(schedule.name, name);
}

#[test]
fn test_holiday_config_management() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    
    env.mock_all_auths();
    client.initialize(&employer);

    let start_date = env.ledger().timestamp() + 86400;
    let holidays = vec![&env, start_date + 172800];
    
    let schedule_id = client.create_flexible_schedule(
        &employer,
        &String::from_str(&env, "Test"),
        &String::from_str(&env, "Description"),
        &ScheduleType::Recurring,
        &ScheduleFrequency::Weekly,
        &start_date,
        &None,
        &true,
        &holidays,
        &WeekendHandling::ProcessEarly,
    );

    let config = client.get_holiday_config(&schedule_id);
    assert_eq!(config.skip_weekends, true);
    assert_eq!(config.holidays.len(), 1);
}

#[test]
fn test_update_holiday_config() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    
    env.mock_all_auths();
    client.initialize(&employer);

    let start_date = env.ledger().timestamp() + 86400;
    
    let schedule_id = client.create_flexible_schedule(
        &employer,
        &String::from_str(&env, "Test"),
        &String::from_str(&env, "Description"),
        &ScheduleType::Recurring,
        &ScheduleFrequency::Monthly,
        &start_date,
        &None,
        &false,
        &vec![&env],
        &WeekendHandling::Skip,
    );

    let new_holidays = vec![&env, start_date + 86400, start_date + 172800];
    
    client.update_holiday_config(
        &employer,
        &schedule_id,
        &true,
        &new_holidays,
        &WeekendHandling::ProcessLate,
    );

    let config = client.get_holiday_config(&schedule_id);
    assert_eq!(config.skip_weekends, true);
    assert_eq!(config.holidays.len(), 2);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_update_holiday_config_unauthorized() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    
    env.mock_all_auths();
    client.initialize(&employer);

    let start_date = env.ledger().timestamp() + 86400;
    
    let schedule_id = client.create_flexible_schedule(
        &employer,
        &String::from_str(&env, "Test"),
        &String::from_str(&env, "Description"),
        &ScheduleType::Recurring,
        &ScheduleFrequency::Monthly,
        &start_date,
        &None,
        &false,
        &vec![&env],
        &WeekendHandling::Skip,
    );

    client.update_holiday_config(
        &unauthorized,
        &schedule_id,
        &true,
        &vec![&env],
        &WeekendHandling::ProcessLate,
    );
}

#[test]
fn test_create_conditional_trigger() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    
    env.mock_all_auths();
    client.initialize(&employer);

    let condition = RuleCondition {
        field: String::from_str(&env, "amount"),
        operator: ConditionOperator::GreaterThan,
        value: String::from_str(&env, "1000"),
        logical_operator: LogicalOperator::And,
    };
    
    let action = RuleAction {
        action_type: ActionType::DisburseSalary,
        parameters: vec![&env],
        delay_seconds: 0,
        retry_count: 3,
    };

    let conditions = vec![&env, condition];
    let actions = vec![&env, action];
    
    let rule_id = client.create_conditional_trigger(
        &employer,
        &String::from_str(&env, "Performance Bonus"),
        &String::from_str(&env, "Bonus for high performers"),
        &String::from_str(&env, "performance"),
        &conditions,
        &actions,
        &5000i128,
    );

    assert!(rule_id > 0);

    let rule = client.get_automation_rule(&rule_id);
    assert_eq!(rule.name, String::from_str(&env, "Performance Bonus"));
}

#[test]
#[should_panic(expected = "Error(Contract, #34)")]
fn test_create_trigger_empty_conditions() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    
    env.mock_all_auths();
    client.initialize(&employer);

    let conditions = vec![&env]; // Empty
    let actions = vec![&env];
    
    client.create_conditional_trigger(
        &employer,
        &String::from_str(&env, "Test"),
        &String::from_str(&env, "Description"),
        &String::from_str(&env, "test"),
        &conditions,
        &actions,
        &1000i128,
    );
}

#[test]
fn test_apply_automated_adjustment() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, _) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &5000i128,
        &86400u64,
        &2592000u64,
    );

    let adjustment_id = client.apply_automated_adjustment(
        &employer,
        &employee,
        &String::from_str(&env, "bonus"),
        &1000i128,
        &String::from_str(&env, "Performance bonus"),
    );

    assert!(adjustment_id > 0);

    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.amount, 6000i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_adjustment_unauthorized() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, _) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let unauthorized = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &5000i128,
        &86400u64,
        &2592000u64,
    );

    client.apply_automated_adjustment(
        &unauthorized,
        &employee,
        &String::from_str(&env, "bonus"),
        &1000i128,
        &String::from_str(&env, "Unauthorized bonus"),
    );
}

#[test]
fn test_get_employee_adjustments() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, _) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &5000i128,
        &86400u64,
        &2592000u64,
    );

    client.apply_automated_adjustment(
        &employer,
        &employee,
        &String::from_str(&env, "bonus"),
        &1000i128,
        &String::from_str(&env, "Q1 Bonus"),
    );

    client.apply_automated_adjustment(
        &employer,
        &employee,
        &String::from_str(&env, "overtime"),
        &500i128,
        &String::from_str(&env, "Extra hours"),
    );

    let adjustments = client.get_employee_adjustments(&employee);
    assert_eq!(adjustments.len(), 2);
    assert_eq!(adjustments.get(0).unwrap().amount, 1000i128);
    assert_eq!(adjustments.get(1).unwrap().amount, 500i128);
}

#[test]
fn test_forecast_payroll() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, _) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(
        &employer,
        &employee1,
        &token_address,
        &5000i128,
        &86400u64,
        &2592000u64,
    );

    client.create_or_update_escrow(
        &employer,
        &employee2,
        &token_address,
        &3000i128,
        &86400u64,
        &2592000u64,
    );

    let forecasts = client.forecast_payroll(&employer, &3u32, &30u32);
    assert_eq!(forecasts.len(), 3);
    assert_eq!(forecasts.get(0).unwrap().estimated_amount, 8000i128);
    assert_eq!(forecasts.get(0).unwrap().employee_count, 2);
}

#[test]
fn test_compliance_checks_all_pass() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, _) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &5000i128,
        &86400u64,
        &2592000u64,
    );

    let check = client.run_compliance_checks(&employer);
    assert_eq!(check.passed, true);
    assert_eq!(check.issues_found.len(), 0);
}

#[test]
fn test_compliance_checks_with_issues() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, _) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &500i128, // Below minimum
        &3600u64,
        &2592000u64,
    );

    let check = client.run_compliance_checks(&employer);
    assert_eq!(check.passed, false);
    assert!(check.issues_found.len() >= 2);
}

#[test]
fn test_compliance_check_overdue_payment() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, _) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &5000i128,
        &86400u64,
        &86400u64,
    );

    let next_timestamp = env.ledger().timestamp() + 86400 + 86401;
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

    let check = client.run_compliance_checks(&employer);
    assert_eq!(check.passed, false);
    assert!(check.issues_found.len() > 0);
}

#[test]
fn test_negative_adjustment_reduces_amount() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, _) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &5000i128,
        &86400u64,
        &2592000u64,
    );

    client.apply_automated_adjustment(
        &employer,
        &employee,
        &String::from_str(&env, "penalty"),
        &-500i128,
        &String::from_str(&env, "Late submission"),
    );

    let payroll = client.get_payroll(&employee).unwrap();
    assert_eq!(payroll.amount, 4500i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_adjustment_cannot_go_negative() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, _) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &1000i128,
        &86400u64,
        &2592000u64,
    );

    client.apply_automated_adjustment(
        &employer,
        &employee,
        &String::from_str(&env, "penalty"),
        &-2000i128,
        &String::from_str(&env, "Too much penalty"),
    );
}

#[test]
fn test_forecast_with_no_employees() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    let forecasts = client.forecast_payroll(&employer, &3u32, &30u32);
    assert_eq!(forecasts.len(), 3);
    
    for forecast in forecasts.iter() {
        assert_eq!(forecast.estimated_amount, 0i128);
        assert_eq!(forecast.employee_count, 0);
    }
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_adjustment_nonexistent_employee() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let nonexistent = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    client.apply_automated_adjustment(
        &employer,
        &nonexistent,
        &String::from_str(&env, "bonus"),
        &1000i128,
        &String::from_str(&env, "Bonus"),
    );
}

#[test]
fn test_empty_adjustment_history() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employee = Address::generate(&env);

    env.mock_all_auths();

    let adjustments = client.get_employee_adjustments(&employee);
    assert_eq!(adjustments.len(), 0);
}

#[test]
fn test_schedule_with_end_date() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    
    env.mock_all_auths();
    client.initialize(&employer);

    let start_date = env.ledger().timestamp() + 86400;
    let end_date = start_date + (30 * 86400);
    
    let schedule_id = client.create_flexible_schedule(
        &employer,
        &String::from_str(&env, "Limited Schedule"),
        &String::from_str(&env, "Schedule with end date"),
        &ScheduleType::Recurring,
        &ScheduleFrequency::Weekly,
        &start_date,
        &Some(end_date),
        &false,
        &vec![&env],
        &WeekendHandling::Skip,
    );

    let schedule = client.get_schedule(&schedule_id);
    assert_eq!(schedule.end_date, Some(end_date));
}

#[test]
fn test_get_forecast_by_id() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, _) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &5000i128,
        &86400u64,
        &2592000u64,
    );

    client.forecast_payroll(&employer, &2u32, &30u32);
    
    let forecast = client.get_forecast(&1);
    assert_eq!(forecast.period, 1);
}

#[test]
fn test_get_compliance_check_by_id() {
    let env = Env::default();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    let (token_address, _) = setup_token(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&employer);

    client.create_or_update_escrow(
        &employer,
        &employee,
        &token_address,
        &5000i128,
        &86400u64,
        &2592000u64,
    );

    let result = client.run_compliance_checks(&employer);
    let check_id = result.check_id;
    
    let retrieved = client.get_compliance_check(&check_id);
    assert_eq!(retrieved.check_id, check_id);
}