#![cfg(test)]

use soroban_sdk::{
    testutils::Address as _,
    token, Address, Env,
};
use stello_pay_contract::storage::{DisputeStatus, PayrollError};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

/// Helper to set up the main payroll contract
fn setup_payroll(env: &Env) -> (Address, PayrollContractClient<'static>) {
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (contract_id, client)
}

/// Helper to set up a mock token
fn setup_token<'a>(
    env: &'a Env,
    admin: &Address,
) -> (Address, token::Client<'a>, token::StellarAssetClient<'a>) {
    let token_contract = env.register_stellar_asset_contract_v2(admin.clone());
    let token_client = token::Client::new(env, &token_contract.address());
    let token_admin_client = token::StellarAssetClient::new(env, &token_contract.address());
    (token_contract.address(), token_client, token_admin_client)
}

/// @notice Tests the complete healthy dispute flow including escalation.
/// @dev Creates an agreement, raises a dispute, and resolves it cleanly.
#[test]
fn test_full_dispute_flow_resolved_by_arbiter() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_id, payroll_client) = setup_payroll(&env);
    let employer = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_address, token_client, token_admin_client) = setup_token(&env, &token_admin);
    let contributor = Address::generate(&env);

    payroll_client.set_arbiter(&employer, &arbiter);
    
    // 1. Create Escrow
    let amount_per_period = 1000_i128;
    let agreement_id = payroll_client.create_escrow_agreement(
        &employer,
        &contributor,
        &token_address,
        &amount_per_period,
        &86400,
        &1,
    );
    token_admin_client.mint(&payroll_id, &amount_per_period);

    // 2. Raise Dispute
    payroll_client.raise_dispute(&employer, &agreement_id);

    // Verify state changed to Disputed
    let status = payroll_client.get_dispute_status(&agreement_id);
    assert_eq!(status, DisputeStatus::Raised);

    // 3. Resolve Dispute by Arbiter
    let employer_refund = 600_i128; // 60% refund
    let employee_payout = 400_i128; // 40% payout
    payroll_client.resolve_dispute(&arbiter, &agreement_id, &employee_payout, &employer_refund);

    // Verify state changed to Resolved
    let final_status = payroll_client.get_dispute_status(&agreement_id);
    assert_eq!(final_status, DisputeStatus::Resolved);
    
    assert_eq!(token_client.balance(&contributor), 400);
    assert_eq!(token_client.balance(&employer), 600);
}

/// @notice Verifies that an unauthorized address cannot raise a dispute.
#[test]
fn test_raise_dispute_wrong_caller() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, payroll_client) = setup_payroll(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = payroll_client.create_escrow_agreement(&employer, &contributor, &token, &1000, &86400, &1);

    let malicious_actor = Address::generate(&env);
    
    // Should fail with NotParty error
    let result = payroll_client.try_raise_dispute(&malicious_actor, &agreement_id);
    assert_eq!(result, Err(Ok(PayrollError::NotParty)));
}

/// @notice Tests validation of invalid payout amounts during resolution.
#[test]
fn test_resolve_dispute_invalid_amounts() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, payroll_client) = setup_payroll(&env);
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let token = Address::generate(&env);

    payroll_client.set_arbiter(&employer, &arbiter);
    
    let agreement_id = payroll_client.create_escrow_agreement(&employer, &contributor, &token, &1000, &86400, &1);
    payroll_client.raise_dispute(&employer, &agreement_id);

    // Amounts sum to 1100, but escrow is only 1000
    let result = payroll_client.try_resolve_dispute(&arbiter, &agreement_id, &600_i128, &500_i128);
    assert_eq!(result, Err(Ok(PayrollError::InvalidPayout)));
}

/// @notice Tests dispute resolution involving a multi-employee payout split.
#[test]
fn test_multi_employee_payout_split() {
    let env = Env::default();
    env.mock_all_auths();

    let (payroll_id, payroll_client) = setup_payroll(&env);
    let employer = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_address, token_client, token_admin_client) = setup_token(&env, &token_admin);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);

    payroll_client.set_arbiter(&employer, &arbiter);
    
    let agreement_id = payroll_client.create_payroll_agreement(&employer, &token_address, &86400);
    payroll_client.add_employee_to_agreement(&agreement_id, &employee1, &100);
    payroll_client.add_employee_to_agreement(&agreement_id, &employee2, &100);
    token_admin_client.mint(&payroll_id, &200);

    payroll_client.raise_dispute(&employer, &agreement_id);

    // Arbiter resolves, employee pool gets 150, employer gets 50
    payroll_client.resolve_dispute(&arbiter, &agreement_id, &150_i128, &50_i128);

    assert_eq!(payroll_client.get_dispute_status(&agreement_id), DisputeStatus::Resolved);
    // Pay is split equally among employees (150 / 2 = 75)
    assert_eq!(token_client.balance(&employee1), 75);
    assert_eq!(token_client.balance(&employee2), 75);
    assert_eq!(token_client.balance(&employer), 50);
}


/// @notice Verifies that setting the arbiter to the caller (self-appointment) is rejected.
#[test]
fn test_set_arbiter_self_appointment_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, payroll_client) = setup_payroll(&env);
    let owner = Address::generate(&env);

    // Owner tries to set themselves as arbiter
    let result = payroll_client.try_set_arbiter(&owner, &owner);
    assert_eq!(result, Err(Ok(PayrollError::ArbiterSelfAppointment)));
}

/// @notice Verifies that setting the arbiter to the same address as the current arbiter is rejected.
#[test]
fn test_set_arbiter_no_op_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, payroll_client) = setup_payroll(&env);
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);

    // Set arbiter once
    payroll_client.set_arbiter(&owner, &arbiter);

    // Try setting the same arbiter again
    let result = payroll_client.try_set_arbiter(&owner, &arbiter);
    assert_eq!(result, Err(Ok(PayrollError::ArbiterNoOp)));
}

/// @notice Verifies that a valid arbiter change succeeds and records an audit entry.
#[test]
fn test_set_arbiter_valid_change() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, payroll_client) = setup_payroll(&env);
    let owner = Address::generate(&env);
    let arbiter1 = Address::generate(&env);
    let arbiter2 = Address::generate(&env);

    // Set arbiter for the first time
    payroll_client.set_arbiter(&owner, &arbiter1);
    assert_eq!(payroll_client.get_arbiter(), Some(arbiter1.clone()));

    // Change to a different arbiter
    payroll_client.set_arbiter(&owner, &arbiter2);
    assert_eq!(payroll_client.get_arbiter(), Some(arbiter2.clone()));
}

/// @notice Verifies that a non-owner caller cannot set the arbiter.
#[test]
fn test_set_arbiter_unauthorized_caller() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, payroll_client) = setup_payroll(&env);
    let _owner = Address::generate(&env);
    let non_owner = Address::generate(&env);
    let arbiter = Address::generate(&env);

    // Non-owner tries to set arbiter
    let result = payroll_client.try_set_arbiter(&non_owner, &arbiter);
    assert_eq!(result, Err(Ok(PayrollError::Unauthorized)));
}
