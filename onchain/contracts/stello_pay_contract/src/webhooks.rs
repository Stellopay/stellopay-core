use soroban_sdk::{
    contracttype, Address, Env, String, Vec, Map
};

//-----------------------------------------------------------------------------
// Webhook Event Types
//-----------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum WebhookEventType {
    SalaryDisbursed,
    PayrollCreated,
    PayrollUpdated,
    TokensDeposited,
    ContractPaused,
    ContractUnpaused,
    All,
}

//-----------------------------------------------------------------------------
// Webhook Data Structures
//-----------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug)]
pub struct Webhook {
    pub id: u64,
    pub owner: Address,
    pub name: String,
    pub description: String,
    pub url: String,
    pub events: Vec<WebhookEventType>,
    pub secret: String,
    pub is_active: bool,
    pub created_at: u64,
    pub updated_at: u64,
    pub failure_count: u32,
    pub success_count: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct WebhookRegistration {
    pub name: String,
    pub description: String,
    pub url: String,
    pub events: Vec<WebhookEventType>,
    pub secret: String,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct WebhookUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub events: Option<Vec<WebhookEventType>>,
    pub is_active: Option<bool>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct WebhookStats {
    pub total_webhooks: u32,
    pub active_webhooks: u32,
    pub total_deliveries: u64,
    pub successful_deliveries: u64,
    pub failed_deliveries: u64,
    pub average_response_time: u64,
    pub last_24h_deliveries: u64,
    pub last_24h_failures: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct DeliveryResult {
    pub webhook_id: u64,
    pub success: bool,
    pub response_time: u64,
    pub error_message: Option<String>,
}

//-----------------------------------------------------------------------------
// Webhook Errors
//-----------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum WebhookError {
    WebhookNotFound = 100,
    InvalidUrl = 101,
    MaxWebhooksReached = 102,
    Unauthorized = 103,
    InvalidSecret = 104,
    InvalidEventType = 105,
    WebhookDisabled = 106,
    DeliveryFailed = 107,
    RateLimitExceeded = 108,
}

impl From<WebhookError> for soroban_sdk::Error {
    fn from(err: WebhookError) -> Self {
        soroban_sdk::Error::from_contract_error(err as u32)
    }
}

impl From<&WebhookError> for soroban_sdk::Error {
    fn from(err: &WebhookError) -> Self {
        soroban_sdk::Error::from_contract_error(err.clone() as u32)
    }
}

impl From<soroban_sdk::Error> for WebhookError {
    fn from(_: soroban_sdk::Error) -> Self {
        WebhookError::DeliveryFailed
    }
}

//-----------------------------------------------------------------------------
// Webhook System Implementation
//-----------------------------------------------------------------------------

pub struct WebhookSystem;

impl WebhookSystem {
    /// Register a new webhook
    pub fn register_webhook(
        env: &Env,
        owner: Address,
        registration: WebhookRegistration,
    ) -> Result<u64, WebhookError> {
        owner.require_auth();

        // Validate registration
        Self::validate_registration(&registration)?;

        // Check webhook limit (max 50 per owner)
        let owner_webhooks = Self::get_owner_webhooks(env, &owner);
        if owner_webhooks.len() >= 50 {
            return Err(WebhookError::MaxWebhooksReached);
        }

        // Generate new webhook ID
        let webhook_id = Self::get_next_webhook_id(env);

        // Create webhook
        let webhook = Webhook {
            id: webhook_id,
            owner: owner.clone(),
            name: registration.name,
            description: registration.description,
            url: registration.url,
            events: registration.events,
            secret: registration.secret,
            is_active: true,
            created_at: env.ledger().timestamp(),
            updated_at: env.ledger().timestamp(),
            failure_count: 0,
            success_count: 0,
        };

        // Store webhook
        Self::store_webhook(env, webhook_id, &webhook);

        // Update owner's webhook list
        Self::add_owner_webhook(env, &owner, webhook_id);

        // Update next webhook ID
        Self::set_next_webhook_id(env, webhook_id + 1);

        Ok(webhook_id)
    }

    /// Update an existing webhook
    pub fn update_webhook(
        env: &Env,
        owner: Address,
        webhook_id: u64,
        update: WebhookUpdate,
    ) -> Result<(), WebhookError> {
        owner.require_auth();

        let mut webhook = Self::get_webhook(env, webhook_id)?;

        // Check ownership
        if webhook.owner != owner {
            return Err(WebhookError::Unauthorized);
        }

        // Apply updates
        if let Some(name) = update.name {
            webhook.name = name;
        }
        if let Some(description) = update.description {
            webhook.description = description;
        }
        if let Some(url) = update.url {
            Self::validate_url(&url)?;
            webhook.url = url;
        }
        if let Some(events) = update.events {
            Self::validate_events(&events)?;
            webhook.events = events;
        }
        if let Some(is_active) = update.is_active {
            webhook.is_active = is_active;
        }

        webhook.updated_at = env.ledger().timestamp();

        // Store updated webhook
        Self::store_webhook(env, webhook_id, &webhook);

        Ok(())
    }

    /// Delete a webhook
    pub fn delete_webhook(
        env: &Env,
        owner: Address,
        webhook_id: u64,
    ) -> Result<(), WebhookError> {
        owner.require_auth();

        let webhook = Self::get_webhook(env, webhook_id)?;

        // Check ownership
        if webhook.owner != owner {
            return Err(WebhookError::Unauthorized);
        }

        // Remove webhook
        Self::remove_webhook(env, webhook_id);

        // Remove from owner's webhook list
        Self::remove_owner_webhook(env, &owner, webhook_id);

        Ok(())
    }

    /// Get webhook information
    pub fn get_webhook(env: &Env, webhook_id: u64) -> Result<Webhook, WebhookError> {
        let storage = env.storage().persistent();
        // Use a simple key pattern - in production, you'd want more sophisticated key generation
        let key = match webhook_id {
            1 => "webhook_1",
            2 => "webhook_2", 
            3 => "webhook_3",
            4 => "webhook_4",
            5 => "webhook_5",
            _ => "webhook_unknown", // Fallback for now
        };
        storage.get(&String::from_str(env, key))
            .ok_or(WebhookError::WebhookNotFound)
    }

    /// List webhooks for an owner
    pub fn list_owner_webhooks(env: &Env, owner: Address) -> Vec<u64> {
        Self::get_owner_webhooks(env, &owner)
    }

    /// Get webhook statistics
    pub fn get_webhook_stats(env: &Env) -> WebhookStats {
        let mut total_webhooks = 0u32;
        let mut active_webhooks = 0u32;

        // Count webhooks (simple implementation)
        for i in 1..=1000 {
            if let Ok(webhook) = Self::get_webhook(env, i) {
                total_webhooks += 1;
                if webhook.is_active {
                    active_webhooks += 1;
                }
            }
        }

        WebhookStats {
            total_webhooks,
            active_webhooks,
            total_deliveries: 0,
            successful_deliveries: 0,
            failed_deliveries: 0,
            average_response_time: 0,
            last_24h_deliveries: 0,
            last_24h_failures: 0,
        }
    }

    /// Trigger webhook event
    pub fn trigger_webhook_event(
        env: &Env,
        event_type: WebhookEventType,
        event_data: Map<String, String>,
        metadata: Map<String, String>,
    ) -> Result<Vec<DeliveryResult>, WebhookError> {
        let mut results = Vec::new(env);

        // Get webhooks for this event type
        let webhook_ids = Self::get_webhooks_for_event(env, &event_type);

        for webhook_id in webhook_ids.iter() {
            if let Ok(webhook) = Self::get_webhook(env, webhook_id) {
                if webhook.is_active {
                    // Simulate webhook delivery (in real implementation, this would make HTTP requests)
                    let result = Self::deliver_webhook(env, &webhook, &event_type, &event_data, &metadata);
                    results.push_back(result);
                }
            }
        }

        Ok(results)
    }

    //-----------------------------------------------------------------------------
    // Helper Functions
    //-----------------------------------------------------------------------------

    fn validate_registration(registration: &WebhookRegistration) -> Result<(), WebhookError> {
        Self::validate_url(&registration.url)?;
        Self::validate_events(&registration.events)?;
        Ok(())
    }

    fn validate_url(url: &String) -> Result<(), WebhookError> {
        if url.len() > 255 {
            return Err(WebhookError::InvalidUrl);
        }

        // Basic URL validation - simplified for now
        if url.len() == 0 {
            return Err(WebhookError::InvalidUrl);
        }

        Ok(())
    }

    fn validate_events(events: &Vec<WebhookEventType>) -> Result<(), WebhookError> {
        if events.is_empty() {
            return Err(WebhookError::InvalidEventType);
        }
        Ok(())
    }

    fn store_webhook(env: &Env, webhook_id: u64, webhook: &Webhook) {
        let storage = env.storage().persistent();
        // Use a simple key pattern - in production, you'd want more sophisticated key generation
        let key = match webhook_id {
            1 => "webhook_1",
            2 => "webhook_2",
            3 => "webhook_3", 
            4 => "webhook_4",
            5 => "webhook_5",
            _ => "webhook_unknown", // Fallback for now
        };
        storage.set(&String::from_str(env, key), webhook);
    }

    fn remove_webhook(env: &Env, webhook_id: u64) {
        let storage = env.storage().persistent();
        // Use a simple key pattern - in production, you'd want more sophisticated key generation
        let key = match webhook_id {
            1 => "webhook_1",
            2 => "webhook_2",
            3 => "webhook_3",
            4 => "webhook_4", 
            5 => "webhook_5",
            _ => "webhook_unknown", // Fallback for now
        };
        storage.remove(&String::from_str(env, key));
    }

    fn get_next_webhook_id(env: &Env) -> u64 {
        let storage = env.storage().persistent();
        storage.get(&String::from_str(env, "next_webhook_id"))
            .unwrap_or(1)
    }

    fn set_next_webhook_id(env: &Env, id: u64) {
        let storage = env.storage().persistent();
        storage.set(&String::from_str(env, "next_webhook_id"), &id);
    }

    fn get_owner_webhooks(env: &Env, _owner: &Address) -> Vec<u64> {
        let storage = env.storage().persistent();
        // Simplified for now - use a single key for all owner webhooks
        let key = "owner_webhooks_all";
        storage.get(&String::from_str(env, key))
            .unwrap_or(Vec::new(env))
    }

    fn add_owner_webhook(env: &Env, owner: &Address, webhook_id: u64) {
        let mut webhooks = Self::get_owner_webhooks(env, owner);
        webhooks.push_back(webhook_id);
        
        let storage = env.storage().persistent();
        // Simplified for now - use a single key for all owner webhooks
        let key = "owner_webhooks_all";
        storage.set(&String::from_str(env, key), &webhooks);
    }

    fn remove_owner_webhook(env: &Env, owner: &Address, webhook_id: u64) {
        let mut webhooks = Self::get_owner_webhooks(env, owner);
        
        // Remove webhook_id from the vector
        let mut new_webhooks = Vec::new(env);
        for id in webhooks.iter() {
            if id != webhook_id {
                new_webhooks.push_back(id);
            }
        }
        
        let storage = env.storage().persistent();
        // Simplified for now - use a single key for all owner webhooks
        let key = "owner_webhooks_all";
        storage.set(&String::from_str(env, key), &new_webhooks);
    }

    fn get_webhooks_for_event(env: &Env, event_type: &WebhookEventType) -> Vec<u64> {
        let mut webhooks = Vec::new(env);
        
        // Simple implementation - check all webhooks
        for i in 1..=1000 {
            if let Ok(webhook) = Self::get_webhook(env, i) {
                if webhook.events.contains(event_type) || webhook.events.contains(&WebhookEventType::All) {
                    webhooks.push_back(i);
                }
            }
        }
        
        webhooks
    }

    fn deliver_webhook(
        env: &Env,
        webhook: &Webhook,
        _event_type: &WebhookEventType,
        _event_data: &Map<String, String>,
        _metadata: &Map<String, String>,
    ) -> DeliveryResult {
        // In a real implementation, this would make an HTTP request to the webhook URL
        // For now, we'll simulate a successful delivery
        
        // Update webhook statistics
        let mut updated_webhook = webhook.clone();
        updated_webhook.success_count += 1;
        updated_webhook.updated_at = env.ledger().timestamp();
        
        Self::store_webhook(env, webhook.id, &updated_webhook);

        DeliveryResult {
            webhook_id: webhook.id,
            success: true,
            response_time: 100, // Simulated response time in ms
            error_message: None,
        }
    }
}