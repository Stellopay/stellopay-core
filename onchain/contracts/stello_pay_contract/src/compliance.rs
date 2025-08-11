use soroban_sdk::{
    contracttype, symbol_short, Address, Env, Symbol, String, Vec, Map, 
    contracterror, contractimpl, contract
};

use crate::storage::DataKey;
use crate::events::*;

//-----------------------------------------------------------------------------
// Compliance Errors
//-----------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ComplianceError {
    /// Jurisdiction not supported
    UnsupportedJurisdiction = 1,
    /// Compliance rule validation failed
    ComplianceRuleViolation = 2,
    /// Regulatory reporting failed
    ReportingFailed = 3,
    /// Audit trail operation failed
    AuditTrailError = 4,
    /// Compliance monitoring threshold exceeded
    MonitoringThresholdExceeded = 5,
    /// Invalid compliance configuration
    InvalidComplianceConfig = 6,
    /// Unauthorized compliance operation
    UnauthorizedComplianceOp = 7,
    /// Compliance upgrade failed
    ComplianceUpgradeFailed = 8,
}

//-----------------------------------------------------------------------------
// Compliance Data Structures
//-----------------------------------------------------------------------------

/// Supported jurisdictions with their specific compliance rules
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum Jurisdiction {
    US,           // United States
    EU,           // European Union
    UK,           // United Kingdom
    CA,           // Canada
    AU,           // Australia
    SG,           // Singapore
    JP,           // Japan
    IN,           // India
    BR,           // Brazil
    MX,           // Mexico
    Custom(String), // Custom jurisdiction with specific rules
}

/// Compliance rule types
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ComplianceRuleType {
    MinimumWage,
    MaximumHours,
    OvertimeRate,
    TaxWithholding,
    SocialSecurity,
    UnemploymentInsurance,
    WorkersCompensation,
    HealthInsurance,
    PensionContribution,
    LeaveEntitlement,
    Custom(String),
}

/// Compliance rule structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ComplianceRule {
    pub rule_type: ComplianceRuleType,
    pub jurisdiction: Jurisdiction,
    pub min_value: i128,
    pub max_value: Option<i128>,
    pub required: bool,
    pub description: String,
    pub effective_date: u64,
    pub expiry_date: Option<u64>,
}

/// Compliance validation result
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ComplianceValidation {
    pub is_compliant: bool,
    pub violations: Vec<ComplianceViolation>,
    pub warnings: Vec<String>,
    pub timestamp: u64,
}

/// Compliance violation details
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ComplianceViolation {
    pub rule_type: ComplianceRuleType,
    pub jurisdiction: Jurisdiction,
    pub violation_type: String,
    pub severity: ViolationSeverity,
    pub description: String,
    pub timestamp: u64,
}

/// Violation severity levels
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ViolationSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Regulatory report structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct RegulatoryReport {
    pub report_id: String,
    pub jurisdiction: Jurisdiction,
    pub report_type: ReportType,
    pub period_start: u64,
    pub period_end: u64,
    pub data: Map<String, String>,
    pub submitted_at: u64,
    pub status: ReportStatus,
}

/// Report types
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ReportType {
    PayrollTax,
    EmploymentTax,
    SocialSecurity,
    Unemployment,
    WorkersComp,
    HealthInsurance,
    Pension,
    Custom(String),
}

/// Report status
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ReportStatus {
    Draft,
    Submitted,
    Accepted,
    Rejected,
    Amended,
}

/// Compliance monitoring metrics
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ComplianceMetrics {
    pub jurisdiction: Jurisdiction,
    pub total_employees: u32,
    pub total_payroll_amount: i128,
    pub compliance_score: u32, // 0-100
    pub violations_count: u32,
    pub last_audit_date: u64,
    pub next_audit_date: u64,
}

/// Audit trail entry
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AuditEntry {
    pub entry_id: String,
    pub action: String,
    pub actor: Address,
    pub target: Option<Address>,
    pub details: Map<String, String>,
    pub timestamp: u64,
    pub block_number: u32,
    pub transaction_hash: String,
}

/// Compliance configuration for a jurisdiction
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct JurisdictionConfig {
    pub jurisdiction: Jurisdiction,
    pub rules: Vec<ComplianceRule>,
    pub reporting_frequency: u64, // in seconds
    pub audit_frequency: u64,     // in seconds
    pub enabled: bool,
    pub last_updated: u64,
}

/// Global compliance settings
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ComplianceSettings {
    pub enabled_jurisdictions: Vec<Jurisdiction>,
    pub audit_trail_enabled: bool,
    pub monitoring_enabled: bool,
    pub reporting_enabled: bool,
    pub compliance_officer: Option<Address>,
    pub last_updated: u64,
}

//-----------------------------------------------------------------------------
// Compliance System Implementation
//-----------------------------------------------------------------------------

#[contract]
pub struct ComplianceSystem;

#[contractimpl]
impl ComplianceSystem {
    //-----------------------------------------------------------------------------
    // Jurisdiction Management
    //-----------------------------------------------------------------------------

    /// Add or update jurisdiction configuration
    pub fn set_jurisdiction_config(
        env: Env,
        caller: Address,
        config: JurisdictionConfig,
    ) -> Result<(), ComplianceError> {
        caller.require_auth();
        Self::require_compliance_authorized(&env, &caller)?;

        let storage = env.storage().persistent();
        let jurisdiction_hash = Self::hash_jurisdiction(&env, &config.jurisdiction);
        let key = DataKey::JurisdictionConfig(jurisdiction_hash);
        storage.set(&key, &config);

        // Add to audit trail
        Self::add_audit_entry(
            &env,
            "jurisdiction_config_updated",
            &caller,
            None,
            &Map::new(&env),
        );

        Ok(())
    }

    /// Get jurisdiction configuration
    pub fn get_jurisdiction_config(
        env: Env,
        jurisdiction: Jurisdiction,
    ) -> Option<JurisdictionConfig> {
        let storage = env.storage().persistent();
        let key = DataKey::JurisdictionConfig(jurisdiction);
        storage.get(&key)
    }

    /// Enable or disable jurisdiction
    pub fn toggle_jurisdiction(
        env: Env,
        caller: Address,
        jurisdiction: Jurisdiction,
        enabled: bool,
    ) -> Result<(), ComplianceError> {
        caller.require_auth();
        Self::require_compliance_authorized(&env, &caller)?;

        let storage = env.storage().persistent();
        let key = DataKey::JurisdictionConfig(jurisdiction.clone());
        
        if let Some(mut config) = storage.get(&key) {
            config.enabled = enabled;
            config.last_updated = env.ledger().timestamp();
            storage.set(&key, &config);

            // Add to audit trail
            let mut details = Map::new(&env);
            details.set(&String::from_slice(&env, "enabled"), &enabled.to_string());
            Self::add_audit_entry(&env, "jurisdiction_toggled", &caller, None, &details);
        }

        Ok(())
    }

    //-----------------------------------------------------------------------------
    // Compliance Validation
    //-----------------------------------------------------------------------------

    /// Validate payroll compliance for a specific jurisdiction
    pub fn validate_payroll_compliance(
        env: Env,
        employer: Address,
        employee: Address,
        jurisdiction: Jurisdiction,
        payroll_amount: i128,
        hours_worked: Option<u32>,
    ) -> ComplianceValidation {
        let storage = env.storage().persistent();
        let key = DataKey::JurisdictionConfig(jurisdiction.clone());
        
        let config = match storage.get(&key) {
            Some(config) if config.enabled => config,
            _ => {
                return ComplianceValidation {
                    is_compliant: false,
                    violations: Vec::new(&env),
                    warnings: vec![String::from_slice(&env, "Jurisdiction not configured or disabled")],
                    timestamp: env.ledger().timestamp(),
                };
            }
        };

        let mut violations = Vec::new(&env);
        let mut warnings = Vec::new(&env);
        let current_time = env.ledger().timestamp();

        // Validate each rule
        for rule in config.rules.iter() {
            if rule.effective_date <= current_time && 
               (rule.expiry_date.is_none() || rule.expiry_date.unwrap() > current_time) {
                
                match Self::validate_rule(&env, &rule, payroll_amount, hours_worked) {
                    Ok(()) => {}, // Rule passed
                    Err(violation) => {
                        if rule.required {
                            violations.push_back(violation);
                        } else {
                            warnings.push_back(violation.description);
                        }
                    }
                }
            }
        }

        ComplianceValidation {
            is_compliant: violations.len() == 0,
            violations,
            warnings,
            timestamp: current_time,
        }
    }

    /// Validate a specific compliance rule
    fn validate_rule(
        env: &Env,
        rule: &ComplianceRule,
        payroll_amount: i128,
        hours_worked: Option<u32>,
    ) -> Result<(), ComplianceViolation> {
        let current_time = env.ledger().timestamp();

        match &rule.rule_type {
            ComplianceRuleType::MinimumWage => {
                if payroll_amount < rule.min_value {
                    return Err(ComplianceViolation {
                        rule_type: rule.rule_type.clone(),
                        jurisdiction: rule.jurisdiction.clone(),
                        violation_type: String::from_slice(env, "below_minimum_wage"),
                        severity: ViolationSeverity::High,
                        description: String::from_slice(env, "Payroll amount below minimum wage requirement"),
                        timestamp: current_time,
                    });
                }
            },
            ComplianceRuleType::MaximumHours => {
                if let Some(hours) = hours_worked {
                    if let Some(max_hours) = rule.max_value {
                        if hours > max_hours as u32 {
                            return Err(ComplianceViolation {
                                rule_type: rule.rule_type.clone(),
                                jurisdiction: rule.jurisdiction.clone(),
                                violation_type: String::from_slice(env, "exceeds_maximum_hours"),
                                severity: ViolationSeverity::Medium,
                                description: String::from_slice(env, "Hours worked exceed maximum allowed"),
                                timestamp: current_time,
                            });
                        }
                    }
                }
            },
            ComplianceRuleType::OvertimeRate => {
                // Overtime validation logic would go here
                // This is a simplified implementation
            },
            _ => {
                // Handle other rule types
            }
        }

        Ok(())
    }

    //-----------------------------------------------------------------------------
    // Regulatory Reporting
    //-----------------------------------------------------------------------------

    /// Generate regulatory report
    pub fn generate_regulatory_report(
        env: Env,
        caller: Address,
        jurisdiction: Jurisdiction,
        report_type: ReportType,
        period_start: u64,
        period_end: u64,
    ) -> Result<RegulatoryReport, ComplianceError> {
        caller.require_auth();
        Self::require_compliance_authorized(&env, &caller)?;

        let current_time = env.ledger().timestamp();
        let report_id = Self::generate_report_id(&env, &jurisdiction, &report_type, period_start);

        let report_data = Self::collect_report_data(&env, &jurisdiction, &report_type, period_start, period_end)?;

        let report = RegulatoryReport {
            report_id: report_id.clone(),
            jurisdiction: jurisdiction.clone(),
            report_type: report_type.clone(),
            period_start,
            period_end,
            data: report_data,
            submitted_at: current_time,
            status: ReportStatus::Draft,
        };

        // Store report
        let storage = env.storage().persistent();
        let key = DataKey::RegulatoryReport(report_id.clone());
        storage.set(&key, &report);

        // Add to audit trail
        let mut details = Map::new(&env);
        details.set(&String::from_slice(&env, "report_id"), &report_id);
        details.set(&String::from_slice(&env, "report_type"), &format!("{:?}", report_type));
        Self::add_audit_entry(&env, "regulatory_report_generated", &caller, None, &details);

        Ok(report)
    }

    /// Submit regulatory report
    pub fn submit_regulatory_report(
        env: Env,
        caller: Address,
        report_id: String,
    ) -> Result<(), ComplianceError> {
        caller.require_auth();
        Self::require_compliance_authorized(&env, &caller)?;

        let storage = env.storage().persistent();
        let key = DataKey::RegulatoryReport(report_id.clone());
        
        if let Some(mut report) = storage.get(&key) {
            report.status = ReportStatus::Submitted;
            storage.set(&key, &report);

            // Add to audit trail
            let mut details = Map::new(&env);
            details.set(&String::from_slice(&env, "report_id"), &report_id);
            Self::add_audit_entry(&env, "regulatory_report_submitted", &caller, None, &details);
        }

        Ok(())
    }

    /// Get regulatory report by ID
    pub fn get_regulatory_report(env: Env, report_id: String) -> Option<RegulatoryReport> {
        let storage = env.storage().persistent();
        let key = DataKey::RegulatoryReport(report_id);
        storage.get(&key)
    }

    //-----------------------------------------------------------------------------
    // Compliance Monitoring
    //-----------------------------------------------------------------------------

    /// Update compliance metrics
    pub fn update_compliance_metrics(
        env: Env,
        jurisdiction: Jurisdiction,
        total_employees: u32,
        total_payroll_amount: i128,
        violations_count: u32,
    ) -> Result<ComplianceMetrics, ComplianceError> {
        let current_time = env.ledger().timestamp();
        
        // Calculate compliance score (0-100)
        let compliance_score = if total_employees > 0 {
            let violation_rate = (violations_count as f64) / (total_employees as f64);
            let score = (1.0 - violation_rate) * 100.0;
            score as u32
        } else {
            100
        };

        let metrics = ComplianceMetrics {
            jurisdiction: jurisdiction.clone(),
            total_employees,
            total_payroll_amount,
            compliance_score,
            violations_count,
            last_audit_date: current_time,
            next_audit_date: current_time + 86400 * 30, // 30 days from now
        };

        // Store metrics
        let storage = env.storage().persistent();
        let key = DataKey::ComplianceMetrics(jurisdiction.clone());
        storage.set(&key, &metrics);

        // Check if monitoring thresholds are exceeded
        Self::check_monitoring_thresholds(&env, &metrics)?;

        Ok(metrics)
    }

    /// Get compliance metrics for a jurisdiction
    pub fn get_compliance_metrics(env: Env, jurisdiction: Jurisdiction) -> Option<ComplianceMetrics> {
        let storage = env.storage().persistent();
        let key = DataKey::ComplianceMetrics(jurisdiction);
        storage.get(&key)
    }

    /// Check monitoring thresholds and trigger alerts
    fn check_monitoring_thresholds(
        env: &Env,
        metrics: &ComplianceMetrics,
    ) -> Result<(), ComplianceError> {
        // Define thresholds (these could be configurable)
        const LOW_COMPLIANCE_THRESHOLD: u32 = 70;
        const HIGH_VIOLATION_THRESHOLD: u32 = 10;

        if metrics.compliance_score < LOW_COMPLIANCE_THRESHOLD {
            // Emit low compliance alert
            env.events().publish(
                (symbol_short!("low_compliance"),),
                (metrics.jurisdiction.clone(), metrics.compliance_score),
            );
        }

        if metrics.violations_count > HIGH_VIOLATION_THRESHOLD {
            // Emit high violation alert
            env.events().publish(
                (symbol_short!("high_violations"),),
                (metrics.jurisdiction.clone(), metrics.violations_count),
            );
        }

        Ok(())
    }

    //-----------------------------------------------------------------------------
    // Audit Trail
    //-----------------------------------------------------------------------------

    /// Add entry to audit trail
    pub fn add_audit_entry(
        env: &Env,
        action: &str,
        actor: &Address,
        target: Option<Address>,
        details: &Map<String, String>,
    ) {
        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();
        let entry_id = Self::generate_audit_entry_id(env, actor, current_time);

        let entry = AuditEntry {
            entry_id: entry_id.clone(),
            action: String::from_slice(env, action),
            actor: actor.clone(),
            target,
            details: details.clone(),
            timestamp: current_time,
            block_number: env.ledger().sequence(),
            transaction_hash: String::from_slice(env, "tx_hash_placeholder"), // In real implementation, get from env
        };

        // Store audit entry
        let key = DataKey::AuditEntry(entry_id);
        storage.set(&key, &entry);

        // Add to audit index for efficient querying
        let index_key = DataKey::AuditIndex(actor.clone());
        let mut audit_entries: Vec<String> = storage.get(&index_key).unwrap_or(Vec::new(env));
        audit_entries.push_back(entry_id);
        storage.set(&index_key, &audit_entries);
    }

    /// Get audit entries for an address
    pub fn get_audit_entries(env: Env, address: Address) -> Vec<AuditEntry> {
        let storage = env.storage().persistent();
        let index_key = DataKey::AuditIndex(address);
        
        if let Some(entry_ids) = storage.get::<DataKey, Vec<String>>(&index_key) {
            let mut entries = Vec::new(&env);
            for entry_id in entry_ids.iter() {
                let key = DataKey::AuditEntry(entry_id);
                if let Some(entry) = storage.get(&key) {
                    entries.push_back(entry);
                }
            }
            entries
        } else {
            Vec::new(&env)
        }
    }

    /// Get audit entry by ID
    pub fn get_audit_entry(env: Env, entry_id: String) -> Option<AuditEntry> {
        let storage = env.storage().persistent();
        let key = DataKey::AuditEntry(entry_id);
        storage.get(&key)
    }

    //-----------------------------------------------------------------------------
    // Compliance Settings Management
    //-----------------------------------------------------------------------------

    /// Set global compliance settings
    pub fn set_compliance_settings(
        env: Env,
        caller: Address,
        settings: ComplianceSettings,
    ) -> Result<(), ComplianceError> {
        caller.require_auth();
        Self::require_compliance_authorized(&env, &caller)?;

        let storage = env.storage().persistent();
        storage.set(&DataKey::ComplianceSettings, &settings);

        // Add to audit trail
        let mut details = Map::new(&env);
        details.set(&String::from_slice(&env, "audit_trail_enabled"), &settings.audit_trail_enabled.to_string());
        details.set(&String::from_slice(&env, "monitoring_enabled"), &settings.monitoring_enabled.to_string());
        Self::add_audit_entry(&env, "compliance_settings_updated", &caller, None, &details);

        Ok(())
    }

    /// Get global compliance settings
    pub fn get_compliance_settings(env: Env) -> Option<ComplianceSettings> {
        let storage = env.storage().persistent();
        storage.get(&DataKey::ComplianceSettings)
    }

    //-----------------------------------------------------------------------------
    // Helper Functions
    //-----------------------------------------------------------------------------

    /// Check if caller is authorized for compliance operations
    fn require_compliance_authorized(env: &Env, caller: &Address) -> Result<(), ComplianceError> {
        let storage = env.storage().persistent();
        
        // Check if caller is contract owner
        if let Some(owner) = storage.get::<DataKey, Address>(&DataKey::Owner) {
            if caller == &owner {
                return Ok(());
            }
        }

        // Check if caller is compliance officer
        if let Some(settings) = storage.get::<DataKey, ComplianceSettings>(&DataKey::ComplianceSettings) {
            if let Some(officer) = settings.compliance_officer {
                if caller == &officer {
                    return Ok(());
                }
            }
        }

        Err(ComplianceError::UnauthorizedComplianceOp)
    }

    /// Generate unique report ID
    fn generate_report_id(
        env: &Env,
        jurisdiction: &Jurisdiction,
        report_type: &ReportType,
        period_start: u64,
    ) -> String {
        let timestamp = env.ledger().timestamp();
        format!("{}_{:?}_{}_{}", jurisdiction, report_type, period_start, timestamp)
    }

    /// Generate unique audit entry ID
    fn generate_audit_entry_id(env: &Env, actor: &Address, timestamp: u64) -> String {
        format!("audit_{}_{}_{}", actor, timestamp, env.ledger().sequence())
    }

    /// Collect data for regulatory report
    fn collect_report_data(
        env: &Env,
        jurisdiction: &Jurisdiction,
        report_type: &ReportType,
        period_start: u64,
        period_end: u64,
    ) -> Result<Map<String, String>, ComplianceError> {
        let mut data = Map::new(env);
        
        // This is a simplified implementation
        // In a real system, this would collect actual payroll and compliance data
        data.set(&String::from_slice(env, "period_start"), &period_start.to_string());
        data.set(&String::from_slice(env, "period_end"), &period_end.to_string());
        data.set(&String::from_slice(env, "jurisdiction"), &format!("{:?}", jurisdiction));
        data.set(&String::from_slice(env, "report_type"), &format!("{:?}", report_type));
        data.set(&String::from_slice(env, "generated_at"), &env.ledger().timestamp().to_string());

        Ok(data)
    }

    //-----------------------------------------------------------------------------
    // Compliance Upgrade Mechanism
    //-----------------------------------------------------------------------------

    /// Upgrade compliance rules for a jurisdiction
    pub fn upgrade_compliance_rules(
        env: Env,
        caller: Address,
        jurisdiction: Jurisdiction,
        new_rules: Vec<ComplianceRule>,
    ) -> Result<(), ComplianceError> {
        caller.require_auth();
        Self::require_compliance_authorized(&env, &caller)?;

        let storage = env.storage().persistent();
        let key = DataKey::JurisdictionConfig(jurisdiction.clone());
        
        if let Some(mut config) = storage.get(&key) {
            config.rules = new_rules;
            config.last_updated = env.ledger().timestamp();
            storage.set(&key, &config);

            // Add to audit trail
            let mut details = Map::new(&env);
            details.set(&String::from_slice(&env, "jurisdiction"), &format!("{:?}", jurisdiction));
            Self::add_audit_entry(&env, "compliance_rules_upgraded", &caller, None, &details);
        } else {
            return Err(ComplianceError::ComplianceUpgradeFailed);
        }

        Ok(())
    }

    /// Get all supported jurisdictions
    pub fn get_supported_jurisdictions(env: Env) -> Vec<Jurisdiction> {
        let storage = env.storage().persistent();
        let mut jurisdictions = Vec::new(&env);
        
        // This would iterate through all jurisdiction configs
        // For now, return a default list
        jurisdictions.push_back(Jurisdiction::US);
        jurisdictions.push_back(Jurisdiction::EU);
        jurisdictions.push_back(Jurisdiction::UK);
        
        jurisdictions
    }
} 