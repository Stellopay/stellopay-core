#![no_std]

mod events;
mod payroll;
mod storage;
mod insurance;
mod webhooks;
mod webhook_contract;
// take note: The following modules have compilation issues with current Soroban SDK
// They need to be fixed to work with the current version
// mod compliance;    // Issues: symbol_short! length limits, format! macro, unsupported types
// mod token_swap;    // Issues: format! macro, missing storage keys, deprecated methods
mod enterprise;

#[cfg(test)]
mod test;

#[cfg(test)]
mod tests;
