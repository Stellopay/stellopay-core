//! Instruction-count benchmarks for high-frequency payroll operations.
//!
//! Captures Soroban CPU instruction costs via [`soroban_sdk::testutils::budget`]
//! and compares against committed baselines in `benchmarks/stello_pay_contract_gas.json`.
//! CI fails when measured counts exceed baseline by more than 5%.
//!
//! # Running
//!
//! ```bash
//! cd onchain
//! cargo test -p stello_pay_contract gas_benchmark -- --nocapture
//! ```
//!
//! # Updating baselines
//!
//! After an intentional contract change that increases instruction counts:
//!
//! ```bash
//! UPDATE_GAS_BASELINES=1 cargo test -p stello_pay_contract gas_benchmark -- --nocapture
//! ```

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, EnvTestConfig, Ledger},
    token::StellarAssetClient,
    vec, Address, Env, Vec as SdkVec,
};
use stello_pay_contract::{
    storage::{DataKey, PayrollCreateParams, MAX_BATCH_SIZE},
    PayrollContract, PayrollContractClient,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const ONE_DAY: u64 = 86_400;
const ONE_WEEK: u64 = 604_800;
const SALARY: i128 = 1_000;
const REGRESSION_TOLERANCE_PCT: u64 = 5;

const BASELINE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../benchmarks/stello_pay_contract_gas.json"
);

const CLAIM_PAYROLL_PERIODS: [u32; 3] = [1, 10, 50];
const BATCH_CLAIM_MILESTONES: [usize; 3] = [1, 5, MAX_BATCH_SIZE as usize];
/// Documented ceiling for max-size milestone batches. This is intentionally
/// higher than the committed baseline tolerance so the test records both the
/// exact regression baseline and an absolute safe ceiling for `MAX_BATCH_SIZE`.
/// Updated for MAX_BATCH_SIZE=20: measured baseline is ~8_442_457 instructions
/// (includes the per-claim reentrancy guard added to the claim paths).
const MAX_BATCH_CLAIM_MILESTONE_INSTRUCTIONS: u64 = 9_500_000;

const BATCH_CREATE_PAYROLL_SIZES: [usize; 4] = [1, 5, 10, MAX_BATCH_SIZE as usize];
/// Documented ceiling for a max-size batch_create_payroll_agreements call.
/// Measured baseline at MAX_BATCH_SIZE=20 is ~5_808_763 instructions;
/// this ceiling provides ~20% headroom for SDK-version fluctuations.
const MAX_BATCH_CREATE_PAYROLL_INSTRUCTIONS: u64 = 7_000_000;

// ---------------------------------------------------------------------------
// Baseline I/O
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct GasBaseline {
    periods_or_milestones: u32,
    instructions: u64,
}

#[derive(Debug)]
struct GasBaselines {
    claim_payroll: std::vec::Vec<GasBaseline>,
    batch_claim_milestones: std::vec::Vec<GasBaseline>,
    batch_create_payroll: std::vec::Vec<GasBaseline>,
}

fn parse_u64_field(json: &str, key: &str) -> u64 {
    let needle = format!("\"{key}\": ");
    let start = json
        .find(&needle)
        .unwrap_or_else(|| panic!("missing field {key} in baseline JSON"))
        + needle.len();
    let rest = &json[start..];
    rest.split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("invalid numeric value for {key}"))
}

fn parse_first_u64(s: &str) -> u64 {
    s.split(|c: char| !c.is_ascii_digit())
        .find(|part| !part.is_empty())
        .and_then(|part| part.parse().ok())
        .unwrap_or_else(|| panic!("invalid numeric value in baseline JSON"))
}

fn parse_claim_payroll_cases(json: &str) -> std::vec::Vec<GasBaseline> {
    let section = json
        .split("\"claim_payroll\"")
        .nth(1)
        .expect("claim_payroll section missing");
    section
        .split("{ \"periods\"")
        .skip(1)
        .map(|block| GasBaseline {
            periods_or_milestones: parse_first_u64(block) as u32,
            instructions: parse_u64_field(block, "instructions"),
        })
        .collect()
}

fn parse_batch_create_payroll_cases(json: &str) -> std::vec::Vec<GasBaseline> {
    let section = json
        .split("\"batch_create_payroll\"")
        .nth(1)
        .expect("batch_create_payroll section missing");
    section
        .split("{ \"agreements\"")
        .skip(1)
        .map(|block| GasBaseline {
            periods_or_milestones: parse_first_u64(block) as u32,
            instructions: parse_u64_field(block, "instructions"),
        })
        .collect()
}

fn parse_batch_claim_cases(json: &str) -> std::vec::Vec<GasBaseline> {
    let section = json
        .split("\"batch_claim_milestones\"")
        .nth(1)
        .expect("batch_claim_milestones section missing");
    section
        .split("{ \"milestones\"")
        .skip(1)
        .map(|block| GasBaseline {
            periods_or_milestones: parse_first_u64(block) as u32,
            instructions: parse_u64_field(block, "instructions"),
        })
        .collect()
}

fn load_baselines() -> GasBaselines {
    let json = std::fs::read_to_string(BASELINE_PATH)
        .unwrap_or_else(|e| panic!("failed to read {BASELINE_PATH}: {e}"));
    GasBaselines {
        claim_payroll: parse_claim_payroll_cases(&json),
        batch_claim_milestones: parse_batch_claim_cases(&json),
        batch_create_payroll: parse_batch_create_payroll_cases(&json),
    }
}

fn write_baselines(claim: &[(u32, u64)], batch: &[(u32, u64)], create: &[(u32, u64)]) {
    let body = format!(
        r#"{{
  "version": 1,
  "sdk_version": "23.5.2",
  "captured_at": "2026-07-28",
  "regression_tolerance_pct": 5,
  "host": "soroban-sdk test host (native Rust, not WASM)",
  "claim_payroll": {{
    "description": "CPU instructions for claim_payroll with N elapsed payroll periods (single transfer, O(1) in backlog size)",
    "cases": [
{claim_cases}
    ]
  }},
  "batch_claim_milestones": {{
    "description": "CPU instructions for batch_claim_milestones with N approved milestones",
    "cases": [
{batch_cases}
    ]
  }},
  "batch_create_payroll": {{
    "description": "CPU instructions for batch_create_payroll_agreements with N agreements (linear O(N) in batch size)",
    "cases": [
{create_cases}
    ]
  }}
}}"#,
        claim_cases = claim
            .iter()
            .map(|(p, i)| format!("      {{ \"periods\": {p}, \"instructions\": {i} }},"))
            .collect::<Vec<_>>()
            .join("\n"),
        batch_cases = batch
            .iter()
            .map(|(m, i)| format!("      {{ \"milestones\": {m}, \"instructions\": {i} }},"))
            .collect::<Vec<_>>()
            .join("\n"),
        create_cases = create
            .iter()
            .map(|(a, i)| format!("      {{ \"agreements\": {a}, \"instructions\": {i} }},"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    std::fs::write(BASELINE_PATH, body)
        .unwrap_or_else(|e| panic!("failed to write {BASELINE_PATH}: {e}"));
}

// ---------------------------------------------------------------------------
// Budget measurement (soroban_sdk::testutils::budget)
// ---------------------------------------------------------------------------

/// Resets the Soroban budget tracker, runs `f`, and returns CPU instruction cost.
fn measure_instructions<F: FnOnce()>(env: &Env, f: F) -> u64 {
    env.cost_estimate().budget().reset_default();
    f();
    env.cost_estimate().budget().cpu_instruction_cost()
}

fn assert_within_tolerance(label: &str, n: u32, measured: u64, baseline: u64) {
    let max_allowed = baseline + (baseline * REGRESSION_TOLERANCE_PCT / 100);
    assert!(
        measured <= max_allowed,
        "{label} n={n}: measured {measured} instructions exceeds baseline {baseline} + {REGRESSION_TOLERANCE_PCT}% (= {max_allowed})"
    );
    println!("{label} n={n}: measured={measured} baseline={baseline} max={max_allowed}");
}

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

fn bench_env() -> Env {
    Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    })
}

fn deploy(env: &Env) -> (Address, PayrollContractClient<'_>) {
    let id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (id, client)
}

fn make_token(env: &Env) -> Address {
    let admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

/// Prepares an active payroll agreement with `periods` elapsed and zero prior claims.
fn setup_payroll_for_periods(
    env: &Env,
    contract_id: &Address,
    client: &PayrollContractClient,
    periods: u32,
) -> (u128, Address) {
    let employer = Address::generate(env);
    let employee = Address::generate(env);
    let token = make_token(env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &ONE_WEEK);
    client.add_employee_to_agreement(&agreement_id, &employee, &SALARY);
    client.activate_agreement(&agreement_id);

    let now = env.ledger().timestamp();
    let escrow = SALARY * (periods as i128) * 2;
    StellarAssetClient::new(env, &token).mint(contract_id, &escrow);

    env.as_contract(contract_id, || {
        DataKey::set_agreement_activation_time(env, agreement_id, now);
        DataKey::set_agreement_period_duration(env, agreement_id, ONE_DAY);
        DataKey::set_agreement_token(env, agreement_id, &token);
        DataKey::set_agreement_escrow_balance(env, agreement_id, &token, escrow);
        DataKey::set_employee(env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(env, agreement_id, 0, SALARY);
        DataKey::set_employee_claimed_periods(env, agreement_id, 0, 0);
        DataKey::set_employee_count(env, agreement_id, 1);
    });

    env.ledger()
        .with_mut(|li| li.timestamp += ONE_DAY * (periods as u64));

    (agreement_id, employee)
}

/// Prepares a milestone agreement with `n` approved milestones and funded escrow.
fn setup_funded_milestones(
    env: &Env,
    contract_id: &Address,
    client: &PayrollContractClient,
    n: usize,
) -> (u128, soroban_sdk::Vec<u32>) {
    let employer = Address::generate(env);
    let contributor = Address::generate(env);
    let token = make_token(env);
    let amount: i128 = 1_000;

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    for _ in 0..n {
        client.add_milestone(&agreement_id, &amount);
    }

    let total_amount = amount * n as i128;
    StellarAssetClient::new(env, &token).mint(&employer, &total_amount);
    client.fund_milestone_agreement(&agreement_id, &employer, &total_amount);

    for i in 1..=(n as u32) {
        client.approve_milestone(&agreement_id, &i);
    }

    let mut ids = soroban_sdk::Vec::<u32>::new(env);
    for i in 1..=(n as u32) {
        ids.push_back(i);
    }

    (agreement_id, ids)
}

// ---------------------------------------------------------------------------
// Benchmark tests
// ---------------------------------------------------------------------------

/// @notice Measures CPU instruction cost of `claim_payroll` at 1, 10, and 50 elapsed periods.
/// @dev Instruction count is O(1) in backlog size because all periods settle in one transfer.
#[test]
fn gas_benchmark_claim_payroll() {
    let baselines = load_baselines();
    assert_eq!(
        baselines.claim_payroll.len(),
        CLAIM_PAYROLL_PERIODS.len(),
        "baseline file must define one entry per claim_payroll size"
    );

    let update = std::env::var("UPDATE_GAS_BASELINES").ok().as_deref() == Some("1");
    let mut measured: std::vec::Vec<(u32, u64)> = std::vec::Vec::new();

    for (idx, &periods) in CLAIM_PAYROLL_PERIODS.iter().enumerate() {
        let env = bench_env();
        env.mock_all_auths();
        let (contract_id, client) = deploy(&env);
        let (agreement_id, employee) =
            setup_payroll_for_periods(&env, &contract_id, &client, periods);

        let instructions = measure_instructions(&env, || {
            client.claim_payroll(&employee, &agreement_id, &0u32);
        });

        measured.push((periods, instructions));

        if !update {
            let baseline = baselines.claim_payroll[idx].instructions;
            assert_within_tolerance("claim_payroll", periods, instructions, baseline);
        }
    }

    if update {
        let batch = load_baselines().batch_claim_milestones;
        let batch_pairs: std::vec::Vec<(u32, u64)> = batch
            .iter()
            .map(|b| (b.periods_or_milestones, b.instructions))
            .collect();
        let create = load_baselines().batch_create_payroll;
        let create_pairs: std::vec::Vec<(u32, u64)> = create
            .iter()
            .map(|b| (b.periods_or_milestones, b.instructions))
            .collect();
        write_baselines(&measured, &batch_pairs, &create_pairs);
    }
}

/// @notice Measures CPU instruction cost of `batch_claim_milestones` at 1, 5, and MAX_BATCH_SIZE
/// milestones. @dev Cost scales linearly with N (one transfer + storage write per milestone).
/// MAX_BATCH_SIZE is the enforced public ceiling and must stay under
/// MAX_BATCH_CLAIM_MILESTONE_INSTRUCTIONS.
#[test]
fn gas_benchmark_batch_claim_milestones() {
    let baselines = load_baselines();
    assert_eq!(
        baselines.batch_claim_milestones.len(),
        BATCH_CLAIM_MILESTONES.len(),
        "baseline file must define one entry per batch_claim_milestones size"
    );

    let update = std::env::var("UPDATE_GAS_BASELINES").ok().as_deref() == Some("1");
    let mut measured: std::vec::Vec<(u32, u64)> = std::vec::Vec::new();

    for (idx, &n) in BATCH_CLAIM_MILESTONES.iter().enumerate() {
        let env = bench_env();
        env.mock_all_auths();
        let (contract_id, client) = deploy(&env);
        let (agreement_id, ids) = setup_funded_milestones(&env, &contract_id, &client, n);

        let instructions = measure_instructions(&env, || {
            let result = client.batch_claim_milestones(&agreement_id, &ids);
            assert_eq!(result.successful_claims, n as u32);
        });

        measured.push((n as u32, instructions));

        if !update {
            let baseline = baselines.batch_claim_milestones[idx].instructions;
            assert_within_tolerance("batch_claim_milestones", n as u32, instructions, baseline);
            if n == MAX_BATCH_SIZE as usize {
                assert!(
                    instructions <= MAX_BATCH_CLAIM_MILESTONE_INSTRUCTIONS,
                    "batch_claim_milestones at MAX_BATCH_SIZE={MAX_BATCH_SIZE}: measured {instructions} exceeds documented ceiling {MAX_BATCH_CLAIM_MILESTONE_INSTRUCTIONS}"
                );
            }
        }
    }

    if update {
        let claim = load_baselines().claim_payroll;
        let claim_pairs: std::vec::Vec<(u32, u64)> = claim
            .iter()
            .map(|b| (b.periods_or_milestones, b.instructions))
            .collect();
        let create = load_baselines().batch_create_payroll;
        let create_pairs: std::vec::Vec<(u32, u64)> = create
            .iter()
            .map(|b| (b.periods_or_milestones, b.instructions))
            .collect();
        write_baselines(&claim_pairs, &measured, &create_pairs);
    }
}

/// @notice Measures CPU instruction cost of `batch_create_payroll_agreements` at 1, 5, 10, and
/// MAX_BATCH_SIZE agreements. Cost must scale linearly (O(N)) with batch size.
#[test]
fn gas_benchmark_batch_create_payroll_agreements() {
    let baselines = load_baselines();
    assert_eq!(
        baselines.batch_create_payroll.len(),
        BATCH_CREATE_PAYROLL_SIZES.len(),
        "baseline file must define one entry per batch_create_payroll size"
    );

    let update = std::env::var("UPDATE_GAS_BASELINES").ok().as_deref() == Some("1");
    let mut measured: std::vec::Vec<(u32, u64)> = std::vec::Vec::new();

    for (idx, &n) in BATCH_CREATE_PAYROLL_SIZES.iter().enumerate() {
        let env = bench_env();
        env.mock_all_auths();
        let (_contract_id, client) = deploy(&env);
        let employer = Address::generate(&env);
        let token = make_token(&env);

        let mut items: SdkVec<PayrollCreateParams> = vec![&env];
        for _ in 0..n {
            items.push_back(PayrollCreateParams {
                token: token.clone(),
                grace_period_seconds: ONE_WEEK,
            });
        }

        let instructions = measure_instructions(&env, || {
            let result = client.batch_create_payroll_agreements(&employer, &items);
            assert_eq!(result.total_created, n as u32);
        });

        measured.push((n as u32, instructions));

        if !update {
            let baseline = baselines.batch_create_payroll[idx].instructions;
            assert_within_tolerance(
                "batch_create_payroll_agreements",
                n as u32,
                instructions,
                baseline,
            );
            if n == MAX_BATCH_SIZE as usize {
                assert!(
                    instructions <= MAX_BATCH_CREATE_PAYROLL_INSTRUCTIONS,
                    "batch_create_payroll_agreements at MAX_BATCH_SIZE={MAX_BATCH_SIZE}: measured {instructions} exceeds documented ceiling {MAX_BATCH_CREATE_PAYROLL_INSTRUCTIONS}"
                );
            }
        }
    }

    // Linearity check: average cost per agreement at MAX_BATCH_SIZE must not
    // exceed 1.5x the cost of a single agreement.  The Soroban test host adds a
    // small overhead per host-object traversal (the items Vec is iterated twice),
    // which makes the per-agreement marginal cost rise slightly with batch size.
    // A quadratic algorithm would show >>1.5x growth; 1.5x is enough to catch
    // accidental O(N²) while tolerating host-level iteration overhead.
    if !update && measured.len() >= 3 {
        let (n1, c1) = measured[0];
        let (n_last, c_last) = measured[3];
        let avg_per_agreement_1 = c1 / n1 as u64;
        let avg_per_agreement_last = c_last / n_last as u64;
        let ratio = if avg_per_agreement_1 > 0 {
            (avg_per_agreement_last as f64) / (avg_per_agreement_1 as f64)
        } else {
            1.0
        };
        assert!(
            ratio <= 1.50,
            "batch_create_payroll_agreements avg cost per agreement grew {ratio:.2}x (from {avg_per_agreement_1} at n={n1} to {avg_per_agreement_last} at n={n_last}) — expected ≤1.50x for O(N) scaling"
        );
        println!(
            "batch_create_payroll_agreements linearity: avg_cost_per_agreement n={n1}={avg_per_agreement_1} n={n_last}={avg_per_agreement_last} ratio={ratio:.2}"
        );
    }

    if update {
        let claim = load_baselines().claim_payroll;
        let claim_pairs: std::vec::Vec<(u32, u64)> = claim
            .iter()
            .map(|b| (b.periods_or_milestones, b.instructions))
            .collect();
        let batch = load_baselines().batch_claim_milestones;
        let batch_pairs: std::vec::Vec<(u32, u64)> = batch
            .iter()
            .map(|b| (b.periods_or_milestones, b.instructions))
            .collect();
        write_baselines(&claim_pairs, &batch_pairs, &measured);
    }
}

// ---------------------------------------------------------------------------
// Edge cases — correctness guards around benchmark setup (not instruction limits)
// ---------------------------------------------------------------------------

/// @notice claim_payroll with zero elapsed periods must fail before any transfer.
#[test]
fn gas_benchmark_edge_no_periods_to_claim() {
    let env = bench_env();
    env.mock_all_auths();
    let (contract_id, client) = deploy(&env);
    let (agreement_id, employee) = setup_payroll_for_periods(&env, &contract_id, &client, 1);

    // Rewind to one second before the first full period elapses.
    env.ledger().with_mut(|li| li.timestamp -= 1);

    let err = client
        .try_claim_payroll(&employee, &agreement_id, &0u32)
        .unwrap_err()
        .unwrap();
    assert_eq!(
        err,
        stello_pay_contract::storage::PayrollError::NoPeriodsToClaim
    );
}

/// @notice batch_claim_milestones with an empty ID list is rejected at the
///         pre-flight guard with a typed error rather than panicking.
#[test]
fn gas_benchmark_edge_empty_milestone_batch() {
    let env = bench_env();
    env.mock_all_auths();
    let (_contract_id, client) = deploy(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = make_token(&env);
    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);
    let ids = soroban_sdk::Vec::<u32>::new(&env);
    let err = client
        .try_batch_claim_milestones(&agreement_id, &ids)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, stello_pay_contract::storage::PayrollError::InvalidData);
}

/// @notice claim_payroll rejects a caller who is not the indexed employee.
#[test]
fn gas_benchmark_edge_unauthorized_caller() {
    let env = bench_env();
    env.mock_all_auths();
    let (contract_id, client) = deploy(&env);
    let (agreement_id, _employee) = setup_payroll_for_periods(&env, &contract_id, &client, 1);
    let impostor = Address::generate(&env);

    let err = client
        .try_claim_payroll(&impostor, &agreement_id, &0u32)
        .unwrap_err()
        .unwrap();
    assert_eq!(
        err,
        stello_pay_contract::storage::PayrollError::Unauthorized
    );
}

/// @notice batch_create_payroll_agreements with an empty items list must be rejected.
#[test]
fn gas_benchmark_edge_empty_batch_create_payroll() {
    let env = bench_env();
    env.mock_all_auths();
    let (_contract_id, client) = deploy(&env);
    let employer = Address::generate(&env);
    let items = SdkVec::<PayrollCreateParams>::new(&env);
    let err = client
        .try_batch_create_payroll_agreements(&employer, &items)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, stello_pay_contract::storage::PayrollError::InvalidData);
}

/// @notice batch_create_payroll_agreements exceeding MAX_BATCH_SIZE must be rejected.
#[test]
fn gas_benchmark_edge_batch_create_payroll_too_large() {
    let env = bench_env();
    env.mock_all_auths();
    let (_contract_id, client) = deploy(&env);
    let employer = Address::generate(&env);
    let token = make_token(&env);
    let mut items: SdkVec<PayrollCreateParams> = vec![&env];
    for _ in 0..=MAX_BATCH_SIZE {
        items.push_back(PayrollCreateParams {
            token: token.clone(),
            grace_period_seconds: ONE_WEEK,
        });
    }
    assert_eq!(items.len(), MAX_BATCH_SIZE + 1);
    let err = client
        .try_batch_create_payroll_agreements(&employer, &items)
        .unwrap_err()
        .unwrap();
    assert_eq!(
        err,
        stello_pay_contract::storage::PayrollError::BatchTooLarge
    );
}

/// @notice batch_create_payroll_agreements with zero grace period must be rejected.
#[test]
fn gas_benchmark_edge_batch_create_payroll_zero_grace() {
    let env = bench_env();
    env.mock_all_auths();
    let (_contract_id, client) = deploy(&env);
    let employer = Address::generate(&env);
    let token = make_token(&env);
    let mut items: SdkVec<PayrollCreateParams> = vec![&env];
    items.push_back(PayrollCreateParams {
        token,
        grace_period_seconds: 0,
    });
    let err = client
        .try_batch_create_payroll_agreements(&employer, &items)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, stello_pay_contract::storage::PayrollError::InvalidData);
}
