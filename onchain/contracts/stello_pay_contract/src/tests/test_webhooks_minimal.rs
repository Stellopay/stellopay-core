#[cfg(test)]
mod test_webhooks_minimal {
    use crate::webhooks::{WebhookRegistration, WebhookUpdate, WebhookEventType, WebhookSystem};
    use soroban_sdk::{
        testutils::Address as _,
        vec, Address, Env, String, Map
    };

    fn create_test_env() -> Env {
        let env = Env::default();
        env
    }

    fn create_test_webhook_registration(env: &Env) -> WebhookRegistration {
        WebhookRegistration {
            name: String::from_str(env, "Test Webhook"),
            description: String::from_str(env, "A test webhook for integration"),
            url: String::from_str(env, "https://example.com/webhook"),
            events: vec![env, WebhookEventType::SalaryDisbursed],
            secret: String::from_str(env, "test-secret"),
        }
    }

    #[test]
    fn test_webhook_registration() {
        let env = create_test_env();
        let owner = Address::generate(&env);
        let registration = create_test_webhook_registration(&env);

        // Test webhook registration
        let webhook_id = WebhookSystem::register_webhook(&env, owner.clone(), registration.clone()).unwrap();
        assert!(webhook_id > 0);

        // Test webhook retrieval
        let webhook = WebhookSystem::get_webhook(&env, webhook_id).unwrap();
        assert_eq!(webhook.name, registration.name);
        assert_eq!(webhook.url, registration.url);
        assert_eq!(webhook.events.len(), 1);
        assert_eq!(webhook.events.get(0), Some(WebhookEventType::SalaryDisbursed));
    }

    #[test]
    fn test_webhook_update() {
        let env = create_test_env();
        let owner = Address::generate(&env);
        let registration = create_test_webhook_registration(&env);

        // Register webhook
        let webhook_id = WebhookSystem::register_webhook(&env, owner.clone(), registration.clone()).unwrap();

        // Update webhook
        let update = WebhookUpdate {
            name: Some(String::from_str(&env, "Updated Webhook")),
            description: None,
            url: None,
            events: None,
            is_active: None,
        };

        WebhookSystem::update_webhook(&env, owner.clone(), webhook_id, update).unwrap();

        // Verify update
        let webhook = WebhookSystem::get_webhook(&env, webhook_id).unwrap();
        assert_eq!(webhook.name, String::from_str(&env, "Updated Webhook"));
    }

    #[test]
    fn test_webhook_deletion() {
        let env = create_test_env();
        let owner = Address::generate(&env);
        let registration = create_test_webhook_registration(&env);

        // Register webhook
        let webhook_id = WebhookSystem::register_webhook(&env, owner.clone(), registration.clone()).unwrap();

        // Delete webhook
        WebhookSystem::delete_webhook(&env, owner.clone(), webhook_id).unwrap();

        // Verify deletion
        let result = WebhookSystem::get_webhook(&env, webhook_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_list_owner_webhooks() {
        let env = create_test_env();
        let owner = Address::generate(&env);
        let registration = create_test_webhook_registration(&env);

        // Register multiple webhooks
        let webhook_id1 = WebhookSystem::register_webhook(&env, owner.clone(), registration.clone()).unwrap();
        
        let mut registration2 = registration.clone();
        registration2.name = String::from_str(&env, "Second Webhook");
        let webhook_id2 = WebhookSystem::register_webhook(&env, owner.clone(), registration2).unwrap();

        // List webhooks
        let webhooks = WebhookSystem::list_owner_webhooks(&env, owner.clone());
        assert_eq!(webhooks.len(), 2);
        assert!(webhooks.contains(&webhook_id1));
        assert!(webhooks.contains(&webhook_id2));
    }

    #[test]
    fn test_webhook_event_triggering() {
        let env = create_test_env();
        let owner = Address::generate(&env);
        let registration = create_test_webhook_registration(&env);

        // Register webhook
        let _webhook_id = WebhookSystem::register_webhook(&env, owner.clone(), registration.clone()).unwrap();

        // Create event data
        let mut event_data = Map::new(&env);
        event_data.set(String::from_str(&env, "employer"), owner.to_string());
        event_data.set(String::from_str(&env, "employee"), Address::generate(&env).to_string());
        event_data.set(String::from_str(&env, "amount"), String::from_str(&env, "1000"));

        let mut metadata = Map::new(&env);
        metadata.set(String::from_str(&env, "timestamp"), String::from_str(&env, "1640995200"));

        // Trigger webhook event
        let results = WebhookSystem::trigger_webhook_event(
            &env,
            WebhookEventType::SalaryDisbursed,
            event_data,
            metadata,
        ).unwrap();

        // Verify results
        assert_eq!(results.len(), 1);
        assert!(results.get(0).unwrap().success);
    }

    #[test]
    fn test_webhook_statistics() {
        let env = create_test_env();
        let owner = Address::generate(&env);
        let registration = create_test_webhook_registration(&env);

        // Register webhook
        let _webhook_id = WebhookSystem::register_webhook(&env, owner.clone(), registration.clone()).unwrap();

        // Test basic functionality - statistics functions are not implemented yet
        let webhooks = WebhookSystem::list_owner_webhooks(&env, owner.clone());
        assert_eq!(webhooks.len(), 1);
    }
}
