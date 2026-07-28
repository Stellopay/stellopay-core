//! Soroban host cost sampling for core payroll contract paths (instruction count after each call).
//!
//! Run: `cargo bench --bench critical_paths`
//!
//! Uses the agreement-based API on `main` (`PayrollContract` in the crate root).

#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    vec, Address, Env, Vec,
};
use stello_pay_contract::{
    storage::{DataKey, PayrollCreateParams, MAX_BATCH_SIZE},
    PayrollContract, PayrollContractClient,
};

fn main() {
    println!("stellopay critical path costs (test host)");
    println!("------------------------------------------");
    bench_initialize();
    bench_create_payroll_agreement();
    bench_create_escrow_agreement();
    bench_get_agreement();
    bench_create_milestone_agreement();
    bench_get_arbiter();
    bench_batch_create_payroll();
    bench_claim_payroll_in_token();
}

fn setup_env() -> (Env, PayrollContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);

    env.cost_estimate().budget().reset_default();
    client.initialize(&owner);

    (env, client, owner)
}

fn bench_initialize() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);

    env.cost_estimate().budget().reset_default();
    client.initialize(&owner);
    println!(
        "initialize: cpu_insns={}",
        env.cost_estimate().budget().cpu_instruction_cost()
    );
}

fn bench_create_payroll_agreement() {
    let (env, client, _owner) = setup_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let grace = 604_800u64;

    env.cost_estimate().budget().reset_default();
    let _agreement_id = client.create_payroll_agreement(&employer, &token, &grace);
    println!(
        "create_payroll_agreement: cpu_insns={}",
        env.cost_estimate().budget().cpu_instruction_cost()
    );
}

fn bench_create_escrow_agreement() {
    let (env, client, _owner) = setup_env();
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    env.cost_estimate().budget().reset_default();
    let escrow_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &1000i128,
        &86400u64,
        &4u32,
    );
    println!(
        "create_escrow_agreement: cpu_insns={}",
        env.cost_estimate().budget().cpu_instruction_cost()
    );
    assert!(escrow_id >= 1);
}

fn bench_get_agreement() {
    let (env, client, _owner) = setup_env();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let grace = 604_800u64;
    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace);

    env.cost_estimate().budget().reset_default();
    client.get_agreement(&agreement_id);
    println!(
        "get_agreement: cpu_insns={}",
        env.cost_estimate().budget().cpu_instruction_cost()
    );
}

fn bench_create_milestone_agreement() {
    let (env, client, _owner) = setup_env();
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    env.cost_estimate().budget().reset_default();
    client.create_milestone_agreement(&employer, &contributor, &token);
    println!(
        "create_milestone_agreement: cpu_insns={}",
        env.cost_estimate().budget().cpu_instruction_cost()
    );
}

fn bench_get_arbiter() {
    let (env, client, _owner) = setup_env();

    // Measure cost with no arbiter set (returning None).
    env.cost_estimate().budget().reset_default();
    client.get_arbiter();
    println!(
        "get_arbiter: cpu_insns={}",
        env.cost_estimate().budget().cpu_instruction_cost()
    );
}

fn bench_batch_create_payroll() {
    let (env, client, _owner) = setup_env();
    let token = Address::generate(&env);
    let grace = 604_800u64;

    println!();
    println!("batch_create_payroll_agreements (marginal cost per agreement):");
    let batch_sizes: [usize; 4] = [1, 5, 10, MAX_BATCH_SIZE as usize];
    let mut prev_cost: Option<u64> = None;
    let mut prev_size: Option<usize> = None;

    for &n in &batch_sizes {
        let employer = Address::generate(&env);
        let mut items: Vec<PayrollCreateParams> = vec![&env];
        for _ in 0..n {
            items.push_back(PayrollCreateParams {
                token: token.clone(),
                grace_period_seconds: grace,
            });
        }

        env.cost_estimate().budget().reset_default();
        let result = client.batch_create_payroll_agreements(&employer, &items);
        let cost = env.cost_estimate().budget().cpu_instruction_cost();
        let total = result.total_created;
        assert_eq!(total, n as u32);

        let marginal = prev_cost.map(|pc| {
            let ps = prev_size.unwrap();
            (cost - pc) / ((n - ps) as u64)
        });

        println!(
            "  n={n}: cpu_insns={cost}{}",
            marginal
                .map(|m| format!("  marginal_per_agreement≈{m}"))
                .unwrap_or_default()
        );

        prev_cost = Some(cost);
        prev_size = Some(n);
    }
}

/// Benchmarks `claim_payroll_in_token` with an active currency conversion.
///
/// Sets up a payroll agreement denominated in `base_token`, configures an FX
/// rate (1 base = 2 payout), funds escrow in `payout_token`, advances one
/// period, and measures the cost of claiming in the payout currency.
fn bench_claim_payroll_in_token() {
    let (env, client, owner) = setup_env();

    // ── Token setup ──────────────────────────────────────────────────────────
    let base_admin = Address::generate(&env);
    let base_token = env.register_stellar_asset_contract_v2(base_admin).address();
    let payout_admin = Address::generate(&env);
    let payout_token = env
        .register_stellar_asset_contract_v2(payout_admin)
        .address();

    // FX rate: 1 base = 2 payout (scaled by 1e6).
    let fx_rate: i128 = 2_000_000;
    client.set_exchange_rate(&owner, &base_token, &payout_token, &fx_rate);

    // ── Agreement + employee setup ───────────────────────────────────────────
    let employer = Address::generate(&env);
    let grace_period: u64 = 604_800; // 7 days
    let period_seconds: u64 = 86_400; // 1 day
    let salary_per_period: i128 = 1_000;

    let agreement_id = client.create_payroll_agreement(&employer, &base_token, &grace_period);

    let employee = Address::generate(&env);
    client.add_employee_to_agreement(&agreement_id, &employee, &salary_per_period);
    client.activate_agreement(&agreement_id);

    // ── Seed DataKey metadata and escrow for the payout token ────────────────
    let contract_address = client.address.clone();
    let escrow_total: i128 = 100_000;

    // Fund payout tokens to the contract.
    let payout_stellar_client = StellarAssetClient::new(&env, &payout_token);
    payout_stellar_client.mint(&contract_address, &escrow_total);

    env.as_contract(&contract_address, || {
        let now = env.ledger().timestamp();

        DataKey::set_agreement_activation_time(&env, agreement_id, now);
        DataKey::set_agreement_period_duration(&env, agreement_id, period_seconds);
        DataKey::set_agreement_token(&env, agreement_id, &base_token);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary_per_period);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee_count(&env, agreement_id, 1);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &payout_token, escrow_total);
    });

    // Advance one full period so exactly one salary is claimable.
    env.ledger().with_mut(|li| {
        li.timestamp += period_seconds;
    });

    // ── Measure the multi-currency claim cost ────────────────────────────────
    env.cost_estimate().budget().reset_default();
    client.claim_payroll_in_token(&employee, &agreement_id, &0u32, &payout_token);
    println!(
        "claim_payroll_in_token (multi-currency, 1 period): cpu_insns={}",
        env.cost_estimate().budget().cpu_instruction_cost()
    );
}
