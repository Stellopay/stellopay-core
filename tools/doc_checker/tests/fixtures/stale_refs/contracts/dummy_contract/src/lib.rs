#![no_std]

use soroban_sdk::{contract, contractimpl, Env};

#[contract]
pub struct DummyContract;

#[contractimpl]
impl DummyContract {
    pub fn initialize(env: Env) {
    }

    pub fn check_and_consume(env: Env) -> u32 {
        0
    }

    pub fn set_global_limit(env: Env) {
    }

    pub fn get_limit_for(env: Env) -> u32 {
        0
    }
}
