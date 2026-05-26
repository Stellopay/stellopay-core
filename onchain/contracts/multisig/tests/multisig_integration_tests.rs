//! Integration tests for multisig-gated LargePayment and DisputeResolution flows.
//!
//! These tests verify:
//! - M-of-N approval is required before high-value payroll claims proceed.
//! - M-of-N approval is required before dispute resolutions above the threshold.
//! - Operations below the threshold proceed without multisig.
//! - Insufficient approvals block execution.
//! - Configurable per-kind thresholds override the global threshold.
//! - `is_operation_approved` returns the correct state at each stage.

#![cfg(test)]

use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};

use multisig::{
    MultisigContract, MultisigContractClient, OperationKind, OperationStatus, OperationThresholds,
};

// ── helpers ──────────────────────────────────────────────────────────────────

fn env() -> Env {
    let e = Env::default();
    e.mock_all_auths();
    e
}

fn deploy_multisig(e: &Env) -> (Address, MultisigContractClient<'static>) {
    #[allow(deprecated)]
    let id = e.register_contract(None, MultisigContract);
    let client = MultisigContractClient::new(e, &id);
    (id, client)
}

fn mint_token<'a>(e: &Env, admin: &Address, recipient: &Address, amount: i128) -> TokenClient<'a> {
    let addr = e.register_stellar_asset_contract(admin.clone());
    let asset_client = StellarAssetClient::new(e, &addr);
    asset_client.mint(recipient, &amount);
    TokenClient::new(e, &addr)
}

/// Sets up a 2-of-3 multisig and returns (contract_id, client, owner, [s1,s2,s3]).
fn setup_2of3(
    e: &Env,
) -> (Address, MultisigContractClient<'static>, Address, Vec<Address>) {
    let (id, client) = deploy_multisig(e);
    let owner = Address::generate(e);
    let mut signers = Vec::new(e);
    for _ in 0..3 {
        signers.push_back(Address::generate(e));
    }
    client.initialize(&owner, &signers, &2u32, &None);
    (id, client, owner, signers)
}

// ── OperationThresholds ───────────────────────────────────────────────────────

/// Owner can set and retrieve per-kind thresholds.
#[test]
fn set_and_get_operation_thresholds() {
    let e = env();
    let (_id, client, owner, _signers) = setup_2of3(&e);

    assert!(client.get_operation_thresholds().is_none());

    let thresholds = OperationThresholds {
        large_payment: 3,
        dispute_resolution: 2,
    };
    client.set_operation_thresholds(&owner, &thresholds);

    let stored = client.get_operation_thresholds().unwrap();
    assert_eq!(stored.large_payment, 3);
    assert_eq!(stored.dispute_resolution, 2);
}

/// Non-owner cannot set thresholds.
#[test]
fn set_thresholds_non_owner_rejected() {
    let e = env();
    let (_id, client, _owner, signers) = setup_2of3(&e);

    let thresholds = OperationThresholds {
        large_payment: 2,
        dispute_resolution: 2,
    };
    let result = client.try_set_operation_thresholds(&signers.get(0).unwrap(), &thresholds);
    assert!(result.is_err());
}

/// Threshold values must be within [1, signer_count].
#[test]
fn set_thresholds_out_of_range_rejected() {
    let e = env();
    let (_id, client, owner, _signers) = setup_2of3(&e);

    // 0 is invalid
    let result = client.try_set_operation_thresholds(
        &owner,
        &OperationThresholds { large_payment: 0, dispute_resolution: 2 },
    );
    assert!(result.is_err());

    // > signer_count (3) is invalid
    let result = client.try_set_operation_thresholds(
        &owner,
        &OperationThresholds { large_payment: 4, dispute_resolution: 2 },
    );
    assert!(result.is_err());
}

// ── is_operation_approved ─────────────────────────────────────────────────────

/// Returns false for unknown operation IDs.
#[test]
fn is_approved_unknown_operation() {
    let e = env();
    let (_id, client, _owner, _signers) = setup_2of3(&e);
    assert!(!client.is_operation_approved(&999u128));
}

/// Returns false while pending, true after execution.
#[test]
fn is_approved_reflects_execution_state() {
    let e = env();
    let (multisig_id, client, _owner, signers) = setup_2of3(&e);

    let admin = Address::generate(&e);
    let token = mint_token(&e, &admin, &multisig_id, 1_000);
    let recipient = Address::generate(&e);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 100),
    );

    // Pending — not yet approved
    assert!(!client.is_operation_approved(&op_id));

    // Second signer reaches threshold → auto-executes
    client.approve_operation(&signers.get(1).unwrap(), &op_id);
    assert!(client.is_operation_approved(&op_id));
}

/// Cancelled operations are not considered approved.
#[test]
fn is_approved_cancelled_operation() {
    let e = env();
    let (_id, client, _owner, signers) = setup_2of3(&e);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(Address::generate(&e), 1u128, 500, 0),
    );

    client.cancel_operation(&signers.get(0).unwrap(), &op_id);
    assert!(!client.is_operation_approved(&op_id));
}

// ── LargePayment M-of-N flows ─────────────────────────────────────────────────

/// 2-of-3: payment executes only after the second approval.
#[test]
fn large_payment_requires_2of3_approvals() {
    let e = env();
    let (multisig_id, client, _owner, signers) = setup_2of3(&e);

    let admin = Address::generate(&e);
    let token = mint_token(&e, &admin, &multisig_id, 1_000);
    let recipient = Address::generate(&e);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 400),
    );

    // One approval — still pending
    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Pending);
    assert_eq!(token.balance(&recipient), 0);

    // Second approval — threshold met, transfer executes
    client.approve_operation(&signers.get(1).unwrap(), &op_id);
    assert_eq!(client.get_operation(&op_id).unwrap().status, OperationStatus::Executed);
    assert_eq!(token.balance(&recipient), 400);
}

/// 3-of-3: payment requires all three signers.
#[test]
fn large_payment_requires_3of3_approvals() {
    let e = env();
    let (multisig_id, client, owner, signers) = setup_2of3(&e);

    // Raise threshold to 3-of-3 for LargePayment
    client.set_operation_thresholds(
        &owner,
        &OperationThresholds { large_payment: 3, dispute_resolution: 2 },
    );

    let admin = Address::generate(&e);
    let token = mint_token(&e, &admin, &multisig_id, 1_000);
    let recipient = Address::generate(&e);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 300),
    );

    // Two approvals — not enough
    client.approve_operation(&signers.get(1).unwrap(), &op_id);
    assert_eq!(client.get_operation(&op_id).unwrap().status, OperationStatus::Pending);
    assert_eq!(token.balance(&recipient), 0);

    // Third approval — executes
    client.approve_operation(&signers.get(2).unwrap(), &op_id);
    assert_eq!(client.get_operation(&op_id).unwrap().status, OperationStatus::Executed);
    assert_eq!(token.balance(&recipient), 300);
}

/// Duplicate approvals from the same signer are idempotent.
#[test]
fn large_payment_duplicate_approval_ignored() {
    let e = env();
    let (multisig_id, client, _owner, signers) = setup_2of3(&e);

    let admin = Address::generate(&e);
    let token = mint_token(&e, &admin, &multisig_id, 1_000);
    let recipient = Address::generate(&e);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 200),
    );

    // Same signer approves twice — should not double-count
    client.approve_operation(&signers.get(0).unwrap(), &op_id);
    client.approve_operation(&signers.get(0).unwrap(), &op_id);

    // Still only 1 approval (proposer), threshold not met
    assert_eq!(client.get_operation(&op_id).unwrap().status, OperationStatus::Pending);
    assert_eq!(client.get_approvals(&op_id).len(), 1);
}

/// Non-signer cannot approve a LargePayment operation.
#[test]
fn large_payment_non_signer_cannot_approve() {
    let e = env();
    let (multisig_id, client, _owner, signers) = setup_2of3(&e);

    let admin = Address::generate(&e);
    let token = mint_token(&e, &admin, &multisig_id, 1_000);
    let outsider = Address::generate(&e);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), outsider.clone(), 100),
    );

    let result = client.try_approve_operation(&outsider, &op_id);
    assert!(result.is_err());
}

/// Approving an already-executed operation is a no-op (status stays Executed).
#[test]
fn approve_already_executed_operation_rejected() {
    let e = env();
    let (multisig_id, client, _owner, signers) = setup_2of3(&e);

    let admin = Address::generate(&e);
    let token = mint_token(&e, &admin, &multisig_id, 1_000);
    let recipient = Address::generate(&e);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 50),
    );
    client.approve_operation(&signers.get(1).unwrap(), &op_id);
    assert_eq!(client.get_operation(&op_id).unwrap().status, OperationStatus::Executed);

    // Third signer tries to approve after execution — should fail
    let result = client.try_approve_operation(&signers.get(2).unwrap(), &op_id);
    assert!(result.is_err());
}

// ── DisputeResolution M-of-N flows ────────────────────────────────────────────

/// DisputeResolution reaches Executed after threshold approvals.
#[test]
fn dispute_resolution_requires_2of3_approvals() {
    let e = env();
    let (_id, client, _owner, signers) = setup_2of3(&e);

    let payroll_contract = Address::generate(&e);
    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(payroll_contract.clone(), 42u128, 800, 200),
    );

    // One approval — pending
    assert_eq!(client.get_operation(&op_id).unwrap().status, OperationStatus::Pending);
    assert!(!client.is_operation_approved(&op_id));

    // Second approval — executed (no-op on-chain; stello_pay reads the status)
    client.approve_operation(&signers.get(1).unwrap(), &op_id);
    assert_eq!(client.get_operation(&op_id).unwrap().status, OperationStatus::Executed);
    assert!(client.is_operation_approved(&op_id));
}

/// DisputeResolution with a 3-of-3 override requires all signers.
#[test]
fn dispute_resolution_3of3_threshold_override() {
    let e = env();
    let (_id, client, owner, signers) = setup_2of3(&e);

    client.set_operation_thresholds(
        &owner,
        &OperationThresholds { large_payment: 2, dispute_resolution: 3 },
    );

    let payroll_contract = Address::generate(&e);
    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(payroll_contract.clone(), 7u128, 500, 500),
    );

    client.approve_operation(&signers.get(1).unwrap(), &op_id);
    // Still pending — need 3
    assert_eq!(client.get_operation(&op_id).unwrap().status, OperationStatus::Pending);

    client.approve_operation(&signers.get(2).unwrap(), &op_id);
    assert_eq!(client.get_operation(&op_id).unwrap().status, OperationStatus::Executed);
}

/// Creator can cancel a pending DisputeResolution before threshold is met.
#[test]
fn dispute_resolution_can_be_cancelled_before_threshold() {
    let e = env();
    let (_id, client, _owner, signers) = setup_2of3(&e);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(Address::generate(&e), 1u128, 100, 0),
    );

    client.cancel_operation(&signers.get(0).unwrap(), &op_id);
    assert_eq!(client.get_operation(&op_id).unwrap().status, OperationStatus::Cancelled);
    assert!(!client.is_operation_approved(&op_id));
}

// ── Emergency guardian bypass ─────────────────────────────────────────────────

/// Guardian can execute a DisputeResolution without reaching threshold.
#[test]
fn guardian_can_bypass_threshold_for_dispute_resolution() {
    let e = env();
    let (id, client) = deploy_multisig(&e);
    let owner = Address::generate(&e);
    let guardian = Address::generate(&e);
    let mut signers = Vec::new(&e);
    for _ in 0..3 {
        signers.push_back(Address::generate(&e));
    }
    client.initialize(&owner, &signers, &3u32, &Some(guardian.clone()));

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::DisputeResolution(Address::generate(&e), 99u128, 1000, 0),
    );

    // Only 1 of 3 approvals — normally not enough
    assert_eq!(client.get_operation(&op_id).unwrap().status, OperationStatus::Pending);

    client.emergency_execute(&guardian, &op_id);
    assert_eq!(client.get_operation(&op_id).unwrap().status, OperationStatus::Executed);
    assert!(client.is_operation_approved(&op_id));

    let _ = id;
}

// ── Threshold interaction: global vs per-kind ─────────────────────────────────

/// When per-kind threshold equals global threshold, behaviour is unchanged.
#[test]
fn per_kind_threshold_equal_to_global_no_change() {
    let e = env();
    let (multisig_id, client, owner, signers) = setup_2of3(&e);

    // Set per-kind thresholds equal to global (2)
    client.set_operation_thresholds(
        &owner,
        &OperationThresholds { large_payment: 2, dispute_resolution: 2 },
    );

    let admin = Address::generate(&e);
    let token = mint_token(&e, &admin, &multisig_id, 500);
    let recipient = Address::generate(&e);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 100),
    );

    client.approve_operation(&signers.get(1).unwrap(), &op_id);
    assert_eq!(client.get_operation(&op_id).unwrap().status, OperationStatus::Executed);
    assert_eq!(token.balance(&recipient), 100);
}

/// Per-kind threshold lower than global is clamped to global.
#[test]
fn per_kind_threshold_lower_than_global_clamped() {
    let e = env();
    let (multisig_id, client, owner, signers) = setup_2of3(&e);

    // Attempt to set per-kind threshold below global (2) — effective threshold stays 2
    client.set_operation_thresholds(
        &owner,
        &OperationThresholds { large_payment: 1, dispute_resolution: 1 },
    );

    let admin = Address::generate(&e);
    let token = mint_token(&e, &admin, &multisig_id, 500);
    let recipient = Address::generate(&e);

    let op_id = client.propose_operation(
        &signers.get(0).unwrap(),
        &OperationKind::LargePayment(token.address.clone(), recipient.clone(), 50),
    );

    // Only 1 approval (proposer) — effective threshold is max(2,1)=2, so still pending
    assert_eq!(client.get_operation(&op_id).unwrap().status, OperationStatus::Pending);

    client.approve_operation(&signers.get(1).unwrap(), &op_id);
    assert_eq!(client.get_operation(&op_id).unwrap().status, OperationStatus::Executed);
}
