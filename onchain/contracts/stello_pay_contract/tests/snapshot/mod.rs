#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, Vec,
};

use stello_pay_contract::storage::{
    Agreement, AgreementStatus, BatchPayrollResult, DataKey, MilestoneKey,
};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

// ============================================================================
// CORE SNAPSHOT HARNESS
// ============================================================================

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

// ============================================================================
// SHARED HELPERS  original tests
// ============================================================================

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

// ============================================================================
// ORIGINAL SNAPSHOT TESTS (preserved)
// ============================================================================

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
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token, total);
    });

    advance_time(&env, 86400 + 1);

    let batch: BatchPayrollResult = client
        .batch_claim_payroll(&e1, &agreement_id, &Vec::from_array(&env, [0u32, 1u32]))
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
    client.set_exchange_rate_admin(&owner, &owner).expect("set fx admin");
    client.set_exchange_rate(&owner, &base, &quote, &1_500i128).expect("set rate");

    let converted = client.convert_currency(&base, &quote, &1_000i128).expect("convert");

    // Resolve dispute by splitting funds.
    env.as_contract(&contract_id, || {
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

// ============================================================================
// SNAPSHOT REGRESSION TESTS  Agreement Lifecycle
// ============================================================================
//
// These tests assert that storage state remains consistent across critical
// transitions. Timestamps are pinned via env.ledger().with_mut so snapshots
// are fully deterministic. Only stable (non-timestamp) fields are captured.
//
// Security invariants verified:
//   - Paused agreements block all claim operations
//   - Emergency pause blocks payroll and milestone claims
//   - Dispute transitions are irreversible (idempotency)
//   - Boundary timestamps (exact grace expiry) are handled correctly
//
// To regenerate after an intentional change:
//   UPDATE_SNAPSHOTS=1 cargo test -p stello_pay_contract --test snapshot

// ============================================================================
// HELPERS  regression snapshot tests
// ============================================================================

/// Pin the ledger to a fixed base timestamp so snapshots are deterministic.
fn pin_time(env: &Env, ts: u64) {
    env.ledger().with_mut(|li| {
        li.timestamp = ts;
    });
}

/// Advance ledger by `seconds` from current timestamp.
fn tick(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| {
        li.timestamp += seconds;
    });
}

/// Deploy contract, initialize with a fixed owner, return (contract_id, client, owner).
fn setup(env: &Env) -> (Address, PayrollContractClient<'static>, Address) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (contract_id, client, owner)
}

/// Create a real Stellar Asset token and return its address.
fn new_token(env: &Env) -> Address {
    let admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

/// Mint `amount` of `token` directly into `to`.
fn fund(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

/// Seed the contract internal escrow balance for `agreement_id`.
fn seed_escrow(
    env: &Env,
    contract_id: &Address,
    agreement_id: u128,
    token: &Address,
    amount: i128,
) {
    fund(env, token, contract_id, amount);
    env.as_contract(contract_id, || {
        DataKey::set_agreement_escrow_balance(env, agreement_id, token, amount);
    });
}

/// Serialize only the stable, non-timestamp fields of an Agreement.
fn stable_agreement_fields(a: &Agreement) -> String {
    format!(
        "id: {}\nmode: {:?}\nstatus: {:?}\ntotal_amount: {}\npaid_amount: {}\n\
grace_period_seconds: {}\ndispute_status: {:?}\namount_per_period: {:?}\n\
period_seconds: {:?}\nnum_periods: {:?}\nclaimed_periods: {:?}\n",
        a.id,
        a.mode,
        a.status,
        a.total_amount,
        a.paid_amount,
        a.grace_period_seconds,
        a.dispute_status,
        a.amount_per_period,
        a.period_seconds,
        a.num_periods,
        a.claimed_periods,
    )
}

// ============================================================================
// SCENARIO 1 - Agreement Created -> Funded -> First Payroll Claim
// ============================================================================

#[test]
fn snapshot_payroll_lifecycle_created_funded_first_claim() {
    let env = Env::default();
    env.mock_all_auths();
    pin_time(&env, 1_000_000);

    let (contract_id, client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = new_token(&env);

    const SALARY: i128 = 5_000;
    const PERIOD: u64 = 86_400;
    const GRACE: u64 = 604_800;

    let id = client.create_payroll_agreement(&employer, &token, &GRACE);
    let after_create = client.get_agreement(&id).unwrap();

    client.add_employee_to_agreement(&id, &employee, &SALARY);
    let after_add_emp = client.get_agreement(&id).unwrap();
    let employees = client.get_agreement_employees(&id);

    client.activate_agreement(&id);
    let after_activate = client.get_agreement(&id).unwrap();

    let escrow_total: i128 = SALARY * 3;
    seed_escrow(&env, &contract_id, id, &token, escrow_total);

    env.as_contract(&contract_id, || {
        DataKey::set_employee_count(&env, id, 1);
        DataKey::set_employee(&env, id, 0, &employee);
        DataKey::set_employee_salary(&env, id, 0, SALARY);
        DataKey::set_agreement_activation_time(&env, id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, id, PERIOD);
        DataKey::set_agreement_token(&env, id, &token);
    });

    tick(&env, PERIOD + 1);

    let claimed_before = client.get_employee_claimed_periods(&id, &0u32);
    client.claim_payroll(&employee, &id, &0u32).unwrap();
    let claimed_after = client.get_employee_claimed_periods(&id, &0u32);
    let after_claim = client.get_agreement(&id).unwrap();

    // Idempotency: second claim in same period must fail
    let second_claim_rejected = client.try_claim_payroll(&employee, &id, &0u32).is_err();

    let snapshot = format!(
        "=== PHASE: after_create ===\n{}\
=== PHASE: after_add_employee ===\n{}employee_count: {}\n\n\
=== PHASE: after_activate ===\n{}\
=== PHASE: after_first_claim ===\n{}\
claimed_periods_before: {}\nclaimed_periods_after: {}\n\
second_claim_same_period_rejected: {}\n",
        stable_agreement_fields(&after_create),
        stable_agreement_fields(&after_add_emp),
        employees.len(),
        stable_agreement_fields(&after_activate),
        stable_agreement_fields(&after_claim),
        claimed_before,
        claimed_after,
        second_claim_rejected,
    );

    assert_snapshot("payroll_lifecycle_created_funded_first_claim", &snapshot);
}

// ============================================================================
// SCENARIO 2 - Dispute Opened -> Escalation -> Resolution
// ============================================================================

#[test]
fn snapshot_dispute_opened_escalation_resolution() {
    let env = Env::default();
    env.mock_all_auths();
    pin_time(&env, 2_000_000);

    let (contract_id, client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let token = new_token(&env);

    const SALARY: i128 = 10_000;
    const GRACE: u64 = 604_800;

    let id = client.create_payroll_agreement(&employer, &token, &GRACE);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);

    let escrow_total: i128 = SALARY * 2;
    seed_escrow(&env, &contract_id, id, &token, escrow_total);
    client.set_arbiter(&employer, &arbiter);

    // Phase 1: Active, no dispute
    let status_active = client.get_agreement(&id).unwrap().status.clone();
    let dispute_before = client.get_dispute_status(&id);

    // Phase 2: Cancel -> starts grace period
    client.cancel_agreement(&id);
    let after_cancel = client.get_agreement(&id).unwrap();

    // Phase 3: Raise dispute halfway through grace window
    tick(&env, GRACE / 2);
    client.raise_dispute(&employer, &id).unwrap();
    let after_raise = client.get_agreement(&id).unwrap();
    let dispute_raised = client.get_dispute_status(&id);

    // Idempotency: duplicate raise_dispute must fail
    let duplicate_rejected = client.try_raise_dispute(&employer, &id).is_err();

    // Phase 4: Resolve dispute
    let pay_employee: i128 = escrow_total / 2;
    let refund_employer: i128 = escrow_total / 2;
    client.resolve_dispute(&arbiter, &id, &pay_employee, &refund_employer).unwrap();
    let after_resolve = client.get_agreement(&id).unwrap();
    let dispute_after = client.get_dispute_status(&id);

    // Boundary: dispute after grace window expires must fail
    let id2 = client.create_payroll_agreement(&employer, &token, &GRACE);
    client.add_employee_to_agreement(&id2, &employee, &SALARY);
    client.activate_agreement(&id2);
    client.cancel_agreement(&id2);
    tick(&env, GRACE + 1);
    let dispute_outside_grace_rejected = client.try_raise_dispute(&employer, &id2).is_err();

    let snapshot = format!(
        "=== PHASE: active_no_dispute ===\nstatus: {:?}\ndispute_status: {:?}\n\n\
=== PHASE: after_cancel ===\n{}\
=== PHASE: after_raise_dispute ===\n{}dispute_status: {:?}\n\
duplicate_raise_rejected: {}\n\n\
=== PHASE: after_resolve_dispute ===\n{}dispute_status: {:?}\n\n\
=== BOUNDARY: dispute_outside_grace_window ===\n\
dispute_outside_grace_rejected: {}\n",
        status_active,
        dispute_before,
        stable_agreement_fields(&after_cancel),
        stable_agreement_fields(&after_raise),
        dispute_raised,
        duplicate_rejected,
        stable_agreement_fields(&after_resolve),
        dispute_after,
        dispute_outside_grace_rejected,
    );

    assert_snapshot("dispute_opened_escalation_resolution", &snapshot);
}

// ============================================================================
// SCENARIO 3 - Emergency Pause Toggled -> Operations Blocked / Unblocked
//
// Security assertions:
//   - payroll claim blocked while emergency paused
//   - milestone claim blocked while emergency paused
//   - both succeed after unpause
// ============================================================================

#[test]
fn snapshot_emergency_pause_blocks_and_unblocks_operations() {
    let env = Env::default();
    env.mock_all_auths();
    pin_time(&env, 3_000_000);

    let (contract_id, client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = new_token(&env);

    const SALARY: i128 = 3_000;
    const PERIOD: u64 = 86_400;
    const GRACE: u64 = 604_800;
    const MILESTONE_AMOUNT: i128 = 2_000;

    // Setup payroll agreement
    let payroll_id = client.create_payroll_agreement(&employer, &token, &GRACE);
    client.add_employee_to_agreement(&payroll_id, &employee, &SALARY);
    client.activate_agreement(&payroll_id);
    seed_escrow(&env, &contract_id, payroll_id, &token, SALARY * 5);

    env.as_contract(&contract_id, || {
        DataKey::set_employee_count(&env, payroll_id, 1);
        DataKey::set_employee(&env, payroll_id, 0, &employee);
        DataKey::set_employee_salary(&env, payroll_id, 0, SALARY);
        DataKey::set_agreement_activation_time(&env, payroll_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, payroll_id, PERIOD);
        DataKey::set_agreement_token(&env, payroll_id, &token);
    });

    // Setup milestone agreement
    let ms_id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&ms_id, &MILESTONE_AMOUNT);
    client.approve_milestone(&ms_id, &1u32);
    fund(&env, &token, &contract_id, MILESTONE_AMOUNT);

    // Advance so payroll period is claimable
    tick(&env, PERIOD + 1);

    // Phase 1: Not paused
    let paused_before = client.is_emergency_paused();

    // Phase 2: Emergency pause
    client.emergency_pause().unwrap();
    let paused_after_pause = client.is_emergency_paused();
    let pause_state = client.get_emergency_pause_state().unwrap();
    let pause_state_snap = format!(
        "is_paused: {}\ntimelock_end: {:?}\n",
        pause_state.is_paused, pause_state.timelock_end,
    );

    // Security: both claim types must be blocked
    let payroll_blocked = client.try_claim_payroll(&employee, &payroll_id, &0u32).is_err();
    let milestone_blocked = client.try_claim_milestone(&ms_id, &1u32).is_err();

    // Phase 3: Unpause
    client.emergency_unpause().unwrap();
    let paused_after_unpause = client.is_emergency_paused();

    // Phase 4: Claims succeed after unpause
    let payroll_claim_ok = client.try_claim_payroll(&employee, &payroll_id, &0u32).is_ok();
    let milestone_claim_ok = client.try_claim_milestone(&ms_id, &1u32).is_ok();

    let snapshot = format!(
        "=== PHASE: before_pause ===\nis_emergency_paused: {}\n\n\
=== PHASE: after_emergency_pause ===\nis_emergency_paused: {}\n{}\
payroll_claim_blocked: {}\nmilestone_claim_blocked: {}\n\n\
=== PHASE: after_emergency_unpause ===\nis_emergency_paused: {}\n\
payroll_claim_succeeds: {}\nmilestone_claim_succeeds: {}\n",
        paused_before,
        paused_after_pause,
        pause_state_snap,
        payroll_blocked,
        milestone_blocked,
        paused_after_unpause,
        payroll_claim_ok,
        milestone_claim_ok,
    );

    assert_snapshot("emergency_pause_blocks_and_unblocks_operations", &snapshot);
}

// ============================================================================
// SCENARIO 4 - Milestone Completion (all milestones claimed -> Completed)
//
// Security assertions:
//   - claim while paused is rejected
//   - re-claiming an already-claimed milestone is rejected
//   - auto-completion fires on last claim
// ============================================================================

#[test]
fn snapshot_milestone_completion_all_claimed() {
    let env = Env::default();
    env.mock_all_auths();
    pin_time(&env, 4_000_000);

    let (contract_id, client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = new_token(&env);

    const M1: i128 = 1_000;
    const M2: i128 = 2_000;

    let id = client.create_milestone_agreement(&employer, &contributor, &token);
    client.add_milestone(&id, &M1);
    client.add_milestone(&id, &M2);
    client.approve_milestone(&id, &1u32);
    client.approve_milestone(&id, &2u32);
    fund(&env, &token, &contract_id, M1 + M2);

    let m1_before = client.get_milestone(&id, &1u32).unwrap();
    let m2_before = client.get_milestone(&id, &2u32).unwrap();

    // Security: claim while paused must fail
    client.pause_agreement(&id);
    let claim_while_paused = client.try_claim_milestone(&id, &1u32).is_err();
    client.resume_agreement(&id);

    // Claim milestone 1
    client.claim_milestone(&id, &1u32);
    let m1_after = client.get_milestone(&id, &1u32).unwrap();
    let count_after_m1 = client.get_milestone_count(&id);

    // Idempotency: re-claim must fail
    let reclaim_m1_rejected = client.try_claim_milestone(&id, &1u32).is_err();

    // Claim milestone 2 -> auto-complete
    client.claim_milestone(&id, &2u32);
    let m2_after = client.get_milestone(&id, &2u32).unwrap();

    let final_status = env.as_contract(&contract_id, || {
        env.storage()
            .instance()
            .get::<_, AgreementStatus>(&MilestoneKey::Status(id))
            .unwrap()
    });

    let snapshot = format!(
        "=== PHASE: before_claims ===\n\
m1: approved={} claimed={} amount={}\n\
m2: approved={} claimed={} amount={}\n\n\
=== SECURITY: claim_while_paused_rejected ===\n{}\n\n\
=== PHASE: after_claim_m1 ===\n\
m1: approved={} claimed={} amount={}\n\
milestone_count: {}\nreclaim_m1_rejected: {}\n\n\
=== PHASE: after_claim_m2_auto_complete ===\n\
m2: approved={} claimed={} amount={}\n\
final_status: {:?}\n",
        m1_before.approved, m1_before.claimed, m1_before.amount,
        m2_before.approved, m2_before.claimed, m2_before.amount,
        claim_while_paused,
        m1_after.approved, m1_after.claimed, m1_after.amount,
        count_after_m1,
        reclaim_m1_rejected,
        m2_after.approved, m2_after.claimed, m2_after.amount,
        final_status,
    );

    assert_snapshot("milestone_completion_all_claimed", &snapshot);
}

// ============================================================================
// SCENARIO 5 - Pause / Resume Preserves All Stable Agreement Fields
// ============================================================================

#[test]
fn snapshot_pause_resume_preserves_agreement_fields() {
    let env = Env::default();
    env.mock_all_auths();
    pin_time(&env, 5_000_000);

    let (_contract_id, client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = new_token(&env);

    const SALARY: i128 = 7_500;
    const GRACE: u64 = 604_800;

    let id = client.create_payroll_agreement(&employer, &token, &GRACE);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);

    let before_pause = client.get_agreement(&id).unwrap();

    client.pause_agreement(&id);
    let while_paused = client.get_agreement(&id).unwrap();
    client.resume_agreement(&id);
    let after_resume = client.get_agreement(&id).unwrap();

    let fields_before = stable_agreement_fields(&before_pause);
    let fields_after = stable_agreement_fields(&after_resume);
    let fields_preserved = fields_before == fields_after;

    let snapshot = format!(
        "=== PHASE: before_pause ===\n{}\
=== PHASE: while_paused ===\nstatus: {:?}\n\n\
=== PHASE: after_resume ===\n{}\
stable_fields_preserved_across_pause_resume: {}\n",
        fields_before,
        while_paused.status,
        fields_after,
        fields_preserved,
    );

    assert_snapshot("pause_resume_preserves_agreement_fields", &snapshot);
}

// ============================================================================
// SCENARIO 6 - Repeated Transitions Rejected (Idempotency)
// ============================================================================

#[test]
fn snapshot_repeated_transitions_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    pin_time(&env, 6_000_000);

    let (_contract_id, client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = new_token(&env);

    const SALARY: i128 = 1_000;
    const GRACE: u64 = 604_800;

    let id = client.create_payroll_agreement(&employer, &token, &GRACE);
    client.add_employee_to_agreement(&id, &employee, &SALARY);
    client.activate_agreement(&id);

    let double_activate_rejected = client.try_activate_agreement(&id).is_err();

    client.pause_agreement(&id);
    let double_pause_rejected = client.try_pause_agreement(&id).is_err();

    client.resume_agreement(&id);
    let double_resume_rejected = client.try_resume_agreement(&id).is_err();

    client.cancel_agreement(&id);
    let double_cancel_rejected = client.try_cancel_agreement(&id).is_err();

    let snapshot = format!(
        "double_activate_rejected: {}\n\
double_pause_rejected: {}\n\
double_resume_rejected: {}\n\
double_cancel_rejected: {}\n",
        double_activate_rejected,
        double_pause_rejected,
        double_resume_rejected,
        double_cancel_rejected,
    );

    assert_snapshot("repeated_transitions_rejected", &snapshot);
}

// ============================================================================
// SCENARIO 7 - Escrow Agreement: Created -> Funded -> First Period Claim
// ============================================================================

#[test]
fn snapshot_escrow_lifecycle_created_funded_first_claim() {
    let env = Env::default();
    env.mock_all_auths();
    pin_time(&env, 7_000_000);

    let (contract_id, client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = new_token(&env);

    const AMOUNT_PER_PERIOD: i128 = 4_000;
    const PERIOD: u64 = 86_400;
    const NUM_PERIODS: u32 = 3;
    const TOTAL: i128 = AMOUNT_PER_PERIOD * (NUM_PERIODS as i128);

    let id = client
        .create_escrow_agreement(&employer, &contributor, &token, &AMOUNT_PER_PERIOD, &PERIOD, &NUM_PERIODS)
        .unwrap();

    let after_create = client.get_agreement(&id).unwrap();

    client.activate_agreement(&id);
    let after_activate = client.get_agreement(&id).unwrap();

    seed_escrow(&env, &contract_id, id, &token, TOTAL);

    // Claim first period
    tick(&env, PERIOD + 1);
    client.claim_time_based(&id).unwrap();
    let after_first_claim = client.get_agreement(&id).unwrap();

    // Claim remaining periods -> auto-complete
    tick(&env, PERIOD * (NUM_PERIODS as u64 - 1) + 1);
    client.claim_time_based(&id).unwrap();
    let after_all_claimed = client.get_agreement(&id).unwrap();

    let snapshot = format!(
        "=== PHASE: after_create ===\n{}\
=== PHASE: after_activate ===\n{}\
=== PHASE: after_first_claim ===\n{}\
=== PHASE: after_all_claimed_auto_complete ===\n{}\n",
        stable_agreement_fields(&after_create),
        stable_agreement_fields(&after_activate),
        stable_agreement_fields(&after_first_claim),
        stable_agreement_fields(&after_all_claimed),
    );

    assert_snapshot("escrow_lifecycle_created_funded_first_claim", &snapshot);
}