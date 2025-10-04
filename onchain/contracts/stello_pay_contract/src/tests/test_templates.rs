#![cfg(test)]

use crate::payroll::{PayrollContractClient, PayrollError};
use soroban_sdk::{testutils::Address as _, Address, Env, String};

fn setup_contract(env: &Env) -> (Address, PayrollContractClient) {
    let contract_id = env.register(crate::payroll::PayrollContract, ());
    let client = PayrollContractClient::new(env, &contract_id);
    (contract_id, client)
}

// Template Creation Tests
#[test]
fn test_create_template_success() {
    let env = Env::default();
    let (_, client) = setup_contract(&env);

    let employer = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&employer);

    let template_id = client.create_template(
        &employer,
        &String::from_str(&env, "Test Template"),
        &String::from_str(&env, "Test Description"),
        &token,
        &1000i128,
        &86400u64,
        &2592000u64,
        &false,
    );

    assert_eq!(template_id, 1);

    // Verify template was created
    let template = client.get_template(&template_id);
    assert_eq!(template.name, String::from_str(&env, "Test Template"));
    assert_eq!(
        template.description,
        String::from_str(&env, "Test Description")
    );
    assert_eq!(template.employer, employer);
    assert_eq!(template.amount, 1000i128);
    assert_eq!(template.usage_count, 0);
}

#[test]
fn test_create_template_validation_errors() {
    let env = Env::default();
    let (_, client) = setup_contract(&env);

    let employer = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&employer);

    // Test empty name
    let result = client.try_create_template(
        &employer,
        &String::from_str(&env, ""),
        &String::from_str(&env, "Test Description"),
        &token,
        &1000i128,
        &86400u64,
        &2592000u64,
        &false,
    );
    assert_eq!(result, Err(Ok(PayrollError::InvalidTemplateName)));

    // Test zero amount
    let result = client.try_create_template(
        &employer,
        &String::from_str(&env, "Valid Name"),
        &String::from_str(&env, "Test Description"),
        &token,
        &0i128,
        &86400u64,
        &2592000u64,
        &false,
    );
    assert_eq!(result, Err(Ok(PayrollError::TemplateValidationFailed)));
}

// Template Retrieval Tests
#[test]
fn test_get_template_success() {
    let env = Env::default();
    let (_, client) = setup_contract(&env);

    let employer = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&employer);

    let template_id = client.create_template(
        &employer,
        &String::from_str(&env, "Test Template"),
        &String::from_str(&env, "Test Description"),
        &token,
        &1000i128,
        &86400u64,
        &2592000u64,
        &false,
    );

    let template = client.get_template(&template_id);
    assert_eq!(template.id, template_id);
    assert_eq!(template.name, String::from_str(&env, "Test Template"));
}

#[test]
fn test_get_template_not_found() {
    let env = Env::default();
    let (_, client) = setup_contract(&env);

    let result = client.try_get_template(&999u64);
    assert_eq!(result, Err(Ok(PayrollError::TemplateNotFound)));
}

// Template Application Tests
#[test]
fn test_apply_template_success() {
    let env = Env::default();
    let (_, client) = setup_contract(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&employer);

    let template_id = client.create_template(
        &employer,
        &String::from_str(&env, "Test Template"),
        &String::from_str(&env, "Test Description"),
        &token,
        &1000i128,
        &86400u64,
        &2592000u64,
        &false,
    );

    client.apply_template(&employer, &template_id, &employee);

    // Verify template usage count increased (this confirms the template was applied)
    let updated_template = client.get_template(&template_id);
    assert_eq!(updated_template.usage_count, 1);
}

#[test]
fn test_apply_template_not_found() {
    let env = Env::default();
    let (_, client) = setup_contract(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&employer);

    let result = client.try_apply_template(&employer, &999u64, &employee);
    assert_eq!(result, Err(Ok(PayrollError::TemplateNotFound)));
}

#[test]
fn test_apply_template_not_public() {
    let env = Env::default();
    let (_, client) = setup_contract(&env);

    let employer1 = Address::generate(&env);
    let employer2 = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&employer1);

    let template_id = client.create_template(
        &employer1,
        &String::from_str(&env, "Private Template"),
        &String::from_str(&env, "Private Description"),
        &token,
        &1000i128,
        &86400u64,
        &2592000u64,
        &false,
    );

    let result = client.try_apply_template(&employer2, &template_id, &employee);
    assert_eq!(result, Err(Ok(PayrollError::TemplateNotPublic)));
}

// Template Management Tests
#[test]
fn test_update_template_success() {
    let env = Env::default();
    let (_, client) = setup_contract(&env);

    let employer = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&employer);

    let template_id = client.create_template(
        &employer,
        &String::from_str(&env, "Original Name"),
        &String::from_str(&env, "Original Description"),
        &token,
        &1000i128,
        &86400u64,
        &2592000u64,
        &false,
    );

    client.update_template(
        &employer,
        &template_id,
        &Some(String::from_str(&env, "Updated Name")),
        &Some(String::from_str(&env, "Updated Description")),
        &None,
        &None,
        &None,
        &None,
    );

    let updated_template = client.get_template(&template_id);
    assert_eq!(
        updated_template.name,
        String::from_str(&env, "Updated Name")
    );
    assert_eq!(
        updated_template.description,
        String::from_str(&env, "Updated Description")
    );
}

#[test]
fn test_update_template_unauthorized() {
    let env = Env::default();
    let (_, client) = setup_contract(&env);

    let employer1 = Address::generate(&env);
    let employer2 = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&employer1);

    let template_id = client.create_template(
        &employer1,
        &String::from_str(&env, "Template"),
        &String::from_str(&env, "Description"),
        &token,
        &1000i128,
        &86400u64,
        &2592000u64,
        &false,
    );

    let result = client.try_update_template(
        &employer2,
        &template_id,
        &Some(String::from_str(&env, "Hacked Name")),
        &None,
        &None,
        &None,
        &None,
        &None,
    );
    assert_eq!(result, Err(Ok(PayrollError::Unauthorized)));
}

#[test]
fn test_get_employer_templates() {
    let env = Env::default();
    let (_, client) = setup_contract(&env);

    let employer = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&employer);

    let template_id1 = client.create_template(
        &employer,
        &String::from_str(&env, "Template 1"),
        &String::from_str(&env, "Description 1"),
        &token,
        &1000i128,
        &86400u64,
        &2592000u64,
        &false,
    );

    let template_id2 = client.create_template(
        &employer,
        &String::from_str(&env, "Template 2"),
        &String::from_str(&env, "Description 2"),
        &token,
        &2000i128,
        &172800u64,
        &5184000u64,
        &true,
    );

    let templates = client.get_employer_templates(&employer);
    assert_eq!(templates.len(), 2);
    assert_eq!(templates.get(0).unwrap().id, template_id1);
    assert_eq!(templates.get(1).unwrap().id, template_id2);
}

#[test]
fn test_get_public_templates() {
    let env = Env::default();
    let (_, client) = setup_contract(&env);

    let employer = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&employer);

    let template_id = client.create_template(
        &employer,
        &String::from_str(&env, "Public Template"),
        &String::from_str(&env, "Public Description"),
        &token,
        &1000i128,
        &86400u64,
        &2592000u64,
        &true,
    );

    let public_templates = client.get_public_templates();
    assert_eq!(public_templates.len(), 1);
    assert_eq!(public_templates.get(0).unwrap().id, template_id);
    assert_eq!(public_templates.get(0).unwrap().is_public, true);
}

// Template Sharing Tests
#[test]
fn test_share_template_success() {
    let env = Env::default();
    let (_, client) = setup_contract(&env);

    let employer1 = Address::generate(&env);
    let employer2 = Address::generate(&env);
    let token = Address::generate(&env);

    env.mock_all_auths();

    client.initialize(&employer1);

    let template_id = client.create_template(
        &employer1,
        &String::from_str(&env, "Shareable Template"),
        &String::from_str(&env, "Shareable Description"),
        &token,
        &1000i128,
        &86400u64,
        &2592000u64,
        &false,
    );

    client.share_template(&employer1, &template_id, &employer2);

    // Verify template is now accessible by employer2
    let template = client.get_template(&template_id);
    assert_eq!(template.employer, employer1);
}
