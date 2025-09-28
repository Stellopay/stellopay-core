use soroban_sdk::{contract, contractimpl, Address, Env, Map, String, Vec};

use crate::webhooks::{WebhookEventType, WebhookRegistration, WebhookSystem, WebhookUpdate};

#[contract]
pub struct WebhookContract;

#[contractimpl]
impl WebhookContract {
    /// Register a webhook
    pub fn register_webhook(
        env: &Env,
        caller: Address,
        registration: WebhookRegistration,
    ) -> Result<u64, crate::webhooks::WebhookError> {
        // No authorization check - this should be called from the main contract
        WebhookSystem::register_webhook(env, caller, registration)
    }

    /// Update a webhook
    pub fn update_webhook(
        env: &Env,
        caller: Address,
        webhook_id: u64,
        update: WebhookUpdate,
    ) -> Result<(), crate::webhooks::WebhookError> {
        // No authorization check - this should be called from the main contract
        WebhookSystem::update_webhook(env, caller, webhook_id, update)
    }

    /// Get a webhook
    pub fn get_webhook(
        env: &Env,
        webhook_id: u64,
    ) -> Result<crate::webhooks::Webhook, crate::webhooks::WebhookError> {
        WebhookSystem::get_webhook(env, webhook_id)
    }

    /// Delete a webhook
    pub fn delete_webhook(
        env: &Env,
        caller: Address,
        webhook_id: u64,
    ) -> Result<(), crate::webhooks::WebhookError> {
        // No authorization check - this should be called from the main contract
        WebhookSystem::delete_webhook(env, caller, webhook_id)
    }

    /// List owner webhooks
    pub fn list_owner_webhooks(env: &Env, owner: Address) -> Vec<u64> {
        WebhookSystem::list_owner_webhooks(env, owner)
    }

    /// Trigger webhook event
    pub fn trigger_webhook_event(
        env: &Env,
        event_type: WebhookEventType,
        event_data: Map<String, String>,
        metadata: Map<String, String>,
    ) -> Result<Vec<crate::webhooks::DeliveryResult>, crate::webhooks::WebhookError> {
        WebhookSystem::trigger_webhook_event(env, event_type, event_data, metadata)
    }

    /// Get webhook statistics
    pub fn get_webhook_stats(env: &Env) -> crate::webhooks::WebhookStats {
        WebhookSystem::get_webhook_stats(env)
    }
}
