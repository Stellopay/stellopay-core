#![no_std]

mod events;
mod storage;

use events::PaymentRecorded;
use soroban_sdk::{contract, contractimpl, Address, Env, Vec};
use storage::{PaymentRecord, StorageKey};

#[contract]
pub struct PaymentHistoryContract;

#[contractimpl]
impl PaymentHistoryContract {
    /// Initialize the contract
    pub fn initialize(env: Env, owner: Address, payroll_contract: Address) {
        if env.storage().persistent().has(&StorageKey::Owner) {
            panic!("Already initialized");
        }
        env.storage().persistent().set(&StorageKey::Owner, &owner);
        env.storage()
            .persistent()
            .set(&StorageKey::PayrollContract, &payroll_contract);
        env.storage()
            .persistent()
            .set(&StorageKey::GlobalPaymentCount, &0u128);
    }

    /// Record a payment (Only callable by Payroll Contract)
    pub fn record_payment(
        env: Env,
        agreement_id: u128,
        token: Address,
        amount: i128,
        from: Address,
        to: Address,
        timestamp: u64,
    ) -> u128 {
        // Access Control
        let payroll_contract: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::PayrollContract)
            .unwrap();
        payroll_contract.require_auth();

        // 1. Get new Global ID
        let mut global_count: u128 = env
            .storage()
            .persistent()
            .get(&StorageKey::GlobalPaymentCount)
            .unwrap_or(0);
        global_count += 1;
        env.storage()
            .persistent()
            .set(&StorageKey::GlobalPaymentCount, &global_count);

        let id = global_count;

        // 2. Store Payment Record
        let record = PaymentRecord {
            id,
            agreement_id,
            token: token.clone(),
            amount,
            from: from.clone(),
            to: to.clone(),
            timestamp,
        };
        env.storage()
            .persistent()
            .set(&StorageKey::Payment(id), &record);

        // 3. Update Indices

        // Agreement Index
        let mut agg_count: u32 = env
            .storage()
            .persistent()
            .get(&StorageKey::AgreementPaymentCount(agreement_id))
            .unwrap_or(0);
        agg_count += 1;
        env.storage()
            .persistent()
            .set(&StorageKey::AgreementPaymentCount(agreement_id), &agg_count);
        env.storage()
            .persistent()
            .set(&StorageKey::AgreementPayment(agreement_id, agg_count), &id);

        // Employer Index (From)
        let mut from_count: u32 = env
            .storage()
            .persistent()
            .get(&StorageKey::EmployerPaymentCount(from.clone()))
            .unwrap_or(0);
        from_count += 1;
        env.storage()
            .persistent()
            .set(&StorageKey::EmployerPaymentCount(from.clone()), &from_count);
        env.storage()
            .persistent()
            .set(&StorageKey::EmployerPayment(from.clone(), from_count), &id);

        // Employee Index (To)
        let mut to_count: u32 = env
            .storage()
            .persistent()
            .get(&StorageKey::EmployeePaymentCount(to.clone()))
            .unwrap_or(0);
        to_count += 1;
        env.storage()
            .persistent()
            .set(&StorageKey::EmployeePaymentCount(to.clone()), &to_count);
        env.storage()
            .persistent()
            .set(&StorageKey::EmployeePayment(to.clone(), to_count), &id);

        // Emit Event
        events::emit_payment_recorded(
            &env,
            PaymentRecorded {
                agreement_id,
                token,
                amount,
                from,
                to,
                timestamp,
            },
        );

        id
    }

    /// Get total payment count for an agreement
    pub fn get_agreement_payment_count(env: Env, agreement_id: u128) -> u32 {
        env.storage()
            .persistent()
            .get(&StorageKey::AgreementPaymentCount(agreement_id))
            .unwrap_or(0)
    }

    /// Get payments for an agreement with pagination
    /// - limit: max number of records to return
    /// - reverse: if true, returns latest payments first
    /// - offset: number of records to skip (1-based index concept, but calculated from total)
    ///     Actually, purely index based access is better for pagination.
    ///     Let's use (page_number, page_size) or (start_index, limit)?
    ///     Standard limit/offset is easier.
    pub fn get_payments_by_agreement(
        env: Env,
        agreement_id: u128,
        start_index: u32,
        limit: u32,
    ) -> Vec<PaymentRecord> {
        let count = Self::get_agreement_payment_count(env.clone(), agreement_id);
        let mut result = Vec::new(&env);

        if start_index == 0 || start_index > count {
            return result;
        }

        let end = core::cmp::min(start_index + limit, count + 1);

        for i in start_index..end {
            let global_id: u128 = env
                .storage()
                .persistent()
                .get(&StorageKey::AgreementPayment(agreement_id, i))
                .unwrap();
            let record: PaymentRecord = env
                .storage()
                .persistent()
                .get(&StorageKey::Payment(global_id))
                .unwrap();
            result.push_back(record);
        }
        result
    }

    /// Get total payment count for an employer
    pub fn get_employer_payment_count(env: Env, employer: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&StorageKey::EmployerPaymentCount(employer))
            .unwrap_or(0)
    }

    /// Get payments for an employer
    pub fn get_payments_by_employer(
        env: Env,
        employer: Address,
        start_index: u32,
        limit: u32,
    ) -> Vec<PaymentRecord> {
        let count = Self::get_employer_payment_count(env.clone(), employer.clone());
        let mut result = Vec::new(&env);

        if start_index == 0 || start_index > count {
            return result;
        }

        let end = core::cmp::min(start_index + limit, count + 1);

        for i in start_index..end {
            let global_id: u128 = env
                .storage()
                .persistent()
                .get(&StorageKey::EmployerPayment(employer.clone(), i))
                .unwrap();
            let record: PaymentRecord = env
                .storage()
                .persistent()
                .get(&StorageKey::Payment(global_id))
                .unwrap();
            result.push_back(record);
        }
        result
    }

    /// Get total payment count for an employee
    pub fn get_employee_payment_count(env: Env, employee: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&StorageKey::EmployeePaymentCount(employee))
            .unwrap_or(0)
    }

    /// Get payments for an employee
    pub fn get_payments_by_employee(
        env: Env,
        employee: Address,
        start_index: u32,
        limit: u32,
    ) -> Vec<PaymentRecord> {
        let count = Self::get_employee_payment_count(env.clone(), employee.clone());
        let mut result = Vec::new(&env);

        if start_index == 0 || start_index > count {
            return result;
        }

        let end = core::cmp::min(start_index + limit, count + 1);

        for i in start_index..end {
            let global_id: u128 = env
                .storage()
                .persistent()
                .get(&StorageKey::EmployeePayment(employee.clone(), i))
                .unwrap();
            let record: PaymentRecord = env
                .storage()
                .persistent()
                .get(&StorageKey::Payment(global_id))
                .unwrap();
            result.push_back(record);
        }
        result
    }
}
