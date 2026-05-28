//! Integration tests for multisig-gated LargePayment and DisputeResolution flows.
//!
//! Covers:
//! - M-of-N approval required before resolve_dispute_multisig succeeds
//! - M-of-N approval required before claim_payroll_multisig succeeds
//! - Rejection when threshold not yet met (insufficient signatures)
//! - Rejection when multisig op kind/params don't match the call
//! - Below-threshold calls bypass multisig entirely
#![cfg(test)]

use multisig::{MultisigContract, MultisigContractClient, OperationKind, OperationStatus};
use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};
use stello_pay_contract::{
    storage::{DisputeStatus, PayrollError},
    PayrollContract, PayrollContractClient,
};

// ── helpers ──────────────────────────────────────────────────────────────────

fn setup_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_token<'a>(env: &'a Env, admin: &Address) -> (Address, TokenClient<'a>) {
    let addr = env.register_stellar_asset_contract_v2(admin.clone()).address();
    let client = TokenClient::new(env, &addr);
    (addr, client)
}

fn setup_payroll(env: &Env) -> (Address, PayrollContractClient<'static>, Address) {
    let id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(env, &id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (id, client, owner)
}

/// Creates a 2-of-3 multisig and returns (contract_id, client, signers[3]).
fn setup_multisig(env: &Env) -> (Address, MultisigContractClient<'static>, Vec<Address>) {
    #[allow(deprecated)]
    let id = env.register_contract(None, MultisigContract);
    let client = MultisigContractClient::new(env, &id);
    let owner = Address::generate(env);
    let mut signers = Vec::new(env);
    for _ in 0..3 {
        signers.push_back(Address::generate(env));
    }
    client.initialize(&owner, &signers, &2u32, &None);
    (id, client, signers)
}

// ── DisputeResolution tests ───────────────────────────────────────────────────

/// Happy path: 2-of-3 signers approve a DisputeResolution op, then
/// resolve_dispute_multisig succeeds.
#[test]
fn test_dispute_multisig_2of3_approval_succeeds() {
    let env = setup_env();
    let (payroll_id, payroll, owner) = setup_payroll(&env);
    let (multisig_id, ms, signers) = setup_multisig(&env);

    let employer = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_addr, token_client) = setup_token(&env, &token_admin);
    StellarAssetClient::new(&env, &token_addr).mint(&payroll_id, &1000);

    payroll.set_arbiter(&employer, &arbiter);
    payroll.set_multisig_config(
        &owner,
        &multisig_id,
        &0i128,    // large_payment_threshold: disabled
        &500i128,  // dispute_resolution_threshold: 500
    );

    let agreement_id =
        payroll.create_escrow_agreement(&employer, &employer, &token_addr, &1000, &86400, &1);
    payroll.raise_dispute(&employer, &agreement_id);

    let pay_employee = 600i128;
    let refund_employer = 400i128;

    // Propose DisputeResolution in multisig
    let op_id = ms.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(
            payroll_id.clone(),
            agreement_id,
            pay_employee,
            refund_employer,
        ),
    );
    // After proposer auto-approves (1/2), op is still Pending
    assert_eq!(
        ms.get_operation(&op_id).unwrap().status,
        OperationStatus::Pending
    );

    // Second signer approves → threshold met → Executed
    ms.approve_operation(&signers.get(1).unwrap(), &op_id);
    assert_eq!(
        ms.get_operation(&op_id).unwrap().status,
        OperationStatus::Executed
    );

    // Now resolve via multisig path
    payroll
        .resolve_dispute_multisig(
            &arbiter,
            &agreement_id,
            &pay_employee,
            &refund_employer,
            &op_id,
        )
        .unwrap();

    assert_eq!(
        payroll.get_dispute_status(&agreement_id),
        DisputeStatus::Resolved
    );
    // employer (also contributor in this test) receives refund
    assert_eq!(token_client.balance(&employer), refund_employer);
}

/// Rejection: only 1-of-2 required signers have approved — op still Pending.
#[test]
fn test_dispute_multisig_insufficient_signatures_rejected() {
    let env = setup_env();
    let (payroll_id, payroll, owner) = setup_payroll(&env);
    let (multisig_id, ms, signers) = setup_multisig(&env);

    let employer = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_addr, _) = setup_token(&env, &token_admin);
    StellarAssetClient::new(&env, &token_addr).mint(&payroll_id, &1000);

    payroll.set_arbiter(&employer, &arbiter);
    payroll.set_multisig_config(
        &owner,
        &multisig_id,
        &0i128,
        &500i128,
    );

    let agreement_id =
        payroll.create_escrow_agreement(&employer, &employer, &token_addr, &1000, &86400, &1);
    payroll.raise_dispute(&employer, &agreement_id);

    let pay_employee = 600i128;
    let refund_employer = 400i128;

    // Only proposer approves (1 of 2 needed) — op stays Pending
    let op_id = ms.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(
            payroll_id.clone(),
            agreement_id,
            pay_employee,
            refund_employer,
        ),
    );
    assert_eq!(
        ms.get_operation(&op_id).unwrap().status,
        OperationStatus::Pending
    );

    // Attempt to resolve — must fail
    let result = payroll.try_resolve_dispute_multisig(
        &arbiter,
        &agreement_id,
        &pay_employee,
        &refund_employer,
        &op_id,
    );
    assert_eq!(result, Err(Ok(PayrollError::MultisigApprovalRequired)));
}

/// Rejection: resolve_dispute (non-multisig path) is blocked when total payout
/// meets the configured threshold.
#[test]
fn test_dispute_direct_blocked_above_threshold() {
    let env = setup_env();
    let (payroll_id, payroll, owner) = setup_payroll(&env);
    let (multisig_id, _ms, _signers) = setup_multisig(&env);

    let employer = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_addr, _) = setup_token(&env, &token_admin);
    StellarAssetClient::new(&env, &token_addr).mint(&payroll_id, &1000);

    payroll.set_arbiter(&employer, &arbiter);
    payroll.set_multisig_config(
        &owner,
        &multisig_id,
        &0i128,
        &500i128, // threshold = 500
    );

    let agreement_id =
        payroll.create_escrow_agreement(&employer, &employer, &token_addr, &1000, &86400, &1);
    payroll.raise_dispute(&employer, &agreement_id);

    // 600 + 400 = 1000 >= 500 → must be blocked
    let result = payroll.try_resolve_dispute(&arbiter, &agreement_id, &600i128, &400i128);
    assert_eq!(result, Err(Ok(PayrollError::MultisigApprovalRequired)));
}

/// Below-threshold dispute resolution bypasses multisig entirely.
#[test]
fn test_dispute_below_threshold_bypasses_multisig() {
    let env = setup_env();
    let (payroll_id, payroll, owner) = setup_payroll(&env);
    let (multisig_id, _ms, _signers) = setup_multisig(&env);

    let employer = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_addr, _) = setup_token(&env, &token_admin);
    StellarAssetClient::new(&env, &token_addr).mint(&payroll_id, &1000);

    payroll.set_arbiter(&employer, &arbiter);
    payroll.set_multisig_config(
        &owner,
        &multisig_id,
        &0i128,
        &2000i128, // threshold = 2000, well above our payout
    );

    let agreement_id =
        payroll.create_escrow_agreement(&employer, &employer, &token_addr, &1000, &86400, &1);
    payroll.raise_dispute(&employer, &agreement_id);

    // 200 + 300 = 500 < 2000 → direct path allowed
    payroll
        .resolve_dispute(&arbiter, &agreement_id, &200i128, &300i128)
        .unwrap();
    assert_eq!(
        payroll.get_dispute_status(&agreement_id),
        DisputeStatus::Resolved
    );
}

// ── LargePayment / claim_payroll tests ───────────────────────────────────────

/// Happy path: 2-of-3 signers approve a LargePayment op, then
/// claim_payroll_multisig succeeds.
#[test]
fn test_claim_payroll_multisig_2of3_approval_succeeds() {
    let env = setup_env();
    let (payroll_id, payroll, owner) = setup_payroll(&env);
    let (multisig_id, ms, signers) = setup_multisig(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_addr, token_client) = setup_token(&env, &token_admin);

    let salary = 1000i128;
    let period = 86400u64;

    // Fund escrow
    StellarAssetClient::new(&env, &token_addr).mint(&payroll_id, &salary);

    payroll.set_multisig_config(
        &owner,
        &multisig_id,
        &500i128, // large_payment_threshold = 500
        &0i128,
    );

    let agreement_id = payroll.create_payroll_agreement(&employer, &token_addr, &period);
    payroll.add_employee_to_agreement(&agreement_id, &employee, &salary);
    payroll.activate_agreement(&agreement_id);

    // Advance ledger by one full period so one period is claimable
    env.ledger().with_mut(|l| l.timestamp += period + 1);

    // Propose LargePayment in multisig (amount = salary * 1 period = 1000)
    let op_id = ms.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token_addr.clone(), employee.clone(), salary),
    );
    // Still pending after 1 approval
    assert_eq!(
        ms.get_operation(&op_id).unwrap().status,
        OperationStatus::Pending
    );

    // Second signer approves → Executed
    ms.approve_operation(&signers.get(1).unwrap(), &op_id);
    assert_eq!(
        ms.get_operation(&op_id).unwrap().status,
        OperationStatus::Executed
    );

    payroll
        .claim_payroll_multisig(&employee, &agreement_id, &0u32, &op_id)
        .unwrap();

    assert_eq!(token_client.balance(&employee), salary);
}

/// Rejection: only 1-of-2 required signers approved — claim_payroll_multisig fails.
#[test]
fn test_claim_payroll_multisig_insufficient_signatures_rejected() {
    let env = setup_env();
    let (payroll_id, payroll, owner) = setup_payroll(&env);
    let (multisig_id, ms, signers) = setup_multisig(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_addr, _) = setup_token(&env, &token_admin);

    let salary = 1000i128;
    let period = 86400u64;

    StellarAssetClient::new(&env, &token_addr).mint(&payroll_id, &salary);

    payroll.set_multisig_config(
        &owner,
        &multisig_id,
        &500i128,
        &0i128,
    );

    let agreement_id = payroll.create_payroll_agreement(&employer, &token_addr, &period);
    payroll.add_employee_to_agreement(&agreement_id, &employee, &salary);
    payroll.activate_agreement(&agreement_id);
    env.ledger().with_mut(|l| l.timestamp += period + 1);

    // Only proposer approves — op stays Pending
    let op_id = ms.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token_addr.clone(), employee.clone(), salary),
    );
    assert_eq!(
        ms.get_operation(&op_id).unwrap().status,
        OperationStatus::Pending
    );

    let result = payroll.try_claim_payroll_multisig(&employee, &agreement_id, &0u32, &op_id);
    assert_eq!(result, Err(Ok(PayrollError::MultisigApprovalRequired)));
}

/// Rejection: direct claim_payroll is blocked when amount meets threshold.
#[test]
fn test_claim_payroll_direct_blocked_above_threshold() {
    let env = setup_env();
    let (payroll_id, payroll, owner) = setup_payroll(&env);
    let (multisig_id, _ms, _signers) = setup_multisig(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_addr, _) = setup_token(&env, &token_admin);

    let salary = 1000i128;
    let period = 86400u64;

    StellarAssetClient::new(&env, &token_addr).mint(&payroll_id, &salary);

    payroll.set_multisig_config(
        &owner,
        &multisig_id,
        &500i128, // threshold = 500; salary 1000 >= 500
        &0i128,
    );

    let agreement_id = payroll.create_payroll_agreement(&employer, &token_addr, &period);
    payroll.add_employee_to_agreement(&agreement_id, &employee, &salary);
    payroll.activate_agreement(&agreement_id);
    env.ledger().with_mut(|l| l.timestamp += period + 1);

    let result = payroll.try_claim_payroll(&employee, &agreement_id, &0u32);
    assert_eq!(result, Err(Ok(PayrollError::MultisigApprovalRequired)));
}

/// Below-threshold claim bypasses multisig entirely.
#[test]
fn test_claim_payroll_below_threshold_bypasses_multisig() {
    let env = setup_env();
    let (payroll_id, payroll, owner) = setup_payroll(&env);
    let (multisig_id, _ms, _signers) = setup_multisig(&env);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_addr, token_client) = setup_token(&env, &token_admin);

    let salary = 100i128;
    let period = 86400u64;

    StellarAssetClient::new(&env, &token_addr).mint(&payroll_id, &salary);

    payroll.set_multisig_config(
        &owner,
        &multisig_id,
        &500i128, // threshold = 500; salary 100 < 500
        &0i128,
    );

    let agreement_id = payroll.create_payroll_agreement(&employer, &token_addr, &period);
    payroll.add_employee_to_agreement(&agreement_id, &employee, &salary);
    payroll.activate_agreement(&agreement_id);
    env.ledger().with_mut(|l| l.timestamp += period + 1);

    payroll.claim_payroll(&employee, &agreement_id, &0u32).unwrap();
    assert_eq!(token_client.balance(&employee), salary);
}

/// Rejection: multisig op kind doesn't match (wrong operation type).
#[test]
fn test_dispute_multisig_wrong_op_kind_rejected() {
    let env = setup_env();
    let (payroll_id, payroll, owner) = setup_payroll(&env);
    let (multisig_id, ms, signers) = setup_multisig(&env);

    let employer = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_addr, _) = setup_token(&env, &token_admin);
    StellarAssetClient::new(&env, &token_addr).mint(&payroll_id, &1000);

    payroll.set_arbiter(&employer, &arbiter);
    payroll.set_multisig_config(
        &owner,
        &multisig_id,
        &0i128,
        &500i128,
    );

    let agreement_id =
        payroll.create_escrow_agreement(&employer, &employer, &token_addr, &1000, &86400, &1);
    payroll.raise_dispute(&employer, &agreement_id);

    // Propose a LargePayment op (wrong kind for dispute resolution)
    let op_id = ms.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token_addr.clone(), employer.clone(), 600i128),
    );
    ms.approve_operation(&signers.get(1).unwrap(), &op_id);
    assert_eq!(
        ms.get_operation(&op_id).unwrap().status,
        OperationStatus::Executed
    );

    // Should fail — op kind is LargePayment, not DisputeResolution
    let result = payroll.try_resolve_dispute_multisig(
        &arbiter,
        &agreement_id,
        &600i128,
        &400i128,
        &op_id,
    );
    assert_eq!(result, Err(Ok(PayrollError::MultisigApprovalRequired)));
}
