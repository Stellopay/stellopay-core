#![cfg(test)]

<<<<<<< HEAD
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
=======
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    token::StellarAssetClient,
    Address, BytesN, Env,
};

use slashing_penalty::{
    SlashingPenaltyContract, SlashingPenaltyContractClient,
    Offense, SlashStatus, SlashError,
};

// ─── Test Helpers ─────────────────────────────────────────────────────────────

/// Default appeal window in seconds (7 days).
const APPEAL_WINDOW: u64 = 7 * 24 * 60 * 60;

struct TestEnv {
    env: Env,
    client: SlashingPenaltyContractClient<'static>,
    admin: Address,
    slasher1: Address,
    slasher2: Address,
    slasher3: Address,
    offender: Address,
    token: Address,
}

impl TestEnv {
    fn setup() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, SlashingPenaltyContract);
        let client = SlashingPenaltyContractClient::new(&env, &contract_id);

        let admin    = Address::generate(&env);
        let slasher1 = Address::generate(&env);
        let slasher2 = Address::generate(&env);
        let slasher3 = Address::generate(&env);
        let offender = Address::generate(&env);
        let token_admin = Address::generate(&env);
        let token = env.register_stellar_asset_contract_v2(token_admin).address();
        let token_sac = StellarAssetClient::new(&env, &token);
        token_sac.mint(&offender, &1_000_000i128);

        // Per-event cap: 50%, period cap: 6_000, lifetime cap: 9_000, period: 1 day.
        client.initialize(&admin, &token, &2u32, &5_000u32, &6_000i128, &9_000i128, &86_400u64);
        client.add_slasher(&slasher1);
        client.add_slasher(&slasher2);
        client.add_slasher(&slasher3);

        // Give offender an initial staked balance.
        client.stake(&offender, &10_000i128);

        TestEnv { env, client, admin, slasher1, slasher2, slasher3, offender, token }
    }

    fn evidence_hash(&self, seed: u8) -> BytesN<32> {
        BytesN::from_array(&self.env, &[seed; 32])
    }

    fn advance_time(&self, seconds: u64) {
        let current = self.env.ledger().timestamp();
        self.env.ledger().set(LedgerInfo {
            timestamp: current + seconds,
            ..self.env.ledger().get()
        });
    }
}

// ─── Initialisation ───────────────────────────────────────────────────────────

#[test]
fn test_initialize_sets_admin_and_quorum() {
    let t = TestEnv::setup();
    assert_eq!(t.client.get_quorum(), 2u32);
    let slashers = t.client.get_slashers();
    assert!(slashers.contains(&t.slasher1));
    assert!(slashers.contains(&t.slasher2));
}

#[test]
fn test_initialize_twice_fails() {
    let t = TestEnv::setup();
    let result = t.client.try_initialize(
        &t.admin,
        &t.token,
        &2u32,
        &5_000u32,
        &6_000i128,
        &9_000i128,
        &86_400u64,
    );
    assert_eq!(result, Err(Ok(SlashError::AlreadyInitialized)));
}

#[test]
fn test_initialize_zero_quorum_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SlashingPenaltyContract);
    let client = SlashingPenaltyContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);

    // quorum = 0 must be rejected with a typed error, never silently coerced.
    let result = client.try_initialize(
        &admin,
        &token,
        &0u32,
        &5_000u32,
        &6_000i128,
        &9_000i128,
        &86_400u64,
    );
    assert_eq!(result, Err(Ok(SlashError::ZeroQuorum)));
}

#[test]
fn test_initialize_quorum_one_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SlashingPenaltyContract);
    let client = SlashingPenaltyContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);

    // quorum = 1 is the minimum valid value and must be stored as-is (not raised to DEFAULT_QUORUM).
    client.initialize(
        &admin,
        &token,
        &1u32,
        &5_000u32,
        &6_000i128,
        &9_000i128,
        &86_400u64,
    );
    assert_eq!(client.get_quorum(), 1u32);
}

// ─── Role Management ─────────────────────────────────────────────────────────

#[test]
fn test_add_and_remove_slasher() {
    let t = TestEnv::setup();
    let new_slasher = Address::generate(&t.env);
    t.client.add_slasher(&new_slasher);
    assert!(t.client.get_slashers().contains(&new_slasher));

    t.client.remove_slasher(&new_slasher);
    assert!(!t.client.get_slashers().contains(&new_slasher));
}

#[test]
fn test_non_slasher_cannot_slash() {
    let t = TestEnv::setup();
    let rando = Address::generate(&t.env);
    let result = t.client.try_slash_with_evidence(
        &rando,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &t.evidence_hash(1),
        &0u64,
    );
    assert_eq!(result, Err(Ok(SlashError::Unauthorized)));
}

// ─── Stake Management ─────────────────────────────────────────────────────────

#[test]
fn test_stake_increases_balance() {
    let t = TestEnv::setup();
    let initial = t.client.get_stake_balance(&t.offender);
    t.client.stake(&t.offender, &5_000i128);
    assert_eq!(t.client.get_stake_balance(&t.offender), initial + 5_000);
}

#[test]
fn test_unstake_decreases_balance() {
    let t = TestEnv::setup();
    let initial = t.client.get_stake_balance(&t.offender);
    t.client.unstake(&t.offender, &3_000i128);
    assert_eq!(t.client.get_stake_balance(&t.offender), initial - 3_000);
}

#[test]
fn test_unstake_more_than_balance_fails() {
    let t = TestEnv::setup();
    let result = t.client.try_unstake(&t.offender, &999_999i128);
    assert_eq!(result, Err(Ok(SlashError::InsufficientStake)));
}

// ─── Evidence-Based Slash ─────────────────────────────────────────────────────

#[test]
fn test_slash_with_evidence_creates_pending_record() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(1);

    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );

    let record = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(record.status, SlashStatus::Pending);
    assert_eq!(record.penalty_bps, 1_000u32);
    // 10% of 10_000 stake = 1_000
    assert_eq!(record.escrowed_amount, 1_000i128);
    assert_eq!(t.client.get_stake_balance(&t.offender), 9_000i128);
}

#[test]
fn test_slash_proportionality() {
    let t = TestEnv::setup();
    // 25% penalty on 10_000 stake = 2_500
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::FraudProof,
        &2_500u32,
        &t.evidence_hash(2),
        &0u64,
    );
    let record = t.client.get_slash_record(&t.evidence_hash(2)).unwrap();
    assert_eq!(record.escrowed_amount, 2_500i128);
}

#[test]
fn test_zero_slash_fails() {
    let t = TestEnv::setup();
    let result = t.client.try_slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &0u32,
        &t.evidence_hash(3),
        &0u64,
    );
    assert_eq!(result, Err(Ok(SlashError::ZeroPenalty)));
}

#[test]
fn test_max_slash_boundary_passes() {
    let t = TestEnv::setup();
    // Exactly at MAX_PENALTY_BPS (5_000) should succeed
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::FraudProof,
        &5_000u32,
        &t.evidence_hash(4),
        &0u64,
    );
    let record = t.client.get_slash_record(&t.evidence_hash(4)).unwrap();
    assert_eq!(record.escrowed_amount, 5_000i128);
}

#[test]
fn test_exceed_max_slash_fails() {
    let t = TestEnv::setup();
    let result = t.client.try_slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::FraudProof,
        &5_001u32,
        &t.evidence_hash(5),
        &0u64,
    );
    assert_eq!(result, Err(Ok(SlashError::PenaltyTooHigh)));
}

#[test]
fn test_invalid_cap_config_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SlashingPenaltyContract);
    let client = SlashingPenaltyContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);

    let init_bad = client.try_initialize(
        &admin,
        &token,
        &2u32,
        &5_000u32,
        &10_000i128,
        &5_000i128, // per-period cannot exceed lifetime
        &86_400u64,
    );
    assert_eq!(init_bad, Err(Ok(SlashError::InvalidConfig)));
}

#[test]
fn test_duplicate_evidence_fails() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(6);
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::DoubleSigning, &1_000u32, &hash, &0u64,
    );
    let result = t.client.try_slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::DoubleSigning, &1_000u32, &hash, &0u64,
    );
    assert_eq!(result, Err(Ok(SlashError::DuplicateEvidence)));
}

#[test]
fn test_offender_with_no_stake_fails() {
    let t = TestEnv::setup();
    let no_stake_addr = Address::generate(&t.env);
    let result = t.client.try_slash_with_evidence(
        &t.slasher1,
        &no_stake_addr,
        &Offense::DoubleSigning,
        &1_000u32,
        &t.evidence_hash(7),
        &0u64,
    );
    assert_eq!(result, Err(Ok(SlashError::InsufficientStake)));
}

// ─── Attestation-Based Slash ──────────────────────────────────────────────────

#[test]
fn test_attestation_requires_quorum_before_execute() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(10);

    // Only one attestor — quorum is 2
    t.client.attest_slash(
        &t.slasher1, &t.offender, &Offense::DoubleSigning, &1_000u32, &hash, &0u64,
    );

    t.advance_time(APPEAL_WINDOW + 1);

    let result = t.client.try_execute_slash(&hash);
    assert_eq!(result, Err(Ok(SlashError::QuorumNotMet)));
}

#[test]
fn test_attestation_quorum_met_allows_execute() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(11);

    t.client.attest_slash(
        &t.slasher1, &t.offender, &Offense::DoubleSigning, &1_000u32, &hash, &0u64,
    );
    t.client.attest_slash(
        &t.slasher2, &t.offender, &Offense::DoubleSigning, &1_000u32, &hash, &0u64,
    );

    t.advance_time(APPEAL_WINDOW + 1);
    t.client.execute_slash(&hash);

    let record = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(record.status, SlashStatus::Executed);
}

#[test]
fn test_double_attestation_by_same_slasher_fails() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(12);

    t.client.attest_slash(
        &t.slasher1, &t.offender, &Offense::DoubleSigning, &1_000u32, &hash, &0u64,
    );
    let result = t.client.try_attest_slash(
        &t.slasher1, &t.offender, &Offense::DoubleSigning, &1_000u32, &hash, &0u64,
    );
    assert_eq!(result, Err(Ok(SlashError::AlreadyAttested)));
}

// ─── Appeal Window ────────────────────────────────────────────────────────────

#[test]
fn test_execute_before_appeal_window_fails() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(20);
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::DoubleSigning, &1_000u32, &hash, &0u64,
    );
    let result = t.client.try_execute_slash(&hash);
    assert_eq!(result, Err(Ok(SlashError::AppealWindowOpen)));
}

#[test]
fn test_execute_after_appeal_window_succeeds() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(21);
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::DoubleSigning, &1_000u32, &hash, &0u64,
    );
    t.advance_time(APPEAL_WINDOW + 1);
    t.client.execute_slash(&hash);
    let record = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(record.status, SlashStatus::Executed);
}

#[test]
fn test_raise_appeal_within_window() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(22);
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::MissedDuty, &500u32, &hash, &0u64,
    );
    // Should not panic — event emitted
    t.client.raise_appeal(&t.offender, &hash);
}

#[test]
fn test_raise_appeal_after_window_fails() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(23);
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::MissedDuty, &500u32, &hash, &0u64,
    );
    t.advance_time(APPEAL_WINDOW + 1);
    let result = t.client.try_raise_appeal(&t.offender, &hash);
    assert_eq!(result, Err(Ok(SlashError::AppealWindowClosed)));
}

// ─── Appeal Resolution ────────────────────────────────────────────────────────

#[test]
fn test_appeal_upheld_returns_funds() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(30);
    let before = t.client.get_stake_balance(&t.offender);

    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::DoubleSigning, &1_000u32, &hash, &0u64,
    );
    assert_eq!(t.client.get_stake_balance(&t.offender), before - 1_000);

    t.client.raise_appeal(&t.offender, &hash);
    t.client.resolve_appeal(&hash, &true);

    // Funds returned to offender's stake
    assert_eq!(t.client.get_stake_balance(&t.offender), before);
    let record = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(record.status, SlashStatus::Reversed);
}

#[test]
fn test_appeal_rejected_burns_funds() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(31);
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::FraudProof, &2_000u32, &hash, &0u64,
    );
    t.client.raise_appeal(&t.offender, &hash);
    t.client.resolve_appeal(&hash, &false);

    let record = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(record.status, SlashStatus::AppealRejected);
    // Stake remains reduced — funds burned
    assert_eq!(t.client.get_stake_balance(&t.offender), 8_000i128);
}

#[test]
fn test_cannot_resolve_already_resolved_appeal() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(32);
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::DoubleSigning, &1_000u32, &hash, &0u64,
    );
    t.client.resolve_appeal(&hash, &true);
    let result = t.client.try_resolve_appeal(&hash, &false);
    assert_eq!(result, Err(Ok(SlashError::InvalidState)));
}

// ─── Repeated Offences ────────────────────────────────────────────────────────

#[test]
fn test_repeated_offenses_with_different_evidence_hashes() {
    let t = TestEnv::setup();

    // First offense: 10%
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::MissedDuty, &1_000u32, &t.evidence_hash(40), &0u64,
    );
    assert_eq!(t.client.get_stake_balance(&t.offender), 9_000i128);

    // Second offense: another 10% of remaining stake
    // (stake is now 9_000; 10% = 900)
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::MissedDuty, &1_000u32, &t.evidence_hash(41), &1u64,
    );
    assert_eq!(t.client.get_stake_balance(&t.offender), 8_100i128);
}

#[test]
fn test_repeated_penalties_saturate_period_cap() {
    let t = TestEnv::setup();

    // 30% of 10_000 = 3_000 (within period cap 6_000)
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::FraudProof, &3_000u32, &t.evidence_hash(42), &0u64,
    );
    // 30% of 7_000 = 2_100 (cumulative 5_100, still within cap)
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::FraudProof, &3_000u32, &t.evidence_hash(43), &1u64,
    );

    // 30% of 4_900 = 1_470 would exceed period cap (5_100 + 1_470 > 6_000)
    let result = t.client.try_slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::FraudProof, &3_000u32, &t.evidence_hash(44), &2u64,
    );
    assert_eq!(result, Err(Ok(SlashError::PeriodCapExceeded)));
}

#[test]
fn test_boundary_conditions_at_caps() {
    let t = TestEnv::setup();

    // Exactly reach period cap: 60% of 10_000 is blocked by per-event cap,
    // so use two events to hit period cap exactly.
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::MissedDuty, &5_000u32, &t.evidence_hash(45), &0u64,
    ); // 5_000
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::MissedDuty, &2_000u32, &t.evidence_hash(46), &1u64,
    ); // 1_000

    // Now exactly at 6_000 period cap. Any additional positive slash in same period fails.
    let same_period = t.client.try_slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::MissedDuty, &100u32, &t.evidence_hash(47), &2u64,
    );
    assert_eq!(same_period, Err(Ok(SlashError::PeriodCapExceeded)));

    // Advance into next period to test lifetime boundary at 9_000.
    t.advance_time(86_401);
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::MissedDuty, &3_000u32, &t.evidence_hash(48), &3u64,
    ); // 1_200 => cumulative 7_200
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::MissedDuty, &5_000u32, &t.evidence_hash(49), &4u64,
    ); // 1_400 => cumulative 8_600
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::MissedDuty, &2_000u32, &t.evidence_hash(52), &5u64,
    ); // 280 => cumulative 8_880

    // This slash is still below lifetime cap.
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::MissedDuty, &1_000u32, &t.evidence_hash(53), &6u64,
    ); // 112 => cumulative 8_992

    // Next slash crosses lifetime cap (8_992 + 11 > 9_000)
    let over_lifetime = t.client.try_slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::MissedDuty, &100u32, &t.evidence_hash(60), &7u64,
    );
    assert_eq!(over_lifetime, Err(Ok(SlashError::LifetimeCapExceeded)));
}

#[test]
fn test_minimal_balance_does_not_underflow_or_create_negative_accounting() {
    let t = TestEnv::setup();
    t.client.unstake(&t.offender, &9_999i128);
    assert_eq!(t.client.get_stake_balance(&t.offender), 1i128);

    // 1 bps of 1 rounds to 0 -> rejected.
    let too_small = t.client.try_slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::MissedDuty, &1u32, &t.evidence_hash(54), &0u64,
    );
    assert_eq!(too_small, Err(Ok(SlashError::ZeroPenalty)));
    assert_eq!(t.client.get_stake_balance(&t.offender), 1i128);

    // 100% slash is still bounded by per-event cap (50%), so set 5_000 bps.
    let ok = t.client.try_slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::MissedDuty, &5_000u32, &t.evidence_hash(55), &1u64,
    );
    assert_eq!(ok, Err(Ok(SlashError::ZeroPenalty)));
    assert_eq!(t.client.get_stake_balance(&t.offender), 1i128);
}

#[test]
fn test_simulated_concurrent_triggers_are_capped() {
    let t = TestEnv::setup();

    // Same ledger-time burst from different slashers with unique evidence hashes.
    for (seed, slasher) in [
        (56u8, &t.slasher1),
        (57u8, &t.slasher2),
        (58u8, &t.slasher3),
        (59u8, &t.slasher1),
    ] {
        let _ = t.client.try_slash_with_evidence(
            slasher, &t.offender, &Offense::FraudProof, &2_000u32, &t.evidence_hash(seed), &10u64,
        );
    }

    // First four are accepted: 2_000 + 1_600 + 1_280 + 1_024 = 5_904 < period cap.
    // Next trigger in the same burst would exceed period cap.
    let result = t.client.try_slash_with_evidence(
        &t.slasher2, &t.offender, &Offense::FraudProof, &2_000u32, &t.evidence_hash(61), &10u64,
    );
    assert_eq!(result, Err(Ok(SlashError::PeriodCapExceeded)));
}

#[test]
fn test_execute_then_re_slash_uses_new_hash() {
    let t = TestEnv::setup();
    let hash1 = t.evidence_hash(50);

    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::DoubleSigning, &1_000u32, &hash1, &0u64,
    );
    t.advance_time(APPEAL_WINDOW + 1);
    t.client.execute_slash(&hash1);

    // New offense with a different hash — should succeed
    let hash2 = t.evidence_hash(51);
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::DoubleSigning, &1_000u32, &hash2, &1u64,
    );
    let record = t.client.get_slash_record(&hash2).unwrap();
    assert_eq!(record.status, SlashStatus::Pending);
}

// ─── Edge Cases ───────────────────────────────────────────────────────────────

#[test]
fn test_unknown_evidence_hash_returns_none() {
    let t = TestEnv::setup();
    let result = t.client.get_slash_record(&t.evidence_hash(99));
    assert!(result.is_none());
}

#[test]
fn test_execute_nonexistent_slash_fails() {
    let t = TestEnv::setup();
    let result = t.client.try_execute_slash(&t.evidence_hash(100));
    assert_eq!(result, Err(Ok(SlashError::RecordNotFound)));
}

#[test]
fn test_appeal_nonexistent_slash_fails() {
    let t = TestEnv::setup();
    let result = t.client.try_raise_appeal(&t.offender, &t.evidence_hash(101));
    assert_eq!(result, Err(Ok(SlashError::RecordNotFound)));
}

#[test]
fn test_slash_exactly_at_appeal_deadline_still_open() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(60);
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::MissedDuty, &500u32, &hash, &0u64,
    );
    // Advance to exactly the deadline — window still open (>)
    t.advance_time(APPEAL_WINDOW);
    let result = t.client.try_execute_slash(&hash);
    assert_eq!(result, Err(Ok(SlashError::AppealWindowOpen)));
}

// ─── Keyed Evidence-Hash Replay Protection (O(1) lookup) ─────────────────────

/// A fresh evidence hash must be accepted; reusing the same hash must be rejected.
/// This holds regardless of how many prior slashes have been recorded.
#[test]
fn test_fresh_evidence_hash_accepted_reused_rejected() {
    let t = TestEnv::setup();

    let fresh = t.evidence_hash(110);

    // First use of this hash — must succeed.
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::DoubleSigning, &1_000u32, &fresh, &0u64,
    );

    // Second use of the exact same hash — must be rejected.
    let result = t.client.try_slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::DoubleSigning, &1_000u32, &fresh, &0u64,
    );
    assert_eq!(result, Err(Ok(SlashError::DuplicateEvidence)));
}

/// Replay detection must remain correct after many prior slashes (proves O(1) keyed
/// lookup — not a scan that could time-out as the set grows).
#[test]
fn test_replay_rejection_independent_of_prior_slash_count() {
    let t = TestEnv::setup();

    // Record several slashes with distinct hashes so the used-evidence store is
    // populated. Each slash is small enough to stay within caps.
    for seed in 120u8..124u8 {
        t.client.slash_with_evidence(
            &t.slasher1, &t.offender, &Offense::MissedDuty, &100u32, &t.evidence_hash(seed), &0u64,
        );
    }

    let target = t.evidence_hash(120); // already used in the loop above

    // Reuse of a hash that was consumed earlier must still be rejected.
    let result = t.client.try_slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::MissedDuty, &100u32, &target, &0u64,
    );
    assert_eq!(result, Err(Ok(SlashError::DuplicateEvidence)));

    // A genuinely new hash must still be accepted.
    t.client.slash_with_evidence(
        &t.slasher1, &t.offender, &Offense::MissedDuty, &100u32, &t.evidence_hash(130), &0u64,
    );
}
>>>>>>> origin/main
