//! Soroban host cost sampling for core payroll contract paths (instruction count after each call).
//!
//! Run: `cargo bench --bench critical_paths`
//!
//! Uses the agreement-based API on `main` (`PayrollContract` in the crate root).

#![allow(deprecated)]

use soroban_sdk::{testutils::Address as _, Address, Env};

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
}
