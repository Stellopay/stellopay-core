//! Regression tests for Soroban state-archival prevention (issue #1267).
//!
//! The `stello_pay_contract` uses persistent storage for agreements, escrow
//! balances, and employee records. Under Soroban's state-archival model those
//! entries are archived once their time-to-live (TTL) lapses, and an archived
//! entry is not readable until it is explicitly restored. The contract
//! therefore routes every persistent read/write through the TTL-bumping
//! accessors in `storage.rs` and bumps the contract instance TTL on every
//! entrypoint.
//!
//! These tests pin that behaviour:
//! * `persistent_state_survives_ledger_advance_past_threshold` — a milestone
//!   agreement remains readable and claimable after the ledger advances past
//!   `PERSISTENT_TTL_THRESHOLD`.
//! * `contract_instance_survives_ledger_advance_past_threshold` — the contract
//!   instance itself remains live across the same advance.
//! * `persistent_read_rebumps_ttl_when_below_threshold` — a read near expiry
//!   restores the full `PERSISTENT_BUMP_AMOUNT` runway.
//! * `persistent_and_instance_storage_only_accessed_through_storage_layer` —
//!   static guard proving no production module bypasses the accessor layer.

use soroban_sdk::testutils::storage::{Instance as _, Persistent as _};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{vec, Address, Env};
use stello_pay_contract::storage::{
    MilestoneKey, PERSISTENT_BUMP_AMOUNT, PERSISTENT_TTL_THRESHOLD,
};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

/// Deploys the contract and returns `(env, contract_id, client)`.
fn setup_env() -> (Env, Address, PayrollContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);
    (env, contract_id, client)
}

/// Creates a funded, approved one-milestone agreement and returns the
/// agreement id plus its contributor.
fn setup_funded_milestone(
    env: &Env,
    client: &PayrollContractClient,
) -> (u128, Address, Address, Address) {
    let owner = Address::generate(env);
    client.initialize(&owner);

    let employer = Address::generate(env);
    let contributor = Address::generate(env);
    let token = env
        .register_stellar_asset_contract_v2(Address::generate(env))
        .address();

    soroban_sdk::token::StellarAssetClient::new(env, &token).mint(&employer, &1_000i128);
    let id =
        client.create_milestone_agreement(&employer, &contributor, &token, &vec![env, 1_000i128]);
    client.fund_milestone_agreement(&id, &employer, &1_000i128);
    client.approve_milestone(&id, &1);
    (id, employer, contributor, token)
}

/// A milestone agreement created well before `PERSISTENT_TTL_THRESHOLD`
/// elapses must remain readable and claimable. Before the TTL work, the
/// milestone entries received only the network default persistent TTL
/// (4096 ledgers in the test host), so advancing by the 30-day threshold
/// archived them and the read/claim failed.
#[test]
fn persistent_state_survives_ledger_advance_past_threshold() {
    let (env, contract_id, client) = setup_env();
    let (id, _employer, _contributor, _token) = setup_funded_milestone(&env, &client);

    // The write path must already have extended the entry beyond the threshold.
    let amount_key = MilestoneKey::MilestoneAmount(id, 1);
    let ttl_at_write = env.as_contract(&contract_id, || {
        env.storage().persistent().get_ttl(&amount_key)
    });
    assert!(
        ttl_at_write > PERSISTENT_TTL_THRESHOLD,
        "write path did not extend TTL past the threshold (got {ttl_at_write})"
    );

    // Advance the ledger past the threshold.
    let seq = env.ledger().sequence();
    env.ledger()
        .with_mut(|l| l.sequence_number = seq + PERSISTENT_TTL_THRESHOLD + 1);

    // The agreement state is still readable...
    let milestone = client
        .get_milestone(&id, &1)
        .expect("milestone must remain readable after the threshold");
    assert_eq!(milestone.amount, 1_000);
    assert!(milestone.approved);

    // ...and the claim path still settles against the funded agreement.
    client.claim_milestone(&id, &1);
    assert!(
        client.get_milestone(&id, &1).unwrap().claimed,
        "milestone must be claimable after the threshold"
    );
}

/// The contract instance entry must be bumped too: if it archives, the entire
/// contract becomes unusable, not just a single key.
#[test]
fn contract_instance_survives_ledger_advance_past_threshold() {
    let (env, contract_id, client) = setup_env();
    let owner = Address::generate(&env);
    client.initialize(&owner);

    // `initialize` must have refreshed the instance entry well past the
    // threshold. Without the entrypoint bump the host default (4096 ledgers)
    // would be all the runway the instance has.
    let ttl_after_init = env.as_contract(&contract_id, || env.storage().instance().get_ttl());
    assert!(
        ttl_after_init > PERSISTENT_TTL_THRESHOLD,
        "entrypoints did not extend the instance TTL (got {ttl_after_init})"
    );

    // Advance the ledger past the threshold...
    let seq = env.ledger().sequence();
    env.ledger()
        .with_mut(|l| l.sequence_number = seq + PERSISTENT_TTL_THRESHOLD + 1);

    // ...and the instance is still usable, then re-bumped by the next call.
    assert!(!client.is_emergency_paused());
    let instance_ttl = env.as_contract(&contract_id, || env.storage().instance().get_ttl());
    assert!(
        instance_ttl > PERSISTENT_TTL_THRESHOLD,
        "instance entry must remain live past the threshold (got {instance_ttl})"
    );
}

/// A read that finds an entry with less than `PERSISTENT_TTL_THRESHOLD` of TTL
/// left must restore it to `PERSISTENT_BUMP_AMOUNT`.
#[test]
fn persistent_read_rebumps_ttl_when_below_threshold() {
    let (env, contract_id, client) = setup_env();
    let (id, _employer, _contributor, _token) = setup_funded_milestone(&env, &client);

    let amount_key = MilestoneKey::MilestoneAmount(id, 1);

    // Move the ledger forward so that less than the threshold remains, keeping
    // the entry itself alive.
    let seq = env.ledger().sequence();
    let advance = PERSISTENT_BUMP_AMOUNT - (PERSISTENT_TTL_THRESHOLD / 2);
    env.ledger().with_mut(|l| l.sequence_number = seq + advance);

    let ttl_before = env.as_contract(&contract_id, || {
        env.storage().persistent().get_ttl(&amount_key)
    });
    assert!(
        ttl_before < PERSISTENT_TTL_THRESHOLD,
        "test setup expected a near-expiry entry (got {ttl_before})"
    );

    // A read through the contract re-bumps the entry.
    let _ = client.get_milestone(&id, &1);

    let ttl_after = env.as_contract(&contract_id, || {
        env.storage().persistent().get_ttl(&amount_key)
    });
    assert!(
        ttl_after > PERSISTENT_TTL_THRESHOLD,
        "read path did not restore the TTL runway (got {ttl_after})"
    );
}

/// Static guard: production modules may only touch persistent/instance storage
/// through the `storage.rs` accessor layer. `storage.rs` itself and the
/// `#[cfg(test)]` mock contract are the only permitted exceptions.
#[test]
fn persistent_and_instance_storage_only_accessed_through_storage_layer() {
    use std::path::Path;

    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    // `storage.rs` implements the layer; `mock_contract.rs` is a test-only
    // double compiled under `#[cfg(test)]` and never part of a release build.
    let exempt = ["storage.rs", "mock_contract.rs"];

    let mut offenders = Vec::new();
    for entry in std::fs::read_dir(&src_dir).expect("src dir must exist") {
        let path = entry.expect("readable dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        if exempt.contains(&file_name.as_str()) {
            continue;
        }

        let contents = std::fs::read_to_string(&path).expect("readable source file");
        for (line_no, line) in contents.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if code.contains(".persistent()") || code.contains(".instance()") {
                offenders.push(format!("{file_name}:{}", line_no + 1));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "raw storage access bypasses the TTL-bumping layer in storage.rs: {offenders:?}"
    );
}
