#![no_std]

pub mod events;
pub mod storage_types;

use crate::events::{AgreementPaused, AgreementResumed};
use crate::storage_types::{DataKey, Payroll};
use soroban_sdk::{contract, contractimpl, Address, Env, Symbol};

#[contract]
pub struct PayrollContract;

#[contractimpl]
impl PayrollContract {
    pub fn initialize(env: Env, owner: Address) {
        if env.storage().persistent().has(&DataKey::Owner) {
            panic!("Already initialized");
        }
        owner.require_auth();
        env.storage().persistent().set(&DataKey::Owner, &owner);
    }

    // Simplified version to allow testing
    pub fn create_or_update_escrow(
        env: Env,
        employer: Address,
        employee: Address,
        token: Address,
        amount: i128,
        interval: u32,
        recurrence_frequency: u32,
    ) {
        employer.require_auth();

        let payroll = Payroll {
            amount,
            employer,
            interval,
            is_paused: false,
            last_payment_time: 0, // Simplified
            next_payout_timestamp: env.ledger().timestamp() + interval as u64, // Simplified
            recurrence_frequency,
            token,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Payroll(employee), &payroll);
    }

    pub fn pause_agreement(env: Env, employee: Address) {
        let key = DataKey::Payroll(employee.clone());
        if !env.storage().persistent().has(&key) {
            panic!("Agreement not found");
        }

        let mut payroll: Payroll = env.storage().persistent().get(&key).unwrap();

        // Authorization: Only employer or owner can pause?
        // Issue says "Only employer can pause/resume".
        // Also maybe Owner? For now, enforcing Employer.
        payroll.employer.require_auth();

        if payroll.is_paused {
            panic!("Agreement already paused");
        }

        payroll.is_paused = true;
        env.storage().persistent().set(&key, &payroll);

        env.events().publish(
            (Symbol::new(&env, "agreement_paused"),),
            AgreementPaused { employee },
        );
    }

    pub fn resume_agreement(env: Env, employee: Address) {
        let key = DataKey::Payroll(employee.clone());
        if !env.storage().persistent().has(&key) {
            panic!("Agreement not found");
        }

        let mut payroll: Payroll = env.storage().persistent().get(&key).unwrap();

        payroll.employer.require_auth();

        if !payroll.is_paused {
            panic!("Agreement not paused");
        }

        payroll.is_paused = false;
        env.storage().persistent().set(&key, &payroll);

        env.events().publish(
            (Symbol::new(&env, "agreement_resumed"),),
            AgreementResumed { employee },
        );
    }

    pub fn get_payroll(env: Env, employee: Address) -> Option<Payroll> {
        env.storage().persistent().get(&DataKey::Payroll(employee))
    }

    // Simplified claim function to verify blocking
    pub fn claim_payroll(env: Env, employee: Address) {
        employee.require_auth();

        let key = DataKey::Payroll(employee.clone());
        let payroll: Payroll = env
            .storage()
            .persistent()
            .get(&key)
            .expect("Payroll not found");

        if payroll.is_paused {
            panic!("Agreement is paused");
        }

        // Logic for transfer would go here...
    }
}
