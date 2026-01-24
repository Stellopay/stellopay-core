use soroban_sdk::{contracterror, contracttype, Address, Env};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Milestone {
    pub id: u32,
    pub amount: i128,
    pub approved: bool,
    pub claimed: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaymentType {
    LumpSum,
    MilestoneBased,
}

/// Core milestone agreement structure
#[contracttype]
#[derive(Clone, Debug)]
pub struct MilestoneAgreement {
    pub id: u128,
    pub employer: Address,
    pub contributor: Address,
    pub token: Address,
    pub payment_type: PaymentType,
    pub status: AgreementStatus,
    pub total_amount: i128,
}

#[contracttype]
#[derive(Clone)]
pub enum MilestoneKey {
    /// Counter for agreement IDs
    AgreementCounter,
    /// Agreement data: agreement_id -> MilestoneAgreement
    Agreement(u128),
    /// Employer address: agreement_id -> Address
    Employer(u128),
    /// Contributor address: agreement_id -> Address
    Contributor(u128),
    /// Token address: agreement_id -> Address
    Token(u128),
    /// Payment type: agreement_id -> PaymentType
    PaymentType(u128),
    /// Agreement status: agreement_id -> AgreementStatus
    Status(u128),
    /// Total amount: agreement_id -> i128
    TotalAmount(u128),

    // Milestone-specific keys
    /// Number of milestones: agreement_id -> u32
    MilestoneCount(u128),
    /// Milestone amount: (agreement_id, milestone_id) -> i128
    MilestoneAmount(u128, u32),
    /// Milestone approval status: (agreement_id, milestone_id) -> bool
    MilestoneApproved(u128, u32),
    /// Milestone claim status: (agreement_id, milestone_id) -> bool
    MilestoneClaimed(u128, u32),
}

impl Milestone {
    pub fn new(id: u32, amount: i128) -> Self {
        Self {
            id,
            amount,
            approved: false,
            claimed: false,
        }
    }

    pub fn can_claim(&self) -> bool {
        self.approved && !self.claimed
    }
}

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
    pub dispute_status: DisputeStatus,
    pub dispute_raised_at: Option<u64>,
    // Time-based payment fields (for escrow mode)
    pub amount_per_period: Option<i128>,
    pub period_seconds: Option<u64>,
    pub num_periods: Option<u32>,
    pub claimed_periods: Option<u32>,
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
    /// Dispute Status
    DisputeStatus(u128),
    DisputeRaisedAt(u128),
    Arbiter,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, PartialOrd)]
pub enum DisputeStatus {
    None,
    Raised,
    Resolved,
}

/// Error types for payroll operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum PayrollError {
    DisputeAlreadyRaised = 1,
    NotInGracePeriod = 2,
    NotParty = 3,
    NotArbiter = 4,
    InvalidPayout = 5,
    ActiveDispute = 6,
    AgreementNotFound = 7,
    NoDispute = 8,
    NoEmployee = 9,
    NotActivated = 10,
    Unauthorized = 11,
    InvalidEmployeeIndex = 12,
    InvalidData = 13,
    TransferFailed = 14,
    InsufficientEscrowBalance = 15,
    NoPeriodsToClaim = 16,
    AgreementNotActivated = 17,
    InvalidAgreementMode = 18,
    AgreementPaused = 19,
    AllPeriodsClaimed = 20,
    ZeroAmountPerPeriod = 21,
    ZeroPeriodDuration = 22,
    ZeroNumPeriods = 23,
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
        env.storage().persistent().get(&key).unwrap_or(0u32)
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
        env.storage().persistent().get(&key).unwrap_or(0u32)
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
        env.storage().persistent().get(&key).unwrap_or(0i128)
    }

    /// Set total paid amount for an agreement
    pub fn set_agreement_paid_amount(env: &Env, agreement_id: u128, amount: i128) {
        let key: DataKey = DataKey::AgreementPaidAmount(agreement_id);
        env.storage().persistent().set(&key, &amount);
    }

    /// Get escrow balance for an agreement and token
    pub fn get_agreement_escrow_balance(env: &Env, agreement_id: u128, token: &Address) -> i128 {
        let key: DataKey = DataKey::AgreementEscrowBalance(agreement_id, token.clone());
        env.storage().persistent().get(&key).unwrap_or(0i128)
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
