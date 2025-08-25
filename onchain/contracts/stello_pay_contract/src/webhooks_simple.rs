use soroban_sdk::{
    contracterror, contracttype,
    Address, BytesN, Env, String, Vec
};

//-----------------------------------------------------------------------------
// Simple Webhook System for StelloPay Core
//-----------------------------------------------------------------------------

/// Webhook event types
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventType {
    SalaryDisbursed,
    PayrollCreated,
    PayrollUpdated,
    All,
}

/// Webhook configuration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Webhook {
    pub id: u64,
    pub owner: Address,
    pub url: String,
    pub events: Vec<EventType>,
    pub secret_hash: BytesN<32>,
    pub is_active: bool,
    pub failure_count: u32,
    pub created_at: u64,
}

/// Simple webhook registration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct WebhookRegistration {
    pub url: String,
    pub events: Vec<EventType>,
    pub secret: String,
}

/// Webhook errors
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum WebhookError {
    WebhookNotFound = 100,
    InvalidUrl = 101,
    MaxWebhooksReached = 102,
    Unauthorized = 103,
}

/// Simple webhook system
pub struct WebhookSystem;

impl WebhookSystem {
    /// Register a webhook (simplified)
    pub fn register_webhook(
        env: &Env,
        owner: Address,
        url: String,
        events: Vec<EventType>,
        _secret: String,
    ) -> Result<u64, WebhookError> {
        owner.require_auth();

        if url.len() > 255 {
            return Err(WebhookError::InvalidUrl);
        }

        let webhook_id = Self::get_next_webhook_id(env);
        // For simplicity, use a fixed hash for now (in production, properly hash the secret)
        let secret_hash: BytesN<32> = BytesN::from_array(env, &[0u8; 32]);

        let webhook = Webhook {
            id: webhook_id,
            owner: owner.clone(),
            url,
            events,
            secret_hash: secret_hash,
            is_active: true,
            failure_count: 0,
            created_at: env.ledger().timestamp(),
        };

        env.storage().persistent().set(&crate::storage::DataKey::Webhook(webhook_id), &webhook);

        Ok(webhook_id)
    }

    /// Get webhook by ID
    pub fn get_webhook(env: &Env, webhook_id: u64) -> Result<Webhook, WebhookError> {
        env.storage()
            .persistent()
            .get(&crate::storage::DataKey::Webhook(webhook_id))
            .ok_or(WebhookError::WebhookNotFound)
    }

    /// Delete webhook
    pub fn delete_webhook(
        env: &Env,
        owner: Address,
        webhook_id: u64,
    ) -> Result<(), WebhookError> {
        owner.require_auth();

        let webhook = Self::get_webhook(env, webhook_id)?;

        if webhook.owner != owner {
            return Err(WebhookError::Unauthorized);
        }

        env.storage().persistent().remove(&crate::storage::DataKey::Webhook(webhook_id));

        Ok(())
    }

    fn get_next_webhook_id(env: &Env) -> u64 {
        let id = env.storage()
            .persistent()
            .get(&crate::storage::DataKey::NextWebhookId)
            .unwrap_or(1u64);
        
        env.storage()
            .persistent()
            .set(&crate::storage::DataKey::NextWebhookId, &(id + 1));
        
        id
    }
}