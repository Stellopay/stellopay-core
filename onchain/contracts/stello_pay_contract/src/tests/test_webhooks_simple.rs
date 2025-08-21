#[cfg(test)]
mod test_webhooks_simple {
    use crate::webhooks_simple::{WebhookSystem, EventType, WebhookError};
    use soroban_sdk::{
        testutils::Address as _,
        vec, Address, Env, String,
    };

    #[test]
    fn test_register_simple_webhook() {
        let env = Env::default();
        env.mock_all_auths();
        
        let integration_owner = Address::generate(&env);
        
        // Register webhook directly through the WebhookSystem
        let result = WebhookSystem::register_webhook(
            &env,
            integration_owner.clone(),
            String::from_str(&env, "https://example.com/webhook"),
            vec![&env, EventType::SalaryDisbursed],
            String::from_str(&env, "secret123"),
        );
        
        assert!(result.is_ok());
        let webhook_id = result.unwrap();
        assert_eq!(webhook_id, 1);
        
        // Verify webhook was created
        let webhook_result = WebhookSystem::get_webhook(&env, webhook_id);
        assert!(webhook_result.is_ok());
        let webhook = webhook_result.unwrap();
        assert_eq!(webhook.owner, integration_owner);
        assert_eq!(webhook.is_active, true);
        assert_eq!(webhook.failure_count, 0);
    }

    #[test]
    fn test_delete_webhook() {
        let env = Env::default();
        env.mock_all_auths();
        
        let integration_owner = Address::generate(&env);
        
        // Register webhook
        let webhook_id = WebhookSystem::register_webhook(
            &env,
            integration_owner.clone(),
            String::from_str(&env, "https://example.com/webhook"),
            vec![&env, EventType::SalaryDisbursed],
            String::from_str(&env, "secret123"),
        ).unwrap();
        
        // Delete webhook
        let result = WebhookSystem::delete_webhook(&env, integration_owner, webhook_id);
        assert!(result.is_ok());
        
        // Try to get deleted webhook (should fail)
        let get_result = WebhookSystem::get_webhook(&env, webhook_id);
        assert!(get_result.is_err());
    }

    #[test]
    fn test_unauthorized_delete() {
        let env = Env::default();
        env.mock_all_auths();
        
        let integration_owner = Address::generate(&env);
        let unauthorized_user = Address::generate(&env);
        
        // Register webhook
        let webhook_id = WebhookSystem::register_webhook(
            &env,
            integration_owner.clone(),
            String::from_str(&env, "https://example.com/webhook"),
            vec![&env, EventType::SalaryDisbursed],
            String::from_str(&env, "secret123"),
        ).unwrap();
        
        // Try to delete with unauthorized user (should fail)
        let result = WebhookSystem::delete_webhook(&env, unauthorized_user, webhook_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_url() {
        let env = Env::default();
        env.mock_all_auths();
        
        let integration_owner = Address::generate(&env);
        
        // Try to register webhook with very long URL (over 255 chars)
        let long_url = "https://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let result = WebhookSystem::register_webhook(
            &env,
            integration_owner,
            String::from_str(&env, long_url),
            vec![&env, EventType::All],
            String::from_str(&env, "secret"),
        );
        
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), WebhookError::InvalidUrl);
    }
}