use soroban_sdk::{contracttype, Address, Env};

pub type Error = soroban_sdk::Error;

/// Operating mode for agreements
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgreementMode {
    /// Escrow mode for freelance/contract work
    Escrow,
    /// Payroll mode for traditional employee payroll
    Payroll,
}

/// Lifecycle states for agreements
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgreementStatus {
    /// Agreement created but not yet funded/activated
    Created,
    /// Agreement is active and payments can be processed
    Active,
    /// Agreement temporarily paused
    Paused,
    /// Agreement cancelled by employer
    Cancelled,
    /// Agreement completed successfully
    Completed,
    /// Agreement in dispute
    Disputed,
}

/// Core agreement structure
#[contracttype]
#[derive(Clone, Debug)]
pub struct Agreement {
    pub id: u128,
    pub employer: Address,
    pub token: Address,
    pub mode: AgreementMode,
    pub status: AgreementStatus,
    pub total_amount: i128,
    pub paid_amount: i128,
    pub created_at: u64,
    pub activated_at: Option<u64>,
    pub cancelled_at: Option<u64>,
    pub grace_period_seconds: u64,
}

/// Employee info within an agreement
#[contracttype]
#[derive(Clone, Debug)]
pub struct EmployeeInfo {
    pub address: Address,
    pub salary_per_period: i128,
    pub added_at: u64,
}

/// Storage keys
#[contracttype]
#[derive(Clone)]
pub enum StorageKey {
    /// Contract owner
    Owner,
    /// Agreement by ID
    Agreement(u128),
    /// List of employees for an agreement
    AgreementEmployees(u128),
    /// Next agreement ID counter
    NextAgreementId,
    /// List of agreement IDs for an employer
    EmployerAgreements(Address),
}

/// Represents payroll information for an employee within an agreement.
#[derive(Clone)]
#[contracttype]
pub struct EmployeePayroll {
    /// Employee address
    pub employee: Address,
    /// Salary amount per period
    pub salary_per_period: i128,
    /// Number of periods already claimed by this employee
    pub claimed_periods: u32,
}

/// Storage keys for the payroll claiming system.
#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    /// Maps agreement ID to the number of employees in that agreement
    /// Key: AgreementEmployeeCount(u128)
    /// Value: u32
    AgreementEmployeeCount(u128),

    /// Maps agreement ID and employee index to employee address
    /// Key: AgreementEmployee(u128, u32)
    /// Value: Address
    AgreementEmployee(u128, u32),

    /// Maps agreement ID and employee index to salary per period
    /// Key: EmployeeSalary(u128, u32)
    /// Value: i128
    EmployeeSalary(u128, u32),

    /// Maps agreement ID and employee index to number of claimed periods
    /// Key: EmployeeClaimedPeriods(u128, u32)
    /// Value: u32
    EmployeeClaimedPeriods(u128, u32),

    /// Maps agreement ID to activation timestamp
    /// Key: AgreementActivationTime(u128)
    /// Value: u64
    AgreementActivationTime(u128),

    /// Maps agreement ID to period duration in seconds
    /// Key: AgreementPeriodDuration(u128)
    /// Value: u64
    AgreementPeriodDuration(u128),

    /// Maps agreement ID to token address
    /// Key: AgreementToken(u128)
    /// Value: Address
    AgreementToken(u128),

    /// Maps agreement ID to total paid amount
    /// Key: AgreementPaidAmount(u128)
    /// Value: i128
    AgreementPaidAmount(u128),

    /// Maps agreement ID and token to escrow balance
    /// Key: AgreementEscrowBalance(u128, Address)
    /// Value: i128
    AgreementEscrowBalance(u128, Address),
}

impl DataKey {
    /// Get the number of employees in an agreement
    pub fn get_employee_count(env: &Env, agreement_id: u128) -> u32 {
        let key: DataKey = DataKey::AgreementEmployeeCount(agreement_id);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or(0u32)
    }

    /// Set the number of employees in an agreement
    pub fn set_employee_count(env: &Env, agreement_id: u128, count: u32) {
        let key: DataKey = DataKey::AgreementEmployeeCount(agreement_id);
        env.storage().persistent().set(&key, &count);
    }

    /// Get employee address at a specific index in an agreement
    pub fn get_employee(env: &Env, agreement_id: u128, employee_index: u32) -> Option<Address> {
        let key: DataKey = DataKey::AgreementEmployee(agreement_id, employee_index);
        env.storage().persistent().get(&key)
    }

    /// Set employee address at a specific index in an agreement
    pub fn set_employee(env: &Env, agreement_id: u128, employee_index: u32, employee: &Address) {
        let key: DataKey = DataKey::AgreementEmployee(agreement_id, employee_index);
        env.storage().persistent().set(&key, employee);
    }

    /// Get salary per period for an employee at a specific index
    pub fn get_employee_salary(env: &Env, agreement_id: u128, employee_index: u32) -> Option<i128> {
        let key: DataKey = DataKey::EmployeeSalary(agreement_id, employee_index);
        env.storage().persistent().get(&key)
    }

    /// Set salary per period for an employee at a specific index
    pub fn set_employee_salary(env: &Env, agreement_id: u128, employee_index: u32, salary: i128) {
        let key: DataKey = DataKey::EmployeeSalary(agreement_id, employee_index);
        env.storage().persistent().set(&key, &salary);
    }

    /// Get number of claimed periods for an employee at a specific index
    pub fn get_employee_claimed_periods(env: &Env, agreement_id: u128, employee_index: u32) -> u32 {
        let key: DataKey = DataKey::EmployeeClaimedPeriods(agreement_id, employee_index);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or(0u32)
    }

    /// Set number of claimed periods for an employee at a specific index
    pub fn set_employee_claimed_periods(
        env: &Env,
        agreement_id: u128,
        employee_index: u32,
        periods: u32,
    ) {
        let key: DataKey = DataKey::EmployeeClaimedPeriods(agreement_id, employee_index);
        env.storage().persistent().set(&key, &periods);
    }

    /// Get activation timestamp for an agreement
    pub fn get_agreement_activation_time(env: &Env, agreement_id: u128) -> Option<u64> {
        let key: DataKey = DataKey::AgreementActivationTime(agreement_id);
        env.storage().persistent().get(&key)
    }

    /// Set activation timestamp for an agreement
    pub fn set_agreement_activation_time(env: &Env, agreement_id: u128, timestamp: u64) {
        let key: DataKey = DataKey::AgreementActivationTime(agreement_id);
        env.storage().persistent().set(&key, &timestamp);
    }

    /// Get period duration in seconds for an agreement
    pub fn get_agreement_period_duration(env: &Env, agreement_id: u128) -> Option<u64> {
        let key: DataKey = DataKey::AgreementPeriodDuration(agreement_id);
        env.storage().persistent().get(&key)
    }

    /// Set period duration in seconds for an agreement
    pub fn set_agreement_period_duration(env: &Env, agreement_id: u128, duration: u64) {
        let key: DataKey = DataKey::AgreementPeriodDuration(agreement_id);
        env.storage().persistent().set(&key, &duration);
    }

    /// Get token address for an agreement
    pub fn get_agreement_token(env: &Env, agreement_id: u128) -> Option<Address> {
        let key: DataKey = DataKey::AgreementToken(agreement_id);
        env.storage().persistent().get(&key)
    }

    /// Set token address for an agreement
    pub fn set_agreement_token(env: &Env, agreement_id: u128, token: &Address) {
        let key: DataKey = DataKey::AgreementToken(agreement_id);
        env.storage().persistent().set(&key, token);
    }

    /// Get total paid amount for an agreement
    pub fn get_agreement_paid_amount(env: &Env, agreement_id: u128) -> i128 {
        let key: DataKey = DataKey::AgreementPaidAmount(agreement_id);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or(0i128)
    }

    /// Set total paid amount for an agreement
    pub fn set_agreement_paid_amount(env: &Env, agreement_id: u128, amount: i128) {
        let key: DataKey = DataKey::AgreementPaidAmount(agreement_id);
        env.storage().persistent().set(&key, &amount);
    }

    /// Get escrow balance for an agreement and token
    pub fn get_agreement_escrow_balance(env: &Env, agreement_id: u128, token: &Address) -> i128 {
        let key: DataKey = DataKey::AgreementEscrowBalance(agreement_id, token.clone());
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or(0i128)
    }

    /// Set escrow balance for an agreement and token
    pub fn set_agreement_escrow_balance(
        env: &Env,
        agreement_id: u128,
        token: &Address,
        amount: i128,
    ) {
        let key: DataKey = DataKey::AgreementEscrowBalance(agreement_id, token.clone());
        env.storage().persistent().set(&key, &amount);
    }
}
