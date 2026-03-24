#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    Address, BytesN, Env, Vec,
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
        let token    = Address::generate(&env);

        client.initialize(&admin, &token, &2u32);
        client.add_slasher(&slasher1);
        client.add_slasher(&slasher2);
        client.add_slasher(&slasher3);

        // Give offender a stake of 10_000 tokens
        // (in real tests you'd use a mock token; here we seed the stakes map directly
        //  by calling stake() which triggers a token transfer — mocked)
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
    let result = t.client.try_initialize(&t.admin, &t.token, &2u32);
    assert_eq!(result, Err(Ok(SlashError::AlreadyInitialized)));
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