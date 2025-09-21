#![no_std]

mod events;
mod payroll;
mod storage;
mod insurance;
mod webhooks;
mod webhook_contract;
// NOTE: The following modules have fundamental compatibility issues with current Soroban SDK
// and require significant refactoring to work properly. They are temporarily disabled
// to maintain core contract functionality while the webhook system is integrated.
//
// Issues identified:
// - compliance.rs: Missing DataKey variants, format! macro usage in no_std, Map type issues
// - token_swap.rs: Missing DataKey variants, deprecated String::from_slice usage, storage key mismatches
//
// These modules will be re-enabled after proper refactoring to match current SDK requirements.
// mod compliance;    // Requires DataKey enum expansion and Map<K,V> type fixes
// mod token_swap;    // Requires DataKey enum expansion and String API updates
mod enterprise;

#[cfg(test)]
mod test;

#[cfg(test)]
mod tests;
