//! Load-testing scenarios for contract behavior under high transaction volumes.
//! The tests print timing metrics that can be tracked in CI or locally.

#![allow(deprecated)]

use std::time::{Duration, Instant};

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};

use stello_pay_contract::storage::DataKey;
use stello_pay_contract::{PayrollContract, PayrollContractClient};

const ONE_DAY: u64 = 86_400;
const ONE_WEEK: u64 = 604_800;
const SALARY: i128 = 100;

#[derive(Clone, Copy)]
struct WorkloadMetrics {
    transactions: usize,
    duration: Duration,
}

impl WorkloadMetrics {
    fn throughput_tps(self) -> f64 {
        let seconds = self.duration.as_secs_f64();
        if seconds == 0.0 {
            return self.transactions as f64;
        }
        self.transactions as f64 / seconds
    }

    fn latency_per_tx_us(self) -> f64 {
        if self.transactions == 0 {
            return 0.0;
        }
        self.duration.as_micros() as f64 / self.transactions as f64
    }
}

fn env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn addr(env: &Env) -> Address {
    Address::generate(env)
}

fn token(env: &Env) -> Address {
    env.register_stellar_asset_contract_v2(addr(env)).address()
}

fn deploy_payroll(env: &Env) -> (Address, PayrollContractClient<'_>) {
    let id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &id);
    client.initialize(&addr(env));
    (id, client)
}

/// @notice Seeds internal payroll storage so claims can execute under load.
/// @dev This mirrors the data layout required by payroll claim paths.
/// @param env Test environment.
/// @param contract_id Payroll contract id.
/// @param agreement_id Agreement to fund.
/// @param tok Payment token.
/// @param employees Employee tuples `(address, salary)`.
/// @param total_fund Total escrow amount for this agreement.
fn fund_payroll_internal(
    env: &Env,
    contract_id: &Address,
    agreement_id: u128,
    tok: &Address,
    employees: &[(Address, i128)],
    total_fund: i128,
) {
    StellarAssetClient::new(env, tok).mint(contract_id, &total_fund);

    env.as_contract(contract_id, || {
        DataKey::set_agreement_escrow_balance(env, agreement_id, tok, total_fund);
        DataKey::set_agreement_activation_time(env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(env, agreement_id, tok);
        DataKey::set_employee_count(env, agreement_id, employees.len() as u32);

        for (index, (employee, salary)) in employees.iter().enumerate() {
            let idx = index as u32;
            DataKey::set_employee(env, agreement_id, idx, employee);
            DataKey::set_employee_salary(env, agreement_id, idx, *salary);
            DataKey::set_employee_claimed_periods(env, agreement_id, idx, 0);
        }
    });
}

/// @notice Executes a full claim workload and returns timing metrics.
/// @dev Used for degradation profiling between small/medium/large scales.
fn run_claim_workload(agreement_count: u32, employees_per_agreement: u32) -> WorkloadMetrics {
    let env = env();
    let (contract_id, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let tok = token(&env);

    let mut agreement_employees: std::vec::Vec<(u128, std::vec::Vec<Address>)> =
        std::vec::Vec::new();

    for _ in 0..agreement_count {
        let agreement_id = client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);

        let mut employees = std::vec::Vec::new();
        for _ in 0..employees_per_agreement {
            let employee = addr(&env);
            client.add_employee_to_agreement(&agreement_id, &employee, &SALARY);
            employees.push(employee);
        }

        client.activate_agreement(&agreement_id);

        let employee_salary_pairs: std::vec::Vec<(Address, i128)> = employees
            .iter()
            .cloned()
            .map(|employee| (employee, SALARY))
            .collect();

        let total_fund = SALARY * employees_per_agreement as i128 * 2;
        fund_payroll_internal(
            &env,
            &contract_id,
            agreement_id,
            &tok,
            &employee_salary_pairs,
            total_fund,
        );

        agreement_employees.push((agreement_id, employees));
    }

    env.ledger().with_mut(|li| li.timestamp += ONE_DAY);

    let transaction_count = (agreement_count as usize) * (employees_per_agreement as usize);

    let start = Instant::now();
    for (agreement_id, employees) in &agreement_employees {
        for (idx, employee) in employees.iter().enumerate() {
            client.claim_payroll(employee, agreement_id, &(idx as u32));
        }
    }
    let elapsed = start.elapsed();

    WorkloadMetrics {
        transactions: transaction_count,
        duration: elapsed,
    }
}

/// @notice Measures create-agreement throughput at high request volume.
#[test]
fn test_load_high_agreement_creation_rate() {
    let env = env();
    let (_contract_id, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let tok = token(&env);

    let agreements = 500u32;

    let start = Instant::now();
    for _ in 0..agreements {
        let _ = client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    }
    let elapsed = start.elapsed();

    let metrics = WorkloadMetrics {
        transactions: agreements as usize,
        duration: elapsed,
    };

    println!(
        "[load] agreements={} duration_ms={} throughput_tps={:.2} latency_us_per_tx={:.2}",
        agreements,
        metrics.duration.as_millis(),
        metrics.throughput_tps(),
        metrics.latency_per_tx_us()
    );

    assert!(metrics.transactions > 0);
    assert!(metrics.duration.as_nanos() > 0);
}

/// @notice Measures behavior when a single agreement has many employees.
#[test]
fn test_load_large_employee_set_on_single_agreement() {
    let env = env();
    let (_contract_id, client) = deploy_payroll(&env);
    let employer = addr(&env);
    let tok = token(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &tok, &ONE_WEEK);
    let employees = 1000u32;

    let start = Instant::now();
    for _ in 0..employees {
        let employee = addr(&env);
        client.add_employee_to_agreement(&agreement_id, &employee, &SALARY);
    }
    let elapsed = start.elapsed();

    let registered = client.get_agreement_employees(&agreement_id);
    let metrics = WorkloadMetrics {
        transactions: employees as usize,
        duration: elapsed,
    };

    println!(
        "[load] employees={} duration_ms={} throughput_tps={:.2} latency_us_per_tx={:.2}",
        employees,
        metrics.duration.as_millis(),
        metrics.throughput_tps(),
        metrics.latency_per_tx_us()
    );

    assert_eq!(registered.len(), employees);
    assert!(metrics.duration.as_nanos() > 0);
}

/// @notice Measures high transaction claim throughput across many agreements/employees.
#[test]
fn test_load_high_transaction_claim_rate() {
    let metrics = run_claim_workload(12, 10);

    println!(
        "[load] claim_tx={} duration_ms={} throughput_tps={:.2} latency_us_per_tx={:.2}",
        metrics.transactions,
        metrics.duration.as_millis(),
        metrics.throughput_tps(),
        metrics.latency_per_tx_us()
    );

    assert_eq!(metrics.transactions, 120);
    assert!(metrics.duration.as_nanos() > 0);
}

/// @notice Profiles performance degradation by comparing workload scales.
#[test]
fn test_load_performance_degradation_profile() {
    let small = run_claim_workload(3, 5);
    let medium = run_claim_workload(6, 8);
    let large = run_claim_workload(10, 10);

    let small_per_tx = small.latency_per_tx_us();
    let medium_per_tx = medium.latency_per_tx_us();
    let large_per_tx = large.latency_per_tx_us();

    println!(
        "[load] profile small_tx={} small_us_per_tx={:.2} medium_tx={} medium_us_per_tx={:.2} large_tx={} large_us_per_tx={:.2}",
        small.transactions,
        small_per_tx,
        medium.transactions,
        medium_per_tx,
        large.transactions,
        large_per_tx,
    );

    // Larger total workload should take longer in absolute time.
    assert!(large.duration > small.duration);
    assert!(medium.duration > Duration::from_nanos(0));

    // Guardrail against runaway degradation while still allowing growth under load.
    if small_per_tx > 0.0 {
        assert!(large_per_tx < small_per_tx * 50.0);
    }
}
