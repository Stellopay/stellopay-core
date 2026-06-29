#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Bytes, Env, Vec};

use slashing_penalty::{SlashError, SlashingPenaltyContract, SlashingPenaltyContractClient};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn register_contract(env: &Env) -> (Address, SlashingPenaltyContractClient<'static>) {
    #[allow(deprecated)]
    let id = env.register_contract(None, SlashingPenaltyContract);
    let client = SlashingPenaltyContractClient::new(env, &id);
    (id, client)
}

fn setup(env: &Env, quorum: u32) -> (Address, SlashingPenaltyContractClient<'static>, Address) {
    let (_id, client) = register_contract(env);
    let admin = Address::generate(env);
    client.initialize(&admin, &quorum).unwrap();
    let target = Address::generate(env);
    (admin, client, target)
}

fn empty_bytes(env: &Env) -> Bytes {
    Bytes::new(env)
}

fn some_evidence(env: &Env) -> Bytes {
    let mut b = Bytes::new(env);
    b.push_back(0xde);
    b.push_back(0xad);
    b.push_back(0xbe);
    b.push_back(0xef);
    b
}

fn make_attestors(env: &Env, n: u32) -> Vec<Address> {
    let mut v = Vec::new(env);
    for _ in 0..n {
        v.push_back(Address::generate(env));
    }
    v
}

// ---------------------------------------------------------------------------
// Initialisation tests
// ---------------------------------------------------------------------------

#[test]
fn initialize_rejects_zero_quorum() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    let res = client.try_initialize(&admin, &0u32);
    assert_eq!(res.unwrap_err().unwrap(), SlashError::ZeroQuorum);
}

#[test]
fn initialize_succeeds_with_valid_quorum() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin, &2u32).unwrap();
    assert_eq!(client.get_quorum(), 2u32);
}

#[test]
fn initialize_rejects_double_init() {
    let env = create_env();
    let (_id, client) = register_contract(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin, &1u32).unwrap();
    // Second call should panic (assert in contract body).
    let res = client.try_initialize(&admin, &1u32);
    assert!(res.is_err());
}

// ---------------------------------------------------------------------------
// Attestor-backed slash tests
// ---------------------------------------------------------------------------

#[test]
fn execute_slash_below_quorum_is_rejected() {
    // quorum = 3, but only 2 attestors supplied → BelowQuorum
    let env = create_env();
    let (admin, client, target) = setup(&env, 3);
    let attestors = make_attestors(&env, 2);
    let res = client.try_execute_slash(
        &admin,
        &1u128,
        &target,
        &500u32,
        &attestors,
        &empty_bytes(&env),
    );
    assert_eq!(res.unwrap_err().unwrap(), SlashError::BelowQuorum);
}

#[test]
fn execute_slash_at_exact_quorum_is_allowed() {
    // quorum = 2, exactly 2 attestors → should succeed
    let env = create_env();
    let (admin, client, target) = setup(&env, 2);
    let attestors = make_attestors(&env, 2);
    client
        .execute_slash(
            &admin,
            &1u128,
            &target,
            &500u32,
            &attestors,
            &empty_bytes(&env),
        )
        .unwrap();

    let record = client.get_slash_record(&1u128).unwrap();
    assert!(record.executed);
}

#[test]
fn execute_slash_above_quorum_is_allowed() {
    // quorum = 2, three attestors → should succeed
    let env = create_env();
    let (admin, client, target) = setup(&env, 2);
    let attestors = make_attestors(&env, 3);
    client
        .execute_slash(
            &admin,
            &2u128,
            &target,
            &100u32,
            &attestors,
            &empty_bytes(&env),
        )
        .unwrap();

    let record = client.get_slash_record(&2u128).unwrap();
    assert!(record.executed);
    assert_eq!(record.penalty_bps, 100u32);
}

// ---------------------------------------------------------------------------
// Evidence-only (no-attestor) slash tests
// ---------------------------------------------------------------------------

#[test]
fn execute_slash_no_attestors_with_evidence_is_allowed() {
    // Zero attestors + valid evidence → evidence-only path, allowed
    let env = create_env();
    let (admin, client, target) = setup(&env, 2);
    let no_attestors = Vec::new(&env);
    client
        .execute_slash(
            &admin,
            &10u128,
            &target,
            &1000u32,
            &no_attestors,
            &some_evidence(&env),
        )
        .unwrap();

    let record = client.get_slash_record(&10u128).unwrap();
    assert!(record.executed);
}

#[test]
fn execute_slash_no_attestors_no_evidence_is_rejected() {
    // Zero attestors + no evidence → MissingEvidence
    let env = create_env();
    let (admin, client, target) = setup(&env, 2);
    let no_attestors = Vec::new(&env);
    let res = client.try_execute_slash(
        &admin,
        &11u128,
        &target,
        &1000u32,
        &no_attestors,
        &empty_bytes(&env),
    );
    assert_eq!(res.unwrap_err().unwrap(), SlashError::MissingEvidence);
}

// ---------------------------------------------------------------------------
// Double-slash guard
// ---------------------------------------------------------------------------

#[test]
fn execute_slash_double_slash_is_rejected() {
    let env = create_env();
    let (admin, client, target) = setup(&env, 1);
    let attestors = make_attestors(&env, 1);

    client
        .execute_slash(
            &admin,
            &20u128,
            &target,
            &200u32,
            &attestors,
            &empty_bytes(&env),
        )
        .unwrap();

    // Second slash on same agreement should fail.
    let res = client.try_execute_slash(
        &admin,
        &20u128,
        &target,
        &200u32,
        &attestors,
        &empty_bytes(&env),
    );
    assert_eq!(res.unwrap_err().unwrap(), SlashError::AlreadySlashed);
}

// ---------------------------------------------------------------------------
// Authorisation guard
// ---------------------------------------------------------------------------

#[test]
fn execute_slash_non_admin_is_rejected() {
    let env = create_env();
    let (_, client, target) = setup(&env, 1);
    let not_admin = Address::generate(&env);
    let attestors = make_attestors(&env, 1);

    let res = client.try_execute_slash(
        &not_admin,
        &30u128,
        &target,
        &300u32,
        &attestors,
        &empty_bytes(&env),
    );
    assert_eq!(res.unwrap_err().unwrap(), SlashError::Unauthorized);
}
