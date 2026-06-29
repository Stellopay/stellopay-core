//! Gas optimization tests for `stello_pay_contract`.
//!
//! Measures Soroban resource consumption — CPU instructions, memory bytes,
//! ledger write bytes, and estimated transaction fee in stroops — for the
//! three most storage-intensive batch operations at three input sizes.
//!
//! # Running
//!
//! ```bash
//! # from onchain/contracts/stello_pay_contract/
//! cargo test gas -- --nocapture
//! ```
//!
//! # WASM prerequisite
//!
//! The file `tests/stello_pay_contract.wasm` must reflect the current source
//! before running. Rebuild it with:
//!
//! ```bash
//! stellar contract build   # from this contract's directory
//! cp ../../target/wasm32v1-none/release/stello_pay_contract.wasm tests/
//! ```

#![cfg(test)]
#![allow(deprecated)] // register_contract_wasm is deprecated; kept for consistency with other tests

use soroban_sdk::{
    testutils::{Address as _, EnvTestConfig},
    token::StellarAssetClient,
    Address, Env, Vec,
};
use stello_pay_contract::storage::PayrollCreateParams;
use stello_pay_contract::PayrollContractClient;

// ---------------------------------------------------------------------------
// WASM binary — must be up to date with the current source
// ---------------------------------------------------------------------------

const WASM: &[u8] = include_bytes!("stello_pay_contract.wasm");

// ---------------------------------------------------------------------------
// Input size levels used by all three tests
// ---------------------------------------------------------------------------

const SIZES: [usize; 3] = [1, 10, 50];

// ---------------------------------------------------------------------------
// Regression bounds
//
// Guards are active when the constant is > 0.  The N=50 bounds are set to
// ~1.2× the measured baseline (soroban-sdk 23.5.2, 2026-03-23).  The N=1
// and N=10 constants remain 0 (disabled) until a tighter baseline is needed.
// ---------------------------------------------------------------------------

const BATCH_CREATE_1_INSTR_MAX: i64 = 0;
const BATCH_CREATE_10_INSTR_MAX: i64 = 0;
const BATCH_CREATE_50_INSTR_MAX: i64 = 27_000_000; // baseline 22_532_825

const BATCH_CLAIM_1_INSTR_MAX: i64 = 0;
const BATCH_CLAIM_10_INSTR_MAX: i64 = 0;
const BATCH_CLAIM_50_INSTR_MAX: i64 = 33_000_000; // baseline 27_712_846

const ADD_EMP_1ST_INSTR_MAX: i64 = 0;
const ADD_EMP_10TH_INSTR_MAX: i64 = 0;
const ADD_EMP_50TH_INSTR_MAX: i64 = 1_400_000; // baseline 1_137_103

// ---------------------------------------------------------------------------
// ResourceReport — snapshot of cost_estimate() after a single contract call
// ---------------------------------------------------------------------------

struct ResourceReport {
    label: &'static str,
    /// Input size (number of items, or call index for single-item operations).
    n: usize,
    /// CPU instructions consumed (primary "gas" analog on Soroban).
    instructions: i64,
    /// Memory bytes allocated.
    mem_bytes: i64,
    /// Total bytes written to the ledger.
    write_bytes: u32,
    /// Number of ledger entries written.
    write_entries: u32,
    /// Estimated transaction fee in stroops (Pubnet fee schedule, 2024-12-11).
    fee_stroops: i64,
}

impl ResourceReport {
    /// Capture metrics from the most recent top-level contract invocation.
    ///
    /// Must be called immediately after the target contract method returns.
    fn capture(env: &Env, label: &'static str, n: usize) -> Self {
        let ce = env.cost_estimate();
        let res = ce.resources();
        let fee = ce.fee();
        Self {
            label,
            n,
            instructions: res.instructions,
            mem_bytes: res.mem_bytes,
            write_bytes: res.write_bytes,
            write_entries: res.write_entries,
            fee_stroops: fee.total,
        }
    }
}

// ---------------------------------------------------------------------------
// Output — Markdown table printed to stdout (visible with --nocapture)
// ---------------------------------------------------------------------------

fn print_table(reports: &[ResourceReport]) {
    println!(
        "\n| {:<44} | {:>4} | {:>14} | {:>12} | {:>12} | {:>13} | {:>13} |",
        "Label", "N", "Instructions", "Mem bytes", "Write bytes", "Write entries", "Fee (stroops)"
    );
    println!(
        "|{:-<46}|{:-<6}|{:-<16}|{:-<14}|{:-<14}|{:-<15}|{:-<15}|",
        "", "", "", "", "", "", ""
    );
    for r in reports {
        println!(
            "| {:<44} | {:>4} | {:>14} | {:>12} | {:>12} | {:>13} | {:>13} |",
            r.label,
            r.n,
            r.instructions,
            r.mem_bytes,
            r.write_bytes,
            r.write_entries,
            r.fee_stroops,
        );
    }
    println!();
}

// ---------------------------------------------------------------------------
// Shared setup helpers
// ---------------------------------------------------------------------------

/// Creates an `Env` with snapshot capture disabled.
///
/// Snapshot capture is suppressed to prevent gas test runs from polluting the
/// snapshot directory used by `tests/snapshot/mod.rs`.
fn gas_env() -> Env {
    Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    })
}

/// Deploys the contract from the compiled WASM binary and runs `initialize`.
///
/// Returns `(contract_address, client)`.  The contract address is required
/// when minting tokens directly to the contract's escrow balance.
fn deploy(env: &Env) -> (Address, PayrollContractClient<'_>) {
    let id = env.register_contract_wasm(None, WASM);
    let client = PayrollContractClient::new(env, &id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (id, client)
}

/// Registers a Stellar Asset Contract and returns its address.
fn make_token(env: &Env) -> Address {
    let admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

/// Mints `amount` tokens to `to` using the SAC admin path.
fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

// ---------------------------------------------------------------------------
// Test A — batch_create_payroll_agreements (N = 1, 10, 50)
// ---------------------------------------------------------------------------
//
// Each item in the batch writes one persistent Agreement entry plus one
// persistent AgreementEmployees entry and emits one event.  Cost scales
// linearly with N.

#[test]
fn gas_batch_create_payroll_agreements() {
    const LABEL: &str = "batch_create_payroll_agreements";
    const GRACE: u64 = 7 * 24 * 60 * 60; // one week

    let instr_bounds = [
        BATCH_CREATE_1_INSTR_MAX,
        BATCH_CREATE_10_INSTR_MAX,
        BATCH_CREATE_50_INSTR_MAX,
    ];

    let mut reports = std::vec::Vec::new();

    for (idx, &n) in SIZES.iter().enumerate() {
        // Fresh environment per size so prior state does not affect metrics.
        let env = gas_env();
        env.mock_all_auths();
        let (_id, client) = deploy(&env);

        let employer = Address::generate(&env);

        let mut items = Vec::<PayrollCreateParams>::new(&env);
        for _ in 0..n {
            items.push_back(PayrollCreateParams {
                token: make_token(&env),
                grace_period_seconds: GRACE,
            });
        }

        // ── measured call ──────────────────────────────────────────────────
        client.batch_create_payroll_agreements(&employer, &items);
        // ───────────────────────────────────────────────────────────────────

        let report = ResourceReport::capture(&env, LABEL, n);

        if instr_bounds[idx] > 0 {
            assert!(
                report.instructions <= instr_bounds[idx],
                "{LABEL} N={n}: instructions {} exceeded regression bound {}",
                report.instructions,
                instr_bounds[idx],
            );
        }

        reports.push(report);
    }

    print_table(&reports);
}

// ---------------------------------------------------------------------------
// Test B — batch_claim_milestones (N = 1, 10, 50)
// ---------------------------------------------------------------------------
//
// Each milestone claim performs two instance reads (approved, claimed flags),
// two instance writes, one token.transfer cross-contract call, and emits one
// event.  Cost scales linearly with N.

/// Prepares a milestone agreement with `n` approved milestones and a funded
/// token balance on the contract ready for batch claiming.
fn setup_funded_milestones(
    env: &Env,
    contract_addr: &Address,
    client: &PayrollContractClient<'_>,
    n: usize,
) -> (u128, Vec<u32>) {
    let employer = Address::generate(env);
    let contributor = Address::generate(env);
    let token = make_token(env);
    let amount_per_milestone: i128 = 1_000;

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token);

    for _ in 0..n {
        client.add_milestone(&agreement_id, &amount_per_milestone);
    }

    // Fund the contract so that token.transfer succeeds during claiming.
    mint(env, &token, contract_addr, amount_per_milestone * n as i128);

    // Approve every milestone (IDs are 1-based).
    for i in 1..=(n as u32) {
        client.approve_milestone(&agreement_id, &i);
    }

    let mut ids = Vec::<u32>::new(env);
    for i in 1..=(n as u32) {
        ids.push_back(i);
    }

    (agreement_id, ids)
}

#[test]
fn gas_batch_claim_milestones() {
    const LABEL: &str = "batch_claim_milestones";

    let instr_bounds = [
        BATCH_CLAIM_1_INSTR_MAX,
        BATCH_CLAIM_10_INSTR_MAX,
        BATCH_CLAIM_50_INSTR_MAX,
    ];

    let mut reports = std::vec::Vec::new();

    for (idx, &n) in SIZES.iter().enumerate() {
        let env = gas_env();
        env.mock_all_auths();
        let (id, client) = deploy(&env);

        let (agreement_id, ids) = setup_funded_milestones(&env, &id, &client, n);

        // ── measured call ──────────────────────────────────────────────────
        client.batch_claim_milestones(&agreement_id, &ids);
        // ───────────────────────────────────────────────────────────────────

        let report = ResourceReport::capture(&env, LABEL, n);

        if instr_bounds[idx] > 0 {
            assert!(
                report.instructions <= instr_bounds[idx],
                "{LABEL} N={n}: instructions {} exceeded regression bound {}",
                report.instructions,
                instr_bounds[idx],
            );
        }

        reports.push(report);
    }

    print_table(&reports);
}

// ---------------------------------------------------------------------------
// Test C — add_employee_to_agreement: Nth call cost (N = 1, 10, 50)
// ---------------------------------------------------------------------------
//
// `add_employee_to_agreement` reads the full AgreementEmployees Vec from
// persistent storage, appends one EmployeeInfo entry, and writes the entire
// Vec back.  The serialised size of the Vec grows with each call, so the Nth
// call writes more bytes than the 1st — an O(N) write_bytes curve.
//
// This test isolates the resource cost of exactly the Nth call by building
// up N-1 employees in setup before capturing the final, measured call.

fn measure_nth_add_employee(target_n: usize) -> ResourceReport {
    const LABEL: &str = "add_employee_to_agreement (Nth call)";
    const SALARY: i128 = 1_000;
    const GRACE: u64 = 7 * 24 * 60 * 60;

    let env = gas_env();
    env.mock_all_auths();
    let (_id, client) = deploy(&env);

    let employer = Address::generate(&env);
    let token = make_token(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &GRACE);

    // Build state: add (target_n - 1) employees without capturing resources.
    for _ in 0..(target_n - 1) {
        let emp = Address::generate(&env);
        client.add_employee_to_agreement(&agreement_id, &emp, &SALARY);
    }

    // ── measured call (the Nth employee) ──────────────────────────────────
    let last_emp = Address::generate(&env);
    client.add_employee_to_agreement(&agreement_id, &last_emp, &SALARY);
    // ───────────────────────────────────────────────────────────────────────

    ResourceReport::capture(&env, LABEL, target_n)
}

#[test]
fn gas_add_employee_scaling() {
    let instr_bounds = [
        ADD_EMP_1ST_INSTR_MAX,
        ADD_EMP_10TH_INSTR_MAX,
        ADD_EMP_50TH_INSTR_MAX,
    ];

    let mut reports = std::vec::Vec::new();

    for (idx, &n) in SIZES.iter().enumerate() {
        let report = measure_nth_add_employee(n);

        if instr_bounds[idx] > 0 {
            assert!(
                report.instructions <= instr_bounds[idx],
                "add_employee_to_agreement N={n}: instructions {} exceeded regression bound {}",
                report.instructions,
                instr_bounds[idx],
            );
        }

        reports.push(report);
    }

    print_table(&reports);
}
