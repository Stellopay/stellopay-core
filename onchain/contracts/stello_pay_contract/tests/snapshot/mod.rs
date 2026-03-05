#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, Vec,
};

use stello_pay_contract::storage::{
    Agreement, AgreementMode, AgreementStatus, BatchPayrollResult, DisputeStatus,
};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

/// Writes or compares a snapshot on disk under
/// `tests/snapshot/__snapshots__/{name}.snap`.
///
/// Set `UPDATE_SNAPSHOTS=1` in the environment to overwrite existing
/// snapshots instead of asserting equality.
fn assert_snapshot(name: &str, content: &str) {
    use std::env;
    use std::fs;
    use std::path::PathBuf;

    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests");
    path.push("snapshot");
    path.push("__snapshots__");

    fs::create_dir_all(&path).expect("failed to create snapshot directory");

    path.push(format!("{name}.snap"));

    let update = env::var("UPDATE_SNAPSHOTS").ok().as_deref() == Some("1");

    if path.exists() {
        let expected = fs::read_to_string(&path).expect("failed to read snapshot");
        if update && expected != content {
            fs::write(&path, content).expect("failed to update snapshot");
        } else {
            assert_eq!(expected, content, "snapshot mismatch for {name}");
        }
    } else {
        fs::write(&path, content).expect("failed to write new snapshot");
    }
}

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn register_contract(env: &Env) -> (Address, PayrollContractClient<'static>) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &contract_id);
    (contract_id, client)
}

fn create_token(env: &Env) -> Address {
    let admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

fn advance_time(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| {
        li.timestamp += seconds;
    });
}

/// Snapshot of core agreement creation flows and getters.
#[test]
fn snapshot_agreement_creation_and_getters() {
    let env = create_env();
    let (_id, client) = register_contract(&env);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = create_token(&env);

    let grace_period = 604800u64;
    let payroll_id = client.create_payroll_agreement(&employer, &token, &grace_period);
    let escrow_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &1000i128,
        &86400u64,
        &4u32,
    );

    let payroll: Agreement = client.get_agreement(&payroll_id).unwrap();
    let escrow: Agreement = client.get_agreement(&escrow_id).unwrap();
    let employees_payroll = client.get_agreement_employees(&payroll_id);
    let employees_escrow = client.get_agreement_employees(&escrow_id);

    let snapshot = format!(
        "payroll_id: {payroll_id}\nescrow_id: {escrow_id}\n\npayroll: {:#?}\nescrow: {:#?}\nemp_payroll: {:#?}\nemp_escrow: {:#?}\n",
        payroll, escrow, employees_payroll, employees_escrow
    );

    assert_snapshot("agreement_creation_and_getters", &snapshot);
}

/// Snapshot of payroll claim path including claimed periods and batch result.
#[test]
fn snapshot_payroll_claim_and_batch_result() {
    let env = create_env();
    let (contract_id, client) = register_contract(&env);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    let employer = Address::generate(&env);
    let e1 = Address::generate(&env);
    let e2 = Address::generate(&env);
    let token = create_token(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &e1, &1000i128);
    client.add_employee_to_agreement(&agreement_id, &e2, &2000i128);
    client.activate_agreement(&agreement_id);

    // Fund escrow and advance time so one period is claimable.
    let total = 3000i128;
    mint(&env, &token, &contract_id, total);

    env.as_contract(&contract_id, || {
        use stello_pay_contract::storage::DataKey;
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, total);
    });

    advance_time(&env, 86400 + 1);

    let batch: BatchPayrollResult = client.batch_claim_payroll(
        &e1,
        &agreement_id,
        &Vec::from_array(&env, [0u32, 1u32]),
    )
    .unwrap();

    let claimed_e1 = client.get_employee_claimed_periods(&agreement_id, &0u32);
    let claimed_e2 = client.get_employee_claimed_periods(&agreement_id, &1u32);

    let snapshot = format!(
        "agreement_status: {:?}\nmode: {:?}\nclaimed_e1: {}\nclaimed_e2: {}\n\nbatch_result: {:#?}\n",
        client.get_agreement(&agreement_id).unwrap().status,
        client.get_agreement(&agreement_id).unwrap().mode,
        claimed_e1,
        claimed_e2,
        batch
    );

    assert_snapshot("payroll_claim_and_batch_result", &snapshot);
}

/// Snapshot of dispute lifecycle and FX conversion helpers.
#[test]
fn snapshot_dispute_and_fx_helpers() {
    let env = create_env();
    let (contract_id, client) = register_contract(&env);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let base = create_token(&env);
    let quote = create_token(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &base, &604800u64);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000i128);
    client.activate_agreement(&agreement_id);

    client.set_arbiter(&employer, &arbiter);
    let before = client.get_dispute_status(&agreement_id);

    client.raise_dispute(&employer, &agreement_id).unwrap();
    let raised = client.get_dispute_status(&agreement_id);

    // Configure FX admin and rate.
    client
        .set_exchange_rate_admin(&owner, &owner)
        .expect("set fx admin");
    client
        .set_exchange_rate(&owner, &base, &quote, &1_500i128)
        .expect("set rate");

    let converted = client
        .convert_currency(&base, &quote, &1_000i128)
        .expect("convert");

    // Resolve dispute by splitting funds.
    env.as_contract(&contract_id, || {
        use stello_pay_contract::storage::DataKey;
        let total = 2_000i128;
        mint(&env, &base, &contract_id, total);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &base, total);
    });
    client
        .resolve_dispute(&arbiter, &agreement_id, &1_000i128, &1_000i128)
        .unwrap();
    let after = client.get_dispute_status(&agreement_id);

    let snapshot = format!(
        "dispute_before: {:?}\ndispute_raised: {:?}\ndispute_after: {:?}\nconverted_1000_base_to_quote: {}\n",
        before, raised, after, converted
    );

    assert_snapshot("dispute_and_fx_helpers", &snapshot);
}

/// Snapshot of emergency pause configuration and state helpers.
#[test]
fn snapshot_emergency_pause_state() {
    let env = create_env();
    let (_id, client) = register_contract(&env);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    let g1 = Address::generate(&env);
    let g2 = Address::generate(&env);
    let g3 = Address::generate(&env);

    let guardians = Vec::from_array(&env, [g1.clone(), g2.clone(), g3.clone()]);
    client.set_emergency_guardians(&guardians);

    let stored_guardians = client.get_emergency_guardians().unwrap();
    let paused_before = client.is_emergency_paused();

    // Propose and approve pause (threshold 2/3).
    client.propose_emergency_pause(&g1, &0u64).unwrap();
    client.approve_emergency_pause(&g2).unwrap();

    let paused_after = client.is_emergency_paused();
    let state = client.get_emergency_pause_state().unwrap();

    let snapshot = format!(
        "guardians: {:#?}\npaused_before: {}\npaused_after: {}\nstate: {:#?}\n",
        stored_guardians, paused_before, paused_after, state
    );

    assert_snapshot("emergency_pause_state", &snapshot);
}

