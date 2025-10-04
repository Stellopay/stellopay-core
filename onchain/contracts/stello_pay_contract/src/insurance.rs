use soroban_sdk::{contracterror, contracttype, symbol_short, Address, Env, String, Symbol, Vec};

//-----------------------------------------------------------------------------
// Insurance Errors
//-----------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum InsuranceError {
    /// Raised when insurance is not active for an employee
    InsuranceNotActive = 100,
    /// Raised when premium calculation fails
    InvalidPremiumCalculation = 101,
    /// Raised when claim amount exceeds coverage
    ClaimExceedsCoverage = 102,
    /// Raised when insurance pool has insufficient funds
    InsufficientPoolFunds = 103,
    /// Raised when claim is not eligible
    ClaimNotEligible = 104,
    /// Raised when insurance period has not started
    InsurancePeriodNotStarted = 105,
    /// Raised when insurance period has expired
    InsurancePeriodExpired = 106,
    /// Raised when premium payment is insufficient
    InsufficientPremiumPayment = 107,
    /// Raised when guarantee amount exceeds available pool
    GuaranteeExceedsPool = 108,
    /// Raised when claim is already processed
    ClaimAlreadyProcessed = 109,
    /// Raised when risk assessment fails
    InvalidRiskAssessment = 110,
}

impl From<crate::payroll::PayrollError> for InsuranceError {
    fn from(_: crate::payroll::PayrollError) -> Self {
        InsuranceError::ClaimNotEligible
    }
}

//-----------------------------------------------------------------------------
// Insurance Data Structures
//-----------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct InsurancePolicy {
    pub employee: Address,
    pub employer: Address,
    pub token: Address,
    pub coverage_amount: i128,     // Maximum coverage amount
    pub premium_rate: u32,         // Premium rate in basis points (1/10000)
    pub premium_amount: i128,      // Calculated premium amount
    pub start_timestamp: u64,      // When insurance coverage starts
    pub end_timestamp: u64,        // When insurance coverage ends
    pub is_active: bool,           // Whether policy is active
    pub risk_score: u32,           // Risk assessment score (1-100)
    pub claim_count: u32,          // Number of claims made
    pub total_claims_paid: i128,   // Total amount paid in claims
    pub last_premium_payment: u64, // Last premium payment timestamp
    pub next_premium_due: u64,     // Next premium due timestamp
    pub premium_frequency: u64,    // Premium payment frequency in seconds
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct InsuranceClaim {
    pub claim_id: u64,
    pub employee: Address,
    pub employer: Address,
    pub token: Address,
    pub claim_amount: i128,
    pub claim_reason: String, // Reason for claim
    pub claim_timestamp: u64,
    pub status: ClaimStatus,
    pub approved_amount: i128,
    pub approved_timestamp: Option<u64>,
    pub approved_by: Option<Address>,
    pub evidence_hash: Option<String>, // Hash of supporting evidence
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ClaimStatus {
    Pending,
    Approved,
    Rejected,
    Paid,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct InsurancePool {
    pub token: Address,
    pub total_funds: i128,              // Total funds in pool
    pub available_funds: i128,          // Available funds for claims
    pub reserved_funds: i128,           // Reserved for pending claims
    pub total_premiums_collected: i128, // Total premiums collected
    pub total_claims_paid: i128,        // Total claims paid out
    pub active_policies: u32,           // Number of active policies
    pub risk_adjustment_factor: u32,    // Risk adjustment factor (1-200)
    pub min_coverage_amount: i128,      // Minimum coverage amount
    pub max_coverage_amount: i128,      // Maximum coverage amount
    pub pool_fee_rate: u32,             // Pool management fee rate in basis points
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct GuaranteeFund {
    pub token: Address,
    pub total_funds: i128,             // Total guarantee funds
    pub available_funds: i128,         // Available guarantee funds
    pub total_guarantees_issued: i128, // Total guarantees issued
    pub total_guarantees_repaid: i128, // Total guarantees repaid
    pub guarantee_fee_rate: u32,       // Guarantee fee rate in basis points
    pub max_guarantee_amount: i128,    // Maximum guarantee amount per employer
    pub min_guarantee_amount: i128,    // Minimum guarantee amount
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Guarantee {
    pub guarantee_id: u64,
    pub employer: Address,
    pub token: Address,
    pub guarantee_amount: i128,
    pub issued_timestamp: u64,
    pub expiry_timestamp: u64,
    pub is_active: bool,
    pub is_repaid: bool,
    pub fee_amount: i128,
    pub collateral_amount: i128, // Collateral provided by employer
}

//-----------------------------------------------------------------------------
// Insurance Events
//-----------------------------------------------------------------------------

pub const INSURANCE_POLICY_CREATED: Symbol = symbol_short!("ins_pol_c");
pub const INSURANCE_POLICY_UPDATED: Symbol = symbol_short!("ins_pol_u");
pub const INSURANCE_CLAIM_FILED: Symbol = symbol_short!("ins_clm_f");
pub const INSURANCE_CLAIM_APPROVED: Symbol = symbol_short!("ins_clm_a");
pub const INSURANCE_CLAIM_PAID: Symbol = symbol_short!("ins_clm_p");
pub const PREMIUM_PAID: Symbol = symbol_short!("prem_pai");
pub const GUARANTEE_ISSUED: Symbol = symbol_short!("guar_iss");
pub const GUARANTEE_REPAID: Symbol = symbol_short!("guar_rep");
pub const POOL_FUNDED: Symbol = symbol_short!("pool_fun");

//-----------------------------------------------------------------------------
// Insurance Storage Keys
//-----------------------------------------------------------------------------

#[contracttype]
pub enum InsuranceDataKey {
    // Insurance policies
    InsurancePolicy(Address), // employee -> InsurancePolicy

    // Insurance claims
    InsuranceClaim(u64), // claim_id -> InsuranceClaim
    NextClaimId,         // Next available claim ID

    // Insurance pools by token
    InsurancePool(Address), // token -> InsurancePool

    // Guarantee funds by token
    GuaranteeFund(Address), // token -> GuaranteeFund

    // Guarantees
    Guarantee(u64),  // guarantee_id -> Guarantee
    NextGuaranteeId, // Next available guarantee ID

    // Employer guarantees
    EmployerGuarantees(Address), // employer -> Vec<u64> (guarantee IDs)

    // Risk assessment data
    RiskAssessment(Address), // employee -> u32 (risk score)

    // Insurance settings
    InsuranceSettings, // Global insurance settings
}

//-----------------------------------------------------------------------------
// Insurance Settings
//-----------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct InsuranceSettings {
    pub default_premium_rate: u32,  // Default premium rate in basis points
    pub max_risk_score: u32,        // Maximum acceptable risk score
    pub min_premium_frequency: u64, // Minimum premium payment frequency
    pub claim_processing_fee: u32,  // Fee for processing claims in basis points
    pub max_claim_amount: i128,     // Maximum claim amount per policy
    pub claim_approval_threshold: u32, // Minimum approvals needed for large claims
    pub insurance_enabled: bool,    // Whether insurance system is enabled
}

//-----------------------------------------------------------------------------
// Insurance Implementation
//-----------------------------------------------------------------------------

pub struct InsuranceSystem;

impl InsuranceSystem {
    /// Create or update an insurance policy for an employee
    pub fn create_or_update_insurance_policy(
        env: &Env,
        employer: &Address,
        employee: &Address,
        token: &Address,
        coverage_amount: i128,
        premium_frequency: u64,
    ) -> Result<InsurancePolicy, InsuranceError> {
        // Validate inputs
        if coverage_amount <= 0 {
            return Err(InsuranceError::InvalidPremiumCalculation);
        }

        if premium_frequency == 0 {
            return Err(InsuranceError::InvalidPremiumCalculation);
        }

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get or create insurance pool
        let mut pool = Self::get_or_create_insurance_pool(env, token)?;

        // Calculate risk score
        let risk_score = Self::calculate_risk_score(env, employee, employer)?;

        // Calculate premium rate based on risk
        let premium_rate = Self::calculate_premium_rate(&pool, risk_score)?;

        // Calculate premium amount
        let premium_amount =
            Self::calculate_premium_amount(coverage_amount, premium_rate, premium_frequency)?;

        // Get existing policy or create new one
        let mut policy = storage
            .get(&InsuranceDataKey::InsurancePolicy(employee.clone()))
            .unwrap_or(InsurancePolicy {
                employee: employee.clone(),
                employer: employer.clone(),
                token: token.clone(),
                coverage_amount,
                premium_rate,
                premium_amount,
                start_timestamp: current_time,
                end_timestamp: current_time + (premium_frequency * 12), // 12 periods default
                is_active: true,
                risk_score,
                claim_count: 0,
                total_claims_paid: 0,
                last_premium_payment: current_time,
                next_premium_due: current_time + premium_frequency,
                premium_frequency,
            });

        // Update policy if it exists
        if policy.is_active {
            policy.coverage_amount = coverage_amount;
            policy.premium_rate = premium_rate;
            policy.premium_amount = premium_amount;
            policy.risk_score = risk_score;
            policy.premium_frequency = premium_frequency;
            policy.next_premium_due = current_time + premium_frequency;
        }

        // Store policy
        storage.set(
            &InsuranceDataKey::InsurancePolicy(employee.clone()),
            &policy,
        );

        // Update pool active policies count
        pool.active_policies += 1;
        storage.set(&InsuranceDataKey::InsurancePool(token.clone()), &pool);

        // Emit event
        env.events().publish(
            (INSURANCE_POLICY_CREATED,),
            (
                employer.clone(),
                employee.clone(),
                coverage_amount,
                premium_amount,
            ),
        );

        Ok(policy)
    }

    /// Pay insurance premium
    pub fn pay_premium(
        env: &Env,
        employer: &Address,
        employee: &Address,
        amount: i128,
    ) -> Result<(), InsuranceError> {
        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get policy
        let mut policy: InsurancePolicy = storage
            .get(&InsuranceDataKey::InsurancePolicy(employee.clone()))
            .ok_or(InsuranceError::InsuranceNotActive)?;

        if !policy.is_active {
            return Err(InsuranceError::InsuranceNotActive);
        }

        // Check if premium is due
        if current_time < policy.next_premium_due {
            return Err(InsuranceError::InsurancePeriodNotStarted);
        }

        // Check if payment is sufficient
        if amount < policy.premium_amount {
            return Err(InsuranceError::InsufficientPremiumPayment);
        }

        // Update policy
        policy.last_premium_payment = current_time;
        policy.next_premium_due = current_time + policy.premium_frequency;
        policy.is_active = true;

        // Store updated policy
        storage.set(
            &InsuranceDataKey::InsurancePolicy(employee.clone()),
            &policy,
        );

        // Update insurance pool
        let mut pool = Self::get_insurance_pool(env, &policy.token)?;
        pool.total_premiums_collected += amount;
        pool.available_funds += amount;
        storage.set(
            &InsuranceDataKey::InsurancePool(policy.token.clone()),
            &pool,
        );

        // Emit event
        env.events().publish(
            (PREMIUM_PAID,),
            (employer.clone(), employee.clone(), amount, current_time),
        );

        Ok(())
    }

    /// File an insurance claim
    pub fn file_claim(
        env: &Env,
        employee: &Address,
        claim_amount: i128,
        claim_reason: String,
        evidence_hash: Option<String>,
    ) -> Result<u64, InsuranceError> {
        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get policy
        let policy: InsurancePolicy = storage
            .get(&InsuranceDataKey::InsurancePolicy(employee.clone()))
            .ok_or(InsuranceError::InsuranceNotActive)?;

        if !policy.is_active {
            return Err(InsuranceError::InsuranceNotActive);
        }

        // Check if claim amount is within coverage
        if claim_amount > policy.coverage_amount {
            return Err(InsuranceError::ClaimExceedsCoverage);
        }

        // Check if insurance period is active
        if current_time < policy.start_timestamp || current_time > policy.end_timestamp {
            return Err(InsuranceError::InsurancePeriodExpired);
        }

        // Get next claim ID
        let claim_id = storage.get(&InsuranceDataKey::NextClaimId).unwrap_or(1u64);

        // Create claim
        let claim = InsuranceClaim {
            claim_id,
            employee: employee.clone(),
            employer: policy.employer.clone(),
            token: policy.token.clone(),
            claim_amount,
            claim_reason,
            claim_timestamp: current_time,
            status: ClaimStatus::Pending,
            approved_amount: 0,
            approved_timestamp: None,
            approved_by: None,
            evidence_hash,
        };

        // Store claim
        storage.set(&InsuranceDataKey::InsuranceClaim(claim_id), &claim);
        storage.set(&InsuranceDataKey::NextClaimId, &(claim_id + 1));

        // Update policy claim count
        let mut updated_policy = policy.clone();
        updated_policy.claim_count += 1;
        storage.set(
            &InsuranceDataKey::InsurancePolicy(employee.clone()),
            &updated_policy,
        );

        // Emit event
        env.events().publish(
            (INSURANCE_CLAIM_FILED,),
            (employee.clone(), claim_id, claim_amount, current_time),
        );

        Ok(claim_id)
    }

    /// Approve an insurance claim (admin function)
    pub fn approve_claim(
        env: &Env,
        approver: &Address,
        claim_id: u64,
        approved_amount: i128,
    ) -> Result<(), InsuranceError> {
        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get claim
        let mut claim: InsuranceClaim = storage
            .get(&InsuranceDataKey::InsuranceClaim(claim_id))
            .ok_or(InsuranceError::ClaimNotEligible)?;

        if claim.status != ClaimStatus::Pending {
            return Err(InsuranceError::ClaimAlreadyProcessed);
        }

        // Check if approved amount is valid
        if approved_amount > claim.claim_amount || approved_amount <= 0 {
            return Err(InsuranceError::InvalidPremiumCalculation);
        }

        // Update claim
        claim.status = ClaimStatus::Approved;
        claim.approved_amount = approved_amount;
        claim.approved_timestamp = Some(current_time);
        claim.approved_by = Some(approver.clone());

        // Store updated claim
        storage.set(&InsuranceDataKey::InsuranceClaim(claim_id), &claim);

        // Emit event
        env.events().publish(
            (INSURANCE_CLAIM_APPROVED,),
            (claim_id, approver.clone(), approved_amount, current_time),
        );

        Ok(())
    }

    /// Pay out an approved claim
    pub fn pay_claim(env: &Env, claim_id: u64) -> Result<(), InsuranceError> {
        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get claim
        let mut claim: InsuranceClaim = storage
            .get(&InsuranceDataKey::InsuranceClaim(claim_id))
            .ok_or(InsuranceError::ClaimNotEligible)?;

        if claim.status != ClaimStatus::Approved {
            return Err(InsuranceError::ClaimNotEligible);
        }

        // Get insurance pool
        let mut pool = Self::get_insurance_pool(env, &claim.token)?;

        // Check if pool has sufficient funds
        if pool.available_funds < claim.approved_amount {
            return Err(InsuranceError::InsufficientPoolFunds);
        }

        // Update pool
        pool.available_funds -= claim.approved_amount;
        pool.total_claims_paid += claim.approved_amount;
        storage.set(&InsuranceDataKey::InsurancePool(claim.token.clone()), &pool);

        // Update claim
        claim.status = ClaimStatus::Paid;
        storage.set(&InsuranceDataKey::InsuranceClaim(claim_id), &claim);

        // Update policy
        let mut policy: InsurancePolicy = storage
            .get(&InsuranceDataKey::InsurancePolicy(claim.employee.clone()))
            .ok_or(InsuranceError::InsuranceNotActive)?;
        policy.total_claims_paid += claim.approved_amount;
        storage.set(
            &InsuranceDataKey::InsurancePolicy(claim.employee.clone()),
            &policy,
        );

        // Emit event
        env.events().publish(
            (INSURANCE_CLAIM_PAID,),
            (
                claim_id,
                claim.employee.clone(),
                claim.approved_amount,
                current_time,
            ),
        );

        Ok(())
    }

    /// Issue a guarantee for an employer
    pub fn issue_guarantee(
        env: &Env,
        employer: &Address,
        token: &Address,
        guarantee_amount: i128,
        collateral_amount: i128,
        expiry_duration: u64,
    ) -> Result<u64, InsuranceError> {
        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Validate inputs
        if guarantee_amount <= 0 || collateral_amount <= 0 {
            return Err(InsuranceError::InvalidPremiumCalculation);
        }

        // Get or create guarantee fund
        let mut fund = Self::get_or_create_guarantee_fund(env, token)?;

        // Check if guarantee amount is within limits
        if guarantee_amount > fund.max_guarantee_amount {
            return Err(InsuranceError::GuaranteeExceedsPool);
        }

        if guarantee_amount < fund.min_guarantee_amount {
            return Err(InsuranceError::InvalidPremiumCalculation);
        }

        // Check if fund has sufficient capacity
        if fund.available_funds < guarantee_amount {
            return Err(InsuranceError::InsufficientPoolFunds);
        }

        // Calculate guarantee fee
        let fee_amount = (guarantee_amount * fund.guarantee_fee_rate as i128) / 10000;

        // Get next guarantee ID
        let guarantee_id = storage
            .get(&InsuranceDataKey::NextGuaranteeId)
            .unwrap_or(1u64);

        // Create guarantee
        let guarantee = Guarantee {
            guarantee_id,
            employer: employer.clone(),
            token: token.clone(),
            guarantee_amount,
            issued_timestamp: current_time,
            expiry_timestamp: current_time + expiry_duration,
            is_active: true,
            is_repaid: false,
            fee_amount,
            collateral_amount,
        };

        // Store guarantee
        storage.set(&InsuranceDataKey::Guarantee(guarantee_id), &guarantee);
        storage.set(&InsuranceDataKey::NextGuaranteeId, &(guarantee_id + 1));

        // Update guarantee fund
        fund.available_funds -= guarantee_amount;
        fund.total_guarantees_issued += guarantee_amount;
        storage.set(&InsuranceDataKey::GuaranteeFund(token.clone()), &fund);

        // Add to employer guarantees
        let mut employer_guarantees: Vec<u64> = storage
            .get(&InsuranceDataKey::EmployerGuarantees(employer.clone()))
            .unwrap_or(Vec::new(env));
        employer_guarantees.push_back(guarantee_id);
        storage.set(
            &InsuranceDataKey::EmployerGuarantees(employer.clone()),
            &employer_guarantees,
        );

        // Emit event
        env.events().publish(
            (GUARANTEE_ISSUED,),
            (
                employer.clone(),
                guarantee_id,
                guarantee_amount,
                current_time,
            ),
        );

        Ok(guarantee_id)
    }

    /// Repay a guarantee
    pub fn repay_guarantee(
        env: &Env,
        employer: &Address,
        guarantee_id: u64,
        repayment_amount: i128,
    ) -> Result<(), InsuranceError> {
        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get guarantee
        let mut guarantee: Guarantee = storage
            .get(&InsuranceDataKey::Guarantee(guarantee_id))
            .ok_or(InsuranceError::ClaimNotEligible)?;

        if !guarantee.is_active || guarantee.is_repaid {
            return Err(InsuranceError::ClaimNotEligible);
        }

        if guarantee.employer != *employer {
            return Err(InsuranceError::ClaimNotEligible);
        }

        // Check if guarantee has expired
        if current_time > guarantee.expiry_timestamp {
            return Err(InsuranceError::InsurancePeriodExpired);
        }

        // Update guarantee
        guarantee.is_repaid = true;
        guarantee.is_active = false;
        storage.set(&InsuranceDataKey::Guarantee(guarantee_id), &guarantee);

        // Update guarantee fund
        let mut fund = Self::get_guarantee_fund(env, &guarantee.token)?;
        fund.available_funds += guarantee.guarantee_amount;
        fund.total_guarantees_repaid += repayment_amount;
        storage.set(
            &InsuranceDataKey::GuaranteeFund(guarantee.token.clone()),
            &fund,
        );

        // Emit event
        env.events().publish(
            (GUARANTEE_REPAID,),
            (
                employer.clone(),
                guarantee_id,
                repayment_amount,
                current_time,
            ),
        );

        Ok(())
    }

    /// Fund the insurance pool
    pub fn fund_insurance_pool(
        env: &Env,
        funder: &Address,
        token: &Address,
        amount: i128,
    ) -> Result<(), InsuranceError> {
        if amount <= 0 {
            return Err(InsuranceError::InvalidPremiumCalculation);
        }

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        // Get or create pool
        let mut pool = Self::get_or_create_insurance_pool(env, token)?;

        // Update pool
        pool.total_funds += amount;
        pool.available_funds += amount;
        storage.set(&InsuranceDataKey::InsurancePool(token.clone()), &pool);

        // Emit event
        env.events()
            .publish((POOL_FUNDED,), (funder.clone(), amount, current_time));

        Ok(())
    }

    //-----------------------------------------------------------------------------
    // Helper Functions
    //-----------------------------------------------------------------------------

    /// Get or create insurance pool
    fn get_or_create_insurance_pool(
        env: &Env,
        token: &Address,
    ) -> Result<InsurancePool, InsuranceError> {
        let storage = env.storage().persistent();

        if let Some(pool) = storage.get(&InsuranceDataKey::InsurancePool(token.clone())) {
            Ok(pool)
        } else {
            // Create default pool
            let pool = InsurancePool {
                token: token.clone(),
                total_funds: 0,
                available_funds: 0,
                reserved_funds: 0,
                total_premiums_collected: 0,
                total_claims_paid: 0,
                active_policies: 0,
                risk_adjustment_factor: 100,  // 1.0x
                min_coverage_amount: 1000,    // Minimum coverage
                max_coverage_amount: 1000000, // Maximum coverage
                pool_fee_rate: 50,            // 0.5% pool fee
            };
            storage.set(&InsuranceDataKey::InsurancePool(token.clone()), &pool);
            Ok(pool)
        }
    }

    /// Get insurance pool
    fn get_insurance_pool(env: &Env, token: &Address) -> Result<InsurancePool, InsuranceError> {
        let storage = env.storage().persistent();
        storage
            .get(&InsuranceDataKey::InsurancePool(token.clone()))
            .ok_or(InsuranceError::ClaimNotEligible)
    }

    /// Get or create guarantee fund
    fn get_or_create_guarantee_fund(
        env: &Env,
        token: &Address,
    ) -> Result<GuaranteeFund, InsuranceError> {
        let storage = env.storage().persistent();

        if let Some(fund) = storage.get(&InsuranceDataKey::GuaranteeFund(token.clone())) {
            Ok(fund)
        } else {
            // Create default fund
            let fund = GuaranteeFund {
                token: token.clone(),
                total_funds: 0,
                available_funds: 0,
                total_guarantees_issued: 0,
                total_guarantees_repaid: 0,
                guarantee_fee_rate: 100,      // 1% guarantee fee
                max_guarantee_amount: 500000, // Maximum guarantee per employer
                min_guarantee_amount: 1000,   // Minimum guarantee
            };
            storage.set(&InsuranceDataKey::GuaranteeFund(token.clone()), &fund);
            Ok(fund)
        }
    }

    /// Get guarantee fund
    fn get_guarantee_fund(env: &Env, token: &Address) -> Result<GuaranteeFund, InsuranceError> {
        let storage = env.storage().persistent();
        storage
            .get(&InsuranceDataKey::GuaranteeFund(token.clone()))
            .ok_or(InsuranceError::ClaimNotEligible)
    }

    /// Calculate risk score for an employee
    fn calculate_risk_score(
        env: &Env,
        employee: &Address,
        _employer: &Address,
    ) -> Result<u32, InsuranceError> {
        let storage = env.storage().persistent();

        // Check if we have a cached risk assessment
        if let Some(cached_score) = storage.get(&InsuranceDataKey::RiskAssessment(employee.clone()))
        {
            return Ok(cached_score);
        }

        // Simple risk calculation based on employer and employee factors
        // In a real implementation, this would be more sophisticated
        let base_score = 50u32; // Base risk score

        // Adjust based on employer factors (simplified)
        let employer_factor = 10u32; // Could be based on employer history

        // Adjust based on employee factors (simplified)
        let employee_factor = 5u32; // Could be based on employee history

        let risk_score = base_score
            .saturating_add(employer_factor)
            .saturating_add(employee_factor);
        let risk_score = risk_score.min(100); // Cap at 100

        // Cache the risk score
        storage.set(
            &InsuranceDataKey::RiskAssessment(employee.clone()),
            &risk_score,
        );

        Ok(risk_score)
    }

    /// Calculate premium rate based on risk and pool factors
    fn calculate_premium_rate(
        pool: &InsurancePool,
        risk_score: u32,
    ) -> Result<u32, InsuranceError> {
        if risk_score == 0 || risk_score > 100 {
            return Err(InsuranceError::InvalidRiskAssessment);
        }

        // Base premium rate (0.5%)
        let base_rate = 50u32;

        // Risk adjustment (higher risk = higher premium)
        let risk_adjustment = (risk_score as u32 * 2) / 100; // 0-2% additional

        // Pool adjustment based on pool health
        let pool_health_factor = if pool.total_claims_paid > 0 {
            let claims_ratio = (pool.total_claims_paid * 10000) / pool.total_premiums_collected;
            if claims_ratio > 8000 {
                // >80% claims ratio
                50u32 // 0.5% additional
            } else if claims_ratio > 6000 {
                // >60% claims ratio
                25u32 // 0.25% additional
            } else {
                0u32
            }
        } else {
            0u32
        };

        let premium_rate = base_rate
            .saturating_add(risk_adjustment)
            .saturating_add(pool_health_factor);

        Ok(premium_rate)
    }

    /// Calculate premium amount
    fn calculate_premium_amount(
        coverage_amount: i128,
        premium_rate: u32,
        frequency: u64,
    ) -> Result<i128, InsuranceError> {
        if coverage_amount <= 0 || premium_rate == 0 || frequency == 0 {
            return Err(InsuranceError::InvalidPremiumCalculation);
        }

        // Convert frequency to annual basis (assuming frequency is in seconds)
        let annual_frequency = 31536000u64; // 365 days in seconds
        let frequency_factor = annual_frequency / frequency;

        // Calculate annual premium
        let annual_premium = (coverage_amount * premium_rate as i128) / 10000;

        // Calculate periodic premium
        let periodic_premium = annual_premium / frequency_factor as i128;

        Ok(periodic_premium)
    }

    /// Get insurance policy for an employee
    pub fn get_insurance_policy(env: &Env, employee: &Address) -> Option<InsurancePolicy> {
        let storage = env.storage().persistent();
        storage.get(&InsuranceDataKey::InsurancePolicy(employee.clone()))
    }

    /// Get insurance claim by ID
    pub fn get_insurance_claim(env: &Env, claim_id: u64) -> Option<InsuranceClaim> {
        let storage = env.storage().persistent();
        storage.get(&InsuranceDataKey::InsuranceClaim(claim_id))
    }

    /// Get guarantee by ID
    pub fn get_guarantee(env: &Env, guarantee_id: u64) -> Option<Guarantee> {
        let storage = env.storage().persistent();
        storage.get(&InsuranceDataKey::Guarantee(guarantee_id))
    }

    /// Get employer guarantees
    pub fn get_employer_guarantees(env: &Env, employer: &Address) -> Vec<u64> {
        let storage = env.storage().persistent();
        storage
            .get(&InsuranceDataKey::EmployerGuarantees(employer.clone()))
            .unwrap_or_else(|| Vec::new(env))
    }

    /// Get insurance settings
    pub fn get_insurance_settings(env: &Env) -> InsuranceSettings {
        let storage = env.storage().persistent();
        storage
            .get(&InsuranceDataKey::InsuranceSettings)
            .unwrap_or(InsuranceSettings {
                default_premium_rate: 50,     // 0.5% default
                max_risk_score: 100,          // Maximum risk score
                min_premium_frequency: 86400, // 1 day minimum
                claim_processing_fee: 25,     // 0.25% processing fee
                max_claim_amount: 100000,     // Maximum claim amount
                claim_approval_threshold: 2,  // Minimum approvals needed
                insurance_enabled: true,      // Insurance system enabled
            })
    }

    /// Set insurance settings (admin function)
    pub fn set_insurance_settings(
        env: &Env,
        settings: InsuranceSettings,
    ) -> Result<(), InsuranceError> {
        let storage = env.storage().persistent();
        storage.set(&InsuranceDataKey::InsuranceSettings, &settings);
        Ok(())
    }
}
