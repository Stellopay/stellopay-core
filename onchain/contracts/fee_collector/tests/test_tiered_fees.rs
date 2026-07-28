#![cfg(test)]

use fee_collector::{FeeCollectorContract, FeeCollectorContractClient, FeeMode, FeeTier};
use soroban_sdk::{testutils::Address as _, token, Address, Env, Vec};

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Creates a Stellar test token, mints `amount` to `recipient`, and returns its client.
fn create_token<'a>(env: &'a Env, admin: &Address) -> token::Client<'a> {
    let addr = env.register_stellar_asset_contract(admin.clone());
    token::Client::new(env, &addr)
}

/// Mints additional tokens to a given address using the token's admin client.
fn mint(env: &Env, token: &token::Client, to: &Address, amount: i128) {
    token::StellarAssetClient::new(env, &token.address).mint(to, &amount);
}

/// Deploys and initializes a FeeCollector in Tiered mode with the standard 3-tier schedule:
/// - Tier 1: gross ≤ 1 000  → 500 bps (5 %)
/// - Tier 2: gross ≤ 5 000  → 250 bps (2.5 %)
/// - Tier 3: gross > 5 000  → 100 bps (1 %)
fn setup_tiered<'a>(
    env: &'a Env,
    admin: &Address,
    treasury: &Address,
) -> FeeCollectorContractClient<'a> {
    let client =
        FeeCollectorContractClient::new(env, &env.register(FeeCollectorContract, ()));
    // Initialize in Percentage mode so initialize() accepts fee_bps = 0.
    client.initialize(admin, treasury, &0u32, &0i128, &FeeMode::Percentage);

    let mut schedule = Vec::new(env);
    schedule.push_back(FeeTier {
        limit: 1_000,
        fee_bps: 500,
    }); // 5 %
    schedule.push_back(FeeTier {
        limit: 5_000,
        fee_bps: 250,
    }); // 2.5 %
    schedule.push_back(FeeTier {
        limit: i128::MAX,
        fee_bps: 100,
    }); // 1 %
    client.update_tiered_schedule(admin, &schedule);

    // Switch active mode to Tiered.
    client.update_fee_config(admin, &0u32, &0i128, &FeeMode::Tiered);

    client
}

/// Pure helper: computes the tiered fee for `gross` against the standard 3-tier schedule.
/// Mirrors the on-chain logic so the test has an independent ledger to compare against.
fn expected_tiered_fee(gross: i128) -> i128 {
    // Schedule: ≤1000 → 500 bps, ≤5000 → 250 bps, else 100 bps
    let bps: i128 = if gross <= 1_000 {
        500
    } else if gross <= 5_000 {
        250
    } else {
        100
    };
    gross * bps / 10_000
}

#[test]
fn test_tiered_fee_selection() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = FeeCollectorContractClient::new(&env, &env.register(FeeCollectorContract, ()));

    client.initialize(&admin, &treasury, &0, &0, &FeeMode::Percentage);

    // Setup schedule:
    // Tier 1: up to 1000 -> 500 bps (5%)
    // Tier 2: up to 5000 -> 250 bps (2.5%)
    // Tier 3: above 5000 -> 100 bps (1%)
    let mut schedule = Vec::new(&env);
    schedule.push_back(FeeTier {
        limit: 1000,
        fee_bps: 500,
    });
    schedule.push_back(FeeTier {
        limit: 5000,
        fee_bps: 250,
    });
    schedule.push_back(FeeTier {
        limit: i128::MAX,
        fee_bps: 100,
    });

    client.update_tiered_schedule(&admin, &schedule);

    // Switch to Tiered mode
    let config = client.get_config();
    client.update_fee_config(&admin, &config.fee_bps, &config.flat_fee, &FeeMode::Tiered);

    // Test Tier 1 (within 1000)
    let (_, fee) = client.calculate_fee(&500);
    assert_eq!(fee, 25); // 5% of 500

    // Test Tier 2 (within 5000)
    let (_, fee) = client.calculate_fee(&2000);
    assert_eq!(fee, 50); // 2.5% of 2000

    // Test Tier 3 (above 5000)
    let (_, fee) = client.calculate_fee(&10000);
    assert_eq!(fee, 100); // 1% of 10000
}

#[test]
fn test_rounding_invariant() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = FeeCollectorContractClient::new(&env, &env.register(FeeCollectorContract, ()));

    client.initialize(&admin, &treasury, &333, &0, &FeeMode::Percentage); // 3.33%

    let gross_amts = [1, 9, 10, 100, 1000, 10000, 1234567, 99999999];

    for &gross in gross_amts.iter() {
        let (net, fee) = client.calculate_fee(&gross);
        assert_eq!(net + fee, gross, "Invariant failed for gross={}", gross);
    }
}

#[test]
fn test_tiered_empty_schedule_defaults_to_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = FeeCollectorContractClient::new(&env, &env.register(FeeCollectorContract, ()));

    client.initialize(&admin, &treasury, &500, &0, &FeeMode::Tiered);

    let (net, fee) = client.calculate_fee(&1000);
    assert_eq!(fee, 0); // Empty schedule
    assert_eq!(net, 1000);
}

#[test]
#[should_panic(expected = "Unauthorized: caller is not admin")]
fn test_update_tiered_schedule_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = FeeCollectorContractClient::new(&env, &env.register(FeeCollectorContract, ()));

    client.initialize(&admin, &treasury, &0, &0, &FeeMode::Percentage);

    let schedule = Vec::new(&env);
    client.update_tiered_schedule(&attacker, &schedule);
}

#[test]
#[should_panic(expected = "Tier limits must be strictly increasing and positive")]
fn test_update_tiered_schedule_negative_limit_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = FeeCollectorContractClient::new(&env, &env.register(FeeCollectorContract, ()));

    client.initialize(&admin, &treasury, &0, &0, &FeeMode::Percentage);

    let mut schedule = Vec::new(&env);
    schedule.push_back(FeeTier {
        limit: -100,
        fee_bps: 500,
    });

    client.update_tiered_schedule(&admin, &schedule);
}

#[test]
#[should_panic(expected = "Tier limits must be strictly increasing and positive")]
fn test_update_tiered_schedule_zero_limit_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = FeeCollectorContractClient::new(&env, &env.register(FeeCollectorContract, ()));

    client.initialize(&admin, &treasury, &0, &0, &FeeMode::Percentage);

    let mut schedule = Vec::new(&env);
    schedule.push_back(FeeTier {
        limit: 0,
        fee_bps: 500,
    });

    client.update_tiered_schedule(&admin, &schedule);
}

#[test]
#[should_panic(expected = "Tier limits must be strictly increasing and positive")]
fn test_update_tiered_schedule_non_increasing_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = FeeCollectorContractClient::new(&env, &env.register(FeeCollectorContract, ()));

    client.initialize(&admin, &treasury, &0, &0, &FeeMode::Percentage);

    let mut schedule = Vec::new(&env);
    schedule.push_back(FeeTier {
        limit: 1000,
        fee_bps: 500,
    });
    schedule.push_back(FeeTier {
        limit: 500,
        fee_bps: 250,
    }); // lower than previous

    client.update_tiered_schedule(&admin, &schedule);
}

#[test]
#[should_panic(expected = "Tier limits must be strictly increasing and positive")]
fn test_update_tiered_schedule_duplicate_limit_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = FeeCollectorContractClient::new(&env, &env.register(FeeCollectorContract, ()));

    client.initialize(&admin, &treasury, &0, &0, &FeeMode::Percentage);

    let mut schedule = Vec::new(&env);
    schedule.push_back(FeeTier {
        limit: 1000,
        fee_bps: 500,
    });
    schedule.push_back(FeeTier {
        limit: 1000,
        fee_bps: 250,
    }); // same as previous

    client.update_tiered_schedule(&admin, &schedule);
}

#[test]
fn test_update_tiered_schedule_valid_limits_accepted() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = FeeCollectorContractClient::new(&env, &env.register(FeeCollectorContract, ()));

    client.initialize(&admin, &treasury, &0, &0, &FeeMode::Percentage);

    let mut schedule = Vec::new(&env);
    schedule.push_back(FeeTier {
        limit: 1000,
        fee_bps: 500,
    });
    schedule.push_back(FeeTier {
        limit: 5000,
        fee_bps: 250,
    });
    schedule.push_back(FeeTier {
        limit: i128::MAX,
        fee_bps: 100,
    });

    client.update_tiered_schedule(&admin, &schedule);
}

// ─── Reconciliation tests ────────────────────────────────────────────────────
//
// These tests independently compute the expected fee for every collect_fee call
// and assert that get_total_fees_collected() always matches the hand-summed ledger.
// A mid-sequence update_tiered_schedule change is also covered to verify the
// running total is never reset and always reflects actual transfers.

/// Basic reconciliation: multiple collect_fee calls across different tiers.
///
/// Each payment hits a different tier; the test maintains an independent running
/// sum and compares it to get_total_fees_collected() after every call.
#[test]
fn test_total_fees_reconciles_with_independent_sum_across_tiers() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);

    let client = setup_tiered(&env, &admin, &treasury);

    // Payment amounts that land in each tier:
    //   500  →  Tier 1 (≤1 000,  5 %)  →  fee = 25
    // 2 000  →  Tier 2 (≤5 000,  2.5%) →  fee = 50
    // 8 000  →  Tier 3 (>5 000,  1 %)  →  fee = 80
    let payments: [i128; 3] = [500, 2_000, 8_000];
    let total_gross: i128 = payments.iter().sum();
    mint(&env, &tok, &payer, total_gross);

    let mut independent_sum: i128 = 0;

    for &gross in payments.iter() {
        let expected_fee = expected_tiered_fee(gross);
        let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &gross);

        // Per-call invariant: net + fee == gross.
        assert_eq!(
            net + fee,
            gross,
            "Invariant broken for gross={}: net={} fee={}",
            gross,
            net,
            fee
        );
        // Per-call fee matches independent calculation.
        assert_eq!(
            fee, expected_fee,
            "Fee mismatch for gross={}: got {} want {}",
            gross, fee, expected_fee
        );

        independent_sum += fee;

        // Running total must match on every iteration.
        let on_chain_total = client.get_total_fees_collected();
        assert_eq!(
            on_chain_total, independent_sum,
            "Running total mismatch after gross={}: on_chain={} independent={}",
            gross, on_chain_total, independent_sum
        );
    }
}

/// Extended reconciliation: larger payment sequence spanning all three tiers
/// to confirm no rounding error accumulates in the running total.
#[test]
fn test_total_fees_reconciles_multi_call_sequence() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);

    let client = setup_tiered(&env, &admin, &treasury);

    // Seven payments: boundary values and mid-tier values.
    let payments: [i128; 7] = [
        1,      // Tier 1 minimum — floor(1×500/10000) = 0
        1_000,  // Tier 1 boundary — floor(1000×500/10000) = 50
        1_001,  // Tier 2 just above boundary — floor(1001×250/10000) = 25
        5_000,  // Tier 2 boundary — floor(5000×250/10000) = 125
        5_001,  // Tier 3 just above boundary — floor(5001×100/10000) = 50
        10_000, // Tier 3 mid — floor(10000×100/10000) = 100
        99_999, // Tier 3 large — floor(99999×100/10000) = 999
    ];
    let total_gross: i128 = payments.iter().sum();
    mint(&env, &tok, &payer, total_gross);

    let mut independent_sum: i128 = 0;

    for &gross in payments.iter() {
        let expected_fee = expected_tiered_fee(gross);
        let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &gross);

        assert_eq!(
            net + fee,
            gross,
            "Invariant broken for gross={}",
            gross
        );
        assert_eq!(
            fee, expected_fee,
            "Fee mismatch for gross={}: got {} want {}",
            gross, fee, expected_fee
        );

        independent_sum += fee;

        let on_chain_total = client.get_total_fees_collected();
        assert_eq!(
            on_chain_total, independent_sum,
            "Running total mismatch after gross={}: on_chain={} independent={}",
            gross, on_chain_total, independent_sum
        );
    }

    // Final sanity: confirm treasury received exactly the sum of all fees.
    assert_eq!(
        tok.balance(&treasury),
        independent_sum,
        "Treasury balance does not match total fees collected"
    );
}

/// Schedule-change reconciliation: collect_fee calls before AND after a
/// update_tiered_schedule mid-sequence; running total must be continuous.
///
/// The test verifies that:
///  1. Fees collected under the old schedule are preserved in the counter.
///  2. Fees after the schedule change use the new rates.
///  3. get_total_fees_collected() always equals the full hand-summed ledger.
#[test]
fn test_total_fees_reconciles_after_tiered_schedule_change() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);

    let client = setup_tiered(&env, &admin, &treasury);

    // ── Phase 1: 3 payments under the original schedule ──────────────────────
    // Original: ≤1000 → 500 bps, ≤5000 → 250 bps, >5000 → 100 bps
    let phase1_payments: [i128; 3] = [800, 3_000, 7_000];
    let phase1_gross: i128 = phase1_payments.iter().sum();
    mint(&env, &tok, &payer, phase1_gross);

    let mut independent_sum: i128 = 0;

    for &gross in phase1_payments.iter() {
        let expected_fee = expected_tiered_fee(gross);
        let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &gross);

        assert_eq!(net + fee, gross, "Phase 1 invariant broken for gross={}", gross);
        assert_eq!(fee, expected_fee, "Phase 1 fee mismatch for gross={}", gross);

        independent_sum += fee;
        let on_chain_total = client.get_total_fees_collected();
        assert_eq!(
            on_chain_total, independent_sum,
            "Phase 1 running total mismatch after gross={}",
            gross
        );
    }

    let total_after_phase1 = independent_sum;

    // ── Schedule change ───────────────────────────────────────────────────────
    // New schedule: ≤2000 → 200 bps (2%), >2000 → 50 bps (0.5%)
    let mut new_schedule = Vec::new(&env);
    new_schedule.push_back(FeeTier {
        limit: 2_000,
        fee_bps: 200,
    }); // 2 %
    new_schedule.push_back(FeeTier {
        limit: i128::MAX,
        fee_bps: 50,
    }); // 0.5 %
    client.update_tiered_schedule(&admin, &new_schedule);

    // ── Phase 2: 3 payments under the new schedule ───────────────────────────
    // Inline expected_fee helper for new schedule (independent of contract logic).
    let expected_fee_new = |gross: i128| -> i128 {
        let bps: i128 = if gross <= 2_000 { 200 } else { 50 };
        gross * bps / 10_000
    };

    let phase2_payments: [i128; 3] = [1_500, 4_000, 20_000];
    let phase2_gross: i128 = phase2_payments.iter().sum();
    mint(&env, &tok, &payer, phase2_gross);

    for &gross in phase2_payments.iter() {
        let expected_fee = expected_fee_new(gross);
        let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &gross);

        assert_eq!(net + fee, gross, "Phase 2 invariant broken for gross={}", gross);
        assert_eq!(
            fee, expected_fee,
            "Phase 2 fee mismatch for gross={}: got {} want {}",
            gross, fee, expected_fee
        );

        independent_sum += fee;
        let on_chain_total = client.get_total_fees_collected();
        assert_eq!(
            on_chain_total, independent_sum,
            "Phase 2 running total mismatch after gross={}",
            gross
        );
    }

    // The running total must include ALL fees from both phases without reset.
    assert!(
        independent_sum > total_after_phase1,
        "Phase 2 fees were not added to the running total"
    );

    // Final: treasury balance must match the full accumulated fee total.
    assert_eq!(
        tok.balance(&treasury),
        independent_sum,
        "Treasury balance does not match total fees after schedule change"
    );
}

/// Zero-fee tier reconciliation: a tier with fee_bps = 0 must not increment
/// the running total, and get_total_fees_collected() must remain unchanged.
#[test]
fn test_total_fees_unchanged_for_zero_fee_tier() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);

    let client =
        FeeCollectorContractClient::new(&env, &env.register(FeeCollectorContract, ()));
    client.initialize(&admin, &treasury, &0u32, &0i128, &FeeMode::Percentage);

    // Schedule with a 0-bps tier for amounts ≤ 1 000 and 100 bps above.
    let mut schedule = Vec::new(&env);
    schedule.push_back(FeeTier {
        limit: 1_000,
        fee_bps: 0,
    });
    schedule.push_back(FeeTier {
        limit: i128::MAX,
        fee_bps: 100,
    });
    client.update_tiered_schedule(&admin, &schedule);
    client.update_fee_config(&admin, &0u32, &0i128, &FeeMode::Tiered);

    // First payment in zero-fee tier.
    mint(&env, &tok, &payer, 2_000);
    let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &500);
    assert_eq!(fee, 0);
    assert_eq!(net, 500);
    assert_eq!(client.get_total_fees_collected(), 0);

    // Second payment crosses into fee-bearing tier.
    let (net2, fee2) = client.collect_fee(&payer, &recipient, &tok.address, &1_500);
    let expected_fee2 = 1_500i128 * 100 / 10_000; // = 15
    assert_eq!(fee2, expected_fee2);
    assert_eq!(net2, 1_500 - expected_fee2);
    assert_eq!(client.get_total_fees_collected(), expected_fee2);
}

/// Two-phase reconciliation with three schedule rotations to stress-test that
/// the running total is additive across an arbitrary number of schedule changes.
#[test]
fn test_total_fees_reconciles_across_multiple_schedule_rotations() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);

    let client =
        FeeCollectorContractClient::new(&env, &env.register(FeeCollectorContract, ()));
    client.initialize(&admin, &treasury, &0u32, &0i128, &FeeMode::Percentage);

    // Three rotations: (fee_bps, gross_amount).
    // Each chosen so that expected fee = gross * bps / 10_000 = 300 exactly.
    let rotations: [(u32, i128); 3] = [
        (300, 10_000), // 3 % of 10 000 = 300
        (150, 20_000), // 1.5 % of 20 000 = 300
        (600, 5_000),  // 6 % of 5 000 = 300
    ];

    let total_gross: i128 = rotations.iter().map(|(_, g)| g).sum();
    mint(&env, &tok, &payer, total_gross);

    let mut independent_sum: i128 = 0;

    for &(bps, gross) in rotations.iter() {
        // Rotate to a single catch-all tier with this bps.
        let mut s = Vec::new(&env);
        s.push_back(FeeTier {
            limit: i128::MAX,
            fee_bps: bps,
        });
        client.update_tiered_schedule(&admin, &s);
        client.update_fee_config(&admin, &0u32, &0i128, &FeeMode::Tiered);

        let expected_fee = gross * bps as i128 / 10_000;
        let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &gross);

        assert_eq!(net + fee, gross, "Invariant broken for gross={} bps={}", gross, bps);
        assert_eq!(
            fee, expected_fee,
            "Fee mismatch for gross={} bps={}: got {} want {}",
            gross, bps, fee, expected_fee
        );

        independent_sum += fee;
        let on_chain_total = client.get_total_fees_collected();
        assert_eq!(
            on_chain_total, independent_sum,
            "Running total mismatch after rotation bps={}: on_chain={} independent={}",
            bps, on_chain_total, independent_sum
        );
    }

    // Each rotation yielded 300 fee units → total should be 900.
    assert_eq!(independent_sum, 900);
    assert_eq!(client.get_total_fees_collected(), 900);
    assert_eq!(tok.balance(&treasury), 900);
}
