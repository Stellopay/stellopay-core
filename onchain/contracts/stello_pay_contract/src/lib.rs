#![no_std]

mod events;
mod governance;
mod insurance;
mod payroll;
mod storage;
mod webhook_contract;
mod webhooks;
// take note: The following modules have compilation issues with current Soroban SDK
// They need to be fixed to work with the current version
// mod compliance;    // Issues: symbol_short! length limits, format! macro, unsupported types
mod enterprise;
mod token_swap; // Issues: format! macro, missing storage keys, deprecated methods
mod utils;

#[cfg(test)]
mod test;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod governance_test;
