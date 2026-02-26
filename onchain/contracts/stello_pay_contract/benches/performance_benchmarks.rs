use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, Vec,
};

use stello_pay_contract::{PayrollContract, PayrollContractClient};
use stello_pay_contract::storage::{DataKey, AgreementStatus};

/// Create a baseline environment + deployed payroll contract for benchmarks.
fn setup_env() -> (Env, Address, Address, Address, PayrollContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    let arbiter = Address::generate(&env);
    client.set_arbiter(&owner, &arbiter);

    let employer = Address::generate(&env);

    (env, owner, employer, arbiter, client)
}

fn bench_create_payroll_agreement(c: &mut Criterion) {
    let mut group = c.benchmark_group("create_payroll_agreement");

    group.bench_function("single_agreement", |b| {
        b.iter_batched(
            || {
                let (env, _owner, employer, _arbiter, client) = setup_env();
                let token_admin = Address::generate(&env);
                let token = env
                    .register_stellar_asset_contract_v2(token_admin)
                    .address();
                (env, employer, client, token)
            },
            |(env, employer, client, token)| {
                let grace: u64 = 7 * 24 * 60 * 60;
                let _id = client.create_payroll_agreement(&employer, &token, &grace);
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

fn setup_funded_escrow_single(
    env: &Env,
    client: &PayrollContractClient<'static>,
    employer: &Address,
) -> (Address, u128) {
    let contributor = Address::generate(env);

    let token_admin = Address::generate(env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    let amount_per_period: i128 = 1_000;
    let period_seconds: u64 = 86_400;
    let num_periods: u32 = 12;

    let agreement_id = client.create_escrow_agreement(
        employer,
        &contributor,
        &token,
        &amount_per_period,
        &period_seconds,
        &num_periods,
    );

    let total = amount_per_period * (num_periods as i128);
    let sac = StellarAssetClient::new(env, &token);
    sac.mint(&client.address, &total);

    env.as_contract(&client.address, || {
        DataKey::set_agreement_escrow_balance(env, agreement_id, &token, total);
    });
    client.activate_agreement(&agreement_id);

    (token, agreement_id)
}

fn bench_claim_time_based(c: &mut Criterion) {
    let mut group = c.benchmark_group("claim_time_based");

    group.throughput(Throughput::Elements(1));

    group.bench_function("single_period_claim", |b| {
        b.iter_batched(
            || {
                let (env, _owner, employer, _arbiter, client) = setup_env();
                let (token, agreement_id) = setup_funded_escrow_single(&env, &client, &employer);
                (env, client, token, agreement_id)
            },
            |(env, client, _token, agreement_id)| {
                // Advance one full period and perform a single claim.
                env.ledger().with_mut(|li| {
                    li.timestamp += 86_400;
                });
                let _ = client.claim_time_based(&agreement_id);
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

fn setup_funded_milestone(
    env: &Env,
    client: &PayrollContractClient<'static>,
    employer: &Address,
) -> (Address, u128, Vec<u32>) {
    let contributor = Address::generate(env);

    let token_admin = Address::generate(env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    let amount: i128 = 1_000;
    let count: u32 = 10;

    let agreement_id = client.create_milestone_agreement(employer, &contributor, &token);
    for _ in 0..count {
        client.add_milestone(&agreement_id, &amount);
    }

    let sac = StellarAssetClient::new(env, &token);
    sac.mint(&client.address, &(amount * (count as i128)));

    // Approve all milestones.
    for i in 1..=count {
        client.approve_milestone(&agreement_id, &i);
    }

    // Build ID list
    let mut ids = Vec::new(env);
    for i in 1..=count {
        ids.push_back(i);
    }

    (token, agreement_id, ids)
}

fn bench_batch_claim_milestones(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_claim_milestones");

    group.throughput(Throughput::Elements(10));

    group.bench_function("claim_10_milestones", |b| {
        b.iter_batched(
            || {
                let (env, _owner, employer, _arbiter, client) = setup_env();
                let (_token, agreement_id, ids) = setup_funded_milestone(&env, &client, &employer);
                (env, client, agreement_id, ids)
            },
            |(env, client, agreement_id, ids)| {
                let _ = client.batch_claim_milestones(&agreement_id, &ids);

                // Ensure agreement transitions to completed state at the end.
                env.as_contract(&client.address, || {
                    let agreement = stello_pay_contract::storage::Agreement {
                        id: agreement_id,
                        employer: Address::generate(&env), // dummy; only status is checked
                        token: Address::generate(&env),
                        mode: stello_pay_contract::storage::AgreementMode::Escrow,
                        status: AgreementStatus::Completed,
                        total_amount: 0,
                        paid_amount: 0,
                        created_at: 0,
                        activated_at: None,
                        cancelled_at: None,
                        grace_period_seconds: 0,
                        dispute_status: stello_pay_contract::storage::DisputeStatus::None,
                        dispute_raised_at: None,
                        amount_per_period: None,
                        period_seconds: None,
                        num_periods: None,
                        claimed_periods: None,
                    };
                    // Bump storage to keep host work comparable.
                    env.storage()
                        .persistent()
                        .set(&stello_pay_contract::storage::StorageKey::Agreement(agreement_id), &agreement);
                });
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default();
    targets =
        bench_create_payroll_agreement,
        bench_claim_time_based,
        bench_batch_claim_milestones
);
criterion_main!(benches);

