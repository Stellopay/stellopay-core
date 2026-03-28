//! Additional edge case tests for the FeeCollector contract.
//!
//! Focus: fee split routing, multi-recipient, split config edge cases.

use fee_collector::{FeeCollectorContract, FeeCollectorContractClient, FeeMode, FeeSplit};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{token, Address, Env};

fn create_token<'a>(env: &Env, admin: &Address) -> token::Client<'a> {
    let addr = env.register_stellar_asset_contract(admin.clone());
    token::Client::new(env, &addr)
}

fn create_contract<'a>(env: &Env) -> FeeCollectorContractClient<'a> {
    let id = env.register_contract(None, FeeCollectorContract);
    FeeCollectorContractClient::new(env, &id)
}

fn setup<'a>(
    env: &'a Env,
    admin: &Address,
    treasury: &Address,
    fee_bps: u32,
) -> FeeCollectorContractClient<'a> {
    let client = create_contract(env);
    client.initialize(admin, treasury, &fee_bps, &0i128, &FeeMode::Percentage);
    client
}

// ==================== Fee Split — Treasury ====================

#[test]
fn test_split_treasury_routes_all_fees() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &1_000);

    let client = setup(&env, &admin, &treasury, 100);

    // Set explicit treasury split
    client.update_fee_split(&admin, &Some(FeeSplit::Treasury(treasury.clone())));

    let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &1_000);

    assert_eq!(fee, 10);
    assert_eq!(net, 990);
    assert_eq!(tok.balance(&treasury), 10);
    assert_eq!(tok.balance(&recipient), 990);
}

// ==================== Fee Split — Burn ====================

#[test]
fn test_split_burn_routes_all_fees_to_burn_address() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let burn_addr = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &1_000);

    let client = setup(&env, &admin, &treasury, 100);

    client.update_fee_split(&admin, &Some(FeeSplit::Burn(burn_addr.clone())));

    let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &1_000);

    assert_eq!(fee, 10);
    assert_eq!(net, 990);
    assert_eq!(tok.balance(&burn_addr), 10);
    assert_eq!(tok.balance(&treasury), 0); // original treasury gets nothing
}

// ==================== Fee Split — Split ====================

#[test]
fn test_split_50_50_between_treasury_and_burn() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let burn_addr = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &10_000);

    let client = setup(&env, &admin, &treasury, 100); // 1% fee

    client.update_fee_split(
        &admin,
        &Some(FeeSplit::Split {
            treasury: treasury.clone(),
            burn: burn_addr.clone(),
            treasury_bps: 5_000, // 50%
            burn_bps: 5_000,     // 50%
        }),
    );

    let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &10_000);

    assert_eq!(fee, 100);
    assert_eq!(net, 9_900);
    // 50% of 100 = 50 to treasury, 50 to burn
    assert_eq!(tok.balance(&treasury), 50);
    assert_eq!(tok.balance(&burn_addr), 50);
}

#[test]
fn test_split_80_20_between_treasury_and_burn() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let burn_addr = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &1_000);

    let client = setup(&env, &admin, &treasury, 100);

    client.update_fee_split(
        &admin,
        &Some(FeeSplit::Split {
            treasury: treasury.clone(),
            burn: burn_addr.clone(),
            treasury_bps: 8_000, // 80%
            burn_bps: 2_000,     // 20%
        }),
    );

    let (net, fee) = client.collect_fee(&payer, &recipient, &tok.address, &1_000);

    assert_eq!(fee, 10);
    // floor(10 * 8000 / 10000) = 8 to treasury, 2 to burn
    assert_eq!(tok.balance(&treasury), 8);
    assert_eq!(tok.balance(&burn_addr), 2);
}

// ==================== Split Config Validation ====================

#[test]
#[should_panic(expected = "Split BPS must equal 10000")]
fn test_split_bps_must_sum_to_10000() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let burn_addr = Address::generate(&env);

    let client = setup(&env, &admin, &treasury, 100);

    client.update_fee_split(
        &admin,
        &Some(FeeSplit::Split {
            treasury: treasury.clone(),
            burn: burn_addr.clone(),
            treasury_bps: 6_000,
            burn_bps: 3_000, // sums to 9_000, not 10_000
        }),
    );
}

#[test]
fn test_split_none_returns_to_default_routing() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let burn_addr = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &2_000);

    let client = setup(&env, &admin, &treasury, 100);

    // First: set burn split
    client.update_fee_split(&admin, &Some(FeeSplit::Burn(burn_addr.clone())));
    client.collect_fee(&payer, &recipient, &tok.address, &1_000);
    assert_eq!(tok.balance(&burn_addr), 10);

    // Then: remove split → back to default treasury routing
    client.update_fee_split(&admin, &None);
    client.collect_fee(&payer, &recipient, &tok.address, &1_000);
    assert_eq!(tok.balance(&treasury), 10);
}

// ==================== get_config includes split ====================

#[test]
fn test_get_config_reflects_split() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = setup(&env, &admin, &treasury, 100);

    // Initially no split
    assert!(client.get_config().split.is_none());

    // Set split
    client.update_fee_split(&admin, &Some(FeeSplit::Treasury(treasury.clone())));
    let cfg = client.get_config();
    assert!(cfg.split.is_some());
}

// ==================== Total fees accumulate with split ====================

#[test]
fn test_total_fees_accumulate_with_split() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let burn_addr = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&payer, &3_000);

    let client = setup(&env, &admin, &treasury, 100);

    client.update_fee_split(
        &admin,
        &Some(FeeSplit::Split {
            treasury: treasury.clone(),
            burn: burn_addr.clone(),
            treasury_bps: 5_000,
            burn_bps: 5_000,
        }),
    );

    client.collect_fee(&payer, &recipient, &tok.address, &1_000);
    client.collect_fee(&payer, &recipient, &tok.address, &1_000);
    client.collect_fee(&payer, &recipient, &tok.address, &1_000);

    assert_eq!(client.get_total_fees_collected(), 30);
}
