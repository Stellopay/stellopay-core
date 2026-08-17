#![cfg(test)]
use slashing_penalty::{
    Offense, SlashError, SlashRecord, SlashStatus, SlashingPenaltyContract,
    SlashingPenaltyContractClient,
};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger, LedgerInfo},
    token::StellarAssetClient,
    Address, BytesN, Env, Map, Symbol,
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

        let admin = Address::generate(&env);
        let slasher1 = Address::generate(&env);
        let slasher2 = Address::generate(&env);
        let slasher3 = Address::generate(&env);
        let offender = Address::generate(&env);
        let token_admin = Address::generate(&env);
        let token = env
            .register_stellar_asset_contract_v2(token_admin)
            .address();
        let token_sac = StellarAssetClient::new(&env, &token);
        token_sac.mint(&offender, &1_000_000i128);

        // Per-event cap: 50%, period cap: 6_000, lifetime cap: 9_000, period: 1 day.
        client.initialize(
            &admin, &token, &2u32, &5_000u32, &6_000i128, &9_000i128, &86_400u64,
        );
        client.add_slasher(&slasher1);
        client.add_slasher(&slasher2);
        client.add_slasher(&slasher3);

        // Give offender an initial staked balance.
        client.stake(&offender, &10_000i128);

        TestEnv {
            env,
            client,
            admin,
            slasher1,
            slasher2,
            slasher3,
            offender,
            token,
        }
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
        &t.admin, &t.token, &2u32, &5_000u32, &6_000i128, &9_000i128, &86_400u64,
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
        &admin, &token, &0u32, &5_000u32, &6_000i128, &9_000i128, &86_400u64,
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

    // quorum = 1 is the minimum valid value and must be stored as-is (not raised to
    // DEFAULT_QUORUM).
    client.initialize(
        &admin, &token, &1u32, &5_000u32, &6_000i128, &9_000i128, &86_400u64,
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
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );
    let result = t.client.try_slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
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
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
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
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );
    t.client.attest_slash(
        &t.slasher2,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
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
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );
    let result = t.client.try_attest_slash(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );
    assert_eq!(result, Err(Ok(SlashError::AlreadyAttested)));
}

// ─── Appeal Window ────────────────────────────────────────────────────────────

#[test]
fn test_execute_before_appeal_window_fails() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(20);
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );
    let result = t.client.try_execute_slash(&hash);
    assert_eq!(result, Err(Ok(SlashError::AppealWindowOpen)));
}

#[test]
fn test_execute_after_appeal_window_succeeds() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(21);
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
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
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &500u32,
        &hash,
        &0u64,
    );
    // Should not panic — event emitted
    t.client.raise_appeal(&t.offender, &hash);
}

#[test]
fn test_raise_appeal_after_window_fails() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(23);
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &500u32,
        &hash,
        &0u64,
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
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
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
        &t.slasher1,
        &t.offender,
        &Offense::FraudProof,
        &2_000u32,
        &hash,
        &0u64,
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
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
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
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &1_000u32,
        &t.evidence_hash(40),
        &0u64,
    );
    assert_eq!(t.client.get_stake_balance(&t.offender), 9_000i128);

    // Second offense: another 10% of remaining stake
    // (stake is now 9_000; 10% = 900)
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &1_000u32,
        &t.evidence_hash(41),
        &1u64,
    );
    assert_eq!(t.client.get_stake_balance(&t.offender), 8_100i128);
}

#[test]
fn test_repeated_penalties_saturate_period_cap() {
    let t = TestEnv::setup();

    // 30% of 10_000 = 3_000 (within period cap 6_000)
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::FraudProof,
        &3_000u32,
        &t.evidence_hash(42),
        &0u64,
    );
    // 30% of 7_000 = 2_100 (cumulative 5_100, still within cap)
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::FraudProof,
        &3_000u32,
        &t.evidence_hash(43),
        &1u64,
    );

    // 30% of 4_900 = 1_470 would exceed period cap (5_100 + 1_470 > 6_000)
    let result = t.client.try_slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::FraudProof,
        &3_000u32,
        &t.evidence_hash(44),
        &2u64,
    );
    assert_eq!(result, Err(Ok(SlashError::PeriodCapExceeded)));
}

#[test]
fn test_boundary_conditions_at_caps() {
    let t = TestEnv::setup();

    // Exactly reach period cap: 60% of 10_000 is blocked by per-event cap,
    // so use two events to hit period cap exactly.
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &5_000u32,
        &t.evidence_hash(45),
        &0u64,
    ); // 5_000
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &2_000u32,
        &t.evidence_hash(46),
        &1u64,
    ); // 1_000

    // Now exactly at 6_000 period cap. Any additional positive slash in same period fails.
    let same_period = t.client.try_slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &100u32,
        &t.evidence_hash(47),
        &2u64,
    );
    assert_eq!(same_period, Err(Ok(SlashError::PeriodCapExceeded)));

    // Advance into next period to test lifetime boundary at 9_000.
    t.advance_time(86_401);
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &3_000u32,
        &t.evidence_hash(48),
        &3u64,
    ); // 1_200 => cumulative 7_200
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &5_000u32,
        &t.evidence_hash(49),
        &4u64,
    ); // 1_400 => cumulative 8_600
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &2_000u32,
        &t.evidence_hash(52),
        &5u64,
    ); // 280 => cumulative 8_880

    // This slash is still below lifetime cap.
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &1_000u32,
        &t.evidence_hash(53),
        &6u64,
    ); // 112 => cumulative 8_992

    // Next slash crosses lifetime cap (8_992 + 11 > 9_000)
    let over_lifetime = t.client.try_slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &100u32,
        &t.evidence_hash(60),
        &7u64,
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
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &1u32,
        &t.evidence_hash(54),
        &0u64,
    );
    assert_eq!(too_small, Err(Ok(SlashError::ZeroPenalty)));
    assert_eq!(t.client.get_stake_balance(&t.offender), 1i128);

    // 100% slash is still bounded by per-event cap (50%), so set 5_000 bps.
    let ok = t.client.try_slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &5_000u32,
        &t.evidence_hash(55),
        &1u64,
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
            slasher,
            &t.offender,
            &Offense::FraudProof,
            &2_000u32,
            &t.evidence_hash(seed),
            &10u64,
        );
    }

    // First four are accepted: 2_000 + 1_600 + 1_280 + 1_024 = 5_904 < period cap.
    // Next trigger in the same burst would exceed period cap.
    let result = t.client.try_slash_with_evidence(
        &t.slasher2,
        &t.offender,
        &Offense::FraudProof,
        &2_000u32,
        &t.evidence_hash(61),
        &10u64,
    );
    assert_eq!(result, Err(Ok(SlashError::PeriodCapExceeded)));
}

#[test]
fn test_execute_then_re_slash_uses_new_hash() {
    let t = TestEnv::setup();
    let hash1 = t.evidence_hash(50);

    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash1,
        &0u64,
    );
    t.advance_time(APPEAL_WINDOW + 1);
    t.client.execute_slash(&hash1);

    // New offense with a different hash — should succeed
    let hash2 = t.evidence_hash(51);
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash2,
        &1u64,
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
    let result = t
        .client
        .try_raise_appeal(&t.offender, &t.evidence_hash(101));
    assert_eq!(result, Err(Ok(SlashError::RecordNotFound)));
}

#[test]
fn test_slash_exactly_at_appeal_deadline_still_open() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(60);
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &500u32,
        &hash,
        &0u64,
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
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &fresh,
        &0u64,
    );

    // Second use of the exact same hash — must be rejected.
    let result = t.client.try_slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &fresh,
        &0u64,
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
            &t.slasher1,
            &t.offender,
            &Offense::MissedDuty,
            &100u32,
            &t.evidence_hash(seed),
            &0u64,
        );
    }

    let target = t.evidence_hash(120); // already used in the loop above

    // Reuse of a hash that was consumed earlier must still be rejected.
    let result = t.client.try_slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &100u32,
        &target,
        &0u64,
    );
    assert_eq!(result, Err(Ok(SlashError::DuplicateEvidence)));

    // A genuinely new hash must still be accepted.
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &100u32,
        &t.evidence_hash(130),
        &0u64,
    );
}

// ─── Double-Execution Guard (issue #938) ──────────────────────────────────────
//
// Requirements addressed:
//   1. A second `execute_slash` call for the same `slash_record_id` (evidence hash) must be
//      rejected with `SlashError::InvalidState` — it must never apply the penalty a second time.
//   2. After a single successful execution, `get_stake_balance` must reflect exactly one penalty
//      deduction (no double-burn, no partial accounting).

/// Requirement 1 — double-execution is rejected.
///
/// Sequence:
///   1. Slash offender via `slash_with_evidence` (status → Pending, stake debited to escrow).
///   2. Advance past the appeal deadline.
///   3. First `execute_slash` call → Ok(()), status → Executed, escrow burned.
///   4. Second `execute_slash` call for the same hash → Err(InvalidState).
///
/// The `InvalidState` error on the second call proves the guard fires before any
/// state mutation, ensuring the penalty cannot be applied twice.
#[test]
fn test_execute_slash_double_execution_is_rejected() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(200);

    // Step 1: initiate the slash.
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32, // 10% of 10_000 stake = 1_000 slashed
        &hash,
        &0u64,
    );

    // Confirm record is Pending.
    let record_pending = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(
        record_pending.status,
        SlashStatus::Pending,
        "record must be Pending before execute"
    );

    // Step 2: advance past the 7-day appeal window.
    t.advance_time(APPEAL_WINDOW + 1);

    // Step 3: first execute — must succeed.
    t.client.execute_slash(&hash);

    // Confirm record is now Executed (terminal state).
    let record_executed = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(
        record_executed.status,
        SlashStatus::Executed,
        "record must be Executed after first execute_slash"
    );

    // Step 4: second execute — the double-execution guard must fire.
    let result = t.client.try_execute_slash(&hash);
    assert_eq!(
        result,
        Err(Ok(SlashError::InvalidState)),
        "second execute_slash must return InvalidState (double-execution guard)"
    );
}

/// Requirement 2 — stake balance reflects exactly one penalty deduction.
///
/// With a 10% penalty on a 10_000 stake, exactly 1_000 tokens must be moved to
/// escrow at slash initiation and burned at execution. After a single successful
/// `execute_slash`, `get_stake_balance` must return 9_000, not 8_000 (two deductions)
/// or 10_000 (no deduction).
///
/// This test also confirms that a second `execute_slash` call does not further
/// reduce the balance, so the accounting remains correct even when a caller
/// mistakenly retries.
#[test]
fn test_execute_slash_stake_balance_reflects_single_execution() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(201);

    // Capture starting balance (10_000 from TestEnv::setup).
    let initial_balance = t.client.get_stake_balance(&t.offender);
    assert_eq!(initial_balance, 10_000i128, "pre-condition: starting stake");

    // Slash at 10% (1_000 bps of 10_000 = 1_000 tokens).
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::FraudProof,
        &1_000u32,
        &hash,
        &0u64,
    );

    // After slash initiation the escrowed amount is deducted from the stake
    // and held in escrow — verify the intermediate balance.
    let balance_after_slash = t.client.get_stake_balance(&t.offender);
    assert_eq!(
        balance_after_slash, 9_000i128,
        "stake must drop by exactly the slashed amount (1_000) after initiation"
    );

    // Advance past appeal window and execute.
    t.advance_time(APPEAL_WINDOW + 1);
    t.client.execute_slash(&hash);

    // After execution the escrowed amount is burned (not returned to stake).
    // Balance must remain 9_000 — exactly one penalty deduction, no refund.
    let balance_after_execute = t.client.get_stake_balance(&t.offender);
    assert_eq!(
        balance_after_execute,
        9_000i128,
        "balance after execute must equal initial minus exactly one penalty (10_000 - 1_000 = 9_000)"
    );

    // Attempt a second execute — rejected by the guard.
    let second_result = t.client.try_execute_slash(&hash);
    assert_eq!(
        second_result,
        Err(Ok(SlashError::InvalidState)),
        "second execute_slash must be rejected"
    );

    // Balance must be unchanged after the rejected second call — no double-burn.
    let balance_after_rejected = t.client.get_stake_balance(&t.offender);
    assert_eq!(
        balance_after_rejected, 9_000i128,
        "stake balance must be unchanged after the rejected double-execution attempt"
    );
}

/// Edge case — double-execution guard fires for attestation-based slashes too.
///
/// Attestation-based slashes go through the same `execute_slash` codepath and
/// the same `Pending → Executed` transition, so the guard must work identically.
#[test]
fn test_attestation_slash_execute_slash_double_execution_is_rejected() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(202);

    // Two attestors meet the quorum of 2.
    t.client.attest_slash(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );
    t.client.attest_slash(
        &t.slasher2,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );

    t.advance_time(APPEAL_WINDOW + 1);

    // First execute — succeeds.
    t.client.execute_slash(&hash);
    assert_eq!(
        t.client.get_slash_record(&hash).unwrap().status,
        SlashStatus::Executed
    );

    // Second execute — must be rejected by the double-execution guard.
    let result = t.client.try_execute_slash(&hash);
    assert_eq!(
        result,
        Err(Ok(SlashError::InvalidState)),
        "double-execution guard must fire for attestation-based slashes"
    );
}

// ─── Maximum Slash Percentage Cap (per_event_bps) ─────────────────────────────

/// Helper to create a fresh environment with a custom per-event bps cap.
struct CustomCapEnv {
    env: Env,
    client: SlashingPenaltyContractClient<'static>,
    admin: Address,
    slasher: Address,
    offender: Address,
    token: Address,
}

impl CustomCapEnv {
    fn new(per_event_bps_cap: u32) -> Self {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, SlashingPenaltyContract);
        let client = SlashingPenaltyContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let slasher = Address::generate(&env);
        let offender = Address::generate(&env);
        let token_admin = Address::generate(&env);
        let token = env
            .register_stellar_asset_contract_v2(token_admin)
            .address();
        let token_sac = StellarAssetClient::new(&env, &token);
        token_sac.mint(&offender, &1_000_000i128);

        client.initialize(
            &admin,
            &token,
            &2u32,
            &per_event_bps_cap,
            &1_000_000i128,
            &10_000_000i128,
            &86_400u64,
        );
        client.add_slasher(&slasher);
        client.stake(&offender, &100_000i128);
        CustomCapEnv {
            env,
            client,
            admin,
            slasher,
            offender,
            token,
        }
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

/// Slashing at exactly the per-event bps cap must succeed.
#[test]
fn test_slash_at_percentage_cap_succeeds() {
    let t = CustomCapEnv::new(2_000); // 20% cap
    let hash = t.evidence_hash(210);

    t.client.slash_with_evidence(
        &t.slasher,
        &t.offender,
        &Offense::FraudProof,
        &2_000u32, // exactly at 20% cap
        &hash,
        &0u64,
    );

    let record = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(record.status, SlashStatus::Pending);
    // 20% of 100_000 = 20_000
    assert_eq!(record.escrowed_amount, 20_000i128);
    assert_eq!(t.client.get_stake_balance(&t.offender), 80_000i128);
}

/// Slashing above the per-event bps cap must be rejected.
#[test]
fn test_slash_above_percentage_cap_fails() {
    let t = CustomCapEnv::new(2_000); // 20% cap
    let hash = t.evidence_hash(211);

    let result = t.client.try_slash_with_evidence(
        &t.slasher,
        &t.offender,
        &Offense::FraudProof,
        &2_001u32, // 1 bps above the 20% cap
        &hash,
        &0u64,
    );
    assert_eq!(result, Err(Ok(SlashError::PenaltyTooHigh)));
    // Stake must be untouched
    assert_eq!(t.client.get_stake_balance(&t.offender), 100_000i128);
}

/// Slash at cap, execute full lifecycle, verify correct amount deducted end-to-end.
#[test]
fn test_execute_slash_respects_percentage_cap() {
    let t = CustomCapEnv::new(1_000); // 10% cap
    let hash = t.evidence_hash(212);

    // Slash exactly at 10% cap
    t.client.slash_with_evidence(
        &t.slasher,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );

    let balance_after_slash = t.client.get_stake_balance(&t.offender);
    assert_eq!(balance_after_slash, 90_000i128); // 100_000 - 10_000

    // Execute after appeal window
    t.advance_time(APPEAL_WINDOW + 1);
    t.client.execute_slash(&hash);

    // Balance must remain 90_000 (escrow burned, not returned)
    assert_eq!(t.client.get_stake_balance(&t.offender), 90_000i128);
    let record = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(record.status, SlashStatus::Executed);
    assert_eq!(record.escrowed_amount, 10_000i128);
}

/// Attestation-based slash at cap must succeed.
#[test]
fn test_attestation_slash_at_percentage_cap_succeeds() {
    let t = CustomCapEnv::new(3_000); // 30% cap
    let hash = t.evidence_hash(213);

    t.client.attest_slash(
        &t.slasher,
        &t.offender,
        &Offense::DoubleSigning,
        &3_000u32, // exactly at 30% cap
        &hash,
        &0u64,
    );
    // Quorum of 2 — second attestor also needed
    let admin_addr = t.admin.clone();
    t.client.add_slasher(&admin_addr);
    t.client.attest_slash(
        &admin_addr,
        &t.offender,
        &Offense::DoubleSigning,
        &3_000u32,
        &hash,
        &0u64,
    );

    let record = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(record.status, SlashStatus::Pending);
    // 30% of 100_000 = 30_000
    assert_eq!(record.escrowed_amount, 30_000i128);
}

/// Attestation-based slash above cap must be rejected.
#[test]
fn test_attestation_slash_above_percentage_cap_fails() {
    let t = CustomCapEnv::new(3_000); // 30% cap
    let hash = t.evidence_hash(214);

    let result = t.client.try_attest_slash(
        &t.slasher,
        &t.offender,
        &Offense::FraudProof,
        &3_001u32, // 1 bps above the 30% cap
        &hash,
        &0u64,
    );
    assert_eq!(result, Err(Ok(SlashError::PenaltyTooHigh)));
}

/// Zero per_event_bps_cap must be rejected at initialization.
#[test]
fn test_zero_per_event_bps_cap_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SlashingPenaltyContract);
    let client = SlashingPenaltyContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);

    let result = client.try_initialize(
        &admin,
        &token,
        &2u32,
        &0u32,
        &10_000i128,
        &50_000i128,
        &86_400u64,
    );
    assert_eq!(result, Err(Ok(SlashError::InvalidConfig)));
}

// ─── Removed-Slasher Attestation Rejection (point-in-time authorisation) ─────
//
// Security requirement: `attest_slash` must gate authorisation against the
// *current* slasher set at the moment the call is made, not at the moment the
// slash was first proposed.  Once `remove_slasher` is called, that address is
// no longer in the authorised set and every subsequent `attest_slash` from that
// address must be rejected with `SlashError::Unauthorized`, even when:
//
//   a) The address was a legitimate slasher when it submitted an earlier
//      attestation for the same evidence hash.
//   b) The evidence hash is still in a `Pending` state.
//
// At the same time, attestations that were accepted *before* removal must
// remain counted in `record.attestors` and must contribute toward the quorum
// required by `execute_slash`.
//
// Point-in-time model
// -------------------
// An attestation is accepted or rejected based on whether the attestor is in
// `get_slashers()` **at the ledger in which `attest_slash` is invoked**.
// Removal is retroactive in the sense that it prevents future attestations;
// it is *not* retroactive in the sense of invalidating already-recorded ones.

/// A slasher removed via `remove_slasher` must be rejected by `attest_slash`
/// even when it was a legitimate slasher at slash-creation time.
///
/// Sequence:
///   1. slasher1 creates the slash record (first attestation).
///   2. Admin removes slasher1 via `remove_slasher`.
///   3. slasher1 tries to countersign a *different* evidence hash — must fail with
///      `Unauthorized` (slasher1 is no longer in the authorised set).
///   4. slasher1 tries to attest the *original* hash (as a countersign) — also
///      must fail with `Unauthorized`.
#[test]
fn test_removed_slasher_attestation_rejected() {
    let t = TestEnv::setup();

    // Step 1: slasher1 creates the slash record.
    let hash = t.evidence_hash(230);
    t.client.attest_slash(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );
    // Confirm slasher1's attestation was recorded.
    let record_after_first = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(
        record_after_first.attestors.len(),
        1u32,
        "one attestor recorded"
    );

    // Step 2: admin removes slasher1.
    t.client.remove_slasher(&t.slasher1);
    assert!(
        !t.client.get_slashers().contains(&t.slasher1),
        "slasher1 must no longer be in the authorised set"
    );

    // Step 3: slasher1 attempts to create a new slash (first attestor on a fresh hash)
    //         — must be rejected because it is no longer a slasher.
    let hash_new = t.evidence_hash(231);
    let result_new = t.client.try_attest_slash(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash_new,
        &0u64,
    );
    assert_eq!(
        result_new,
        Err(Ok(SlashError::Unauthorized)),
        "removed slasher must not be able to create a new attestation"
    );

    // Step 4: slasher1 attempts to countersign the original hash — also rejected.
    let result_countersign = t.client.try_attest_slash(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );
    assert_eq!(
        result_countersign,
        Err(Ok(SlashError::Unauthorized)),
        "removed slasher must not be able to countersign an existing pending record"
    );
}

/// Attestations made *before* removal must remain valid toward `execute_slash` quorum.
///
/// This confirms the point-in-time model: removal prevents *future* attestations
/// but does not retroactively invalidate ones that were accepted while the slasher
/// was authorised.
///
/// Sequence:
///   1. slasher1 and slasher2 each attest (quorum = 2 → satisfied).
///   2. Admin removes slasher1.
///   3. Advance past the appeal window.
///   4. `execute_slash` must succeed because the two pre-removal attestations
///      still count — quorum is already recorded in `record.attestors`.
#[test]
fn test_pre_removal_attestations_count_toward_quorum() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(232);

    // Step 1: two slashers attest — meets quorum of 2.
    t.client.attest_slash(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );
    t.client.attest_slash(
        &t.slasher2,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );
    let record_before_removal = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(
        record_before_removal.attestors.len(),
        2u32,
        "both pre-removal attestations must be recorded"
    );

    // Step 2: admin removes slasher1 after the attestations are in.
    t.client.remove_slasher(&t.slasher1);

    // Step 3: advance past the appeal deadline.
    t.advance_time(APPEAL_WINDOW + 1);

    // Step 4: execute_slash must succeed — quorum is met by the recorded attestors
    //         regardless of their current role status.
    t.client.execute_slash(&hash);
    let record_executed = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(
        record_executed.status,
        SlashStatus::Executed,
        "slash must execute successfully when quorum was met before slasher removal"
    );
    // Stake balance must reflect exactly one penalty deduction (10% of 10_000 = 1_000).
    assert_eq!(
        t.client.get_stake_balance(&t.offender),
        9_000i128,
        "stake balance must reflect the single penalty, not be affected by the removal"
    );
}

/// per_event_bps_cap above MAX_PENALTY_BPS must be rejected.
#[test]
fn test_per_event_bps_cap_exceeds_max_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SlashingPenaltyContract);
    let client = SlashingPenaltyContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);

    let result = client.try_initialize(
        &admin,
        &token,
        &2u32,
        &5_001u32,
        &10_000i128,
        &50_000i128,
        &86_400u64,
    );
    assert_eq!(result, Err(Ok(SlashError::InvalidConfig)));
}

/// Slashing at the hard MAX_PENALTY_BPS (5_000 = 50%) through
/// the full slash-with-evidence path must succeed.
#[test]
fn test_max_bps_boundary_slash_succeeds() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(220);

    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::FraudProof,
        &5_000u32, // MAX_PENALTY_BPS = 50%
        &hash,
        &0u64,
    );
    let record = t.client.get_slash_record(&hash).unwrap();
    // 50% of 10_000 = 5_000
    assert_eq!(record.escrowed_amount, 5_000i128);
    assert_eq!(t.client.get_stake_balance(&t.offender), 5_000i128);
}

/// Lower the per-event bps cap via set_penalty_caps and verify that
/// subsequent slashes are gated by the updated cap.
#[test]
fn test_update_cap_then_enforce() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SlashingPenaltyContract);
    let client = SlashingPenaltyContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let slasher = Address::generate(&env);
    let offender = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let token_sac = StellarAssetClient::new(&env, &token);
    token_sac.mint(&offender, &1_000_000i128);

    // Initialize with 40% cap.
    client.initialize(
        &admin,
        &token,
        &2u32,
        &4_000u32,
        &1_000_000i128,
        &10_000_000i128,
        &86_400u64,
    );
    client.add_slasher(&slasher);
    client.stake(&offender, &100_000i128);

    // Lower cap to 20% (2_000 bps).
    client.set_penalty_caps(&2_000u32, &1_000_000i128, &10_000_000i128, &86_400u64);

    // 15% is below the new 20% cap — must succeed.
    let hash_ok = BytesN::from_array(&env, &[215u8; 32]);
    client.slash_with_evidence(
        &slasher,
        &offender,
        &Offense::MissedDuty,
        &1_500u32,
        &hash_ok,
        &0u64,
    );
    let record = client.get_slash_record(&hash_ok).unwrap();
    assert_eq!(record.escrowed_amount, 15_000i128);

    // 25% exceeds the new 20% cap — must be rejected.
    let result = client.try_slash_with_evidence(
        &slasher,
        &offender,
        &Offense::FraudProof,
        &2_500u32,
        &BytesN::from_array(&env, &[216u8; 32]),
        &0u64,
    );
    assert_eq!(result, Err(Ok(SlashError::PenaltyTooHigh)));
}

// ─── Evidence-Hash-Mismatch Rejection ─────────────────────────────────────────
//
// Requirements addressed:
//   1. The slash-execution path re-validates the evidence hash against the originally
//      attested reference recorded at attest_slash / slash_with_evidence time.
//   2. A submitted evidence hash that does not match the recorded reference is
//      rejected with SlashError::EvidenceHashMismatch.
//   3. Execution succeeds when the evidence hash matches exactly.
//
// Security invariant:
//   The evidence_hash is both the map key AND a field inside SlashRecord. Under
//   normal operation they are always equal. The explicit check in execute_slash
//   and attest_slash is defense-in-depth against storage-corruption edge cases.

/// Positive test: execution succeeds when the evidence hash matches the recorded reference.
///
/// This establishes the baseline — a correctly-matched hash flows through the entire
/// slash lifecycle without triggering EvidenceHashMismatch.
#[test]
fn test_execute_slash_matching_evidence_hash_succeeds() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(230);

    // Create a slash record via slash_with_evidence (hash = key = stored field).
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );

    // Advance past the appeal window.
    t.advance_time(APPEAL_WINDOW + 1);

    // Execute with the SAME hash — must succeed because the submitted hash matches
    // the one stored in the record at creation time.
    t.client.execute_slash(&hash);

    let record = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(
        record.status,
        SlashStatus::Executed,
        "slash must be Executed when evidence hash matches"
    );
    assert_eq!(
        record.evidence_hash, hash,
        "record.evidence_hash must equal the hash used for lookup"
    );
}

/// Positive test: attest_slash followed by execute_slash with matching hash succeeds.
///
/// Attestation-based slashes go through the same execute_slash codepath, so the
/// hash-match invariant must hold for them as well.
#[test]
fn test_execute_slash_attestation_matching_evidence_hash_succeeds() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(231);

    // Create attestation record (two slashers meet quorum of 2).
    t.client.attest_slash(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );
    t.client.attest_slash(
        &t.slasher2,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );

    // Advance past appeal window.
    t.advance_time(APPEAL_WINDOW + 1);

    // Execute with matching hash — must succeed.
    t.client.execute_slash(&hash);

    let record = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(record.status, SlashStatus::Executed);
}

/// Negative test: submitting a different hash at execute time produces
/// RecordNotFound (because the record is keyed by a different hash).
///
/// Even though there EXISTS a record, looking it up with a non-matching hash
/// returns a different key — RecordNotFound, not EvidenceHashMismatch.
/// This test documents that failure mode explicitly in context.
#[test]
fn test_execute_slash_wrong_hash_key_returns_record_not_found() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(232);
    let other_hash = t.evidence_hash(233);

    // Create a record at `hash`.
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );

    // Advance past appeal window.
    t.advance_time(APPEAL_WINDOW + 1);

    // Try to execute with `other_hash` — different key, so the record isn't found.
    let result = t.client.try_execute_slash(&other_hash);
    assert_eq!(
        result,
        Err(Ok(SlashError::RecordNotFound)),
        "a wrong hash key must produce RecordNotFound since the record is keyed by the original hash"
    );

    // Sanity check: the record at `hash` is still intact.
    let record = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(record.status, SlashStatus::Pending);
}

/// Negative test: direct storage corruption that causes a mismatch between the map
/// key and the stored evidence_hash field triggers EvidenceHashMismatch.
///
/// This proves the defense-in-depth check in execute_slash works even when the map
/// key points to a valid record whose evidence_hash field has diverged from the key.
#[test]
fn test_execute_slash_rejects_storage_corrupted_evidence_hash() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, SlashingPenaltyContract);
    let client = SlashingPenaltyContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let slasher1 = Address::generate(&env);
    let offender = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let token_sac = StellarAssetClient::new(&env, &token);
    token_sac.mint(&offender, &1_000_000i128);

    client.initialize(
        &admin, &token, &2u32, &5_000u32, &6_000i128, &9_000i128, &86_400u64,
    );
    client.add_slasher(&slasher1);
    client.stake(&offender, &10_000i128);

    let hash = BytesN::from_array(&env, &[1; 32]);
    let wrong_hash = BytesN::from_array(&env, &[2; 32]);

    // Step 1: create a slash record at `hash`.
    client.slash_with_evidence(
        &slasher1,
        &offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );

    // Step 2: advance past the appeal window.
    let current = env.ledger().timestamp();
    env.ledger().set(LedgerInfo {
        timestamp: current + APPEAL_WINDOW + 1,
        ..env.ledger().get()
    });

    // Step 3: directly corrupt the stored record — change its evidence_hash field
    // to `wrong_hash` while keeping the map key as `hash`.
    env.as_contract(&contract_id, || {
        let rec_key: Symbol = symbol_short!("SLASHREC");
        let mut records: Map<BytesN<32>, SlashRecord> =
            env.storage().instance().get(&rec_key).unwrap();
        if let Some(mut record) = records.get(hash.clone()) {
            record.evidence_hash = wrong_hash.clone();
            records.set(hash.clone(), record);
            env.storage().instance().set(&rec_key, &records);
        }
    });

    // Step 4: try to execute with `hash` — the map key finds the record, but the
    // stored evidence_hash (`wrong_hash`) does not match the submitted hash (`hash`).
    let result = client.try_execute_slash(&hash);
    assert_eq!(
        result,
        Err(Ok(SlashError::EvidenceHashMismatch)),
        "execute_slash must reject a record whose evidence_hash field diverges from the map key"
    );

    // Step 5: confirm the record was NOT modified (status still Pending, escrow intact).
    let record = client.get_slash_record(&hash).unwrap();
    assert_eq!(
        record.status,
        SlashStatus::Pending,
        "record must remain Pending when execution is rejected"
    );
    assert_eq!(
        record.evidence_hash, wrong_hash,
        "corrupted evidence_hash should still be visible in storage"
    );
    assert!(record.escrowed_amount > 0, "escrow must be intact");
}

/// Negative test: attest_slash countersign path rejects a mismatched evidence hash
/// when the record's stored hash has been corrupted away from the map key.
#[test]
fn test_attest_slash_countersign_rejects_mismatched_evidence_hash() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, SlashingPenaltyContract);
    let client = SlashingPenaltyContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let slasher1 = Address::generate(&env);
    let slasher2 = Address::generate(&env);
    let offender = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let token_sac = StellarAssetClient::new(&env, &token);
    token_sac.mint(&offender, &1_000_000i128);

    client.initialize(
        &admin, &token, &2u32, &5_000u32, &6_000i128, &9_000i128, &86_400u64,
    );
    client.add_slasher(&slasher1);
    client.add_slasher(&slasher2);
    client.stake(&offender, &10_000i128);

    let hash = BytesN::from_array(&env, &[10; 32]);
    let wrong_hash = BytesN::from_array(&env, &[20; 32]);

    // Step 1: first attestor creates a record at `hash`.
    client.attest_slash(
        &slasher1,
        &offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );

    // Step 2: corrupt the stored record's evidence_hash so it diverges from the
    // map key.
    env.as_contract(&contract_id, || {
        let rec_key: Symbol = symbol_short!("SLASHREC");
        let mut records: Map<BytesN<32>, SlashRecord> =
            env.storage().instance().get(&rec_key).unwrap();
        if let Some(mut record) = records.get(hash.clone()) {
            record.evidence_hash = wrong_hash.clone();
            records.set(hash.clone(), record);
            env.storage().instance().set(&rec_key, &records);
        }
    });

    // Step 3: second attestor tries to countersign with `hash` (the correct key).
    // The map lookup succeeds (record exists), but the stored evidence_hash no
    // longer matches — must be rejected.
    let result = client.try_attest_slash(
        &slasher2,
        &offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );
    assert_eq!(
        result,
        Err(Ok(SlashError::EvidenceHashMismatch)),
        "attest_slash countersign must reject a record whose evidence_hash diverges from the submitted hash"
    );

    // Step 4: the first attestor's record remains intact (no double-attestation).
    let record = client.get_slash_record(&hash).unwrap();
    assert_eq!(
        record.attestors.len(),
        1,
        "only slasher1 should be an attestor"
    );
    assert!(
        record.attestors.contains(&slasher1),
        "slasher1 must still be the sole attestor"
    );
}

/// Verify that the EvidenceHashMismatch error is not triggered during normal
/// attest_slash flow (defense-in-depth does not break the happy path).
#[test]
fn test_attest_slash_countersign_matching_hash_succeeds() {
    let t = TestEnv::setup();
    let hash = t.evidence_hash(240);

    // First attestor creates the record.
    t.client.attest_slash(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );

    // Second attestor countersigns with the same hash — must succeed
    // (no EvidenceHashMismatch).
    t.client.attest_slash(
        &t.slasher2,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );

    // Two attestors recorded.
    let record = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(record.attestors.len(), 2);
    assert!(record.attestors.contains(&t.slasher1));
    assert!(record.attestors.contains(&t.slasher2));
}

// ─── Slasher-Set Deduplication (issue #937) ───────────────────────────────────
//
// Requirements addressed (issue #937):
//
//   1. Adding a slasher, removing it, and re-adding it must leave exactly ONE
//      entry for that address in get_slashers() — no duplicates.
//
//   2. get_quorum() must be completely unaffected by any add/remove/re-add cycle.
//      The quorum threshold is stored independently and is never derived from the
//      runtime length of the slasher list.
//
//   3. add_slasher is idempotent: calling it N times for the same address still
//      results in exactly one entry.
//
//   4. remove_slasher is idempotent: calling it for an address that is not in the
//      set is a no-op (returns Ok(()), set is unchanged).
//
//   5. The set-size invariant: after any sequence of add/remove/re-add operations
//      the slasher list length matches the number of *unique* currently-active
//      slashers, with no phantom or duplicate entries.
//
//   6. A re-added slasher is immediately functional: it can attest slashes and
//      those attestations count toward quorum.
//
// All evidence hashes in this section use seeds 250+ to avoid collisions with
// the seeds used in the rest of the test file (which go up to ~240).

/// Requirement 1 — add/remove/re-add cycle leaves exactly one entry.
///
/// Sequence:
///   1. Create a fresh address (not yet a slasher).
///   2. add_slasher(A) → A appears exactly once in get_slashers().
///   3. remove_slasher(A) → A is gone from get_slashers().
///   4. add_slasher(A) → A appears exactly once again.
///
/// Invariant asserted after each step: count_in_list(A) is either 0 or 1, never >1.
#[test]
fn test_add_remove_readd_no_duplicate_entry() {
    let t = TestEnv::setup();
    let target = Address::generate(&t.env);

    // Step 1: not yet added — must not appear.
    let slashers_before = t.client.get_slashers();
    assert!(
        !slashers_before.contains(&target),
        "address must not be in slasher set before being added"
    );

    // Step 2: first add.
    t.client.add_slasher(&target);
    let slashers_after_add = t.client.get_slashers();
    let count_after_add = slashers_after_add.iter().filter(|s| *s == target).count();
    assert_eq!(
        count_after_add, 1,
        "exactly one entry expected after first add_slasher"
    );

    // Step 3: remove.
    t.client.remove_slasher(&target);
    let slashers_after_remove = t.client.get_slashers();
    assert!(
        !slashers_after_remove.contains(&target),
        "address must not appear in slasher set after remove_slasher"
    );

    // Step 4: re-add.
    t.client.add_slasher(&target);
    let slashers_after_readd = t.client.get_slashers();
    let count_after_readd = slashers_after_readd.iter().filter(|s| *s == target).count();
    assert_eq!(
        count_after_readd, 1,
        "exactly one entry expected after re-add — no duplicate must appear"
    );
}

/// Requirement 1 (extended) — multiple add/remove/re-add cycles still leave
/// exactly one entry.
///
/// This catches any scenario where the dedup guard is bypassed on the second or
/// later re-add (e.g., a guard that only applies on the very first insertion).
#[test]
fn test_multiple_add_remove_readd_cycles_no_duplicate() {
    let t = TestEnv::setup();
    let target = Address::generate(&t.env);

    for cycle in 0u8..5 {
        t.client.add_slasher(&target);
        let after_add = t.client.get_slashers();
        let count = after_add.iter().filter(|s| *s == target).count();
        assert_eq!(
            count, 1,
            "cycle {cycle}: exactly one entry expected after add_slasher"
        );

        t.client.remove_slasher(&target);
        let after_remove = t.client.get_slashers();
        assert!(
            !after_remove.contains(&target),
            "cycle {cycle}: address must be absent after remove_slasher"
        );
    }

    // Final re-add after all cycles.
    t.client.add_slasher(&target);
    let final_slashers = t.client.get_slashers();
    let final_count = final_slashers.iter().filter(|s| *s == target).count();
    assert_eq!(
        final_count, 1,
        "exactly one entry expected after final re-add following 5 cycles"
    );
}

/// Requirement 3 — add_slasher is idempotent: N consecutive calls for the same
/// address must yield exactly one entry (not N entries).
#[test]
fn test_add_slasher_idempotent_multiple_calls() {
    let t = TestEnv::setup();
    let target = Address::generate(&t.env);

    // Call add_slasher 5 times for the same address.
    for _ in 0..5 {
        t.client.add_slasher(&target);
    }

    let slashers = t.client.get_slashers();
    let count = slashers.iter().filter(|s| *s == target).count();
    assert_eq!(
        count, 1,
        "add_slasher must be idempotent: 5 calls must still yield exactly one entry"
    );
}

/// Requirement 3 (variant) — calling add_slasher twice for an already-present
/// slasher (one that was added during TestEnv::setup) still leaves exactly one entry.
#[test]
fn test_add_existing_slasher_is_noop() {
    let t = TestEnv::setup();

    // slasher1 was already added in setup.
    let before_count = t
        .client
        .get_slashers()
        .iter()
        .filter(|s| *s == t.slasher1)
        .count();
    assert_eq!(before_count, 1, "pre-condition: slasher1 present once");

    // Adding it again must be a no-op.
    t.client.add_slasher(&t.slasher1);

    let after_count = t
        .client
        .get_slashers()
        .iter()
        .filter(|s| *s == t.slasher1)
        .count();
    assert_eq!(
        after_count, 1,
        "re-adding an existing slasher must not create a duplicate"
    );
}

/// Requirement 4 — remove_slasher is idempotent: calling it for an address that
/// is not in the set must succeed (Ok(())) and leave the set unchanged.
#[test]
fn test_remove_nonexistent_slasher_is_noop() {
    let t = TestEnv::setup();
    let not_a_slasher = Address::generate(&t.env);

    // Sanity: the address is not in the set.
    assert!(!t.client.get_slashers().contains(&not_a_slasher));

    let size_before = t.client.get_slashers().len();

    // Removing an address that was never added must not error.
    t.client.remove_slasher(&not_a_slasher);

    let size_after = t.client.get_slashers().len();
    assert_eq!(
        size_after, size_before,
        "remove_slasher on a non-member must not change the set size"
    );
}

/// Requirement 4 (variant) — calling remove_slasher twice for the same address
/// must be idempotent: the second call must not error and the set must remain
/// correct.
#[test]
fn test_remove_slasher_twice_is_idempotent() {
    let t = TestEnv::setup();
    let target = Address::generate(&t.env);

    t.client.add_slasher(&target);
    assert!(t.client.get_slashers().contains(&target));

    // First removal.
    t.client.remove_slasher(&target);
    assert!(!t.client.get_slashers().contains(&target));

    // Second removal — must be a no-op, not an error.
    t.client.remove_slasher(&target);
    assert!(
        !t.client.get_slashers().contains(&target),
        "double-remove must leave the address absent (idempotent)"
    );
}

/// Requirement 5 — set-size invariant: the slasher list length always equals
/// the number of unique currently-active slashers.
///
/// After TestEnv::setup three slashers are present. We exercise multiple
/// add/remove/re-add operations and verify that the list length tracks the
/// exact number of unique members at each step.
#[test]
fn test_slasher_set_size_invariant() {
    let t = TestEnv::setup();

    // Baseline: 3 slashers from setup.
    let baseline = t.client.get_slashers().len();
    assert_eq!(baseline, 3u32, "pre-condition: 3 slashers after setup");

    // Add a new slasher → size 4.
    let extra = Address::generate(&t.env);
    t.client.add_slasher(&extra);
    assert_eq!(
        t.client.get_slashers().len(),
        4u32,
        "size must be 4 after adding extra"
    );

    // Add extra again (idempotent) → size stays 4.
    t.client.add_slasher(&extra);
    assert_eq!(
        t.client.get_slashers().len(),
        4u32,
        "size must remain 4 after redundant add"
    );

    // Remove extra → size 3.
    t.client.remove_slasher(&extra);
    assert_eq!(
        t.client.get_slashers().len(),
        3u32,
        "size must return to 3 after removing extra"
    );

    // Remove extra again (idempotent) → size stays 3.
    t.client.remove_slasher(&extra);
    assert_eq!(
        t.client.get_slashers().len(),
        3u32,
        "size must stay 3 after redundant remove"
    );

    // Re-add extra → size 4 again.
    t.client.add_slasher(&extra);
    assert_eq!(
        t.client.get_slashers().len(),
        4u32,
        "size must be 4 after re-adding extra"
    );

    // Remove slasher1 → size 3.
    t.client.remove_slasher(&t.slasher1);
    assert_eq!(
        t.client.get_slashers().len(),
        3u32,
        "size must be 3 after removing slasher1"
    );

    // Re-add slasher1 → size back to 4.
    t.client.add_slasher(&t.slasher1);
    assert_eq!(
        t.client.get_slashers().len(),
        4u32,
        "size must be 4 after re-adding slasher1"
    );
}

/// Requirement 2 — quorum threshold is unaffected by add/remove/re-add cycles.
///
/// The quorum value is stored in a separate `QUORUM` key and is never derived
/// from the runtime length of the slasher list. Any add/remove/re-add sequence
/// must leave get_quorum() returning the same value it had at initialisation.
#[test]
fn test_quorum_unaffected_by_add_remove_readd_cycle() {
    let t = TestEnv::setup();

    let quorum_initial = t.client.get_quorum();
    assert_eq!(
        quorum_initial, 2u32,
        "pre-condition: quorum is 2 after setup"
    );

    let target = Address::generate(&t.env);

    // Perform multiple add/remove/re-add cycles.
    for _ in 0..3 {
        t.client.add_slasher(&target);
        assert_eq!(
            t.client.get_quorum(),
            quorum_initial,
            "quorum must not change after add_slasher"
        );

        t.client.remove_slasher(&target);
        assert_eq!(
            t.client.get_quorum(),
            quorum_initial,
            "quorum must not change after remove_slasher"
        );
    }

    // Final re-add.
    t.client.add_slasher(&target);
    assert_eq!(
        t.client.get_quorum(),
        quorum_initial,
        "quorum must not change after final re-add"
    );

    // Also remove one of the original slashers to vary the set size.
    t.client.remove_slasher(&t.slasher1);
    assert_eq!(
        t.client.get_quorum(),
        quorum_initial,
        "quorum must not change after removing an original slasher"
    );

    // Re-add the original slasher.
    t.client.add_slasher(&t.slasher1);
    assert_eq!(
        t.client.get_quorum(),
        quorum_initial,
        "quorum must not change after re-adding an original slasher"
    );
}

/// Requirement 2 (end-to-end) — quorum accounting is correct after a full
/// add/remove/re-add cycle: attestation-based slashes still require exactly
/// `quorum_threshold` distinct signatures.
///
/// Scenario:
///   - quorum = 2 (set at initialisation).
///   - Perform add/remove/re-add on slasher1.
///   - One attestation from slasher1 (re-added) must be insufficient.
///   - Two attestations from slasher1 + slasher2 must satisfy quorum.
#[test]
fn test_quorum_enforcement_after_readd_cycle() {
    let t = TestEnv::setup();

    // add/remove/re-add cycle for slasher1.
    t.client.remove_slasher(&t.slasher1);
    t.client.add_slasher(&t.slasher1);

    // Confirm slasher1 is present exactly once and quorum is still 2.
    let count = t
        .client
        .get_slashers()
        .iter()
        .filter(|s| *s == t.slasher1)
        .count();
    assert_eq!(
        count, 1,
        "slasher1 must be present exactly once after re-add"
    );
    assert_eq!(t.client.get_quorum(), 2u32, "quorum must still be 2");

    let hash = t.evidence_hash(250);

    // One attestation — quorum not yet met.
    t.client.attest_slash(
        &t.slasher1,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );

    t.advance_time(APPEAL_WINDOW + 1);
    let result_one_sig = t.client.try_execute_slash(&hash);
    assert_eq!(
        result_one_sig,
        Err(Ok(SlashError::QuorumNotMet)),
        "one attestation must not meet quorum of 2 even after re-add cycle"
    );

    // Second attestation — quorum met; execution must succeed.
    t.client.attest_slash(
        &t.slasher2,
        &t.offender,
        &Offense::DoubleSigning,
        &1_000u32,
        &hash,
        &0u64,
    );

    t.client.execute_slash(&hash);
    let record = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(
        record.status,
        SlashStatus::Executed,
        "slash must execute after quorum is met following a re-add cycle"
    );
    // Stake must reflect exactly one penalty deduction (10% of 10_000 = 1_000).
    assert_eq!(
        t.client.get_stake_balance(&t.offender),
        9_000i128,
        "stake must be reduced by exactly one penalty after execution"
    );
}

/// Requirement 6 — a re-added slasher is immediately functional for attestation.
///
/// After an add/remove/re-add cycle the re-added slasher must be able to:
///   a) Create a new slash record (first attestor).
///   b) Have that attestation count toward quorum.
#[test]
fn test_readded_slasher_can_attest_and_contributes_to_quorum() {
    let t = TestEnv::setup();

    let new_slasher = Address::generate(&t.env);

    // add/remove/re-add the new slasher.
    t.client.add_slasher(&new_slasher);
    t.client.remove_slasher(&new_slasher);
    t.client.add_slasher(&new_slasher);

    // Confirm present exactly once.
    let count = t
        .client
        .get_slashers()
        .iter()
        .filter(|s| *s == new_slasher)
        .count();
    assert_eq!(count, 1, "re-added slasher must be present exactly once");

    let hash = t.evidence_hash(251);

    // re-added slasher creates the slash record.
    t.client.attest_slash(
        &new_slasher,
        &t.offender,
        &Offense::MissedDuty,
        &500u32,
        &hash,
        &0u64,
    );

    let record_after_first = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(
        record_after_first.attestors.len(),
        1u32,
        "re-added slasher's attestation must be recorded"
    );
    assert!(
        record_after_first.attestors.contains(&new_slasher),
        "new_slasher must appear in attestors"
    );

    // Second attestor (slasher1) completes quorum.
    t.client.attest_slash(
        &t.slasher1,
        &t.offender,
        &Offense::MissedDuty,
        &500u32,
        &hash,
        &0u64,
    );

    t.advance_time(APPEAL_WINDOW + 1);
    t.client.execute_slash(&hash);

    let record_executed = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(
        record_executed.status,
        SlashStatus::Executed,
        "slash must execute when re-added slasher contributed to quorum"
    );
}

/// Invariant — the other slashers in the set are unaffected by add/remove/re-add
/// operations on a different address.
///
/// This guards against an implementation bug where remove_slasher could
/// accidentally shift or drop adjacent entries (e.g. off-by-one in a filter loop).
#[test]
fn test_peer_slashers_unaffected_by_cycle_on_different_address() {
    let t = TestEnv::setup();

    let target = Address::generate(&t.env);

    // Add, remove, re-add target and verify slasher1/slasher2/slasher3 are intact.
    t.client.add_slasher(&target);
    t.client.remove_slasher(&target);
    t.client.add_slasher(&target);

    let slashers = t.client.get_slashers();
    assert!(
        slashers.contains(&t.slasher1),
        "slasher1 must still be present after cycle on another address"
    );
    assert!(
        slashers.contains(&t.slasher2),
        "slasher2 must still be present after cycle on another address"
    );
    assert!(
        slashers.contains(&t.slasher3),
        "slasher3 must still be present after cycle on another address"
    );
    assert!(
        slashers.contains(&target),
        "target must be present after re-add"
    );

    // Each must appear exactly once.
    for addr in [&t.slasher1, &t.slasher2, &t.slasher3, &target] {
        let count = slashers.iter().filter(|s| *s == *addr).count();
        assert_eq!(
            count, 1,
            "each slasher must appear exactly once after cycle on another address"
        );
    }
}

/// Edge case — removing all slashers one-by-one and then re-adding them must
/// restore the full set without any duplicates.
#[test]
fn test_remove_all_then_readd_all_no_duplicates() {
    let t = TestEnv::setup();

    // Remove all three original slashers.
    t.client.remove_slasher(&t.slasher1);
    t.client.remove_slasher(&t.slasher2);
    t.client.remove_slasher(&t.slasher3);
    assert_eq!(
        t.client.get_slashers().len(),
        0u32,
        "slasher set must be empty after removing all slashers"
    );

    // Re-add all three.
    t.client.add_slasher(&t.slasher1);
    t.client.add_slasher(&t.slasher2);
    t.client.add_slasher(&t.slasher3);

    let slashers = t.client.get_slashers();
    assert_eq!(
        slashers.len(),
        3u32,
        "slasher set must have exactly 3 entries after re-adding all"
    );

    // Each must appear exactly once.
    for addr in [&t.slasher1, &t.slasher2, &t.slasher3] {
        let count = slashers.iter().filter(|s| *s == *addr).count();
        assert_eq!(count, 1, "each slasher must appear exactly once");
    }

    // Quorum must be unchanged.
    assert_eq!(
        t.client.get_quorum(),
        2u32,
        "quorum must still be 2 after removing and re-adding all slashers"
    );
}

/// Evidence-based slash still works normally after a re-add cycle on the slasher.
///
/// This is a functional sanity check: the slasher can still call
/// `slash_with_evidence` after being removed and re-added, and the resulting
/// record is in the expected Pending state.
#[test]
fn test_readded_slasher_can_use_slash_with_evidence() {
    let t = TestEnv::setup();

    // Cycle slasher1.
    t.client.remove_slasher(&t.slasher1);
    t.client.add_slasher(&t.slasher1);

    let hash = t.evidence_hash(252);
    t.client.slash_with_evidence(
        &t.slasher1,
        &t.offender,
        &Offense::FraudProof,
        &1_000u32,
        &hash,
        &0u64,
    );

    let record = t.client.get_slash_record(&hash).unwrap();
    assert_eq!(
        record.status,
        SlashStatus::Pending,
        "slash initiated by a re-added slasher must be Pending"
    );
    assert_eq!(
        record.penalty_bps, 1_000u32,
        "penalty_bps must be stored correctly"
    );
    // 10% of 10_000 stake = 1_000.
    assert_eq!(
        record.escrowed_amount, 1_000i128,
        "correct amount must be escrowed"
    );
    assert_eq!(
        t.client.get_stake_balance(&t.offender),
        9_000i128,
        "offender stake must reflect the deduction"
    );
}
