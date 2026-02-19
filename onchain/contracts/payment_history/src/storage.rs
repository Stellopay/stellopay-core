use soroban_sdk::{contracttype, Address};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentRecord {
    pub id: u128,
    pub agreement_id: u128,
    pub token: Address,
    pub amount: i128,
    pub from: Address,
    pub to: Address,
    pub timestamp: u64,
}

#[contracttype]
pub enum StorageKey {
    Owner,
    PayrollContract,
    // Global counter for all payments
    GlobalPaymentCount,
    // Main storage: Global ID -> PaymentRecord
    Payment(u128),
    // Indices references
    // Count of payments for a specific entity
    AgreementPaymentCount(u128),   // agreement_id -> count
    EmployerPaymentCount(Address), // employer -> count
    EmployeePaymentCount(Address), // employee -> count

    // Mapping index to Global ID
    // (agreement_id, index) -> global_id
    AgreementPayment(u128, u32),
    // (employer, index) -> global_id
    EmployerPayment(Address, u32),
    // (employee, index) -> global_id
    EmployeePayment(Address, u32),
}
