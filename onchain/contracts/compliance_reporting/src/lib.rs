#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, Env, Vec,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ComplianceError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
    InvalidDateRange = 4,
    QueryLimitExceeded = 5,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReportType {
    Payroll,
    Tax,
    Regulatory,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComplianceRecord {
    pub id: u32,
    pub employer: Address,
    pub employee: Address,
    pub token: Address,
    pub amount: i128,
    pub timestamp: u64,
    pub report_type: ReportType,
    pub metadata: Bytes, // Additional off-chain reference data (e.g., IPFS hash of a payslip)
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComplianceReport {
    pub employer: Address,
    pub start_date: u64,
    pub end_date: u64,
    pub total_amount: i128,
    pub record_count: u32,
    pub records: Vec<ComplianceRecord>, // Export payload
}

#[contracttype]
pub enum DataKey {
    Admin,
    // Tracks the total number of records per employer: EmployerAddress -> u32
    RecordCount(Address),
    // Stores the actual record: (EmployerAddress, u32) -> ComplianceRecord
    Record(Address, u32),
}

#[contract]
pub struct ComplianceReportingContract;

#[contractimpl]
impl ComplianceReportingContract {
    /// @notice Initializes the compliance reporting contract.
    /// @param admin The administrator address.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ComplianceError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ComplianceError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    /// @notice Logs a new compliance record onto the ledger.
    /// @dev Requires authorization from the employer logging the data.
    /// @param employer The company/entity making the payment.
    /// @param employee The recipient of the payment.
    /// @param token The SPL/Soroban token used.
    /// @param amount The token amount.
    /// @param report_type Classification of the record (Tax, Payroll, Regulatory).
    /// @param metadata Extra bytes for off-chain linkage.
    pub fn log_record(
        env: Env,
        employer: Address,
        employee: Address,
        token: Address,
        amount: i128,
        report_type: ReportType,
        metadata: Bytes,
    ) -> Result<u32, ComplianceError> {
        Self::require_initialized(&env)?;
        employer.require_auth();

        let count_key = DataKey::RecordCount(employer.clone());
        let next_id: u32 = env.storage().persistent().get(&count_key).unwrap_or(0) + 1;

        let timestamp = env.ledger().timestamp();

        let record = ComplianceRecord {
            id: next_id,
            employer: employer.clone(),
            employee,
            token,
            amount,
            timestamp,
            report_type,
            metadata,
        };

        // Store the record
        env.storage()
            .persistent()
            .set(&DataKey::Record(employer.clone(), next_id), &record);
        
        // Update count
        env.storage().persistent().set(&count_key, &next_id);

        // Emit indexable event
        env.events().publish(
            (symbol_short!("log_comp"), employer.clone()),
            (next_id, timestamp, amount),
        );

        Ok(next_id)
    }

    /// @notice Generates an aggregated report and exports matching records.
    /// @dev To prevent gas limits (CPU/Memory), we limit the query to the latest `limit` records.
    /// @param employer The employer to generate the report for.
    /// @param start_date Unix timestamp of the range start.
    /// @param end_date Unix timestamp of the range end.
    /// @param filter_type Optional filter for a specific report type.
    /// @param limit Maximum records to process (to prevent instruction limit overflows).
    pub fn generate_report(
        env: Env,
        employer: Address,
        start_date: u64,
        end_date: u64,
        filter_type: Option<ReportType>,
        limit: u32,
    ) -> Result<ComplianceReport, ComplianceError> {
        Self::require_initialized(&env)?;
        
        if start_date > end_date {
            return Err(ComplianceError::InvalidDateRange);
        }
        if limit > 100 {
            return Err(ComplianceError::QueryLimitExceeded); // Cap to prevent timeout
        }

        let count_key = DataKey::RecordCount(employer.clone());
        let total_records: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let mut matching_records = Vec::new(&env);
        let mut total_amount: i128 = 0;
        let mut processed: u32 = 0;

        // Iterate backwards to get the most recent records first
        let mut current_id = total_records;

        while current_id > 0 && processed < limit {
            if let Some(record) = env
                .storage()
                .persistent()
                .get::<_, ComplianceRecord>(&DataKey::Record(employer.clone(), current_id))
            {
                // Check if within date range
                if record.timestamp >= start_date && record.timestamp <= end_date {
                    // Check type filter
                    let type_matches = match &filter_type {
                        Some(t) => &record.report_type == t,
                        None => true,
                    };

                    if type_matches {
                        total_amount += record.amount;
                        matching_records.push_back(record);
                        processed += 1;
                    }
                } else if record.timestamp < start_date {
                    // Since we iterate backwards chronologically, if we hit a record older than our start_date, 
                    // we can safely break early to save gas.
                    break; 
                }
            }
            current_id -= 1;
        }

        Ok(ComplianceReport {
            employer,
            start_date,
            end_date,
            total_amount,
            record_count: matching_records.len() as u32,
            records: matching_records,
        })
    }

    /// @notice Returns the total number of records logged by an employer.
    pub fn get_record_count(env: Env, employer: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::RecordCount(employer))
            .unwrap_or(0)
    }

    fn require_initialized(env: &Env) -> Result<(), ComplianceError> {
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(ComplianceError::NotInitialized);
        }
        Ok(())
    }
}