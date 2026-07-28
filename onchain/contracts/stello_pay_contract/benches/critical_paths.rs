//! Soroban host cost sampling for core payroll contract paths (instruction count after each call).
//!
//! Run: `cargo bench --bench critical_paths`
//!
//! Uses the agreement-based API on `main` (`PayrollContract` in the crate root).

#![allow(deprecated)]

use soroban_sdk::{testutils::Address as _, vec, Address, Env, Vec};

use stello_pay_contract::storage::{PayrollCreateParams, MAX_BATCH_SIZE};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn main() {
    let env = Env::default();
    env.mock_all_auths();

    println!("stellopay critical path costs (test host)");
    println!("------------------------------------------");

    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);
    let owner = Address::generate(&env);

    env.cost_estimate().budget().reset_default();
    client.initialize(&owner);
    println!(
        "initialize: cpu_insns={}",
        env.cost_estimate().budget().cpu_instruction_cost()
    );

    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let grace = 604_800u64;

    env.cost_estimate().budget().reset_default();
    let agreement_id = client.create_payroll_agreement(&employer, &token, &grace);
    println!(
        "create_payroll_agreement: cpu_insns={}",
        env.cost_estimate().budget().cpu_instruction_cost()
    );

    let contributor = Address::generate(&env);
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
    assert!(agreement_id >= 1);

    env.cost_estimate().budget().reset_default();
    client.get_agreement(&agreement_id);
    println!(
        "get_agreement: cpu_insns={}",
        env.cost_estimate().budget().cpu_instruction_cost()
    );

    env.cost_estimate().budget().reset_default();
    client.create_milestone_agreement(&employer, &contributor, &token);
    println!(
        "create_milestone_agreement: cpu_insns={}",
        env.cost_estimate().budget().cpu_instruction_cost()
    );

    env.cost_estimate().budget().reset_default();
    client.get_arbiter();
    println!(
        "get_arbiter: cpu_insns={}",
        env.cost_estimate().budget().cpu_instruction_cost()
    );

    // ── batch_create_payroll_agreements (marginal-cost scaling) ──────────────
    println!();
    println!("batch_create_payroll_agreements (marginal cost per agreement):");
    let batch_sizes: [usize; 4] = [1, 5, 10, MAX_BATCH_SIZE as usize];
    let token = Address::generate(&env);
    let grace = 604_800u64;
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
