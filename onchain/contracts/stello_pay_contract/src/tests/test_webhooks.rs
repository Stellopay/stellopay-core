#[cfg(test)]
mod test_webhooks {
    use crate::payroll::{PayrollContract, PayrollContractClient};
    use crate::webhooks::{
        WebhookEventType, WebhookRegistration, WebhookUpdate
    };
    use soroban_sdk::{
        testutils::Address as _,
        vec, Address, Env, String, Map
    };

    fn create_test_webhook_registration(env: &Env) -> WebhookRegistration {
        WebhookRegistration {
            name: String::from_str(env, "Test Webhook"),
            description: String::from_str(env, "Test webhook for integration testing"),
            url: String::from_str(env, "https://example.com/webhook"),
            events: vec![env, WebhookEventType::SalaryDisbursed, WebhookEventType::PayrollCreated],
            secret: String::from_str(env, "test_secret_123"),
        }
    }

    #[test]
    fn test_register_comprehensive_webhook() {
        let env = Env::default();
        let contract_id = env.register(PayrollContract, ());
        let client = PayrollContractClient::new(&env, &contract_id);
        
        env.mock_all_auths();
        
        let webhook_owner = Address::generate(&env);
        
        // Initialize the contract first
        client.initialize(&webhook_owner);
        
        // Create webhook registration
        let registration = create_test_webhook_registration(&env);
        
        // Register webhook
        let webhook_id = client.register_webhook(&webhook_owner, &registration);
        
        assert_eq!(webhook_id, 1);
        
        // Verify webhook was created
        let webhook = client.get_webhook(&webhook_id);
        assert_eq!(webhook.owner, webhook_owner);
        assert_eq!(webhook.name, String::from_str(&env, "Test Webhook"));
        assert_eq!(webhook.is_active, true);
        assert_eq!(webhook.failure_count, 0);
        assert_eq!(webhook.success_count, 0);
        assert_eq!(webhook.events.len(), 2);
    }

    #[test]
    fn test_update_webhook() {
        let env = Env::default();
        let contract_id = env.register(PayrollContract, ());
        let client = PayrollContractClient::new(&env, &contract_id);
        
        env.mock_all_auths();
        
        let webhook_owner = Address::generate(&env);
        
        // Initialize the contract first
        client.initialize(&webhook_owner);
        
        // Create and register webhook
        let registration = create_test_webhook_registration(&env);
        let webhook_id = client.register_webhook(&webhook_owner, &registration);
        
        // Update webhook
        let update = WebhookUpdate {
            name: Some(String::from_str(&env, "Updated Webhook Name")),
            description: Some(String::from_str(&env, "Updated description")),
            url: Some(String::from_str(&env, "https://updated.example.com/webhook")),
            events: Some(vec![&env, WebhookEventType::All]),
            is_active: Some(false),
        };
        
        client.update_webhook(&webhook_owner, &webhook_id, &update);
        
        // Verify webhook was updated
        let webhook = client.get_webhook(&webhook_id);
        assert_eq!(webhook.name, String::from_str(&env, "Updated Webhook Name"));
        assert_eq!(webhook.description, String::from_str(&env, "Updated description"));
        assert_eq!(webhook.url, String::from_str(&env, "https://updated.example.com/webhook"));
        assert_eq!(webhook.is_active, false);
        assert_eq!(webhook.events.len(), 1);
        assert_eq!(webhook.events.get(0), Some(WebhookEventType::All));
    }

    #[test]
    fn test_delete_webhook() {
        let env = Env::default();
        let contract_id = env.register(PayrollContract, ());
        let client = PayrollContractClient::new(&env, &contract_id);
        
        env.mock_all_auths();
        
        let webhook_owner = Address::generate(&env);
        
        // Initialize the contract first
        client.initialize(&webhook_owner);
        
        // Create and register webhook
        let registration = create_test_webhook_registration(&env);
        let webhook_id = client.register_webhook(&webhook_owner, &registration);
        
        // Delete webhook
        client.delete_webhook(&webhook_owner, &webhook_id);
        
        // Verify webhook was deleted by checking owner's webhook list
        let owner_webhooks = client.list_owner_webhooks(&webhook_owner);
        assert_eq!(owner_webhooks.len(), 0);
    }

    #[test]
    fn test_list_owner_webhooks() {
        let env = Env::default();
        let contract_id = env.register(PayrollContract, ());
        let client = PayrollContractClient::new(&env, &contract_id);
        
        env.mock_all_auths();
        
        let webhook_owner = Address::generate(&env);
        
        // Initialize the contract first
        client.initialize(&webhook_owner);
        
        // Register multiple webhooks
        let mut registration1 = create_test_webhook_registration(&env);
        registration1.name = String::from_str(&env, "Webhook 1");
        registration1.url = String::from_str(&env, "https://webhook1.example.com");
        
        let mut registration2 = create_test_webhook_registration(&env);
        registration2.name = String::from_str(&env, "Webhook 2");
        registration2.url = String::from_str(&env, "https://webhook2.example.com");
        
        let webhook_id1 = client.register_webhook(&webhook_owner, &registration1);
        let webhook_id2 = client.register_webhook(&webhook_owner, &registration2);
        
        // List owner's webhooks
        let owner_webhooks = client.list_owner_webhooks(&webhook_owner);
        assert_eq!(owner_webhooks.len(), 2);
        assert!(owner_webhooks.contains(&webhook_id1));
        assert!(owner_webhooks.contains(&webhook_id2));
    }

    #[test]
    fn test_get_webhook_stats() {
        let env = Env::default();
        let contract_id = env.register(PayrollContract, ());
        let client = PayrollContractClient::new(&env, &contract_id);
        
        env.mock_all_auths();
        
        let webhook_owner = Address::generate(&env);
        
        // Initialize the contract first
        client.initialize(&webhook_owner);
        
        // Register a webhook
        let registration = create_test_webhook_registration(&env);
        let _webhook_id = client.register_webhook(&webhook_owner, &registration);
        
        // Get webhook statistics
        let stats = client.get_webhook_stats();
        assert_eq!(stats.total_webhooks, 1);
        assert_eq!(stats.active_webhooks, 1);
        assert_eq!(stats.total_deliveries, 0);
        assert_eq!(stats.successful_deliveries, 0);
        assert_eq!(stats.failed_deliveries, 0);
    }

    #[test]
    fn test_webhook_triggering_integration() {
        let env = Env::default();
        let contract_id = env.register(PayrollContract, ());
        let client = PayrollContractClient::new(&env, &contract_id);
        
        env.mock_all_auths();
        
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token = Address::generate(&env);
        let webhook_owner = Address::generate(&env);
        
        // Initialize the contract first
        client.initialize(&employer);
        
        // Register webhook for salary disbursement events
        let mut registration = create_test_webhook_registration(&env);
        registration.events = vec![&env, WebhookEventType::SalaryDisbursed];
        let webhook_id = client.register_webhook(&webhook_owner, &registration);
        
        // Create payroll
        let amount = 1000i128;
        let interval = 86400u64;
        let recurrence_frequency = 2592000u64;
        
        client.create_or_update_escrow(
            &employer,
            &employee,
            &token,
            &amount,
            &interval,
            &recurrence_frequency,
        );
        
        // Deposit tokens
        client.deposit_tokens(&employer, &token, &10000);
        
        // Disburse salary (this should trigger webhook)
        client.disburse_salary(&employer, &employee);
        
        // Verify webhook was triggered by checking stats
        let stats = client.get_webhook_stats();
        assert!(stats.total_deliveries > 0);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #103)")]
    fn test_unauthorized_webhook_update() {
        let env = Env::default();
        let contract_id = env.register(PayrollContract, ());
        let client = PayrollContractClient::new(&env, &contract_id);
        
        env.mock_all_auths();
        
        let webhook_owner = Address::generate(&env);
        let unauthorized_user = Address::generate(&env);
        
        // Initialize the contract first
        client.initialize(&webhook_owner);
        
        // Create and register webhook
        let registration = create_test_webhook_registration(&env);
        let webhook_id = client.register_webhook(&webhook_owner, &registration);
        
        // Try to update webhook with unauthorized user
        let update = WebhookUpdate {
            name: Some(String::from_str(&env, "Unauthorized Update")),
            description: None,
            url: None,
            events: None,
            is_active: None,
        };
        
        client.update_webhook(&unauthorized_user, &webhook_id, &update);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #101)")]
    fn test_invalid_webhook_url() {
        let env = Env::default();
        let contract_id = env.register(PayrollContract, ());
        let client = PayrollContractClient::new(&env, &contract_id);
        
        env.mock_all_auths();
        
        let webhook_owner = Address::generate(&env);
        
        // Initialize the contract first
        client.initialize(&webhook_owner);
        
        // Try to register webhook with invalid URL
        let mut registration = create_test_webhook_registration(&env);
        registration.url = String::from_str(&env, "invalid-url");
        
        client.register_webhook(&webhook_owner, &registration);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #102)")]
    fn test_max_webhooks_reached() {
        let env = Env::default();
        let contract_id = env.register(PayrollContract, ());
        let client = PayrollContractClient::new(&env, &contract_id);
        
        env.mock_all_auths();
        
        let webhook_owner = Address::generate(&env);
        
        // Initialize the contract first
        client.initialize(&webhook_owner);
        
        // Register maximum number of webhooks (50)
        for i in 0..50 {
            let mut registration = create_test_webhook_registration(&env);
            registration.name = String::from_str(&env, "Webhook Test");
            registration.url = String::from_str(&env, "https://webhook.example.com");
            
            let _webhook_id = client.register_webhook(&webhook_owner, &registration);
        }
        
        // Try to register one more webhook (should fail)
        let registration = create_test_webhook_registration(&env);
        client.register_webhook(&webhook_owner, &registration);
    }

    #[test]
    fn test_webhook_event_types() {
        let env = Env::default();
        let contract_id = env.register(PayrollContract, ());
        let client = PayrollContractClient::new(&env, &contract_id);
        
        env.mock_all_auths();
        
        let webhook_owner = Address::generate(&env);
        
        // Initialize the contract first
        client.initialize(&webhook_owner);
        
        // Test different event types
        let events = vec![
            &env,
            WebhookEventType::SalaryDisbursed,
            WebhookEventType::PayrollCreated,
            WebhookEventType::PayrollUpdated,
            WebhookEventType::TokensDeposited,
            WebhookEventType::ContractPaused,
            WebhookEventType::ContractUnpaused,
            WebhookEventType::All,
        ];
        
        let mut registration = create_test_webhook_registration(&env);
        registration.events = events;
        
        let webhook_id = client.register_webhook(&webhook_owner, &registration);
        
        // Verify webhook was created with all event types
        let webhook = client.get_webhook(&webhook_id);
        assert_eq!(webhook.events.len(), 8);
    }

    #[test]
    fn test_webhook_retry_configuration() {
        let env = Env::default();
        let contract_id = env.register(PayrollContract, ());
        let client = PayrollContractClient::new(&env, &contract_id);
        
        env.mock_all_auths();
        
        let webhook_owner = Address::generate(&env);
        
        // Initialize the contract first
        client.initialize(&webhook_owner);
        
        // Create webhook with basic configuration
        let mut registration = create_test_webhook_registration(&env);
        registration.name = String::from_str(&env, "Retry Test Webhook");
        
        let webhook_id = client.register_webhook(&webhook_owner, &registration);
        
        // Verify webhook was created
        let webhook = client.get_webhook(&webhook_id);
        assert_eq!(webhook.name, String::from_str(&env, "Retry Test Webhook"));
    }

    #[test]
    fn test_webhook_security_configuration() {
        let env = Env::default();
        let contract_id = env.register(PayrollContract, ());
        let client = PayrollContractClient::new(&env, &contract_id);
        
        env.mock_all_auths();
        
        let webhook_owner = Address::generate(&env);
        
        // Initialize the contract first
        client.initialize(&webhook_owner);
        
        // Create webhook with basic configuration
        let mut registration = create_test_webhook_registration(&env);
        registration.name = String::from_str(&env, "Security Test Webhook");
        
        let webhook_id = client.register_webhook(&webhook_owner, &registration);
        
        // Verify webhook was created
        let webhook = client.get_webhook(&webhook_id);
        assert_eq!(webhook.name, String::from_str(&env, "Security Test Webhook"));
    }
}
