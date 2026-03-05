//! Comprehensive tests for the FeeCollector contract.
//!
//! Coverage targets:
//! * Initialization (happy path, double-init guard, invalid params)
//! * `collect_fee` — percentage mode (exact, floor rounding, zero-bps, max-bps)
//! * `collect_fee` — flat mode (normal, zero flat fee, flat fee ≥ gross)
//! * `calculate_fee` — pure computation, zero gross
//! * Config update & mode switching
//! * Recipient update
//! * Pause / unpause
//! * Admin transfer
//! * Cumulative `TotalFeesCollected` accumulation
//! * View helpers (`get_config`, `get_admin`)
//! * All unauthorised-access paths
//! * Edge cases: 1-token payment, large amounts (overflow safety)

use fee_collector::{FeeCollectorContract, FeeCollectorContractClient, FeeMode};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{token, Address, Env};

// ─── Fixtures ────────────────────────────────────────────────────────────────

/// Creates a Stellar test token and returns its client.
fn create_token<'a>(env: &Env, admin: &Address) -> token::Client<'a> {
    let addr = env.register_stellar_asset_contract(admin.clone());
    token::Client::new(env, &addr)
}

/// Deploys the FeeCollector contract and returns its client.
fn create_contract<'a>(env: &Env) -> FeeCollectorContractClient<'a> {
    let id = env.register_contract(None, FeeCollectorContract);
    FeeCollectorContractClient::new(env, &id)
}

/// Convenience: deploy + initialize with percentage mode and given `fee_bps`.
fn setup_percentage<'a>(
    env: &'a Env,
    admin: &Address,
    treasury: &Address,
    fee_bps: u32,
) -> FeeCollectorContractClient<'a> {
    let client = create_contract(env);
    client.initialize(admin, treasury, &fee_bps, &0i128, &FeeMode::Percentage);
    client
}

/// Convenience: deploy + initialize with flat mode and given `flat_fee`.
fn setup_flat<'a>(
    env: &'a Env,
    admin: &Address,
    treasury: &Address,
    flat_fee: i128,
) -> FeeCollectorContractClient<'a> {
    let client = create_contract(env);
    client.initialize(admin, treasury, &0u32, &flat_fee, &FeeMode::Flat);
    client
}

// ─── Initialization tests ─────────────────────────────────────────────────────

#[test]
fn test_initialize_percentage_mode() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &treasury, 100);

    let cfg = client.get_config();
    assert_eq!(cfg.fee_bps, 100);
    assert_eq!(cfg.flat_fee, 0);
    assert_eq!(cfg.mode, FeeMode::Percentage);
    assert_eq!(cfg.recipient, treasury);
    assert!(!cfg.paused);
}

#[test]
fn test_initialize_flat_mode() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup_flat(&env, &admin, &treasury, 50);

    let cfg = client.get_config();
    assert_eq!(cfg.flat_fee, 50);
    assert_eq!(cfg.mode, FeeMode::Flat);
}

#[test]
fn test_initialize_zero_fee_allowed() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    // 0 bps should be accepted (fee-free operation)
    let client = setup_percentage(&env, &admin, &treasury, 0);
    assert_eq!(client.get_config().fee_bps, 0);
}

#[test]
fn test_get_admin_returns_correct_address() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &treasury, 50);
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_total_fees_collected_starts_at_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &treasury, 100);
    assert_eq!(client.get_total_fees_collected(), 0);
}

#[test]
#[should_panic(expected = "Contract already initialized")]
fn test_double_initialization_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &treasury, 100);
    // Second init must panic.
    client.initialize(&admin, &treasury, &100u32, &0i128, &FeeMode::Percentage);
}

#[test]
#[should_panic(expected = "Fee exceeds maximum allowed (1000 bps)")]
fn test_fee_bps_above_max_panics_on_init() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = create_contract(&env);
    // 1001 bps > MAX_FEE_BPS (1000)
    client.initialize(&admin, &treasury, &1001u32, &0i128, &FeeMode::Percentage);
}

#[test]
#[should_panic(expected = "Flat fee must be non-negative")]
fn test_negative_flat_fee_panics_on_init() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = create_contract(&env);
    client.initialize(&admin, &treasury, &0u32, &-1i128, &FeeMode::Flat);
}

// ─── collect_fee — Percentage mode ───────────────────────────────────────────

#[test]
fn test_collect_fee_percentage_1_pct_exact() {
    // 1 % of 1 000 tokens = 10 fee, 990 net
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &1_000);

    let client = setup_percentage(&env, &admin, &treasury, 100); // 100 bps = 1 %

    let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &1_000);

    assert_eq!(fee, 10);
    assert_eq!(net, 990);
    assert_eq!(tok.balance(&treasury), 10);
    assert_eq!(tok.balance(&recipient), 990);
    assert_eq!(tok.balance(&payer), 0);
}

#[test]
fn test_collect_fee_percentage_floor_rounding() {
    // 1 % of 999 tokens = floor(9.99) = 9 fee, 990 net
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &999);

    let client = setup_percentage(&env, &admin, &treasury, 100);

    let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &999);

    // floor(999 * 100 / 10000) = floor(9.99) = 9
    assert_eq!(fee, 9);
    assert_eq!(net, 990);
    assert_eq!(tok.balance(&treasury), 9);
    assert_eq!(tok.balance(&recipient), 990);
}

#[test]
fn test_collect_fee_percentage_small_amount_floor_to_zero() {
    // 1 % of 9 tokens = floor(0.09) = 0 fee (all goes to recipient)
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &9);

    let client = setup_percentage(&env, &admin, &treasury, 100);

    let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &9);

    assert_eq!(fee, 0);
    assert_eq!(net, 9);
    assert_eq!(tok.balance(&treasury), 0);
    assert_eq!(tok.balance(&recipient), 9);
}

#[test]
fn test_collect_fee_percentage_zero_bps_no_fee() {
    // 0 bps → entire gross goes to recipient, treasury receives nothing
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &500);

    let client = setup_percentage(&env, &admin, &treasury, 0);

    let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &500);

    assert_eq!(fee, 0);
    assert_eq!(net, 500);
    assert_eq!(tok.balance(&treasury), 0);
    assert_eq!(tok.balance(&recipient), 500);
}

#[test]
fn test_collect_fee_percentage_max_bps_10_pct() {
    // 1000 bps = 10 % of 1 000 tokens = 100 fee, 900 net
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &1_000);

    let client = setup_percentage(&env, &admin, &treasury, 1_000); // MAX_FEE_BPS

    let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &1_000);

    assert_eq!(fee, 100);
    assert_eq!(net, 900);
    assert_eq!(tok.balance(&treasury), 100);
    assert_eq!(tok.balance(&recipient), 900);
}

#[test]
fn test_collect_fee_percentage_one_token_payment() {
    // 100 bps on 1 token = floor(0.01) = 0 fee
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &1);

    let client = setup_percentage(&env, &admin, &treasury, 100);

    let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &1);

    assert_eq!(fee, 0);
    assert_eq!(net, 1);
}

#[test]
fn test_collect_fee_percentage_large_amount() {
    // 50 bps on 10_000_000_000 (10 billion) tokens
    // fee = 10_000_000_000 * 50 / 10_000 = 50_000_000
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    let large: i128 = 10_000_000_000;
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &large);

    let client = setup_percentage(&env, &admin, &treasury, 50); // 0.5 %

    let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &large);

    assert_eq!(fee, 50_000_000);
    assert_eq!(net, large - 50_000_000);
    assert_eq!(tok.balance(&treasury), 50_000_000);
}

// ─── collect_fee — Flat mode ──────────────────────────────────────────────────

#[test]
fn test_collect_fee_flat_normal() {
    // flat fee 50 on 1 000 tokens → 50 fee, 950 net
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &1_000);

    let client = setup_flat(&env, &admin, &treasury, 50);

    let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &1_000);

    assert_eq!(fee, 50);
    assert_eq!(net, 950);
    assert_eq!(tok.balance(&treasury), 50);
    assert_eq!(tok.balance(&recipient), 950);
}

#[test]
fn test_collect_fee_flat_zero() {
    // flat fee 0 → no fee, all to recipient
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &200);

    let client = setup_flat(&env, &admin, &treasury, 0);

    let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &200);

    assert_eq!(fee, 0);
    assert_eq!(net, 200);
    assert_eq!(tok.balance(&treasury), 0);
    assert_eq!(tok.balance(&recipient), 200);
}

#[test]
fn test_collect_fee_flat_equal_to_gross() {
    // flat fee == gross → fee capped at gross, net = 0
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &100);

    // flat_fee = 100, gross = 100 → fee = min(100, 100) = 100, net = 0
    let client = setup_flat(&env, &admin, &treasury, 100);

    let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &100);

    assert_eq!(fee, 100);
    assert_eq!(net, 0);
    assert_eq!(tok.balance(&treasury), 100);
    assert_eq!(tok.balance(&recipient), 0);
}

#[test]
fn test_collect_fee_flat_exceeds_gross_capped() {
    // flat fee (200) > gross (100) → capped to 100, net = 0
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &100);

    let client = setup_flat(&env, &admin, &treasury, 200);

    let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &100);

    assert_eq!(fee, 100); // capped
    assert_eq!(net, 0);
    assert_eq!(tok.balance(&treasury), 100);
}

// ─── calculate_fee ────────────────────────────────────────────────────────────

#[test]
fn test_calculate_fee_percentage_no_transfer() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &treasury, 200); // 2 %

    let (net, fee) = client.calculate_fee(&1_000);
    assert_eq!(fee, 20);
    assert_eq!(net, 980);
    // No token state should change — no token was involved.
}

#[test]
fn test_calculate_fee_flat_no_transfer() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup_flat(&env, &admin, &treasury, 75);

    let (net, fee) = client.calculate_fee(&500);
    assert_eq!(fee, 75);
    assert_eq!(net, 425);
}

#[test]
fn test_calculate_fee_zero_gross_returns_zeros() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &treasury, 100);

    let (net, fee) = client.calculate_fee(&0);
    assert_eq!(fee, 0);
    assert_eq!(net, 0);
}

#[test]
fn test_calculate_fee_rounding_identical_to_collect() {
    // Verify calculate_fee and collect_fee agree on the same amount.
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &777);

    let client = setup_percentage(&env, &admin, &treasury, 150); // 1.5 %

    let (calc_net, calc_fee) = client.calculate_fee(&777);
    let (actual_net, actual_fee) = client.collect_fee(&payer, &recipient, &tok.address, &777);

    assert_eq!(calc_fee, actual_fee);
    assert_eq!(calc_net, actual_net);
}

// ─── Admin config update ──────────────────────────────────────────────────────

#[test]
fn test_update_fee_config_changes_rate() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &treasury, 100);

    client.update_fee_config(&admin, &500u32, &0i128, &FeeMode::Percentage);

    let cfg = client.get_config();
    assert_eq!(cfg.fee_bps, 500);
    assert_eq!(cfg.mode, FeeMode::Percentage);
}

#[test]
fn test_update_fee_config_switches_to_flat_mode() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &treasury, 100);

    client.update_fee_config(&admin, &0u32, &25i128, &FeeMode::Flat);

    let cfg = client.get_config();
    assert_eq!(cfg.mode, FeeMode::Flat);
    assert_eq!(cfg.flat_fee, 25);
}

#[test]
fn test_update_fee_config_to_zero_disables_fee() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &1_000);

    let client = setup_percentage(&env, &admin, &treasury, 300);

    // Disable fee
    client.update_fee_config(&admin, &0u32, &0i128, &FeeMode::Percentage);

    let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &1_000);
    assert_eq!(fee, 0);
    assert_eq!(net, 1_000);
}

#[test]
#[should_panic(expected = "Fee exceeds maximum allowed (1000 bps)")]
fn test_update_fee_config_above_max_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &treasury, 100);

    client.update_fee_config(&admin, &1001u32, &0i128, &FeeMode::Percentage);
}

#[test]
#[should_panic(expected = "Unauthorized: caller is not admin")]
fn test_update_fee_config_unauthorized_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &treasury, 100);

    client.update_fee_config(&attacker, &50u32, &0i128, &FeeMode::Percentage);
}

// ─── Recipient update ─────────────────────────────────────────────────────────

#[test]
fn test_update_recipient_routes_fees_to_new_address() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let old_treasury = Address::generate(&env);
    let new_treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &1_000);

    let client = setup_percentage(&env, &admin, &old_treasury, 100);

    client.update_recipient(&admin, &new_treasury);

    let (_, fee) = client.collect_fee(&payer, &recipient, &tok.address, &1_000);

    // Fee must land in the new treasury, not the old one.
    assert_eq!(tok.balance(&new_treasury), fee);
    assert_eq!(tok.balance(&old_treasury), 0);
}

#[test]
fn test_update_recipient_get_config_reflects_change() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let old_treasury = Address::generate(&env);
    let new_treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &old_treasury, 100);

    client.update_recipient(&admin, &new_treasury);
    assert_eq!(client.get_config().recipient, new_treasury);
}

#[test]
#[should_panic(expected = "Unauthorized: caller is not admin")]
fn test_update_recipient_unauthorized_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let treasury = Address::generate(&env);
    let new_treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &treasury, 100);

    client.update_recipient(&attacker, &new_treasury);
}

// ─── Pause / unpause ──────────────────────────────────────────────────────────

#[test]
fn test_set_paused_config_reflects_paused_state() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &treasury, 100);

    client.set_paused(&admin, &true);
    assert!(client.get_config().paused);
}

#[test]
#[should_panic(expected = "Contract is paused")]
fn test_collect_fee_blocked_when_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &500);

    let client = setup_percentage(&env, &admin, &treasury, 100);
    client.set_paused(&admin, &true);
    client.collect_fee(&payer, &recipient, &tok.address, &500);
}

#[test]
fn test_unpause_re_enables_collect_fee() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &500);

    let client = setup_percentage(&env, &admin, &treasury, 100);
    client.set_paused(&admin, &true);
    client.set_paused(&admin, &false);

    assert!(!client.get_config().paused);

    // Must succeed after unpausing.
    let (_, fee) = client.collect_fee(&payer, &recipient, &tok.address, &500);
    assert_eq!(fee, 5);
}

#[test]
#[should_panic(expected = "Unauthorized: caller is not admin")]
fn test_set_paused_unauthorized_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &treasury, 100);

    client.set_paused(&attacker, &true);
}

// ─── Admin transfer ───────────────────────────────────────────────────────────

#[test]
fn test_transfer_admin_new_admin_can_update_config() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &treasury, 100);

    client.transfer_admin(&admin, &new_admin);

    // New admin can update config.
    client.update_fee_config(&new_admin, &200u32, &0i128, &FeeMode::Percentage);
    assert_eq!(client.get_config().fee_bps, 200);
    assert_eq!(client.get_admin(), new_admin);
}

#[test]
#[should_panic(expected = "Unauthorized: caller is not admin")]
fn test_old_admin_cannot_act_after_transfer() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &treasury, 100);

    client.transfer_admin(&admin, &new_admin);

    // Old admin must now be rejected.
    client.update_fee_config(&admin, &50u32, &0i128, &FeeMode::Percentage);
}

#[test]
#[should_panic(expected = "Unauthorized: caller is not admin")]
fn test_transfer_admin_unauthorized_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &treasury, 100);

    client.transfer_admin(&attacker, &attacker);
}

// ─── Cumulative total fees ─────────────────────────────────────────────────────

#[test]
fn test_total_fees_accumulates_across_multiple_payments() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &3_000);

    let client = setup_percentage(&env, &admin, &treasury, 100); // 1 %

    // Three payments of 1 000 each → 10 fee each → 30 total
    client.collect_fee(&payer, &recipient, &tok.address, &1_000);
    client.collect_fee(&payer, &recipient, &tok.address, &1_000);
    client.collect_fee(&payer, &recipient, &tok.address, &1_000);

    assert_eq!(client.get_total_fees_collected(), 30);
    assert_eq!(tok.balance(&treasury), 30);
}

#[test]
fn test_total_fees_not_updated_when_zero_fee() {
    // When fee is 0 bps, TotalFeesCollected must remain 0.
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &1_000);

    let client = setup_percentage(&env, &admin, &treasury, 0);
    client.collect_fee(&payer, &recipient, &tok.address, &1_000);

    assert_eq!(client.get_total_fees_collected(), 0);
}

#[test]
fn test_total_fees_after_config_change() {
    // Change fee mid-stream; accumulator reflects actual fees collected.
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &2_000);

    let client = setup_percentage(&env, &admin, &treasury, 100); // 1 %

    // First payment at 1 % → 10 fee
    client.collect_fee(&payer, &recipient, &tok.address, &1_000);
    assert_eq!(client.get_total_fees_collected(), 10);

    // Change to 5 % (500 bps)
    client.update_fee_config(&admin, &500u32, &0i128, &FeeMode::Percentage);

    // Second payment at 5 % → 50 fee
    client.collect_fee(&payer, &recipient, &tok.address, &1_000);
    assert_eq!(client.get_total_fees_collected(), 60);
}

// ─── Collect fee error cases ───────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Gross amount must be positive")]
fn test_collect_fee_zero_gross_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);

    let client = setup_percentage(&env, &admin, &treasury, 100);
    client.collect_fee(&payer, &recipient, &tok.address, &0);
}

#[test]
#[should_panic(expected = "Contract is paused")]
fn test_collect_fee_while_paused_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &500);

    let client = setup_percentage(&env, &admin, &treasury, 100);
    client.set_paused(&admin, &true);
    client.collect_fee(&payer, &recipient, &tok.address, &500);
}

// ─── calculate_fee error cases ────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Gross amount must be non-negative")]
fn test_calculate_fee_negative_gross_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &treasury, 100);

    client.calculate_fee(&-1);
}

// ─── get_config completeness ──────────────────────────────────────────────────

#[test]
fn test_get_config_all_fields_after_updates() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let new_treasury = Address::generate(&env);
    let client = setup_percentage(&env, &admin, &treasury, 200);

    client.update_fee_config(&admin, &300u32, &15i128, &FeeMode::Flat);
    client.update_recipient(&admin, &new_treasury);
    client.set_paused(&admin, &true);

    let cfg = client.get_config();
    assert_eq!(cfg.fee_bps, 300);
    assert_eq!(cfg.flat_fee, 15);
    assert_eq!(cfg.mode, FeeMode::Flat);
    assert_eq!(cfg.recipient, new_treasury);
    assert!(cfg.paused);
}

// ─── Identical payer and recipient edge case ─────────────────────────────────

#[test]
fn test_payer_is_payment_recipient_still_pays_fee() {
    // If payer == payment_recipient, they still owe the fee to the treasury.
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let user = Address::generate(&env); // payer == recipient
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&user, &1_000);

    let client = setup_percentage(&env, &admin, &treasury, 100);

    let (net, fee) = client.collect_fee(&user, &user, &tok.address, &1_000);

    assert_eq!(fee, 10);
    assert_eq!(net, 990);
    assert_eq!(tok.balance(&treasury), 10);
    assert_eq!(tok.balance(&user), 990); // net returned to same address
}
