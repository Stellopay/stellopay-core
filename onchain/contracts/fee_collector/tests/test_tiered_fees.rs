#![cfg(test)]

use fee_collector::{FeeCollectorContract, FeeCollectorContractClient, FeeMode, FeeTier};
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

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
    schedule.push_back(FeeTier { limit: 1000, fee_bps: 500 });
    schedule.push_back(FeeTier { limit: 5000, fee_bps: 250 });
    schedule.push_back(FeeTier { limit: i128::MAX, fee_bps: 100 });

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
