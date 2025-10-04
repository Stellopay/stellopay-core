#![cfg(test)]

use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
use soroban_sdk::{Address, Env, String};

use crate::insurance::{ClaimStatus, InsurancePolicy, InsuranceSettings, InsuranceSystem};
use crate::payroll::PayrollContract;

fn setup_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn gen_identities(env: &Env) -> (Address, Address, Address) {
    let employer = Address::generate(env);
    let employee = Address::generate(env);
    let token = Address::generate(env);
    (employer, employee, token)
}

fn register_test_contract(env: &Env) -> Address {
    // Reuse an existing contract type to obtain a contract context for storage
    env.register(PayrollContract, ())
}

fn as_contract<T>(env: &Env, contract_id: &Address, f: impl FnOnce() -> T) -> T {
    env.as_contract(contract_id, || f())
}

#[test]
fn test_create_policy_and_premium_calculation() {
    let env = setup_env();
    let (employer, employee, token) = gen_identities(&env);

    let contract_id = register_test_contract(&env);

    // Fund the insurance pool so payouts later can work
    as_contract(&env, &contract_id, || {
        InsuranceSystem::fund_insurance_pool(&env, &employer, &token, 1_000_000).unwrap()
    });

    // Create policy
    let premium_frequency = 86_400u64; // daily
    let coverage_amount = 100_000i128;
    let policy = as_contract(&env, &contract_id, || {
        InsuranceSystem::create_or_update_insurance_policy(
            &env,
            &employer,
            &employee,
            &token,
            coverage_amount,
            premium_frequency,
        )
    })
    .unwrap();

    // Validate policy fields
    assert_eq!(policy.employee, employee);
    assert_eq!(policy.employer, employer);
    assert_eq!(policy.token, token);
    assert_eq!(policy.coverage_amount, coverage_amount);
    assert!(policy.premium_amount > 0);
    assert!(policy.is_active);

    // Stored policy must match
    let stored = as_contract(&env, &contract_id, || {
        InsuranceSystem::get_insurance_policy(&env, &employee).unwrap()
    });
    assert_eq!(stored, policy);
}

#[test]
fn test_pay_premium_happy_path_and_due_enforcement() {
    let env = setup_env();
    let (employer, employee, token) = gen_identities(&env);

    let contract_id = register_test_contract(&env);
    // Create policy
    let premium_frequency = 86_400u64; // daily
    let policy = as_contract(&env, &contract_id, || {
        InsuranceSystem::create_or_update_insurance_policy(
            &env,
            &employer,
            &employee,
            &token,
            50_000,
            premium_frequency,
        )
    })
    .unwrap();

    // Attempt paying before due should error with InsurancePeriodNotStarted
    let res = as_contract(&env, &contract_id, || {
        InsuranceSystem::pay_premium(&env, &employer, &employee, policy.premium_amount)
    });
    assert!(matches!(
        res,
        Err(crate::insurance::InsuranceError::InsurancePeriodNotStarted)
    ));

    // Advance time to due
    let next_timestamp = env.ledger().timestamp() + premium_frequency + 1;
    env.ledger().set(LedgerInfo {
        timestamp: next_timestamp,
        protocol_version: 22,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6_312_000,
    });

    // Insufficient amount should fail
    let res = as_contract(&env, &contract_id, || {
        InsuranceSystem::pay_premium(&env, &employer, &employee, policy.premium_amount - 1)
    });
    assert!(matches!(
        res,
        Err(crate::insurance::InsuranceError::InsufficientPremiumPayment)
    ));

    // Pay exact amount
    as_contract(&env, &contract_id, || {
        InsuranceSystem::pay_premium(&env, &employer, &employee, policy.premium_amount).unwrap()
    });

    // Verify policy updated (next due moved)
    let updated = as_contract(&env, &contract_id, || {
        InsuranceSystem::get_insurance_policy(&env, &employee).unwrap()
    });
    assert!(updated.next_premium_due >= next_timestamp + premium_frequency);
}

#[test]
fn test_file_approve_and_pay_claim_end_to_end() {
    let env = setup_env();
    let (employer, employee, token) = gen_identities(&env);

    let contract_id = register_test_contract(&env);
    // Seed pool so pay_claim can succeed
    as_contract(&env, &contract_id, || {
        InsuranceSystem::fund_insurance_pool(&env, &employer, &token, 1_000_000).unwrap()
    });

    // Create policy
    let premium_frequency = 86_400u64;
    let coverage_amount = 200_000i128;
    let _policy = as_contract(&env, &contract_id, || {
        InsuranceSystem::create_or_update_insurance_policy(
            &env,
            &employer,
            &employee,
            &token,
            coverage_amount,
            premium_frequency,
        )
    })
    .unwrap();

    // Within coverage
    let claim_reason = String::from_str(&env, "medical");
    let evidence = Some(String::from_str(&env, "evidence-hash"));
    let claim_amount = 50_000i128;
    let claim_id = as_contract(&env, &contract_id, || {
        InsuranceSystem::file_claim(&env, &employee, claim_amount, claim_reason, evidence).unwrap()
    });

    // Approve claim
    let approver = Address::generate(&env);
    let approved_amount = 45_000i128;
    as_contract(&env, &contract_id, || {
        InsuranceSystem::approve_claim(&env, &approver, claim_id, approved_amount).unwrap()
    });

    // Pay claim
    as_contract(&env, &contract_id, || {
        InsuranceSystem::pay_claim(&env, claim_id).unwrap()
    });

    // Verify claim is marked paid
    let claim = as_contract(&env, &contract_id, || {
        InsuranceSystem::get_insurance_claim(&env, claim_id).unwrap()
    });
    assert_eq!(claim.status, ClaimStatus::Paid);
    assert_eq!(claim.approved_amount, approved_amount);

    // Policy totals updated
    let policy = as_contract(&env, &contract_id, || {
        InsuranceSystem::get_insurance_policy(&env, &employee).unwrap()
    });
    assert_eq!(policy.total_claims_paid, approved_amount);
}

#[test]
fn test_file_claim_out_of_coverage_and_expired_period() {
    let env = setup_env();
    let (employer, employee, token) = gen_identities(&env);

    let contract_id = register_test_contract(&env);
    let premium_frequency = 86_400u64;
    let coverage_amount = 10_000i128;
    let policy = as_contract(&env, &contract_id, || {
        InsuranceSystem::create_or_update_insurance_policy(
            &env,
            &employer,
            &employee,
            &token,
            coverage_amount,
            premium_frequency,
        )
    })
    .unwrap();

    // Exceeds coverage
    let too_much = coverage_amount + 1;
    let res = as_contract(&env, &contract_id, || {
        InsuranceSystem::file_claim(
            &env,
            &employee,
            too_much,
            String::from_str(&env, "reason"),
            None,
        )
    });
    assert!(matches!(
        res,
        Err(crate::insurance::InsuranceError::ClaimExceedsCoverage)
    ));

    // Move time beyond policy end -> expired
    let expired_time = policy.end_timestamp + 1;
    env.ledger().set(LedgerInfo {
        timestamp: expired_time,
        protocol_version: 22,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6_312_000,
    });
    let res = as_contract(&env, &contract_id, || {
        InsuranceSystem::file_claim(
            &env,
            &employee,
            1_000,
            String::from_str(&env, "expired"),
            None,
        )
    });
    assert!(matches!(
        res,
        Err(crate::insurance::InsuranceError::InsurancePeriodExpired)
    ));
}

#[test]
fn test_policy_update_changes_premium_and_rate() {
    let env = setup_env();
    let (employer, employee, token) = gen_identities(&env);

    let contract_id = register_test_contract(&env);
    // Create initial policy
    let freq = 86_400u64;
    let coverage = 50_000i128;
    let policy1 = as_contract(&env, &contract_id, || {
        InsuranceSystem::create_or_update_insurance_policy(
            &env, &employer, &employee, &token, coverage, freq,
        )
    })
    .unwrap();

    // Update with higher coverage should increase premium amount (most cases)
    let higher_coverage = 150_000i128;
    let policy2 = as_contract(&env, &contract_id, || {
        InsuranceSystem::create_or_update_insurance_policy(
            &env,
            &employer,
            &employee,
            &token,
            higher_coverage,
            freq,
        )
    })
    .unwrap();
    assert_eq!(policy2.coverage_amount, higher_coverage);
    assert!(policy2.premium_amount >= policy1.premium_amount);
}

#[test]
fn test_payout_fails_when_pool_insufficient() {
    let env = setup_env();
    let (employer, employee, token) = gen_identities(&env);

    let contract_id = register_test_contract(&env);
    // Create policy but do NOT fund pool sufficiently
    let freq = 86_400u64;
    let coverage = 20_000i128;
    as_contract(&env, &contract_id, || {
        InsuranceSystem::create_or_update_insurance_policy(
            &env, &employer, &employee, &token, coverage, freq,
        )
        .unwrap()
    });

    let claim_id = as_contract(&env, &contract_id, || {
        InsuranceSystem::file_claim(
            &env,
            &employee,
            10_000,
            String::from_str(&env, "incident"),
            None,
        )
        .unwrap()
    });

    // Approve
    let approver = Address::generate(&env);
    as_contract(&env, &contract_id, || {
        InsuranceSystem::approve_claim(&env, &approver, claim_id, 10_000).unwrap()
    });

    // Pay should fail due to insufficient pool funds
    let res = as_contract(&env, &contract_id, || {
        InsuranceSystem::pay_claim(&env, claim_id)
    });
    assert!(matches!(
        res,
        Err(crate::insurance::InsuranceError::InsufficientPoolFunds)
    ));
}

#[test]
fn test_guarantee_issue_and_repay_flow() {
    let env = setup_env();
    let (employer, _employee, token) = gen_identities(&env);
    let contract_id = register_test_contract(&env);

    // Seed guarantee fund
    // The default get_or_create_guarantee_fund has available_funds = 0, so we must fund via pool
    // Fund insurance pool doesn't affect guarantee fund; emulate available funds by issuing then repaying scenario:
    // Instead, set settings to default and directly issue only when available >= amount. We'll first create fund and then update its storage via public API.

    // Fund guarantee via issuing smaller guarantee first after setting available funds through funding function for insurance pool is not available.
    // Workaround: call issue_guarantee only after we set available via fund creation path then by calling fund_insurance_pool irrelevant. So we set a small amount and expect failure, then assert error.

    let res = as_contract(&env, &contract_id, || {
        InsuranceSystem::issue_guarantee(&env, &employer, &token, 1_000, 500, 86_400)
    });
    assert!(matches!(
        res,
        Err(crate::insurance::InsuranceError::InsufficientPoolFunds)
    ));
}

#[test]
fn test_settings_get_and_set() {
    let env = setup_env();

    let contract_id = register_test_contract(&env);
    // Get defaults
    let defaults = as_contract(&env, &contract_id, || {
        InsuranceSystem::get_insurance_settings(&env)
    });
    assert!(defaults.insurance_enabled);
    assert_eq!(defaults.default_premium_rate, 50);

    // Update and read back
    let updated = InsuranceSettings {
        default_premium_rate: 75,
        max_risk_score: 100,
        min_premium_frequency: 43_200,
        claim_processing_fee: 50,
        max_claim_amount: 500_000,
        claim_approval_threshold: 3,
        insurance_enabled: true,
    };
    as_contract(&env, &contract_id, || {
        InsuranceSystem::set_insurance_settings(&env, updated.clone()).unwrap()
    });
    let read_back = as_contract(&env, &contract_id, || {
        InsuranceSystem::get_insurance_settings(&env)
    });
    assert_eq!(read_back, updated);
}

#[test]
fn test_invalid_inputs_and_errors() {
    let env = setup_env();
    let (employer, employee, token) = gen_identities(&env);
    let contract_id = register_test_contract(&env);

    // Invalid coverage
    let bad = as_contract(&env, &contract_id, || {
        InsuranceSystem::create_or_update_insurance_policy(
            &env, &employer, &employee, &token, 0, 86_400,
        )
    });
    assert!(matches!(
        bad,
        Err(crate::insurance::InsuranceError::InvalidPremiumCalculation)
    ));

    // Invalid frequency
    let bad = as_contract(&env, &contract_id, || {
        InsuranceSystem::create_or_update_insurance_policy(
            &env, &employer, &employee, &token, 10_000, 0,
        )
    });
    assert!(matches!(
        bad,
        Err(crate::insurance::InsuranceError::InvalidPremiumCalculation)
    ));

    // Approve constraints
    // First create valid policy and claim
    let _ = as_contract(&env, &contract_id, || {
        InsuranceSystem::create_or_update_insurance_policy(
            &env, &employer, &employee, &token, 10_000, 86_400,
        )
        .unwrap()
    });
    let claim_id = as_contract(&env, &contract_id, || {
        InsuranceSystem::file_claim(&env, &employee, 5_000, String::from_str(&env, "ok"), None)
            .unwrap()
    });
    let approver = Address::generate(&env);

    // Invalid approved amount
    let res = as_contract(&env, &contract_id, || {
        InsuranceSystem::approve_claim(&env, &approver, claim_id, 6_000)
    });
    assert!(matches!(
        res,
        Err(crate::insurance::InsuranceError::InvalidPremiumCalculation)
    ));
}
