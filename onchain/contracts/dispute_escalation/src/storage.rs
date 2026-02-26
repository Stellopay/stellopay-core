use crate::types::{DisputeDetails, EscalationLevel, StorageKey};
use soroban_sdk::{Address, Env};

/// Set the time limit (in seconds) for a specific escalation level
pub fn set_level_time_limit(env: &Env, level: EscalationLevel, limit_seconds: u64) {
    let key = StorageKey::LevelTimeLimit(level);
    env.storage().persistent().set(&key, &limit_seconds);
}

/// Get the time limit (in seconds) for a specific escalation level.
/// Defaults to 7 days (604800 seconds) if not set.
pub fn get_level_time_limit(env: &Env, level: EscalationLevel) -> u64 {
    let key = StorageKey::LevelTimeLimit(level);
    env.storage().persistent().get(&key).unwrap_or(604800)
}

/// Get details of an active dispute
pub fn get_dispute(env: &Env, agreement_id: u128) -> Option<DisputeDetails> {
    let key = StorageKey::Dispute(agreement_id);
    env.storage().persistent().get(&key)
}

/// Save dispute details
pub fn set_dispute(env: &Env, agreement_id: u128, details: &DisputeDetails) {
    let key = StorageKey::Dispute(agreement_id);
    env.storage().persistent().set(&key, details);
}

/// Check if a given address is the contract administrator
pub fn is_admin(env: &Env, caller: &Address) -> bool {
    if let Some(admin) = env
        .storage()
        .persistent()
        .get::<_, Address>(&StorageKey::Admin)
    {
        return admin == *caller;
    }
    // Fall back to owner if no admin is explicitly set
    if let Some(owner) = env
        .storage()
        .persistent()
        .get::<_, Address>(&StorageKey::Owner)
    {
        return owner == *caller;
    }
    false
}
