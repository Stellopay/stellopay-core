#![cfg(test)]

use soroban_sdk::{testutils::Address as _, token, Address, Env, Symbol, Vec};
use tax_withholding::{
    TaxComputation, TaxError, TaxWithholdingContract, TaxWithholdingContractClient,
};

// ─── Fixtures ────────────────────────────────────────────────────────────────

fn setup() -> (Env, Address, TaxWithholdingContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, TaxWithholdingContract);
    let client = TaxWithholdingContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    client.initialize(&owner);

    (env, owner, client)
}

fn create_token<'a>(env: &Env, admin: &Address) -> token::Client<'a> {
    let addr = env.register_stellar_asset_contract(admin.clone());
    token::Client::new(env, &addr)
}

// ─── Calculation tests ────────────────────────────────────────────────────────

#[test]
fn test_single_jurisdiction_withholding() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_CA");

    client.set_jurisdiction_rate(&owner, &jurisdiction, &1500u32, &1);

    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    let gross: i128 = 10_000;
    let result: TaxComputation = client.calculate_withholding(&employee, &gross);

    assert_eq!(result.gross_amount, gross);
    assert_eq!(result.total_tax, 1_500);
    assert_eq!(result.net_amount, 8_500);
    assert_eq!(result.shares.len(), 1);
    assert_eq!(result.ruleset_version, 1);

    let share = result.shares.get(0).unwrap();
    assert_eq!(share.jurisdiction, jurisdiction);
    assert_eq!(share.amount, 1_500);
}

#[test]
fn test_multi_jurisdiction_withholding() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let j1 = Symbol::new(&env, "US_FED");
    let j2 = Symbol::new(&env, "US_STATE");

    client.set_jurisdiction_rate(&owner, &j1, &1000u32, &1);
    client.set_jurisdiction_rate(&owner, &j2, &500u32, &1);

    let jurisdictions = Vec::from_array(&env, [j1.clone(), j2.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    let gross: i128 = 20_000;
    let result: TaxComputation = client.calculate_withholding(&employee, &gross);

    assert_eq!(result.total_tax, 3_000);
    assert_eq!(result.net_amount, 17_000);
    assert_eq!(result.shares.len(), 2);
    assert_eq!(result.ruleset_version, 1);
}

#[test]
fn test_not_configured_employee() {
    let (env, _owner, client) = setup();

    let employee = Address::generate(&env);
    let res = client.try_calculate_withholding(&employee, &5_000i128);
    assert_eq!(res, Err(Ok(TaxError::NotConfigured)));
}

#[test]
fn test_zero_rate_jurisdiction() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "ZERO");

    client.set_jurisdiction_rate(&owner, &jurisdiction, &0u32, &1);

    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    let result: TaxComputation = client.calculate_withholding(&employee, &10_000i128);
    assert_eq!(result.total_tax, 0);
    assert_eq!(result.net_amount, 10_000);
}

#[test]
fn test_invalid_rate_rejected() {
    let (env, owner, client) = setup();

    let jurisdiction = Symbol::new(&env, "OVER");
    let res = client.try_set_jurisdiction_rate(&owner, &jurisdiction, &10_001u32, &1);
    assert_eq!(res, Err(Ok(TaxError::InvalidRate)));
}

#[test]
fn test_max_rate_accepted() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "MAX");

    client.set_jurisdiction_rate(&owner, &jurisdiction, &10_000u32, &1);

    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    let result: TaxComputation = client.calculate_withholding(&employee, &10_000i128);
    assert_eq!(result.total_tax, 10_000);
    assert_eq!(result.net_amount, 0);
}

#[test]
fn test_floor_rounding_protects_employee() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_CA");

    // 15% of 10_001 = 1500.15 → floored to 1500 (employee keeps the 0.15)
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1500u32, &1);

    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    let result: TaxComputation = client.calculate_withholding(&employee, &10_001i128);
    assert_eq!(result.total_tax, 1_500);
    assert_eq!(result.net_amount, 8_501);
}

// ─── Treasury configuration tests ────────────────────────────────────────────

#[test]
fn test_set_and_get_jurisdiction_treasury() {
    let (env, owner, client) = setup();

    let treasury = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    client.set_jurisdiction_treasury(&owner, &jurisdiction, &treasury);

    let stored = client.get_jurisdiction_treasury(&jurisdiction);
    assert_eq!(stored, Some(treasury));
}

#[test]
fn test_get_jurisdiction_treasury_not_set_returns_none() {
    let (env, _owner, client) = setup();

    let jurisdiction = Symbol::new(&env, "EU_DE");
    assert_eq!(client.get_jurisdiction_treasury(&jurisdiction), None);
}

#[test]
fn test_unauthorized_set_treasury() {
    let (env, _owner, client) = setup();

    let non_owner = Address::generate(&env);
    let treasury = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    let res = client.try_set_jurisdiction_treasury(&non_owner, &jurisdiction, &treasury);
    assert_eq!(res, Err(Ok(TaxError::Unauthorized)));
}

// ─── Accrual hook tests ───────────────────────────────────────────────────────

#[test]
fn test_accrue_withholding_basic() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1); // 10%
    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    let result: TaxComputation = client.accrue_withholding(&owner, &employee, &10_000i128);

    assert_eq!(result.gross_amount, 10_000);
    assert_eq!(result.total_tax, 1_000);
    assert_eq!(result.net_amount, 9_000);

    assert_eq!(client.get_accrued_balance(&jurisdiction), 1_000);
}

#[test]
fn test_accrue_withholding_accumulates_across_periods() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);
    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    client.accrue_withholding(&owner, &employee, &10_000i128);
    client.accrue_withholding(&owner, &employee, &10_000i128);
    client.accrue_withholding(&owner, &employee, &10_000i128);

    // Three pay periods → 3 × 1_000 = 3_000 accrued
    assert_eq!(client.get_accrued_balance(&jurisdiction), 3_000);
}

#[test]
fn test_accrue_withholding_multi_jurisdiction() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let j1 = Symbol::new(&env, "US_FED");
    let j2 = Symbol::new(&env, "US_STATE");

    client.set_jurisdiction_rate(&owner, &j1, &1000u32, &1); // 10%
    client.set_jurisdiction_rate(&owner, &j2, &500u32, &1); // 5%

    let jurisdictions = Vec::from_array(&env, [j1.clone(), j2.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    client.accrue_withholding(&owner, &employee, &20_000i128);

    assert_eq!(client.get_accrued_balance(&j1), 2_000);
    assert_eq!(client.get_accrued_balance(&j2), 1_000);
}

#[test]
fn test_accrue_withholding_unauthorized() {
    let (env, owner, client) = setup();

    let non_owner = Address::generate(&env);
    let employee = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);
    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    let res = client.try_accrue_withholding(&non_owner, &employee, &10_000i128);
    assert_eq!(res, Err(Ok(TaxError::Unauthorized)));
}

#[test]
fn test_accrue_withholding_not_configured() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let res = client.try_accrue_withholding(&owner, &employee, &10_000i128);
    assert_eq!(res, Err(Ok(TaxError::NotConfigured)));
}

#[test]
fn test_get_accrued_balance_zero_when_not_set() {
    let (env, _owner, client) = setup();

    let jurisdiction = Symbol::new(&env, "EU_DE");
    assert_eq!(client.get_accrued_balance(&jurisdiction), 0);
}

// ─── Remittance hook tests ────────────────────────────────────────────────────

#[test]
fn test_remit_withholding_transfers_to_treasury() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let treasury = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);
    client.set_jurisdiction_treasury(&owner, &jurisdiction, &treasury);
    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    client.accrue_withholding(&owner, &employee, &10_000i128);
    assert_eq!(client.get_accrued_balance(&jurisdiction), 1_000);

    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&owner, &1_000i128);

    let remitted = client.remit_withholding(&owner, &jurisdiction, &tok.address, &1_000i128);
    assert_eq!(remitted, 1_000);

    // Accrued balance reset to zero after remittance
    assert_eq!(client.get_accrued_balance(&jurisdiction), 0);

    // Treasury received the funds
    assert_eq!(tok.balance(&treasury), 1_000);

    // Owner no longer holds the withheld amount
    assert_eq!(tok.balance(&owner), 0);
}

#[test]
fn test_remit_withholding_resets_balance_to_zero() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let treasury = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    client.set_jurisdiction_rate(&owner, &jurisdiction, &500u32, &1);
    client.set_jurisdiction_treasury(&owner, &jurisdiction, &treasury);
    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    client.accrue_withholding(&owner, &employee, &20_000i128);

    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&owner, &1_000i128);

    client.remit_withholding(&owner, &jurisdiction, &tok.address, &1_000i128);
    assert_eq!(client.get_accrued_balance(&jurisdiction), 0);
}

#[test]
fn test_remit_treasury_not_set() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);
    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    client.accrue_withholding(&owner, &employee, &10_000i128);

    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);

    let res = client.try_remit_withholding(&owner, &jurisdiction, &tok.address, &1i128);
    assert_eq!(res, Err(Ok(TaxError::TreasuryNotSet)));
}

#[test]
fn test_remit_nothing_to_remit() {
    let (env, owner, client) = setup();

    let treasury = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    client.set_jurisdiction_treasury(&owner, &jurisdiction, &treasury);

    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);

    let res = client.try_remit_withholding(&owner, &jurisdiction, &tok.address, &1i128);
    assert_eq!(res, Err(Ok(TaxError::NothingToRemit)));
}

#[test]
fn test_remit_withholding_unauthorized() {
    let (env, owner, client) = setup();

    let non_owner = Address::generate(&env);
    let treasury = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    client.set_jurisdiction_treasury(&owner, &jurisdiction, &treasury);

    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);

    let res = client.try_remit_withholding(&non_owner, &jurisdiction, &tok.address, &1i128);
    assert_eq!(res, Err(Ok(TaxError::Unauthorized)));
}

#[test]
fn test_remit_partial_then_accrue_and_remit_again() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let treasury = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);
    client.set_jurisdiction_treasury(&owner, &jurisdiction, &treasury);
    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&owner, &5_000i128);

    // Period 1: accrue 1_000, remit it
    client.accrue_withholding(&owner, &employee, &10_000i128);
    client.remit_withholding(&owner, &jurisdiction, &tok.address, &1_000i128);
    assert_eq!(tok.balance(&treasury), 1_000);
    assert_eq!(client.get_accrued_balance(&jurisdiction), 0);

    // Period 2: accrue 1_000 again
    client.accrue_withholding(&owner, &employee, &10_000i128);
    assert_eq!(client.get_accrued_balance(&jurisdiction), 1_000);

    client.remit_withholding(&owner, &jurisdiction, &tok.address, &1_000i128);
    assert_eq!(tok.balance(&treasury), 2_000);
    assert_eq!(client.get_accrued_balance(&jurisdiction), 0);
}

/// A calculated amount is only remittable after it has been accrued, and every
/// successful remittance reduces the outstanding liability by exactly its amount.
#[test]
fn test_cumulative_remittances_never_exceed_calculated_liability() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let treasury = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);
    client.set_jurisdiction_treasury(&owner, &jurisdiction, &treasury);
    client.set_employee_jurisdictions(
        &owner,
        &employee,
        &Vec::from_array(&env, [jurisdiction.clone()]),
    );

    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&owner, &10_000i128);

    let mut cumulative_calculated = 0i128;
    let mut cumulative_remitted = 0i128;

    // Each calculation is verified against the accrual performed for the same
    // completed pay period. The remittances deliberately include partial and
    // full settlements to exercise the outstanding-liability invariant.
    for (gross, requested_remittance) in [(10_000i128, 400i128), (15_000, 1_100), (5_000, 500)] {
        let calculated = client.calculate_withholding(&employee, &gross);
        cumulative_calculated += calculated.total_tax;

        let accrued = client.accrue_withholding(&owner, &employee, &gross);
        assert_eq!(accrued, calculated);

        let outstanding_before = client.get_accrued_balance(&jurisdiction);
        let remitted =
            client.remit_withholding(&owner, &jurisdiction, &tok.address, &requested_remittance);
        cumulative_remitted += remitted;

        assert_eq!(remitted, requested_remittance);
        assert_eq!(
            client.get_accrued_balance(&jurisdiction),
            outstanding_before - remitted,
        );
        assert!(cumulative_remitted <= cumulative_calculated);
    }

    assert_eq!(cumulative_calculated, 3_000);
    assert_eq!(cumulative_remitted, 2_000);
    assert_eq!(client.get_accrued_balance(&jurisdiction), 1_000);
    assert_eq!(tok.balance(&treasury), cumulative_remitted);
}

#[test]
fn test_remit_more_than_outstanding_liability_is_rejected() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let treasury = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);
    client.set_jurisdiction_treasury(&owner, &jurisdiction, &treasury);
    client.set_employee_jurisdictions(
        &owner,
        &employee,
        &Vec::from_array(&env, [jurisdiction.clone()]),
    );
    client.accrue_withholding(&owner, &employee, &10_000i128);

    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&owner, &2_000i128);

    let res = client.try_remit_withholding(&owner, &jurisdiction, &tok.address, &1_001i128);
    assert_eq!(res, Err(Ok(TaxError::AmountExceedsAccrued)));
    assert_eq!(client.get_accrued_balance(&jurisdiction), 1_000);
    assert_eq!(tok.balance(&owner), 2_000);
    assert_eq!(tok.balance(&treasury), 0);
}

#[test]
fn test_remit_non_positive_amount_is_rejected_without_changing_liability() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let treasury = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");
    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);
    client.set_jurisdiction_treasury(&owner, &jurisdiction, &treasury);
    client.set_employee_jurisdictions(
        &owner,
        &employee,
        &Vec::from_array(&env, [jurisdiction.clone()]),
    );
    client.accrue_withholding(&owner, &employee, &10_000i128);

    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&owner, &1_000i128);

    for invalid_amount in [0i128, -1i128] {
        let res =
            client.try_remit_withholding(&owner, &jurisdiction, &tok.address, &invalid_amount);
        assert_eq!(res, Err(Ok(TaxError::AmountExceedsAccrued)));
        assert_eq!(client.get_accrued_balance(&jurisdiction), 1_000);
        assert_eq!(tok.balance(&owner), 1_000);
        assert_eq!(tok.balance(&treasury), 0);
    }
}

// ─── Security invariant tests ─────────────────────────────────────────────────

#[test]
fn test_non_owner_cannot_set_jurisdiction_rate() {
    let (env, _owner, client) = setup();

    let non_owner = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    let res = client.try_set_jurisdiction_rate(&non_owner, &jurisdiction, &1000u32, &1);
    assert_eq!(res, Err(Ok(TaxError::Unauthorized)));
}

#[test]
fn test_non_owner_cannot_set_employee_jurisdictions() {
    let (env, _owner, client) = setup();

    let non_owner = Address::generate(&env);
    let employee = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    let res = client.try_set_employee_jurisdictions(&non_owner, &employee, &jurisdictions);
    assert_eq!(res, Err(Ok(TaxError::Unauthorized)));
}

#[test]
fn test_withholding_destination_is_owner_controlled() {
    // Verify that treasury is read from owner-controlled storage during remit,
    // not from any caller-supplied address.
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let legitimate_treasury = Address::generate(&env);
    let attacker_treasury = Address::generate(&env);
    let jurisdiction = Symbol::new(&env, "US_FED");

    client.set_jurisdiction_rate(&owner, &jurisdiction, &1000u32, &1);
    // Owner sets the legitimate treasury
    client.set_jurisdiction_treasury(&owner, &jurisdiction, &legitimate_treasury);
    let jurisdictions = Vec::from_array(&env, [jurisdiction.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &jurisdictions);

    client.accrue_withholding(&owner, &employee, &10_000i128);

    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&owner, &1_000i128);

    // Attacker cannot set treasury (verified by test_unauthorized_set_treasury).
    // Remit goes to legitimate_treasury, not attacker_treasury.
    client.remit_withholding(&owner, &jurisdiction, &tok.address, &1_000i128);
    assert_eq!(tok.balance(&legitimate_treasury), 1_000);
    assert_eq!(tok.balance(&attacker_treasury), 0);
}

// ─── Jurisdiction removal historical preservation tests ────────────────────────

#[test]
fn test_jurisdiction_removal_preserves_accrued_balance() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let j_fed = Symbol::new(&env, "US_FED");
    let j_state = Symbol::new(&env, "US_STATE");

    client.set_jurisdiction_rate(&owner, &j_fed, &1000u32, &1); // 10%
    client.set_jurisdiction_rate(&owner, &j_state, &500u32, &1); // 5%

    // Step 1: Assign US_FED and US_STATE to employee
    let initial_jurisdictions = Vec::from_array(&env, [j_fed.clone(), j_state.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &initial_jurisdictions);

    // Step 2: Accrue withholding for a pay period (10,000 gross pay)
    let comp1 = client.accrue_withholding(&owner, &employee, &10_000i128);
    assert_eq!(comp1.total_tax, 1_500);
    assert_eq!(client.get_accrued_balance(&j_fed), 1_000);
    assert_eq!(client.get_accrued_balance(&j_state), 500);

    // Step 3: Remove US_STATE from employee's assigned jurisdictions
    let updated_jurisdictions = Vec::from_array(&env, [j_fed.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &updated_jurisdictions);

    let current_jurisdictions = client.get_employee_jurisdictions(&employee);
    assert_eq!(current_jurisdictions.len(), 1);
    assert_eq!(current_jurisdictions.get(0).unwrap(), j_fed);

    // Step 4: Assert prior accrual record for US_STATE is still present and intact
    assert_eq!(client.get_accrued_balance(&j_state), 500);
    assert_eq!(client.get_accrued_balance(&j_fed), 1_000);

    // Step 5: Accrue withholding for next pay period — only US_FED should accrue
    let comp2 = client.accrue_withholding(&owner, &employee, &10_000i128);
    assert_eq!(comp2.total_tax, 1_000);
    assert_eq!(comp2.shares.len(), 1);
    assert_eq!(comp2.shares.get(0).unwrap().jurisdiction, j_fed);

    // US_FED increases to 2_000, US_STATE remains unchanged at 500
    assert_eq!(client.get_accrued_balance(&j_fed), 2_000);
    assert_eq!(client.get_accrued_balance(&j_state), 500);
}

#[test]
fn test_jurisdiction_removal_historical_annual_accrual_intact() {
    let (env, owner, client) = setup();

    let employee = Address::generate(&env);
    let j_fed = Symbol::new(&env, "US_FED");
    let j_state = Symbol::new(&env, "US_STATE");
    let state_treasury = Address::generate(&env);

    client.set_jurisdiction_rate(&owner, &j_fed, &1000u32, &1); // 10%
    client.set_jurisdiction_rate(&owner, &j_state, &500u32, &1); // 5%
    client.set_jurisdiction_treasury(&owner, &j_state, &state_treasury);

    let initial_jurisdictions = Vec::from_array(&env, [j_fed.clone(), j_state.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &initial_jurisdictions);

    // Simulate 12 pay periods (10,000 gross per period)
    for _ in 0..12 {
        client.accrue_withholding(&owner, &employee, &10_000i128);
    }

    // Historical annual accruals before removal
    let pre_removal_fed = client.get_accrued_balance(&j_fed);
    let pre_removal_state = client.get_accrued_balance(&j_state);
    assert_eq!(pre_removal_fed, 12_000);
    assert_eq!(pre_removal_state, 6_000);
    let annual_total_before = pre_removal_fed + pre_removal_state;
    assert_eq!(annual_total_before, 18_000);

    // Employee moves or removes US_STATE assignment post year-end
    let updated_jurisdictions = Vec::from_array(&env, [j_fed.clone()]);
    client.set_employee_jurisdictions(&owner, &employee, &updated_jurisdictions);

    // Assert annual summary query still reflects historical accruals accurately
    let post_removal_fed = client.get_accrued_balance(&j_fed);
    let post_removal_state = client.get_accrued_balance(&j_state);
    assert_eq!(post_removal_fed, 12_000);
    assert_eq!(post_removal_state, 6_000);
    assert_eq!(post_removal_fed + post_removal_state, annual_total_before);

    // Assert that remittance for the removed jurisdiction succeeds with exact historical amount
    let token_admin = Address::generate(&env);
    let tok = create_token(&env, &token_admin);
    token::StellarAssetClient::new(&env, &tok.address).mint(&owner, &6_000i128);

    let remitted = client.remit_withholding(&owner, &j_state, &tok.address);
    assert_eq!(remitted, 6_000);
    assert_eq!(tok.balance(&state_treasury), 6_000);
    assert_eq!(client.get_accrued_balance(&j_state), 0);
}
